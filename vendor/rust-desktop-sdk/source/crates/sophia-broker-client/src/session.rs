//! Developing admitted broker session. Owns the sole transport driver;
//! submission receipts and negotiation outcomes remain caller-consumed events.
use crate::protocol::{Event, EventRecord, Request, ResponseRow};
use crate::{
    Candidate, Custody, Journal,
    phase::{Ordering, Phase},
    startup::Opened,
    transaction::{Operation, Transaction},
};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, PipelineError, Reply};
use sophia_9p_records::{Fid, QidKind, Tag};
use std::os::fd::{AsFd, BorrowedFd};
struct OwnedRequest {
    value: Request<'static>,
    label: [u8; 128],
    length: Option<usize>,
    due: u64,
}
impl OwnedRequest {
    fn new(request: Request<'_>, due: u64) -> Self {
        let mut label = [0; 128];
        let length = request.label.map(|text| {
            label[..text.len()].copy_from_slice(text.as_bytes());
            text.len()
        });
        Self {
            value: Request {
                label: None,
                ..request
            },
            label,
            length,
            due,
        }
    }
    fn view(&self) -> Request<'_> {
        Request {
            label: self
                .length
                .map(|n| std::str::from_utf8(&self.label[..n]).unwrap()),
            ..self.value
        }
    }
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
    ordering: Ordering,
    request: Option<OwnedRequest>,
}
impl Session {
    /// Consumes verified broker discovery. The first submission ID is one
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
        let mut transaction = Transaction::new(
            opened.epoch,
            1,
            Candidate::Negotiate { minimum, maximum },
            opened.now,
            deadline,
            opened.limits.assembly_timeout_ms,
        )?;
        transaction.retain_submit(None);
        let now = opened.now;
        let epoch = opened.epoch;
        let ordering = Ordering::new(epoch, opened.limits, 1, now)?;
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
            ordering,
            request: None,
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
    /// Negotiated and finished with the prior transaction's acknowledged cleanup.
    pub fn transaction_ready(&self) -> bool {
        self.ready() && self.transaction.done()
    }
    /// True only after a retryable submit refusal, never merely local queuing.
    pub fn retry_ready(&self) -> bool {
        self.error.is_none() && self.transaction.retry_ready()
    }
    pub fn request(&self) -> Option<Request<'_>> {
        if self.error.is_some() || self.ordering.phase() != Phase::Ready {
            return None;
        }
        self.request.as_ref().map(OwnedRequest::view)
    }
    /// Encodes borrowed rows once into fixed transaction storage. The pending
    /// request is retained independently of journal consumption.
    pub fn respond(
        &mut self,
        request_id: u64,
        rows: &[ResponseRow<'_>],
        now: u64,
    ) -> Result<(), Error> {
        self.advance(now)?;
        if !self.ready() || !self.transaction.done() || self.ordering.phase() != Phase::Ready {
            return Err(super::Error::Busy.into());
        }
        let pending = self.request.as_ref().ok_or(super::Error::Busy)?;
        if pending.value.id != request_id {
            return Err(super::Error::Invalid.into());
        }
        let id = self
            .last_submission
            .checked_add(1)
            .ok_or(super::Error::Overflow)?;
        let request = pending.view();
        let opened = self.opened.as_ref().ok_or(super::Error::Closed)?;
        let mut transaction = Transaction::new(
            self.epoch,
            id,
            Candidate::Response {
                request: &request,
                rows,
            },
            now,
            pending.due,
            opened.limits.assembly_timeout_ms,
        )?;
        transaction.retain_submit(Some(&self.transaction));
        self.ordering.response(request_id, id, now)?;
        self.transaction = transaction;
        self.last_submission = id;
        self.releasing = false;
        self.drive().map_err(|e| self.fail(e))
    }
    pub fn event(&self) -> Option<EventRecord<'_>> {
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
            self.request.as_ref().map(|r| r.due),
        ]
        .into_iter()
        .flatten()
        .min()
    }
    fn fail(&mut self, e: Error) -> Error {
        let e = self.error.get_or_insert(e).clone();
        self.transaction.close();
        self.journal.close();
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
        if matches!(event.event, Event::Submitted { .. }) {
            self.transaction.receipt(event, self.now)?;
        }
        self.ordering.event(event, self.now)?;
        match event.event {
            Event::Submitted {
                candidate_kind: 257,
                ..
            } => self.request = None,
            Event::Negotiated { .. } => self.negotiated = Some(event.sequence),
            Event::Refused => {
                self.refused = Some((
                    event.sequence,
                    self.ordering
                        .refusal_deadline()
                        .ok_or(super::Error::Protocol)?,
                ))
            }
            Event::Request(request) => {
                let timeout = self
                    .opened
                    .as_ref()
                    .ok_or(super::Error::Closed)?
                    .limits
                    .response_timeout_ms;
                let due = self
                    .now
                    .checked_add(u64::from(timeout))
                    .ok_or(super::Error::Overflow)?;
                self.request = Some(OwnedRequest::new(request, due));
            }
            _ => (),
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
            self.transaction_tag = Some(tag);
        }
        Ok(())
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
        } else if self.transaction_tag == Some(tag) {
            self.transaction_tag = None;
            self.transaction.reply(reply, self.now)?;
        } else {
            return Err(super::Error::Protocol.into());
        }
        Ok(())
    }
}
