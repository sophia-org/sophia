use crate::{
    Obligations, Session, SessionError, SessionState, startup::Opening, submission::Custody,
};
use sophia_9p_client::pipeline::{
    IoInterest, Pipeline, PipelineError, PipelineLimits, VersionHandshake,
};
use sophia_9p_records::Fid;
use sophia_output_protocol::{
    OutputV1ClientHello, SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
    output_files::{OutputFileLimits, OutputFileRefusal},
};
use std::os::{fd::BorrowedFd, unix::net::UnixStream};

enum Phase {
    Version(VersionHandshake),
    Files(Box<Opening>),
    Session(Box<Session>),
}

/// Nonblocking output startup, from a connected socket or an authenticated root.
/// One absolute deadline covers version, file startup and role negotiation.
pub struct Connecting {
    phase: Option<Phase>,
    offer: OutputV1ClientHello,
    now: u64,
    deadline: u64,
    error: Option<SessionError>,
}
impl Connecting {
    /// For supplied-admission exports. Protected production launches use
    /// `attached`; authentication failure must never fall back to this path.
    pub fn direct(
        stream: UnixStream,
        limits: PipelineLimits,
        offer: OutputV1ClientHello,
        now: u64,
        deadline: u64,
    ) -> Result<Self, SessionError> {
        if offer.capabilities & SOPHIA_OUTPUT_CAPABILITY_OBSERVE == 0 {
            return Err(SessionError::Protocol(
                "output observation capability required",
            ));
        }
        Ok(Self {
            phase: Some(Phase::Version(VersionHandshake::new(
                stream, limits, now, deadline,
            )?)),
            offer,
            now,
            deadline,
            error: None,
        })
    }
    pub fn attached(
        pipeline: Pipeline,
        root: Fid,
        offer: OutputV1ClientHello,
        now: u64,
        deadline: u64,
    ) -> Result<Self, SessionError> {
        Ok(Self {
            phase: Some(Phase::Files(Box::new(Opening::attached(
                pipeline, root, offer, now, deadline,
            )?))),
            offer,
            now,
            deadline,
            error: None,
        })
    }
    pub fn ready(&self) -> bool {
        self.error.is_none()
            && matches!(&self.phase,Some(Phase::Session(s)) if s.state()==SessionState::Ready)
    }
    pub fn error(&self) -> Option<&SessionError> {
        self.error.as_ref()
    }
    pub fn state(&self) -> SessionState {
        if let Some(Phase::Session(session)) = &self.phase {
            return session.state();
        }
        if self.remote_error() == Some(116) {
            return SessionState::Stale;
        }
        match &self.error {
            None if self.phase.is_some() => SessionState::Negotiating,
            None
            | Some(
                SessionError::Closed
                | SessionError::Deadline
                | SessionError::Wire(
                    PipelineError::Timeout | PipelineError::Io(std::io::ErrorKind::UnexpectedEof),
                )
                | SessionError::Startup(
                    crate::startup::Error::Deadline
                    | crate::startup::Error::Closed
                    | crate::startup::Error::Wire(
                        PipelineError::Timeout
                        | PipelineError::Io(std::io::ErrorKind::UnexpectedEof),
                    ),
                ),
            ) => SessionState::Closed,
            Some(_) => SessionState::Failed,
        }
    }
    pub fn epoch(&self) -> Option<u64> {
        match &self.phase {
            Some(Phase::Files(f)) => f.epoch(),
            Some(Phase::Session(s)) => Some(s.epoch()),
            _ => None,
        }
    }
    pub fn limits(&self) -> Option<OutputFileLimits> {
        match &self.phase {
            Some(Phase::Files(f)) => f.limits(),
            Some(Phase::Session(s)) => Some(s.limits()),
            _ => None,
        }
    }
    pub fn capabilities(&self) -> Option<u64> {
        match &self.phase {
            Some(Phase::Session(s)) => s.capabilities(),
            _ => None,
        }
    }
    pub fn refusal(&self) -> Option<OutputFileRefusal> {
        match &self.phase {
            Some(Phase::Session(s)) => s.refusal(),
            _ => None,
        }
    }
    pub fn remote_error(&self) -> Option<u32> {
        if let Some(Phase::Session(s)) = &self.phase {
            return s.remote_error();
        }
        match &self.error {
            Some(SessionError::Wire(PipelineError::Remote(errno)))
            | Some(SessionError::Startup(crate::startup::Error::Wire(PipelineError::Remote(
                errno,
            )))) => Some(errno.0),
            Some(SessionError::Startup(crate::startup::Error::Remote(errno))) => Some(*errno),
            _ => None,
        }
    }
    /// Negotiation ticket results remain readable after startup failure or
    /// close, until the ready Session is transferred out of this owner.
    pub fn custody(&self, ticket: u64) -> Option<Custody> {
        match &self.phase {
            Some(Phase::Session(s)) => s.custody(ticket),
            _ => None,
        }
    }
    pub fn obligations(&self) -> Obligations {
        if let Some(Phase::Session(s)) = &self.phase {
            return s.obligations();
        }
        Obligations {
            consumed: 0,
            acked: 0,
            submitted_sequence: 0,
            deadline: if self.error.is_none() && self.phase.is_some() {
                Some(self.deadline)
            } else {
                None
            },
            retry_at: None,
            event_pending: false,
            topology_pending: false,
            topology_ready: false,
            waiting_for_ack: false,
        }
    }
    pub fn deadline(&self) -> u64 {
        self.deadline
    }
    /// The next timer wake, including negotiation retries before handoff.
    /// Using only the bootstrap deadline can sleep past an EAGAIN backoff.
    pub fn wake_deadline(&self) -> Option<u64> {
        if self.error.is_some() {
            return None;
        }
        match self.phase.as_ref()? {
            Phase::Version(v) => Some(v.deadline()),
            Phase::Files(f) => Some(f.deadline()),
            Phase::Session(s) => s.wake_deadline(),
        }
    }
    pub fn fd(&self) -> Option<BorrowedFd<'_>> {
        match self.phase.as_ref()? {
            Phase::Version(v) => v.fd(),
            Phase::Files(f) => f.fd(),
            Phase::Session(s) => s.fd(),
        }
    }
    pub fn io_interest(&self) -> Result<IoInterest, SessionError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let mut interest = match self.phase.as_ref().ok_or(SessionError::Closed)? {
            Phase::Version(v) => v.io_interest(),
            Phase::Files(f) => f.io_interest()?,
            Phase::Session(s) => s.io_interest()?,
        };
        interest.immediate |= self.ready();
        Ok(interest)
    }
    /// One phase performs socket I/O per call; phase transitions do not
    /// replenish the caller's budget. Each next phase is queued without I/O.
    pub fn dispatch(&mut self, now: u64, bytes: usize) -> Result<(), SessionError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if now < self.now {
            return Err(SessionError::ClockRegression);
        }
        self.now = now;
        let mut phase = self.phase.take().ok_or(SessionError::Closed)?;
        let result = (|| -> Result<(), SessionError> {
            match &mut phase {
                Phase::Version(v) => {
                    v.dispatch(now, bytes)?;
                    if let Some(pipeline) = v.take_pipeline() {
                        phase = Phase::Files(Box::new(Opening::direct(
                            pipeline,
                            self.offer,
                            now,
                            self.deadline,
                        )?));
                    }
                }
                Phase::Files(f) => f.dispatch(now, bytes)?,
                Phase::Session(s) => s.dispatch(now, bytes)?,
            }
            Ok(())
        })();
        if result.is_ok()
            && matches!(&phase,Phase::Files(f) if f.is_opened())
            && let Phase::Files(files) = phase
        {
            match (*files)
                .into_opened()
                .map_err(SessionError::from)
                .and_then(Session::from_opened)
            {
                Ok(session) => phase = Phase::Session(Box::new(session)),
                Err(error) => {
                    self.error = Some(error.clone());
                    return Err(error);
                }
            }
        }
        self.phase = Some(phase);
        if let Err(error) = &result {
            self.error = Some(error.clone());
            self.close_phase();
        }
        result
    }
    pub fn take_session(&mut self) -> Option<Session> {
        if !self.ready() {
            return None;
        }
        match self.phase.take()? {
            Phase::Session(s) => Some(*s),
            _ => unreachable!(),
        }
    }
    fn close_phase(&mut self) {
        if let Some(phase) = &mut self.phase {
            match phase {
                Phase::Version(v) => v.close(),
                Phase::Files(f) => f.close(),
                Phase::Session(s) => s.close(),
            }
        }
    }
    pub fn close(&mut self) {
        self.close_phase();
        self.error.get_or_insert(SessionError::Closed);
    }
}
