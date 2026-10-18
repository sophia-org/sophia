//! One bounded immutable submission; explicit retry and transaction recycling.
use crate::{Candidate, Prepared};
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Fid, QidKind};
use sophia_lock_protocol::{BinaryCodecError, lock_files::*};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Custody {
    Staged,
    Issued,
    Submitted,
    Refused(u32),
    UnknownDisconnected,
    DroppedUnsent,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Busy,
    Invalid,
    Closed,
    Remote(u32),
    Encode(BinaryCodecError),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    Idle,
    Write,
    Submit,
    Wait,
    Clunk,
    Walk,
    Open,
}
#[derive(Debug, PartialEq)]
pub enum Operation<'a> {
    Write {
        fid: Fid,
        offset: u64,
        bytes: &'a [u8],
    },
    Submit(&'a [u8; 24]),
    Clunk(Fid),
    Walk,
    Open(Fid),
}

pub struct Submission {
    epoch: u64,
    next: u64,
    fid: Fid,
    iounit: u32,
    stage: Stage,
    outstanding: bool,
    closed: bool,
    value: Option<Prepared>,
    latest: Option<(u64, Custody)>,
    written: usize,
    sent: usize,
    control: [u8; 24],
    submitted_sequence: u64,
    replied: bool,
    retry_wait: bool,
}
impl Submission {
    pub fn new(epoch: u64, fid: Fid, iounit: u32) -> Result<Self, Error> {
        if epoch == 0 || fid == Fid::NOFID {
            return Err(Error::Invalid);
        }
        Ok(Self {
            epoch,
            next: 1,
            fid,
            iounit,
            stage: Stage::Idle,
            outstanding: false,
            closed: false,
            value: None,
            latest: None,
            written: 0,
            sent: 0,
            control: [0; 24],
            submitted_sequence: 0,
            replied: false,
            retry_wait: false,
        })
    }
    pub fn latest(&self) -> Option<(u64, Custody)> {
        self.latest
    }

