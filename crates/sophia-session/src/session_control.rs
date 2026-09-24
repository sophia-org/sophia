use std::collections::{BTreeSet, VecDeque};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError};
use std::time::{Duration, Instant};

use sophia_protocol::{SurfaceId, TransactionId};
use sophia_x_authority::{
    XAuthorityClientControlAck, XAuthorityClientControlCommand, XAuthorityControlKind,
    XAuthorityControlOutcome, XServerFrontendClientId, XServerFrontendControlRouter,
    XServerFrontendRouteError,
};

pub const SESSION_CONTROL_CAPACITY: usize = 32;
pub const SESSION_CONTROL_QUEUE_TIMEOUT: Duration = Duration::from_millis(500);
pub const SESSION_CONTROL_ACKNOWLEDGEMENT_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionControlKey {
    pub client: XServerFrontendClientId,
    pub kind: XAuthorityControlKind,
    pub transaction: TransactionId,
    pub surface: SurfaceId,
}

impl SessionControlKey {
    pub fn from_command(command: XAuthorityClientControlCommand) -> Self {
        Self {
            client: command.client,
            kind: command.command.kind(),
            transaction: command.command.transaction(),
            surface: command.command.surface(),
        }
    }

    fn from_ack(ack: XAuthorityClientControlAck) -> Self {
        Self {
            client: ack.client,
            kind: ack.acknowledgement.kind,
            transaction: ack.acknowledgement.transaction,
            surface: ack.acknowledgement.surface,
        }
    }

