{
    scripting.service(wm_session.as_mut(), &layout, output, logout_requested);
    if active_output_topology_preparation.is_none()
        && wm_session
            .as_ref()
            .is_none_or(|wm| !wm.output_topology_effect_pending())
        && layout.constraint_relayout_required()
        && layout.pending.is_none()
        && let Some(wm) = wm_session.as_mut()
    {
        match wm.enqueue_relayout(&layout, output)? {
            LiveWmRequestAdmission::Admitted | LiveWmRequestAdmission::Duplicate => {
                layout.acknowledge_constraint_relayout();
            }
            LiveWmRequestAdmission::RejectedCapacity => {
                return Err("WM recovery-constraint relayout exceeded owner capacity".into());
            }
        }
    }
    if let Some(wm) = wm_session.as_mut() {
        let launch_input_idle = client_keys.pending_len() == 0
            && input_delivery.pending.is_empty()
            && wm.shortcuts.as_ref().is_none_or(WmShortcutRouter::shortcut_idle);
        wm.settle_desktop_reload(config, launch_input_idle)?;
        if profile_reload_requested && launch_input_idle && !config_reload_pending {
            profile_reload_requested = matches!(wm.reload_desktop_profile(config)?, DesktopProfileReloadOutcome::Deferred);
        }
        if wm_restart_requested {
            wm_restart_requested = false;
            wm.request_deliberate_restart();
        }
        let _ = wm.poll_restart(&mut layout, output)?;
        wm.poll_output_authority()?;
        if let Some(mut execution) = active_output_topology_preparation.take() {
            // Keep a recovery copy in owner state across every fallible effect.
            // Normal progress overwrites or clears it below; an early `?`
            // leaves completion enough information to restore frontend state.
            active_output_topology_preparation = Some(execution.clone());
            let transaction = execution.effect.transaction;
            let native = native_scanout
                .as_mut()
                .ok_or("output topology preparation lost its native owner")?;
            let mut retain_execution = true;
            let cancellation_reason = wm.output_topology_cancellation_reason(transaction);
            if let Some(reason) = cancellation_reason.as_ref()
                && execution.phase != LiveOutputTopologyExecutionPhase::WaitingForQuiescence
            {
                match execution.phase {
                    LiveOutputTopologyExecutionPhase::WaitingForQuiescence => {
                        unreachable!("waiting cancellation is reduced below")
                    }
                    LiveOutputTopologyExecutionPhase::Preparing => {
                        if !native.request_abort_output_topology_preparation(reason.clone()) {
                            return Err("output cancellation lost native preparation state".into());
                        }
                    }
                    LiveOutputTopologyExecutionPhase::Applying => {
                        if !native.request_abort_output_topology_preparation(reason.clone()) {
                            return Err("output cancellation lost native apply state".into());
                        }
                        match native.output_topology_preparation_phase() {
                            Some(
                                sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::RollingBack,
                            ) => {
                                wm.reject_output_topology_effect(
                                    transaction,
                                    sophia_engine::OutputTopologyTransactionFailure::Stale,
                                )?;
                                execution.phase =
                                    LiveOutputTopologyExecutionPhase::RollingBack;
                            }
                            Some(
                                sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::Failed,
                            ) => {
                                // No card was mutated. Reuse the preparation
                                // terminal path below to cancel resources and
                                // settle the authority candidate locally.
                                execution.phase = LiveOutputTopologyExecutionPhase::Preparing;
                            }
                            phase => {
                                return Err(format!(
                                    "output cancellation produced an invalid native phase: {phase:?}"
                                )
                                .into());
                            }
                        }
                    }
                    LiveOutputTopologyExecutionPhase::AwaitingFirstPresentation
                    | LiveOutputTopologyExecutionPhase::Reconciling => {
                        native.request_output_topology_rollback(reason.clone())?;
                        wm.reject_output_topology_effect(
                            transaction,
                            sophia_engine::OutputTopologyTransactionFailure::Stale,
                        )?;
                        execution.phase = LiveOutputTopologyExecutionPhase::RollingBack;
                    }
                    LiveOutputTopologyExecutionPhase::RollingBack => {}
                }
                tracing::warn!(
                    "sophia_live_output_authority schema=2 status=cancellation_observed transaction={} phase={:?} reason={reason:?} published={}",
                    transaction.raw(),
                    execution.phase,
                    execution.frontend_candidate_published,
                );
            }
            match execution.phase {
                LiveOutputTopologyExecutionPhase::WaitingForQuiescence => {
                    use crate::live_output_authority::{
                        OutputTopologyPreparationWaitDecision as Decision,
                        OutputTopologyPreparationWaitObservation as Observation,
                        reduce_output_topology_preparation_wait,
                    };
                    let ordinary_settlement_idle = pending_wm_update.is_none()
                        && layout.pending.is_none()
                        && wm.ordinary_policy_settlement_idle();
                    let quiescence_blocker =
                        native.output_topology_preparation_quiescence_blocker();
                    // The rebind that follows this wait has its own quiescence
                    // requirement. Waiting on only the scanout's let the wait
                    // pass and the rebind fail immediately afterwards, so both
                    // read the same predicate now.
                    let runtime_quiescent = runtime
                        .as_ref()
                        .is_none_or(LiveProductionVisualRuntime::topology_rebind_quiescent);
                    let decision = reduce_output_topology_preparation_wait(Observation {
                        cancellation_requested: cancellation_reason.is_some(),
                        ordinary_settlement_idle,
                        native_quiescent: quiescence_blocker.is_none() && runtime_quiescent,
                        deadline_reached: Instant::now() >= execution.preparation_deadline,
                        escalation_available: !execution.quiescence_escalated,
                    });
                    match decision {
                        Decision::Cancel => {
                            wm.reject_output_topology_effect(
                                transaction,
                                sophia_engine::OutputTopologyTransactionFailure::Stale,
                            )?;
                            output_topology_owner.cancel_policy_change()?;
                            retain_execution = false;
                            tracing::warn!(
                                "sophia_live_output_authority schema=2 status=cancellation_observed transaction={} phase=WaitingForQuiescence reason={:?} published=false",
                                transaction.raw(),
                                cancellation_reason.as_deref().unwrap_or("cancelled"),
                            );
                        }
                        Decision::Begin => {
                            let preparation = (|| -> Result<_, Box<dyn std::error::Error>> {
                                let plan = native.plan_output_topology(&execution.effect.resolved)?;
                                let rollback = native.published_output_topology(
                                    &execution.effect.published_snapshot,
                                )?;
                                let runtime = runtime.as_ref().ok_or(
                                    "output authority effect has no visual runtime",
                                )?;
                                let candidate_frames = runtime.compose_output_topology_head_frames(
                                    scene,
                                    &execution.effect.resolved,
                                    execution.effect.candidate_topology_epoch,
                                )?;
                                let rollback_frames = runtime.compose_output_topology_head_frames(
                                    scene,
                                    &rollback,
                                    execution.effect.candidate_topology_epoch,
                                )?;
                                let heads = plan.heads.len();
                                let outputs = plan.outputs.len();
                                let report = native.begin_output_topology_preparation(
                                    plan,
                                    rollback,
                                    candidate_frames,
                                    rollback_frames,
                                )?;
                                Ok((heads, outputs, report))
                            })();
                            match preparation {
                                Ok((heads, outputs, report)) => {
                                    execution.phase = LiveOutputTopologyExecutionPhase::Preparing;
                                    tracing::info!(
                                        "sophia_live_output_authority schema=2 status=resource_preparation_started transaction={} base_epoch={} candidate_epoch={} heads={} outputs={} phase={:?} candidate_prepared={} rollback_prepared={} kms_submits=0 published=false input=quarantined",
                                        transaction.raw(),
                                        execution.effect.base_topology_epoch,
                                        execution.effect.candidate_topology_epoch,
                                        heads,
                                        outputs,
                                        report.phase,
                                        report.candidate_prepared,
                                        report.rollback_prepared,
                                    );
                                }
                                Err(error) => {
                                    wm.reject_output_topology_effect(
                                        transaction,
                                        sophia_engine::OutputTopologyTransactionFailure::Preparation,
                                    )?;
                                    output_topology_owner.cancel_policy_change()?;
                                    retain_execution = false;
                                    tracing::warn!(
                                        "sophia_live_output_authority schema=2 status=resource_preparation_rejected transaction={} error={error} kms_submits=0 published=false",
                                        transaction.raw(),
                                    );
                                }
                            }
                        }
                        Decision::Escalate => {
                            // The owners this wait blocks on advance only while
                            // it waits, so an expiry that merely reports a
                            // stall has not tried to clear it. Skip what is
                            // runnable and what is waiting, settling each as
                            // Skipped so no client is left expecting feedback,
                            // and give the candidate one more window. The
                            // displayed topology is untouched.
                            let skipped = runtime.as_mut().map_or_else(
                                Default::default,
                                |runtime| runtime.skip_presentations_for_topology(Some(native)),
                            );
                            execution.quiescence_escalated = true;
                            execution.preparation_deadline =
                                Instant::now() + OUTPUT_TOPOLOGY_QUIESCENCE_TIMEOUT;
                            tracing::warn!(
                                "sophia_live_output_authority schema=3 status=quiescence_escalated escalated=1 skipped_in_flight={} skipped_queued={} skipped_software={} settlement_idle={} native_blocker={} runtime_blocker={} transaction={} timeout_msec={}",
                                skipped
                                    .skipped_in_flight
                                    .map_or_else(|| "none".to_owned(), |t| t.raw().to_string()),
                                skipped.skipped_queued,
                                skipped.skipped_software,
                                ordinary_settlement_idle,
                                // Both read after the skip. Reporting the
                                // blocker observed before it, beside a runtime
                                // report taken after, would describe a state
                                // that never existed.
                                native
                                    .output_topology_preparation_quiescence_blocker()
                                    .unwrap_or("none"),
                                runtime.as_ref().map_or_else(
                                    || "none".to_owned(),
                                    LiveProductionVisualRuntime::topology_rebind_quiescence_report,
                                ),
                                transaction.raw(),
                                OUTPUT_TOPOLOGY_QUIESCENCE_TIMEOUT.as_millis(),
                            );
                        }
                        Decision::TimedOut => {
                            wm.reject_output_topology_effect(
                                transaction,
                                sophia_engine::OutputTopologyTransactionFailure::Preparation,
                            )?;
                            output_topology_owner.cancel_policy_change()?;
                            retain_execution = false;
                            // Say which of the two conditions was unmet, and
                            // which owner still held a frame. Reporting only
                            // that a wait expired sends its reader to the
                            // source to guess between eight candidates.
                            tracing::warn!(
                                "sophia_live_output_authority schema=3 status=quiescence_timed_out settlement_idle={} native_blocker={} runtime_blocker={} native_head={} wm_update_pending={} layout_pending={} policy_settlement_idle={} transaction={} timeout_msec={} kms_submits=0 preserved_topology=true",
                                ordinary_settlement_idle,
                                quiescence_blocker.unwrap_or("none"),
                                runtime.as_ref().map_or_else(
                                    || "none".to_owned(),
                                    LiveProductionVisualRuntime::topology_rebind_quiescence_report,
                                ),
                                native
                                    .output_topology_quiescence_head_report()
                                    .unwrap_or_else(|| "none".to_owned()),
                                pending_wm_update.is_some(),
                                layout.pending.is_some(),
                                wm.ordinary_policy_settlement_idle(),
                                transaction.raw(),
                                OUTPUT_TOPOLOGY_QUIESCENCE_TIMEOUT.as_millis(),
                            );
                        }
                        Decision::Wait => {}
                    }
                }
                LiveOutputTopologyExecutionPhase::Preparing => {
                    let report = native.service_output_topology_preparation()?;
                    if execution.last_preparation_progress != Some(report) {
                        tracing::info!(
                            "sophia_live_output_authority schema=2 status=resource_progress transaction={} phase={:?} candidate_prepared={} rollback_prepared={} heads={} kms_submits=0 published=false",
                            transaction.raw(),
                            report.phase,
                            report.candidate_prepared,
                            report.rollback_prepared,
                            report.affected_heads,
                        );
                        execution.last_preparation_progress = Some(report);
                    }
                    if report.phase
                        == sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::Prepared
                    {
                        let heads = native.begin_prepared_output_topology_apply()?;
                        wm.begin_output_topology_apply(transaction, &heads)?;
                        execution.phase = LiveOutputTopologyExecutionPhase::Applying;
                        tracing::info!(
                            "sophia_live_output_authority schema=2 status=apply_started transaction={} heads={} cards=ordered published=false",
                            transaction.raw(),
                            heads.len(),
                        );
                    } else if report.phase
                        == sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::Failed
                    {
                        let (plan, error) =
                            native.finish_failed_output_topology_preparation()?;
                        wm.reject_output_topology_effect(
                            transaction,
                            sophia_engine::OutputTopologyTransactionFailure::Preparation,
                        )?;
                        output_topology_owner.cancel_policy_change()?;
                        retain_execution = false;
                        tracing::warn!(
                            "sophia_live_output_authority schema=2 status=resource_preparation_rejected transaction={} heads={} error={error} kms_submits=0 published=false",
                            transaction.raw(),
                            plan.heads.len(),
                        );
                    }
                }
                LiveOutputTopologyExecutionPhase::Applying
                | LiveOutputTopologyExecutionPhase::RollingBack => {
                    use sophia_backend_live::LiveProductionNativeTopologyApplyTransition as Transition;
                    let transition = native.service_prepared_output_topology_apply()?;
                    match transition {
                        Transition::Retry => {}
                        Transition::CardApplied { card_index, heads } => {
                            wm.observe_output_topology_applied(transaction, &heads)?;
                            tracing::info!(
                                "sophia_live_output_authority schema=2 status=card_applied transaction={} card={} heads={} published=false",
                                transaction.raw(), card_index, heads.len(),
                            );
                        }
                        Transition::Applied { card_index, heads } => {
                            wm.observe_output_topology_applied(transaction, &heads)?;
                            if output_proof_rollback_after_apply.take_for_startup(
                                wm.is_startup_output_transaction(transaction),
                            ) {
                                tracing::warn!(
                                    "sophia_live_output_authority schema=3 status=proof_rollback_triggered transaction={} boundary=after_apply card={} heads={} candidate_installed=false published=false",
                                    transaction.raw(),
                                    card_index,
                                    heads.len(),
                                );
                                wm.reject_output_topology_effect(
                                    transaction,
                                    sophia_engine::OutputTopologyTransactionFailure::Apply,
                                )?;
                                native.request_output_topology_rollback(
                                    "proof requested rollback after KMS apply",
                                )?;
                                execution.phase = LiveOutputTopologyExecutionPhase::RollingBack;
                                tracing::warn!(
                                    "sophia_live_output_authority schema=3 status=rollback_started transaction={} reason=proof_after_apply published=false",
                                    transaction.raw(),
                                );
                                active_output_topology_preparation = Some(execution.clone());
                                continue;
                            }
                            let installation = (|| -> Result<_, Box<dyn std::error::Error>> {
                                let runtime = runtime
                                    .as_mut()
                                    .ok_or("output topology apply lost the visual runtime")?;
                                let frames = runtime.compose_output_topology_head_frames(
                                    scene,
                                    &execution.effect.resolved,
                                    execution.effect.candidate_topology_epoch,
                                )?;
                                let target_outputs = execution
                                    .effect
                                    .resolved
                                    .targets
                                    .iter()
                                    .map(|target| (target.head, target.output))
                                    .collect::<BTreeMap<_, _>>();
                                let mut by_output: BTreeMap<_, Vec<_>> = BTreeMap::new();
                                for frame in frames {
                                    let output = target_outputs
                                        .get(&frame.head)
                                        .copied()
                                        .ok_or("first topology frame targets an unknown head")?;
                                    by_output.entry(output).or_default().push(frame);
                                }
                                // Installation discards the head frames any
                                // bound software present was lowered onto, so
                                // those bindings must settle here. Left alone
                                // they would wait forever on a flip that can no
                                // longer happen, and the runtime would never
                                // report quiescent again.
                                let orphaned = runtime.settle_discarded_software_presents();
                                if orphaned != 0 {
                                    tracing::warn!(
                                        "sophia_live_present_skip schema=1 status=topology_discarded reason=install settled={orphaned}",
                                    );
                                }
                                let candidate_outputs = native.install_applied_output_topology()?;
                                let candidate_viewports = execution
                                    .effect
                                    .resolved
                                    .logical_viewports
                                    .iter()
                                    .map(|viewport| (viewport.output, viewport.logical))
                                    .collect::<Vec<_>>();
                                runtime.rebind_applied_native_topology(
                                    native,
                                    &candidate_outputs,
                                    &candidate_viewports,
                                )?;
                                let first_frames = native.queue_topology_first_frames(by_output.into_iter().collect())?;
                                native.arm_installed_output_topology_first_presentation(&first_frames)?;
                                Ok((candidate_outputs, first_frames))
                            })();
                            match installation {
                                Ok((candidate_outputs, first_frames)) => {
                                    execution.first_frames = first_frames;
                                    execution.phase =
                                        LiveOutputTopologyExecutionPhase::AwaitingFirstPresentation;
                                    tracing::info!(
                                        "sophia_live_output_authority schema=2 status=candidate_installed transaction={} card={} outputs={} first_frames={} published=false rollback_retained=true",
                                        transaction.raw(),
                                        card_index,
                                        candidate_outputs.len(),
                                        execution.first_frames.len(),
                                    );
                                }
                                Err(error) => {
                                    wm.reject_output_topology_effect(
                                        transaction,
                                        sophia_engine::OutputTopologyTransactionFailure::FirstPresentation,
                                    )?;
                                    native.request_output_topology_rollback(format!(
                                        "candidate installation failed: {error}"
                                    ))?;
                                    execution.phase = LiveOutputTopologyExecutionPhase::RollingBack;
                                    tracing::warn!(
                                        "sophia_live_output_authority schema=2 status=rollback_started transaction={} reason=installation error={error} published=false",
                                        transaction.raw(),
                                    );
                                }
                            }
                        }
                        Transition::RollbackRequired { failed_card_index } => {
                            wm.reject_output_topology_effect(
                                transaction,
                                sophia_engine::OutputTopologyTransactionFailure::Apply,
                            )?;
                            execution.phase = LiveOutputTopologyExecutionPhase::RollingBack;
                            tracing::warn!(
                                "sophia_live_output_authority schema=2 status=rollback_started transaction={} reason=card_refused card={} published=false",
                                transaction.raw(), failed_card_index,
                            );
                        }
                        Transition::CardRolledBack { card_index, heads } => {
                            wm.observe_output_topology_rolled_back(transaction, &heads)?;
                            tracing::info!(
                                "sophia_live_output_authority schema=2 status=card_rolled_back transaction={} card={} heads={}",
                                transaction.raw(), card_index, heads.len(),
                            );
                        }
                        Transition::RolledBack { card_index, heads } => {
                            let (plan, reason) = native.install_rolled_back_output_topology()?;
                            if let Some(runtime) = runtime.as_mut() {
                                let orphaned = runtime.settle_discarded_software_presents();
                                if orphaned != 0 {
                                    tracing::warn!(
                                        "sophia_live_present_skip schema=1 status=topology_discarded reason=rollback settled={orphaned}",
                                    );
                                }
                            }
                            let rollback_outputs = native.outputs();
                            let rollback_viewports = plan
                                .logical_viewports
                                .iter()
                                .map(|viewport| (viewport.output, viewport.logical))
                                .collect::<Vec<_>>();
                            runtime
                                .as_mut()
                                .ok_or("output rollback lost the visual runtime")?
                                .rebind_applied_native_topology(
                                    native,
                                    &rollback_outputs,
                                    &rollback_viewports,
                                )?;
                            let rollback_primary = rollback_outputs
                                .iter()
                                .find(|candidate| {
                                    candidate.id
                                        == execution.effect.published_snapshot.primary_output
                                })
                                .copied()
                                .or_else(|| rollback_outputs.first().copied())
                                .ok_or("output rollback restored no logical output")?;
                            if scene.reconfigure_output_size(rollback_primary.size)? {
                                let committed = runtime
                                    .as_ref()
                                    .map(|runtime| runtime.committed_surfaces().to_vec())
                                    .unwrap_or_default();
                                scene.compose(&committed, None, pointer.position())?;
                            }
                            if execution.frontend_candidate_published {
                                let generation = output_topology_owner
                                    .publication_generation
                                    .checked_add(2)
                                    .ok_or("output publication generation exhausted")?;
                                let snapshot = output_topology_from_authority_at_generation(
                                    &execution.effect.published_snapshot,
                                    generation,
                                )?;
                                let (ack_sender, ack_receiver) = sync_channel(1);
                                frontend_service_sender.send(
                                    XServerFrontendServiceCommand::UpdateOutputTopology {
                                        snapshot,
                                        acknowledgement: ack_sender,
                                    },
                                )?;
                                if !matches!(
                                    ack_receiver.recv_timeout(Duration::from_secs(1))?,
                                    sophia_x_authority::XAuthorityOutputUpdateOutcome::Applied { .. }
                                ) {
                                    return Err("X frontend rejected topology rollback".into());
                                }
                                output_topology_owner
                                    .observe_policy_transport_rollback(generation)?;
                            }
                            let rollback_bounds = execution
                                .effect
                                .published_snapshot
                                .groups
                                .iter()
                                .map(|group| (group.output, group.logical))
                                .collect::<Vec<_>>();
                            let _ = wm.update_output_work_areas_at(
                                &layout,
                                &rollback_outputs,
                                &rollback_bounds,
                                rollback_primary,
                            )?;
                            outputs = rollback_outputs;
                            output = rollback_primary;
                            pointer.set_output_bounds(
                                rollback_bounds.iter().map(|(_, bounds)| *bounds).collect(),
                            );
                            cursor_updates.dirty = pointer.position().is_some();
                            cursor_updates.dirty_since = cursor_updates.dirty.then(Instant::now);
                            output_topology_owner.cancel_policy_change()?;
                            wm.observe_output_topology_rolled_back(transaction, &heads)?;
                            retain_execution = false;
                            tracing::warn!(
                                "sophia_live_output_authority schema=2 status=rolled_back transaction={} card={} reason={reason:?} published=false input=enabled",
                                transaction.raw(), card_index,
                            );
                        }
                        Transition::FailedWithoutMutation { card_index } => {
                            let (plan, error) =
                                native.finish_failed_output_topology_preparation()?;
                            wm.reject_output_topology_effect(
                                transaction,
                                sophia_engine::OutputTopologyTransactionFailure::Apply,
                            )?;
                            output_topology_owner.cancel_policy_change()?;
                            retain_execution = false;
                            tracing::warn!(
                                "sophia_live_output_authority schema=2 status=apply_rejected transaction={} card={} heads={} error={error} published=false",
                                transaction.raw(), card_index, plan.heads.len(),
                            );
                        }
                        Transition::RollbackFailed { card_index } => {
                            wm.observe_output_topology_rollback_failed(transaction)?;
                            return Err(format!(
                                "output topology rollback failed on card {card_index}"
                            )
                            .into());
                        }
                        Transition::Accepted
                        | Transition::OutOfOrder
                        | Transition::Terminal => {
                            return Err(format!(
                                "unexpected native topology transition: {transition:?}"
                            )
                            .into());
                        }
                    }
                }
                LiveOutputTopologyExecutionPhase::AwaitingFirstPresentation => {
                    if execution.first_frames.iter().all(|(output, frame)| {
                        native.presented_frame(*output) == Some(*frame)
                    }) {
                        execution.phase = LiveOutputTopologyExecutionPhase::Reconciling;
                        tracing::info!(
                            "sophia_live_output_authority schema=2 status=first_presented transaction={} outputs={} published=false rollback_retained=true",
                            transaction.raw(), execution.first_frames.len(),
                        );
                    }
                }
                LiveOutputTopologyExecutionPhase::Reconciling => {
                    let candidate_outputs = execution.effect.resolved.outputs.clone();
                    let candidate_primary = candidate_outputs
                        .iter()
                        .find(|candidate| {
                            candidate.id == execution.effect.resolved.primary_output
                        })
                        .copied()
                        .ok_or("candidate topology lost its primary output")?;
                    let generation = output_topology_owner
                        .publication_generation
                        .checked_add(1)
                        .ok_or("output publication generation exhausted")?;
                    if !execution.frontend_candidate_published {
                        let snapshot = prepare_output_topology_publication(
                            &execution.effect.candidate_snapshot,
                            &execution.effect.resolved,
                            generation,
                        )?;
                        let (ack_sender, ack_receiver) = sync_channel(1);
                        frontend_service_sender.send(
                            XServerFrontendServiceCommand::UpdateOutputTopology {
                                snapshot,
                                acknowledgement: ack_sender,
                            },
                        )?;
                        match ack_receiver.recv_timeout(Duration::from_secs(1))? {
                            sophia_x_authority::XAuthorityOutputUpdateOutcome::Applied { .. } => {
                                execution.frontend_candidate_published = true;
                                active_output_topology_preparation = Some(execution.clone());
                                tracing::info!(
                                    "sophia_live_output_authority schema=3 status=frontend_candidate_published transaction={} generation={} published=false rollback_retained=true",
                                    transaction.raw(),
                                    generation,
                                );
                            }
                            outcome => {
                                wm.reject_output_topology_effect(
                                    transaction,
                                    sophia_engine::OutputTopologyTransactionFailure::FirstPresentation,
                                )?;
                                native.request_output_topology_rollback(format!(
                                    "X frontend rejected candidate topology: {outcome:?}"
                                ))?;
                                execution.phase = LiveOutputTopologyExecutionPhase::RollingBack;
                            }
                        }
                    }
                    if execution.phase == LiveOutputTopologyExecutionPhase::Reconciling {
                        let bounds = resolved_output_bounds(&execution.effect.resolved);
                        let admission = wm.update_output_work_areas_at(
                            &layout,
                            &candidate_outputs,
                            &bounds,
                            candidate_primary,
                        )?;
                        if admission == LiveWmRequestAdmission::RejectedCapacity {
                            wm.reject_output_topology_effect(
                                transaction,
                                sophia_engine::OutputTopologyTransactionFailure::FirstPresentation,
                            )?;
                            native.request_output_topology_rollback(
                                "WM output reconciliation exceeded owner capacity",
                            )?;
                            execution.phase = LiveOutputTopologyExecutionPhase::RollingBack;
                        } else {
                            if scene.reconfigure_output_size(candidate_primary.size)? {
                                let committed = runtime
                                    .as_ref()
                                    .map(|runtime| runtime.committed_surfaces().to_vec())
                                    .unwrap_or_default();
                                scene.compose(&committed, None, pointer.position())?;
                            }
                            let policy_required = admission == LiveWmRequestAdmission::Admitted
                                || wm.has_current_relayout_request(&layout);
                            output_topology_policy_commit_baseline =
                                wm.topology_policy_commit_serial();
                            let retirements = native.retirements;
                            let mut published_topology_owner = output_topology_owner.clone();
                            published_topology_owner.observe_policy_rebuild(
                                candidate_outputs.clone(),
                                native.head_fingerprint(),
                                execution.effect.candidate_topology_epoch,
                            )?;
                            published_topology_owner.mark_published(
                                retirements.saturating_sub(1),
                                policy_required,
                            )?;
                            let presented_outputs = execution
                                .first_frames
                                .keys()
                                .copied()
                                .collect::<Vec<_>>();
                            let preview = wm.preview_output_topology_first_presented(
                                transaction,
                                &presented_outputs,
                            )?;
                            if preview != execution.effect.candidate_snapshot {
                                return Err(
                                    "output authority previewed the wrong candidate".into()
                                );
                            }
                            // This is the point of no return: every physical,
                            // Engine, X, and WM fact has been validated while
                            // rollback resources are still owned. Releasing the
                            // rollback pool makes the candidate authoritative;
                            // later peer loss degrades transport, not topology.
                            let _ = native.commit_installed_output_topology()?;
                            let published = wm
                                .observe_output_topology_first_presented(
                                    transaction,
                                    &presented_outputs,
                                )?
                                .ok_or("output authority committed without a snapshot")?;
                            if published != execution.effect.candidate_snapshot {
                                return Err("output authority published the wrong candidate".into());
                            }
                            output_topology_owner = published_topology_owner;
                            outputs = candidate_outputs;
                            output = candidate_primary;
                            pointer.set_output_bounds(
                                bounds.iter().map(|(_, bounds)| *bounds).collect(),
                            );
                            cursor_updates.dirty = pointer.position().is_some();
                            cursor_updates.dirty_since = cursor_updates.dirty.then(Instant::now);
                            retain_execution = false;
                            tracing::info!(
                                "sophia_live_output_authority schema=2 status=committed transaction={} topology_epoch={} outputs={} policy_required={} input=quarantined",
                                transaction.raw(),
                                published.topology_epoch,
                                outputs.len(),
                                policy_required,
                            );
                        }
                    }
                }
            }
            if retain_execution {
                active_output_topology_preparation = Some(execution);
            } else {
                active_output_topology_preparation = None;
            }
        }
        // A reloaded profile asking for different displays becomes an ordinary
        // candidate here, where the native scanout is in reach. The effect it
        // leaves behind is drained by the block below, on this same iteration,
        // so a reload and a startup reach the modeset by one road.
        if active_output_topology_preparation.is_none()
            && output_topology_owner.phase == LiveOutputTopologyPhase::Stable
            && wm.ordinary_policy_settlement_idle()
            && wm.take_output_topology_reload_request()
        {
            match (native_scanout.as_ref(), wm.published_output_snapshot()) {
                (Some(native), Some(snapshot)) => {
                    match build_reloaded_output_topology_candidate(
                        native,
                        config,
                        &snapshot,
                        initial_head_mapping,
                    ) {
                        Ok(candidate) => {
                            wm.admit_reloaded_output_topology(candidate)?;
                        }
                        // A profile naming a mode or a connector the hardware
                        // does not have is the ordinary way to reach this, and
                        // it must cost the operator a log line rather than the
                        // desktop they are looking at.
                        Err(error) => crate::session_eprintln!(
                            "sophia_live_output_authority schema=3 status=reload_declined reason=candidate detail={error}"
                        ),
                    }
                }
                _ => crate::session_eprintln!(
                    "sophia_live_output_authority schema=3 status=reload_declined reason=no_native_authority"
                ),
            }
        }
        if active_output_topology_preparation.is_none()
            && runtime.is_some()
            && pending_wm_update.is_none()
            && layout.pending.is_none()
            && wm.ordinary_policy_settlement_idle()
            && output_topology_owner.phase == LiveOutputTopologyPhase::Stable
            && let Some(effect) = wm.take_output_topology_effect()
        {
            if output_topology_owner.begin_policy_change()? {
                let revoked_input_leases = advance_application_input_security_epoch(
                    &mut application_route_leases,
                    input_sender,
                    &layout.client_routes,
                    route_lease_release_sender,
                )?;
                revoke_chrome_captures!("output_policy");
                if let Some(interaction) = floating_pointer_gesture.cancel() {
                    if matches!(
                        wm.enqueue_pointer_interaction(interaction, &layout)?,
                        LiveWmRequestAdmission::RejectedCapacity
                    ) {
                        return Err(
                            "output-policy cancellation exceeded WM owner capacity".into()
                        );
                    }
                    if let Some(runtime) = runtime.as_mut() {
                        let _ = runtime.set_floating_outline(
                            None,
                            scene,
                            native_scanout.as_mut(),
                        )?;
                    }
                    crate::session_println!(
                        "sophia_live_wm_pointer schema=2 status=interaction_cancelled reason=output_policy surface={}",
                        interaction.surface.index(),
                    );
                }
                pointer_focus_handoff = PointerFocusHandoffState::default();
                keyboard_focus_handoff = KeyboardFocusHandoffState::default();
                deferred_physical_key_timings.clear();
                key_repeat.cancel_seat(seat);
                crate::session_println!(
                    "sophia_live_input_epoch schema=1 reason=output_policy transition={} epoch={} revoked_leases={revoked_input_leases}",
                    output_topology_owner.transition,
                    application_route_leases.control_epoch(),
                );
            }
            active_output_topology_preparation = Some(LiveOutputTopologyExecution {
                effect: effect.clone(),
                phase: LiveOutputTopologyExecutionPhase::WaitingForQuiescence,
                preparation_deadline: Instant::now() + OUTPUT_TOPOLOGY_QUIESCENCE_TIMEOUT,
                first_frames: BTreeMap::new(),
                frontend_candidate_published: false,
                quiescence_escalated: false,
                last_preparation_progress: None,
            });
            tracing::info!(
                "sophia_live_output_authority schema=2 status=waiting_for_quiescence transaction={} timeout_msec={} input=quarantined",
                effect.transaction.raw(),
                OUTPUT_TOPOLOGY_QUIESCENCE_TIMEOUT.as_millis(),
            );
        }
    }
    synchronize_wm_pointer_epoch!();
    if let Some(runtime) = runtime.as_mut() {
        let style = wm_session
            .as_ref()
            .and_then(|wm| wm.surface_chrome_style())
            .unwrap_or(config.surface_chrome_style);
        synchronize_runtime_surface_chrome_style(runtime, style);
    }
    if active_output_topology_preparation.is_none()
        && pending_wm_update.is_none()
        && layout.pending.is_none()
        && let Some(wm) = wm_session.as_mut()
    {
        // Held both before the effect is taken and while the candidate is in
        // flight. Only the first was checked, so cycles resumed at the moment
        // the candidate dispatched: a new layout transaction would start under
        // the quarantine, keep ordinary settlement busy, and deny the candidate
        // the idle window it waits for until it timed out and was rejected.
        // Causes queue rather than drop, so nothing is lost by waiting.
        let allow_new_cycle =
            !wm.output_topology_effect_pending() && !wm.output_candidate_active();
        if let Some(proposal) = wm.poll_request(&mut layout, output, allow_new_cycle)? {
            let public_projection = proposal
                .policy_settlement
                .is_some_and(|settlement| !settlement.session_operation);
            if public_projection && !wm.preflight_staged_presentation(runtime.as_ref(), native_scanout.as_ref()) {
                wm.reject_staged_presentation(proposal.policy_settlement.expect("public projection has settlement"))?;
            } else if public_projection
                && wm.trigger_public_proof_fault(PublicPolicyFaultPoint::ProposalStaged)
            {
                let _ = wm.poll_restart(&mut layout, output)?;
            } else {
                let previous_focus = focus.focused_surface(seat);
                if let Some(result) = layout.stage(proposal, &mut session_controls)? {
                    pending_wm_update = Some(apply_wm_commit_result!(result, previous_focus));
                } else if public_projection
                    && wm.trigger_public_proof_fault(PublicPolicyFaultPoint::FrontendPending)
                {
                    let _ = wm.poll_restart(&mut layout, output)?;
                }
            }
        }
    }
    service_layout_progress!("wm_stage");
}
