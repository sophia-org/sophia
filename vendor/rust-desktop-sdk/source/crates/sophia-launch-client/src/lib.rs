//! Session-attested, Factotum-verified local launch-channel authentication.
//!
//! This is Sophia's launch conversation, not a generic 9P or SASL mechanism.
//! The caller transfers exclusive ownership of the connected role stream and
//! its supervisor-provisioned generation stream. Version negotiation blocks
//! within the same five-second total deadline; subsequent progress is bounded
//! and nonblocking. No identity, launch number or nonce is accepted from the
//! caller. Claims must match the server's policy and select no authority.
use sophia_9p_client::pipeline::{Pipeline, PipelineError, PipelineLimits, Reply};
use sophia_9p_client::records::{Errno, Fid, Tag};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

mod exchange;
use exchange::Stage;

pub const MAX_DURATION: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Wire(PipelineError),
    Remote(Errno),
    Invalid(&'static str),
    Timeout,
    Cancelled,
    NotReady,
}
impl From<PipelineError> for Error {
    fn from(error: PipelineError) -> Self {
        Self::Wire(error)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wire(e) => write!(f, "launch authentication: {e}"),
            Self::Remote(e) => write!(f, "launch authentication refused: {}", e.0),
            Self::Invalid(what) => write!(f, "invalid launch authentication: {what}"),
            Self::Timeout => f.write_str("launch authentication deadline"),
            Self::Cancelled => f.write_str("launch authentication cancelled"),
            Self::NotReady => f.write_str("launch authentication is not ready"),
        }
    }
}
impl std::error::Error for Error {}

/// One conversation. Do not drive these streams from another client while it
/// owns them. Failure/cancellation shuts both transports; there is no legacy
/// attach fallback and a partially published root cannot escape on failure.
pub struct Client {
    role: Option<Pipeline>,
    relay: Option<Pipeline>,
    claims: [Vec<u8>; 2],
    deadline: Instant,
    retry: Instant,
    stage: Stage,
    pending: Option<Tag>,
    afid: Option<Fid>,
    root: Option<Fid>,
    relay_root: Option<Fid>,
    rpc: Option<Fid>,
    challenge: [u8; 40],
    failed: Option<Error>,
}

/// The authenticated root and both exclusive pipelines. Temporary afid and
/// relay fids have been clunked. Keep the relay for the whole launch generation:
/// a later Tversion requires a fresh attempt over that same capability.
pub struct Authenticated {
    pub role: Pipeline,
    pub relay: Pipeline,
    pub root: Fid,
    claims: [Vec<u8>; 2],
}
impl Authenticated {
    /// Reset both wire conversations and prove the same claims again. This
    /// deliberately invalidates all role fids. The server retains its immutable
    /// principal/namespace/launch pin; the old verdict is never reused.
    pub fn reauthenticate(self) -> Result<Client, Error> {
        if self.role.outstanding() != 0 || self.relay.outstanding() != 0 {
            return Err(Error::Invalid("pending requests at reset"));
        }
        let role = duplicate_stream(&self.role)?;
        let relay = duplicate_stream(&self.relay)?;
        let claims = self.claims.clone();
        drop(self);
        Client::over(role, relay, &claims[0], &claims[1])
    }
}
fn duplicate_stream(pipe: &Pipeline) -> Result<UnixStream, Error> {
    pipe.as_fd()
        .try_clone_to_owned()
        .map(UnixStream::from)
        .map_err(|e| Error::Wire(PipelineError::Io(e.kind())))
}

impl Client {
    pub fn over(
        role: UnixStream,
        relay: UnixStream,
        uname: &[u8],
        aname: &[u8],
    ) -> Result<Self, Error> {
        Self::with_timeout(role, relay, uname, aname, MAX_DURATION)
    }

