//! A single immutable snapshot on an existing admitted administrative connection.
//! The connection driver matches tags and closes the transport on terminal error.
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Fid, QidKind};
use sophia_admin_protocol::{
    CATALOG_MAX_BYTES, LINE_MAX_BYTES, decode_catalog, decode_info, decode_status,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum File {
    Info,
    Commands,
    Status,
}
impl File {
    pub const fn name(self) -> &'static [u8] {
        match self {
            Self::Info => b"info",
            Self::Commands => b"commands",
            Self::Status => b"status",
        }
    }
    pub const fn maximum(self) -> usize {
        match self {
            Self::Commands => CATALOG_MAX_BYTES,
            _ => LINE_MAX_BYTES,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Busy,
    Deadline,
    ClockRegression,
    Closed,
    Remote(u32),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Walk,
    Open,
    Read,
    Clunk,
    Ready,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Walk(File),
    Open(Fid),
    Read { fid: Fid, offset: u64, count: u32 },
    Clunk(Fid),
}

pub struct Fetch {
    file: File,
    stage: Stage,
    fid: Fid,
    outstanding: bool,
    sent: usize,
    iounit: u32,
    bytes: Vec<u8>,
    now: u64,
    deadline: u64,
    error: Option<Error>,
}
impl Fetch {
    /// Reuses supplied storage after clearing it. Reserve once for this file's
    /// bound; subsequent reads never grow the buffer. No I/O in the constructor.
    pub fn new(file: File, mut bytes: Vec<u8>, now: u64, deadline: u64) -> Result<Self, Error> {
        if deadline <= now {
            return Err(Error::Deadline);
        }
        bytes.clear();
        bytes.reserve(file.maximum());
        Ok(Self {
            file,
            stage: Stage::Walk,
            fid: Fid::NOFID,
            outstanding: false,
            sent: 0,
            iounit: 0,
            bytes,
            now,
            deadline,
            error: None,
        })
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.stage != Stage::Ready && self.error.is_none()).then_some(self.deadline)
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn result(&self) -> Option<&[u8]> {
        (self.stage == Stage::Ready && self.error.is_none()).then_some(self.bytes.as_slice())
    }
    /// Transfers storage without copying it. Before successful completion these
    /// bytes are unverified scratch, not a snapshot result; use result() to check.
    pub fn into_storage(self) -> Vec<u8> {
        self.bytes
    }
    pub fn close(&mut self) {
        self.error.get_or_insert(Error::Closed);
    }
    fn check(&self, now: u64) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(error) = self.error {
            return Err(error);
        }
        if self.stage != Stage::Ready && now >= self.deadline {
            return Err(Error::Deadline);
        }
        Ok(())
    }
    pub fn operation(&self, now: u64, msize: u32) -> Result<Option<Operation>, Error> {
        self.check(now)?;
        if self.outstanding || self.stage == Stage::Ready {
            return Ok(None);
        }
        Ok(Some(match self.stage {
            Stage::Walk => Operation::Walk(self.file),
            Stage::Open => Operation::Open(self.fid),
            Stage::Read => {
                let mut count = (self.file.maximum() + 1 - self.bytes.len()).min(
                    msize
                        .checked_sub(11)
                        .filter(|n| *n > 0)
                        .ok_or(Error::Invalid)? as usize,
                ) as u32;
                if self.iounit > 0 {
                    count = count.min(self.iounit);
                }
                Operation::Read {
                    fid: self.fid,
                    offset: self.bytes.len() as u64,
                    count,
                }
            }
            Stage::Clunk => Operation::Clunk(self.fid),
            Stage::Ready => unreachable!(),
        }))
    }
    /// Call only after the pipeline accepts the operation. Only Walk supplies fid.
    pub fn issued(&mut self, now: u64, msize: u32, fid: Fid) -> Result<(), Error> {
        let op = self.operation(now, msize)?.ok_or(Error::Busy)?;
        if matches!(op, Operation::Walk(_)) != (fid != Fid::NOFID) {
            return Err(Error::Invalid);
        }
        if matches!(op, Operation::Walk(_)) {
            self.fid = fid;
        }
        self.sent = if let Operation::Read { count, .. } = op {
            count as usize
        } else {
            0
        };
        self.now = now;
        self.outstanding = true;
        Ok(())
    }
    /// Only matched replies belong here. Any terminal error forbids continuation;
    /// caller must close the connection, never reconnect or retry automatically.
    pub fn reply(&mut self, reply: Reply, now: u64) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        let result = self.accept(reply, now);
        if let Err(error) = result {
            self.error.get_or_insert(error);
        }
        result
    }
    fn accept(&mut self, reply: Reply, now: u64) -> Result<(), Error> {
        self.check(now)?;
        if !self.outstanding {
            return Err(Error::Invalid);
        }
        self.now = now;
        self.outstanding = false;
        if let Reply::Error(errno) = reply {
            return Err(Error::Remote(errno.0));
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
                if data.len() > self.sent || data.len() > self.file.maximum() - self.bytes.len() {
                    return Err(Error::Invalid);
                }
                if data.is_empty() {
                    match self.file {
                        File::Info => decode_info(&self.bytes).map(|_| ()),
                        File::Commands => decode_catalog(&self.bytes).map(|_| ()),
                        File::Status => decode_status(&self.bytes).map(|_| ()),
                    }
                    .map_err(|_| Error::Invalid)?;
                    self.stage = Stage::Clunk;
                } else {
                    self.bytes.extend_from_slice(&data);
                }
            }
            (Stage::Clunk, Reply::Clunk) => self.stage = Stage::Ready,
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}
