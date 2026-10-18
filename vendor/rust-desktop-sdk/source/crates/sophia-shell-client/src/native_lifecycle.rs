//! Ordered native launcher lifecycle. Server records decide grants, resource
//! acceptance and presentation; local deadlines are scheduling hints only.
use crate::{
    Admission, ContentReservation, Custody, NativeLauncherObservation, ShellClientError,
    ShellConnection, Ticket,
};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

mod image;
mod observe;
mod resources;
pub use resources::{NativeAllocationParams, NativeResourceState};
mod pacing;
pub use pacing::NativeScene;
mod action;
pub use action::{NativeActionReservation, NativeActionStatus};
mod session;
pub use session::{NativeLauncherSession, NativeObligations, NativeSessionResponse};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeLifecycleState {
    Live,
    Ended,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeInputStatus {
    pub current: bool,
    pub editable: bool,
    pub can_activate: bool,
    /// Earlier scheduling deadline in the host CLOCK_MONOTONIC domain, not
    /// a guarantee of arrival before Session's own deadline.
    pub ack_due_ms: u64,
}

/// Metadata of the last Presented candidate in the current opening. It is
/// evidence of presentation, not a claim that its focus is still eligible.
pub struct NativePresentation<'a> {
    pub transaction: TransactionId,
    pub ticket: Ticket,
    pub opening: u64,
    pub generation: u64,
    pub output: ContentOutputId,
    pub allocation: ContentAllocationId,
    pub catalog_generation: u64,
    pub state_revision: u64,
    pub interaction_generation: u64,
    pub presentation_epoch: u64,
    pub selected: u16,
    pub rows: &'a [u16],
}

#[derive(Clone, Copy)]
struct Candidate {
    transaction: TransactionId,
    ticket: Ticket,
    opening: u64,
    generation: u64,
    output: ContentOutputId,
    allocation: ContentAllocationId,
    catalog: u64,
    revision: u64,
    interaction: u64,
    presentation: u64,
    selected: u16,
    rows: [u16; 32],
    row_count: usize,
}

/// Feed observations in wire order, exactly once. A retained input or action
/// owing an ACK must be answered before another observation is fed.
/// Re-read `input()` when local admission is busy; it never reapplies an edit.
/// Allocation/resource and frame-demand helpers track their own custody. Use
/// `present_native_scene` for those checks; the lower-level candidate method
/// only registers presentation metadata. Use `prepare_native_action` for
/// pointer responses and `prepare_native_input` for keyboard responses.
pub struct NativeLauncherLifecycle {
    limits: ContentLimits,
    state: NativeLifecycleState,
    opening: Option<NativeLauncherOpening>,
    last_opening: u64,
    revision: u64,
    catalog: u64,
    facts: u64,
    catalog_announced: u64,
    facts_announced: u64,
    candidate_high_water: u64,
    pending: Option<Candidate>,
    shown: Option<Candidate>,
    focus: Option<NativeLauncherBinding>,
    launched: bool,
    input: Option<(NativeLauncherInput, NativeInputStatus)>,
    action: Option<(ContentAction, NativeActionStatus)>,
    activation: Option<(TransactionId, NativeLauncherActivation, Ticket)>,
    acks: [Option<Ticket>; 16],
    lost_acks: u8,
    allocation: resources::Allocation,
    resources: [Option<resources::Resource>; 16],
    upload: Option<resources::Upload>,
    pacing: pacing::Pacing,
}

