//! One retained watch. It identifies its own walked object, not a separately
//! fetched payload. No idle reads or automatic reopening follow revocation.
use crate::startup::Opened;
use sophia_9p_client::pipeline::{IoBudget, IoInterest, PipelineError, Reply};
use sophia_9p_records::{Fid, QidKind, Tag};
use std::os::fd::{AsFd, BorrowedFd};
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    ClockRegression,
    Deadline,
    Closed,
    Remote(u32),
    Wire(PipelineError),
}
impl From<PipelineError> for Error {
    fn from(e: PipelineError) -> Self {
        Self::Wire(e)
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Walk,
    Open,
    Read,
    Clunk,
    Ready,
}
pub struct Watch {
    opened: Option<Opened>,
    stage: Stage,
    pending: Option<Tag>,
    fid: Fid,
    now: u64,
    deadline: u64,
    error: Option<Error>,
}
impl Watch {
    /// Exclusive role ownership and one absolute caller deadline. Retain the
    /// launch relay separately. This constructor queues work but performs no I/O.
    pub fn new(opened: Opened, now: u64, deadline: u64) -> Result<Self, Error> {
        if now < opened.now {
            return Err(Error::ClockRegression);
        }
        if deadline <= now {
            return Err(Error::Deadline);
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
            stage: Stage::Walk,
            pending: None,
            fid: Fid::NOFID,
            now,
            deadline,
            error: None,
        };
        s.drive()?;
        Ok(s)
    }
    /// The nonempty offset-zero watch read has been queued, not acknowledged.
    pub fn waiting(&self) -> bool {
        self.error.is_none() && self.stage == Stage::Read
    }
    /// True only after ESTALE on the retained object and successful clunk.
    pub fn revoked(&self) -> bool {
        self.error.is_none() && self.stage == Stage::Ready
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.error.is_none() && !self.revoked()).then_some(self.deadline)
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
        self.opened.take();
        self.error.get_or_insert(e).clone()
    }
    /// Cancels by closing the owned role, including its pending read. It does
    /// not claim revocation or preserve this connection for reuse.
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    /// Return the same root after verified revocation; explicit reopening may
    /// select a different object and is never done automatically.
    pub fn finish(mut self) -> Result<Opened, Error> {
        if let Some(e) = self.error {
            return Err(e);
        }
        if !self.revoked() {
            return Err(Error::Invalid);
        }
        let mut o = self.opened.take().ok_or(Error::Closed)?;
        o.now = self.now;
        Ok(o)
    }
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.now = now;
        if self.revoked() {
            return Ok(());
        }
        if now >= self.deadline {
            return Err(self.fail(Error::Deadline));
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
            if self.pending.take() != Some(tag) {
                return Err(self.fail(Error::Invalid));
            }
            if let Err(e) = self.accept(reply).and_then(|()| self.drive()) {
                return Err(self.fail(e));
            }
            if self.revoked() || budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
    fn drive(&mut self) -> Result<(), Error> {
        if self.pending.is_some() || self.stage == Stage::Ready {
            return Ok(());
        }
        let o = self.opened.as_mut().ok_or(Error::Closed)?;
        self.pending = Some(match self.stage {
            Stage::Walk => {
                let (t, f) = o.pipeline.walk(o.root, &[b"watch"])?;
                self.fid = f;
                t
            }
            Stage::Open => o.pipeline.lopen(self.fid, 0)?,
            Stage::Read => o.pipeline.read(self.fid, 0, 1)?,
            Stage::Clunk => o.pipeline.clunk(self.fid)?,
            Stage::Ready => unreachable!(),
        });
        Ok(())
    }
    fn accept(&mut self, reply: Reply) -> Result<(), Error> {
        if let Reply::Error(e) = reply {
            if e.0 == 116 && matches!(self.stage, Stage::Open | Stage::Read) {
                self.stage = Stage::Clunk;
                return Ok(());
            }
            return Err(Error::Remote(e.0));
        }
        match (self.stage, reply) {
            (Stage::Walk, Reply::Walk(q)) if q.len() == 1 && q[0].kind == QidKind::File => {
                self.stage = Stage::Open
            }
            (Stage::Open, Reply::Lopen { qid, .. }) if qid.kind == QidKind::File => {
                self.stage = Stage::Read
            }
            (Stage::Clunk, Reply::Clunk) => self.stage = Stage::Ready,
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}
