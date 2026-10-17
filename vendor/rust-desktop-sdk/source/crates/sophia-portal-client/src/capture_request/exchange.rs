//! Capture operation state shared by exclusive and multiplexed drivers.
//! Queues bounded protocol operations but never polls or owns the connection.
use super::Error;
use crate::{
    capture::{Custody, Pending},
    protocol::{CaptureRequest, CaptureState},
    snapshot::{Fetch, File, Operation},
};
use sophia_9p_client::pipeline::{Pipeline, Reply};
use sophia_9p_records::{Fid, QidKind, Tag};
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Walk,
    Open,
    Write,
    Clunk,
    Wait,
    Status,
    Ready,
}

pub(crate) struct Exchange {
    capture: Pending,
    fetch: Option<Fetch<'static>>,
    stage: Stage,
    pending: Option<Tag>,
    fid: Fid,
    serial: u64,
    now: u64,
    deadline: u64,
    error: Option<Error>,
}
impl Exchange {
    pub(crate) fn new(
        request: CaptureRequest,
        previous_serial: u64,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if request.serial <= previous_serial {
            return Err(Error::Invalid);
        }
        Ok(Self {
            capture: Pending::new(request, now, deadline)?,
            fetch: None,
            stage: Stage::Walk,
            pending: None,
            fid: Fid::NOFID,
            serial: request.serial,
            now,
            deadline,
            error: None,
        })
    }
    pub(crate) fn now(&self) -> u64 {
        self.now
    }
    pub(crate) fn matches(&self, tag: Tag) -> bool {
        self.pending == Some(tag)
    }
    pub fn custody(&self) -> Custody {
        self.capture.custody()
    }
    pub fn serial(&self) -> u64 {
        self.serial
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    pub fn outcome(&self) -> Option<CaptureState> {
        (self.stage == Stage::Ready && self.error.is_none())
            .then(|| self.capture.outcome())
            .flatten()
    }
    pub fn next_wake(&self) -> Option<u64> {
        if self.error.is_some() || self.stage == Stage::Ready {
            None
        } else if self.stage == Stage::Wait {
            self.capture.next_wake()
        } else {
            Some(self.deadline)
        }
    }
    pub(crate) fn fail(&mut self, e: Error) -> Error {
        self.capture.close();
        self.error.get_or_insert(e).clone()
    }
    pub(crate) fn step(&mut self, now: u64, w: &mut Pipeline, root: Fid) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.now = now;
        if self.stage == Stage::Ready {
            return Ok(());
        }
        self.capture.advance(now)?;
        self.drive(w, root)
    }
    pub(crate) fn reply(&mut self, tag: Tag, reply: Reply) -> Result<(), Error> {
        if self.pending.take() != Some(tag) {
            return Err(Error::Invalid);
        }
        self.accept(reply)
    }
    fn drive(&mut self, w: &mut Pipeline, root: Fid) -> Result<(), Error> {
        if self.pending.is_some() || self.stage == Stage::Ready {
            return Ok(());
        }
        if self.stage == Stage::Wait {
            if self.capture.next_wake().is_some_and(|t| t > self.now) {
                return Ok(());
            }
            let deadline = self.capture.start_status(self.now)?;
            self.fetch = Some(Fetch::new(File::CaptureStatus, self.now, deadline)?);
            self.stage = Stage::Status;
        }
        self.pending = Some(match self.stage {
            Stage::Walk => {
                let (t, f) = w.walk(root, &[b"capture"])?;
                self.fid = f;
                t
            }
            Stage::Open => w.lopen(self.fid, 1)?,
            Stage::Write => {
                let tag = w.write(self.fid, 0, self.capture.bytes())?;
                self.capture.issued(self.now)?;
                tag
            }
            Stage::Clunk => w.clunk(self.fid)?,
            Stage::Status => {
                let fetch = self.fetch.as_mut().ok_or(Error::Invalid)?;
                let op = fetch
                    .operation(self.now, w.msize())?
                    .ok_or(Error::Invalid)?;
                let mut fid = Fid::NOFID;
                let t = match op {
                    Operation::Walk(file) => {
                        let (t, f) = w.walk(root, &[file.name()])?;
                        fid = f;
                        t
                    }
                    Operation::Open(f) => w.lopen(f, 0)?,
                    Operation::Read { fid, offset, count } => w.read(fid, offset, count)?,
                    Operation::Clunk(f) => w.clunk(f)?,
                };
                fetch.issued(self.now, w.msize(), fid)?;
                t
            }
            _ => return Err(Error::Invalid),
        });
        Ok(())
    }
    fn accept(&mut self, reply: Reply) -> Result<(), Error> {
        if self.stage == Stage::Write {
            let result = match reply {
                Reply::Write(n) => Ok(n),
                Reply::Error(e) => Err(e.0),
                _ => return Err(Error::Invalid),
            };
            self.capture.write_reply(result, self.now)?;
            self.stage = Stage::Clunk;
            return Ok(());
        }
        if let Reply::Error(e) = reply {
            return Err(Error::Remote(e.0));
        }
        if self.stage == Stage::Status {
            let fetch = self.fetch.as_mut().ok_or(Error::Invalid)?;
            fetch.reply(reply, self.now)?;
            if let Some(bytes) = fetch.result() {
                self.capture.status(bytes, self.now)?;
                self.stage = if self.capture.outcome().is_some() {
                    Stage::Ready
                } else {
                    Stage::Wait
                };
                self.fetch = None;
            }
            return Ok(());
        }
        match (self.stage, reply) {
            (Stage::Walk, Reply::Walk(q)) if q.len() == 1 && q[0].kind == QidKind::File => {
                self.stage = Stage::Open
            }
            (Stage::Open, Reply::Lopen { qid, iounit })
                if qid.kind == QidKind::File && (iounit == 0 || iounit >= 24) =>
            {
                self.stage = Stage::Write
            }
            (Stage::Clunk, Reply::Clunk) => self.stage = Stage::Wait,
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}
