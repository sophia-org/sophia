//! Session-attested, Factotum-verified local launch-channel authentication.
//!
//! This is Sophia's launch conversation, not a generic 9P or SASL mechanism.
//! The caller transfers exclusive ownership of the connected role stream and
//! its supervisor-provisioned generation stream. `Client::new` starts without
//! socket I/O; caller time and bounded dispatch cover version negotiation too.
//! The convenience constructors wait for version negotiation within the same
//! five-second total deadline. No identity, launch number or nonce is accepted from the
//! caller. Claims must match the server's policy and select no authority.
use sophia_9p_client::pipeline::{
    IoBudget, Pipeline, PipelineError, PipelineLimits, Reply, VersionHandshake,
};
use sophia_9p_client::records::{Errno, Fid, Tag};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

mod event_loop;
mod exchange;
use exchange::Stage;

pub const MAX_DURATION: Duration = Duration::from_secs(5);
const RETRY_MS: u64 = 10;

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
    versions: [Option<VersionHandshake>; 2],
    claims: [Vec<u8>; 2],
    origin: Instant,
    origin_ms: u64,
    wall_deadline: Option<Instant>,
    now: u64,
    deadline: u64,
    retry: u64,
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
        let (role, relay, claims) = self.reset_streams()?;
        Client::over(role, relay, &claims[0], &claims[1])
    }

    /// Caller-driven reset, with the same invalidation and ownership rules as
    /// `reauthenticate`. Queues negotiation without socket I/O.
    pub fn reauthenticate_at(self, now: u64, deadline: u64) -> Result<Client, Error> {
        let (role, relay, claims) = self.reset_streams()?;
        Client::new(role, relay, &claims[0], &claims[1], now, deadline)
    }

    fn reset_streams(self) -> Result<(UnixStream, UnixStream, [Vec<u8>; 2]), Error> {
        if self.role.outstanding() != 0 || self.relay.outstanding() != 0 {
            return Err(Error::Invalid("pending requests at reset"));
        }
        if !self.role.is_quiescent() || !self.relay.is_quiescent() || self.relay.fid_count() != 0 {
            return Err(Error::Invalid("nonquiescent authentication reset"));
        }
        let role = duplicate_stream(&self.role)?;
        let relay = duplicate_stream(&self.relay)?;
        let claims = self.claims.clone();
        drop(self);
        Ok((role, relay, claims))
    }
}
fn duplicate_stream(pipe: &Pipeline) -> Result<UnixStream, Error> {
    pipe.as_fd()
        .try_clone_to_owned()
        .map(UnixStream::from)
        .map_err(|e| Error::Wire(PipelineError::Io(e.kind())))
}

impl Client {
    /// Authenticate another endpoint using exclusive ownership of the launch's
    /// quiescent relay. Only the relay is reset: previously admitted role
    /// pipelines and their fids remain owned by the caller and are untouched.
    /// Do not retain another driver or duplicate of the relay. As with `over`,
    /// conversation failure shuts the relay and may revoke the whole launch.
    pub fn over_relay(
        role: UnixStream,
        relay: Pipeline,
        uname: &[u8],
        aname: &[u8],
    ) -> Result<Self, Error> {
        Self::over(role, empty_relay_stream(relay)?, uname, aname)
    }

