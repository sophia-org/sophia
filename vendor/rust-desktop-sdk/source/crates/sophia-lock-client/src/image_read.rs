//! Bounded whole-file reads on the session pipeline for image discovery.
//! A successful result is visible only after EOF and clunk. Remote errors keep
//! their errno; the adapter alone decides whether an absent file permits bytes.
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Fid, QidKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Busy,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Walk,
    Open,
    Read,
    Clunk,
    Done,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    Walk,
    Open(Fid),
    Read { fid: Fid, offset: u64, count: u32 },
    Clunk(Fid),
}

pub struct ImageRead {
    stage: Stage,
    pending: bool,
    fid: Fid,
    qid: u64,
    iounit: u32,
    sent: usize,
    cap: usize,
    bytes: Vec<u8>,
    remote: Option<u32>,
    clunk_failed: bool,
}
impl ImageRead {
    pub fn new(cap: usize) -> Result<Self, Error> {
        // No image discovery object exceeds the normative formats maximum.
        if cap == 0 || cap > 64 + 40 * 4096 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            stage: Stage::Walk,
            pending: false,
            fid: Fid::NOFID,
            qid: 0,
            iounit: 0,
            sent: 0,
            cap,
            bytes: Vec::new(),
            remote: None,
            clunk_failed: false,
        })
    }
    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }
    /// A failed cleanup is never evidence that discovery is absent.
    pub fn clunk_failed(&self) -> bool {
        self.clunk_failed
    }
    pub fn operation(&self, msize: u32) -> Result<Option<Operation>, Error> {
        if self.pending || self.done() {
            return Ok(None);
        }
        Ok(Some(match self.stage {
            Stage::Walk => Operation::Walk,
            Stage::Open => Operation::Open(self.fid),
            Stage::Read => {
                let room = (self.cap - self.bytes.len()).max(1); // EOF probe at exact capacity.
                let mut count = room.min(
                    msize
                        .checked_sub(11)
                        .filter(|n| *n > 0)
                        .ok_or(Error::Invalid)? as usize,
                ) as u32;
                if self.iounit != 0 {
                    count = count.min(self.iounit);
                }
                Operation::Read {
                    fid: self.fid,
                    offset: self.bytes.len() as u64,
                    count,
                }
            }
            Stage::Clunk => Operation::Clunk(self.fid),
            Stage::Done => unreachable!(),
        }))
    }
    pub fn issued(&mut self, msize: u32, fid: Fid) -> Result<(), Error> {
        let op = self.operation(msize)?.ok_or(Error::Busy)?;
        if matches!(op, Operation::Walk) != (fid != Fid::NOFID) {
            return Err(Error::Invalid);
        }
        if let Operation::Read { count, .. } = op {
            self.sent = count as usize;
        }
        if matches!(op, Operation::Walk) {
            self.fid = fid;
        }
        self.pending = true;
        Ok(())
    }
    pub fn reply(&mut self, reply: Reply) -> Result<(), Error> {
        if !self.pending {
            return Err(Error::Invalid);
        }
        self.pending = false;
        if let Reply::Error(errno) = reply {
            if self.stage == Stage::Clunk {
                self.remote = Some(errno.0);
                self.clunk_failed = true;
            } else {
                self.remote.get_or_insert(errno.0);
            }
            self.stage = match self.stage {
                Stage::Walk | Stage::Clunk => Stage::Done,
                _ => Stage::Clunk,
            };
            return Ok(());
        }
        match (self.stage, reply) {
            (Stage::Walk, Reply::Walk(qids))
                if qids.len() == 1 && qids[0].kind == QidKind::File =>
            {
                self.qid = qids[0].path;
                self.stage = Stage::Open;
            }
            (Stage::Open, Reply::Lopen { qid, iounit })
                if qid.kind == QidKind::File && qid.path == self.qid =>
            {
                self.iounit = iounit;
                self.stage = Stage::Read;
            }
            (Stage::Read, Reply::Read(bytes)) => {
                if bytes.len() > self.sent {
                    return Err(Error::Invalid);
                }
                if bytes.len() > self.cap - self.bytes.len() {
                    self.remote = Some(71); // EPROTO: failed discovery, never absence.
                    self.stage = Stage::Clunk;
                    return Ok(());
                }
                if bytes.is_empty() {
                    self.stage = Stage::Clunk;
                } else {
                    self.bytes.extend_from_slice(&bytes);
                }
            }
            (Stage::Clunk, Reply::Clunk) => self.stage = Stage::Done,
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
    pub fn into_result(self) -> Result<Result<Vec<u8>, u32>, Error> {
        if !self.done() {
            return Err(Error::Busy);
        }
        Ok(match self.remote {
            Some(errno) => Err(errno),
            None => Ok(self.bytes),
        })
    }
}
