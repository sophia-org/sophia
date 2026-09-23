impl LiveWmSession {
    const fn max_request(&self) -> Duration {
        self.max_request
    }

    /// Requests already sent to the policy peer and not yet settled.
    ///
    /// Distinct from `pending_request_count`, which also counts causes still
    /// queued locally. At a runtime deadline only this matters: a queued cause
    /// was promised to nobody and can be dropped when the session stops, while
    /// an issued request is owed an answer. Counting the queue there never
    /// converges anyway, because a moving pointer keeps raising fresh causes
    /// for as long as the drain waits.
    fn in_flight_request_count(&self) -> usize {
        self.public
            .as_ref()
            .map_or(0, |public| usize::from(public.in_flight_request.is_some()))
    }

    fn pending_request_count(&self) -> usize {
        self.public
            .as_ref()
            .map_or(0, |public| {
                public
                .queue
                .len()
                .saturating_add(usize::from(public.in_flight_request.is_some()))
            })
    }

    /// Whether any live output shows this surface.
    ///
    /// The presentation layout is one scene across every output, so asking a
    /// single output is a different question with a different answer. Asking
    /// the primary one dropped everything the policy had placed elsewhere: on
    /// a mixed topology the extended output's surface left the scene, so its
    /// staged Present never became visible, never retired, and never committed
    /// the resize that would have placed it. The head stayed blank and its
    /// client, still waiting on that Present, stopped drawing entirely.
    fn surface_visible_on_any_output(
        &self,
        surface: SurfaceId,
        outputs: &[sophia_engine::HeadlessOutput],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let public = self.public.as_ref().ok_or("public WM state is unavailable")?;
        let ids = outputs.iter().map(|output| output.id).collect::<Vec<_>>();
        Ok(policy_projections_place_surface(
            &public.reducer.committed(),
            &ids,
            surface,
        ))
    }

    fn apply_commit_result(
        &mut self,
        mut result: LiveWmCommitResult,
        previous_focus: Option<SurfaceId>,
        _output: sophia_protocol::OutputId,
    ) -> Result<LiveWmOwnerCommit, Box<dyn std::error::Error>> {
        let settlement = result
            .policy_settlement
            .take()
            .ok_or("public WM result is missing settlement identity")?;
        self.apply_public_commit_result(result, settlement, previous_focus)
    }
}

impl LiveWmSession {
    fn topology_policy_commit_serial(&self) -> u64 {
        self.public
            .as_ref()
            .map_or(0, |public| public.reducer.commit_serial())
    }

