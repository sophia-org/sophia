//! Metadata startup on an exclusively owned authenticated recipient root.
//! Caller retains the launch relay; this driver never attaches or negotiates.
use crate::{
    protocol::{Api, decode_api},
    snapshot::{Fetch, File, Operation},
};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, Pipeline, PipelineError, Reply};
use sophia_9p_records::{Fid, Tag};
use std::os::fd::{AsFd, BorrowedFd};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Wire(PipelineError),
    Snapshot(crate::snapshot::Error),
    Invalid,
    Remote(u32),
    Deadline,
    ClockRegression,
    Closed,
    NotOpened,
}
impl From<PipelineError> for Error {
    fn from(v: PipelineError) -> Self {
        Self::Wire(v)
    }
}
impl From<crate::snapshot::Error> for Error {
    fn from(v: crate::snapshot::Error) -> Self {
        Self::Snapshot(v)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Api,
    Ready,
}

pub struct Opening {
    wire: Option<Pipeline>,
    stage: Stage,
    pending: Option<Tag>,
    root: Fid,
    fetch: Fetch<'static>,
    info: Option<Api>,
    now: u64,
    deadline: u64,
    error: Option<Error>,
}
/// Same pipeline and fids; handoff performs no attach, version or reconnect.
pub struct Opened {
    pub pipeline: Pipeline,
    pub root: Fid,
    pub info: Api,
    pub now: u64,
}
impl Opening {
    /// Requires the sole root from completed launch authentication. Keep the relay
    /// alive externally. Root ownership is checked, but is not proof of admission.
    pub fn from_attached(
        wire: Pipeline,
        root: Fid,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if deadline <= now {
            return Err(Error::Deadline);
        }
        let limits = wire.limits();
        if !wire.is_quiescent()
            || wire.fid_count() != 1
            || !wire.owns_fid(root)
            || !(512..=65536).contains(&wire.msize())
            || limits.max_fids < 2
            || limits.max_outstanding < 2
            || limits.max_buffered_input < wire.msize() as usize
        {
            return Err(Error::Invalid);
        }
        let mut value = Self {
            wire: Some(wire),
            stage: Stage::Api,
            pending: None,
            root,
            fetch: Fetch::new(File::Api, now, deadline)?,
            info: None,
            now,
            deadline,
            error: None,
        };
        value.drive()?;
        Ok(value)
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    pub fn is_opened(&self) -> bool {
        self.stage == Stage::Ready && self.error.is_none()
    }
    pub fn deadline(&self) -> Option<u64> {
        (!self.is_opened() && self.error.is_none()).then_some(self.deadline)
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.wire.as_ref().map(AsFd::as_fd)
    }
    pub fn io_interest(&self) -> Result<IoInterest, Error> {
        self.wire
            .as_ref()
            .ok_or(Error::Closed)?
            .io_interest()
            .map_err(Into::into)
    }
    fn fail(&mut self, error: Error) -> Result<(), Error> {
        let error = self.error.get_or_insert(error).clone();
        self.wire.take();
        Err(error)
    }
    pub fn close(&mut self) {
        let _ = self.fail(Error::Closed);
    }
    /// Shared byte/syscall budget across at most 32 rounds. No internal wait.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.now = now;
        if self.is_opened() {
            return Ok(());
        }
        if now >= self.deadline {
            return self.fail(Error::Deadline);
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            let wire = self.wire.as_mut().ok_or(Error::Closed)?;
            if let Err(error) = wire.poll_with_budget(&mut budget) {
                return self.fail(error.into());
            }
            let Some((tag, reply)) = wire.take_reply() else {
                break;
            };
            if self.pending.take() != Some(tag) {
                return self.fail(Error::Invalid);
            }
            if let Err(error) = self.accept(reply).and_then(|()| self.drive()) {
                return self.fail(error);
            }
            if self.is_opened() || budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
    pub fn into_opened(mut self) -> Result<Opened, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if !self.is_opened() {
            return Err(Error::NotOpened);
        }
        Ok(Opened {
            pipeline: self.wire.take().ok_or(Error::Closed)?,
            root: self.root,
            info: self.info.ok_or(Error::Invalid)?,
            now: self.now,
        })
    }
    fn drive(&mut self) -> Result<(), Error> {
        if self.pending.is_some() || self.stage == Stage::Ready {
            return Ok(());
        }
        let wire = self.wire.as_mut().ok_or(Error::Closed)?;
        self.pending = Some(match self.stage {
            Stage::Api => {
                let op = self
                    .fetch
                    .operation(self.now, wire.msize())?
                    .ok_or(Error::Invalid)?;
                let mut fid = Fid::NOFID;
                let tag = match op {
                    Operation::Walk(file) => {
                        let (tag, walked) = wire.walk(self.root, &[file.name()])?;
                        fid = walked;
                        tag
                    }
                    Operation::Open(fid) => wire.lopen(fid, 0)?,
                    Operation::Read { fid, offset, count } => wire.read(fid, offset, count)?,
                    Operation::Clunk(fid) => wire.clunk(fid)?,
                };
                self.fetch.issued(self.now, wire.msize(), fid)?;
                tag
            }
            Stage::Ready => unreachable!(),
        });
        Ok(())
    }
    fn accept(&mut self, reply: Reply) -> Result<(), Error> {
        if let Reply::Error(errno) = reply {
            return Err(Error::Remote(errno.0));
        }
        match self.stage {
            Stage::Api => {
                self.fetch.reply(reply, self.now)?;
                if let Some(bytes) = self.fetch.result() {
                    let info = decode_api(bytes).map_err(|_| Error::Invalid)?;
                    self.info = Some(info);
                    self.stage = Stage::Ready;
                }
            }
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}
