//! Fetch one publication's immutable topology. The session owns transport and
//! ends the connection on any error; no partially verified value is exposed.
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Fid, QidKind};
use sophia_output_protocol::{OutputV1Snapshot, output_files::*};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Busy,
    Deadline,
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
    publication: OutputFilePublication,
    stage: Stage,
    outstanding: bool,
    fid: Fid,
    iounit: u32,
    sent: usize,
    bytes: Vec<u8>,
    value: Option<OutputV1Snapshot>,
    deadline: u64,
    retry_at: Option<u64>,
    retry_delay: u64,
    retry_walk: bool,
}
impl Fetch {
    pub fn new(
        epoch: u64,
        publication: OutputFilePublication,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if epoch == 0 || publication.qid_path == 0 || publication.topology_epoch == 0 {
            return Err(Error::Invalid);
        }
        if deadline <= now {
            return Err(Error::Deadline);
        }
        Ok(Self {
            epoch,
            publication,
            stage: Stage::Walk,
            outstanding: false,
            fid: Fid::NOFID,
            iounit: 0,
            sent: 0,
            bytes: vec![],
            value: None,
            deadline,
            retry_at: None,
            retry_delay: 0,
            retry_walk: false,
        })
    }
    pub fn deadline(&self) -> Option<u64> {
        (self.stage != Stage::Ready).then_some(self.deadline)
    }
    pub fn retry_at(&self) -> Option<u64> {
        self.retry_at
    }
    pub fn result(&self) -> Option<&OutputV1Snapshot> {
        if self.stage == Stage::Ready {
            self.value.as_ref()
        } else {
            None
        }
    }
    pub fn is_ready(&self) -> bool {
        self.stage == Stage::Ready
    }
    /// Transfer the verified object without copying its head, mode and group
    /// allocations. Readiness remains true after ownership passes to Session.
    pub fn take_result(&mut self) -> Option<OutputV1Snapshot> {
        if self.is_ready() {
            self.value.take()
        } else {
            None
        }
    }
    pub fn operation(&self, now: u64, msize: u32) -> Result<Option<Operation>, Error> {
        if self.stage == Stage::Ready {
            return Ok(None);
        }
        if now >= self.deadline {
            return Err(Error::Deadline);
        }
        if self.outstanding || self.retry_at.is_some_and(|at| now < at) {
            return Ok(None);
        }
        Ok(Some(match self.stage {
            Stage::Walk => Operation::Walk,
            Stage::Open => Operation::Open(self.fid),
            Stage::Read => {
                let mut n = (OUTPUT_FILE_MAX_TOPOLOGY_BYTES - self.bytes.len())
                    .max(1)
                    .min(
                        msize
                            .checked_sub(11)
                            .filter(|n| *n != 0)
                            .ok_or(Error::Invalid)? as usize,
                    ) as u32;
                if self.iounit != 0 {
                    n = n.min(self.iounit);
                }
                Operation::Read {
                    fid: self.fid,
                    offset: self.bytes.len() as u64,
                    count: n,
                }
            }
            Stage::Clunk => Operation::Clunk(self.fid),
            Stage::Ready => unreachable!(),
        }))
    }
    /// Call after the pipeline accepts the request; only Walk supplies a fid.
    pub fn issued(&mut self, now: u64, msize: u32, fid: Fid) -> Result<(), Error> {
        let op = self.operation(now, msize)?.ok_or(Error::Busy)?;
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
    pub fn reply(&mut self, reply: Reply, now: u64) -> Result<(), Error> {
        if !self.outstanding {
            return Err(Error::Invalid);
        }
        if now >= self.deadline {
            return Err(Error::Deadline);
        }
        self.outstanding = false;
        if let Reply::Error(errno) = reply {
            if errno.0 == 11 && matches!(self.stage, Stage::Walk | Stage::Open) {
                if self.stage == Stage::Open {
                    self.stage = Stage::Clunk;
                    self.retry_walk = true;
                }
                self.retry_delay = if self.retry_delay == 0 {
                    4
                } else {
                    (self.retry_delay * 2).min(256)
                };
                self.retry_at = Some(now.saturating_add(self.retry_delay));
                return Ok(());
            }
            return Err(Error::Remote(errno.0));
        }
        match (self.stage, reply) {
            (Stage::Walk, Reply::Walk(qids))
                if qids.len() == 1
                    && qids[0].kind == QidKind::File
                    && qids[0].path == self.publication.qid_path =>
            {
                self.stage = Stage::Open
            }
            (Stage::Open, Reply::Lopen { qid, iounit })
                if qid.kind == QidKind::File && qid.path == self.publication.qid_path =>
            {
                self.iounit = iounit;
                self.retry_at = None;
                self.stage = Stage::Read;
            }
            (Stage::Read, Reply::Read(data)) => {
                if data.len() > self.sent
                    || data.len() > OUTPUT_FILE_MAX_TOPOLOGY_BYTES - self.bytes.len()
                {
                    return Err(Error::Invalid);
                }
                if data.is_empty() {
                    let record = decode_output_file_record(&self.bytes, OutputFileClass::Object)
                        .map_err(|_| Error::Invalid)?;
                    if record.header.kind != OutputFileKind::Topology
                        || record.header.connection_epoch != self.epoch
                    {
                        return Err(Error::Invalid);
                    }
                    let value = decode_output_file_topology(record.body, self.epoch)
                        .map_err(|_| Error::Invalid)?;
                    if value.snapshot.topology_epoch != self.publication.topology_epoch {
                        return Err(Error::Invalid);
                    }
                    self.value = Some(value);
                    self.stage = Stage::Clunk;
                } else {
                    self.bytes.extend_from_slice(&data);
                    if self.bytes.len() >= 4 {
                        let n = u32::from_le_bytes(self.bytes[..4].try_into().unwrap()) as usize;
                        if !(OUTPUT_FILE_HEADER_BYTES..=OUTPUT_FILE_MAX_TOPOLOGY_BYTES).contains(&n)
                            || self.bytes.len() > n
                        {
                            return Err(Error::Invalid);
                        }
                    }
                }
            }
            (Stage::Clunk, Reply::Clunk) => {
                if self.retry_walk {
                    self.retry_walk = false;
                    self.stage = Stage::Walk;
                } else {
                    self.stage = Stage::Ready;
                    self.bytes.clear();
                }
            }
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
}
