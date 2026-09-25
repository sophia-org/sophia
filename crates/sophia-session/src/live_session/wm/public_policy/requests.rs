impl LiveWmSession {
    fn poll_public_request(
        &mut self,
        layout: &mut PersistentLiveLayout,
        _output: sophia_engine::HeadlessOutput,
        allow_new_cycle: bool,
    ) -> Result<Option<LiveWmProposal>, Box<dyn std::error::Error>> {
        if self.control_restart.is_some() || self.degraded { return Ok(None); }
        let chrome_style = self.candidate_chrome_style();
        let mut public = self.public.take().expect("public WM state is present");
        public.poll_output_authority()?;
        public.flush_deferred_command()?;
        let event = public
            .worker
            .as_ref()
            .ok_or("public WM transport is unavailable")?
            .try_event();
        let mut transport_failed = None;
        let mut defer_cycle = false;
        let proposal = match event {
            Ok(Some(PolicyTransportEvent::Negotiated)) => {
                public.negotiated = true;
                if let Some(key) = public.profile_key {
                    crate::session_println!("sophia_session_profile schema=1 status=activated role=wm epoch={} generation={} digest={}", public.connection_epoch, key.generation().raw(), key.digest());
                }
                // A client that negotiated is a client that works, so the
                // restart budget starts over. Without this the count only ever
                // rises: ProcessHealthy is the one event that clears it and
                // nothing emitted it, so three restarts across an entire
                // session -- however many hours apart, however deliberate --
                // ended the desktop on the third.
                let (state, _) = update_supervisor(
                    self.supervisor_state.clone(),
                    SupervisorEvent::ProcessHealthy,
                    self.restart_policy,
                );
                self.supervisor_state = state;
                None
            }
            Ok(Some(PolicyTransportEvent::ReadyForCycle { capabilities })) => {
                public.selected_capabilities = capabilities;
                if let Ok(mut origins) = public.launch_origins.lock() { origins.set_epoch(if capabilities & sophia_protocol::SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN != 0 { public.connection_epoch } else { 0 }); }
                public.transport_ready = true;
                None
            }
            Ok(Some(PolicyTransportEvent::Configuration {
                transaction,
                configuration,
            })) => {
                defer_cycle = true;
                let outcome = self.stage_policy_configuration(&mut public, &configuration)?;
                public.submit_or_defer(PolicyTransportCommand::ConfigurationOutcome {
                        transaction,
                        generation: configuration.generation,
                        outcome,
                    })?;
                if outcome != sophia_protocol::PolicyProjectionOutcome::Committed {
                    transport_failed = Some("invalid_configuration".to_owned());
                }
                None
            }
            Ok(Some(PolicyTransportEvent::Projection(projection))) => {
                let source = public
                    .in_flight_source
                    .ok_or("public WM projection has no owner cause")?;
                // Surface withdrawal may race a policy response. Advance the
                // canonical scene before touching response placements so a
                // proposal derived from the retired snapshot is rejected as
                // stale instead of trying to materialize a dead surface.
                let current_scene = public.snapshot(layout, chrome_style)?;
                if current_scene.generation > public.reducer.scene().generation {
                    public.reducer.observe_scene(current_scene)?;
                }
                if projection.base_generation != public.reducer.scene().generation {
                    defer_cycle = true;
                    public.settle_rejected_projection(
                        &projection,
                        sophia_protocol::PolicyProjectionOutcome::RejectedStale,
                    )?;
                    self.stale_responses = self.stale_responses.saturating_add(1);
                    crate::session_println!(
                        "sophia_live_wm schema=1 status=stale_response_rejected transaction={} reason=scene_advanced rearmed=true",
                        projection.transaction.raw(),
                    );
                    None
                } else {
                    if let LiveWmProposalSource::Manage(surface) = source {
                        layout.synchronize_admission_extent(surface);
                    }
                    let reconciliation = reconcile_public_policy_proposal(
                        layout,
                        &projection,
                        &public.work_areas,
                        &public.output_bounds,
                        chrome_style,
                    )?;
                    if reconciliation.adjusted_surfaces != 0 {
                        crate::session_println!(
                            "sophia_live_wm schema=1 status=constraints_reconciled transaction={} adjusted_surfaces={}",
                            reconciliation.policy.transaction.raw(),
                            reconciliation.adjusted_surfaces,
                        );
                    }
                    let context_valid = projection.launch_contexts.iter().all(|context| context.epoch == public.connection_epoch && public.reducer.scene().surfaces.iter().any(|s| s.surface == context.surface))
                        && projection.output_launch_contexts.iter().all(|context| context.epoch == public.connection_epoch && public.reducer.scene().outputs.iter().any(|o| o.output == context.output && o.generation == context.output_generation));
                    match if context_valid { public.reducer.stage_proposal(&reconciliation.policy) } else { Err(sophia_protocol::PolicyProjectionOutcome::RejectedInvalid) } {
                    Ok(staged) => {
                        let expected_operation_slot = match source {
                            LiveWmProposalSource::Action(action) => public
                                .actions
                                .iter()
                                .find(|registered| registered.action == action)
                                .and_then(|registered| registered.session_operation_slot),
                            _ => None,
                        };
                        let expect_session_operation = expected_operation_slot.is_some();
                        let identity = LivePolicySettlementIdentity {
                            connection_epoch: projection.connection_epoch,
                            request_id: projection.request_id,
                            scene_generation: projection.base_generation,
                            transaction: projection.transaction,
                            expect_session_operation,
                            session_operation: false,
                        };
                        public.expected_operation_slot = expected_operation_slot;
                        let projections = staged.projections();
                        let active_output = projection.active_output;
                        public.staged_launch_contexts = projection.launch_contexts.clone();
                        public.staged_output_launch_contexts = projection.output_launch_contexts.clone();
                        public.staged = Some(staged);
                        let mut live = public_live_proposal(
                            layout,
                            active_output,
                            projections,
                            projection.transaction,
                            source,
                            identity,
                            &reconciliation,
                        )?;
                        for layer in &mut live.layers {
                            if !projection.outputs.iter().any(|output| {
                                Some(output.output) == layer.output
                            }) {
                                continue;
                            }
                            layer.translation = projection.translation_groups.iter()
                                .find(|group| {
                                    Some(group.output) == layer.output
                                        && group.members.contains(&layer.surface)
                                })
                                .map(|group| sophia_protocol::LayerTranslation {
                                    connection_epoch: projection.connection_epoch,
                                    group: group.group,
                                    x: group.x,
                                    y: group.y,
                                });
                        }
                        Some(live)
                    }
                    Err(outcome) => {
                        defer_cycle = true;
                        public.settle_rejected_projection(&reconciliation.policy, outcome)?;
                        None
                    }
                    }
                }
            }
            Ok(Some(PolicyTransportEvent::Dirty(request))) => {
                if let Err(error) = public.admit_dirty(request) {
                    transport_failed = Some(format!("invalid_dirty:{error}"));
                }
                None
            }
            Ok(Some(PolicyTransportEvent::SessionOperation {
                transaction,
                request,
            })) => {
                let identity = LivePolicySettlementIdentity {
                    connection_epoch: request.connection_epoch,
                    request_id: request.request_id,
                    scene_generation: public.reducer.scene().generation,
                    transaction,
                    expect_session_operation: false,
                    session_operation: true,
                };
                let action = public.operation_actions.get(&request.operation).copied();
                let operation = public
                    .session_operations
                    .iter()
                    .find(|operation| operation.token == request.operation);
                let expected_slot = public.expected_operation_slot.take();
                let valid_target = request.target.is_none_or(|target| {
                    public
                        .reducer
                        .scene()
                        .surfaces
                        .iter()
                        .any(|surface| surface.surface == target)
                });
                let target_permitted = match (operation, request.target) {
                    (Some(operation), Some(_)) => operation.permits_surface_target,
                    (Some(_), None) => true,
                    (None, _) => false,
                };
                if request.connection_epoch != public.connection_epoch
                    || action.is_none()
                    || operation.map(|operation| operation.slot) != expected_slot
                    || !valid_target
                    || !target_permitted
                {
                    defer_cycle = true;
                    public.submit_or_defer(PolicyTransportCommand::SessionOperationOutcome {
                            transaction,
                            request_id: request.request_id,
                            outcome: sophia_protocol::PolicyProjectionOutcome::RejectedInvalid,
                        })?;
                    None
                } else {
                    public.pending_operation = Some((transaction, request));
                    Some(public_operation_proposal(
                        layout,
                        transaction,
                        identity,
                    ))
                }
            }
            Ok(Some(PolicyTransportEvent::Failed(error))) => {
                transport_failed = Some(error);
                None
            }
            Ok(None) => None,
            Err(()) => {
                transport_failed = Some("worker_disconnected".to_owned());
                None
            }
        };

        public.materialize_pending_dirty();

        if proposal.is_none()
            && transport_failed.is_none()
            && !defer_cycle
            && allow_new_cycle
            && public.configured
            && !public.cycle_submitted
            && public.transport_ready
            && public.in_flight_request.is_none()
            && public.deferred_command.is_none()
            && !public.queue.is_empty()
        {
            let scene = public.snapshot(layout, chrome_style)?;
            if scene.generation > public.reducer.scene().generation {
                public.reducer.observe_scene(scene.clone())?;
            }
            // A cause whose subject is gone is moot, and the projection
            // reducer refuses it outright rather than ignoring it, which ends
            // the session. Withdrawal raises its own cause, so dropping this
            // one loses nothing. Causes are only queued long enough to matter
            // because ordinary cycles are held for the whole of a topology
            // candidate, which is exactly when a surface can disappear.
            let mut dropped = 0usize;
            let cause = loop {
                let Some(cause) = public.queue.pop_front() else {
                    break None;
                };
                if let sophia_protocol::PolicyRequestCause::Action { activation_serial, .. } | sophia_protocol::PolicyRequestCause::OutputAction { activation_serial, .. } = cause.cause
                    && let Some(ticket) = public.control_tickets.get(&activation_serial)
                {
                    if ticket.cancelled() || ticket.generation != public.control_generation {
                        ticket.finish(sophia_protocol::ControlOutcome::Stale);
                        public.control_tickets.remove(&activation_serial);
                        continue;
                    }
                    if !ticket.claim() {
                        public.queue.push_front(cause);
                        break None;
                    }
                }
                if policy_cause_subject_is_live(cause.cause, &scene) {
                    break Some(cause);
                }
                if let sophia_protocol::PolicyRequestCause::OutputAction { activation_serial, action, output, output_generation } = cause.cause {
                    crate::session_println!("sophia_shell_action_target schema=1 status=withdrawn policy_connection_epoch={} activation_serial={} action={} target_output={} target_generation={} reason=output_replaced", public.connection_epoch, activation_serial, action.raw(), output.raw(), output_generation);
                }
                dropped = dropped.saturating_add(1);
            };
            if dropped != 0 {
                tracing::warn!(
                    "sophia_live_wm_policy schema=1 status=cause_withdrawn dropped={dropped}",
                );
            }
            let Some(cause) = cause else {
                self.public = Some(public);
                return Ok(proposal);
            };
            // A cause names the outputs it was raised for, and it may have been
            // queued before a topology change replaced them. Its outputs are a
            // hint about where work is owed, not an identity, so they are
            // resolved against the scene the request will actually carry. A
            // cause that outlived every output it named still needs servicing:
            // the topology moved, which is precisely a reason to lay out again.
            let affected_outputs = resolve_public_policy_affected_outputs(
                cause.affected_outputs,
                scene.outputs.iter().map(|output| output.output),
            );
            let request = public
                .reducer
                .issue_request_with_cause(affected_outputs, cause.cause)?;
            let snapshot_transaction = public.mint_transaction()?;
            let request_transaction = public.mint_transaction()?;
            let classifications =
                public_launch_classification_snapshot(&public.launch_classifications, &scene);
            let launch_origins = public.launch_origins.lock().map(|r| r.origins(scene.surfaces.iter().map(|s| s.surface))).unwrap_or_default();
            public.in_flight_origin_surfaces = launch_origins.iter().map(|c| c.surface).collect();
            public
                .worker
                .as_ref()
                .ok_or("public WM transport is unavailable")?
                .try_command(PolicyTransportCommand::Cycle {
                    snapshot_transaction,
                    request_transaction,
                    scene: Box::new(scene),
                    actions: public.actions.clone(),
                    classifications,
                    launch_origins,
                    request: request.clone(),
                })
                .map_err(|_| "public WM cycle queue is busy")?;
            public.in_flight_source = Some(cause.source);
            public.in_flight_request = Some(request);
            public.cycle_submitted = true;
            public.transport_ready = false;
            self.requests = self.requests.saturating_add(1);
        }
        self.public = Some(public);
        if let Some(error) = transport_failed {
            self.request_transport_restart("public_transport_failed", Some(&error));
        }
        Ok(proposal)
    }
}