    fn is_focus(self) -> bool {
        matches!(
            self.kind,
            XAuthorityControlKind::FocusSurface | XAuthorityControlKind::ClearFocus
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionControlFailure {
    Capacity,
    Duplicate,
    Rejected(XAuthorityControlOutcome),
    TimedOut,
    UnexpectedAcknowledgement,
    Disconnected,
    ClientDisconnected,
    /// Retired because the session began its quiescence after a successful
    /// primary exit and nothing is left to answer it (t187). Never the
    /// session's failure: a control that finds no frontend on the way out is
    /// the way out, not a fault.
    Quiescing,
}

impl SessionControlFailure {
    /// A target can disappear while its exact command is in flight. Retire
    /// obsolete close, focus and metadata commands without reporting them as
    /// applied. Admission and configuration still require a surviving target.
    pub const fn is_stale_target_for(self, kind: XAuthorityControlKind) -> bool {
        matches!(
            self,
            Self::ClientDisconnected | Self::Rejected(XAuthorityControlOutcome::ClientGone)
        ) || matches!(
            kind,
            XAuthorityControlKind::CloseSurface
                | XAuthorityControlKind::FocusSurface
                | XAuthorityControlKind::ClearFocus
                | XAuthorityControlKind::PublishMetadataRule
        ) && matches!(
            self,
            Self::Rejected(XAuthorityControlOutcome::UnknownSurface)
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionControlCompletion {
    pub key: SessionControlKey,
    pub failure: Option<SessionControlFailure>,
    pub queue_dwell: Duration,
    pub acknowledgement_latency: Duration,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionControlMetrics {
    pub enqueued: usize,
    pub dispatched: usize,
    pub delivered: usize,
    pub stale_targets_retired: usize,
    pub rejected: usize,
    pub timed_out: usize,
    pub unexpected: usize,
    pub disconnected: usize,
    pub recovered_timeouts: usize,
    /// Controls retired by quiescence before they were dispatched, or
    /// enqueued after it began (t187).
    pub quiesced_before_dispatch: usize,
    /// Controls in flight when quiescence began (t187).
    pub quiesced_in_flight: usize,
    pub peak_depth: usize,
    pub max_queue_dwell: Duration,
    pub max_acknowledgement_latency: Duration,
}

impl SessionControlMetrics {
    /// All accepted controls settled, with contained client failures recorded as
    /// failures. The strict proof predicate below intentionally stays stricter.
    pub const fn is_settled(self, pending: usize) -> bool {
        pending == 0
            && self.unexpected == 0
            && self.rejected == 0
            && self.timed_out == self.recovered_timeouts
            && self.enqueued
                == self.delivered
                    + self.stale_targets_retired
                    + self.disconnected
                    + self.timed_out
                    + self.quiesced_before_dispatch
                    + self.quiesced_in_flight
    }

    pub const fn is_drained(self, pending: usize) -> bool {
        pending == 0
            && self.enqueued == self.dispatched + self.quiesced_before_dispatch
            && self.dispatched
                == self
                    .delivered
                    .saturating_add(self.stale_targets_retired)
                    .saturating_add(self.quiesced_in_flight)
            && self.rejected == 0
            && self.timed_out == 0
            && self.unexpected == 0
    }
}

#[derive(Clone, Copy, Debug)]
struct PendingControl {
    command: XAuthorityClientControlCommand,
    key: SessionControlKey,
    queued_at: Instant,
    dispatch_eligible_at: Option<Instant>,
    dispatched_at: Option<Instant>,
}

#[derive(Debug, Default)]
pub struct SessionControlQueue {
    pending: VecDeque<PendingControl>,
    revoked_clients: BTreeSet<XServerFrontendClientId>,
    metrics: SessionControlMetrics,
    /// Set by `begin_quiescence` (t187): what was pending is retired, what
    /// is enqueued afterwards completes at once, and a late acknowledgement
    /// for either is inert.
    quiescing: bool,
    quiesced_keys: Vec<SessionControlKey>,
    quiesced_completions: Vec<SessionControlCompletion>,
}

pub trait SessionControlSender {
    fn try_send_control(
        &self,
        command: XAuthorityClientControlCommand,
    ) -> Result<(), TrySendError<XAuthorityClientControlCommand>>;
}

impl SessionControlSender for SyncSender<XAuthorityClientControlCommand> {
    fn try_send_control(
        &self,
        command: XAuthorityClientControlCommand,
    ) -> Result<(), TrySendError<XAuthorityClientControlCommand>> {
        self.try_send(command)
    }
}

impl SessionControlSender for XServerFrontendControlRouter {
    fn try_send_control(
        &self,
        command: XAuthorityClientControlCommand,
    ) -> Result<(), TrySendError<XAuthorityClientControlCommand>> {
        match self.route_control(command) {
            Ok(()) => Ok(()),
            Err(XServerFrontendRouteError::ClientQueueFull { .. }) => {
                Err(TrySendError::Full(command))
            }
            Err(_) => Err(TrySendError::Disconnected(command)),
        }
    }
}

impl SessionControlQueue {
    pub fn enqueue(
        &mut self,
        command: XAuthorityClientControlCommand,
        now: Instant,
    ) -> Result<SessionControlKey, SessionControlFailure> {
        let key = SessionControlKey::from_command(command);
        if self.pending.iter().any(|pending| pending.key == key) {
            return Err(SessionControlFailure::Duplicate);
        }
        if self.pending.len() >= SESSION_CONTROL_CAPACITY {
            return Err(SessionControlFailure::Capacity);
        }
        if self.quiescing {
            // Accepted and retired in the same breath: the caller's queueing
            // succeeds, the completion says why nothing will be applied.
            self.metrics.enqueued += 1;
            self.metrics.quiesced_before_dispatch += 1;
            self.quiesced_keys.push(key);
            self.quiesced_completions.push(SessionControlCompletion {
                key,
                failure: Some(SessionControlFailure::Quiescing),
                queue_dwell: Duration::ZERO,
                acknowledgement_latency: Duration::ZERO,
            });
            return Ok(key);
        }
        self.pending.push_back(PendingControl {
            command,
            key,
            queued_at: now,
            dispatch_eligible_at: None,
            dispatched_at: None,
        });
        self.metrics.enqueued += 1;
        self.metrics.peak_depth = self.metrics.peak_depth.max(self.pending.len());
        Ok(key)
    }

    pub fn service(
        &mut self,
        sender: &impl SessionControlSender,
        receiver: &Receiver<XAuthorityClientControlAck>,
        now: Instant,
        completions: &mut Vec<SessionControlCompletion>,
    ) -> Result<(), SessionControlFailure> {
        self.service_when(sender, receiver, now, completions, true)
    }

    pub fn service_when(
        &mut self,
        sender: &impl SessionControlSender,
        receiver: &Receiver<XAuthorityClientControlAck>,
        now: Instant,
        completions: &mut Vec<SessionControlCompletion>,
        dispatch_ready: bool,
    ) -> Result<(), SessionControlFailure> {
        completions.append(&mut self.quiesced_completions);
        self.retire_revoked(now, completions);
        self.receive_acknowledgements(receiver, now, completions)?;
        if dispatch_ready {
            for pending in &mut self.pending {
                if pending.dispatched_at.is_none() && pending.dispatch_eligible_at.is_none() {
                    pending.dispatch_eligible_at = Some(now);
                }
            }
        }
        self.expire(now, completions);
        if dispatch_ready {
            self.dispatch(sender, now)?;
        }
        Ok(())
    }

    pub fn revoke_client(
        &mut self,
        client: XServerFrontendClientId,
        now: Instant,
        completions: &mut Vec<SessionControlCompletion>,
    ) {
        self.revoked_clients.insert(client);
        self.retire_revoked(now, completions);
    }

    pub fn observe_recovered_timeout(&mut self) {
        self.metrics.recovered_timeouts += 1;
    }

    /// The session is on its way out after a successful primary exit and the
    /// frontend is draining (t187): every pending control is retired as
    /// `Quiescing`, reported by the next service, and so is anything enqueued
    /// from now on. Nothing here is a failure of the session.
    pub fn begin_quiescence(&mut self, now: Instant) {
        self.quiescing = true;
        while let Some(pending) = self.pending.pop_front() {
            if pending.dispatched_at.is_some() {
                self.metrics.quiesced_in_flight += 1;
            } else {
                self.metrics.quiesced_before_dispatch += 1;
            }
            self.quiesced_keys.push(pending.key);
            self.quiesced_completions.push(SessionControlCompletion {
                key: pending.key,
                failure: Some(SessionControlFailure::Quiescing),
                queue_dwell: pending
                    .dispatched_at
                    .unwrap_or(now)
                    .saturating_duration_since(pending.queued_at),
                acknowledgement_latency: pending
                    .dispatched_at
                    .map_or(Duration::ZERO, |sent| now.saturating_duration_since(sent)),
            });
        }
    }

    pub const fn is_quiescing(&self) -> bool {
        self.quiescing
    }

    fn retire_revoked(&mut self, now: Instant, completions: &mut Vec<SessionControlCompletion>) {
        let mut index = 0;
        while index < self.pending.len() {
            if !self
                .revoked_clients
                .contains(&self.pending[index].key.client)
            {
                index += 1;
                continue;
            }
            let pending = self.pending.remove(index).expect("located revoked control");
            self.metrics.disconnected += 1;
            completions.push(SessionControlCompletion {
                key: pending.key,
                failure: Some(SessionControlFailure::ClientDisconnected),
                queue_dwell: pending
                    .dispatched_at
                    .unwrap_or(now)
                    .saturating_duration_since(pending.queued_at),
                acknowledgement_latency: pending
                    .dispatched_at
                    .map_or(Duration::ZERO, |sent| now.saturating_duration_since(sent)),
            });
        }
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn has_non_configure_pending(&self) -> bool {
        self.pending
            .iter()
            .any(|pending| pending.key.kind != XAuthorityControlKind::ConfigureSurface)
    }

    pub fn metrics(&self) -> SessionControlMetrics {
        self.metrics
    }

    fn receive_acknowledgements(
        &mut self,
        receiver: &Receiver<XAuthorityClientControlAck>,
        now: Instant,
        completions: &mut Vec<SessionControlCompletion>,
    ) -> Result<(), SessionControlFailure> {
        loop {
            let acknowledgement = match receiver.try_recv() {
                Ok(acknowledgement) => acknowledgement,
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(SessionControlFailure::Disconnected),
            };
            let key = SessionControlKey::from_ack(acknowledgement);
            // Exact connection identity is never reused within a frontend.
            // Nothing from a revoked endpoint can restore control authority.
            if self.revoked_clients.contains(&key.client) {
                continue;
            }
            // A control retired by quiescence may still be answered by a
            // frontend on its way out; the answer changes nothing (t187).
            if self.quiesced_keys.contains(&key) {
                continue;
            }
            let Some(index) = self
                .pending
                .iter()
                .position(|pending| pending.key == key && pending.dispatched_at.is_some())
            else {
                self.metrics.unexpected += 1;
                return Err(SessionControlFailure::UnexpectedAcknowledgement);
            };
            let pending = self.pending.remove(index).expect("located pending control");
            let dispatched_at = pending.dispatched_at.expect("matched dispatched control");
            let queue_dwell = dispatched_at.duration_since(pending.queued_at);
            let acknowledgement_latency = now.duration_since(dispatched_at);
            self.metrics.max_queue_dwell = self.metrics.max_queue_dwell.max(queue_dwell);
            self.metrics.max_acknowledgement_latency = self
                .metrics
                .max_acknowledgement_latency
                .max(acknowledgement_latency);
            let failure =
                if acknowledgement.acknowledgement.outcome == XAuthorityControlOutcome::Delivered {
                    self.metrics.delivered += 1;
                    None
                } else {
                    let failure =
                        SessionControlFailure::Rejected(acknowledgement.acknowledgement.outcome);
                    if failure.is_stale_target_for(key.kind) {
                        self.metrics.stale_targets_retired += 1;
                    } else {
                        self.metrics.rejected += 1;
                    }
                    Some(failure)
                };
            completions.push(SessionControlCompletion {
                key,
                failure,
                queue_dwell,
                acknowledgement_latency,
            });
        }
    }

    fn expire(&mut self, now: Instant, completions: &mut Vec<SessionControlCompletion>) {
        let mut index = 0;
        while index < self.pending.len() {
            let pending = self.pending[index];
            let deadline_crossed = pending.dispatched_at.map_or_else(
                || {
                    pending.dispatch_eligible_at.is_some_and(|eligible_at| {
                        now.duration_since(eligible_at) >= SESSION_CONTROL_QUEUE_TIMEOUT
                    })
                },
                |dispatched_at| {
                    now.duration_since(dispatched_at) >= SESSION_CONTROL_ACKNOWLEDGEMENT_TIMEOUT
                },
            );
            if !deadline_crossed {
                index += 1;
                continue;
            }
            let pending = self.pending.remove(index).expect("pending index exists");
            self.metrics.timed_out += 1;
            completions.push(SessionControlCompletion {
                key: pending.key,
                failure: Some(SessionControlFailure::TimedOut),
                queue_dwell: pending
                    .dispatched_at
                    .unwrap_or(now)
                    .duration_since(pending.queued_at),
                acknowledgement_latency: pending
                    .dispatched_at
                    .map_or(Duration::ZERO, |sent| now.duration_since(sent)),
            });
        }
    }

    fn dispatch(
        &mut self,
        sender: &impl SessionControlSender,
        now: Instant,
    ) -> Result<(), SessionControlFailure> {
        let focus_in_flight = self
            .pending
            .iter()
            .any(|pending| pending.key.is_focus() && pending.dispatched_at.is_some());
        let mut dispatched_focus = focus_in_flight;
        for pending in &mut self.pending {
            if pending.dispatched_at.is_some() || (pending.key.is_focus() && dispatched_focus) {
                continue;
            }
            match sender.try_send_control(pending.command) {
                Ok(()) => {
                    pending.dispatched_at = Some(now);
                    self.metrics.dispatched += 1;
                    dispatched_focus |= pending.key.is_focus();
                }
                Err(TrySendError::Full(_)) => break,
                Err(TrySendError::Disconnected(_)) => {
                    return Err(SessionControlFailure::Disconnected);
                }
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for SessionControlFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session control failure: {self:?}")
    }
}
impl std::error::Error for SessionControlFailure {}
