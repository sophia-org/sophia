//! The protocol table and the interface a protocol module implements.
//!
//! 9front's `Proto` (`sys/src/cmd/auth/factotum/dat.h:119-129`) becomes an
//! enum over the protocols this agent carries, dispatched by `match`. The
//! table order is 9front's `prototab` order for those that remain, then the
//! Linux `pam` login.

mod pam;
mod pass;

use crate::attr::AttrList;
use crate::conversation::Conversation;
use crate::keyring::{Key, KeyLookup, KeyQuery, Keyring};
use crate::logbuf::LogBuf;

pub use pam::{PamRequest, PamState, PamVerdict};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolId {
    Pass,
    Pam,
}

/// `prototab`, as the `proto` file lists it.
pub const PROTOCOLS: [ProtocolId; 2] = [ProtocolId::Pass, ProtocolId::Pam];

impl ProtocolId {
    pub fn from_name(name: &str) -> Option<Self> {
        PROTOCOLS.into_iter().find(|proto| proto.name() == name)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Pam => "pam",
        }
    }

    /// `keyprompt`: what a key of this protocol must carry.
    pub const fn key_prompt(self) -> &'static str {
        match self {
            Self::Pass => "user? !password?",
            Self::Pam => "",
        }
    }

    /// The names a `phase` reply prints. `pass` defines one but 9front never
    /// installs it, so its phases print as numbers.
    const fn phase_names(self) -> &'static [&'static str] {
        match self {
            Self::Pass => &[],
            Self::Pam => pam::PHASE_NAMES,
        }
    }

    /// `addkey`; `None` means the protocol takes no keys.
    pub fn add_key(self, ring: &mut Keyring, key: Key) -> Option<Result<(), String>> {
        match self {
            Self::Pass => {
                ring.replace(key, false);
                Some(Ok(()))
            }
            Self::Pam => None,
        }
    }

    pub(crate) fn init(self, cx: &mut Conversation, env: &mut Env<'_>) -> Status {
        match self {
            Self::Pass => pass::init(cx, env),
            Self::Pam => pam::init(cx, env),
        }
    }

    pub(crate) fn read(
        self,
        cx: &mut Conversation,
        env: &mut Env<'_>,
        max: usize,
    ) -> (Status, Vec<u8>) {
        let _ = env;
        match self {
            Self::Pass => pass::read(cx, max),
            Self::Pam => (cx.phase_error("read"), Vec::new()),
        }
    }

    pub(crate) fn write(self, cx: &mut Conversation, env: &mut Env<'_>, data: &[u8]) -> Status {
        match self {
            Self::Pass => cx.phase_error("write"),
            Self::Pam => pam::write(cx, env, data),
        }
    }

    pub(crate) fn resume(
        self,
        cx: &mut Conversation,
        env: &mut Env<'_>,
        outcome: JobOutcome,
    ) -> Status {
        match (self, outcome) {
            (Self::Pam, JobOutcome::Pam(verdict)) => pam::resume(cx, env, verdict),
            (Self::Pass, _) => Status::Failure("internal error: no job".into()),
        }
    }
}

/// A protocol's private conversation state (9front's `fss->ps`).
#[derive(Debug)]
pub enum ProtocolState {
    Pass { key: std::sync::Arc<Key> },
    Pam(pam::PamState),
}

/// The conversation phase. Protocol phases count from zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    NotStarted,
    Broken,
    Established,
    Proto(u8),
}

impl Phase {
    /// `phasename`: `Broken` when the conversation is broken whatever phase
    /// is asked about, the common names, a protocol's table entry, or the
    /// number.
    pub fn name(self, current: Phase, proto: Option<ProtocolId>) -> String {
        if current == Phase::Broken {
            return "Broken".into();
        }
        match self {
            Phase::NotStarted => "Notstarted".into(),
            Phase::Established => "Established".into(),
            Phase::Broken => "Broken".into(),
            Phase::Proto(index) => proto
                .and_then(|proto| proto.phase_names().get(usize::from(index)).copied())
                .map_or_else(|| index.to_string(), str::to_owned),
        }
    }
}

/// What a protocol step returns (9front's `Rpc*` values). The error text of
/// `Failure` becomes the conversation's error; `Error` does not.
#[derive(Debug)]
pub enum Status {
    Ok,
    Failure(String),
    Error(String),
    Needkey,
    Toosmall(u32),
    /// The text after `phase `, already formatted.
    PhaseError(String),
    Confirm,
    /// Slow work the agent must run before the step can finish.
    Wait(Job),
}

/// Slow work a protocol hands to the agent's workers.
#[derive(Debug)]
pub enum Job {
    Pam(PamRequest),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobOutcome {
    Pam(PamVerdict),
}

/// Which connection a conversation arrived on. `pam` serves only Session's
/// private channel, so no other same-UID process can test passwords with it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChannelClass {
    Session,
    User,
}

/// Agent configuration a protocol may read.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The agent's owner: the only user `pam` verifies.
    pub owner: String,
    /// The PAM service `pam` runs, `sophia-lock` by default.
    pub pam_service: String,
}

/// What a protocol may use while it runs: the key ring, the log, the
/// agent's settings and the class of the conversation's connection.
pub struct Env<'a> {
    pub keyring: &'a mut Keyring,
    pub log: &'a mut LogBuf,
    pub settings: &'a Settings,
    pub channel: ChannelClass,
    /// The agent-wide `start` counter (9front's `seqnum`).
    pub sequence: &'a mut u32,
}

impl Env<'_> {
    /// `++seqnum`: numbers each `start` for the log.
    pub fn next_seqnum(&mut self) -> u32 {
        *self.sequence = self.sequence.wrapping_add(1);
        *self.sequence
    }

    /// `findkey` with the conversation's start attributes and `extra`.
    pub fn find_key(
        &mut self,
        cx: &mut Conversation,
        extra: &str,
    ) -> Result<std::sync::Arc<Key>, Status> {
        let lookup = self.keyring.find(
            &KeyQuery {
                attrs: &cx.attrs,
                extra: AttrList::parse(extra),
                skip: 0,
                no_confirm: false,
                use_disabled: false,
            },
            |name| ProtocolId::from_name(name).is_some(),
        );
        match lookup {
            KeyLookup::Found(key) => Ok(key),
            KeyLookup::Confirm(_) => Err(Status::Confirm),
            KeyLookup::Needkey(template) => {
                cx.keyinfo = template;
                Err(Status::Needkey)
            }
            KeyLookup::Failure(text) => {
                self.log.append(format!(
                    "{}: no key matches {} {}",
                    cx.seqnum,
                    cx.attrs.masked(),
                    AttrList::parse(extra).masked()
                ));
                Err(Status::Failure(text))
            }
        }
    }
}