impl NativeLauncherLifecycle {
    pub fn new(limits: ContentLimits) -> Result<Self, ShellClientError> {
        limits
            .validate()
            .map_err(|_| ShellClientError::InvalidArgument("native limits"))?;
        Ok(Self {
            limits,
            state: NativeLifecycleState::Live,
            opening: None,
            last_opening: 0,
            revision: 0,
            catalog: 0,
            facts: 0,
            catalog_announced: 0,
            facts_announced: 0,
            candidate_high_water: 0,
            pending: None,
            shown: None,
            focus: None,
            launched: false,
            input: None,
            action: None,
            activation: None,
            acks: [None; 16],
            lost_acks: 0,
            allocation: resources::Allocation::default(),
            resources: [None; 16],
            upload: None,
            pacing: pacing::Pacing::default(),
        })
    }
    pub fn state(&self) -> NativeLifecycleState {
        self.state
    }
    pub fn opening(&self) -> Option<&NativeLauncherOpening> {
        self.opening.as_ref()
    }
    pub fn focus(&self) -> Option<&NativeLauncherBinding> {
        self.focus.as_ref()
    }
    pub fn presented(&self) -> Option<NativePresentation<'_>> {
        self.shown.as_ref().map(|v| NativePresentation {
            transaction: v.transaction,
            ticket: v.ticket,
            opening: v.opening,
            generation: v.generation,
            output: v.output,
            allocation: v.allocation,
            catalog_generation: v.catalog,
            state_revision: v.revision,
            interaction_generation: v.interaction,
            presentation_epoch: v.presentation,
            selected: v.selected,
            rows: &v.rows[..v.row_count],
        })
    }
    pub fn revision(&self) -> u64 {
        if self.opening.is_some() {
            self.revision
        } else {
            0
        }
    }
    pub fn input(&self) -> Option<(&NativeLauncherInput, NativeInputStatus)> {
        self.input.as_ref().map(|(input, status)| (input, *status))
    }
    pub fn lost_acks(&self) -> u8 {
        self.lost_acks
    }

    fn live(&self) -> Result<(), ShellClientError> {
        match self.state {
            NativeLifecycleState::Live => Ok(()),
            NativeLifecycleState::Ended => Err(ShellClientError::PeerClosed),
            NativeLifecycleState::Failed => {
                Err(ShellClientError::Protocol("native lifecycle failed"))
            }
        }
    }
    fn end(&mut self, state: NativeLifecycleState) {
        self.state = state;
        self.opening = None;
        self.shown = None;
        self.focus = None;
        self.input = None;
        self.action = None;
        self.allocation = resources::Allocation::default();
        self.resources.fill(None);
        self.upload = None;
        self.pacing = pacing::Pacing::default();
    }
    fn grant(&self, grant: ContentGrant) -> Result<(), ShellClientError> {
        if grant != self.limits.grant {
            return Err(ShellClientError::Protocol("native grant"));
        }
        Ok(())
    }
    fn catalog_current(&self) -> bool {
        self.opening.is_some_and(|v| {
            v.catalog_generation == self.catalog && self.catalog == self.catalog_announced
        })
    }

    /// Apply one decoded observation. Identity/correlation errors fail closed;
    /// attempting to skip an input returns a local error without changing it.
    /// The caller must preserve order across native and shared content records.
    pub fn observe(&mut self, value: &NativeLauncherObservation) -> Result<(), ShellClientError> {
        self.live()?;
        if self.input.is_some() || self.action.as_ref().is_some_and(|(_, s)| s.ack_owed) {
            return Err(ShellClientError::InvalidArgument("native event awaits ACK"));
        }
        self.action = None;
        if let Err(error) = self.apply(value) {
            self.end(NativeLifecycleState::Failed);
            return Err(error);
        }
        Ok(())
    }

    /// Reconcile submit outcomes after driving the issuing connection. A
    /// refused ACK is counted lost, never resent. Submitted proves custody,
    /// not presentation or application startup. No automatic reconnect.
    pub fn service(&mut self, connection: &ShellConnection) -> Result<(), ShellClientError> {
        self.live()?;
        if connection.connection_epoch() != self.limits.grant.connection_epoch {
            return Err(ShellClientError::InvalidArgument("native connection"));
        }
        if connection.io_interest().is_err() {
            self.end(NativeLifecycleState::Ended);
            return Err(ShellClientError::PeerClosed);
        }
        for slot in &mut self.acks {
            if let Some(ticket) = *slot {
                let outcome = connection.custody(ticket);
                if !matches!(outcome, Some(Custody::Queued | Custody::InFlight)) {
                    if matches!(outcome, Some(Custody::Refused(_) | Custody::DroppedUnsent)) {
                        self.lost_acks = self.lost_acks.saturating_add(1);
                    }
                    *slot = None;
                }
            }
        }
        if let Some(pending) = self.pending
            && matches!(
                connection.custody(pending.ticket),
                Some(Custody::Refused(_) | Custody::DroppedUnsent)
            )
        {
            self.pending = None;
            self.candidate_refused();
        }
        if let Some((_, _, ticket)) = self.activation
            && matches!(
                connection.custody(ticket),
                Some(Custody::Refused(_) | Custody::DroppedUnsent)
            )
        {
            self.activation = None;
        }
        self.service_resources(connection);
        self.service_pacing(connection);
        Ok(())
    }
}

