{
    let original_failure_phase = *failure_phase;
    *failure_phase = crate::diagnostics::SessionFailurePhase::Cleanup;
    let SessionLoopMetrics {
        batches,
        transactions,
        cpu_buffer_updates,
        cpu_buffer_replacements,
        cpu_buffer_patch_updates,
        cpu_buffer_patch_rects,
        cpu_buffer_payload_bytes,
        dma_buf_registrations_observed: _,
        fence_registrations_observed: _,
        present_submissions_observed: _,
        software_present_submissions_observed: _,
        cpu_compositions,
        coalesced_batches,
        cadence_deferred_batches,
        cadence_repaints,
        merged_batches,
        max_merge_run,
        backend_ticks,
        runtime_committed,
        runtime_surfaces,
        runtime_max_surfaces,
        physical_events,
        physical_keys_routed,
        key_repeats_routed,
        physical_pointer_events,
        physical_pointer_routed,
        physical_pointer_buttons_routed,
        session_ticks,
        max_compose,
        max_child_reap,
        max_input_phase,
        protocol_error_count,
        expected_protocol_error_count,
        cursor_moves_coalesced,
        cursor_max_motion_to_submit,
    } = metrics;

    let mut cleanup_failures = terminal_client_cleanup_failures;
    // No new shell work may enter while the native drain below is running.
    // Keep the transport/epoch owner outside this loop for final accounting.
    if let Some(shell) = metadata_shell.as_mut() {
        if let Err(error) = shell.stop_for_session_shutdown() {
            cleanup_failures.push(format!("shell admission shutdown failed: {error}"));
        }
        settle_revoked_shell_content_claims!(shell, "session_shutdown");
    }
    if let Some(components) = shell_components.as_mut() {
        if let Err(error) = components.request_shutdown() {
            cleanup_failures.push(format!("component admission shutdown failed: {error}"));
        }
        if let Err(error) = components.settle_revocations(runtime.as_mut()) {
            cleanup_failures.push(format!("component claim cleanup retained: {error}"));
        }
    }
    let mut fatal_cleanup = SessionFatalCleanupEvidence {
        frontend_intake_stopped: terminal_client_intake_stopped,
        native_cleanup_required: native_scanout.is_some(),
        presentations_shutdown: runtime.is_none(),
        ..Default::default()
    };
    let _ = native_owner_retirement::with_active_device_authority(render_owners.seat_active, || {
    let mut topology_rollback_established = false;
    if let Some(native_scanout) = native_scanout.as_mut()
        && native_scanout.output_topology_preparation_active()
    {
        if native_scanout.output_topology_preparation_phase()
            == Some(
                sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::FirstFramesQueued,
            )
            && let Some(runtime) = runtime.as_mut()
        {
            let candidate_outputs = native_scanout.outputs();
            if let Err(error) = runtime.suspend_native_scanout(
                native_scanout,
                &candidate_outputs,
                Duration::from_secs(2),
            ) {
                cleanup_failures.push(format!(
                    "candidate topology first-frame drain failed before rollback: {error}"
                ));
            }
        }
        native_scanout.request_abort_output_topology_preparation(
            "session completion cancelled topology preparation",
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            use sophia_backend_live::LiveProductionNativeTopologyPreparationPhase as Phase;
            match native_scanout.output_topology_preparation_phase() {
                Some(Phase::Failed) => {
                    let published_preserved =
                        native_scanout.output_topology_failed_without_mutation();
                    match native_scanout.finish_failed_output_topology_preparation() {
                        Ok((plan, reason)) => crate::session_println!(
                            "sophia_live_output_topology schema=2 status=completion_cancelled heads={} reason={reason:?}",
                            plan.heads.len(),
                        ),
                        Err(error) => cleanup_failures.push(format!(
                            "topology preparation completion failed: {error}"
                        )),
                    }
                    topology_rollback_established = published_preserved;
                    break;
                }
                Some(Phase::RolledBack) => {
                    match native_scanout.install_rolled_back_output_topology() {
                        Ok((plan, reason)) => {
                            let rollback_outputs = native_scanout.outputs();
                            let rollback_viewports = plan
                                .logical_viewports
                                .iter()
                                .map(|viewport| (viewport.output, viewport.logical))
                                .collect::<Vec<_>>();
                            if let Some(runtime) = runtime.as_mut()
                                && let Err(error) = runtime.rebind_applied_native_topology(
                                    native_scanout,
                                    &rollback_outputs,
                                    &rollback_viewports,
                                )
                            {
                                cleanup_failures.push(format!(
                                    "topology rollback runtime rebind failed: {error}"
                                ));
                            }
                            crate::session_println!(
                                "sophia_live_output_topology schema=2 status=completion_rolled_back heads={} reason={reason:?}",
                                plan.heads.len(),
                            );
                            topology_rollback_established = true;
                        }
                        Err(error) => cleanup_failures.push(format!(
                            "topology rollback installation failed: {error}"
                        )),
                    }
                    break;
                }
                Some(Phase::RollingBack) => {
                    if let Err(error) =
                        native_scanout.service_prepared_output_topology_apply()
                    {
                        cleanup_failures
                            .push(format!("topology completion rollback failed: {error}"));
                        break;
                    }
                }
                Some(
                    Phase::PreparingCandidate
                    | Phase::PreparingRollback
                    | Phase::Prepared
                    | Phase::Aborting,
                ) => {
                    if let Err(error) = native_scanout.service_output_topology_preparation() {
                        cleanup_failures.push(format!(
                            "topology renderer preparation abort failed: {error}"
                        ));
                        break;
                    }
                }
                Some(Phase::Applying | Phase::Applied | Phase::CandidateInstalled | Phase::FirstFramesQueued) => {
                    cleanup_failures.push(
                        "topology completion abort did not enter a safe rollback phase".to_owned(),
                    );
                    break;
                }
                None => break,
            }
            if Instant::now() >= deadline {
                    cleanup_failures.push(
                        "topology transaction did not abort within two seconds".to_owned(),
                    );
                break;
            }
            std::thread::yield_now();
        }
        while native_scanout.output_topology_cleanup_pending() && Instant::now() < deadline {
            native_scanout.retry_output_topology_cleanup();
            std::thread::yield_now();
        }
        if native_scanout.output_topology_cleanup_pending() {
            cleanup_failures
                .push("topology resource cleanup remained pending at native suspension".to_owned());
        }
    }
    if let Some(execution) = active_output_topology_preparation.take() {
        if execution.frontend_candidate_published && topology_rollback_established {
            let generation = output_topology_owner
                .publication_generation
                .checked_add(2);
            // Even exhausted publication state must reach native cleanup and
            // preserve any runtime failure that brought the session here.
            let rollback_snapshot = generation
                .ok_or_else(|| {
                    Box::<dyn std::error::Error>::from(
                        "output publication generation exhausted during completion",
                    )
                })
                .and_then(|generation| {
                    output_topology_from_authority_at_generation(
                        &execution.effect.published_snapshot,
                        generation,
                    )
                    .map(|snapshot| (generation, snapshot))
                });
            match rollback_snapshot {
                Ok((generation, snapshot)) => {
                    let (ack_sender, ack_receiver) = sync_channel(1);
                    match frontend_service_sender.send(
                        XServerFrontendServiceCommand::UpdateOutputTopology {
                            snapshot,
                            acknowledgement: ack_sender,
                        },
                    ) {
                        Ok(()) => match ack_receiver.recv_timeout(Duration::from_secs(1)) {
                            Ok(sophia_x_authority::XAuthorityOutputUpdateOutcome::Applied {
                                ..
                            }) => {
                                if let Err(error) = output_topology_owner
                                    .observe_policy_transport_rollback(generation)
                                {
                                    cleanup_failures.push(format!(
                                        "topology completion transport rollback observation failed: {error}"
                                    ));
                                }
                            }
                            Ok(outcome) => cleanup_failures.push(format!(
                                "X frontend rejected completion topology rollback: {outcome:?}"
                            )),
                            Err(error) => cleanup_failures.push(format!(
                                "X frontend topology rollback acknowledgement failed: {error}"
                            )),
                        },
                        Err(error) => cleanup_failures.push(format!(
                            "X frontend topology rollback dispatch failed: {error}"
                        )),
                    }
                }
                Err(error) => cleanup_failures.push(format!(
                    "completion topology rollback projection failed: {error}"
                )),
            }
        }
        if topology_rollback_established
            && output_topology_owner.phase
                == LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Policy)
            && let Err(error) = output_topology_owner.cancel_policy_change()
        {
            cleanup_failures.push(format!(
                "completion topology quarantine release failed: {error}"
            ));
        }
    }
    if let (Some(runtime), Some(native_scanout)) = (runtime.as_mut(), native_scanout.as_mut()) {
        fatal_cleanup.native_suspend_attempted = true;
        fatal_cleanup.native_heads_in_flight_before =
            native_scanout.head_scanout_in_flight_count();
        let mut detach_established = false;
        match runtime.suspend_native_scanout(native_scanout, &outputs, Duration::from_secs(2)) {
            Ok(report) => {
                detach_established = true;
                fatal_cleanup.native_suspend_reported = true;
                fatal_cleanup.native_drained = report.outcome.drained();
                fatal_cleanup.abandoned_scanouts = report.abandoned_scanouts;
                crate::session_println!(
                    "sophia_live_session_native_suspend schema=2 outcome={} drained={} abandoned_scanouts={} skipped_present={}",
                    report.outcome.reduced_name(),
                    report.outcome.drained(),
                    report.abandoned_scanouts,
                    report.skipped_present.map_or_else(
                        || "none".to_owned(),
                        |transaction| transaction.raw().to_string()
                    ),
                );
                if !report.outcome.drained() || report.abandoned_scanouts != 0 {
                    cleanup_failures.push(format!(
                        "native completion forced detach with {} abandoned scanouts",
                        report.abandoned_scanouts,
                    ));
                }
            }
            Err(error) => {
                if let Some(suspend_error) = error
                    .downcast_ref::<LiveProductionNativeSuspendError>()
                    && let Some(report) = suspend_error.detach_report
                {
                    detach_established = true;
                    fatal_cleanup.native_suspend_reported = true;
                    fatal_cleanup.native_drained = report.outcome.drained();
                    fatal_cleanup.abandoned_scanouts = report.abandoned_scanouts;
                    crate::session_println!(
                        "sophia_live_session_native_suspend schema=2 outcome={} drained={} abandoned_scanouts={} skipped_present={} error={error}",
                        report.outcome.reduced_name(),
                        report.outcome.drained(),
                        report.abandoned_scanouts,
                        report.skipped_present.map_or_else(
                            || "none".to_owned(),
                            |transaction| transaction.raw().to_string()
                        ),
                    );
                } else {
                    crate::session_println!(
                        "sophia_live_session_native_suspend schema=2 outcome=error drained=false abandoned_scanouts=unknown skipped_present=unknown detach_established=false error={error}"
                    );
                }
                cleanup_failures.push(format!("native completion drain failed: {error}"));
            }
        }
        cpu_visual_progress.observe_native_scanout(native_scanout, Instant::now());
        if detach_established && runtime.validate_native_retirement_disposition().is_ok() {
            match native_scanout.clear_renderer_images() {
                Ok(evicted_renderer_images) => {
                    fatal_cleanup.renderer_images_cleared = true;
                    crate::session_println!(
                        "sophia_live_renderer_images schema=1 status=cleared evicted={evicted_renderer_images}"
                    );
                }
                Err(error) => {
                    crate::session_println!("sophia_live_renderer_images schema=1 status=error error={error}");
                    cleanup_failures.push(format!("renderer-image cleanup failed: {error}"));
                }
            }
        } else {
            crate::session_println!(
                "sophia_live_renderer_images schema=1 status=retained reason=native_disposition_not_established"
            );
            cleanup_failures.push(
                "renderer images retained because native disposition was not established".to_owned(),
            );
        }
    }
    if let Some(runtime) = runtime.as_mut() {
        let shutdown = runtime
            .validate_native_retirement_disposition()
            .map_err(Box::<dyn std::error::Error>::from)
            .and_then(|()| runtime.shutdown_presentations());
        match shutdown {
            Ok(report) => {
                fatal_cleanup.presentations_shutdown = true;
                match present_observer.drain_pending_feedback_with_layout(runtime, &mut present_feedback, &mut layout, |_, _, _| None) {
                    Ok(()) => {}
                    Err(error) => cleanup_failures
                        .push(format!("presentation feedback cleanup failed: {error}")),
                }
                present_observer.observe_disconnect(report);
                present_observer.emit_progress(true);
            }
            Err(error) => {
                cleanup_failures.push(format!("presentation shutdown failed: {error}"));
            }
        }
    }
    });
    if let Some((schema, source, original)) = terminal_client_error
        .as_ref()
        .map(|(source, original)| ("client_fatal", *source, original))
        .or_else(|| {
            terminal_runtime_error
                .as_ref()
                .map(|original| ("runtime_fatal", "owner_loop", original))
        })
    {
        let clean = fatal_cleanup.clean() && cleanup_failures.is_empty();
        crate::session_println!(
            "sophia_live_session_{schema} schema=1 status={} source={source} frontend_intake_stopped={} native_heads_in_flight_before={} native_cleanup_required={} native_suspend_attempted={} native_suspend_reported={} native_drained={} abandoned_scanouts={} renderer_images_cleared={} presentations_shutdown={} cleanup_errors={}",
            if clean { "cleaned" } else { "cleanup_failed" },
            fatal_cleanup.frontend_intake_stopped,
            fatal_cleanup.native_heads_in_flight_before,
            fatal_cleanup.native_cleanup_required,
            fatal_cleanup.native_suspend_attempted,
            fatal_cleanup.native_suspend_reported,
            fatal_cleanup.native_drained,
            fatal_cleanup.abandoned_scanouts,
            fatal_cleanup.renderer_images_cleared,
            fatal_cleanup.presentations_shutdown,
            cleanup_failures.len(),
        );
        *failure_phase = original_failure_phase;
        return Err(settle_session_fatal_error(original, fatal_cleanup, &cleanup_failures).into());
    }
    if let Some(error) = cleanup_failures.into_iter().next() {
        return Err(error.into());
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::InputTiming;
    if input_presented_latency.is_none()
        && input_pixel_change
        && let Some(started) = input_proof_started_at
        && native_scanout.as_ref().is_none_or(|native| {
            input_change_submission_baseline.is_some_and(|baseline| {
                native
                    .heads
                    .first()
                    .is_some_and(|head| head.presented_submissions > baseline)
            })
        })
    {
        input_presented_latency = Some(started.elapsed());
    }
    if let (Some(ingress_msec), Some(presented_ust_usec)) =
        (input_raw_ingress_msec, input_presented_ust_usec)
    {
        let ingress_ust_usec = ingress_msec
            .checked_mul(1_000)
            .ok_or("physical input ingress timestamp overflowed microseconds")?;
        let full_chain_usec = presented_ust_usec.checked_sub(ingress_ust_usec).ok_or(
            "physical input and page-flip timestamps were not in the same monotonic clock domain",
        )?;
        let full_chain = Duration::from_micros(full_chain_usec);
        let submit_to_page_flip = input_submit_to_page_flip
            .ok_or("physical input frame retired without submit-to-page-flip timing")?;
        let input_to_submit = full_chain.saturating_sub(submit_to_page_flip);
        let queue_dwell = input_queue_dwell
            .ok_or("physical input frame retired without per-event queue-dwell timing")?;
        let dwell_to_submit = input_to_submit.saturating_sub(queue_dwell);
        input_presented_latency = Some(full_chain);
        crate::session_println!(
            "sophia_live_input_latency schema=1 status=complete source=libinput_to_kernel_page_flip ingress_msec={} queue_dwell_msec={} dwell_to_submit_msec={} submit_to_page_flip_msec={} full_chain_msec={}",
            ingress_msec,
            queue_dwell.as_millis(),
            dwell_to_submit.as_millis(),
            submit_to_page_flip.as_millis(),
            full_chain.as_millis(),
        );
    }

    // The distribution beside the single correlation above. Microseconds, not
    // milliseconds: a threshold of half a 60 Hz refresh is 8.3 ms, and a
    // millisecond-rounded percentile cannot be compared against it honestly.
    if let Some(summary) = input_latency_samples.summary() {
        crate::session_println!(
            "sophia_live_input_latency_distribution schema=2 status=complete source=libinput_to_kernel_page_flip samples={} evicted={} abandoned={} unsettled={} min_usec={} p50_usec={} p95_usec={} p99_usec={} max_usec={} max_queue_dwell_usec={} max_submit_to_page_flip_usec={} p99_submit_to_page_flip_usec={} p99_dwell_to_submit_usec={} max_dwell_to_submit_usec={}",
            summary.samples,
            summary.evicted,
            summary.abandoned,
            summary.pending,
            summary.min_usec,
            summary.p50_usec,
            summary.p95_usec,
            summary.p99_usec,
            summary.max_usec,
            summary.max_queue_dwell_usec,
            summary.max_submit_to_page_flip_usec,
            summary.p99_submit_to_page_flip_usec,
            summary.p99_dwell_to_submit_usec,
            summary.max_dwell_to_submit_usec,
        );
        std::io::stdout().flush()?;
    }

    *failure_phase = crate::diagnostics::SessionFailurePhase::FrameValidation;
    let report = scene
        .last_report()
        .ok_or("persistent live session received no composable X pixels")?;
    *failure_phase = crate::diagnostics::SessionFailurePhase::InputProof;
    include!("completion/input_proof.rs");
    let recovery_extent_count = layout.recovery_extent_count();
    let standing_target_count = layout.standing_target_count();
    if recovery_extent_count != 0
        || standing_target_count != 0
        || layout.constraint_relayout_required()
    {
        return Err(crate::diagnostics::SessionCompletionFailure::IncompleteLayoutRecovery.into());
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::ApplicationProof;
    include!("completion/firefox_proof.rs");
    *failure_phase = crate::diagnostics::SessionFailurePhase::LayoutValidation;
    if config.surface_resize_requested() && !resize_proof_complete {
        return Err(crate::diagnostics::SessionCompletionFailure::ResizeProofIncomplete.into());
    }
    if let Some(wm_session) = wm_session.as_ref()
        && wm_session.committed == 0
    {
        return Err(crate::diagnostics::SessionCompletionFailure::NoCommittedLayout.into());
    }
    let pending_work = [
        layout.pending.is_some(), pending_wm_update.is_some(),
        wm_session.as_ref().is_some_and(|wm| wm.in_flight_request_count() != 0),
        !committed_session_actions.is_empty(), session_launches.pending_len() != 0,
        session_launches.admission().is_some(), !input_delivery.pending.is_empty(),
        output_topology_owner.input_quarantined(),
        wm_session.as_ref().is_some_and(|wm| wm.degraded),
    ].into_iter().enumerate().fold(0u16, |mask, (index, pending)| mask | (u16::from(pending) << index));
    if config.normal_session && pending_work != 0 {
        crate::session_println!("sophia_live_session_completion schema=1 status=failed pending_mask={pending_work} content=redacted");
        return Err(crate::diagnostics::SessionCompletionFailure::PendingWork(pending_work).into());
    }
    let native_totals = native_evidence.snapshot(native_scanout.as_ref());
    if let Some(native) = native_scanout.as_ref() {
        native_evidence.close(&NativeEvidenceSnapshot::capture(native), "completion");
    }
    let native_in_flight = native_totals.in_flight || runtime
        .as_ref()
        .is_some_and(LiveProductionVisualRuntime::native_scanout_in_flight)
        || native_scanout
            .as_ref()
            .is_some_and(LiveProductionNativeScanout::any_head_scanout_in_flight);
    let native_cleanup_pending = native_totals.cleanup_pending || runtime
        .as_ref()
        .is_some_and(LiveProductionVisualRuntime::native_cleanup_pending)
        || native_scanout
            .as_ref()
            .is_some_and(LiveProductionNativeScanout::any_head_cleanup_pending);
    crate::session_println!(
        "sophia_session_launches schema=2 status=complete peak_depth={} rejected={} admission_timeouts={} withdrawn={}",
        session_launches.peak_depth(),
        session_launches.rejected(),
        session_launches.timed_out(),
        session_launches.withdrawn(),
    );
    let input_stats = physical_input
        .as_ref()
        .map_or_else(Default::default, |input| input.stats());
    if let Some(input) = physical_input.as_ref() {
        let policy = input.policy_report();
        crate::session_println!(
            "sophia_live_session_input_devices schema=1 source={} added={} removed={} active={} keyboards={} pointers={} touch={}",
            if policy.udev_managed { "udev" } else { "paths" },
            policy.devices_added,
            policy.devices_removed,
            policy.active_devices,
            policy.keyboards,
            policy.pointers,
            policy.touch_devices,
        );
        crate::session_println!(
            "sophia_live_session_input_device schema=1 status=summary fallbacks={}",
            policy.identity_fallbacks,
        );
    }
    let native_resources = native_totals.resources;
    let native_target_creations = native_resources.target_creations;
    let native_target_recreations = native_resources.target_recreations;
    let native_pipeline_creations = native_resources.pipeline_creations;
    let native_frame_surface_creations = native_resources.frame_surface_creations;
    let native_uploads = native_resources.uploads;
    let native_max_target_create = native_resources.max_target_create;
    let native_max_frame_surface_create = native_resources.max_frame_surface_create;
    let native_max_render = native_resources.max_render;
    let native_max_upload = native_resources.max_upload;
    let direct_scanout_totals = native_totals.direct;
    include!("completion/resource_metrics.rs");
    let present_observation = &present_observer;
    // Present dispositions, always emitted, kept apart from the session line
    // so that separating a direct flip from a retained one does not require
    // bumping a schema forty readers agree on. `complete_flip` here is the
    // `Retained` disposition alone; the session line reports X completion
    // modes and adds the two together.
    crate::session_println!(
        "sophia_live_present_dispositions schema=1 status=complete complete_copy={} complete_flip={} complete_direct={} complete_skip={} idle={}",
        present_observation.complete_copy,
        present_observation.complete_flip,
        present_observation.complete_direct(),
        present_observation.complete_skip,
        present_observation.idle,
    );
    if let Some(cadence) = present_observation.displayed_cadence.summary() {
        crate::session_println!(
            "sophia_live_present_cadence schema=1 status=complete samples={} advancing_intervals={} nonadvancing={} overflowed=false mean_fps={:.3} p95_frame_msec={:.3} evicted={}",
            cadence.samples,
            cadence.advancing_intervals,
            cadence.nonadvancing,
            cadence.mean_fps,
            cadence.p95_frame_msec,
            present_observation.displayed_cadence.evicted,
        );
    } else {
        crate::session_println!(
            "sophia_live_present_cadence schema=1 status=unavailable samples={} advancing_intervals={} nonadvancing={} overflowed=false evicted={}",
            present_observation
                .displayed_cadence
                .intervals_usec
                .len()
                .saturating_add(usize::from(
                    present_observation.displayed_cadence.first_ust.is_some()
                )),
            present_observation.displayed_cadence.intervals_usec.len(),
            present_observation.displayed_cadence.nonadvancing,
            present_observation.displayed_cadence.evicted,
        );
    }
    // `input_pixel_change` and `input_text_match` are results of the physical
    // input proof, and are false when no proof was configured as well as when a
    // configured one failed. `sophia_live_session_input_proof` at startup says
    // which session this is.
    *failure_phase = crate::diagnostics::SessionFailurePhase::Startup;
    let startup_proof_elapsed = if startup_proof_requested {
        (startup_ready_msec.ok_or_else(|| {
            // Readiness waits for the focused surface's present to settle, so
            // say what stopped it settling. Reporting only that readiness was
            // missed sends its reader back to the source to guess between a
            // present that never got a turn and one simply overtaken.
            format!(
                "persistent live session never reached startup readiness: surface={} focus_applied={} visual_detail={} {}",
                usize::from(startup_readiness.surface.is_some()),
                usize::from(startup_readiness.client_focus_applied),
                usize::from(startup_readiness.visual_detail),
                runtime.as_ref().map_or_else(
                    || "defers=none".to_owned(),
                    LiveProductionVisualRuntime::present_supersession_report,
                ),
            )
        })?).to_string()
    } else {
        "not_requested".to_owned()
    };
    crate::session_println!(
        "sophia_live_session schema={} status=bounded_complete display={} elapsed_msec={} startup_ready_msec={} session_ticks={} authority_batches={} authority_transactions={} authority_queue_capacity={} authority_batches_dropped=0 backend_ticks={} runtime_committed={} runtime_surfaces={} runtime_max_surfaces={} cpu_layers={} cpu_max_layers={} cpu_nonzero_pixel_bytes={} cpu_max_nonzero_pixel_bytes={} cpu_nonzero_frames={} cpu_checksum={} cpu_max_compose_msec={} injected_input={} input_events_expected={} input_events_flushed={} input_flush_latency_msec={} input_pixel_change={} input_text_match={} input_presented_latency_msec={} input_dispatch_max_gap_msec={} input_queue_max_depth={} input_queue_dwell_max_msec={} physical_events={} physical_keys_routed={} pointer_pixel_change={} physical_pointer_events={} physical_pointer_routed={} pointer_proof={} native_presentation={} native_submissions={} native_submit_deferred={} native_submit_failures={} native_retirements={} native_retire_failures={} native_max_in_flight_ticks={} native_max_submit_to_page_flip_msec={} native_max_upload_msec={} native_max_target_create_msec={} native_max_frame_surface_create_msec={} native_max_render_msec={} native_target_creations={} native_target_recreations={} native_pipeline_creations={} native_frame_surface_creations={} native_frame_uploads={} native_callback_accepted={} native_callback_rejected={} native_callback_queue_saturated={} native_nonzero_exports={} native_mixed_exports={} native_export_attempts={} native_in_flight={} native_cleanup_pending={} physical_input={} wm_policy={} wm_requests={} wm_committed={} wm_restarts={} wm_degraded={} namespace_profile={} output_update={} output_notifications={} surface_resize={} present_complete_copy={} present_complete_flip={} present_complete_skip={} present_idle={} present_complete_routed={} present_idle_routed={} present_route_failures={} present_idle_fence_triggers={} present_disconnect_sources={} present_disconnect_fences={} present_disconnect_failures={} present_live_sources={} present_live_fences={} present_live_transactions={} present_acquire_waits={} present_controlled_rejections={}",
        if startup_proof_requested { 18 } else { 19 },
        config.display,
        started.elapsed().as_millis(),
        startup_proof_elapsed,
        session_ticks,
        batches,
        transactions,
        SESSION_AUTHORITY_CAPACITY,
        backend_ticks,
        runtime_committed,
        runtime_surfaces,
        runtime_max_surfaces,
        report.layers_composed,
        scene.max_layers_composed(),
        report.nonzero_pixel_bytes,
        scene.max_nonzero_pixel_bytes(),
        scene.nonzero_frames(),
        report.checksum,
        max_compose.as_millis(),
        config.inject_text.is_some(),
        input_delivery.events_expected,
        input_delivery.events_flushed,
        input_delivery
            .flush_latency
            .map_or(0, |duration| duration.as_millis()),
        input_pixel_change,
        input_text_match,
        input_presented_latency
            .map(|latency| latency.as_millis().to_string())
            .unwrap_or_else(|| "none".to_owned()),
        input_stats.max_dispatch_gap_msec,
        input_stats.max_queue_depth,
        input_stats.max_queue_dwell_msec,
        physical_events,
        physical_keys_routed,
        pointer_pixel_change,
        physical_pointer_events,
        physical_pointer_routed,
        if config.expect_physical_pointer {
            "enabled"
        } else {
            "disabled"
        },
        if native_evidence.enabled() {
            "enabled"
        } else {
            "disabled"
        },
        native_totals.submissions,
        native_totals.submit_deferred,
        native_totals.submit_failures,
        native_totals.retirements,
        native_totals.retire_failures,
        native_totals.max_in_flight_ticks,
        native_totals.max_submit_to_page_flip.as_millis(),
        native_max_upload.as_millis(),
        native_max_target_create.as_millis(),
        native_max_frame_surface_create.as_millis(),
        native_max_render.as_millis(),
        native_target_creations,
        native_target_recreations,
        native_pipeline_creations,
        native_frame_surface_creations,
        native_uploads,
        native_totals.callback_accepted,
        native_totals.callback_rejected,
        native_totals.callback_queue_saturated,
        native_totals.nonzero_exports,
        native_totals.mixed_exports,
        native_totals.export_attempts,
        native_in_flight,
        native_cleanup_pending,
        if physical_input.is_some() {
            "enabled"
        } else {
            "disabled"
        },
        if wm_session.is_some() {
            "external"
        } else {
            "disabled"
        },
        wm_session.as_ref().map_or(0, |wm| wm.requests),
        wm_session.as_ref().map_or(0, |wm| wm.committed),
        wm_session.as_ref().map_or(0, |wm| wm.restarts),
        wm_session.as_ref().is_some_and(|wm| wm.degraded),
        match config.namespace_profile {
            NamespaceProfile::ClassicShared => "classic_shared",
            NamespaceProfile::Confined => "confined",
        },
        if config.inject_output_size.is_some() {
            "applied"
        } else {
            "disabled"
        },
        output_notifications,
        if config.surface_resize_requested() && resize_proof_complete {
            "committed"
        } else {
            "disabled"
        },
        present_observation.complete_copy,
        present_observation.complete_flip_modes(),
        present_observation.complete_skip,
        present_observation.idle,
        present_observation.complete_routed,
        present_observation.idle_routed,
        present_observation.route_failures,
        present_observation.idle_fence_triggers,
        present_observation.disconnect_sources,
        present_observation.disconnect_fences,
        present_observation.disconnect_failures,
        runtime
            .as_ref()
            .map_or(0, |runtime| runtime.diagnostics().live_sources),
        runtime
            .as_ref()
            .map_or(0, |runtime| runtime.diagnostics().live_fences),
        runtime
            .as_ref()
            .map_or(0, |runtime| { runtime.diagnostics().live_presentations }),
        runtime
            .as_ref()
            .map_or(0, |runtime| runtime.diagnostics().acquire_waits),
        runtime
            .as_ref()
            .map_or(0, |runtime| runtime.diagnostics().controlled_rejections),
    );
    if config.admit_xtest {
        crate::session_println!(
            "sophia_live_session_xtest schema=1 status=complete admitted=true issued={} denied={} injected_keys={} injected_buttons={} injected_motions={} refused={}",
            x_frontend::xtest::LiveXTestEvidence::get(&xtest_evidence.issued),
            x_frontend::xtest::LiveXTestEvidence::get(&xtest_evidence.denied),
            x_frontend::xtest::LiveXTestEvidence::get(&xtest_evidence.injected_keys),
            x_frontend::xtest::LiveXTestEvidence::get(&xtest_evidence.injected_buttons),
            x_frontend::xtest::LiveXTestEvidence::get(&xtest_evidence.injected_motions),
            x_frontend::xtest::LiveXTestEvidence::get(&xtest_evidence.refused),
        );
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::PresentationValidation;
    if let Some(runtime) = runtime.as_ref()
        && (present_observation.disconnect_failures != 0
            || runtime.diagnostics().live_sources != 0
            || runtime.diagnostics().live_fences != 0
            || runtime.diagnostics().live_presentations != 0
            || present_observation.idle
                != present_observation
                    .complete_copy
                    .saturating_add(present_observation.complete_flip_modes())
                    .saturating_add(present_observation.complete_skip))
    {
        return Err("persistent Present resources did not retire exactly once".into());
    }
    if native_evidence.enabled()
        && (!native_totals.clean() || native_in_flight || native_cleanup_pending
            || native_evidence.unsettled_owners != 0 || native_evidence.settlement_failures != 0)
    {
        return Err(format!(
            "persistent native scanout did not submit, retire, and drain cleanly: overlap_rejections={} phase_rejections={} unsettled_owners={}",
            native_totals.vsync_overlap_rejections,
            native_totals.page_flip_phase_rejections,
            native_evidence.unsettled_owners,
        ).into());
    }
    if let Some(native_scanout) = native_scanout.as_ref() {
        crate::session_println!(
            "sophia_live_vsync schema=1 status=complete outputs={} overlap_rejections={} phase_rejections={} policy=page_flip_paced",
            native_scanout.heads.len(),
            native_totals.vsync_overlap_rejections,
            native_totals.page_flip_phase_rejections,
        );
        let mut content_evidence = Vec::with_capacity(native_scanout.heads.len());
        for head in &native_scanout.heads {
            let Some(content) = head.presented_content else {
                return Err(format!(
                    "native output {} connector {} has no presented logical content identity",
                    head.output.id.raw(),
                    head.selection.connector_id(),
                )
                .into());
            };
            let evidence = NativeOutputContentEvidence {
                output: head.output.id,
                scene_generation: content.frame().raw(),
                logical_content_checksum: head.presented_logical_checksum,
                head_pixel_checksum: None,
            };
            // Emitted once per head despite its name, so a mirror group produces
            // several records naming one output. Verifiers that mean to count
            // outputs must count distinct identities rather than records; the
            // per-head reading of the same counters is the schema=2 record below.
            crate::session_println!(
                "sophia_live_output schema=1 status=complete output={} checksum={} submissions={} retirements={} callbacks={} nonzero_exports={}",
                head.output.id.raw(),
                evidence.logical_content_checksum,
                head.submissions,
                head.retirements,
                head.callback_accepted,
                head.nonzero_exports,
            );
            crate::session_println!(
                "sophia_live_native_head schema=3 status=complete output={} head={} scene_generation={} logical_content_checksum={} head_pixel_checksum={} submissions={} retirements={} callbacks={} nonzero_exports={}",
                head.output.id.raw(),
                head.head.raw(),
                evidence.scene_generation,
                evidence.logical_content_checksum,
                crate::native_output_completion::head_pixel_checksum_field(
                    evidence.head_pixel_checksum
                ),
                head.submissions,
                head.retirements,
                head.callback_accepted,
                head.nonzero_exports,
            );
            content_evidence.push(evidence);
        }
        let incomplete_independent_head = native_scanout.heads.iter().any(|head| {
            !independent_native_output_presented(
                head.submissions,
                head.retirements,
                head.callback_accepted,
                head.initial_modeset_submission.is_some(),
            )
        });
        if incomplete_independent_head && !physical_output_topology_replaced {
            return Err(
                "one or more native outputs did not present and retire independently".into(),
            );
        }
        if !native_session_exported_pixels(
            native_scanout.heads.iter().map(|head| head.nonzero_exports),
        ) && !physical_output_topology_replaced
        {
            return Err("no native output exported nonzero pixels".into());
        }
        crate::session_println!(
            "sophia_live_native_completion schema=1 status=verified profile={} publication_generation={} initial_generation={} heads={}",
            if physical_output_topology_replaced {
                "topology_replacement"
            } else {
                "steady"
            },
            output_topology_owner.publication_generation,
            initial_output_publication_generation,
            native_scanout.heads.len(),
        );
        if let Err(error) = validate_native_output_content_evidence(content_evidence) {
            return Err(match error {
                NativeOutputContentEvidenceError::MirrorGenerationMismatch {
                    output,
                    expected,
                    actual,
                } => format!(
                    "mirrored native heads disagree for logical output {}: expected scene generation {expected}, observed {actual}",
                    output.raw(),
                ),
                NativeOutputContentEvidenceError::MirrorLogicalContentMismatch {
                    output,
                    expected,
                    actual,
                } => format!(
                    "mirrored native heads disagree for logical output {}: expected logical-content checksum {expected}, observed {actual}",
                    output.raw(),
                ),
            }
            .into());
        }
    }
    if let Some(client) = config.client.as_deref() {
        let client_name = std::path::Path::new(client)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("client");
        crate::session_println!(
            "sophia_x_application_session schema=1 status=passed class=gtk3_software client={} profile={} child_outcome=normal exit_code=0 stdout_match={} protocol_errors=0 first_error=none physical_text={} pointer_button={} surface_resize={} buffer_path=cpu_shm native_presentation={} cleanup=clean",
            client_name,
            match config.namespace_profile {
                NamespaceProfile::ClassicShared => "classic_shared",
                NamespaceProfile::Confined => "confined",
            },
            config.expect_client_stdout.is_some(),
            physical_text_proof
                .as_ref()
                .is_some_and(PhysicalTextProof::is_complete),
            physical_pointer_buttons_routed > 0,
            if config.surface_resize_requested() && resize_proof_complete {
                "committed"
            } else {
                "disabled"
            },
            if native_scanout.is_some() {
                "enabled"
            } else {
                "disabled"
            },
        );
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::ControlDrain;
    let control_metrics = session_controls.metrics();
    crate::session_println!(
        "sophia_live_session_control schema=2 status=complete enqueued={} dispatched={} delivered={} stale_retired={} rejected={} timed_out={} unexpected={} pending={} peak_depth={} max_queue_dwell_msec={} max_ack_msec={}",
        control_metrics.enqueued,
        control_metrics.dispatched,
        control_metrics.delivered,
        control_metrics.stale_targets_retired,
        control_metrics.rejected,
        control_metrics.timed_out,
        control_metrics.unexpected,
        session_controls.pending_len(),
        control_metrics.peak_depth,
        control_metrics.max_queue_dwell.as_millis(),
        control_metrics.max_acknowledgement_latency.as_millis(),
    );
    if control_metrics.quiesced_before_dispatch + control_metrics.quiesced_in_flight > 0 {
        crate::session_println!(
            "sophia_live_session_control schema=1 status=quiesced before_dispatch={} in_flight={}",
            control_metrics.quiesced_before_dispatch,
            control_metrics.quiesced_in_flight,
        );
    }
    if if input_delivery.fail_on_client_error {
        !control_metrics.is_drained(session_controls.pending_len())
    } else { !control_metrics.is_settled(session_controls.pending_len()) } {
        return Err(crate::diagnostics::SessionCompletionFailure::ControlsNotSettled.into());
    }
    // Shortcuts the profile asked for that this session cannot perform. Always
    // emitted, so `dropped=0` is the ordinary case rather than silence: a
    // session where Super+Return does nothing should say so somewhere, and
    // before this it neither said so nor started.
    {
        let profile = config.shortcut_profile_candidate.profile.as_str();
        let dropped = &config.dropped_shortcuts;
        let targets = if dropped.is_empty() {
            "none".to_owned()
        } else {
            dropped
                .iter()
                .map(|shortcut| shortcut.profile_name())
                .collect::<Vec<_>>()
                .join(",")
        };
        crate::session_println!(
            "sophia_live_session_shortcuts schema=1 status=complete profile={profile} dropped={} targets={targets} shell={}",
            dropped.len(),
            if config.shell_dropped {
                "dropped"
            } else {
                "kept"
            },
        );
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::KeyDrain;
    let key_metrics = client_keys.metrics();
    let repeat_metrics = key_repeat.metrics();
    crate::session_println!(
        "sophia_live_session_keys schema=2 status=complete pending={} release_barrier_pending={} peak_pressed={} synthetic_releases={} state_only_releases={} orphan_releases_suppressed={} removed_surface_keys={} repeat_active_seats={} repeat_armed={} repeat_routed={} repeat_pulses={} repeat_coalesced={} repeat_cancelled={} repeat_capacity_exhausted={}",
        client_keys.pending_len(),
        client_key_release_barrier.len(),
        key_metrics.peak_pressed,
        key_metrics.synthetic_releases,
        key_metrics.state_only_releases,
        key_metrics.orphan_releases_suppressed,
        key_metrics.removed_surface_keys,
        key_repeat.active_seats(),
        repeat_metrics.armed,
        key_repeats_routed,
        repeat_metrics.pulses,
        repeat_metrics.coalesced,
        repeat_metrics.cancelled,
        repeat_metrics.seat_capacity_exhausted,
    );
    let keyboard_coverage = keyboard_coverage.snapshot();
    crate::session_println!(
        "sophia_live_keyboard_coverage schema=1 status=complete shifted_positions={} shifted_positions_required={} virtual_terminals={} virtual_terminals_required={} content=redacted",
        keyboard_coverage.shifted_positions,
        keyboard_coverage.shifted_positions_required,
        keyboard_coverage.virtual_terminals,
        keyboard_coverage.virtual_terminals_required,
    );
    if client_keys.pending_len() != 0
        || !client_key_release_barrier.is_empty()
        || key_repeat.active_seats() != 0
        || repeat_metrics.seat_capacity_exhausted != 0
        || repeat_metrics.pulses != u64::try_from(key_repeats_routed).unwrap_or(u64::MAX)
    {
        return Err("persistent client key state did not drain cleanly".into());
    }
    Ok(())
}
