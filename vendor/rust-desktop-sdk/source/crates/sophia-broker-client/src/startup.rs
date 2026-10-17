//! Discovery on a caller-admitted trusted metadata broker root. No authentication,
//! attach, version exchange or reconnect occurs here.
use crate::protocol::{Limits, decode_api, decode_limits};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, Pipeline, PipelineError, Reply};
use sophia_9p_records::{Fid, QidKind, Tag};
use std::os::fd::{AsFd, BorrowedFd};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Wire(PipelineError),
    Invalid,
    Remote(u32),
    Deadline,
    ClockRegression,
    Closed,
    NotReady,
}
impl From<PipelineError> for Error {
    fn from(e: PipelineError) -> Self {
        Self::Wire(e)
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Stage {
    Walk,
    Open,
    Read,
    Clunk,
    Ready,
}
pub struct Opening {
    wire: Option<Pipeline>,
    root: Fid,
    fid: Fid,
    tag: Option<Tag>,
    stage: Stage,
    limits_file: bool,
    bytes: [u8; 128],
    used: usize,
    sent: u32,
    iounit: u32,
    epoch: u64,
    limits: Option<Limits>,
    now: u64,
    deadline: u64,
    error: Option<Error>,
}
/// Verified discovery, not completed negotiation. The same admitted root and
/// connection pass to the metadata broker session; the caller retains its authority
/// channel for the connection's lifetime.
pub struct Opened {
    pub pipeline: Pipeline,
    pub root: Fid,
    pub epoch: u64,
    pub limits: Limits,
    pub now: u64,
}
impl Opening {
    /// The caller must supply a trusted metadata broker connection. Root ownership
    /// and matching metadata are not evidence of admission by themselves.
    pub fn from_attached(
        wire: Pipeline,
        root: Fid,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if deadline <= now {
            return Err(Error::Deadline);
        }
        let bounds = wire.limits();
        if !wire.is_quiescent()
            || wire.fid_count() != 1
            || !wire.owns_fid(root)
            || !(512..=65536).contains(&wire.msize())
            || bounds.max_fids < 2
            || bounds.max_outstanding < 2
            || bounds.max_buffered_input < wire.msize() as usize
        {
            return Err(Error::Invalid);
        }
        let mut s = Self {
            wire: Some(wire),
            root,
            fid: Fid::NOFID,
            tag: None,
            stage: Stage::Walk,
            limits_file: false,
            bytes: [0; 128],
            used: 0,
            sent: 0,
            iounit: 0,
            epoch: 0,
            limits: None,
            now,
            deadline,
            error: None,
        };
        s.drive()?;
        Ok(s)
    }
    pub fn ready(&self) -> bool {
        self.error.is_none() && self.stage == Stage::Ready
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    pub fn deadline(&self) -> Option<u64> {
        (!self.ready() && self.error.is_none()).then_some(self.deadline)
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.wire.as_ref().map(AsFd::as_fd)
    }
    pub fn io_interest(&self) -> Result<IoInterest, Error> {
        Ok(self.wire.as_ref().ok_or(Error::Closed)?.io_interest()?)
    }
    fn fail(&mut self, e: Error) -> Result<(), Error> {
        let e = self.error.get_or_insert(e).clone();
        self.wire.take();
        Err(e)
    }
    pub fn close(&mut self) {
        let _ = self.fail(Error::Closed);
    }
    pub fn into_opened(mut self) -> Result<Opened, Error> {
        if let Some(e) = self.error {
            return Err(e);
        }
        if !self.ready() {
            return Err(Error::NotReady);
        }
        Ok(Opened {
            pipeline: self.wire.take().ok_or(Error::Closed)?,
            root: self.root,
            epoch: self.epoch,
            limits: self.limits.ok_or(Error::Invalid)?,
            now: self.now,
        })
    }
    /// One shared I/O budget, at most 32 replies, no wait. The original deadline
    /// spans both exact metadata reads and their successful clunks.
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
            return self.fail(Error::Deadline);
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            let wire = self.wire.as_mut().ok_or(Error::Closed)?;
            if let Err(e) = wire.poll_with_budget(&mut budget) {
                return self.fail(e.into());
            }
            let Some((tag, reply)) = wire.take_reply() else {
                break;
            };
            if self.tag.take() != Some(tag) {
                return self.fail(Error::Invalid);
            }
            if let Err(e) = self.accept(reply).and_then(|()| self.drive()) {
                return self.fail(e);
            }
            if self.ready() || budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
    fn drive(&mut self) -> Result<(), Error> {
        if self.tag.is_some() || self.stage == Stage::Ready {
            return Ok(());
        }
        let wire = self.wire.as_mut().ok_or(Error::Closed)?;
        self.tag = Some(match self.stage {
            Stage::Walk => {
                let name: &[u8] = if self.limits_file { b"limits" } else { b"api" };
                let (tag, fid) = wire.walk(self.root, &[name])?;
                self.fid = fid;
                tag
            }
            Stage::Open => wire.lopen(self.fid, 0)?,
            Stage::Read => {
                let maximum = if self.limits_file { 72 } else { 128 };
                self.sent = (maximum + 1 - self.used as u32).min(wire.msize() - 11);
                if self.iounit != 0 {
                    self.sent = self.sent.min(self.iounit);
                }
                wire.read(self.fid, self.used as u64, self.sent)?
            }
            Stage::Clunk => wire.clunk(self.fid)?,
            Stage::Ready => unreachable!(),
        });
        Ok(())
    }
    fn accept(&mut self, reply: Reply) -> Result<(), Error> {
        if let Reply::Error(e) = reply {
            return Err(Error::Remote(e.0));
        }
        match (self.stage, reply) {
            (Stage::Walk, Reply::Walk(qids))
                if qids.len() == 1 && qids[0].kind == QidKind::File =>
            {
                self.stage = Stage::Open
            }
            (Stage::Open, Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                self.iounit = iounit;
                self.stage = Stage::Read;
            }
            (Stage::Read, Reply::Read(data)) => {
                let max = if self.limits_file { 72 } else { 128 };
                if data.len() > self.sent as usize || data.len() > max - self.used {
                    return Err(Error::Invalid);
                }
                if data.is_empty() {
                    if self.limits_file {
                        self.limits = Some(
                            decode_limits(&self.bytes[..self.used], self.epoch)
                                .map_err(|_| Error::Invalid)?,
                        );
                    } else {
                        self.epoch =
                            decode_api(&self.bytes[..self.used]).map_err(|_| Error::Invalid)?;
                    }
                    self.stage = Stage::Clunk;
                } else {
                    self.bytes[self.used..self.used + data.len()].copy_from_slice(&data);
                    self.used += data.len();
                }
            }
            (Stage::Clunk, Reply::Clunk) => {
                if self.limits_file {
                    self.stage = Stage::Ready;
                } else {
                    self.limits_file = true;
                    self.used = 0;
                    self.iounit = 0;
                    self.stage = Stage::Walk;
                }
            }
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}