    pub fn idle(&self) -> bool {
        self.stage == Stage::Idle
    }
    pub fn waiting_for_retry(&self) -> bool {
        self.retry_wait
    }
    pub fn waiting_for_ack(&self, acked: u64) -> bool {
        self.stage == Stage::Open && acked < self.submitted_sequence
    }
    /// Internal admission: the session restricts public kinds and owns image sequencing.
    pub fn admit(&mut self, candidate: &Candidate) -> Result<u64, Error> {
        let value = self.prepare(candidate)?;
        Ok(self.admit_prepared(value))
    }
    pub(crate) fn prepare(&self, candidate: &Candidate) -> Result<Prepared, Error> {
        self.can_admit()?;
        candidate
            .prepare(self.epoch, self.next)
            .map_err(Error::Encode)
    }
    pub(crate) fn can_admit(&self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if !self.idle() {
            return Err(Error::Busy);
        }
        if self.next == u64::MAX {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    pub(crate) fn admit_image(&mut self, body: &[u8; 96]) -> Result<u64, Error> {
        self.can_admit()?;
        let value = Prepared::image(self.epoch, self.next, body).map_err(Error::Encode)?;
        Ok(self.admit_prepared(value))
    }
    // The session prepares and commits without another submission mutation.
    pub(crate) fn admit_prepared(&mut self, value: Prepared) -> u64 {
        let id = self.next;
        self.control = [0; 24];
        self.control[..8].copy_from_slice(&self.epoch.to_le_bytes());
        self.control[8..16].copy_from_slice(&id.to_le_bytes());
        self.control[16..20].copy_from_slice(&(value.bytes().len() as u32).to_le_bytes());
        self.value = Some(value);
        self.next += 1;
        self.latest = Some((id, Custody::Staged));
        self.written = 0;
        self.stage = Stage::Write;
        self.replied = false;
        self.retry_wait = false;
        self.submitted_sequence = 0;
        id
    }
    pub fn retry(&mut self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if !self.retry_wait || self.stage != Stage::Submit {
            return Err(Error::Invalid);
        }
        self.retry_wait = false;
        Ok(())
    }
    pub fn operation(&self, acked: u64, msize: u32) -> Result<Option<Operation<'_>>, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.outstanding || self.retry_wait {
            return Ok(None);
        }
        Ok(match self.stage {
            Stage::Idle | Stage::Wait => None,
            Stage::Write => {
                let mut count = msize
                    .checked_sub(23)
                    .filter(|n| *n != 0)
                    .ok_or(Error::Invalid)?;
                if self.iounit != 0 {
                    count = count.min(self.iounit);
                }
                let bytes = self.value.as_ref().unwrap().bytes();
                let end = bytes.len().min(self.written + count as usize);
                Some(Operation::Write {
                    fid: self.fid,
                    offset: self.written as u64,
                    bytes: &bytes[self.written..end],
                })
            }
            Stage::Submit => Some(Operation::Submit(&self.control)),
            Stage::Clunk => Some(Operation::Clunk(self.fid)),
            Stage::Walk => Some(Operation::Walk),
            Stage::Open if acked < self.submitted_sequence => None,
            Stage::Open => Some(Operation::Open(self.fid)),
        })
    }
    pub fn issued(&mut self, acked: u64, msize: u32, walk_fid: Fid) -> Result<(), Error> {
        let op = self.operation(acked, msize)?.ok_or(Error::Busy)?;
        if matches!(op, Operation::Walk) != (walk_fid != Fid::NOFID) {
            return Err(Error::Invalid);
        }
        self.sent = match op {
            Operation::Write { bytes, .. } => bytes.len(),
            _ => 0,
        };
        if self.stage == Stage::Walk {
            self.fid = walk_fid;
        }
        if self.stage == Stage::Submit {
            self.latest.as_mut().unwrap().1 = Custody::Issued;
        }
        self.outstanding = true;
        Ok(())
    }
    /// Validate this before interpreting submit replies from the same I/O batch.
    pub fn submitted(&mut self, sequence: u64, value: LockSubmitted) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if sequence == 0
            || self.submitted_sequence != 0
            || self.latest != Some((value.submission_id, Custody::Issued))
            || self.value.as_ref().map(Prepared::kind) != Some(value.candidate_kind)
        {
            return Err(Error::Invalid);
        }
        self.latest.as_mut().unwrap().1 = Custody::Submitted;
        self.submitted_sequence = sequence;
        if self.replied {
            self.stage = Stage::Clunk;
        }
        Ok(())
    }
    pub fn reply(&mut self, reply: Reply, negotiated: bool) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if !self.outstanding {
            return Err(Error::Invalid);
        }
        self.outstanding = false;
        let submitted = self.latest.is_some_and(|(_, c)| c == Custody::Submitted);
        if let Reply::Error(errno) = reply {
            if self.stage == Stage::Submit {
                if errno.0 == 11 && !submitted {
                    self.retry_wait = true;
                    self.latest.as_mut().unwrap().1 = Custody::Staged;
                    return Ok(());
                }
                if errno.0 == 114 && submitted {
                    self.replied = true;
                    self.stage = Stage::Clunk;
                    return Ok(());
                }
                if !matches!(errno.0, 11 | 114 | 116) && negotiated && !submitted {
                    self.latest.as_mut().unwrap().1 = Custody::Refused(errno.0);
                    self.stage = Stage::Clunk;
                    return Ok(());
                }
            }
            return Err(Error::Remote(errno.0));
        }
        match (self.stage, reply) {
            (Stage::Write, Reply::Write(n)) if n != 0 && n as usize <= self.sent => {
                self.written += n as usize;
                if self.written == self.value.as_ref().unwrap().bytes().len() {
                    self.stage = Stage::Submit;
                }
            }
            (Stage::Submit, Reply::Write(24)) => {
                self.replied = true;
                self.stage = if submitted { Stage::Clunk } else { Stage::Wait };
            }
            (Stage::Clunk, Reply::Clunk) => self.stage = Stage::Walk,
            (Stage::Walk, Reply::Walk(q)) if q.len() == 1 && q[0].kind == QidKind::File => {
                self.stage = Stage::Open
            }
            (Stage::Open, Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                self.iounit = iounit;
                self.stage = Stage::Idle;
                self.value = None;
            }
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
    pub fn disconnect(&mut self) {
        if let Some((_, custody)) = &mut self.latest {
            *custody = match *custody {
                Custody::Staged => Custody::DroppedUnsent,
                Custody::Issued => Custody::UnknownDisconnected,
                other => other,
            };
        }
        self.value = None;
        self.closed = true;
    }
}
