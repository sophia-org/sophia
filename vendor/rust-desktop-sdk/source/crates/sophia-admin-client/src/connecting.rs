//! Nonblocking startup from a caller-connected HostDomain socket.
use crate::{session::Session, startup::Opening};
use sophia_9p_client::pipeline::{IoInterest, PipelineError, PipelineLimits, VersionHandshake};
use std::os::{fd::BorrowedFd, unix::net::UnixStream};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Wire(PipelineError),
    Startup(crate::startup::Error),
    Session(crate::session::Error),
    Deadline,
    ClockRegression,
    Closed,
}
enum Phase {
    Version(VersionHandshake),
    Files(Box<Opening>),
    Catalog(Box<Session>),
}
pub struct Connecting {
    phase: Option<Phase>,
    now: u64,
    deadline: u64,
    error: Option<Error>,
}
impl Connecting {
    /// Caller selects and connects the socket. Admission is enforced by the
    /// export; this API neither probes endpoints nor falls back to legacy IPC.
    pub fn new(
        stream: UnixStream,
        limits: PipelineLimits,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        let version = VersionHandshake::new(stream, limits, now, deadline).map_err(Error::Wire)?;
        Ok(Self {
            phase: Some(Phase::Version(version)),
            now,
            deadline,
            error: None,
        })
    }
    pub fn ready(&self) -> bool {
        self.error.is_none()
            && matches!(&self.phase, Some(Phase::Catalog(s)) if s.catalog_generation().is_some())
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    pub fn next_wake(&self) -> Option<u64> {
        if self.error.is_some() || self.ready() {
            return None;
        }
        match self.phase.as_ref()? {
            Phase::Catalog(s) => Some(
                s.next_wake()
                    .map_or(self.deadline, |n| n.min(self.deadline)),
            ),
            _ => Some(self.deadline),
        }
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        match self.phase.as_ref()? {
            Phase::Version(v) => v.fd(),
            Phase::Files(f) => f.fd(),
            Phase::Catalog(s) => s.fd(),
        }
    }
    pub fn io_interest(&self) -> Result<IoInterest, Error> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let mut interest = match self.phase.as_ref().ok_or(Error::Closed)? {
            Phase::Version(v) => v.io_interest(),
            Phase::Files(f) => f.io_interest().map_err(Error::Startup)?,
            Phase::Catalog(s) => s.io_interest().map_err(Error::Session)?,
        };
        interest.immediate |= self.ready();
        Ok(interest)
    }
    fn fail(&mut self, error: Error) -> Error {
        self.phase.take();
        self.error.get_or_insert(error).clone()
    }
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    /// Only one phase spends the caller's I/O budget per call. Transitions
    /// queue work without I/O; the deadline covers version, files and catalog.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.now = now;
        if self.ready() {
            return Ok(());
        }
        if now >= self.deadline {
            return Err(self.fail(Error::Deadline));
        }
        let phase = self.phase.take().ok_or(Error::Closed)?;
        let result = (|| -> Result<Phase, Error> {
            match phase {
                Phase::Version(mut v) => {
                    v.dispatch(now, bytes).map_err(Error::Wire)?;
                    if let Some(wire) = v.take_pipeline() {
                        Ok(Phase::Files(Box::new(
                            Opening::new(wire, now, self.deadline).map_err(Error::Startup)?,
                        )))
                    } else {
                        Ok(Phase::Version(v))
                    }
                }
                Phase::Files(mut f) => {
                    f.dispatch(now, bytes).map_err(Error::Startup)?;
                    if f.is_opened() {
                        Ok(Phase::Catalog(Box::new(
                            Session::from_opened(f.into_opened().map_err(Error::Startup)?)
                                .map_err(Error::Session)?,
                        )))
                    } else {
                        Ok(Phase::Files(f))
                    }
                }
                Phase::Catalog(mut s) => {
                    s.dispatch(now, bytes).map_err(Error::Session)?;
                    Ok(Phase::Catalog(s))
                }
            }
        })();
        match result {
            Ok(phase) => {
                self.phase = Some(phase);
                Ok(())
            }
            Err(e) => Err(self.fail(e)),
        }
    }
    /// Transfer the ready session once. Subsequent calls return None.
    pub fn take_session(&mut self) -> Option<Session> {
        if !self.ready() {
            return None;
        }
        match self.phase.take()? {
            Phase::Catalog(s) => Some(*s),
            _ => unreachable!(),
        }
    }
}
