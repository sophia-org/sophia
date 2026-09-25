use super::*;
use sophia_engine::PresentedContentDismissal;

#[derive(Clone, Debug)]
pub(super) struct PendingDismissal {
    pub action: ContentAction,
    pub deadline_msec: u64,
    pub acknowledged: bool,
    pub notification_sent: bool,
    cancellation_sent: bool,
    withdrawal_error_reported: bool,
}

impl ContentActionLedger {
    pub(super) fn issue_dismissal(
        &mut self,
        popout: PresentedContentDismissal,
        now: u64,
        limits: &sophia_protocol::ContentLimits,
        transaction: TransactionId,
        transport: &mut ShellTransportConnection<'_>,
    ) -> Result<Option<u64>, ShellTransportError> {
        if let Some(pending) = self.dismissals.iter().find(|pending| {
            pending.action.grant == popout.grant
                && pending.action.output == popout.output
                && pending.action.allocation == popout.allocation
        }) {
            // Repeated outside presses cannot renew the withdrawal deadline.
            return Ok(pending.notification_sent.then_some(pending.action.event_id));
        }
        if self.dismissals.len() == self.dismissals.capacity() {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        // Withdrawal is an Engine obligation even when the bounded wire queue
        // cannot admit a notification. Keep its original deadline independently.
        let notification_sent = self.live.len()
            + self
                .dismissals
                .iter()
                .filter(|p| p.notification_sent)
                .count()
            < limits.max_pending_actions as usize
            && transport.content_action_capacity_available();
        let event_id = if notification_sent {
            self.next_event_id
        } else {
            0
        };
        let next = event_id
            .checked_add(1)
            .ok_or(ShellTransportError::InvalidConnectionEpoch)?;
        let deadline_msec = now
            .checked_add(u64::from(limits.action_ack_timeout_ms))
            .ok_or(ShellTransportError::InvalidConnectionEpoch)?;
        let action = ContentAction {
            grant: popout.grant,
            output: popout.output,
            candidate_generation: popout.candidate_generation,
            presentation_epoch: popout.presentation_epoch,
            interaction_generation: popout.interaction_generation,
            allocation: popout.allocation,
            target_id: 0,
            target_generation: 0,
            action_id: 0,
            event_id,
            kind: 2,
            reason: ContentReason::None as u16,
        };
        if notification_sent {
            transport.send_content_action(transaction, &action)?;
            self.next_event_id = next;
            self.issued_high_water = event_id;
        }
        self.dismissals.push(PendingDismissal {
            action,
            deadline_msec,
            acknowledged: false,
            notification_sent,
            cancellation_sent: false,
            withdrawal_error_reported: false,
        });
        Ok(notification_sent.then_some(event_id))
    }

    pub(in crate::live_session::metadata_shell) fn dismissal_expired(
        &self,
        allocation: sophia_protocol::ContentAllocationId,
        now: u64,
    ) -> bool {
        self.dismissals
            .iter()
            .any(|p| p.action.allocation == allocation && now >= p.deadline_msec)
    }
}

impl super::super::LiveContentSession {
    pub(in crate::live_session::metadata_shell) fn issue_presented_dismissal(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        popout: PresentedContentDismissal,
        runtime: &sophia_backend_live::LiveProductionVisualRuntime,
        transaction: &mut dyn FnMut() -> Result<TransactionId, Box<dyn std::error::Error>>,
    ) -> Result<Option<u64>, Box<dyn std::error::Error>> {
        if !self.input_requested || transport.content_grant() != Some(popout.grant) {
            return Ok(None);
        }
        while self.observe_presentation(transport, runtime)? {}
        let current = runtime
            .input_projections()
            .iter()
            .flat_map(|p| &p.content)
            .any(|binding| {
                binding.authority_current
                    && binding.grant == popout.grant
                    && binding.output == popout.output
                    && binding.candidate_generation == popout.candidate_generation
                    && binding.presentation_epoch == popout.presentation_epoch
                    && binding.interaction_generation == popout.interaction_generation
                    && binding
                        .popouts
                        .iter()
                        .any(|p| p.allocation == popout.allocation)
            });
        if !current
            || !self.presented.get(&popout.output).is_some_and(|p| {
                p.grant == popout.grant
                    && p.candidate_generation == popout.candidate_generation
                    && p.presentation_epoch == popout.presentation_epoch
            })
        {
            return Ok(None);
        }
        let limits = transport
            .content_limits()
            .cloned()
            .ok_or("content limits unavailable")?;
        Ok(self.actions.issue_dismissal(
            popout,
            self.now_msec(),
            &limits,
            transaction()?,
            transport,
        )?)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::live_session::metadata_shell) fn service_dismissals(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_backend_live::LiveProductionCpuScene,
        mut native: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
        now: u64,
        transaction: &mut dyn FnMut() -> Result<TransactionId, Box<dyn std::error::Error>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut index = 0;
        while index < self.actions.dismissals.len() {
            let pending = &mut self.actions.dismissals[index];
            let action = pending.action.clone();
            let still_presented = runtime
                .input_projections()
                .iter()
                .flat_map(|p| &p.content)
                .any(|b| {
                    b.grant == action.grant
                        && b.output == action.output
                        && b.popouts.iter().any(|p| p.allocation == action.allocation)
                });
            if still_presented && now < pending.deadline_msec {
                index += 1;
                continue;
            }
            if pending.notification_sent && !pending.acknowledged && !pending.cancellation_sent {
                let mut cancel = action.clone();
                cancel.kind = ACTION_CANCEL;
                cancel.reason = ContentReason::Stale as u16;
                transport.send_content_action(transaction()?, &cancel)?;
                pending.cancellation_sent = true;
            }
            // ACK is receipt, not proof that the peer withdrew anything.
            let withdrawn = match runtime.withdraw_shell_popout(
                action.grant,
                action.output,
                action.allocation,
                scene,
                native.as_deref_mut(),
            ) {
                Ok(withdrawn) => withdrawn,
                Err(error) => {
                    if !pending.withdrawal_error_reported {
                        crate::session_eprintln!(
                            "sophia_live_shell_content schema=1 status=withdrawal_deferred output={} reason={error}",
                            action.output.id
                        );
                        pending.withdrawal_error_reported = true;
                    }
                    false
                }
            };
            if !withdrawn {
                index += 1;
                continue;
            }
            if transport
                .content_allocation_snapshots()
                .iter()
                .any(|a| a.allocation == action.allocation)
            {
                let invalidated = transport.invalidate_content_allocation(
                    transaction()?,
                    action.allocation,
                    ContentReason::AllocationLost,
                );
                if invalidated == Err(ShellTransportError::ContentQueueSaturated) {
                    index += 1;
                    continue;
                }
                invalidated?;
            }
            self.actions.dismissals.remove(index);
        }
        Ok(())
    }
}

impl super::super::super::LiveMetadataShell {
    pub(in crate::live_session) fn issue_content_dismissal(
        &mut self,
        popout: PresentedContentDismissal,
        runtime: &sophia_backend_live::LiveProductionVisualRuntime,
    ) -> Result<Option<u64>, Box<dyn std::error::Error>> {
        let next = &mut self.next_transaction;
        self.content.issue_presented_dismissal(
            &mut self.transport.connection(),
            popout,
            runtime,
            &mut || super::super::super::take_shell_transaction(next),
        )
    }
}
