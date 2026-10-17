use crate::{
    journal::{Control, Event, Journal},
    startup::Opened,
    submission::{Custody, Operation, Progress, Submission},
};
use sophia_9p_client::pipeline::{IoBudget, IoInterest, Pipeline, PipelineError, Reply};
use sophia_9p_records::{Fid, Tag};
use sophia_output_protocol::{OutputV1Proposal, OutputV1Snapshot, TransactionId, output_files::*};
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

/// Passive work obligations, equivalent to C's sophia_os_obligations. A
/// consumed watermark is not acknowledged until its write reply arrives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Obligations {
    pub consumed: u64,
    pub acked: u64,
    pub submitted_sequence: u64,
    pub deadline: Option<u64>,
    pub retry_at: Option<u64>,
    pub event_pending: bool,
    pub topology_pending: bool,
    pub topology_ready: bool,
    pub waiting_for_ack: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionError {
    Startup(crate::startup::Error),
    Wire(PipelineError),
    Journal(crate::journal::Error),
    Submission(crate::submission::Error),
    Protocol(&'static str),
    ClockRegression,
    Deadline,
    Closed,
    NotReady,
    Topology(crate::topology::Error),
    Refused(OutputFileRefusal),
}
impl From<crate::startup::Error> for SessionError {
    fn from(e: crate::startup::Error) -> Self {
        Self::Startup(e)
    }
}
impl From<PipelineError> for SessionError {
    fn from(e: PipelineError) -> Self {
        Self::Wire(e)
    }
}
impl From<crate::journal::Error> for SessionError {
    fn from(e: crate::journal::Error) -> Self {
        Self::Journal(e)
    }
}
impl From<crate::submission::Error> for SessionError {
    fn from(e: crate::submission::Error) -> Self {
        Self::Submission(e)
    }
}

impl From<crate::topology::Error> for SessionError {
    fn from(e: crate::topology::Error) -> Self {
        Self::Topology(e)
    }
}

/// Output session owning all I/O, ordered events and publication-bound topology.
/// Production-export and release qualification remain separate gates.
pub struct Session {
    wire: Option<Pipeline>,
    root: Fid,
    events: Fid,
    submit: Fid,
    ack: Fid,
    events_iounit: u32,
    epoch: u64,
    limits: OutputFileLimits,
    journal: Journal,
    submission: Submission,
    fetch: Option<crate::topology::Fetch>,
    topology: Option<OutputV1Snapshot>,
    object_tag: Option<Tag>,
    event_tag: Option<Tag>,
    ack_tag: Option<Tag>,
    tx_tag: Option<Tag>,
    ack_pending: u64,
    acked: u64,
    now: u64,
    pass: u64,
    bootstrap_deadline: Option<u64>,
    dirty: bool,
    state: SessionState,
    error: Option<SessionError>,
    remote_error: Option<u32>,
}

impl Session {
    /// Continue fixed-file startup without a new attach or version exchange.
    pub fn from_opened(opened: Opened) -> Result<Self, SessionError> {
        let journal = Journal::new(opened.epoch, opened.limits, opened.offer, opened.now)?;
        let mut submission = Submission::new(opened.epoch, opened.limits)?;
        submission.negotiate(opened.offer, opened.now, opened.deadline)?;
        Ok(Self {
            wire: Some(opened.pipeline),
            root: opened.root,
            events: opened.events,
            submit: opened.submit,
            ack: opened.ack,
            events_iounit: opened.events_iounit,
            epoch: opened.epoch,
            limits: opened.limits,
            journal,
            submission,
            fetch: None,
            topology: None,
            object_tag: None,
            event_tag: None,
            ack_tag: None,
            tx_tag: None,
            ack_pending: 0,
            acked: 0,
            now: opened.now,
            pass: 0,
            bootstrap_deadline: Some(opened.deadline),
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
    pub fn limits(&self) -> OutputFileLimits {
        self.limits
    }
    pub fn capabilities(&self) -> Option<u64> {
        self.journal.selected()
    }
    pub fn remote_error(&self) -> Option<u32> {
        self.remote_error
    }
    pub fn error(&self) -> Option<&SessionError> {
        self.error.as_ref()
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.wire.as_ref().map(AsFd::as_fd)
    }
    pub fn custody(&self, ticket: u64) -> Option<Custody> {
        self.submission.custody(ticket)
    }
    pub fn acked(&self) -> u64 {
        self.acked
    }
    pub fn consumed(&self) -> u64 {
        self.journal.consumed()
    }
    pub fn next_transaction(&mut self) -> Result<u64, SessionError> {
        Ok(self.submission.next_transaction()?)
    }
    pub fn event(&self) -> Option<(OutputFileHeader, &Event)> {
        if self.error.is_some() {
            None
        } else {
            self.journal.head()
        }
    }
    pub fn consume(&mut self) -> Result<(), SessionError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let publication = matches!(self.event(), Some((_, Event::ObjectPublished(_))));
        self.journal.consume()?;
        if publication {
            self.fetch = None;
        }
        self.dirty = true;
        Ok(())
    }
    pub fn topology(&self) -> Option<&OutputV1Snapshot> {
        self.topology.as_ref()
    }
    pub fn refusal(&self) -> Option<OutputFileRefusal> {
        self.journal.refusal().map(|(_, reason)| reason)
    }
    pub fn submit(
        &mut self,
        transaction: TransactionId,
        proposal: &OutputV1Proposal,
        deadline: u64,
    ) -> Result<u64, SessionError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.state != SessionState::Ready {
            return Err(SessionError::NotReady);
        }
        let id = self.submission.admit(
            transaction,
            proposal,
            self.journal.selected().unwrap(),
            self.now,
            deadline,
        )?;
        self.dirty = true;
        Ok(id)
    }
    pub fn io_interest(&self) -> Result<IoInterest, SessionError> {
        let mut interest = self
            .wire
            .as_ref()
            .ok_or(SessionError::Closed)?
            .io_interest()?;
        interest.immediate |= self.dirty || self.event().is_some();
        Ok(interest)
    }
    fn deadline(&self) -> Option<u64> {
        [
            self.bootstrap_deadline,
            self.submission.deadline(),
            self.journal.fragment_deadline(),
            self.fetch
                .as_ref()
                .and_then(crate::topology::Fetch::deadline),
        ]
        .into_iter()
        .flatten()
        .min()
    }
    pub fn wake_deadline(&self) -> Option<u64> {
        if self.error.is_some() {
            None
        } else {
            [
                self.deadline(),
                self.submission.retry_at(),
                self.fetch
                    .as_ref()
                    .and_then(crate::topology::Fetch::retry_at),
            ]
            .into_iter()
            .flatten()
            .min()
        }
    }
    pub fn obligations(&self) -> Obligations {
        let live = self.error.is_none();
        Obligations {
            consumed: self.journal.consumed(),
            acked: self.acked,
            submitted_sequence: self.submission.submitted_sequence(),
            deadline: if live { self.deadline() } else { None },
            retry_at: if live {
                [
                    self.submission.retry_at(),
                    self.fetch
                        .as_ref()
                        .and_then(crate::topology::Fetch::retry_at),
                ]
                .into_iter()
                .flatten()
                .min()
            } else {
                None
            },
            event_pending: live && self.journal.head().is_some(),
            topology_pending: live && self.fetch.as_ref().is_some_and(|f| !f.is_ready()),
            topology_ready: live && self.topology.is_some(),
            waiting_for_ack: live && self.submission.waiting_for_ack(self.acked),
        }
    }
    fn progress(&self) -> Progress {
        Progress {
            now: self.now,
            pass: self.pass,
            consumed: self.journal.consumed(),
            acked: self.acked,
        }
    }

    /// Share one byte/syscall budget across at most 32 internal rounds. Time is
    /// caller-supplied monotonic milliseconds. No send occurs after expiry.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), SessionError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if now < self.now {
            return Err(SessionError::ClockRegression);
        }
        self.now = now;
        self.pass = match self.pass.checked_add(1) {
            Some(pass) => pass,
            None => return self.finish(SessionError::Protocol("dispatch pass exhausted")),
        };
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            let _ = self.drain();
            return self.finish(SessionError::Deadline);
        }
        let mut budget = IoBudget::new(bytes);
        for _ in 0..32 {
            let wire_result = self.wire.as_mut().unwrap().poll_with_budget(&mut budget);
            // Validated replies remain useful after EOF or a later wire error.
            let drained = self.drain();
            // A refused export may revoke a pending event read in the same
            // batch as the successful final ACK. That is the expected end.
            if let Some((sequence, reason)) = self.journal.refusal()
                && self.acked >= sequence
            {
                return self.finish(SessionError::Refused(reason));
            }
            if let Err(error) = drained {
                return self.finish(error);
            }
            if let Err(error) = wire_result {
                return self.finish(error.into());
            }
            if self.journal.selected().is_some() {
                self.state = SessionState::Ready;
            }
            if self.state == SessionState::Ready
                && self.submission.idle()
                && self.acked >= self.submission.submitted_sequence()
            {
                self.bootstrap_deadline = None;
            }
            if self.deadline().is_some_and(|deadline| now >= deadline) {
                return self.finish(SessionError::Deadline);
            }
            self.dirty = false;
            let queued = match self.drive() {
                Ok(value) => value,
                Err(error) => return self.finish(error),
            };
            if budget.exhausted() || (!queued && !drained.unwrap()) {
                break;
            }
        }
        Ok(())
    }

    /// No wire I/O. Preserve buffered custody observations before closing.
    pub fn close(&mut self) {
        if self.error.is_none() {
            let _ = self.drain();
            let _ = self.finish(SessionError::Closed);
        }
    }
    fn finish(&mut self, error: SessionError) -> Result<(), SessionError> {
        self.submission.disconnect();
        self.fetch = None;
        self.wire.take();
        self.state = if matches!(error, SessionError::Refused(_)) {
            SessionState::Refused
        } else if self.remote_error == Some(116) {
            SessionState::Stale
        } else if matches!(
            error,
            SessionError::Closed
                | SessionError::Deadline
                | SessionError::Wire(PipelineError::Io(std::io::ErrorKind::UnexpectedEof))
        ) {
            SessionState::Closed
        } else {
            SessionState::Failed
        };
        self.error = Some(error.clone());
        Err(error)
    }

    fn drain(&mut self) -> Result<bool, SessionError> {
        let mut tx_reply = None;
        let mut failure = None;
        let mut progress = false;
        // Events, ACK, one submission and one topology request may be outstanding.
        for _ in 0..8 {
            let Some((tag, reply)) = self.wire.as_mut().and_then(Pipeline::take_reply) else {
                break;
            };
            progress = true;
            if let Reply::Error(errno) = &reply {
                self.remote_error = Some(errno.0);
            }
            let result = if self.tx_tag == Some(tag) {
                self.tx_tag = None;
                tx_reply = Some(reply);
                Ok(())
            } else if self.event_tag == Some(tag) {
                self.event_tag = None;
                match reply {
                    Reply::Read(data) if !data.is_empty() => {
                        let submission = &mut self.submission;
                        self.journal
                            .append(&data, self.now, |header, control| {
                                if let Control::Submitted(value) = control {
                                    submission
                                        .submitted(header.sequence, value)
                                        .map_err(|_| crate::journal::Error::Invalid)?;
                                }
                                Ok(())
                            })
                            .map_err(Into::into)
                    }
                    Reply::Read(_) => Err(SessionError::Closed),
                    _ => Err(SessionError::Protocol("event read refused")),
                }
            } else if self.object_tag == Some(tag) {
                self.object_tag = None;
                self.fetch
                    .as_mut()
                    .ok_or(SessionError::Protocol("missing topology fetch"))?
                    .reply(reply, self.now)
                    .map_err(Into::into)
            } else if self.ack_tag == Some(tag) {
                self.ack_tag = None;
                if reply == Reply::Write(16) {
                    self.acked = self.ack_pending;
                    Ok(())
                } else {
                    Err(SessionError::Protocol("ACK write refused or short"))
                }
            } else {
                Err(SessionError::Protocol("unowned session reply"))
            };
            if failure.is_none() {
                failure = result.err();
            }
        }
        // Never interpret a refusal before all available journal bytes have
        // established their valid custody prefix, even if a later record fails.
        if let Some(reply) = tx_reply {
            let result = self.submission.reply(reply, self.progress());
            if failure.is_none() {
                failure = result.err().map(Into::into);
            }
        }
        if let Some(error) = failure {
            Err(error)
        } else {
            Ok(progress)
        }
    }

    fn drive(&mut self) -> Result<bool, SessionError> {
        if let Some((sequence, publication)) = self.journal.pending_publication() {
            if self.fetch.is_none() {
                self.fetch = Some(crate::topology::Fetch::new(
                    self.epoch,
                    publication,
                    self.now,
                    self.now
                        .saturating_add(u64::from(self.limits.ack_progress_timeout_millis)),
                )?);
            }
            if let Some(value) = self
                .fetch
                .as_mut()
                .and_then(crate::topology::Fetch::take_result)
            {
                self.topology = Some(value);
                self.journal.publication_verified(sequence, publication)?;
            }
        }
        let p = self.progress();
        let wire = self.wire.as_mut().ok_or(SessionError::Closed)?;
        let mut queued = false;
        if self.event_tag.is_none()
            && self.journal.capacity() != 0
            && self.journal.refusal().is_none()
        {
            let mut n = self.journal.capacity().min((wire.msize() - 11) as usize) as u32;
            if self.events_iounit != 0 {
                n = n.min(self.events_iounit);
            }
            match wire.read(self.events, self.journal.offset(), n) {
                Ok(tag) => {
                    self.event_tag = Some(tag);
                    queued = true;
                }
                Err(error) if busy(&error) => (),
                Err(error) => return Err(error.into()),
            }
        }
        if self.ack_tag.is_none() && self.journal.consumed() > self.acked {
            let mut bytes = [0; 16];
            bytes[..8].copy_from_slice(&self.epoch.to_le_bytes());
            bytes[8..].copy_from_slice(&self.journal.consumed().to_le_bytes());
            match wire.write(self.ack, 0, &bytes) {
                Ok(tag) => {
                    self.ack_tag = Some(tag);
                    self.ack_pending = self.journal.consumed();
                    queued = true;
                }
                Err(error) if busy(&error) => (),
                Err(error) => return Err(error.into()),
            }
        }
        if self.tx_tag.is_none() {
            let mut new_fid = Fid::NOFID;
            let result = match self.submission.operation(p, wire.msize())? {
                Some(Operation::Walk) => {
                    Some(wire.walk(self.root, &[b"transaction"]).map(|(tag, fid)| {
                        new_fid = fid;
                        tag
                    }))
                }
                Some(Operation::Open(fid)) => Some(wire.lopen(fid, 2)),
                Some(Operation::Write { fid, offset, bytes }) => {
                    Some(wire.write(fid, offset, bytes))
                }
                Some(Operation::Submit(bytes)) => Some(wire.write(self.submit, 0, bytes)),
                Some(Operation::Clunk(fid)) => Some(wire.clunk(fid)),
                None => None,
            };
            match result {
                Some(Ok(tag)) => {
                    self.submission.issued(p, wire.msize(), new_fid)?;
                    self.tx_tag = Some(tag);
                    queued = true;
                }
                Some(Err(error)) if busy(&error) => (),
                Some(Err(error)) => return Err(error.into()),
                None => (),
            }
        }
        if self.object_tag.is_none()
            && let Some(fetch) = &mut self.fetch
        {
            use crate::topology::Operation as ObjectOp;
            let mut new_fid = Fid::NOFID;
            let result = match fetch.operation(self.now, wire.msize())? {
                Some(ObjectOp::Walk) => {
                    Some(wire.walk(self.root, &[b"topology"]).map(|(tag, fid)| {
                        new_fid = fid;
                        tag
                    }))
                }
                Some(ObjectOp::Open(fid)) => Some(wire.lopen(fid, 0)),
                Some(ObjectOp::Read { fid, offset, count }) => Some(wire.read(fid, offset, count)),
                Some(ObjectOp::Clunk(fid)) => Some(wire.clunk(fid)),
                None => None,
            };
            match result {
                Some(Ok(tag)) => {
                    fetch.issued(self.now, wire.msize(), new_fid)?;
                    self.object_tag = Some(tag);
                    queued = true;
                }
                Some(Err(error)) if busy(&error) => (),
                Some(Err(error)) => return Err(error.into()),
                None => (),
            }
        }
        Ok(queued)
    }
}

pub(crate) fn busy(error: &PipelineError) -> bool {
    matches!(
        error,
        PipelineError::Limit(
            "output buffer full"
                | "max outstanding requests reached"
                | "max fids reached"
                | "reply would not fit max_buffered_input"
        )
    )
}