    /// Caller-driven counterpart of `over_relay`. Refuses live relay fids,
    /// pending traffic and partial input before queuing a fresh negotiation.
    pub fn over_relay_at(
        role: UnixStream,
        relay: Pipeline,
        uname: &[u8],
        aname: &[u8],
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        Self::new(
            role,
            empty_relay_stream(relay)?,
            uname,
            aname,
            now,
            deadline,
        )
    }

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
        let deadline = Instant::now() + timeout;
        let mut client = Self::new(
            role,
            relay,
            uname,
            aname,
            0,
            timeout.as_millis().max(1) as u64,
        )?;
        client.wall_deadline = Some(deadline);
        while client.versions.iter().any(Option::is_some) {
            remaining(deadline)?;
            client.poll()?;
            if client.versions.iter().any(Option::is_some) {
                client.wait_for_io(remaining(deadline)?)?;
            }
        }
        Ok(client)
    }

    /// Adopt both streams without socket I/O. `now` and `deadline` are absolute
    /// monotonic milliseconds from the caller's clock. The total interval must
    /// be positive and at most 5000 ms, including both version handshakes.
    /// Drive with `dispatch`; do not mix caller time with `poll`'s wall clock.
    pub fn new(
        role: UnixStream,
        relay: UnixStream,
        uname: &[u8],
        aname: &[u8],
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if uname.len() > 255 || aname.len() > 255 || deadline <= now || deadline - now > 5000 {
            return Err(Error::Invalid("claims or timeout bound"));
        }
        let flags = rustix::io::fcntl_getfd(&relay)
            .map_err(|e| Error::Wire(PipelineError::Io(std::io::Error::from(e).kind())))?;
        rustix::io::fcntl_setfd(&relay, flags | rustix::io::FdFlags::CLOEXEC)
            .map_err(|e| Error::Wire(PipelineError::Io(std::io::Error::from(e).kind())))?;
        let role = VersionHandshake::new(role, PipelineLimits::default(), now, deadline)?;
        let relay = VersionHandshake::new(
            relay,
            PipelineLimits {
                msize: 4096,
                max_outstanding: 1,
                max_fids: 2,
                max_buffered_input: 8192,
                max_buffered_output: 8192,
            },
            now,
            deadline,
        )?;
        Ok(Self {
            role: None,
            relay: None,
            versions: [Some(role), Some(relay)],
            claims: [uname.to_vec(), aname.to_vec()],
            origin: Instant::now(),
            origin_ms: now,
            wall_deadline: None,
            now,
            deadline,
            retry: now,
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
        if self.failed.is_none()
            && !self.ready()
            && self.wall_deadline.is_some_and(|end| Instant::now() >= end)
        {
            self.fail(Error::Timeout);
        }
        self.dispatch(self.wall_now(), 256 * 1024)
    }

    /// One bounded pass with caller time. At most `bytes` and 32 syscall
    /// attempts per direction across both wires, including negotiation.
    /// Zero bytes permits local progress but no socket I/O. Clock regression
    /// and deadline expiry fail closed; neither can renew the attempt.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self.ready() {
            return Ok(());
        }
        let result = if now < self.now {
            Err(Error::Invalid("clock regressed"))
        } else {
            self.now = now;
            self.progress(&mut IoBudget::new(bytes))
        };
        if let Err(error) = &result {
            self.fail(error.clone());
        }
        result
    }

    fn progress(&mut self, budget: &mut IoBudget) -> Result<(), Error> {
        if self.now >= self.deadline {
            return Err(Error::Timeout);
        }
        if self.versions.iter().any(Option::is_some) {
            for (version, pipe) in self
                .versions
                .iter_mut()
                .zip([&mut self.role, &mut self.relay])
            {
                if let Some(handshake) = version {
                    handshake.poll_with_budget(self.now, budget)?;
                    if let Some(negotiated) = handshake.take_pipeline() {
                        *pipe = Some(negotiated);
                        *version = None;
                    }
                }
            }
            return Ok(());
        }
        if self.pending.is_none() && self.now >= self.retry {
            self.pending = Some(self.start()?);
        }
        self.role.as_mut().unwrap().poll_with_budget(budget)?;
        self.relay.as_mut().unwrap().poll_with_budget(budget)?;
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
                self.retry = self.now.saturating_add(RETRY_MS);
                return Ok(());
            }
            self.answer(reply)?;
        }
        Ok(())
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
        for version in self.versions.iter_mut().flatten() {
            if let Some(fd) = version.fd() {
                let _ = rustix::net::shutdown(fd, rustix::net::Shutdown::Both);
            }
            version.close();
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
        if self.role.is_some() || self.versions.iter().any(Option::is_some) {
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

fn empty_relay_stream(relay: Pipeline) -> Result<UnixStream, Error> {
    if relay.outstanding() != 0 {
        return Err(Error::Invalid("pending requests on shared launch relay"));
    }
    if !relay.is_quiescent() || relay.fid_count() != 0 {
        return Err(Error::Invalid("shared launch relay is not empty"));
    }
    duplicate_stream(&relay)
}
