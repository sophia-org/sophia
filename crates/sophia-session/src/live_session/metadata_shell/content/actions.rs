use std::collections::BTreeMap;

use sophia_engine::PresentedContentTarget;
use sophia_protocol::{
    ContentAction, ContentActionAck, ContentReason, ShellIndicatorActivation, TransactionId,
};
use sophia_runtime::{ShellSessionTransport, ShellTransportError};

const ACTION_ACTIVATE: u16 = 1;
const ACTION_CANCEL: u16 = 3;
const ACK_CONSUMED: u16 = 1;
const ACK_REJECTED_STALE: u16 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AckState {
    Awaiting,
    Consumed,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActivationState {
    Awaiting,
    WmAdmitted,
    Rejected,
}

#[derive(Clone, Debug)]
struct PendingAction {
    action: ContentAction,
    target: PresentedContentTarget,
    deadline_msec: u64,
    ack: AckState,
    activation: ActivationState,
    cancel_sent: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ContentActionLedger {
    next_event_id: u64,
    issued_high_water: u64,
    live: BTreeMap<u64, PendingAction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::live_session) enum LinkedIndicatorAdmission {
    Eligible,
    Stale,
}

impl ContentActionLedger {
    pub(super) fn reset(&mut self) {
        self.next_event_id = 1;
        self.issued_high_water = 0;
        self.live.clear();
    }

    pub(super) fn issue(
        &mut self,
        target: PresentedContentTarget,
        now_msec: u64,
        limits: &sophia_protocol::ContentLimits,
        transaction: TransactionId,
        transport: &mut ShellSessionTransport,
    ) -> Result<Option<u64>, ShellTransportError> {
        if self.live.len() >= limits.max_pending_actions as usize
            || !transport.content_action_capacity_available()
        {
            return Ok(None);
        }
        let event_id = self.next_event_id.max(1);
        let next = event_id
            .checked_add(1)
            .ok_or(ShellTransportError::InvalidConnectionEpoch)?;
        let action = action_from_target(&target, event_id, ACTION_ACTIVATE);
        let deadline_msec = now_msec
            .checked_add(u64::from(limits.action_ack_timeout_ms))
            .ok_or(ShellTransportError::InvalidConnectionEpoch)?;
        transport.send_content_action(transaction, &action)?;
        self.next_event_id = next;
        self.issued_high_water = event_id;
        self.live.insert(
            event_id,
            PendingAction {
                action,
                target,
                deadline_msec,
                ack: AckState::Awaiting,
                activation: ActivationState::Awaiting,
                cancel_sent: false,
            },
        );
        Ok(Some(event_id))
    }

    pub(super) fn service_acks(
        &mut self,
        transport: &mut ShellSessionTransport,
        now_msec: u64,
        maximum: usize,
    ) -> Result<usize, ShellTransportError> {
        let mut processed = 0;
        while processed < maximum {
            let Some((_, ack)) = transport.poll_content_action_ack()? else {
                break;
            };
            processed += 1;
            let Some(pending) = self.live.get_mut(&ack.event_id) else {
                continue;
            };
            if !ack_matches(&ack, &pending.action) || now_msec > pending.deadline_msec {
                pending.ack = AckState::Rejected;
                pending.activation = ActivationState::Rejected;
                continue;
            }
            pending.ack = match ack.disposition {
                ACK_CONSUMED => AckState::Consumed,
                ACK_REJECTED_STALE => {
                    pending.activation = ActivationState::Rejected;
                    AckState::Rejected
                }
                _ => return Err(ShellTransportError::WrongContentRecord),
            };
        }
        self.collect_terminal(now_msec);
        Ok(processed)
    }

    pub(super) fn indicator_admission(
        &self,
        activation: &ShellIndicatorActivation,
        now_msec: u64,
    ) -> LinkedIndicatorAdmission {
        if activation.event_id == 0 || activation.event_id > self.issued_high_water {
            return LinkedIndicatorAdmission::Stale;
        }
        let Some(pending) = self.live.get(&activation.event_id) else {
            return LinkedIndicatorAdmission::Stale;
        };
        if now_msec > pending.deadline_msec
            || pending.ack != AckState::Consumed
            || pending.activation != ActivationState::Awaiting
            || activation.output.raw() != pending.action.output.id
            || activation.indicator != pending.action.target_id
            || activation.action != pending.action.action_id
        {
            return LinkedIndicatorAdmission::Stale;
        }
        LinkedIndicatorAdmission::Eligible
    }

    pub(super) fn wm_admitted(&mut self, event_id: u64, now_msec: u64) {
        if let Some(pending) = self.live.get_mut(&event_id) {
            pending.activation = ActivationState::WmAdmitted;
        }
        self.collect_terminal(now_msec);
    }

    pub(super) fn wm_rejected(&mut self, event_id: u64, now_msec: u64) {
        if let Some(pending) = self.live.get_mut(&event_id) {
            pending.activation = ActivationState::Rejected;
        }
        self.collect_terminal(now_msec);
    }

    pub(super) fn cancel_stale(
        &mut self,
        presented: &[sophia_engine::PresentedContentBinding],
        now_msec: u64,
    ) -> Vec<ContentAction> {
        let mut cancellations = Vec::new();
        for pending in self.live.values_mut() {
            if pending.activation == ActivationState::WmAdmitted
                || presented.iter().any(|binding| {
                    binding
                        .targets
                        .iter()
                        .any(|target| target == &pending.target)
                })
            {
                continue;
            }
            if pending.ack == AckState::Awaiting && !pending.cancel_sent {
                cancellations.push(action_from_target(
                    &pending.target,
                    pending.action.event_id,
                    ACTION_CANCEL,
                ));
                pending.cancel_sent = true;
            }
            pending.activation = ActivationState::Rejected;
        }
        self.collect_terminal(now_msec);
        cancellations
    }

    pub(super) fn expire(&mut self, now_msec: u64) {
        for pending in self.live.values_mut() {
            if now_msec >= pending.deadline_msec
                && pending.activation != ActivationState::WmAdmitted
            {
                pending.activation = ActivationState::Rejected;
            }
        }
        self.collect_terminal(now_msec);
    }

    fn collect_terminal(&mut self, now_msec: u64) {
        self.live.retain(|_, pending| {
            if now_msec >= pending.deadline_msec {
                return false;
            }
            !matches!(
                (pending.ack, pending.activation),
                (AckState::Rejected, _)
                    | (
                        AckState::Consumed,
                        ActivationState::WmAdmitted | ActivationState::Rejected
                    )
            )
        });
    }
}

fn action_from_target(target: &PresentedContentTarget, event_id: u64, kind: u16) -> ContentAction {
    ContentAction {
        grant: target.grant,
        output: target.output,
        candidate_generation: target.candidate_generation,
        presentation_epoch: target.presentation_epoch,
        interaction_generation: target.interaction_generation,
        allocation: target.allocation,
        target_id: target.target_id,
        target_generation: target.target_generation,
        action_id: target.action_id,
        event_id,
        kind,
        reason: ContentReason::None as u16,
    }
}

fn ack_matches(ack: &ContentActionAck, action: &ContentAction) -> bool {
    ack.grant == action.grant
        && ack.output == action.output
        && ack.candidate_generation == action.candidate_generation
        && ack.presentation_epoch == action.presentation_epoch
        && ack.interaction_generation == action.interaction_generation
        && ack.allocation == action.allocation
        && ack.target_id == action.target_id
        && ack.target_generation == action.target_generation
        && ack.action_id == action.action_id
        && ack.event_id == action.event_id
}

impl super::super::LiveMetadataShell {
    pub(in crate::live_session) fn issue_content_activation(
        &mut self,
        target: PresentedContentTarget,
    ) -> Result<Option<u64>, Box<dyn std::error::Error>> {
        if !self.content.input_requested {
            return Ok(None);
        }
        let now = self.content.now_msec();
        let limits = self
            .transport
            .content_limits()
            .cloned()
            .ok_or("content limits are unavailable")?;
        let transaction = self.take_transaction()?;
        Ok(self
            .content
            .actions
            .issue(target, now, &limits, transaction, &mut self.transport)?)
    }

    pub(in crate::live_session) fn service_content_actions(
        &mut self,
        presented: &[sophia_engine::PresentedContentBinding],
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if !self.content.input_requested || self.transport.content_grant().is_none() {
            return Ok(0);
        }
        let now = self.content.now_msec();
        let maximum = self
            .transport
            .content_limits()
            .map_or(0, |limits| limits.max_frames_per_service_tick as usize);
        let mut processed = self
            .content
            .actions
            .service_acks(&mut self.transport, now, maximum)?;
        let cancellations = self.content.actions.cancel_stale(presented, now);
        for cancellation in cancellations {
            let transaction = self.take_transaction()?;
            self.transport
                .send_content_action(transaction, &cancellation)?;
            processed = processed.saturating_add(1);
        }
        self.content.actions.expire(now);
        Ok(processed)
    }

    pub(in crate::live_session) fn content_indicator_admitted_by_ledger(
        &self,
        activation: &ShellIndicatorActivation,
    ) -> bool {
        if !self.content.input_requested {
            return true;
        }
        self.content
            .actions
            .indicator_admission(activation, self.content.now_msec())
            == LinkedIndicatorAdmission::Eligible
    }

    pub(in crate::live_session) fn content_indicator_admitted(&mut self, event_id: u64) {
        let now = self.content.now_msec();
        self.content.actions.wm_admitted(event_id, now);
    }

    pub(in crate::live_session) fn content_indicator_rejected(&mut self, event_id: u64) {
        let now = self.content.now_msec();
        self.content.actions.wm_rejected(event_id, now);
    }

    pub(in crate::live_session) fn content_input_requested(&self) -> bool {
        self.content.input_requested
    }
}

#[path = "actions/tests.rs"]
mod tests;