    fn apply_public_commit_result(
        &mut self,
        result: LiveWmCommitResult,
        settlement: LivePolicySettlementIdentity,
        previous_focus: Option<SurfaceId>,
    ) -> Result<LiveWmOwnerCommit, Box<dyn std::error::Error>> {
        let public = self.public.as_mut().ok_or("public WM settlement lost its session")?;
        let scripted_action = public.in_flight_request.as_ref().is_some_and(|request| {
            matches!(request.cause, sophia_protocol::PolicyRequestCause::Action { activation_serial, .. } | sophia_protocol::PolicyRequestCause::OutputAction { activation_serial, .. }
                if public.control_tickets.contains_key(&activation_serial))
        });
        let layout_committed = result.update.commit.outcome == TransactionOutcome::Committed;
        if settlement.session_operation {
            let (transaction, request) = public
                .pending_operation
                .take()
                .ok_or("public session operation settled without a pending request")?;
            if transaction != settlement.transaction
                || request.connection_epoch != settlement.connection_epoch
                || request.request_id != settlement.request_id
            {
                return Err("public session-operation settlement identity changed".into());
            }
            let action = public.operation_actions.get(&request.operation).copied();
            let operation = public
                .session_operations
                .iter()
                .find(|operation| operation.token == request.operation);
            let valid_target = match (operation, request.target) {
                (Some(operation), Some(_)) => operation.permits_surface_target,
                (Some(_), None) => true,
                (None, _) => false,
            };
            let outcome = if layout_committed && action.is_some() && valid_target {
                sophia_protocol::PolicyProjectionOutcome::Committed
            } else {
                sophia_protocol::PolicyProjectionOutcome::RejectedInvalid
            };
            public.submit_or_defer(PolicyTransportCommand::SessionOperationOutcome {
                    transaction,
                    request_id: request.request_id,
                    outcome,
                })?;
            self.trigger_public_proof_fault(PublicPolicyFaultPoint::TerminalOutcomeQueued);
            return Ok(LiveWmOwnerCommit {
                update: result.update,
                physical_action: None,
                pointer_gesture: None,
                session_action: if outcome == sophia_protocol::PolicyProjectionOutcome::Committed {
                    action.map(|action| (transaction, action, request.target))
                } else {
                    None
                },
                workspace_projection: None,
                clear_focus: None,
            });
        }

        let prepared = public.prepared.take();
        let outcome = if layout_committed && prepared == Some(settlement) {
            let staged = public
                .staged
                .take()
                .ok_or("public projection committed without its staged reducer successor")?;
            public.reducer.commit_staged(staged)
        } else if layout_committed {
            let staged = public
                .staged
                .take()
                .ok_or("public projection settled without its staged reducer successor")?;
            public.reducer.commit_staged(staged)
        } else {
            let stale = public.staged.as_ref().is_some_and(|staged| {
                public.reducer.revalidate_staged(staged)
                    == sophia_protocol::PolicyProjectionOutcome::RejectedStale
            });
            public.staged = None;
            let timeout = public.reducer.timeout(settlement.request_id);
            if stale { sophia_protocol::PolicyProjectionOutcome::RejectedStale } else { timeout }
        };
        let proof_restart_action = if outcome
            == sophia_protocol::PolicyProjectionOutcome::Committed
        {
            match result.source {
                Some(LiveWmProposalSource::Action(action))
                    if !public.proof_restart_triggered
                        && public.proof_restart_checkpoint_before.is_none()
                        && public.proof_restart_after_action == Some(action) =>
                {
                    Some(action)
                }
                _ => None,
            }
        } else {
            None
        };
        let proof_restart_checkpoint_before = proof_restart_action
            .map(|_| policy_checkpoint_identity(&public.checkpoint_path))
            .transpose()?;
        public.submit_or_defer(PolicyTransportCommand::ProjectionOutcome {
                transaction: settlement.transaction,
                request_id: settlement.request_id,
                scene_generation: public.reducer.scene().generation,
                outcome,
                expect_session_operation: settlement.expect_session_operation,
            })?;
        if let Some(request) = public.in_flight_request.as_ref()
            && let sophia_protocol::PolicyRequestCause::Action { activation_serial, action } | sophia_protocol::PolicyRequestCause::OutputAction { activation_serial, action, .. } = request.cause
        {
            let (target_output, target_generation) = match request.cause {
                sophia_protocol::PolicyRequestCause::OutputAction { output, output_generation, .. } => (output.raw(), output_generation),
                _ => (0, 0),
            };
            crate::session_println!(
                "sophia_shell_action_policy schema=1 policy_connection_epoch={} activation_serial={} action={} transaction={} request_id={} indicator_generation={} outcome={:?} target_output={} target_generation={}",
                public.connection_epoch, activation_serial, action.raw(),
                settlement.transaction.raw(), settlement.request_id,
                public.reducer.indicator_publication().generation, outcome, target_output, target_generation,
            );
        }
        public.settle_public_projection(outcome);
        crate::session_println!(
            "sophia_live_wm_chrome schema=2 status=settled transaction={} request_id={} scene_generation={} outcome={outcome:?}",
            settlement.transaction.raw(),
            settlement.request_id,
            public.reducer.scene().generation,
        );
        if outcome != sophia_protocol::PolicyProjectionOutcome::Committed
            || !settlement.expect_session_operation
        {
            public.expected_operation_slot = None;
        }
        if outcome == sophia_protocol::PolicyProjectionOutcome::Committed {
            public.active_output = public.reducer.scene().active_output;
        }
        // Per-output focus is remembered workspace state. An empty active
        // output has no seat focus, even when another output remembers a window.
        // Leaving the old client focused would send typing and launches to
        // different outputs and make a click on that client skip the handoff.
        let clear_focus = if outcome == sophia_protocol::PolicyProjectionOutcome::Committed
            && public.reducer.committed().iter().any(|projection| {
                projection.output == public.active_output && projection.focus.is_none()
            })
        {
            crate::session_println!(
                "sophia_live_session_focus schema=1 status=cleared reason=active_output_empty output={} surface={} generation={} transaction={}",
                public.active_output.raw(), previous_focus.map_or(0, |surface| surface.index()),
                previous_focus.map_or(0, |surface| surface.generation()), settlement.transaction.raw(),
            );
            previous_focus.map(|surface| (settlement.transaction, surface))
        } else {
            None
        };
        if let (Some(action), Some(before)) =
            (proof_restart_action, proof_restart_checkpoint_before)
        {
            public.proof_restart_checkpoint_before = Some(before);
            crate::session_println!(
                "sophia_live_wm schema=4 status=proof_restart_armed adapter=sophia_wm_v1 boundary=checkpoint_replace action={}",
                action.raw(),
            );
        }
        if outcome == sophia_protocol::PolicyProjectionOutcome::Committed {
            self.mark_committed();
        } else {
            self.stale_responses = self.stale_responses.saturating_add(1);
        }
        let physical_action = if !scripted_action && outcome == sophia_protocol::PolicyProjectionOutcome::Committed {
            match result.source {
                Some(LiveWmProposalSource::Action(action)) => Some(action),
                _ => None,
            }
        } else {
            None
        };
        self.trigger_public_proof_fault(PublicPolicyFaultPoint::TerminalOutcomeQueued);
        Ok(LiveWmOwnerCommit {
            update: result.update,
            physical_action,
            pointer_gesture: None,
            session_action: None,
            workspace_projection: None,
            clear_focus,
        })
    }
}

impl Drop for LiveWmSession {
    fn drop(&mut self) {
        self.control_restart.take();
        let _ = self.supervisor.terminate();
        self.control_lifetime.take();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// The output whose committed projection places the surface, if any does.
///
/// None is an ordinary state, not a broken one: a policy-managed surface has
/// no placement between mapping and its first committed layout, while a
/// scrolling layout holds it out of view, or between the transaction that
/// removed it and the one that brings it back. A pointer gesture that arrives
/// in that gap has nowhere to go and is dropped; it is never a reason to end
/// the session, which is what a Super+button on a freshly tiled column did.
fn committed_output_placing(
    projections: &[sophia_protocol::PolicyOutputProjection],
    surface: SurfaceId,
) -> Option<sophia_protocol::OutputId> {
    projections
        .iter()
        .find(|projection| {
            projection
                .placements
                .iter()
                .any(|placement| placement.surface == surface)
        })
        .map(|projection| projection.output)
}

/// Whether any of these outputs places the surface in its committed projection.
fn policy_projections_place_surface(
    projections: &[sophia_protocol::PolicyOutputProjection],
    outputs: &[sophia_protocol::OutputId],
    surface: SurfaceId,
) -> bool {
    projections.iter().any(|projection| {
        outputs.contains(&projection.output)
            && projection
                .placements
                .iter()
                .any(|placement| placement.surface == surface)
    })
}
