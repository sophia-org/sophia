//! Session's side of the agent: the `pam` login, spoken over 9P.
//!
//! The exchange is the one 9front's `auth_userpasswd` drives: `start`, the
//! user, the password, then `authinfo`. Each attempt opens its own `rpc`, so
//! its conversation, and the verdict it ends in, belong to that attempt
//! alone.

use sophia_9p::records::Errno;
use sophia_9p::records::Fid;
use sophia_9p_client::pipeline::{Pipeline, PipelineError, PipelineLimits, Reply};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// A read large enough for any reply of the login (`MIN_RPC_READ` is 64).
const REPLY_COUNT: u32 = 4096;
/// `O_RDWR`.
const READ_WRITE: u32 = 2;

#[derive(Debug)]
pub enum ClientError {
    Transport(PipelineError),
    /// The agent answered a 9P request with an error.
    Remote(Errno),
    /// The agent answered with something the login does not allow.
    Unexpected(&'static str),
}

impl From<PipelineError> for ClientError {
    fn from(error: PipelineError) -> Self {
        Self::Transport(error)
    }
}

/// How one login ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoginVerdict {
    Accepted,
    /// The agent's reason, such as `authentication failed`.
    Refused(String),
}

pub struct UnlockClient {
    pipeline: Pipeline,
    root: Fid,
}

impl UnlockClient {
    /// Negotiates the version and attaches to the `factotum` directory.
    pub fn over(stream: UnixStream, deadline: Duration) -> Result<Self, ClientError> {
        let limits = PipelineLimits {
            msize: 16_384,
            ..PipelineLimits::default()
        };
        let mut pipeline = Pipeline::over(stream, limits, deadline)?;
        let (tag, root) = pipeline.attach(b"", b"factotum")?;
        match pipeline.wait(tag, Instant::now() + deadline)? {
            Reply::Attach(_) => Ok(Self { pipeline, root }),
            Reply::Error(errno) => Err(ClientError::Remote(errno)),
            _ => Err(ClientError::Unexpected("attach")),
        }
    }

    /// One login. Only `ok` to the password, and an `authinfo` naming the
    /// same user, accept it; anything else refuses or fails.
    pub fn login(
        &mut self,
        service: &str,
        user: &str,
        secret: &[u8],
        deadline: Instant,
    ) -> Result<LoginVerdict, ClientError> {
        let (tag, rpc) = self.pipeline.walk(self.root, &[b"rpc"])?;
        match self.pipeline.wait(tag, deadline)? {
            Reply::Walk(qids) if qids.len() == 1 => {}
            Reply::Error(errno) => return Err(ClientError::Remote(errno)),
            _ => return Err(ClientError::Unexpected("walk")),
        }
        let verdict = self.converse(rpc, service, user, secret, deadline);
        // The fid goes whatever happened; closing it ends the conversation.
        if let Ok(tag) = self.pipeline.clunk(rpc) {
            let _ = self.pipeline.wait(tag, deadline);
        }
        verdict
    }

    fn converse(
        &mut self,
        rpc: Fid,
        service: &str,
        user: &str,
        secret: &[u8],
        deadline: Instant,
    ) -> Result<LoginVerdict, ClientError> {
        let tag = self.pipeline.lopen(rpc, READ_WRITE)?;
        match self.pipeline.wait(tag, deadline)? {
            Reply::Lopen { .. } => {}
            Reply::Error(errno) => return Err(ClientError::Remote(errno)),
            _ => return Err(ClientError::Unexpected("open")),
        }
        let start = format!(
            "start proto=pam role=login service={}",
            crate::attr::quote(service)
        );
        if let Some(refused) = self.step(rpc, start.as_bytes(), Bytes::Plain, deadline)? {
            return Ok(refused);
        }
        let request = [b"write ".as_slice(), user.as_bytes()].concat();
        if let Some(refused) = self.step(rpc, &request, Bytes::Plain, deadline)? {
            return Ok(refused);
        }
        // Sized once, so no reallocation leaves part of the secret behind.
        let mut request = zeroize::Zeroizing::new(Vec::with_capacity(6 + secret.len()));
        request.extend_from_slice(b"write ");
        request.extend_from_slice(secret);
        if let Some(refused) = self.step(rpc, &request, Bytes::Secret, deadline)? {
            return Ok(refused);
        }
        drop(request);
        let info = self.exchange(rpc, b"authinfo", Bytes::Plain, deadline)?;
        let Some(encoded) = info.strip_prefix(b"ok ") else {
            return Ok(LoginVerdict::Refused("no authinfo".into()));
        };
        match first_field(encoded) {
            Some(cuid) if cuid == user.as_bytes() => Ok(LoginVerdict::Accepted),
            _ => Ok(LoginVerdict::Refused("authinfo names another user".into())),
        }
    }

    /// Sends one request and reads its reply; `Some` is a refusal.
    fn step(
        &mut self,
        rpc: Fid,
        request: &[u8],
        bytes: Bytes,
        deadline: Instant,
    ) -> Result<Option<LoginVerdict>, ClientError> {
        let reply = self.exchange(rpc, request, bytes, deadline)?;
        if reply == b"ok" {
            return Ok(None);
        }
        let reason = reply.strip_prefix(b"error ").map_or_else(
            || String::from_utf8_lossy(&reply).into_owned(),
            |reason| String::from_utf8_lossy(reason).into_owned(),
        );
        Ok(Some(LoginVerdict::Refused(reason)))
    }

    fn exchange(
        &mut self,
        rpc: Fid,
        request: &[u8],
        bytes: Bytes,
        deadline: Instant,
    ) -> Result<Vec<u8>, ClientError> {
        let tag = match bytes {
            Bytes::Plain => self.pipeline.write(rpc, 0, request)?,
            // The pipeline zeroes every copy of the frame it makes.
            Bytes::Secret => self.pipeline.write_secret(rpc, 0, request)?,
        };
        match self.pipeline.wait(tag, deadline)? {
            Reply::Write(count) if usize::try_from(count).ok() == Some(request.len()) => {}
            Reply::Error(errno) => return Err(ClientError::Remote(errno)),
            _ => return Err(ClientError::Unexpected("write")),
        }
        let tag = self.pipeline.read(rpc, 0, REPLY_COUNT)?;
        match self.pipeline.wait(tag, deadline)? {
            Reply::Read(reply) => Ok(reply),
            Reply::Error(errno) => Err(ClientError::Remote(errno)),
            _ => Err(ClientError::Unexpected("read")),
        }
    }
}

#[derive(Clone, Copy)]
enum Bytes {
    Plain,
    Secret,
}

/// The first length-prefixed field of an encoded AuthInfo: `cuid`.
fn first_field(encoded: &[u8]) -> Option<&[u8]> {
    let [low, high, rest @ ..] = encoded else {
        return None;
    };
    rest.get(..usize::from(u16::from_le_bytes([*low, *high])))
}
