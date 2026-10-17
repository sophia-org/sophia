use super::*;
use crate::wire::Outbound;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeActionStatus {
    pub current: bool,
    pub ack_owed: bool,
    pub can_activate: bool,
}

impl NativeLauncherLifecycle {
    /// The retained pointer action. Invocation and dismissal require a response;
    /// cancellation owes none and is replaced by the next observation.
    pub fn action(&self) -> Option<(&ContentAction, NativeActionStatus)> {
        self.action
            .as_ref()
            .map(|(action, status)| (action, *status))
    }

    pub(super) fn observe_action(&mut self, action: &ContentAction) {
        let current = self.shown.is_some_and(|shown| {
            action.output == shown.output
                && action.allocation == shown.allocation
                && action.candidate_generation == shown.generation
                && action.presentation_epoch == shown.presentation
                && action.interaction_generation == shown.interaction
        });
        let can_activate = action.kind == 1
            && current
            && !self.launched
            && self.activation.is_none()
            && self.catalog_current()
            && self.focus.is_some_and(|focus| {
                focus.state_revision == self.revision
                    && self.shown.is_some_and(|shown| {
                        focus.candidate_generation == shown.generation
                            && focus.presentation_epoch == shown.presentation
                            && (1..=4096).contains(&action.action_id)
                            && shown.rows[..shown.row_count].contains(&(action.action_id as u16))
                    })
            });
        self.action = Some((
            action.clone(),
            NativeActionStatus {
                current,
                ack_owed: matches!(action.kind, 1 | 2),
                can_activate,
            },
        ));
    }
}

/// Prepared exact pointer response. Drop preserves the action; commit consumes
/// it and tracks each record's independent custody. No I/O or allocation at commit.
pub struct NativeActionReservation<'a> {
    reservation: ContentReservation<'a>,
    lifecycle: &'a mut NativeLauncherLifecycle,
    activation: Option<(TransactionId, NativeLauncherActivation)>,
    ack_slot: usize,
}
impl NativeActionReservation<'_> {
    pub fn commit(self) -> Admission {
        let admission = self.reservation.commit();
        if let Some((tx, activation)) = self.activation {
            self.lifecycle.activation = Some((tx, activation, admission.first));
        }
        self.lifecycle.acks[self.ack_slot] =
            Some(Ticket(admission.first.0 + u64::from(admission.count) - 1));
        self.lifecycle.action = None;
        admission
    }
}

impl ShellConnection {
    /// Prepare an exact ActionAck and optional pointer activation for the
    /// retained action. Any displayed eligible row may activate, not just the
    /// keyboard selection. Capacity failure or Drop leaves the action retained.
    pub fn prepare_native_action<'a>(
        &'a mut self,
        lifecycle: &'a mut NativeLauncherLifecycle,
        ack_transaction: TransactionId,
        disposition: u16,
        activation_transaction: Option<TransactionId>,
    ) -> Result<NativeActionReservation<'a>, ShellClientError> {
        lifecycle.service(self)?;
        let (action, status) = lifecycle
            .action
            .as_ref()
            .ok_or(ShellClientError::InvalidArgument("no native action"))?;
        if !status.ack_owed
            || !(1..=2).contains(&disposition)
            || activation_transaction
                .is_some_and(|tx| tx == ack_transaction || !status.can_activate)
        {
            return Err(ShellClientError::InvalidArgument("native action response"));
        }
        let ack_slot = lifecycle
            .acks
            .iter()
            .position(Option::is_none)
            .ok_or(ShellClientError::QueueSaturated)?;
        let activation = activation_transaction.map(|tx| {
            (
                tx,
                NativeLauncherActivation {
                    event: NativeLauncherEvent {
                        binding: lifecycle.focus.unwrap(),
                        event_id: action.event_id,
                        state_revision: lifecycle.revision,
                    },
                    cause: 2,
                    slot: action.action_id as u16,
                },
            )
        });
        let ack = ContentActionAck {
            grant: action.grant,
            output: action.output,
            candidate_generation: action.candidate_generation,
            presentation_epoch: action.presentation_epoch,
            interaction_generation: action.interaction_generation,
            allocation: action.allocation,
            target_id: action.target_id,
            target_generation: action.target_generation,
            action_id: action.action_id,
            event_id: action.event_id,
            disposition,
        };
        // Encode the whole response before reserving any queue ownership.
        // Distinct records retain independent submit and semantic outcomes.
        let units = self.wire.encode(Outbound::NativeActionResponse {
            transaction: ack_transaction,
            ack,
            activation,
        })?;
        self.wire.reserve_encoded()?;
        let prepared = self.output.prepare(units, true, &mut self.ledger)?;
        Ok(NativeActionReservation {
            reservation: ContentReservation {
                connection: self,
                prepared: Some(prepared),
            },
            lifecycle,
            activation,
            ack_slot,
        })
    }
}
