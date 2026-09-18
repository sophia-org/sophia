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
    if config.input_proof_requested()
        && input_delivery.events_expected != input_delivery.events_flushed
    {
        return Err(format!(
            "persistent live session completed with unflushed X11 input: expected={} flushed={} pending={}",
            input_delivery.events_expected,
            input_delivery.events_flushed,
            input_delivery.pending.len(),
        )
        .into());
    }
    if config.input_proof_requested() && input_delivery.flush_latency.is_none() {
        return Err("persistent live session input proof never observed flushed X11 input".into());
    }
    if config.input_proof_requested() && !input_pixel_change {
        return Err(format!(
            "persistent live session input did not change composed terminal pixels: baseline={injection_checksum:?} final_frame={} final_buffers={} input_surface={input_surface:?} input_surface_pixel_change={input_surface_pixel_change} batches={batches} transactions={transactions}",
            report.checksum,
            scene.buffer_checksum(),
        )
        .into());
    }
    if config.input_proof_requested() && input_presented_latency.is_none() {
        let native_heads = runtime.as_ref().map_or_else(
            || "none".to_owned(),
            LiveProductionVisualRuntime::native_diagnostic,
        );
        return Err(format!(
            "persistent live session input pixels were not presented: change_submission_baseline={input_change_submission_baseline:?} primary_presented_submissions={} native_submissions={} native_callbacks={} native_heads={native_heads}",
            native_scanout
                .as_ref()
                .and_then(|native| native.heads.first())
                .map_or(0, |head| head.presented_submissions),
            native_scanout.as_ref().map_or(0, |native| native.submissions),
            native_scanout
                .as_ref()
                .map_or(0, |native| native.callback_accepted),
        )
        .into());
    }
    if config.expect_physical_text.is_some()
        && native_scanout.as_ref().is_some_and(|native| {
            native.kernel_page_flip_timestamp_missing != 0
                || native.pending_kernel_page_flip_timestamps() != 0
        })
    {
        return Err(
            "physical input proof observed fallback or pending kernel page-flip timestamps".into(),
        );
    }
    if config.expect_physical_text.is_some()
        && native_scanout.is_some()
        && (input_raw_ingress_msec.is_none() || input_presented_ust_usec.is_none())
    {
        return Err(
            "physical input proof did not correlate libinput ingress to its presented frame".into(),
        );
    }
    if config.input_proof_requested() && !input_text_match {
        return Err(
            "persistent live session terminal did not receive the expected text and Return".into(),
        );
    }
    if config.expect_physical_text.is_some()
        && (!physical_text_proof
            .as_ref()
            .is_some_and(PhysicalTextProof::is_complete)
            || !physical_input_completion_reported)
    {
        return Err("persistent live session did not complete exact physical text proof".into());
    }
    if config.expect_physical_pointer
        && (!pointer_pixel_change || physical_pointer_buttons_routed == 0)
    {
        return Err(format!(
            "persistent live session pointer input did not change pixels: baseline={pointer_checksum:?} routed={physical_pointer_routed} buttons={physical_pointer_buttons_routed} observed={physical_pointer_events}"
        )
        .into());
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::ApplicationProof;
    if config.application_proof_requested() {
        let status =
            primary_exit_status.ok_or("application proof ended before the client exited")?;
        if config.require_client_normal_exit && !status.success() {
            return Err(format!("application did not exit normally: {status}").into());
        }
        if let Some(expected) = config.expect_client_stdout.as_deref()
            && client_stdout != expected.as_bytes()
        {
            return Err(format!(
                "application stdout mismatch: expected_bytes={} received_bytes={}",
                expected.len(),
                client_stdout.len()
            )
            .into());
        }
        if protocol_error_count != 0 {
            return Err(format!(
                "application emitted {protocol_error_count} X protocol errors; first={first_protocol_error:?}"
            )
            .into());
        }
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::LayoutValidation;
    let recovery_extent_count = layout.recovery_extent_count();
    let standing_target_count = layout.standing_target_count();
    if recovery_extent_count != 0
        || standing_target_count != 0
        || layout.constraint_relayout_required()
    {
        return Err(crate::diagnostics::SessionCompletionFailure::IncompleteLayoutRecovery.into());
    }
    *failure_phase = crate::diagnostics::SessionFailurePhase::ApplicationProof;
    if config.firefox_full_proof_requested() {
        if config.firefox_m10_proof && !firefox_m8_proof.complete() {
            return Err(format!(
                "Firefox M10 promotion proof incomplete: stages={}/{}",
                firefox_m8_proof.completed(),
                firefox_m8_proof.stage_count(),
            )
            .into());
        }
        if config.firefox_m8_proof
            && (!firefox_m8_proof.complete()
                || selection_owner_changes < 2
                || selection_conversions < 2)
        {
            return Err(format!(
                "Firefox M8 proof incomplete: stages={}/{} selection_owner_changes={} selection_conversions={}",
                firefox_m8_proof.completed(),
                firefox_m8_proof.stage_count(),
                selection_owner_changes,
                selection_conversions,
            )
            .into());
        }
        if config.firefox_m10_proof {
            crate::session_println!(
                "sophia_firefox_promotion schema=1 status=complete stages={} selection_gates=focused content=redacted",
                firefox_m8_proof.completed(),
            );
        } else {
            crate::session_println!(
                "sophia_firefox_m8 schema=1 status=complete stages={} selection_owner_changes={} selection_conversions={} content=redacted",
                firefox_m8_proof.completed(),
                selection_owner_changes,
                selection_conversions,
            );
        }
    }
    if config.firefox_m10_rendering_proof {
        if !firefox_m10_rendering_page_ready {
            return Err("Firefox M10 rendering proof did not observe its ready document".into());
        }
        crate::session_println!(
            "sophia_firefox_rendering schema=1 status=complete page_ready=true recovery_extents=0 content=redacted"
        );
    }
    if config.firefox_m10_dialog_proof {
        if !firefox_m10_dialog_proof.complete() || physical_pointer_buttons_routed < 4 {
            return Err(format!(
                "Firefox M10 dialog proof incomplete: checkpoints={}/{} pointer_buttons={physical_pointer_buttons_routed}",
                firefox_m10_dialog_proof.completed,
                FirefoxM10DialogProof::CHECKPOINTS.len(),
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_dialog schema=1 status=complete checkpoints=3 pointer_buttons={physical_pointer_buttons_routed} recovery_extents=0 content=redacted"
        );
    }
    if config.firefox_m10_primary_proof {
        if !firefox_m10_primary_proof.complete()
            || selection_owner_changes < 2
            || selection_conversions < 2
        {
            return Err(format!(
                "Firefox M10 PRIMARY proof incomplete: checkpoints={}/{} selection_owner_changes={} selection_conversions={}",
                firefox_m10_primary_proof.completed,
                FirefoxM10PrimaryProof::CHECKPOINTS.len(),
                selection_owner_changes,
                selection_conversions,
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_primary schema=1 status=complete checkpoints=3 selection_owner_changes={selection_owner_changes} selection_conversions={selection_conversions} content=redacted"
        );
    }
    if config.firefox_m10_proof {
        if !firefox_m10_kitty_proof.complete() {
            return Err(format!(
                "Firefox M10 Kitty proof incomplete: checkpoints={}/{}",
                firefox_m10_kitty_proof.completed(),
                FirefoxM10KittyProof::CHECKPOINTS.len(),
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_m10 schema=3 status=complete kitty_checkpoints={} selection_gates=focused content=redacted",
            firefox_m10_kitty_proof.completed(),
        );
    }
    if config.firefox_m10_selection_proof {
        if firefox_m8_proof.completed() < 4
            || !firefox_m10_selection_kitty_proof.complete()
            || selection_owner_changes < 4
            || selection_conversions < 4
        {
            return Err(format!(
                "Firefox M10 selection proof incomplete: stages={}/4 checkpoints={}/3 selection_owner_changes={} selection_conversions={}",
                firefox_m8_proof.completed().min(4),
                firefox_m10_selection_kitty_proof.completed(),
                selection_owner_changes,
                selection_conversions,
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_selection schema=1 status=complete stages=4 kitty_checkpoints=3 selection_owner_changes={} selection_conversions={} content=redacted",
            selection_owner_changes,
            selection_conversions,
        );
    }
    if config.firefox_m10_lifecycle_proof {
        if !firefox_m8_page_ready_reported || !firefox_m10_kitty_proof.lifecycle_complete() {
            return Err(format!(
                "Firefox M10 lifecycle proof incomplete: page_ready={} checkpoints={}/6",
                firefox_m8_page_ready_reported,
                firefox_m10_kitty_proof.completed().min(6),
            )
            .into());
        }
        crate::session_println!(
            "sophia_firefox_lifecycle schema=1 status=complete page_ready=true kitty_checkpoints=6 content=redacted"
        );
    }
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
    crate::session_println!(
        "sophia_live_native_resources schema=12 status=complete target_creations={} pipeline_creations={} frame_surface_creations={} cpu_target_creations={} dmabuf_target_creations={} composition_target_creations={} composition_target_reuses={} generation_replacements={} recovery_replacements={} snapshot_captures={} snapshot_promotions={} snapshot_rollbacks={} snapshot_evictions={} snapshot_live_entries={} snapshot_live_bytes={} import_cache_imports={} import_cache_hits={} import_cache_evictions={} import_cache_live_entries={} import_cache_descriptor_mismatches={} import_cache_capacity_rejections={} exact_nearest_draws={} sharp_downscale_draws={} sharp_upscale_draws={} linear_fallback_draws={} worker_requests={} worker_completions={} worker_failures={} worker_soft_stalls={} worker_hard_stalls={} worker_release_enqueue_failures={} frame_slot_acquisitions={} frame_slot_reuses={} frame_slot_deferrals={} frame_slot_stale_releases={} frame_slots_leased={} frame_slots_high_watermark={} max_in_flight_per_output={} pending_frame_supersessions={} frame_slot_partial_repaints={} frame_slot_full_repaints={} frame_slot_history_invalidations={} frame_slot_history_records={} max_worker_request_msec={} renderer_workers={} worker_result_misroutes={} worker_max_service_skew={} direct_scanout_attempts={} direct_scanout_flips={} direct_scanout_tests={} direct_scanout_test_rejections={} direct_scanout_refusals={} direct_scanout_unsupported={} direct_scanout_fallbacks={}",
        native_resources.target_creations,
        native_resources.pipeline_creations,
        native_resources.frame_surface_creations,
        native_resources.cpu_target_creations,
        native_resources.dmabuf_target_creations,
        native_resources.composition_target_creations,
        native_resources.composition_target_reuses,
        native_resources.generation_replacements,
        native_resources.recovery_replacements,
        native_resources.snapshot_captures,
        native_resources.snapshot_promotions,
        native_resources.snapshot_rollbacks,
        native_resources.snapshot_evictions,
        native_resources.snapshot_live_entries,
        native_resources.snapshot_live_bytes,
        native_resources.import_cache_imports,
        native_resources.import_cache_hits,
        native_resources.import_cache_evictions,
        native_resources.import_cache_live_entries,
        native_resources.import_cache_descriptor_mismatches,
        native_resources.import_cache_capacity_rejections,
        native_resources.exact_nearest_draws,
        native_resources.sharp_downscale_draws,
        native_resources.sharp_upscale_draws,
        native_resources.linear_fallback_draws,
        native_resources.worker_requests,
        native_resources.worker_completions,
        native_resources.worker_failures,
        native_resources.worker_soft_stalls,
        native_resources.worker_hard_stalls,
        native_resources.worker_release_enqueue_failures,
        native_resources.frame_slot_acquisitions,
        native_resources.frame_slot_reuses,
        native_resources.frame_slot_deferrals,
        native_resources.frame_slot_stale_releases,
        native_resources.frame_slots_leased,
        native_resources.frame_slots_high_watermark,
        native_totals.max_in_flight_per_output,
        native_totals.pending_frame_supersessions,
        native_resources.frame_slot_partial_repaints,
        native_resources.frame_slot_full_repaints,
        native_resources.frame_slot_history_invalidations,
        native_resources.frame_slot_history_records,
        native_resources.max_worker_request.as_millis(),
        native_resources.renderer_workers,
        native_resources.worker_result_misroutes,
        native_totals.max_service_skew,
        direct_scanout_totals.attempts,
        direct_scanout_totals.flips,
        direct_scanout_totals.tests,
        direct_scanout_totals.test_rejections,
        direct_scanout_totals.refusals,
        direct_scanout_totals.unsupported,
        direct_scanout_totals.fallbacks,
    );
    // Why frames were or were not eligible, so a run in which direct scanout
    // never fired can say which of "the path was off", "the scene was never
    // eligible", and "the proof is wrong" it was. Zeros in the counters above
    // cannot distinguish those, and that is the first question anyone asks of
    // a gate that measured nothing.
    {
        let heads = native_scanout.as_ref().map_or_else(
            Vec::new,
            sophia_backend_live::LiveProductionNativeScanout::direct_scanout_head_verdicts,
        );
        let totals = native_totals.verdicts;
        for (output, head, verdicts) in &heads {
            let mut record = format!(
                "sophia_live_direct_scanout_verdicts schema=2 status=head output={} head={}",
                output.raw(),
                head.raw(),
            );
            for (verdict, count) in
                std::iter::zip(sophia_engine::DirectScanoutVerdict::VERDICTS, verdicts)
            {
                record.push_str(&format!(" {}={count}", verdict.reduced_name()));
            }
            crate::session_println!("{record}");
        }
        let mut record =
            String::from("sophia_live_direct_scanout_verdicts schema=2 status=complete");
        for (verdict, count) in
            std::iter::zip(sophia_engine::DirectScanoutVerdict::VERDICTS, totals)
        {
            record.push_str(&format!(" {}={count}", verdict.reduced_name()));
        }
        crate::session_println!("{record}");
    }
    // What a frame cost, split by how it reached the plane. Direct scanout
    // skips a composition pass, so the offer-to-submit half is where a
    // difference has to show; the submit-to-flip half is measured beside it
    // because "the display engine does not care how the buffer got there" is
    // an assumption worth being able to check.
    //
    // Absent populations are omitted rather than reported as zero. A session
    // that never composed has nothing to compare against, and a zero would
    // read as free instead of as absent.
    if native_evidence.enabled() {
        let cost = &native_totals.cost;
        for (population, samples) in [("direct", &cost.direct), ("composed", &cost.composed)] {
            let offer = samples.offer_to_submit.summary();
            let flip = samples.submit_to_flip.summary();
            // Absent means absent: a population with no samples at all never
            // happened, and belongs in no record. But a population with only
            // one half *did* happen and was half-measured, which is a defect
            // in the measuring rather than a fact about the run -- so it is
            // reported with the empty half showing zero frames, where the
            // gate can name it. Requiring both halves here instead made the
            // whole record vanish, and the only symptom was the comparison
            // reporting no direct frames at all.
            if offer.is_none() && flip.is_none() {
                continue;
            }
            let offer = offer.unwrap_or(sophia_backend_live::DirectScanoutCostSummary {
                frames: 0,
                min: 0,
                p50: 0,
                p99: 0,
                max: 0,
                saturated: false,
            });
            let flip = flip.unwrap_or(sophia_backend_live::DirectScanoutCostSummary {
                frames: 0,
                min: 0,
                p50: 0,
                p99: 0,
                max: 0,
                saturated: false,
            });
            crate::session_println!(
                "sophia_live_direct_scanout_cost schema=1 population={population} frames={} offer_submit_us_min={} offer_submit_us_p50={} offer_submit_us_p99={} offer_submit_us_max={} submit_flip_frames={} submit_flip_us_min={} submit_flip_us_p50={} submit_flip_us_p99={} submit_flip_us_max={} saturated={}",
                offer.frames,
                offer.min,
                offer.p50,
                offer.p99,
                offer.max,
                flip.frames,
                flip.min,
                flip.p50,
                flip.p99,
                flip.max,
                offer.saturated || flip.saturated,
            );
        }
    }
    if native_evidence.enabled() {
        crate::session_println!(
            "sophia_live_page_flip_clock schema=1 status=complete source=kernel_monotonic timestamps={} fallbacks={} pending={}",
            native_totals.kernel_page_flip_timestamps,
            native_totals.kernel_page_flip_timestamp_missing,
            native_scanout.as_ref().map_or(0, LiveProductionNativeScanout::pending_kernel_page_flip_timestamps),
        );
    }
    // The sampled population, reported without a verdict. Whether it grew is
    // decided by the verifier from the samples themselves: an emitter that
    // graded its own health would be the only witness to its own failure.
    resource_sampler.report();
    // Schema 2 adds what the copy-on-write backing costs and bounds.
    //
    // `cpu_cow_splits` counts patches that had to copy because a presentation
    // still held the bytes. Near zero is the steady state; tracking the update
    // count means presentations outlive the updates that follow them, which is
    // real work rather than a defect but is not the work this path was
    // optimized for. `cpu_resident_buffers_peak` and `cpu_resident_bytes_peak`
    // are what "bounded" is a claim about: a registry that ends empty having
    // peaked at a thousand buffers reads identically to one that never held
    // more than three, and only the second is bounded.
    crate::session_println!(
        "sophia_live_rendering_efficiency schema=2 status=complete cpu_updates={} cpu_replacements={} cpu_patch_updates={} cpu_patch_rects={} cpu_payload_bytes={} exact_pixel_metric_frames={} damage_scoped_metric_frames={} composition_target_reuses={} cpu_cow_splits={} cpu_resident_buffers_peak={} cpu_resident_bytes_peak={}",
        cpu_buffer_updates,
        cpu_buffer_replacements,
        cpu_buffer_patch_updates,
        cpu_buffer_patch_rects,
        cpu_buffer_payload_bytes,
        scene.exact_pixel_metric_frames(),
        scene.damage_scoped_metric_frames(),
        native_resources.composition_target_reuses,
        scene.cpu_cow_splits(),
        scene.peak_resident_buffers(),
        scene.peak_resident_buffer_bytes(),
    );
    crate::session_println!(
        "{}",
        cpu_visual_progress.record(Instant::now(), startup_ready_msec.unwrap_or_default())
    );
    crate::session_println!(
        "sophia_live_session_scheduler schema=2 authority_batches={batches} cpu_compositions={cpu_compositions} coalesced_batches={coalesced_batches} cadence_deferred_batches={cadence_deferred_batches} cadence_repaints={cadence_repaints} frame_interval_usec={} merged_batches={merged_batches} max_merge_run={max_merge_run}",
        primary_frame_interval.as_micros(),
    );
    crate::session_println!(
        "sophia_live_owner_timing schema=2 status=complete max_child_reap_msec={} max_input_phase_msec={}",
        max_child_reap.as_millis(),
        max_input_phase.as_millis(),
    );
    if let Some(wm) = wm_session.as_ref() {
        crate::session_println!(
            "sophia_live_wm_transport schema=2 status=complete peak_depth={} pending={} rejected={} action_ordered={} action_coalesced=0 stale_responses={} max_queue_dwell_msec={} max_round_trip_msec={}",
            wm.request_peak_depth,
            wm.pending_request_count(),
            wm.request_rejections,
            wm.action_requests_ordered,
            wm.stale_responses,
            wm.max_queue_dwell.as_millis(),
            wm.max_request().as_millis(),
        );
    }
    crate::session_println!(
        "sophia_live_session_cursor schema=6 path={} plane={} moves_coalesced={} max_motion_to_submit_msec={} initialization_max_msec={} initialization_deferrals={} max_update_msec={} updates_primary_in_flight={} buttons_routed={} hardware_updates={} hidden_updates={} hardware_failures={} queued={} backend_coalesced={} rides={} cursor_only={} combined_drops={} fallbacks={} pending={}",
        match native_scanout.as_ref().map(|scanout| scanout.cursor_path) {
            Some(sophia_backend_live::HardwareCursorPath::AtomicPlane) => "atomic_plane",
            _ => "legacy_ioctl",
        },
        // What the card would accept, which is not what the session chose.
        // A run on the legacy ioctl over a card that offers a cursor plane
        // says so, and a run claiming the atomic path over a card that
        // refused one is a contradiction a reader can catch.
        match native_scanout
            .as_ref()
            .and_then(|scanout| scanout.cursor_plane_probe())
        {
            Some(sophia_backend_live::CursorPlaneProbe::Accepted) => "accepted",
            Some(sophia_backend_live::CursorPlaneProbe::Refused) => "refused",
            None => "unprobed",
        },
        cursor_moves_coalesced,
        cursor_max_motion_to_submit.max(
            native_totals.max_cursor_queue_delay
        ).as_millis(),
        native_totals.max_cursor_initialization.as_millis(),
        native_totals.cursor_initialization_deferrals,
        native_totals.max_cursor_update.as_millis(),
        native_totals.cursor_updates_primary_in_flight,
        physical_pointer_buttons_routed,
        native_totals.cursor_updates,
        native_totals.cursor_hidden_updates,
        native_totals.cursor_update_failures,
        native_totals.cursor_updates_queued,
        native_totals.cursor_updates_coalesced,
        native_totals.cursor_updates_ridden,
        native_totals.cursor_only_commits,
        native_totals.cursor_combined_drops,
        native_totals.cursor_legacy_fallbacks,
        native_scanout
            .as_ref()
            .map_or(0, |scanout| scanout.pending_atomic_cursor_count()),
    );
    crate::session_println!(
        "sophia_live_session_health schema=1 status=clean protocol_errors={} pending_wm={} pending_actions={} pending_input={} wm_degraded={}",
        protocol_error_count,
        usize::from(layout.pending.is_some())
            .saturating_add(usize::from(pending_wm_update.is_some()))
            .saturating_add(
                wm_session
                    .as_ref()
                    .map_or(0, LiveWmSession::pending_request_count),
        ),
        committed_session_actions.len(),
        input_delivery.pending.len(),
        wm_session.as_ref().is_some_and(|wm| wm.degraded),
    );
    if let Some(monitor) = output_topology_monitor.as_ref() {
        let stats = monitor.stats();
        crate::session_println!(
            "sophia_live_output_topology_monitor schema=1 source=kernel_and_udev status=complete observed={} coalesced={} delivered={}",
            stats.observed, stats.coalesced, stats.delivered,
        );
    }
    crate::session_println!(
        "sophia_live_output_topology_health schema=1 status=clean quarantined={}",
        output_topology_owner.input_quarantined(),
    );
    crate::session_println!(
        "sophia_live_layout_health schema=2 status=clean recovery_extents={} standing_targets={} constraint_relayout_pending={}",
        recovery_extent_count,
        standing_target_count,
        layout.constraint_relayout_required(),
    );
    // WmWorkspaceState rejects hidden configure/render commands before they
    // can enter an Engine transaction; a clean completion therefore proves
    // the invariant without retaining client identity.
    crate::session_println!(
        "sophia_live_layout_authority schema=1 status=clean hidden_surface_commands=0"
    );
    crate::session_println!(
        "sophia_live_session_protocol_errors schema=1 expected={} unexpected={}",
        expected_protocol_error_count, protocol_error_count,
    );
    crate::session_println!(
        "sophia_live_selection schema=1 status=complete owner_changes={} conversions={} content=redacted",
        selection_owner_changes, selection_conversions,
    );

    if let Some(runtime) = runtime.as_ref() {
        let diagnostics = runtime.diagnostics();
        crate::session_println!(
            "sophia_live_present_scheduler schema=1 status=complete surface_content_capacity={} pending_limit=1 in_flight_limit=1 pending_supersessions={} surface_content_supersessions={} scheduler_supersessions={} max_surface_content_deferred={} max_latest_deferred_per_surface={} max_pending_queued={} max_total_queued={} max_live_sources={} max_live_fences={} max_live_presentations={} present_rejections={} native_suspend_present_rejections={} shutdown_present_rejections={} other_present_rejections={}",
            sophia_engine::SURFACE_CONTENT_STREAM_CAPACITY,
            diagnostics.pending_supersessions,
            diagnostics.surface_content_supersessions,
            diagnostics.scheduler_supersessions,
            diagnostics.max_surface_content_deferred,
            diagnostics.max_latest_deferred_per_surface,
            diagnostics.max_pending_queued,
            diagnostics.max_total_queued,
            diagnostics.max_live_sources,
            diagnostics.max_live_fences,
            diagnostics.max_live_presentations,
            diagnostics.present_rejections,
            diagnostics.native_suspend_present_rejections,
            diagnostics.shutdown_present_rejections,
            diagnostics.other_present_rejections,
        );
    }

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
        "sophia_live_session schema={} status=bounded_complete display={} elapsed_msec={} startup_ready_msec={} session_ticks={} authority_batches={} authority_transactions={} authority_queue_capacity={} authority_batches_dropped=0 backend_ticks={} runtime_committed={} runtime_surfaces={} cpu_layers={} cpu_nonzero_pixel_bytes={} cpu_max_nonzero_pixel_bytes={} cpu_nonzero_frames={} cpu_checksum={} cpu_max_compose_msec={} injected_input={} input_events_expected={} input_events_flushed={} input_flush_latency_msec={} input_pixel_change={} input_text_match={} input_presented_latency_msec={} input_dispatch_max_gap_msec={} input_queue_max_depth={} input_queue_dwell_max_msec={} physical_events={} physical_keys_routed={} pointer_pixel_change={} physical_pointer_events={} physical_pointer_routed={} pointer_proof={} native_presentation={} native_submissions={} native_submit_deferred={} native_submit_failures={} native_retirements={} native_retire_failures={} native_max_in_flight_ticks={} native_max_submit_to_page_flip_msec={} native_max_upload_msec={} native_max_target_create_msec={} native_max_frame_surface_create_msec={} native_max_render_msec={} native_target_creations={} native_target_recreations={} native_pipeline_creations={} native_frame_surface_creations={} native_frame_uploads={} native_callback_accepted={} native_callback_rejected={} native_callback_queue_saturated={} native_nonzero_exports={} native_mixed_exports={} native_export_attempts={} native_in_flight={} native_cleanup_pending={} physical_input={} wm_policy={} wm_requests={} wm_committed={} wm_restarts={} wm_degraded={} namespace_profile={} output_update={} output_notifications={} surface_resize={} present_complete_copy={} present_complete_flip={} present_complete_skip={} present_idle={} present_complete_routed={} present_idle_routed={} present_route_failures={} present_idle_fence_triggers={} present_disconnect_sources={} present_disconnect_fences={} present_disconnect_failures={} present_live_sources={} present_live_fences={} present_live_transactions={} present_acquire_waits={} present_controlled_rejections={}",
        if startup_proof_requested { 16 } else { 17 },
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
        report.layers_composed,
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
