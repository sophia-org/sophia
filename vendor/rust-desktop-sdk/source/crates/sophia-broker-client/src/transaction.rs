//! One staged submission, driven beside the journal by the session's sole I/O
//! owner. No socket polling, automatic retries or connection ownership here.
use crate::protocol::EventRecord;
use crate::{Candidate, Custody, Error, Submission};
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Fid, QidKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stage {
    WalkTransaction,
    OpenTransaction,
    Staging,
    WalkSubmit,
    OpenSubmit,
    Submit,
    Wait,
    ClunkSubmit,
    ClunkTransaction,
    Done,
}
pub enum Operation<'a> {
    Walk(&'static [u8]),
    Open {
        fid: Fid,
        flags: u32,
    },
    Write {
        fid: Fid,
        offset: u64,
        bytes: &'a [u8],
    },
    Clunk(Fid),
}
pub struct Transaction {
    submission: Submission,
    stage: Stage,
    transaction: Fid,
    submit: Fid,
    retain_submit: bool,
    outstanding: bool,
    offset: usize,
    sent: usize,
    iounit: u32,
    now: u64,
    deadline: u64,
    assembly_timeout: u64,
    assembly_due: Option<u64>,
    error: Option<Error>,
}
impl Transaction {
    pub fn new(
        epoch: u64,
        id: u64,
        candidate: Candidate<'_>,
        now: u64,
        deadline: u64,
        assembly_timeout_ms: u32,
    ) -> Result<Self, Error> {
        if !(1..=12000).contains(&assembly_timeout_ms) {
            return Err(Error::Invalid);
        }
        Ok(Self {
            submission: Submission::new(epoch, id, candidate, now, deadline)?,
            stage: Stage::WalkTransaction,
            transaction: Fid::NOFID,
            submit: Fid::NOFID,
            retain_submit: false,
            outstanding: false,
            offset: 0,
            sent: 0,
            iounit: 0,
            now,
            deadline,
            assembly_timeout: u64::from(assembly_timeout_ms),
            assembly_due: None,
            error: None,
        })
    }
    fn fail(&mut self, e: Error) -> Error {
        let e = *self.error.get_or_insert(e);
        self.submission.close();
        e
    }
    // Only Session uses this mode. The submit handle belongs to that connection,
    // while each transaction still owns and clunks its assembly handle.
    pub(crate) fn retain_submit(&mut self, previous: Option<&Self>) {
        debug_assert!(self.stage == Stage::WalkTransaction && !self.outstanding);
        self.retain_submit = true;
        if let Some(previous) = previous {
            debug_assert!(previous.done() && previous.retain_submit);
            self.submit = previous.submit;
        }
    }
    pub fn close(&mut self) {
        self.fail(Error::Closed);
    }
    pub fn error(&self) -> Option<Error> {
        self.error
    }
    pub fn custody(&self) -> Custody {
        self.submission.custody()
    }
    pub fn done(&self) -> bool {
        self.error.is_none() && self.stage == Stage::Done
    }
    pub fn deadline(&self) -> Option<u64> {
        if self.error.is_some() || self.done() {
            return None;
        }
        Some(
            self.assembly_due
                .map_or(self.deadline, |d| d.min(self.deadline)),
        )
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
        if let Err(e) = self.submission.advance(now) {
            return Err(self.fail(e));
        }
        if self.assembly_due.is_some_and(|d| now >= d) {
            return Err(self.fail(Error::Deadline));
        }
        Ok(())
    }
    pub fn operation(&mut self, now: u64, msize: u32) -> Result<Option<Operation<'_>>, Error> {
        self.advance(now)?;
        if self.outstanding || matches!(self.stage, Stage::Wait | Stage::Done) {
            return Ok(None);
        }
        if msize < 512 {
            return Err(self.fail(Error::Invalid));
        }
        Ok(Some(match self.stage {
            Stage::WalkTransaction => Operation::Walk(b"transaction"),
            Stage::OpenTransaction => Operation::Open {
                fid: self.transaction,
                flags: 2,
            },
            Stage::Staging => {
                let mut count =
                    (self.submission.bytes().len() - self.offset).min((msize - 23) as usize);
                if self.iounit != 0 {
                    count = count.min(self.iounit as usize);
                }
                Operation::Write {
                    fid: self.transaction,
                    offset: self.offset as u64,
                    bytes: &self.submission.bytes()[self.offset..self.offset + count],
                }
            }
            Stage::WalkSubmit => Operation::Walk(b"submit"),
            Stage::OpenSubmit => Operation::Open {
                fid: self.submit,
                flags: 1,
            },
            Stage::Submit => Operation::Write {
                fid: self.submit,
                offset: 0,
                bytes: self.submission.control(),
            },
            Stage::ClunkSubmit => Operation::Clunk(self.submit),
            Stage::ClunkTransaction => Operation::Clunk(self.transaction),
            _ => unreachable!(),
        }))
    }
    /// Called only after the pipeline admitted operation(). Walk supplies its
    /// allocated fid; all other operations supply NOFID. Driver owns the tag.
    pub fn issued(&mut self, now: u64, msize: u32, fid: Fid) -> Result<(), Error> {
        let Some(op) = self.operation(now, msize)? else {
            return Err(Error::Busy);
        };
        if matches!(op, Operation::Walk(_)) != (fid != Fid::NOFID) {
            return Err(Error::Invalid);
        }
        self.sent = match op {
            Operation::Write { bytes, .. } => bytes.len(),
            _ => 0,
        };
        match self.stage {
            Stage::WalkTransaction => self.transaction = fid,
            Stage::WalkSubmit => self.submit = fid,
            Stage::Staging if self.assembly_due.is_none() => {
                self.assembly_due = Some(
                    now.checked_add(self.assembly_timeout)
                        .ok_or_else(|| self.fail(Error::Overflow))?,
                );
            }
            Stage::Submit => self.submission.issued(now)?,
            _ => (),
        }
        self.outstanding = true;
        Ok(())
    }
    pub fn receipt(&mut self, event: EventRecord<'_>, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if let Err(e) = self.submission.receipt(event, now) {
            return Err(self.fail(e));
        }
        self.assembly_due = None;
        Ok(())
    }
    /// An EAGAIN reply leaves this operation waiting. Retry is an explicit
    /// caller decision and preserves the original candidate, ID and deadline.
    pub fn retry(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.stage != Stage::Wait || self.outstanding || self.custody() != Custody::Queued {
            return Err(Error::Busy);
        }
        self.stage = Stage::Submit;
        Ok(())
    }
    pub fn release(&mut self, acknowledged: u64, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.stage != Stage::Wait
            || self.outstanding
            || !self.submission.releasable(acknowledged)
        {
            return Err(Error::Busy);
        }
        self.stage = if self.retain_submit {
            Stage::ClunkTransaction
        } else {
            Stage::ClunkSubmit
        };
        Ok(())
    }
    pub fn reply(&mut self, reply: Reply, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if !self.outstanding {
            return Err(Error::Busy);
        }
        self.outstanding = false;
        let result = self.accept(reply, now);
        result.map_err(|e| self.fail(e))
    }
    fn accept(&mut self, reply: Reply, now: u64) -> Result<(), Error> {
        if self.stage == Stage::Submit {
            let result = match reply {
                Reply::Write(n) => Ok(n),
                Reply::Error(e) => Err(e.0),
                _ => return Err(Error::Protocol),
            };
            self.submission.submit_reply(result, now)?;
            if self.custody() != Custody::Queued {
                self.assembly_due = None;
            }
            self.stage = Stage::Wait;
            return Ok(());
        }
        if let Reply::Error(e) = reply {
            return Err(Error::Remote(e.0));
        }
        match (self.stage, reply) {
            (Stage::WalkTransaction | Stage::WalkSubmit, Reply::Walk(qids))
                if qids.len() == 1 && qids[0].kind == QidKind::File =>
            {
                self.stage = if self.stage == Stage::WalkTransaction {
                    Stage::OpenTransaction
                } else {
                    Stage::OpenSubmit
                };
            }
            (Stage::OpenTransaction | Stage::OpenSubmit, Reply::Lopen { qid, iounit })
                if qid.kind == QidKind::File =>
            {
                if self.stage == Stage::OpenSubmit && iounit != 0 && iounit < 24 {
                    return Err(Error::Protocol);
                }
                self.iounit = iounit;
                self.stage = if self.stage == Stage::OpenTransaction {
                    Stage::Staging
                } else {
                    Stage::Submit
                };
            }
            (Stage::Staging, Reply::Write(n)) if n as usize == self.sent => {
                self.offset += self.sent;
                if self.offset == self.submission.bytes().len() {
                    self.stage = if self.submit == Fid::NOFID {
                        Stage::WalkSubmit
                    } else {
                        Stage::Submit
                    };
                }
            }
            (Stage::ClunkSubmit, Reply::Clunk) => self.stage = Stage::ClunkTransaction,
            (Stage::ClunkTransaction, Reply::Clunk) => self.stage = Stage::Done,
            _ => return Err(Error::Protocol),
        }
        Ok(())
    }
}
