//! One upload with caller-owned admission and at most eight wire writes.
use crate::Candidate;
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::Fid;
use sophia_lock_protocol::lock_files::*;

pub const MAX_WRITES: usize = 8;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Busy,
    Closed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Idle,
    Begun,
    Walk,
    Open,
    Ready,
    Write,
    Ending,
    Clunk,
}
#[derive(Debug, PartialEq)]
pub enum Operation<'a> {
    Walk(u16),
    Open(Fid),
    Clunk(Fid),
    Write {
        slot: usize,
        fid: Fid,
        offset: u64,
        bytes: &'a [u8],
    },
    Cancel(LockResourceStep),
}
pub struct Upload {
    window: usize,
    capacity: usize,
    stage: Stage,
    begin: Option<LockResourceBegin>,
    fid: Fid,
    iounit: u32,
    outstanding: bool,
    writes: [Option<usize>; MAX_WRITES],
    data: Option<Vec<u8>>,
    base: u64,
    offset: u64,
    issued: usize,
    accepted: usize,
    cancel: bool,
    closing: bool,
    closed: bool,
    remote_error: Option<u32>,
}
impl Upload {
    pub fn new(request_capacity: usize) -> Result<Self, Error> {
        if request_capacity < 6 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            window: 1,
            capacity: request_capacity,
            stage: Stage::Idle,
            begin: None,
            fid: Fid::NOFID,
            iounit: 0,
            outstanding: false,
            writes: [None; MAX_WRITES],
            data: None,
            base: 0,
            offset: 0,
            issued: 0,
            accepted: 0,
            cancel: false,
            closing: false,
            closed: false,
            remote_error: None,
        })
    }
    pub fn window(&self) -> usize {
        self.window
    }
    pub fn set_window(&mut self, writes: usize) -> Result<(), Error> {
        if writes == 0 || writes > MAX_WRITES || writes + 5 > self.capacity {
            return Err(Error::Invalid);
        }
        if self.closed {
            return Err(Error::Closed);
        }
        if self.pending() {
            return Err(Error::Busy);
        }
        self.window = writes;
        Ok(())
    }
    pub fn pending(&self) -> bool {
        !self.closed && self.stage != Stage::Idle
    }
    pub fn ready(&self) -> bool {
        !self.closed && self.stage == Stage::Ready && !self.closing && !self.cancel
    }
    pub fn remote_error(&self) -> Option<u32> {
        self.remote_error
    }
    pub fn writes_pending(&self) -> usize {
        self.writes.iter().filter(|w| w.is_some()).count()
    }
    /// Session first checks negotiation and passes its candidate admission here.
    /// A failed admission leaves this upload unchanged.
    pub fn begin(
        &mut self,
        value: LockResourceBegin,
        limits: LockFileLimits,
        admit: impl FnOnce(&Candidate) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.pending() {
            return Err(Error::Busy);
        }
        if value.width_px == 0
            || value.height_px == 0
            || value.width_px > LOCK_FILE_MAX_PIXELS_PER_SIDE
            || value.height_px > LOCK_FILE_MAX_PIXELS_PER_SIDE
            || value.slot >= limits.upload_slots
            || value.width_px > limits.max_width_px
            || value.height_px > limits.max_height_px
            || value.total_bytes() > limits.max_resource_bytes
        {
            return Err(Error::Invalid);
        }
        // The submission lane owns complete typed validation and immutable bytes.
        admit(&Candidate::ResourceBegin(value))?;
        self.begin = Some(value);
        self.stage = Stage::Begun;
        self.base = 0;
        self.offset = 0;
        self.issued = 0;
        self.accepted = 0;
        self.cancel = false;
        self.closing = false;
        Ok(())
    }
    /// Transfer the buffer without copying. On failure the caller gets it back.
    /// Previously completed storage can be recovered with take_chunk for reuse.
    pub fn chunk(&mut self, bytes: Vec<u8>) -> Result<(), (Error, Vec<u8>)> {
        let error = if self.closed {
            Some(Error::Closed)
        } else if !self.ready() {
            Some(Error::Busy)
        } else if bytes.is_empty()
            || bytes.len() as u64 > self.begin.unwrap().total_bytes() - self.offset
        {
            Some(Error::Invalid)
        } else {
            None
        };
        if let Some(e) = error {
            return Err((e, bytes));
        }
        self.data = Some(bytes);
        self.base = self.offset;
        self.issued = 0;
        self.accepted = 0;
        self.stage = Stage::Write;
        Ok(())
    }
    /// Retain storage until every issued write settles. A terminal connection
    /// must first stop transport use; Session then calls disconnect.
    pub fn take_chunk(&mut self) -> Option<Vec<u8>> {
        if self.writes_pending() != 0
            || (!self.closed && self.stage == Stage::Write && !self.cancel && !self.closing)
        {
            None
        } else {
            self.data.take()
        }
    }
    pub fn cancel(&mut self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if !self.pending() || matches!(self.stage, Stage::Ending | Stage::Clunk) || self.closing {
            return Err(Error::Busy);
        }
        self.cancel = true;
        Ok(())
    }
    fn step(&self, end: bool) -> LockResourceStep {
        let v = self.begin.unwrap();
        LockResourceStep {
            transaction: v.transaction,
            resource: v.resource,
            total_bytes: end.then_some(self.offset),
        }
    }
    pub fn end(
        &mut self,
        admit: impl FnOnce(&Candidate) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if !self.ready() {
            return Err(Error::Busy);
        }
        if self.offset != self.begin.unwrap().total_bytes() {
            return Err(Error::Invalid);
        }
        admit(&Candidate::ResourceEnd(self.step(true)))?;
        self.stage = Stage::Ending;
        Ok(())
    }
    /// Matching resource status remains an application event. This only updates
    /// the upload lifecycle; it neither consumes the event nor grants custody.
    pub fn status(&mut self, value: LockResourceStatus) {
        if !self.pending() || self.begin.unwrap().resource != value.resource {
            return;
        }
        if value.status == LockResourceState::Admitted
            && self.stage == Stage::Begun
            && !self.closing
        {
            self.stage = Stage::Walk;
        } else if value.status != LockResourceState::Admitted {
            self.closing = true;
        }
    }
    pub fn refused(&mut self, kind: LockFileKind) {
        if (kind == LockFileKind::ResourceBegin && self.stage == Stage::Begun)
            || (matches!(
                kind,
                LockFileKind::ResourceEnd | LockFileKind::ResourceCancel
            ) && self.stage == Stage::Ending)
        {
            self.closing = true;
        }
    }
    pub fn operation(&mut self, msize: u32) -> Result<Option<Operation<'_>>, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.outstanding {
            return Ok(None);
        }
        if self.closing {
            if self.writes_pending() != 0 {
                return Ok(None);
            }
            if self.fid == Fid::NOFID {
                self.stage = Stage::Idle;
                self.closing = false;
            } else {
                self.stage = Stage::Clunk;
            }
        }
        if !self.closing && self.cancel && matches!(self.stage, Stage::Ready | Stage::Write) {
            if self.writes_pending() != 0 {
                return Ok(None);
            }
            return Ok(Some(Operation::Cancel(self.step(false))));
        }
        Ok(match self.stage {
            Stage::Walk => Some(Operation::Walk(self.begin.unwrap().slot)),
            Stage::Open => Some(Operation::Open(self.fid)),
            Stage::Clunk => Some(Operation::Clunk(self.fid)),
            Stage::Write => {
                let data = self.data.as_ref().unwrap();
                let slot = self.writes[..self.window].iter().position(Option::is_none);
                if self.issued == data.len() || slot.is_none() {
                    return Ok(None);
                }
                let mut n = msize
                    .checked_sub(23)
                    .filter(|n| *n != 0)
                    .ok_or(Error::Invalid)?;
                if self.iounit != 0 {
                    n = n.min(self.iounit);
                }
                let end = data.len().min(self.issued + n as usize);
                Some(Operation::Write {
                    slot: slot.unwrap(),
                    fid: self.fid,
                    offset: self.base + self.issued as u64,
                    bytes: &data[self.issued..end],
                })
            }
            _ => None,
        })
    }
    /// Call only after transport accepts this operation, or Submission admits
    /// the Cancel candidate. Session maps each write slot to one wire handle.
    pub fn issued(&mut self, msize: u32, walk_fid: Fid) -> Result<(), Error> {
        let op = self.operation(msize)?.ok_or(Error::Busy)?;
        if matches!(op, Operation::Walk(_)) != (walk_fid != Fid::NOFID) {
            return Err(Error::Invalid);
        }
        match op {
            Operation::Write { slot, bytes, .. } => {
                let n = bytes.len();
                self.writes[slot] = Some(n);
                self.issued += n;
            }
            Operation::Cancel(_) => self.stage = Stage::Ending,
            Operation::Walk(_) => {
                self.fid = walk_fid;
                self.outstanding = true;
            }
            _ => self.outstanding = true,
        }
        Ok(())
    }
    /// A malformed reply terminates the owning session. Remote slot errors
    /// instead drain this upload and request cancellation, preserving the session.
    pub fn write_reply(&mut self, slot: usize, reply: Reply) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let n = self
            .writes
            .get_mut(slot)
            .and_then(Option::take)
            .ok_or(Error::Invalid)?;
        match reply {
            Reply::Error(e) => {
                self.remote_error = Some(e.0);
                self.cancel = true;
            }
            Reply::Write(count) if count != 0 && count as usize <= n => {
                self.accepted += count as usize;
                self.offset += u64::from(count);
                if count as usize != n {
                    if self.window == 1 {
                        self.issued -= n - count as usize;
                    } else {
                        self.cancel = true;
                    }
                }
            }
            _ => return Err(Error::Invalid),
        }
        if self.writes_pending() == 0
            && self.accepted == self.data.as_ref().unwrap().len()
            && !self.cancel
            && !self.closing
        {
            self.stage = Stage::Ready;
        }
        Ok(())
    }
    pub fn reply(&mut self, reply: Reply) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if !self.outstanding {
            return Err(Error::Invalid);
        }
        self.outstanding = false;
        if let Reply::Error(e) = reply {
            self.remote_error = Some(e.0);
            if matches!(self.stage, Stage::Walk | Stage::Clunk) {
                self.fid = Fid::NOFID;
            }
            self.closing = true;
            return Ok(());
        }
        match (self.stage, reply) {
            (Stage::Walk, Reply::Walk(q)) if q.len() == 2 => self.stage = Stage::Open,
            (Stage::Open, Reply::Lopen { iounit, .. }) => {
                self.iounit = iounit;
                self.stage = Stage::Ready;
            }
            (Stage::Clunk, Reply::Clunk) => {
                self.fid = Fid::NOFID;
                self.stage = Stage::Idle;
                self.closing = false;
            }
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
    pub fn disconnect(&mut self) {
        self.closed = true;
        self.outstanding = false;
        self.writes = [None; MAX_WRITES];
    }
}