/// Prepared input response: perform at most one edit after obtaining this
/// handle. `commit` clears the retained input and records its custody tickets;
/// dropping the handle cancels capacity and leaves the input available.
pub struct NativeInputReservation<'a> {
    reservation: ContentReservation<'a>,
    lifecycle: &'a mut NativeLauncherLifecycle,
    activation: Option<(TransactionId, NativeLauncherActivation)>,
    ack_slot: usize,
}
impl NativeInputReservation<'_> {
    fn apply(
        mut self,
        edit: impl FnOnce(&NativeLauncherInput) -> bool,
    ) -> Result<Admission, ShellClientError> {
        // Prepare both dispositions before application code runs. They keep
        // the exact header/transaction and have equal wire size, so choosing
        // after the edit cannot allocate or lose the reserved queue credit.
        let prepared = self.reservation.prepared.as_ref().unwrap();
        let bytes = prepared.last_bytes();
        let header = decode_shell_file_record(bytes, ShellFileClass::Candidate)
            .map_err(sophia_shell_protocol::shell_files::ShellFilePayloadError::from)?
            .header;
        let mut value =
            decode_shell_file_native_launcher_transaction(bytes, ShellFileKind::NativeInputAck)?;
        let ShellNativeLauncherRecord::InputAck(ack) = &mut value.record else {
            unreachable!()
        };
        ack.disposition = 2;
        let stale =
            encode_shell_file_native_launcher_transaction(header, &value)?.into_boxed_slice();
        assert_eq!(stale.len(), bytes.len());
        let (input, status) = self.lifecycle.input.as_ref().unwrap();
        let edited = if status.editable {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| edit(input))) {
                Ok(value) => value,
                Err(panic) => {
                    // The edit may have happened before unwinding. Never let
                    // a caught panic make this event eligible for a second edit.
                    self.lifecycle.end(NativeLifecycleState::Failed);
                    std::panic::resume_unwind(panic);
                }
            }
        } else {
            false
        };
        if !edited && self.activation.is_none() {
            self.reservation
                .prepared
                .as_mut()
                .unwrap()
                .replace_last_same_size(stale);
        }
        Ok(self.commit())
    }
    pub fn status(&self) -> NativeInputStatus {
        self.lifecycle.input.as_ref().unwrap().1
    }
    pub fn commit(self) -> Admission {
        let admission = self.reservation.commit();
        if let Some((tx, activation)) = self.activation {
            self.lifecycle.activation = Some((tx, activation, admission.first));
        }
        self.lifecycle.acks[self.ack_slot] =
            Some(Ticket(admission.first.0 + u64::from(admission.count) - 1));
        self.lifecycle.input = None;
        admission
    }
}

