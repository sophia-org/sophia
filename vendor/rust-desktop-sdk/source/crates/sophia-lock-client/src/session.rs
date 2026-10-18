#[path = "session_image.rs"]
mod image_driver;
use crate::{
    Candidate,
    journal::{Control, Event, Journal},
    startup::Opened,
    submission::{Custody, Submission},
};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, Pipeline, PipelineError, Reply};
use sophia_9p_records::{Fid, Tag};
use sophia_lock_protocol::lock_files::*;
use std::os::fd::{AsFd, BorrowedFd};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    Negotiating,
    Ready,
    Closed,
    Stale,
    Refused,
    Failed,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Obligations {
    pub consumed: u64,
    pub acked: u64,
    pub event_pending: bool,
    pub object_pending: bool,
    pub waiting_for_ack: bool,
    pub waiting_for_retry: bool,
    pub deadline: Option<u64>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionError {
    Startup(crate::startup::Error),
    Wire(PipelineError),
    Journal(crate::journal::Error),
    Submission(crate::submission::Error),
    Object(crate::object::Error),
    Protocol(&'static str),
    Remote(u32),
    ClockRegression,
    Deadline,
    Closed,
    NotReady,
    Refused(LockRefusal),
}
macro_rules! convert {
    ($ty:ty, $kind:ident) => {
        impl From<$ty> for SessionError {
            fn from(e: $ty) -> Self {
                Self::$kind(e)
            }
        }
    };
}
convert!(crate::startup::Error, Startup);
convert!(PipelineError, Wire);
convert!(crate::journal::Error, Journal);
convert!(crate::submission::Error, Submission);
convert!(crate::object::Error, Object);

/// Experimental lock session. Providers render; this grants no credential or
/// unlock authority. Release and production qualification remain separate.
pub struct Session {
    image: Option<crate::LockImageAdapter>,
    image_tag: Option<Tag>,
    wire: Option<Pipeline>,
    root: Fid,
    events: Fid,
    submit: Fid,
    ack: Fid,
    events_iounit: u32,
    epoch: u64,
    limits: LockFileLimits,
    journal: Journal,
    submission: Submission,
    fetch: Option<crate::object::Fetch>,
    object: Option<LockObject>,
    generation: u64,
    event_tag: Option<Tag>,
    ack_tag: Option<Tag>,
    tx_tag: Option<Tag>,
    object_tag: Option<Tag>,
    status_sequence: u64,
    ack_pending: u64,
    acked: u64,
    now: u64,
    bootstrap_deadline: Option<u64>,
    dirty: bool,
    state: SessionState,
    error: Option<SessionError>,
    remote_error: Option<u32>,
}
impl Session {
    pub fn from_opened(o: Opened) -> Result<Self, SessionError> {
        Self::from_opened_inner(o, None)
    }
    pub fn from_opened_with_image(
        o: Opened,
        config: crate::LockImageConfig,
    ) -> Result<Self, SessionError> {
        if o.offer.minimum_revision > 2
            || o.offer.maximum_revision < 2
            || o.offer.requested_capabilities & LOCK_FILE_CAPABILITY_SEALED_MEMORY == 0
            || o.offer.requested_capabilities & LOCK_FILE_CAPABILITY_DMA_BUF != 0
        {
            return Err(SessionError::Protocol(
                "image adapter requires sealed r2 offer",
            ));
        }
        let image = crate::LockImageAdapter::new(config, o.epoch, o.limits, o.now)
            .map_err(|_| SessionError::Protocol("invalid image config"))?;
        Self::from_opened_inner(o, Some(image))
    }
    fn from_opened_inner(
        o: Opened,
        image: Option<crate::LockImageAdapter>,
    ) -> Result<Self, SessionError> {
        let mut journal = Journal::new(o.epoch, &o.offer)?;
        if image.is_some() {
            journal.enable_image();
        }
        let mut submission = Submission::new(o.epoch, o.transaction, o.transaction_iounit)?;
        submission.admit(&Candidate::Negotiate(o.offer))?;
        Ok(Self {
            image,
            image_tag: None,
            wire: Some(o.pipeline),
            root: o.root,
            events: o.events,
            submit: o.submit,
            ack: o.ack,
            events_iounit: o.events_iounit,
            epoch: o.epoch,
            limits: o.limits,
            journal,
            submission,
            fetch: None,
            object: None,
            generation: 0,
            event_tag: None,
            ack_tag: None,
            tx_tag: None,
            object_tag: None,
            status_sequence: 0,
            ack_pending: 0,
            acked: 0,
            now: o.now,
            bootstrap_deadline: Some(o.deadline),
            dirty: true,
            state: SessionState::Negotiating,
            error: None,
            remote_error: None,
        })
    }
    pub fn state(&self) -> SessionState {
        self.state
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn limits(&self) -> LockFileLimits {
        self.limits
    }
    pub fn welcome(&self) -> Option<LockNegotiated> {
        self.journal.welcome()
    }
    pub fn selected_revision(&self) -> Option<u16> {
        self.journal.selected_revision()
    }
    pub fn error(&self) -> Option<&SessionError> {
        self.error.as_ref()
    }
    pub fn remote_error(&self) -> Option<u32> {
        self.remote_error
    }
    pub fn refusal(&self) -> Option<LockRefusal> {
        self.journal.refusal().map(|(_, r)| r)
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.wire.as_ref().map(AsFd::as_fd)
    }
    pub fn latest_submission(&self) -> Option<(u64, Custody)> {
        self.submission.latest()
    }
    pub fn lock(&self) -> Option<(u64, &LockObject)> {
        self.object.as_ref().map(|v| (self.generation, v))
    }
    pub fn event(&self) -> Option<(LockFileHeader, &Event)> {
        if self.error.is_some() {
            None
        } else {
            self.journal.head()
        }
    }
    fn live(&self) -> Result<(), SessionError> {
        self.error.as_ref().map_or(Ok(()), |e| Err(e.clone()))
    }
    fn ready(&self) -> Result<(), SessionError> {
        self.live()?;
        if self.state != SessionState::Ready {
            Err(SessionError::NotReady)
        } else {
            Ok(())
        }
    }
    pub fn consume(&mut self) -> Result<(), SessionError> {
        self.live()?;
        self.journal.consume()?;
        self.dirty = true;
        Ok(())
    }
    pub fn submit(&mut self, candidate: &Candidate) -> Result<u64, SessionError> {
        self.ready()?;
        if !matches!(
            candidate,
            Candidate::Present(_) | Candidate::FrameDemand(_) | Candidate::ResourceRetire(_)
        ) {
            return Err(SessionError::Protocol(
                "candidate is owned by startup or image adapter",
            ));
        }
        let id = self.submission.admit(candidate)?;
        self.dirty = true;
        Ok(id)
    }
    pub fn retry_submission(&mut self) -> Result<(), SessionError> {
        self.live()?;
        self.submission.retry()?;
        self.dirty = true;
        Ok(())
    }
    pub fn io_interest(&self) -> Result<IoInterest, SessionError> {
        let mut i = self
            .wire
            .as_ref()
            .ok_or(SessionError::Closed)?
            .io_interest()?;
        i.immediate |= self.dirty || self.event().is_some();
        Ok(i)
    }
    pub fn wake_deadline(&self) -> Option<u64> {
        if self.error.is_none() {
            let image = self.image.as_ref().and_then(|i| i.deadline(self.now));
            match (self.bootstrap_deadline, image) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            }
        } else {
            None
        }
    }
    pub fn obligations(&self) -> Obligations {
        let live = self.error.is_none();
        Obligations {
            consumed: self.journal.consumed(),
            acked: self.acked,
            event_pending: self.event().is_some(),
            object_pending: live && self.fetch.is_some(),
            waiting_for_ack: live && self.submission.waiting_for_ack(self.acked),
            waiting_for_retry: live && self.submission.waiting_for_retry(),
            deadline: self.wake_deadline(),
        }
    }
    /// One shared byte/syscall budget across at most 32 rounds. Only startup has
    /// a timer; the caller supplies monotonic time and explicitly retries EAGAIN.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), SessionError> {
        self.live()?;
        if now < self.now {
            return Err(SessionError::ClockRegression);
        }
        self.now = now;
        if self.bootstrap_deadline.is_some_and(|d| now >= d) {
            let _ = self.drain();
            return self.finish(SessionError::Deadline);
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            let polled = self.wire.as_mut().unwrap().poll_with_budget(&mut budget);
            let drained = self.drain();
            if let Some((seq, reason)) = self.journal.refusal()
                && self.acked >= seq
            {
                return self.finish(SessionError::Refused(reason));
            }
            if let Err(e) = drained {
                return self.finish(e);
            }
            if let Err(e) = polled {
                return self.finish(e.into());
            }
            if self.journal.welcome().is_some() {
                self.state = SessionState::Ready;
            }
            if self.state == SessionState::Ready && self.submission.idle() {
                self.bootstrap_deadline = None;
            }
            self.dirty = false;
            let queued = match self.drive() {
                Ok(v) => v,
                Err(e) => return self.finish(e),
            };
            if budget.exhausted() || (!queued && !drained.unwrap()) {
                break;
            }
        }
        Ok(())
    }
    pub fn close(&mut self) {
        if self.error.is_none() {
            let _ = self.drain();
            let _ = self.finish(SessionError::Closed);
        }
    }
    fn finish(&mut self, error: SessionError) -> Result<(), SessionError> {
        self.wire.take();
        self.submission.disconnect();
        if let Some(image) = &mut self.image {
            image.settle_submission(self.submission.latest(), self.now);
            image.producer.journal_lost();
        }
        self.fetch = None;
        self.state = match &error {
            SessionError::Refused(_) => SessionState::Refused,
            SessionError::Remote(116)
            | SessionError::Submission(crate::submission::Error::Remote(116))
            | SessionError::Object(crate::object::Error::Remote(116)) => SessionState::Stale,
            SessionError::Closed
            | SessionError::Deadline
            | SessionError::Wire(PipelineError::Io(std::io::ErrorKind::UnexpectedEof)) => {
                SessionState::Closed
            }
            _ => SessionState::Failed,
        };
        self.error = Some(error.clone());
        Err(error)
    }
    fn advance(&mut self) -> Result<(), SessionError> {
        let s = &mut self.submission;
        let image = &mut self.image;
        let now = self.now;
        self.journal.advance(|h, c| {
            if let Some(image) = image.as_mut()
                && let Control::Image(v) = c
            {
                image
                    .record(h.connection_epoch, v, now)
                    .map_err(|_| crate::journal::Error::Invalid)?;
            }
            if let Control::Submitted(v) = c {
                s.submitted(h.sequence, v)
                    .map_err(|_| crate::journal::Error::Invalid)?;
            }
            Ok(())
        })?;
        if let Some(image) = &mut self.image {
            if let (Some(revision), Some(welcome)) =
                (self.journal.selected_revision(), self.journal.welcome())
            {
                image.select(revision, welcome.granted_capabilities);
            }
            image.settle_submission(self.submission.latest(), self.now);
        }
        if let Some((h, Event::ResourceStatus(v))) = self.journal.head()
            && h.sequence != self.status_sequence
        {
            if let Some(image) = &mut self.image {
                image
                    .status(*v, self.now)
                    .map_err(|_| SessionError::Protocol("invalid image status"))?;
            }
            self.status_sequence = h.sequence;
        }
        Ok(())
    }
    fn drain(&mut self) -> Result<bool, SessionError> {
        let mut progress = false;
        let mut tx = None;
        let mut failure = None;
        // Bound reply draining independently of pipeline capacity.
        // Preserve valid custody before EOF.
        for _ in 0..16 {
            let Some((tag, reply)) = self.wire.as_mut().and_then(Pipeline::take_reply) else {
                break;
            };
            progress = true;
            if let Reply::Error(e) = &reply {
                self.remote_error = Some(e.0);
            }
            let result = if self.tx_tag == Some(tag) {
                self.tx_tag = None;
                tx = Some(reply);
                Ok(())
            } else if self.event_tag == Some(tag) {
                self.event_tag = None;
                match reply {
                    Reply::Read(b) if !b.is_empty() => self.journal.append(&b).map_err(Into::into),
                    Reply::Read(_) => Err(SessionError::Closed),
                    Reply::Error(e) => Err(SessionError::Remote(e.0)),
                    _ => Err(SessionError::Protocol("invalid event reply")),
                }
            } else if self.ack_tag == Some(tag) {
                self.ack_tag = None;
                match reply {
                    Reply::Write(16) => {
                        self.acked = self.ack_pending;
                        Ok(())
                    }
                    Reply::Error(e) => Err(SessionError::Remote(e.0)),
                    _ => Err(SessionError::Protocol("invalid ACK reply")),
                }
            } else if self.image_tag == Some(tag) {
                self.image_tag = None;
                self.image
                    .as_mut()
                    .and_then(|i| i.read.as_mut())
                    .ok_or(SessionError::Protocol("missing image read"))?
                    .reply(reply)
                    .map_err(|_| SessionError::Protocol("invalid image read"))
            } else if self.object_tag == Some(tag) {
                self.object_tag = None;
                self.fetch
                    .as_mut()
                    .ok_or(SessionError::Protocol("missing fetch"))?
                    .reply(reply)
                    .map_err(Into::into)
            } else {
                Err(SessionError::Protocol("unowned reply"))
            };
            if failure.is_none() {
                failure = result.err();
            }
        }
        let advanced = self.advance();
        if failure.is_none() {
            failure = advanced.err();
        }
        if let Some(reply) = tx {
            let result = self
                .submission
                .reply(reply, self.journal.welcome().is_some());
            if failure.is_none() {
                failure = result.err().map(Into::into);
            }
        }
        if let Some(image) = &mut self.image {
            image.settle_submission(self.submission.latest(), self.now);
        }
        failure.map_or(Ok(progress), Err)
    }
    fn drive(&mut self) -> Result<bool, SessionError> {
        self.advance()?;
        if let Some((seq, publication)) = self.journal.pending_publication() {
            if self.fetch.is_none() {
                self.fetch = Some(crate::object::Fetch::new(self.epoch, publication)?);
            }
            let f = self.fetch.as_mut().unwrap();
            if f.is_ready() {
                let superseded = f.superseded();
                if let Some(value) = f.take_result() {
                    self.object = Some(value);
                    self.generation = publication.object_generation;
                }
                self.journal
                    .publication_resolved(seq, publication, superseded)?;
                self.fetch = None;
                self.advance()?;
                // A superseded publication may reveal the next one without I/O.
                self.dirty = true;
            }
        }
        let mut queued = self.dirty;
        let wire = self.wire.as_mut().ok_or(SessionError::Closed)?;
        if self.event_tag.is_none()
            && self.journal.capacity() != 0
            && self.journal.refusal().is_none()
        {
            let mut n = self.journal.capacity().min((wire.msize() - 11) as usize) as u32;
            if self.events_iounit != 0 {
                n = n.min(self.events_iounit);
            }
            match wire.read(self.events, self.journal.offset(), n) {
                Ok(t) => {
                    self.event_tag = Some(t);
                    queued = true;
                }
                Err(e) if busy(&e) => (),
                Err(e) => return Err(e.into()),
            }
        }
        if self.ack_tag.is_none() && self.journal.consumed() > self.acked {
            let b = encode_lock_file_ack(LockFileAck {
                connection_epoch: self.epoch,
                sequence: self.journal.consumed(),
            })
            .map_err(|_| SessionError::Protocol("ACK identity"))?;
            match wire.write(self.ack, 0, &b) {
                Ok(t) => {
                    self.ack_tag = Some(t);
                    self.ack_pending = self.journal.consumed();
                    queued = true;
                }
                Err(e) if busy(&e) => (),
                Err(e) => return Err(e.into()),
            }
        }
        if self.object_tag.is_none()
            && let Some(f) = &mut self.fetch
        {
            use crate::object::Operation as Op;
            let mut fid = Fid::NOFID;
            let result = match f.operation(wire.msize())? {
                Some(Op::Walk) => Some(wire.walk(self.root, &[b"lock"]).map(|(t, v)| {
                    fid = v;
                    t
                })),
                Some(Op::Open(v)) => Some(wire.lopen(v, 0)),
                Some(Op::Read { fid, offset, count }) => Some(wire.read(fid, offset, count)),
                Some(Op::Clunk(v)) => Some(wire.clunk(v)),
                None => None,
            };
            match result {
                Some(Ok(t)) => {
                    f.issued(wire.msize(), fid)?;
                    self.object_tag = Some(t);
                    queued = true;
                }
                Some(Err(e)) if busy(&e) => (),
                Some(Err(e)) => return Err(e.into()),
                None => (),
            }
        }
        queued |= self.drive_image()?;
        queued |= self.drive_submission()?;
        Ok(queued)
    }

    fn drive_submission(&mut self) -> Result<bool, SessionError> {
        let mut queued = false;
        let wire = self.wire.as_mut().ok_or(SessionError::Closed)?;
        if self.tx_tag.is_none() {
            use crate::submission::Operation as Op;
            let mut fid = Fid::NOFID;
            let result = match self.submission.operation(self.acked, wire.msize())? {
                Some(Op::Walk) => Some(wire.walk(self.root, &[b"transaction"]).map(|(t, v)| {
                    fid = v;
                    t
                })),
                Some(Op::Open(v)) => Some(wire.lopen(v, 2)),
                Some(Op::Write { fid, offset, bytes }) => Some(wire.write(fid, offset, bytes)),
                Some(Op::Submit(b)) => Some(wire.write(self.submit, 0, b)),
                Some(Op::Clunk(v)) => Some(wire.clunk(v)),
                None => None,
            };
            match result {
                Some(Ok(t)) => {
                    self.submission.issued(self.acked, wire.msize(), fid)?;
                    self.tx_tag = Some(t);
                    queued = true;
                }
                Some(Err(e)) if busy(&e) => (),
                Some(Err(e)) => return Err(e.into()),
                None => (),
            }
        }
        Ok(queued)
    }
}
pub(crate) fn busy(e: &PipelineError) -> bool {
    matches!(
        e,
        PipelineError::Limit(
            "output buffer full"
                | "max outstanding requests reached"
                | "max fids reached"
                | "reply would not fit max_buffered_input"
        )
    )
}
