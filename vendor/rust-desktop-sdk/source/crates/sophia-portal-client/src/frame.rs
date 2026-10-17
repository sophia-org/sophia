//! One retained frame. Partial pixels never become a completed image.
use crate::{protocol::FrameInfo, startup::Opened};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, PipelineError};
use std::os::fd::{AsFd, BorrowedFd};
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Wire(PipelineError),
    Remote(u32),
    Deadline,
    ClockRegression,
    Closed,
    Capacity,
}
impl From<PipelineError> for Error {
    fn from(e: PipelineError) -> Self {
        Self::Wire(e)
    }
}
mod exchange;
pub(crate) use exchange::Exchange;
pub(crate) const READ_CHUNK: u32 = 60 * 1024;
pub struct Frame {
    opened: Option<Opened>,
    exchange: Exchange,
}
impl Frame {
    /// Exclusive role ownership. Reuses caller storage; at most one image
    /// allocation after metadata validation. Retain the launch relay separately.
    pub fn new(opened: Opened, storage: Vec<u8>, now: u64, deadline: u64) -> Result<Self, Error> {
        let w = &opened.pipeline;
        if now < opened.now {
            return Err(Error::ClockRegression);
        }
        if !w.is_quiescent()
            || w.fid_count() != 1
            || !w.owns_fid(opened.root)
            || w.limits().max_fids < 3
            || w.limits().max_outstanding < 4
            || w.limits().max_buffered_input < w.msize() as usize * 4
        {
            return Err(Error::Invalid);
        }
        let exchange = Exchange::new(storage, w.msize(), now, deadline)?;
        let mut s = Self {
            opened: Some(opened),
            exchange,
        };
        s.drive(now)?;
        Ok(s)
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
    pub fn deadline(&self) -> Option<u64> {
        self.exchange.deadline()
    }
    pub fn error(&self) -> Option<&Error> {
        self.exchange.error()
    }
    /// Accepted scratch bytes, not image validity.
    pub fn received(&self) -> u64 {
        self.exchange.received()
    }
    pub fn result(&self) -> Option<(FrameInfo, &[u8])> {
        self.exchange.result()
    }
    /// Transfer the completed image and original role without copying pixels.
    pub fn finish(mut self) -> Result<(Opened, FrameInfo, Vec<u8>), Error> {
        if let Some(e) = self.error() {
            return Err(e.clone());
        }
        let mut o = self.opened.take().ok_or(Error::Closed)?;
        o.now = self.exchange.now();
        let (info, pixels) = self.exchange.finish()?;
        Ok((o, info, pixels))
    }
    /// Failed/cancelled storage remains scratch, never a completed image.
    pub fn into_scratch(self) -> Vec<u8> {
        self.exchange.into_scratch()
    }
    fn fail(&mut self, e: Error) -> Error {
        self.opened.take();
        self.exchange.fail(e)
    }
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    fn drive(&mut self, now: u64) -> Result<(), Error> {
        let o = self.opened.as_mut().ok_or(Error::Closed)?;
        self.exchange.step(now, &mut o.pipeline, o.root)
    }
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
        if self.result().is_some() {
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
            if self.result().is_some() || budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
}
