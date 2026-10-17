//! Developing trusted requester session. Owns the sole transport driver;
//! submission receipts and negotiation outcomes remain caller-consumed events.
use super::{
    Custody, Journal,
    startup::Opened,
    transaction::{Operation, Transaction},
    transfer::{State as TransferState, Transfer},
};
use crate::protocol::requester::{Candidate, Event, EventRecord, TransferRequest};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, PipelineError, Reply};
use sophia_9p_records::{Fid, QidKind, Tag};
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT_ISSUER: AtomicU64 = AtomicU64::new(1);
mod upload;
#[derive(Clone, Copy, Eq, PartialEq)]
enum Command {
    Negotiate,
    Request,
    Cancel,
    Begin,
    End,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum UploadPhase {
    Begin,
    Writing,
    Ending,
    Settling,
}
struct UploadSlot {
    id: RequestId,
    writer: super::upload::Upload,
    phase: UploadPhase,
    length: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error {
    State(super::Error),
    Wire(PipelineError),
    Refused,
}
impl From<super::Error> for Error {
    fn from(e: super::Error) -> Self {
        Self::State(e)
    }
}
impl From<PipelineError> for Error {
    fn from(e: PipelineError) -> Self {
        Self::Wire(e)
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Boot {
    WalkEvents,
    OpenEvents,
    WalkAck,
    OpenAck,
    Ready,
}
/// Connection-scoped request identity; never a transferable authority token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId {
    issuer: u64,
    epoch: u64,
    submission: u64,
}
struct Tracked {
    id: RequestId,
    transfer: Transfer,
    terminal_sequence: Option<u64>,
    cancel_pending: bool,
    begun: bool,
}
pub struct Session {
    opened: Option<Opened>,
    journal: Journal,
    transaction: Transaction,
    boot: Boot,
    boot_tag: Option<Tag>,
    read_tag: Option<Tag>,
    ack_tag: Option<Tag>,
    transaction_tag: Option<Tag>,
    events: Fid,
    ack: Fid,
    events_iounit: u32,
    now: u64,
    deadline: u64,
    observed: u64,
    negotiated: Option<u64>,
    refused: Option<(u64, u64)>,
    releasing: bool,
    error: Option<Error>,
    epoch: u64,
    established: bool,
    last_submission: u64,
    pending_request: Option<usize>,
    transfers: [Option<Tracked>; 65],
    issuer: u64,
    command: Command,
    upload: Option<UploadSlot>,
    upload_tag: Option<Tag>,
}
impl Session {
    /// Consumes verified requester discovery. The first submission ID is one
    /// on this new connection; no state or operation is replayed from an epoch.
    pub fn new(opened: Opened, minimum: u16, maximum: u16, deadline: u64) -> Result<Self, Error> {
        let wire = &opened.pipeline;
        let bounds = wire.limits();
        if !wire.is_quiescent()
            || wire.fid_count() != 1
            || !wire.owns_fid(opened.root)
            || bounds.max_fids < 5
            || bounds.max_outstanding < 3
            || !(512..=65536).contains(&wire.msize())
            || bounds.max_buffered_input < wire.msize() as usize + 22
        {
            return Err(super::Error::Invalid.into());
        }
        let journal = Journal::new(
            opened.epoch,
            opened.now,
            opened.limits.ack_progress_timeout_ms,
        )?;
        let transaction = Transaction::new(
            opened.epoch,
            1,
            Candidate::Negotiate { minimum, maximum },
            opened.now,
            deadline,
            opened.limits.assembly_timeout_ms,
        )?;
        let now = opened.now;
        let epoch = opened.epoch;
        let issuer = NEXT_ISSUER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| super::Error::Overflow)?;
        let mut s = Self {
            opened: Some(opened),
            journal,
            transaction,
            boot: Boot::WalkEvents,
            boot_tag: None,
            read_tag: None,
            ack_tag: None,
            transaction_tag: None,
            events: Fid::NOFID,
            ack: Fid::NOFID,
            events_iounit: 0,
            now,
            deadline,
            observed: 0,
            negotiated: None,
            refused: None,
            releasing: false,
            error: None,
            epoch,
            established: false,
            last_submission: 1,
            pending_request: None,
            transfers: std::array::from_fn(|_| None),
            issuer,
            command: Command::Negotiate,
            upload: None,
            upload_tag: None,
        };
        s.drive()?;
        Ok(s)
    }
    pub fn error(&self) -> Option<&Error> {
        self.error.as_ref()
    }
    pub fn custody(&self) -> Custody {
        self.transaction.custody()
    }
    pub fn ready(&self) -> bool {
        self.error.is_none() && self.refused.is_none() && self.established
    }
    /// Negotiated and free to admit another transaction; retained grants and
    /// the journal may still have work. Individual operations add their checks.
    pub fn transaction_ready(&self) -> bool {
        self.ready() && self.transaction.done() && self.pending_request.is_none()
    }
    /// Copies the request into one immutable transaction. A bounded table holds
    /// 64 grants plus one pending/denied request; release acknowledged terminal
    /// results to reclaim storage. No request or grant is replayed on reconnect.
    pub fn request(
        &mut self,
        request: TransferRequest<'_>,
        now: u64,
        deadline: u64,
    ) -> Result<RequestId, Error> {
        self.advance(now)?;
        if !self.ready() || !self.transaction.done() || self.pending_request.is_some() {
            return Err(super::Error::Busy.into());
        }
        let slot = self
            .transfers
            .iter()
            .position(Option::is_none)
            .ok_or(super::Error::Busy)?;
        let opened = self.opened.as_ref().ok_or(super::Error::Closed)?;
        let transfer = Transfer::new(request, opened.limits.broker_generation)?;
        let submission = self
            .last_submission
            .checked_add(1)
            .ok_or(super::Error::Overflow)?;
        let transaction = Transaction::new(
            self.epoch,
            submission,
            Candidate::Request(request),
            now,
            deadline,
            opened.limits.assembly_timeout_ms,
        )?;
        let id = RequestId {
            issuer: self.issuer,
            epoch: self.epoch,
            submission,
        };
        self.transfers[slot] = Some(Tracked {
            id,
            transfer,
            terminal_sequence: None,
            cancel_pending: false,
            begun: false,
        });
        self.pending_request = Some(slot);
        self.last_submission = submission;
        self.transaction = transaction;
        self.command = Command::Request;
        self.releasing = false;
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        Ok(id)
    }
    /// Last known observations remain readable after disconnect; usable grants
    /// do not. A handle from another epoch cannot name this connection's state.
    pub fn transfer(&self, id: RequestId) -> Option<&Transfer> {
        self.transfers
            .iter()
            .flatten()
            .find(|t| t.id == id)
            .map(|t| &t.transfer)
    }
    /// Queues cancellation only while the grant has not entered End custody.
    /// Submitted is not Revoked; retain the grant's last observation until its
    /// correlated terminal event. A duplicate local cancellation is refused.
    pub fn cancel(&mut self, id: RequestId, now: u64, deadline: u64) -> Result<(), Error> {
        self.advance(now)?;
        if !self.ready() || !self.transaction.done() || self.pending_request.is_some() {
            return Err(super::Error::Busy.into());
        }
        let slot = self
            .transfers
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| t.id == id))
            .ok_or(super::Error::Invalid)?;
        let tracked = self.transfers[slot].as_ref().ok_or(super::Error::Invalid)?;
        if tracked.cancel_pending || tracked.transfer.state() != TransferState::Allowed {
            return Err(super::Error::Busy.into());
        }
        let submission = self
            .last_submission
            .checked_add(1)
            .ok_or(super::Error::Overflow)?;
        let timeout = self
            .opened
            .as_ref()
            .ok_or(super::Error::Closed)?
            .limits
            .assembly_timeout_ms;
        let transaction = Transaction::new(
            self.epoch,
            submission,
            Candidate::Cancel {
                transfer: tracked.transfer.id(),
            },
            now,
            deadline,
            timeout,
        )?;
        self.transfers[slot]
            .as_mut()
            .ok_or(super::Error::Invalid)?
            .cancel_pending = true;
        self.last_submission = submission;
        self.transaction = transaction;
        self.command = Command::Cancel;
        self.releasing = false;
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        Ok(())
    }
    pub fn release_transfer(&mut self, id: RequestId, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.upload.as_ref().is_some_and(|u| u.id == id) {
            return Err(super::Error::Busy.into());
        }
        let slot = self
            .transfers
            .iter()
            .position(|t| t.as_ref().is_some_and(|t| t.id == id))
            .ok_or(super::Error::Invalid)?;
        let tracked = self.transfers[slot].as_ref().ok_or(super::Error::Invalid)?;
        if tracked
            .terminal_sequence
            .is_none_or(|s| self.journal.acknowledged() < s)
        {
            return Err(super::Error::Busy.into());
        }
        self.transfers[slot] = None;
        Ok(())
    }
    pub fn event(&self) -> Option<EventRecord> {
        if self.error.is_some() {
            None
        } else {
            self.journal.event()
        }
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.opened.as_ref().map(|o| o.pipeline.as_fd())
    }
    pub fn io_interest(&self) -> Result<IoInterest, Error> {
        Ok(self
            .opened
            .as_ref()
            .ok_or(super::Error::Closed)?
            .pipeline
            .io_interest()?)
    }
    pub fn deadline(&self) -> Option<u64> {
        if self.error.is_some() {
            return None;
        }
        [
            (!self.ready()).then_some(self.deadline),
            self.journal.deadline(),
            self.transaction.deadline(),
            self.refused.map(|(_, d)| d),
            self.upload.as_ref().and_then(|u| u.writer.deadline()),
        ]
        .into_iter()
        .flatten()
        .min()
    }
    fn fail(&mut self, e: Error) -> Error {
        let e = self.error.get_or_insert(e).clone();
        self.transaction.close();
        self.journal.close();
        if let Some(upload) = &mut self.upload {
            upload.writer.close();
        }
        for tracked in self.transfers.iter_mut().flatten() {
            tracked.transfer.close();
        }
        self.opened.take();
        e
    }
    pub fn close(&mut self) {
        self.fail(super::Error::Closed.into());
    }
    fn advance(&mut self, now: u64) -> Result<(), Error> {
        if now < self.now {
            return Err(super::Error::ClockRegression.into());
        }
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        self.now = now;
        if self.deadline().is_some_and(|d| now >= d) {
            return Err(self.fail(super::Error::Deadline.into()));
        }
        self.journal.advance(now).map_err(|e| self.fail(e.into()))
    }
    /// Consuming only makes a record eligible for ack; it does not send one.
    pub fn consume(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if let Err(e) = self.journal.consume(now) {
            return Err(if e == super::Error::Busy {
                e.into()
            } else {
                self.fail(e.into())
            });
        }
        if let Err(e) = self.observe().and_then(|()| self.drive()) {
            return Err(self.fail(e));
        }
        Ok(())
    }
    pub fn acknowledge(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        let bytes = self.journal.begin_ack(now)?;
        let wire = &mut self.opened.as_mut().ok_or(super::Error::Closed)?.pipeline;
        match wire.write(self.ack, 0, &bytes) {
            Ok(tag) => {
                self.ack_tag = Some(tag);
                Ok(())
            }
            Err(e) => Err(self.fail(e.into())),
        }
    }
    pub fn retry(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        self.transaction.retry(now)?;
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        Ok(())
    }
    /// One shared byte/syscall budget, at most 32 replies, no internal wait.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), Error> {
        self.advance(now)?;
        if let Err(e) = self.drive() {
            return Err(self.fail(e));
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            let wire = &mut self.opened.as_mut().ok_or(super::Error::Closed)?.pipeline;
            if let Err(e) = wire.poll_with_budget(&mut budget) {
                return Err(self.fail(e.into()));
            }
            let Some((tag, reply)) = wire.take_reply() else {
                break;
            };
            if let Err(e) = self
                .accept(tag, reply)
                .and_then(|()| self.observe())
                .and_then(|()| self.drive())
            {
                return Err(self.fail(e));
            }
            if budget.exhausted() {
                break;
            }
        }
        Ok(())
    }
    fn observe(&mut self) -> Result<(), Error> {
        let Some(event) = self.journal.event() else {
            return Ok(());
        };
        if event.sequence == self.observed {
            return Ok(());
        }
        match event.event {
            Event::Submitted { .. } => {
                self.transaction.receipt(event, self.now)?;
                if self.command == Command::Begin {
                    let upload = self.upload.as_mut().ok_or(super::Error::Protocol)?;
                    if upload.phase == UploadPhase::Begin {
                        upload.phase = UploadPhase::Writing;
                    }
                }
            }
            Event::Negotiated | Event::Refused => {
                if self.transaction.custody() != Custody::Submitted
                    || self.negotiated.is_some()
                    || self.refused.is_some()
                {
                    return Err(super::Error::Protocol.into());
                }
                if event.event == Event::Negotiated {
                    self.negotiated = Some(event.sequence);
                } else {
                    // Local observation cannot recover the owner's publication
                    // timestamp. Never extend this bound after partial acks.
                    let timeout = self
                        .opened
                        .as_ref()
                        .ok_or(super::Error::Closed)?
                        .limits
                        .ack_progress_timeout_ms;
                    let due = self
                        .now
                        .checked_add(u64::from(timeout))
                        .ok_or(super::Error::Overflow)?;
                    self.refused = Some((event.sequence, due));
                }
            }
            Event::Denied { .. } | Event::Allowed(_) => {
                if !self.established || self.transaction.custody() != Custody::Submitted {
                    return Err(super::Error::Protocol.into());
                }
                let slot = self.pending_request.ok_or(super::Error::Protocol)?;
                if let Event::Allowed(grant) = event.event
                    && self
                        .transfers
                        .iter()
                        .flatten()
                        .any(|t| t.transfer.id() == grant.transfer && t.transfer.grant().is_some())
                {
                    return Err(super::Error::Protocol.into());
                }
                let tracked = self.transfers[slot]
                    .as_mut()
                    .ok_or(super::Error::Protocol)?;
                tracked.transfer.decision(event.event)?;
                if tracked.transfer.state() == TransferState::Denied {
                    tracked.terminal_sequence = Some(event.sequence);
                }
                self.pending_request = None;
            }
            Event::TransferOutcome { transfer, outcome } => {
                let tracked = self
                    .transfers
                    .iter_mut()
                    .flatten()
                    .find(|t| t.transfer.id() == transfer && t.transfer.grant().is_some())
                    .ok_or(super::Error::Protocol)?;
                tracked.transfer.outcome(transfer, outcome)?;
                tracked.terminal_sequence = Some(event.sequence);
                if let Some(upload) = &mut self.upload
                    && upload.id == tracked.id
                {
                    upload.writer.settle(self.now)?;
                    upload.phase = UploadPhase::Settling;
                }
            }
        }
        self.observed = event.sequence;
        Ok(())
    }
    fn drive(&mut self) -> Result<(), Error> {
        if !self.established
            && self.refused.is_none()
            && self.transaction.done()
            && self
                .negotiated
                .is_some_and(|s| self.journal.acknowledged() >= s)
        {
            self.established = true;
        }
        let opened = self.opened.as_mut().ok_or(super::Error::Closed)?;
        let wire = &mut opened.pipeline;
        if self.boot != Boot::Ready {
            if self.boot_tag.is_none() {
                self.boot_tag = Some(match self.boot {
                    Boot::WalkEvents => {
                        let (tag, fid) = wire.walk(opened.root, &[b"events"])?;
                        self.events = fid;
                        tag
                    }
                    Boot::OpenEvents => wire.lopen(self.events, 0)?,
                    Boot::WalkAck => {
                        let (tag, fid) = wire.walk(opened.root, &[b"ack"])?;
                        self.ack = fid;
                        tag
                    }
                    Boot::OpenAck => wire.lopen(self.ack, 1)?,
                    Boot::Ready => unreachable!(),
                });
            }
            return Ok(());
        }
        if self.read_tag.is_none() && self.refused.is_none() && self.journal.read_count() != 0 {
            let mut count = self.journal.read_count().min(wire.msize() - 11);
            if self.events_iounit != 0 {
                count = count.min(self.events_iounit);
            }
            self.read_tag = Some(wire.read(self.events, self.journal.offset(), count)?);
        }
        if !self.releasing && self.transaction.custody() == Custody::Submitted {
            match self
                .transaction
                .release(self.journal.acknowledged(), self.now)
            {
                Ok(()) => self.releasing = true,
                Err(super::Error::Busy) => (),
                Err(e) => return Err(e.into()),
            }
        }
        let end_submit = self.command == Command::End && self.transaction.issuing_submit();
        if self.transaction_tag.is_none()
            && let Some(op) = self.transaction.operation(self.now, wire.msize())?
        {
            let mut fid = Fid::NOFID;
            let tag = match op {
                Operation::Walk(name) => {
                    let (tag, walked) = wire.walk(opened.root, &[name])?;
                    fid = walked;
                    tag
                }
                Operation::Open { fid, flags } => wire.lopen(fid, flags)?,
                Operation::Write { fid, offset, bytes } => wire.write(fid, offset, bytes)?,
                Operation::Clunk(fid) => wire.clunk(fid)?,
            };
            self.transaction.issued(self.now, wire.msize(), fid)?;
            if end_submit {
                let id = self.upload.as_ref().ok_or(super::Error::Protocol)?.id;
                let tracked = self
                    .transfers
                    .iter_mut()
                    .flatten()
                    .find(|t| t.id == id)
                    .ok_or(super::Error::Protocol)?;
                if tracked.transfer.state() == TransferState::Allowed {
                    tracked.transfer.end_issued()?;
                }
            }
            self.transaction_tag = Some(tag);
        }
        self.drive_upload()
    }
    fn accept(&mut self, tag: Tag, reply: Reply) -> Result<(), Error> {
        if self.boot_tag == Some(tag) {
            self.boot_tag = None;
            if let Reply::Error(e) = reply {
                return Err(super::Error::Remote(e.0).into());
            }
            match (self.boot, reply) {
                (Boot::WalkEvents | Boot::WalkAck, Reply::Walk(qids))
                    if qids.len() == 1 && qids[0].kind == QidKind::File =>
                {
                    self.boot = if self.boot == Boot::WalkEvents {
                        Boot::OpenEvents
                    } else {
                        Boot::OpenAck
                    };
                }
                (Boot::OpenEvents | Boot::OpenAck, Reply::Lopen { qid, iounit })
                    if qid.kind == QidKind::File =>
                {
                    if self.boot == Boot::OpenEvents {
                        self.events_iounit = iounit;
                        self.boot = Boot::WalkAck;
                    } else {
                        if iounit != 0 && iounit < 16 {
                            return Err(super::Error::Protocol.into());
                        }
                        self.boot = Boot::Ready;
                    }
                }
                _ => return Err(super::Error::Protocol.into()),
            }
        } else if self.read_tag == Some(tag) {
            self.read_tag = None;
            match reply {
                Reply::Read(bytes) => self.journal.receive(&bytes, self.now)?,
                Reply::Error(e) => return Err(super::Error::Remote(e.0).into()),
                _ => return Err(super::Error::Protocol.into()),
            }
        } else if self.ack_tag == Some(tag) {
            self.ack_tag = None;
            let result = match reply {
                Reply::Write(n) => Ok(n),
                Reply::Error(e) => Err(e.0),
                _ => return Err(super::Error::Protocol.into()),
            };
            self.journal.ack_reply(result, self.now)?;
            if self
                .refused
                .is_some_and(|(seq, _)| self.journal.acknowledged() >= seq)
            {
                return Err(Error::Refused);
            }
        } else if self.upload_tag == Some(tag) {
            self.upload_tag = None;
            self.upload
                .as_mut()
                .ok_or(super::Error::Protocol)?
                .writer
                .reply(reply, self.now)?;
        } else if self.transaction_tag == Some(tag) {
            self.transaction_tag = None;
            self.transaction.reply(reply, self.now)?;
        } else {
            return Err(super::Error::Protocol.into());
        }
        Ok(())
    }
}
