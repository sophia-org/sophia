//! One opaque retained payload, read directly into borrowed caller storage.
use crate::{
    snapshot::{Fetch, Operation},
    startup::Opened,
};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, PipelineError};
use sophia_9p_records::{Fid, Tag};
use std::os::fd::{AsFd, BorrowedFd};
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    ClockRegression,
    Deadline,
    Closed,
    Wire(PipelineError),
    Snapshot(crate::snapshot::Error),
}
impl From<PipelineError> for Error {
    fn from(e: PipelineError) -> Self {
        Self::Wire(e)
    }
}
impl From<crate::snapshot::Error> for Error {
    fn from(e: crate::snapshot::Error) -> Self {
        Self::Snapshot(e)
    }
}
pub struct Transfer<'a> {
    opened: Option<Opened>,
    fetch: Fetch<'a>,
    pending: Option<Tag>,
    now: u64,
    deadline: u64,
    error: Option<Error>,
}
impl<'a> Transfer<'a> {
    /// Takes exclusive role ownership; retain the launch relay separately.
    /// Storage needs at least 64 KiB and is not cleared. No allocation here;
    /// the transport's owned replies still allocate. Failure drops the role.
    pub fn new(
        opened: Opened,
        storage: &'a mut [u8],
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if now < opened.now {
            return Err(Error::ClockRegression);
        }
        let w = &opened.pipeline;
        if !w.is_quiescent()
            || w.fid_count() != 1
            || !w.owns_fid(opened.root)
            || !(512..=65536).contains(&w.msize())
            || w.limits().max_fids < 2
            || w.limits().max_outstanding < 2
            || w.limits().max_buffered_input < w.msize() as usize
        {
            return Err(Error::Invalid);
        }
        let mut s = Self {
            opened: Some(opened),
            fetch: Fetch::transfer(storage, now, deadline)?,
            pending: None,
            now,
            deadline,
            error: None,
        };
        s.drive()?;
        Ok(s)
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    /// Scratch progress, not a completed result or a grant.
    pub fn received(&self) -> usize {
        self.fetch.received()
    }
    pub fn result(&self) -> Option<&[u8]> {
        if self.error.is_none() {
            self.fetch.result()
        } else {
            None
        }
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.error.is_none() && self.result().is_none()).then_some(self.deadline)
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
    fn fail(&mut self, e: Error) -> Error {
        self.fetch.close();
        self.opened.take();
        self.error.get_or_insert(e).clone()
    }
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    /// Transfer the same root and the completed slice without a payload copy.
    /// A separately walked watch is not known to identify this payload.
    pub fn finish(mut self) -> Result<(Opened, &'a mut [u8]), Error> {
        if let Some(e) = self.error {
            return Err(e);
        }
        let bytes = self.fetch.into_transfer()?;
        let mut opened = self.opened.take().ok_or(Error::Closed)?;
        opened.now = self.now;
        Ok((opened, bytes))
    }
    /// Caller-monotonic clock; one shared byte/syscall budget, at most 32 rounds.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.now = now;
        if self.result().is_some() {
            return Ok(());
        }
        if now >= self.deadline {
            return Err(self.fail(Error::Deadline));
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            let wire = &mut self.opened.as_mut().ok_or(Error::Closed)?.pipeline;
            if let Err(e) = wire.poll_with_budget(&mut budget) {
                return Err(self.fail(e.into()));
            }
            let Some((tag, reply)) = wire.take_reply() else {
                break;
            };
            if self.pending.take() != Some(tag) {
                return Err(self.fail(Error::Invalid));
            }
            if let Err(e) = self
                .fetch
                .reply(reply, now)
                .map_err(Error::from)
                .and_then(|()| self.drive())
            {
                return Err(self.fail(e));
            }
            if self.result().is_some() || budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
    fn drive(&mut self) -> Result<(), Error> {
        if self.pending.is_some() || self.fetch.result().is_some() {
            return Ok(());
        }
        let o = self.opened.as_mut().ok_or(Error::Closed)?;
        let w = &mut o.pipeline;
        let op = self
            .fetch
            .operation(self.now, w.msize())?
            .ok_or(Error::Invalid)?;
        let mut fid = Fid::NOFID;
        let tag = match op {
            Operation::Walk(file) => {
                let (t, f) = w.walk(o.root, &[file.name()])?;
                fid = f;
                t
            }
            Operation::Open(f) => w.lopen(f, 0)?,
            Operation::Read { fid, offset, count } => w.read(fid, offset, count)?,
            Operation::Clunk(f) => w.clunk(f)?,
        };
        self.fetch.issued(self.now, w.msize(), fid)?;
        self.pending = Some(tag);
        Ok(())
    }
}