    /// A caller may shorten the total deadline, never extend the contract's
    /// five-second bound. The same deadline includes both version handshakes.
    pub fn with_timeout(
        role: UnixStream,
        relay: UnixStream,
        uname: &[u8],
        aname: &[u8],
        timeout: Duration,
    ) -> Result<Self, Error> {
        if uname.len() > 255 || aname.len() > 255 || timeout.is_zero() || timeout > MAX_DURATION {
            return Err(Error::Invalid("claims or timeout bound"));
        }
        let flags = rustix::io::fcntl_getfd(&relay)
            .map_err(|e| Error::Wire(PipelineError::Io(std::io::Error::from(e).kind())))?;
        rustix::io::fcntl_setfd(&relay, flags | rustix::io::FdFlags::CLOEXEC)
            .map_err(|e| Error::Wire(PipelineError::Io(std::io::Error::from(e).kind())))?;
        let deadline = Instant::now() + timeout;
        let role = Pipeline::over(role, PipelineLimits::default(), remaining(deadline)?)?;
        let relay = Pipeline::over(
            relay,
            PipelineLimits {
                msize: 4096,
                max_outstanding: 1,
                max_fids: 2,
                max_buffered_input: 8192,
                max_buffered_output: 8192,
            },
            remaining(deadline)?,
        )?;
        Ok(Self {
            role: Some(role),
            relay: Some(relay),
            claims: [uname.to_vec(), aname.to_vec()],
            deadline,
            retry: Instant::now(),
            stage: Stage::RelayAttach,
            pending: None,
            afid: None,
            root: None,
            relay_root: None,
            rpc: None,
            challenge: [0; 40],
            failed: None,
        })
    }
    pub fn ready(&self) -> bool {
        self.failed.is_none() && self.stage == Stage::Done
    }

    pub fn poll(&mut self) -> Result<(), Error> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self.ready() {
            return Ok(());
        }
        let result = self.progress();
        if let Err(error) = &result {
            self.fail(error.clone());
        }
        result
    }

    fn progress(&mut self) -> Result<(), Error> {
        remaining(self.deadline)?;
        if self.pending.is_none() && Instant::now() >= self.retry {
            self.pending = Some(self.start()?);
        }
        self.role.as_mut().unwrap().poll()?;
        self.relay.as_mut().unwrap().poll()?;
        let reply = if self.stage.relay() {
            self.relay.as_mut().unwrap().take_reply()
        } else {
            self.role.as_mut().unwrap().take_reply()
        };
        if let Some((tag, reply)) = reply {
            if self.pending != Some(tag) {
                return Err(Error::Invalid("uncorrelated reply"));
            }
            self.pending = None;
            if self.stage == Stage::Auth && reply == Reply::Error(Errno(11)) {
                self.retry = Instant::now() + RETRY;
                return Ok(());
            }
            self.answer(reply)?;
        }
        Ok(())
    }

    /// Wait on the active wire or the exact retry/deadline, whichever is first.
    /// Call poll again afterwards. This never introduces a fixed polling sleep.
    pub fn wait_for_io(&self, maximum: Duration) -> Result<(), Error> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self.ready() {
            return Ok(());
        }
        let left = remaining(self.deadline)?.min(maximum);
        if self.pending.is_none() {
            std::thread::sleep(left.min(self.retry.saturating_duration_since(Instant::now())));
            return Ok(());
        }
        let pipe = if self.stage.relay() {
            &self.relay
        } else {
            &self.role
        };
        pipe.as_ref()
            .unwrap()
            .wait_for_io(left)
            .map_err(Error::from)
    }
    pub fn cancel(&mut self) {
        self.fail(Error::Cancelled);
    }
    fn fail(&mut self, error: Error) {
        if self.failed.is_none() {
            self.failed = Some(error);
        }
        for pipe in [&self.role, &self.relay].into_iter().flatten() {
            let _ = rustix::net::shutdown(pipe, rustix::net::Shutdown::Both);
        }
    }
    pub fn into_authenticated(mut self) -> Result<Authenticated, Error> {
        if !self.ready() {
            return Err(self.failed.clone().unwrap_or(Error::NotReady));
        }
        Ok(Authenticated {
            role: self.role.take().unwrap(),
            relay: self.relay.take().unwrap(),
            root: self.root.unwrap(),
            claims: self.claims.clone(),
        })
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        // Dropping a pending exchange must not leave an authenticated root
        // usable through an accidental duplicate descriptor.
        if self.role.is_some() {
            self.fail(Error::Cancelled);
        }
    }
}
fn remaining(deadline: Instant) -> Result<Duration, Error> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or(Error::Timeout)
}
