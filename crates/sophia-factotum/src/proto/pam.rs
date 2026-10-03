//! `pam`: verifies the agent owner's password through PAM, for Session's
//! unlock. New to Sophia; it has no 9front original.
//!
//! The conversation is the one 9front's `auth_userpasswd` drives against
//! `dp9ik role=login`: `start proto=pam role=login`, `write <user>`, `write
//! <password>`, then `authinfo`. A verdict belongs to one attempt: any failure
//! breaks the conversation, so another attempt needs another `start`.
//!
//! PAM itself runs in a separately executed helper, so no PAM module shares
//! the agent's address space or can block it.

use super::{ChannelClass, Env, Job, Phase, ProtocolState, Status};
use crate::conversation::{AuthInfo, Conversation};
use crate::secret::SecretBytes;

const NEED_USER: u8 = 0;
const NEED_PASS: u8 = 1;

/// Named after dp9ik's login phases, so `phase` replies read the same.
pub(super) const PHASE_NAMES: &[&str] = &["SNeedUser", "SNeedPass"];

/// Linux-PAM's `PAM_MAX_RESP_SIZE`.
const MAX_SECRET: usize = 512;
const MAX_USER: usize = 256;

#[derive(Debug, Default)]
pub struct PamState {
    user: String,
}

/// One attempt for the helper.
#[derive(Debug)]
pub struct PamRequest {
    pub service: String,
    pub user: String,
    pub secret: SecretBytes,
}

/// How an attempt ended. Nothing about PAM's own messages is kept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PamVerdict {
    Accepted,
    Rejected,
    Unavailable,
    ConversationRefused,
    TimedOut,
    HelperFailed,
}

pub(super) fn init(cx: &mut Conversation, env: &mut Env<'_>) -> Status {
    match cx.attrs.value("role") {
        None => return Status::Failure("role not specified".into()),
        Some("login") => {}
        Some(role) => return Status::Failure(format!("unknown role {}", crate::attr::quote(role))),
    }
    if env.channel != ChannelClass::Session {
        return Status::Failure("pam login not permitted".into());
    }
    if cx
        .attrs
        .value("service")
        .is_some_and(|service| service != env.settings.pam_service)
    {
        return Status::Failure("unknown pam service".into());
    }
    cx.state = Some(ProtocolState::Pam(PamState::default()));
    cx.phase = Phase::Proto(NEED_USER);
    Status::Ok
}

pub(super) fn write(cx: &mut Conversation, env: &mut Env<'_>, data: &[u8]) -> Status {
    match cx.phase {
        Phase::Proto(NEED_USER) => {
            let Ok(user) = std::str::from_utf8(data) else {
                return broken(cx, "bad user name");
            };
            if user.is_empty() || user.len() > MAX_USER || user.contains('\0') {
                return broken(cx, "bad user name");
            }
            if user != env.settings.owner {
                return broken(cx, "user not permitted");
            }
            if let Some(ProtocolState::Pam(state)) = cx.state.as_mut() {
                state.user = user.to_owned();
            }
            cx.phase = Phase::Proto(NEED_PASS);
            Status::Ok
        }
        Phase::Proto(NEED_PASS) => {
            if data.len() > MAX_SECRET {
                return broken(cx, "password too long");
            }
            if data.contains(&0) {
                return broken(cx, "bad password");
            }
            let Some(ProtocolState::Pam(state)) = cx.state.as_ref() else {
                return cx.phase_error("write");
            };
            Status::Wait(Job::Pam(PamRequest {
                service: env.settings.pam_service.clone(),
                user: state.user.clone(),
                secret: SecretBytes::from_slice(data),
            }))
        }
        _ => cx.phase_error("write"),
    }
}

pub(super) fn resume(cx: &mut Conversation, _env: &mut Env<'_>, verdict: PamVerdict) -> Status {
    let reason = match verdict {
        PamVerdict::Accepted => {
            let user = match cx.state.as_ref() {
                Some(ProtocolState::Pam(state)) => state.user.clone(),
                _ => return broken(cx, "pam helper failed"),
            };
            cx.ai = Some(AuthInfo {
                cuid: user.clone(),
                suid: user,
                cap: String::new(),
                secret: SecretBytes::default(),
            });
            cx.phase = Phase::Established;
            return Status::Ok;
        }
        PamVerdict::Rejected => "authentication failed",
        PamVerdict::Unavailable => "pam unavailable",
        PamVerdict::ConversationRefused => "pam conversation refused",
        PamVerdict::TimedOut => "pam timeout",
        PamVerdict::HelperFailed => "pam helper failed",
    };
    broken(cx, reason)
}

/// Fails the attempt and breaks the conversation, so later reads and
/// writes answer the same error and no second verdict can follow.
fn broken(cx: &mut Conversation, reason: &str) -> Status {
    cx.phase = Phase::Broken;
    Status::Failure(reason.into())
}
