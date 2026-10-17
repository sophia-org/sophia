//! Bounded lock file startup on an exclusively owned, versioned pipeline.
use sophia_9p_client::pipeline::{IoBudget, IoInterest, Pipeline, PipelineError, Reply};
use sophia_9p_records::{Fid, QidKind, Tag};
use sophia_lock_protocol::lock_files::*;
use std::os::fd::{AsFd, BorrowedFd};

const API: &[u8] = b"sophia-lock-files version=1 epoch=";
const NAMES: [&[u8]; 6] = [
    b"api",
    b"limits",
    b"events",
    b"transaction",
    b"submit",
    b"ack",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    Wire(PipelineError),
    Protocol(&'static str),
    Remote(u32),
    ClockRegression,
    Deadline,
    Closed,
    NotOpened,
}
impl From<PipelineError> for Error {
    fn from(value: PipelineError) -> Self {
        Self::Wire(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Attach,
    Walk,
    Open,
    Read,
    Clunk,
    Done,
}

/// Opens the six fixed files; it does not submit Negotiate. The absolute
/// deadline remains in force when ownership passes to the session driver.
pub struct Opening {
    wire: Option<Pipeline>,
    root: Fid,
    step: Step,
    index: usize,
    pending: Option<Tag>,
    fid: Fid,
    iounit: u32,
    bytes: Vec<u8>,
    fids: [Fid; 4],
    events_iounit: u32,
    transaction_iounit: u32,
    epoch: u64,
    limits: Option<LockFileLimits>,
    offer: LockNegotiate,
    now: u64,
    deadline: u64,
    error: Option<Error>,
}

/// Private-to-the-driver handoff. This root and these fids stay on the same
/// pipeline; no second attach or version negotiation is permitted.
pub struct Opened {
    pub pipeline: Pipeline,
    pub root: Fid,
    pub events: Fid,
    pub submit: Fid,
    pub ack: Fid,
    pub events_iounit: u32,
    pub transaction_iounit: u32,
    pub transaction: Fid,
    pub epoch: u64,
    pub limits: LockFileLimits,
    pub offer: LockNegotiate,
    pub deadline: u64,
    pub now: u64,
}

impl Opening {
    /// Start on a versioned pipeline with no fids. Anonymous attach is for
    /// supplied-admission exports only, never an authentication fallback.
    pub fn direct(
        wire: Pipeline,
        offer: LockNegotiate,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        Self::new(wire, None, offer, now, deadline)
    }

    /// Continue the launch client's sole admitted root. Takes exclusive
    /// ownership even on failure; no wire I/O occurs in this constructor.
    pub fn attached(
        wire: Pipeline,
        root: Fid,
        offer: LockNegotiate,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        Self::new(wire, Some(root), offer, now, deadline)
    }

    fn new(
        wire: Pipeline,
        root: Option<Fid>,
        offer: LockNegotiate,
        now: u64,
        deadline: u64,
    ) -> Result<Self, Error> {
        if deadline <= now
            || crate::Candidate::Negotiate(offer.clone())
                .prepare(1, 1)
                .is_err()
        {
            return Err(Error::Protocol(
                "invalid deadline or lock negotiation offer",
            ));
        }
        if !wire.is_quiescent() || wire.fid_count() != usize::from(root.is_some()) {
            return Err(Error::Protocol("startup requires a quiescent pipeline"));
        }
        if root.is_some_and(|fid| fid == Fid::NOFID || !wire.owns_fid(fid)) {
            return Err(Error::Protocol("invalid root"));
        }
        let bounds = wire.limits();
        if !(4096..=65536).contains(&wire.msize())
            || bounds.max_outstanding < 6
            || bounds.max_fids < 16
            || bounds.max_buffered_input < 2 * wire.msize() as usize + 64
        {
            return Err(Error::Protocol("insufficient lock pipeline capacity"));
        }
        let mut value = Self {
            wire: Some(wire),
            root: root.unwrap_or(Fid::NOFID),
            step: if root.is_some() {
                Step::Walk
            } else {
                Step::Attach
            },
            index: 0,
            pending: None,
            fid: Fid::NOFID,
            iounit: 0,
            bytes: Vec::with_capacity(128),
            fids: [Fid::NOFID; 4],
            events_iounit: 0,
            transaction_iounit: 0,
            epoch: 0,
            limits: None,
            offer,
            now,
            deadline,
            error: None,
        };
        value.drive()?;
        Ok(value)
    }

    pub fn is_opened(&self) -> bool {
        self.step == Step::Done && self.error.is_none()
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    pub fn deadline(&self) -> u64 {
        self.deadline
    }
    pub fn epoch(&self) -> Option<u64> {
        self.limits.map(|_| self.epoch)
    }
    pub fn limits(&self) -> Option<LockFileLimits> {
        self.limits
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.wire.as_ref().map(AsFd::as_fd)
    }
    pub fn io_interest(&self) -> Result<IoInterest, Error> {
        self.wire
            .as_ref()
            .ok_or(Error::Closed)?
            .io_interest()
            .map_err(Into::into)
    }

    /// At most 32 rounds, sharing one byte/syscall allowance across all rounds.
    /// The caller supplies monotonic milliseconds. An expired deadline never
    /// sends queued requests. Clock regression leaves the connection untouched.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::ClockRegression);
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.now = now;
        if now >= self.deadline {
            return self.fail(Error::Deadline);
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            if self.is_opened() {
                break;
            }
            let wire = self.wire.as_mut().ok_or(Error::Closed)?;
            if let Err(error) = wire.poll_with_budget(&mut budget) {
                return self.fail(error.into());
            }
            let Some((tag, reply)) = wire.take_reply() else {
                break;
            };
            if self.pending.take() != Some(tag) {
                return self.fail(Error::Protocol("unowned startup reply"));
            }
            if let Err(error) = self.reply(reply).and_then(|()| self.drive()) {
                return self.fail(error);
            }
            if budget.exhausted() {
                break;
            }
        }
        Ok(())
    }

    pub fn close(&mut self) {
        let _ = self.fail(Error::Closed);
    }

    fn fail(&mut self, error: Error) -> Result<(), Error> {
        let error = self.error.get_or_insert(error).clone();
        self.wire.take();
        Err(error)
    }

    /// Consumes startup. Calling before completion closes its owned pipeline.
    pub fn into_opened(mut self) -> Result<Opened, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if !self.is_opened() {
            return Err(Error::NotOpened);
        }
        Ok(Opened {
            pipeline: self.wire.take().ok_or(Error::Closed)?,
            root: self.root,
            events: self.fids[0],
            transaction: self.fids[1],
            transaction_iounit: self.transaction_iounit,
            submit: self.fids[2],
            ack: self.fids[3],
            events_iounit: self.events_iounit,
            epoch: self.epoch,
            limits: self.limits.ok_or(Error::Protocol("missing limits"))?,
            offer: self.offer,
            deadline: self.deadline,
            now: self.now,
        })
    }

    fn drive(&mut self) -> Result<(), Error> {
        if self.pending.is_some() || self.step == Step::Done {
            return Ok(());
        }
        let wire = self.wire.as_mut().ok_or(Error::Closed)?;
        self.pending = Some(match self.step {
            Step::Attach => {
                let (tag, root) = wire.attach(b"", b"")?;
                self.root = root;
                tag
            }
            Step::Walk => {
                let (tag, fid) = wire.walk(self.root, &[NAMES[self.index]])?;
                self.fid = fid;
                tag
            }
            Step::Open => wire.lopen(
                self.fid,
                match self.index {
                    3 => 2,
                    4 | 5 => 1,
                    _ => 0,
                },
            )?,
            Step::Read => {
                let maximum = if self.index == 0 {
                    128
                } else {
                    32 + LockFileLimits::BYTES
                };
                let mut count = (maximum + 1 - self.bytes.len()) as u32;
                if self.iounit != 0 {
                    count = count.min(self.iounit);
                }
                wire.read(self.fid, self.bytes.len() as u64, count)?
            }
            Step::Clunk => wire.clunk(self.fid)?,
            Step::Done => return Ok(()),
        });
        Ok(())
    }

    fn next_file(&mut self) {
        self.index += 1;
        self.bytes.clear();
        self.step = if self.index == NAMES.len() {
            Step::Done
        } else {
            Step::Walk
        };
    }

    fn reply(&mut self, reply: Reply) -> Result<(), Error> {
        if let Reply::Error(errno) = reply {
            return Err(Error::Remote(errno.0));
        }
        match (self.step, reply) {
            (Step::Attach, Reply::Attach(_)) => self.step = Step::Walk,
            (Step::Walk, Reply::Walk(qids)) if qids.len() == 1 && qids[0].kind == QidKind::File => {
                self.step = Step::Open;
            }
            (Step::Open, Reply::Lopen { qid, iounit }) if qid.kind == QidKind::File => {
                self.iounit = iounit;
                if self.index < 2 {
                    self.step = Step::Read;
                } else {
                    let minimum = match self.index {
                        4 => 24,
                        5 => 16,
                        _ => 0,
                    };
                    if iounit != 0 && iounit < minimum {
                        return Err(Error::Protocol("control iounit too small"));
                    }
                    self.fids[self.index - 2] = self.fid;
                    if self.index == 2 {
                        self.events_iounit = iounit;
                    }
                    if self.index == 3 {
                        self.transaction_iounit = iounit;
                    }
                    self.next_file();
                }
            }
            (Step::Read, Reply::Read(data)) => {
                let maximum = if self.index == 0 {
                    128
                } else {
                    32 + LockFileLimits::BYTES
                };
                if data.len() > maximum - self.bytes.len() {
                    return Err(Error::Protocol("startup object too large"));
                }
                if data.is_empty() {
                    self.validate_object()?;
                    self.step = Step::Clunk;
                } else {
                    self.bytes.extend_from_slice(&data);
                }
            }
            (Step::Clunk, Reply::Clunk) => self.next_file(),
            _ => return Err(Error::Protocol("unexpected startup reply")),
        }
        Ok(())
    }

    fn validate_object(&mut self) -> Result<(), Error> {
        if self.index == 0 {
            let digits = self
                .bytes
                .strip_prefix(API)
                .and_then(|tail| tail.strip_suffix(b"\n"))
                .filter(|digits| !digits.is_empty() && (b'1'..=b'9').contains(&digits[0]))
                .ok_or(Error::Protocol("lock API mismatch"))?;
            self.epoch = digits
                .iter()
                .try_fold(0u64, |n, byte| {
                    if !byte.is_ascii_digit() {
                        return None;
                    }
                    n.checked_mul(10)?.checked_add(u64::from(byte - b'0'))
                })
                .ok_or(Error::Protocol("invalid API epoch"))?;
            return Ok(());
        }
        let record = decode_lock_file_record(&self.bytes, LockFileClass::Object)
            .map_err(|_| Error::Protocol("invalid Limits envelope"))?;
        if record.header.kind != LockFileKind::Limits
            || record.header.connection_epoch != self.epoch
        {
            return Err(Error::Protocol("expected Limits object"));
        }
        let limits = LockFileLimits::decode(record.body)
            .map_err(|_| Error::Protocol("invalid Limits payload"))?;
        self.limits = Some(limits);
        Ok(())
    }
}
