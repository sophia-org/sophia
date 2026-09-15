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

#[derive(Debug)]
pub(super) struct ContentActionLedger {
    next_event_id: u64,
    issued_high_water: u64,
    live: Vec<PendingAction>,
}

// r5 advertises at most sixteen actions. Reserve once before a connection
// can issue anything; neither issuance nor cancellation bookkeeping allocates.
const ACTION_CAPACITY: usize = 16;

impl Default for ContentActionLedger {
    fn default() -> Self {
        Self {
            next_event_id: 1,
            issued_high_water: 0,
            live: Vec::with_capacity(ACTION_CAPACITY),
        }
    }
}

impl Clone for ContentActionLedger {
    fn clone(&self) -> Self {
        let mut copy = Self {
            next_event_id: self.next_event_id,
            issued_high_water: self.issued_high_water,
            ..Self::default()
        };
        copy.live.extend(self.live.iter().cloned());
        copy
    }
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
            || self.live.len() >= self.live.capacity()
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
        self.live.push(PendingAction {
            action,
            target,
            deadline_msec,
            ack: AckState::Awaiting,
            activation: ActivationState::Awaiting,
            cancel_sent: false,
        });
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
            self.acknowledge(&ack, now_msec)?;
        }
        self.collect_terminal(now_msec);
        transport.retain_content_action_reservations(|event| {
            self.live
                .iter()
                .any(|pending| pending.action.event_id == event)
        });
        Ok(processed)
    }

    fn acknowledge(
        &mut self,
        ack: &ContentActionAck,
        now_msec: u64,
    ) -> Result<(), ShellTransportError> {
        let Some(pending) = self
            .live
            .iter_mut()
            .find(|pending| pending.action.event_id == ack.event_id)
        else {
            return Ok(());
        };
        // An event number is only a lookup key. A different presented identity
        // cannot reject or consume the real event, even when it arrives late.
        if !ack_matches(ack, &pending.action)
            || now_msec > pending.deadline_msec
            || pending.ack != AckState::Awaiting
        {
            return Ok(());
        }
        pending.ack = match ack.disposition {
            ACK_CONSUMED => AckState::Consumed,
            ACK_REJECTED_STALE => AckState::Rejected,
            _ => return Err(ShellTransportError::WrongContentRecord),
        };
        // Receipt and WM authority are orthogonal; neither ACK outcome can
        // undo an effect already admitted by the WM owner.
        Ok(())
    }

    pub(super) fn indicator_admission(
        &self,
        activation: &ShellIndicatorActivation,
        now_msec: u64,
    ) -> LinkedIndicatorAdmission {
        if activation.event_id == 0 || activation.event_id > self.issued_high_water {
            return LinkedIndicatorAdmission::Stale;
        }
        let Some(pending) = self
            .live
            .iter()
            .find(|pending| pending.action.event_id == activation.event_id)
        else {
            return LinkedIndicatorAdmission::Stale;
        };
        if now_msec > pending.deadline_msec
            || activation.connection_epoch != pending.action.grant.connection_epoch
            || pending.activation != ActivationState::Awaiting
            || activation.output.raw() != pending.action.output.id
            || activation.indicator != pending.action.target_id
            || activation.action != pending.action.action_id
        {
            return LinkedIndicatorAdmission::Stale;
        }
        LinkedIndicatorAdmission::Eligible
    }

    pub(super) fn service_indicator_request(
        &mut self,
        transport: &mut ShellSessionTransport,
        indicators: &mut super::super::indicators::LiveIndicatorState,
        input_enabled: bool,
        now_msec: u64,
        admit: impl FnOnce(
            sophia_protocol::WmActionId,
            sophia_protocol::OutputId,
        ) -> Result<
            crate::live_session::LiveWmRequestAdmission,
            Box<dyn std::error::Error>,
        >,
    ) -> Result<bool, super::super::indicators::IndicatorServiceError> {
        use super::super::indicators::IndicatorServiceError as Error;
        let Some(request) = indicators
            .poll_request(transport, input_enabled)
            .map_err(Error::Poll)?
        else {
            return Ok(false);
        };
        self.finish_indicator_request(transport, request, input_enabled, now_msec, admit)
            .map_err(Error::Completion)?;
        Ok(true)
    }

    // This is the owner-loop decision sequence, shared with the private
    // roundtrip fixture. Only WM admission is borrowed from its policy owner.
    pub(super) fn finish_indicator_request(
        &mut self,
        transport: &mut ShellSessionTransport,
        request: super::super::indicators::LiveIndicatorActivationRequest,
        input_enabled: bool,
        now_msec: u64,
        admit: impl FnOnce(
            sophia_protocol::WmActionId,
            sophia_protocol::OutputId,
        ) -> Result<
            crate::live_session::LiveWmRequestAdmission,
            Box<dyn std::error::Error>,
        >,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::live_session::LiveWmRequestAdmission as Admission;
        use sophia_protocol::ShellIndicatorActivationStatus as Status;
        let mut status = request.status;
        let mut reason = 0;
        if status == Status::Accepted
            && input_enabled
            && self.indicator_admission(&request.activation, now_msec)
                != LinkedIndicatorAdmission::Eligible
        {
            status = Status::Stale;
            reason = ContentReason::Stale as u16;
        }
        if status == Status::Accepted {
            match admit(
                sophia_protocol::WmActionId::from_raw(request.activation.action),
                request.activation.output,
            )? {
                Admission::Admitted => self.wm_admitted(request.activation.event_id, now_msec),
                Admission::RejectedCapacity => {
                    self.wm_rejected(request.activation.event_id, now_msec);
                    status = Status::Unknown;
                    reason = ContentReason::Budget as u16;
                }
                Admission::Duplicate => {
                    self.wm_rejected(request.activation.event_id, now_msec);
                    status = Status::Stale;
                    reason = ContentReason::Stale as u16;
                }
            }
        }
        // The effect is complete. Only this exact owned response can be retried
        // by the transport; never repeat the admission closure after refusal.
        transport.finish_indicator_activation(
            request.transaction,
            &request.activation,
            status,
            reason,
        )?;
        Ok(())
    }

    pub(super) fn wm_admitted(&mut self, event_id: u64, now_msec: u64) {
        if let Some(pending) = self
            .live
            .iter_mut()
            .find(|pending| pending.action.event_id == event_id)
        {
            pending.activation = ActivationState::WmAdmitted;
        }
        self.collect_terminal(now_msec);
    }

    pub(super) fn wm_rejected(&mut self, event_id: u64, now_msec: u64) {
        if let Some(pending) = self
            .live
            .iter_mut()
            .find(|pending| pending.action.event_id == event_id)
            && pending.activation != ActivationState::WmAdmitted
        {
            pending.activation = ActivationState::Rejected;
        }
        self.collect_terminal(now_msec);
    }

    /// Select one cancellation without transferring its obligation out of the
    /// ledger. A refused enqueue can retry the same identity and reserved credit.
    fn next_cancellation(
        &mut self,
        presented: &[sophia_engine::PresentedContentBinding],
        now_msec: u64,
    ) -> Option<usize> {
        self.collect_terminal(now_msec);
        for (index, pending) in self.live.iter_mut().enumerate() {
            let expired = now_msec >= pending.deadline_msec;
            let current = presented.iter().any(|binding| {
                binding
                    .targets
                    .iter()
                    .any(|target| target == &pending.target)
            });
            if !expired && (pending.activation == ActivationState::WmAdmitted || current) {
                continue;
            }
            if pending.ack == AckState::Awaiting && !pending.cancel_sent {
                return Some(index);
            }
            if pending.activation != ActivationState::WmAdmitted {
                pending.activation = ActivationState::Rejected;
            }
        }
        None
    }

    fn queue_cancellation(
        &mut self,
        index: usize,
        transaction: TransactionId,
        transport: &mut ShellSessionTransport,
    ) -> Result<(), ShellTransportError> {
        let pending = &self.live[index];
        let cancellation =
            action_from_target(&pending.target, pending.action.event_id, ACTION_CANCEL);
        transport.send_content_action(transaction, &cancellation)?;
        // Index and ownership were validated before enqueue. No allocation,
        // callback or fallible operation intervenes after FIFO ownership.
        self.cancellation_queued(index);
        Ok(())
    }

    fn cancellation_queued(&mut self, index: usize) {
        let pending = &mut self.live[index];
        pending.cancel_sent = true;
        if pending.activation != ActivationState::WmAdmitted {
            pending.activation = ActivationState::Rejected;
        }
    }

    pub(super) fn expire(&mut self, now_msec: u64) {
        for pending in self.live.iter_mut() {
            if now_msec >= pending.deadline_msec
                && pending.activation != ActivationState::WmAdmitted
            {
                pending.activation = ActivationState::Rejected;
            }
        }
        self.collect_terminal(now_msec);
    }

    fn collect_terminal(&mut self, now_msec: u64) {
        self.live.retain(|pending| {
            // Cancel itself has no ACK. Until its FIFO transfer, even an
            // expired event still owns the reserved cancellation response.
            if pending.cancel_sent {
                return false;
            }
            if pending.ack == AckState::Awaiting {
                return true;
            }
            now_msec < pending.deadline_msec && pending.activation == ActivationState::Awaiting
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
        runtime: &sophia_backend_live::LiveProductionVisualRuntime,
    ) -> Result<Option<u64>, Box<dyn std::error::Error>> {
        if !self.content.input_requested || self.transport.content_grant() != Some(target.grant) {
            return Ok(None);
        }
        // Native retirement is the only source of Presented. Publish all
        // bounded pending retirements before putting this Action on the FIFO.
        while self.observe_content_presentation(runtime)? {}
        if !self
            .content
            .presented
            .get(&target.output)
            .is_some_and(|published| {
                published.grant == target.grant
                    && published.candidate_generation == target.candidate_generation
                    && published.presentation_epoch == target.presentation_epoch
            })
        {
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
        while let Some(index) = self.content.actions.next_cancellation(presented, now) {
            let transaction = self.take_transaction()?;
            self.content
                .actions
                .queue_cancellation(index, transaction, &mut self.transport)?;
            processed = processed.saturating_add(1);
        }
        self.content.actions.expire(now);
        self.transport.retain_content_action_reservations(|event| {
            self.content
                .actions
                .live
                .iter()
                .any(|pending| pending.action.event_id == event)
        });
        Ok(processed)
    }
}

#[path = "actions/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../../tests/support/content_actions/transport_tests.rs"]
mod transport_tests;

#[cfg(test)]
#[path = "../../../../tests/support/content_actions/client_roundtrip_tests.rs"]
mod client_roundtrip_tests;

impl super::LiveContentSession {
    pub(in crate::live_session) fn service_indicator_request(
        &mut self,
        transport: &mut ShellSessionTransport,
        indicators: &mut super::super::indicators::LiveIndicatorState,
        admit: impl FnOnce(
            sophia_protocol::WmActionId,
            sophia_protocol::OutputId,
        ) -> Result<
            crate::live_session::LiveWmRequestAdmission,
            Box<dyn std::error::Error>,
        >,
    ) -> Result<bool, super::super::indicators::IndicatorServiceError> {
        // One timestamp for this bounded dispatch; ACK service has its own time.
        let now = self.now_msec();
        self.actions.service_indicator_request(
            transport,
            indicators,
            self.input_requested,
            now,
            admit,
        )
    }
}
