//! A single immutable snapshot on an existing admitted portal recipient connection.
//! The connection driver matches tags and closes the transport on terminal error.
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Fid, QidKind};
use std::ops::{Deref, DerefMut};
pub const TRANSFER_MAX_BYTES: usize = 65_536;
use sophia_portal_protocol::{
    API_MAX_BYTES, CAPTURE_STATUS_BYTES, STATUS_MAX_BYTES, decode_api, decode_capture_status,
    decode_status,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum File {
    Api,
    CaptureStatus,
    Status,
    Transfer,
}
impl File {
    pub const fn name(self) -> &'static [u8] {
        match self {
            Self::Api => b"api",
            Self::CaptureStatus => b"capture-status",
            Self::Status => b"status",
            Self::Transfer => b"transfer",
        }
    }
    pub const fn maximum(self) -> usize {
        match self {
            Self::CaptureStatus => CAPTURE_STATUS_BYTES,
            Self::Api => API_MAX_BYTES,
            Self::Status => STATUS_MAX_BYTES,
            Self::Transfer => TRANSFER_MAX_BYTES,
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

enum Storage<'a> {
    Inline([u8; API_MAX_BYTES]),
    Borrowed(&'a mut [u8]),
}
impl Deref for Storage<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Inline(b) => b,
            Self::Borrowed(b) => b,
        }
    }
}
impl DerefMut for Storage<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Inline(b) => b,
            Self::Borrowed(b) => b,
        }
    }
}
pub struct Fetch<'a> {
    file: File,
    stage: Stage,
    fid: Fid,
    outstanding: bool,
    sent: usize,
    iounit: u32,
    bytes: Storage<'a>,
    used: usize,
    now: u64,
    deadline: u64,
    error: Option<Error>,
}
impl<'a> Fetch<'a> {
    /// Inline bounded metadata storage; no allocation or I/O. Transfer requires
    /// the caller-storage constructor and is refused here.
    pub fn new(file: File, now: u64, deadline: u64) -> Result<Self, Error> {
        Self::with_storage(file, Storage::Inline([0; API_MAX_BYTES]), now, deadline)
    }
    /// Opaque retained transfer, using at least 64 KiB of caller storage. Does
    /// not clear or allocate that storage. Partial bytes remain scratch until
    /// EOF and clunk succeed. The driver matches replies and retains the fid;
    /// no relationship to a separately walked watch fid is inferred.
    pub fn transfer(storage: &'a mut [u8], now: u64, deadline: u64) -> Result<Self, Error> {
        Self::with_storage(File::Transfer, Storage::Borrowed(storage), now, deadline)
    }
    fn with_storage(
        file: File,
        bytes: Storage<'a>,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if bytes.len() < file.maximum() {
            return Err(Error::Invalid);
        }
        if deadline <= now {
            return Err(Error::Deadline);
        }
        Ok(Self {
            file,
            stage: Stage::Walk,
            fid: Fid::NOFID,
            outstanding: false,
            sent: 0,
            iounit: 0,
            bytes,
            used: 0,
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
        (self.stage == Stage::Ready && self.error.is_none()).then_some(&self.bytes[..self.used])
    }
    pub fn received(&self) -> usize {
        self.used
    }
    /// Return the completed transfer slice without copying. Metadata fetches
    /// and unfinished transfers cannot use this handoff.
    pub fn into_transfer(self) -> Result<&'a mut [u8], Error> {
        let (bytes, used) = self.into_transfer_storage()?;
        Ok(&mut bytes[..used])
    }
    /// Return all caller storage plus the completed payload length for reuse.
    pub fn into_transfer_storage(self) -> Result<(&'a mut [u8], usize), Error> {
        if self.result().is_none() || self.file != File::Transfer {
            return Err(Error::Invalid);
        }
        match self.bytes {
            Storage::Borrowed(bytes) => Ok((bytes, self.used)),
            Storage::Inline(_) => Err(Error::Invalid),
        }
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
                let mut count = (self.file.maximum() + 1 - self.used).min(
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
                    offset: self.used as u64,
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
                if data.len() > self.sent || data.len() > self.file.maximum() - self.used {
                    return Err(Error::Invalid);
                }
                if data.is_empty() {
                    match self.file {
                        File::Api => decode_api(&self.bytes[..self.used]).map(|_| ()),
                        File::CaptureStatus => {
                            decode_capture_status(&self.bytes[..self.used]).map(|_| ())
                        }
                        File::Status => decode_status(&self.bytes[..self.used]).map(|_| ()),
                        File::Transfer => Ok(()),
                    }
                    .map_err(|_| Error::Invalid)?;
                    self.stage = Stage::Clunk;
                } else {
                    self.bytes[self.used..self.used + data.len()].copy_from_slice(&data);
                    self.used += data.len();
                }
            }
            (Stage::Clunk, Reply::Clunk) => self.stage = Stage::Ready,
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}
