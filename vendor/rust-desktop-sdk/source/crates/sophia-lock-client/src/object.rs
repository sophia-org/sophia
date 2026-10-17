//! Fetch a Lock object through EOF and clunk, or discard a superseded announcement.
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Fid, QidKind};
use sophia_lock_protocol::lock_files::*;

const OBJECT_BYTES: usize = 1024;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Busy,
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
    Walk,
    Open(Fid),
    Read { fid: Fid, offset: u64, count: u32 },
    Clunk(Fid),
}
pub struct Fetch {
    epoch: u64,
    publication: LockObjectPublished,
    stage: Stage,
    outstanding: bool,
    fid: Fid,
    iounit: u32,
    sent: usize,
    bytes: [u8; OBJECT_BYTES],
    used: usize,
    eof: bool,
    superseded: bool,
    value: Option<LockObject>,
}
impl Fetch {
    pub fn new(epoch: u64, publication: LockObjectPublished) -> Result<Self, Error> {
        if epoch == 0 || publication.object_generation == 0 || publication.qid_path == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            epoch,
            publication,
            stage: Stage::Walk,
            outstanding: false,
            fid: Fid::NOFID,
            iounit: 0,
            sent: 0,
            bytes: [0; OBJECT_BYTES],
            used: 0,
            eof: false,
            superseded: false,
            value: None,
        })
    }
    pub fn is_ready(&self) -> bool {
        self.stage == Stage::Ready
    }
    pub fn superseded(&self) -> bool {
        self.is_ready() && self.superseded
    }
    /// Move the verified value into Session; do not clone its allocations.
    pub fn take_result(&mut self) -> Option<LockObject> {
        if self.is_ready() {
            self.value.take()
        } else {
            None
        }
    }
    pub fn operation(&self, msize: u32) -> Result<Option<Operation>, Error> {
        if self.outstanding || self.is_ready() {
            return Ok(None);
        }
        Ok(Some(match self.stage {
            Stage::Walk => Operation::Walk,
            Stage::Open => Operation::Open(self.fid),
            Stage::Read => {
                let room = if self.eof {
                    1
                } else {
                    OBJECT_BYTES - self.used
                };
                let mut count = room.min(
                    msize
                        .checked_sub(11)
                        .filter(|n| *n != 0)
                        .ok_or(Error::Invalid)? as usize,
                ) as u32;
                if self.iounit != 0 {
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
    pub fn issued(&mut self, msize: u32, fid: Fid) -> Result<(), Error> {
        let op = self.operation(msize)?.ok_or(Error::Busy)?;
        if matches!(op, Operation::Walk) != (fid != Fid::NOFID) {
            return Err(Error::Invalid);
        }
        self.sent = if let Operation::Read { count, .. } = op {
            count as usize
        } else {
            0
        };
        if matches!(op, Operation::Walk) {
            self.fid = fid;
        }
        self.outstanding = true;
        Ok(())
    }
    /// Any error terminates the owning session. There is no implicit retry.
    pub fn reply(&mut self, reply: Reply) -> Result<(), Error> {
        if !self.outstanding {
            return Err(Error::Invalid);
        }
        self.outstanding = false;
        if let Reply::Error(e) = reply {
            return Err(Error::Remote(e.0));
        }
        match (self.stage, reply) {
            (Stage::Walk, Reply::Walk(q)) if q.len() == 1 && q[0].kind == QidKind::File => {
                self.pin(q[0].path)?;
                self.stage = if self.superseded {
                    Stage::Clunk
                } else {
                    Stage::Open
                };
            }
            (Stage::Open, Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                self.pin(qid.path)?;
                self.iounit = iounit;
                self.stage = if self.superseded {
                    Stage::Clunk
                } else {
                    Stage::Read
                };
            }
            (Stage::Read, Reply::Read(data)) => {
                if data.len() > self.sent {
                    return Err(Error::Invalid);
                }
                if self.eof {
                    if !data.is_empty() {
                        return Err(Error::Invalid);
                    }
                    let r =
                        decode_lock_file_record(&self.bytes[..self.used], LockFileClass::Object)
                            .map_err(|_| Error::Invalid)?;
                    if r.header.kind != LockFileKind::Lock
                        || r.header.connection_epoch != self.epoch
                    {
                        return Err(Error::Invalid);
                    }
                    self.value = Some(LockObject::decode(r.body).map_err(|_| Error::Invalid)?);
                    self.stage = Stage::Clunk;
                } else {
                    if data.is_empty() || data.len() > OBJECT_BYTES - self.used {
                        return Err(Error::Invalid);
                    }
                    self.bytes[self.used..self.used + data.len()].copy_from_slice(&data);
                    self.used += data.len();
                    if self.used >= 4 {
                        let n = u32::from_le_bytes(self.bytes[..4].try_into().unwrap()) as usize;
                        if !(LOCK_FILE_HEADER_BYTES..=OBJECT_BYTES).contains(&n) || self.used > n {
                            return Err(Error::Invalid);
                        }
                        self.eof = self.used == n;
                    }
                }
            }
            (Stage::Clunk, Reply::Clunk) => self.stage = Stage::Ready,
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
    fn pin(&mut self, path: u64) -> Result<(), Error> {
        if path < self.publication.qid_path {
            return Err(Error::Invalid);
        }
        self.superseded = path > self.publication.qid_path;
        Ok(())
    }
}
