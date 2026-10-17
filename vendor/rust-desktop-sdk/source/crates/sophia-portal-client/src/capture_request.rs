//! Capture write and status observation on an exclusive authenticated role.
use crate::{
    capture::Custody,
    protocol::{CaptureRequest, CaptureState, RequestCapability},
    startup::Opened,
};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, PipelineError};
use std::os::fd::{AsFd, BorrowedFd};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Unsupported,
    ClockRegression,
    Closed,
    Remote(u32),
    Wire(PipelineError),
    Capture(crate::capture::Error),
    Snapshot(crate::snapshot::Error),
}
impl From<PipelineError> for Error {
    fn from(e: PipelineError) -> Self {
        Self::Wire(e)
    }
}
impl From<crate::capture::Error> for Error {
    fn from(e: crate::capture::Error) -> Self {
        Self::Capture(e)
    }
}
impl From<crate::snapshot::Error> for Error {
    fn from(e: crate::snapshot::Error) -> Self {
        Self::Snapshot(e)
    }
}
mod exchange;
pub(crate) use exchange::Exchange;

pub struct Request {
    opened: Option<Opened>,
    exchange: Exchange,
}
impl Request {
    /// Reserve an increasing durable serial before use, including reconnects.
    /// Takes exclusive role ownership; retain the launch relay separately.
    pub fn new(
        opened: Opened,
        request: CaptureRequest,
        previous_serial: u64,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if now < opened.now {
            return Err(Error::ClockRegression);
        }
        if opened.info.request != RequestCapability::OutputCaptureV1 {
            return Err(Error::Unsupported);
        }
        let w = &opened.pipeline;
        if !w.is_quiescent()
            || w.fid_count() != 1
            || !w.owns_fid(opened.root)
            || w.limits().max_fids < 2
            || w.limits().max_outstanding < 2
            || !(512..=65536).contains(&w.msize())
            || w.limits().max_buffered_input < w.msize() as usize
        {
            return Err(Error::Invalid);
        }
        let exchange = Exchange::new(request, previous_serial, now, deadline)?;
        let mut s = Self {
            opened: Some(opened),
            exchange,
        };
        s.drive(now)?;
        Ok(s)
    }
    pub fn custody(&self) -> Custody {
        self.exchange.custody()
    }
    pub fn serial(&self) -> u64 {
        self.exchange.serial()
    }
    pub fn error(&self) -> Option<&Error> {
        self.exchange.error()
    }
    pub fn outcome(&self) -> Option<CaptureState> {
        self.exchange.outcome()
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.opened.as_ref().map(|o| o.pipeline.as_fd())
    }
    pub fn io_interest(&self) -> Result<IoInterest, Error> {
        self.opened
            .as_ref()
            .ok_or(Error::Closed)?
            .pipeline
            .io_interest()
            .map_err(Into::into)
    }
    pub fn next_wake(&self) -> Option<u64> {
        self.exchange.next_wake()
    }
    fn fail(&mut self, e: Error) -> Error {
        self.opened.take();
        self.exchange.fail(e)
    }
    /// Stops local observation, not an already issued server request.
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    pub fn finish(mut self) -> Result<(Opened, CaptureState, u64), Error> {
        if let Some(e) = self.error() {
            return Err(e.clone());
        }
        let outcome = self.outcome().ok_or(Error::Invalid)?;
        let mut o = self.opened.take().ok_or(Error::Closed)?;
        o.now = self.exchange.now();
        Ok((o, outcome, self.serial()))
    }
    fn drive(&mut self, now: u64) -> Result<(), Error> {
        let o = self.opened.as_mut().ok_or(Error::Closed)?;
        self.exchange.step(now, &mut o.pipeline, o.root)
    }
    /// Shared byte/syscall budget, at most 32 replies, and no internal wait.
    /// Dispatch at next_wake even if the descriptor is idle.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        if now < self.exchange.now() {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = self.error() {
            return Err(e.clone());
        }
        if let Err(e) = self.drive(now) {
            return Err(self.fail(e));
        }
        if self.outcome().is_some() {
            return Ok(());
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            let w = &mut self.opened.as_mut().ok_or(Error::Closed)?.pipeline;
            if let Err(e) = w.poll_with_budget(&mut budget) {
                return Err(self.fail(e.into()));
            }
            let Some((tag, reply)) = w.take_reply() else {
                break;
            };
            if let Err(e) = self
                .exchange
                .reply(tag, reply)
                .and_then(|()| self.drive(now))
            {
                return Err(self.fail(e));
            }
            if self.outcome().is_some() || budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
}