impl ShellConnection {
    /// Reserve before calling `edit`, once, and only for editable input. True
    /// means consumed; false means stale. Activation is consumed without an
    /// edit callback. A callback panic fails the lifecycle closed and resumes
    /// unwinding, because its external effects cannot safely be replayed.
    pub fn apply_native_input(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        ack_transaction: TransactionId,
        activation_transaction: Option<TransactionId>,
        edit: impl FnOnce(&NativeLauncherInput) -> bool,
    ) -> Result<Admission, ShellClientError> {
        self.prepare_native_input(lifecycle, ack_transaction, 1, activation_transaction)?
            .apply(edit)
    }
    /// Atomically admit the exact candidate and its presentation metadata.
    /// The caller supplies a currently valid allocation, accepted resources and
    /// permit; this helper checks opening/catalog/facts/revision, not their
    /// resource or pacing lifecycle. Failed admission registers nothing.
    pub fn enqueue_native_candidate_with_lifecycle(
        &mut self,
        lifecycle: &mut NativeLauncherLifecycle,
        value: &ShellFileNativeCandidate,
    ) -> Result<Admission, ShellClientError> {
        lifecycle.live()?;
        let v = &value.candidate;
        let c = &v.candidate;
        let opening = lifecycle
            .opening
            .ok_or(ShellClientError::InvalidArgument("native opening absent"))?;
        if lifecycle.limits.grant != c.grant
            || self.connection_epoch() != c.grant.connection_epoch
            || lifecycle.pending.is_some()
            || lifecycle.launched
            || v.opening != opening.opening
            || c.output != opening.output
            || !lifecycle.catalog_current()
            || v.catalog_generation != lifecycle.catalog
            || c.facts_generation != lifecycle.facts
            || lifecycle.facts != lifecycle.facts_announced
            || lifecycle.facts == 0
            || v.state_revision != lifecycle.revision
            || c.candidate_generation <= lifecycle.candidate_high_water
            || c.interaction_generation != 1
            || c.surfaces.len() != 1
            || v.rows.len() > 32
        {
            return Err(ShellClientError::InvalidArgument(
                "native candidate context",
            ));
        }
        let mut metadata = Candidate {
            transaction: value.transaction,
            ticket: Ticket(0),
            opening: v.opening,
            generation: c.candidate_generation,
            output: c.output,
            allocation: c.surfaces[0].allocation,
            catalog: v.catalog_generation,
            revision: v.state_revision,
            interaction: c.interaction_generation,
            presentation: 0,
            selected: v.selected,
            rows: [0; 32],
            row_count: v.rows.len(),
        };
        metadata.rows[..v.rows.len()].copy_from_slice(&v.rows);
        let admission = self.enqueue_native_candidate_tracked(value)?;
        metadata.ticket = admission.first;
        lifecycle.candidate_high_water = metadata.generation;
        lifecycle.pending = Some(metadata);
        Ok(admission)
    }

    /// Reserve the retained input's exact ACK and optional keyboard activation.
    /// Refuse activation unless the reducer proved eligibility. Choose the
    /// disposition before the edit (1 consumed, 2 stale). On capacity failure
    /// do not edit; retry against the retained input after servicing I/O.
    pub fn prepare_native_input<'a>(
        &'a mut self,
        lifecycle: &'a mut NativeLauncherLifecycle,
        ack_transaction: TransactionId,
        disposition: u16,
        activation_transaction: Option<TransactionId>,
    ) -> Result<NativeInputReservation<'a>, ShellClientError> {
        lifecycle.service(self)?;
        let (input, status) = lifecycle
            .input
            .as_ref()
            .ok_or(ShellClientError::InvalidArgument("no native input"))?;
        if activation_transaction.is_some() && !status.can_activate {
            return Err(ShellClientError::InvalidArgument(
                "native activation ineligible",
            ));
        }
        let ack_slot = lifecycle
            .acks
            .iter()
            .position(Option::is_none)
            .ok_or(ShellClientError::QueueSaturated)?;
        let ack = NativeLauncherInputAck {
            event: input.event,
            disposition,
        };
        let activation = activation_transaction.map(|tx| {
            (
                tx,
                NativeLauncherActivation {
                    event: input.event,
                    cause: 1,
                    slot: lifecycle.shown.unwrap().selected,
                },
            )
        });
        let reservation = self.reserve_native_input_response(ack_transaction, ack, activation)?;
        Ok(NativeInputReservation {
            reservation,
            lifecycle,
            activation,
            ack_slot,
        })
    }
}
