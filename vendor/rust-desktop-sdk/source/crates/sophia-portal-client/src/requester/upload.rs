//! One upload/0 writer after correlated Begin custody. The session owns tags,
//! admission and the single-slot rule; this component never polls a socket.
use super::Error;
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Fid, QidKind};
#[derive(Clone, Copy, Eq, PartialEq)]
enum Stage {
    Walk,
    Open,
    Write,
    Held,
    Clunk,
    Done,
}
pub enum Operation<'a> {
    Walk,
    Open(Fid),
    Write {
        fid: Fid,
        offset: u64,
        bytes: &'a [u8],
    },
    Clunk(Fid),
}
pub struct Upload {
    bytes: Vec<u8>,
    stage: Stage,
    fid: Fid,
    outstanding: bool,
    written: usize,
    sent: usize,
    iounit: u32,
    now: u64,
    deadline: u64,
    error: Option<Error>,
    settling: bool,
}
impl Upload {
    /// Takes caller storage without copying or allocating. The owner must have
    /// matched Begin's Submitted receipt and exact declared payload length.
    pub fn new(mut bytes: Vec<u8>, maximum: u32, now: u64, deadline: u64) -> Result<Self, Error> {
        Self::take(&mut bytes, maximum, now, deadline)
    }
    pub(crate) fn take(
        bytes: &mut Vec<u8>,
        maximum: u32,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if maximum == 0 || maximum > 65536 || bytes.len() > maximum as usize {
            return Err(Error::Invalid);
        }
        if deadline <= now {
            return Err(Error::Deadline);
        }
        Ok(Self {
            bytes: std::mem::take(bytes),
            stage: Stage::Walk,
            fid: Fid::NOFID,
            outstanding: false,
            written: 0,
            sent: 0,
            iounit: 0,
            now,
            deadline,
            error: None,
            settling: false,
        })
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn written(&self) -> usize {
        self.written
    }
    pub fn held(&self) -> bool {
        self.error.is_none() && self.stage == Stage::Held
    }
    pub fn done(&self) -> bool {
        self.error.is_none() && self.stage == Stage::Done
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.error.is_none() && !self.done()).then_some(self.deadline)
    }
    /// Returns original storage. The caller must close the role connection if
    /// abandoning an unfinished upload; this method does not settle remote fids.
    pub fn into_buffer(self) -> Vec<u8> {
        self.bytes
    }
    pub fn close(&mut self) {
        self.error.get_or_insert(Error::Closed);
    }
    fn fail(&mut self, e: Error) -> Error {
        *self.error.get_or_insert(e)
    }
    fn advance(&mut self, now: u64) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(e) = self.error {
            return Err(e);
        }
        self.now = now;
        if self.stage != Stage::Done && now >= self.deadline {
            return Err(self.fail(Error::Deadline));
        }
        Ok(())
    }
    pub fn operation(&mut self, now: u64, msize: u32) -> Result<Option<Operation<'_>>, Error> {
        self.advance(now)?;
        if self.outstanding || matches!(self.stage, Stage::Held | Stage::Done) {
            return Ok(None);
        }
        if msize < 512 {
            return Err(self.fail(Error::Invalid));
        }
        Ok(Some(match self.stage {
            Stage::Walk => Operation::Walk,
            Stage::Open => Operation::Open(self.fid),
            Stage::Write => {
                let mut count = (self.bytes.len() - self.written).min((msize - 23) as usize);
                if self.iounit != 0 {
                    count = count.min(self.iounit as usize);
                }
                Operation::Write {
                    fid: self.fid,
                    offset: self.written as u64,
                    bytes: &self.bytes[self.written..self.written + count],
                }
            }
            Stage::Clunk => Operation::Clunk(self.fid),
            _ => unreachable!(),
        }))
    }
    /// Walk means exactly [upload, 0] from the admitted root. Supply its new
    /// fid only after pipeline admission; other operations require NOFID.
    pub fn issued(&mut self, now: u64, msize: u32, fid: Fid) -> Result<(), Error> {
        let Some(op) = self.operation(now, msize)? else {
            return Err(Error::Busy);
        };
        if matches!(op, Operation::Walk) != (fid != Fid::NOFID) {
            return Err(Error::Invalid);
        }
        self.sent = match op {
            Operation::Write { bytes, .. } => bytes.len(),
            _ => 0,
        };
        if self.stage == Stage::Walk {
            self.fid = fid;
        }
        self.outstanding = true;
        Ok(())
    }
    /// Explicit cleanup after End custody or terminal settlement. Calling this
    /// before End abandons the writer and can revoke the grant; never infer a
    /// terminal outcome from successful clunk alone.
    pub fn release(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.outstanding || self.stage != Stage::Held {
            return Err(Error::Busy);
        }
        self.stage = Stage::Clunk;
        Ok(())
    }
    /// After a correlated terminal outcome, stop new writes and drain any
    /// outstanding reply before clunking. ESTALE then confirms the ended binding
    /// rather than authorizing a reopen or replay.
    pub fn settle(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        self.settling = true;
        if !self.outstanding && self.stage != Stage::Done {
            self.stage = if self.fid == Fid::NOFID {
                Stage::Done
            } else {
                Stage::Clunk
            };
        }
        Ok(())
    }
    pub fn reply(&mut self, reply: Reply, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if !self.outstanding {
            return Err(Error::Busy);
        }
        self.outstanding = false;
        let result = if self.settling
            && matches!(self.stage, Stage::Walk | Stage::Open | Stage::Write)
            && matches!(reply, Reply::Error(sophia_9p_records::Errno(116)))
        {
            self.stage = if self.stage == Stage::Walk {
                Stage::Done
            } else {
                Stage::Clunk
            };
            Ok(())
        } else {
            self.accept(reply)
        };
        if result.is_ok() && self.settling && self.stage != Stage::Done {
            self.stage = Stage::Clunk;
        }
        result.map_err(|e| self.fail(e))
    }
    fn accept(&mut self, reply: Reply) -> Result<(), Error> {
        if let Reply::Error(e) = reply {
            return Err(Error::Remote(e.0));
        }
        match (self.stage, reply) {
            (Stage::Walk, Reply::Walk(q))
                if q.len() == 2
                    && q[0].kind == QidKind::Directory
                    && q[1].kind == QidKind::File =>
            {
                self.stage = Stage::Open
            }
            (Stage::Open, Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                self.iounit = iounit;
                self.stage = if self.bytes.is_empty() {
                    Stage::Held
                } else {
                    Stage::Write
                };
            }
            (Stage::Write, Reply::Write(count)) if count as usize == self.sent => {
                self.written += self.sent;
                if self.written == self.bytes.len() {
                    self.stage = Stage::Held;
                }
            }
            (Stage::Clunk, Reply::Clunk) => self.stage = Stage::Done,
            _ => return Err(Error::Protocol),
        }
        Ok(())
    }
}
