//! One open of the `rpc` file: a conversation and its pending RPC.
//!
//! Ported from 9front `sys/src/cmd/auth/factotum/rpc.c` (`rpcwrite`,
//! `rpcread`, `retrpc`, `rdwrcheck`) and `util.c` (`convAI2M`, `failure`,
//! `phaseerror`); MIT, Copyright (c) 2021 Plan 9 Foundation and 9front
//! authors. Reply bytes are 9front's exactly; the file-level errors become
//! [`RpcFileError`], which the export maps to errno values.

use crate::attr::{AttrList, quote};
use crate::proto::{Env, Job, JobOutcome, Phase, ProtocolId, ProtocolState, Status};
use crate::secret::SecretBytes;

/// `Maxrpc`: a write must be shorter than this.
pub const MAX_RPC: usize = 4096;
/// `rpcread` refuses reads smaller than this.
pub const MIN_RPC_READ: usize = 64;
/// `ERRMAX - 1`: the longest error text kept.
const MAX_ERROR: usize = 127;
/// `3 * Maxname - 1`: the longest needkey template kept.
const MAX_KEYINFO: usize = 383;

/// The file-level refusals of `rpc`, before any reply is formed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RpcFileError {
    /// `rpc too large`
    TooLarge,
    /// `rpc already pending; read to clear`
    AlreadyPending,
    /// `rpc read too small`
    ReadTooSmall,
    /// `no rpc pending`
    NoRpcPending,
    /// `confirm is closed`: the RPC stays pending.
    ConfirmClosed,
}

/// The result of a read of `rpc`.
#[derive(Debug)]
pub enum RpcRead {
    /// The reply bytes; the RPC is no longer pending.
    Reply(Vec<u8>),
    /// The RPC waits on this job. Reads answer [`RpcRead::Waiting`] until
    /// [`Conversation::finish_job`] supplies its outcome.
    Started(Job),
    Waiting,
    Refused(RpcFileError),
}

/// What `authinfo` returns once a conversation is established.
#[derive(Clone, Debug, Default)]
pub struct AuthInfo {
    pub cuid: String,
    pub suid: String,
    /// Always empty: Linux has no `/dev/caphash`.
    pub cap: String,
    pub secret: SecretBytes,
}

impl AuthInfo {
    /// `convAI2M`: each field a little-endian u16 length and its bytes. Like
    /// 9front, every field needs one byte more than it uses, so `None` unless
    /// `room` exceeds the encoding.
    pub fn encode(&self, room: usize) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for field in [
            self.cuid.as_bytes(),
            self.suid.as_bytes(),
            self.cap.as_bytes(),
            self.secret.as_slice(),
        ] {
            let length = u16::try_from(field.len()).ok()?;
            if out.len() + field.len() + 2 >= room {
                zeroize::Zeroize::zeroize(&mut out);
                return None;
            }
            out.extend_from_slice(&length.to_le_bytes());
            out.extend_from_slice(field);
        }
        Some(out)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Verb {
    AuthInfo,
    Read,
    Start,
    Write,
    Attr,
    Unknown,
}

/// A written, unanswered RPC. Its bytes may carry a password, so they are
/// zeroed when it is answered.
struct PendingRpc {
    verb: Verb,
    argument: SecretBytes,
}

/// 9front's `Fsstate` for one `rpc` open.
pub struct Conversation {
    pub attrs: AttrList,
    pub phase: Phase,
    pub proto: Option<ProtocolId>,
    pub state: Option<ProtocolState>,
    /// The `start` sequence number, printed in log lines.
    pub seqnum: u32,
    /// The last failure's text, answered while the conversation is broken.
    pub err: String,
    /// The template a `needkey` reply carries.
    pub keyinfo: String,
    pub ai: Option<AuthInfo>,
    pending: Option<PendingRpc>,
    /// A job is out for the pending RPC; its outcome, once supplied.
    waiting: bool,
    outcome: Option<JobOutcome>,
}

impl core::fmt::Debug for Conversation {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Conversation")
            .field("proto", &self.proto)
            .field("phase", &self.phase)
            .field("seqnum", &self.seqnum)
            .finish_non_exhaustive()
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            attrs: AttrList::new(),
            phase: Phase::NotStarted,
            proto: None,
            state: None,
            seqnum: 0,
            err: "factotum/fs.c no error".into(),
            keyinfo: String::new(),
            ai: None,
            pending: None,
            waiting: false,
            outcome: None,
        }
    }

    /// `phaseerror`: the `phase` reply for an operation the current phase
    /// does not accept.
    pub fn phase_error(&self, operation: &str) -> Status {
        Status::PhaseError(format!(
            "protocol phase error: {operation} in state {}",
            self.phase.name(self.phase, self.proto)
        ))
    }

    /// `rpcwrite`: stores one RPC. The verb is the text before the first
    /// space; the argument is everything after it, binary-safe.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, RpcFileError> {
        if data.len() >= MAX_RPC {
            return Err(RpcFileError::TooLarge);
        }
        if self.pending.is_some() {
            return Err(RpcFileError::AlreadyPending);
        }
        let (verb, argument) = match data.iter().position(|byte| *byte == b' ') {
            Some(space) => (&data[..space], &data[space + 1..]),
            None => (data, &[][..]),
        };
        let verb = match verb {
            b"authinfo" => Verb::AuthInfo,
            b"read" => Verb::Read,
            b"start" => Verb::Start,
            b"write" => Verb::Write,
            b"attr" => Verb::Attr,
            _ => Verb::Unknown,
        };
        self.pending = Some(PendingRpc {
            verb,
            argument: SecretBytes::from_slice(argument),
        });
        Ok(data.len())
    }

    /// Supplies the outcome of the job a read started. The next read
    /// finishes the RPC with it.
    pub fn finish_job(&mut self, outcome: JobOutcome) {
        if self.waiting {
            self.outcome = Some(outcome);
        }
    }

    /// `rpcread`: answers the pending RPC within `count` bytes.
    pub fn read(&mut self, env: &mut Env<'_>, count: usize) -> RpcRead {
        if count < MIN_RPC_READ {
            return RpcRead::Refused(RpcFileError::ReadTooSmall);
        }
        let Some(verb) = self.pending.as_ref().map(|pending| pending.verb) else {
            return RpcRead::Refused(RpcFileError::NoRpcPending);
        };
        if self.waiting {
            let Some(outcome) = self.outcome.take() else {
                return RpcRead::Waiting;
            };
            self.waiting = false;
            let status = match self.proto {
                Some(proto) => proto.resume(self, env, outcome),
                None => Status::Failure("no current protocol".into()),
            };
            return self.answer_status(env, status, count);
        }
        match verb {
            Verb::Unknown => self.answer(b"error unknown verb".to_vec(), count),
            Verb::Start => self.start(env, count),
            Verb::Read => self.read_step(env, count),
            Verb::Write => self.write_step(env, count),
            Verb::AuthInfo => self.authinfo(count),
            Verb::Attr => {
                let reply = format!("ok {}", self.attrs).into_bytes();
                self.answer(reply, count)
            }
        }
    }

    /// Ends the conversation's protocol, as a second `start` or a clunk does.
    pub fn close(&mut self) {
        self.state = None;
        self.proto = None;
        self.attrs = AttrList::new();
        self.phase = Phase::NotStarted;
        self.ai = None;
        self.waiting = false;
        self.outcome = None;
    }

    fn start(&mut self, env: &mut Env<'_>, count: usize) -> RpcRead {
        if self.phase != Phase::NotStarted {
            env.log.append(format!(
                "{}: implicit close due to second start; old attr '{}'",
                self.seqnum,
                self.attrs.masked()
            ));
            self.close();
        }
        let argument = self.argument_text();
        let attrs = AttrList::parse(&argument);
        let Some(name) = attrs.value("proto") else {
            return self.answer(b"error did not specify proto".to_vec(), count);
        };
        let Some(proto) = ProtocolId::from_name(name) else {
            let reply = format!("error unknown protocol {}", quote(name)).into_bytes();
            return self.answer(reply, count);
        };
        self.attrs = attrs;
        self.proto = Some(proto);
        self.seqnum = env.next_seqnum();
        let status = proto.init(self, env);
        if !matches!(status, Status::Ok) {
            self.attrs = AttrList::new();
            self.state = None;
            self.phase = Phase::NotStarted;
        }
        self.answer_status(env, status, count)
    }

    /// `rdwrcheck`, in 9front's order. `Some` is the reply when the step
    /// cannot run.
    fn check_step(&self) -> Option<Vec<u8>> {
        // 9front tests the protocol state first, so a read before any start
        // is "no current protocol", not "not started".
        if self.state.is_none() || self.proto.is_none() {
            return Some(b"error no current protocol".to_vec());
        }
        match self.phase {
            // No `error` prefix: 9front's own wording.
            Phase::NotStarted => Some(b"protocol not started".to_vec()),
            Phase::Broken => Some(format!("error {}", self.err).into_bytes()),
            Phase::Established if self.ai.is_some() => Some(b"done haveai".to_vec()),
            Phase::Established => Some(b"done".to_vec()),
            Phase::Proto(_) => None,
        }
    }

    fn read_step(&mut self, env: &mut Env<'_>, count: usize) -> RpcRead {
        if !self.argument_is_empty() {
            return self.answer(b"error read needs no parameters".to_vec(), count);
        }
        if let Some(reply) = self.check_step() {
            return self.answer(reply, count);
        }
        let Some(proto) = self.proto else {
            return self.answer(b"error no current protocol".to_vec(), count);
        };
        let (status, data) = proto.read(self, env, count - 3);
        match status {
            Status::Ok => {
                let mut reply = b"ok".to_vec();
                if !data.is_empty() {
                    reply.push(b' ');
                    reply.extend_from_slice(&data);
                }
                let mut data = data;
                zeroize::Zeroize::zeroize(&mut data);
                self.answer(reply, count)
            }
            status => self.answer_status(env, status, count),
        }
    }

    fn write_step(&mut self, env: &mut Env<'_>, count: usize) -> RpcRead {
        if let Some(reply) = self.check_step() {
            return self.answer(reply, count);
        }
        let Some(proto) = self.proto else {
            return self.answer(b"error no current protocol".to_vec(), count);
        };
        let argument = self
            .pending
            .as_ref()
            .map(|pending| pending.argument.clone())
            .unwrap_or_default();
        let status = proto.write(self, env, argument.as_slice());
        self.answer_status(env, status, count)
    }

    fn authinfo(&mut self, count: usize) -> RpcRead {
        if self.phase != Phase::Established {
            return self.answer(b"error authentication unfinished".to_vec(), count);
        }
        let Some(ai) = self.ai.as_ref() else {
            return self.answer(b"error no authinfo available".to_vec(), count);
        };
        match ai.encode(count - 3) {
            Some(encoded) => {
                let mut reply = b"ok ".to_vec();
                reply.extend_from_slice(&encoded);
                let mut encoded = encoded;
                zeroize::Zeroize::zeroize(&mut encoded);
                self.answer(reply, count)
            }
            None => self.answer(b"error read too small".to_vec(), count),
        }
    }

    /// `retrpc`: the reply for a protocol status.
    fn answer_status(&mut self, env: &mut Env<'_>, status: Status, count: usize) -> RpcRead {
        let reply = match status {
            Status::Ok => b"ok".to_vec(),
            Status::Failure(text) => {
                self.err = truncate(&text, MAX_ERROR);
                env.log
                    .append(format!("{}: failure {}", self.seqnum, self.err));
                format!("error {}", self.err).into_bytes()
            }
            Status::Error(text) => format!("error {}", truncate(&text, MAX_ERROR)).into_bytes(),
            Status::Needkey => {
                // No trusted prompt exists yet, so the needkey file is never
                // open and the template is answered directly.
                self.keyinfo = truncate(&self.keyinfo, MAX_KEYINFO);
                format!("needkey {}", self.keyinfo).into_bytes()
            }
            Status::Toosmall(wanted) => format!("toosmall {wanted}").into_bytes(),
            Status::PhaseError(text) => {
                format!("phase {}", truncate(&text, MAX_ERROR)).into_bytes()
            }
            // The confirm file is never open without a trusted prompt.
            Status::Confirm => return RpcRead::Refused(RpcFileError::ConfirmClosed),
            Status::Wait(job) => {
                // The job owns its copy of anything secret now; the RPC keeps
                // only its verb while it waits.
                if let Some(pending) = self.pending.as_mut() {
                    pending.argument = SecretBytes::default();
                }
                self.waiting = true;
                self.outcome = None;
                return RpcRead::Started(job);
            }
        };
        self.answer(reply, count)
    }

    /// `retstring`: the reply cut to the read count; the RPC is answered and
    /// its bytes zeroed.
    fn answer(&mut self, mut reply: Vec<u8>, count: usize) -> RpcRead {
        reply.truncate(count);
        self.pending = None;
        RpcRead::Reply(reply)
    }

    fn argument_text(&self) -> String {
        self.pending.as_ref().map_or_else(String::new, |pending| {
            String::from_utf8_lossy(pending.argument.as_slice()).into_owned()
        })
    }

    fn argument_is_empty(&self) -> bool {
        self.pending
            .as_ref()
            .is_none_or(|pending| pending.argument.is_empty())
    }
}

/// Cuts at most `limit` bytes on a UTF-8 boundary.
fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
