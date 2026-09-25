{
macro_rules! schedule_output_topology_rebuild {
    ($reason:literal, $security_epoch_already_advanced:expr) => {{
        let notice_sequence = output_topology_owner
            .notice_sequence
            .checked_add(1)
            .ok_or("synthetic output topology notice sequence exhausted")?;
        let advance_security_epoch =
            output_topology_owner.begin_rescan(notice_sequence)?;
        if advance_security_epoch && !$security_epoch_already_advanced {
            let revoked_input_leases = advance_application_input_security_epoch(
                &mut application_route_leases,
                input_sender,
                &layout.client_routes,
                route_lease_release_sender,
            )?;
            revoke_floating_pointer_interaction!("output_topology");
            revoke_chrome_captures!("output_topology");
            pointer_focus_handoff = PointerFocusHandoffState::default();
            keyboard_focus_handoff = KeyboardFocusHandoffState::default();
            key_repeat.cancel_seat(seat);
            crate::session_println!(
                "sophia_live_input_epoch schema=1 reason=output_topology transition={} epoch={} revoked_leases={revoked_input_leases}",
                output_topology_owner.transition,
                application_route_leases.control_epoch(),
            );
        }
        output_topology_retry_at = Some(Instant::now());
        tracing::warn!(
            "sophia_live_output_topology schema=1 status=deferred transition={} source={} security_epoch_already_advanced={}",
            output_topology_owner.transition,
            $reason,
            $security_epoch_already_advanced,
        );
    }};
}

macro_rules! publish_resumed_topology_transport {
    ($native:expr) => {{
        if output_topology_owner.phase
            == LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Hotplug)
        {
            let rebuild = output_topology_owner
                .observe_rebuild(outputs.clone(), $native.head_fingerprint())?;
            debug_assert_eq!(rebuild, LiveOutputTopologyRebuild::TransportReplaced);
            output_topology_owner.mark_published($native.retirements, false)?;
            output_topology_retry_at = None;
            tracing::info!(
                "sophia_live_output_topology schema=1 status=published transition={} outputs={} changed=false source=seat_resume input=quarantined",
                output_topology_owner.transition,
                outputs.len(),
            );
        }
    }};
}

let mut native_frame_service_preempted_previous_cycle = false;
let mut native_frame_control_priority_cycles = 0_u8;
let mut last_native_frame_service = Instant::now();
// The cadence follows the desktop primary, not whichever head enumerated
// first. Before the session publishes an output authority there is no primary
// to follow, so this opens on the lowest enabled head and the recompute at the
// top of every pass corrects it once the publication lands.
let primary_refresh_millihz = native_scanout
    .as_ref()
    .and_then(|native| {
        native.cadence_head(
            wm_session
                .as_ref()
                .and_then(LiveWmSession::published_output_snapshot)
                .map(|snapshot| snapshot.primary_output),
        )
    })
    .map_or(60_000, |head| head.refresh_millihz)
    .max(1);
let mut primary_frame_interval = Duration::from_micros(
    (1_000_000_000_u64 / u64::from(primary_refresh_millihz)).max(1),
);
let mut primary_frame_pacer = sophia_engine::PrimaryFramePacer::new(primary_frame_interval);
// Motion is buffered here rather than inside the drain, so a pass that does
// not end on a frame boundary carries its latest motion into the next one.
let mut routed_input_coalescer = sophia_engine::RoutedInputCoalescer::new();
// When the motion it holds was first buffered. A session that composes from
// client submissions asks the pacer for almost no repaints, so the release has
// to be able to happen on the clock rather than only on a paced frame.
let mut routed_input_motion_held_since: Option<Instant> = None;
// Samples the gauges the completion record reports once, so a verifier can ask
// whether they grew rather than only whether they drained.
let mut resource_sampler = LiveResourceSampler::new(started, config.normal_session && crate::diagnostics::recording());
let mut next_surface_sample = started + Duration::from_secs(1);
let mut surface_samples = 0_u32;
let mut native_frame_service_deadline_armed = false;
let mut native_frame_idle_service_cycles = 0_u8;
let session_loop_result = (|| -> Result<(), Box<dyn std::error::Error>> {
    'session: loop {
        *failure_phase = crate::diagnostics::SessionFailurePhase::OwnerLoop;
        if let Some(refresh_millihz) = native_scanout
            .as_ref()
            .and_then(|native| {
                native.cadence_head(
                    wm_session
                        .as_ref()
                        .and_then(LiveWmSession::published_output_snapshot)
                        .map(|snapshot| snapshot.primary_output),
                )
            })
            .map(|head| head.refresh_millihz.max(1))
        {
            let interval = Duration::from_micros(
                (1_000_000_000_u64 / u64::from(refresh_millihz)).max(1),
            );
            if interval != primary_frame_interval {
                primary_frame_interval = interval;
                primary_frame_pacer.set_interval(Instant::now(), interval);
            }
        }
        // Before any phase runs, so a sample describes a settled loop rather
        // than a moment inside one. The gauge reads walk a map and read
        // /proc, which is why they happen on a cadence rather than per pass.
        let sample_now = Instant::now();
        // Opt-in startup diagnostics are bounded in both time and surface count.
        if config.verbose_diagnostics && surface_samples < 60 && sample_now >= next_surface_sample {
            surface_samples += 1;
            next_surface_sample = sample_now + Duration::from_secs(1);
            if let Some(runtime) = runtime.as_ref() {
                log_cpu_surface_sample(scene, runtime.committed_surfaces(), surface_samples);
            }
        }
        if resource_sampler.is_due(sample_now) {
            // Scheduling counters on the same cadence as the resource gauges.
            //
            // These also reach the completion record, but only there, and a
            // session that has not ended yet cannot say whether it is meeting
            // its own pacing. That made a live halving measurable from outside
            // the session and not attributable from within it. Running totals
            // rather than per-interval deltas, because two consecutive samples
            // give the rate and a delta would lose the history.
            crate::session_println!(
                "sophia_live_cadence_sample schema=1 uptime_msec={} frame_interval_usec={} cadence_repaints={} cadence_deferred_batches={} merged_batches={} max_input_phase_msec={}",
                u64::try_from(sample_now.duration_since(started).as_millis()).unwrap_or(u64::MAX),
                primary_frame_interval.as_micros(),
                metrics.cadence_repaints,
                metrics.cadence_deferred_batches,
                metrics.merged_batches,
                metrics.max_input_phase.as_millis(),
            );
            if let Some(shell) = metadata_shell.as_ref() {
                shell.record_content_accounting();
            }
            let native_resources = native_scanout.as_ref().map_or_else(
                sophia_backend_live::LivePersistentRenderMetrics::default,
                LiveProductionNativeScanout::persistent_render_metrics,
            );
            resource_sampler.record(
                sample_now,
                LiveResourceSample {
                    cpu_registry_buffers: scene.resident_buffer_count(),
                    cpu_registry_bytes: scene.resident_buffer_bytes(),
                    cpu_cow_splits: scene.cpu_cow_splits(),
                    frame_slots_leased: u32::try_from(native_resources.frame_slots_leased)
                        .unwrap_or(u32::MAX),
                    snapshot_live_entries: native_resources.snapshot_live_entries,
                    import_cache_live_entries: native_resources.import_cache_live_entries,
                },
            );
        }
        if let Some(broker) = metadata_broker.as_mut() {
            broker.poll()?;
            broker.drain_candidates(metadata_candidate_receiver)?;
        }
        let native_shell_available = runtime.as_ref().zip(native_scanout.as_ref()).is_some_and(
            |(runtime, native)| runtime.shell_content_presentation_available(native),
        );
        let shell_presentation_available = owner_loop_shell_presentation_available(
            seat_state == sophia_backend_live::LiveSeatState::Active,
            native_shell_available,
            active_output_topology_preparation
                .as_ref()
                .map(|execution| execution.phase),
            wm_session.as_ref().is_some_and(LiveWmSession::startup_output_topology_pending),
        );
        // Reconcile against current topology as well as the last committed policy.
        // A stale publication cannot reopen a removed/replaced output.
        let contexts = wm_session.as_ref().and_then(|w| w.public.as_ref()).and_then(|p| {
            p.launch_origins.lock().ok().map(|origins| origins.output_contexts().iter().copied()
                .filter(|c| p.outputs.iter().any(|o| o.id == c.output)
                    && p.output_generations.get(&c.output) == Some(&c.output_generation)).collect::<Vec<_>>())
        }).unwrap_or_default();
        session_launches.set_output_launch_contexts(&contexts);
        if shell_components.is_some() && shell_presentation_available {
            component_catalog.visit_scan(config, session_launches, xauthority)?;
        }
        if shell_presentation_available {
            if let Some((runtime, native)) = runtime.as_ref().zip(native_scanout.as_ref()) {
                content_mapping_evidence.observe(native, runtime.input_projections());
            }
            if let Some(shell) = metadata_shell.as_mut() {
                let _ = shell.set_presentation_available(true, "native_available")?;
            }
        } else {
            pause_metadata_shell_presentation!("native_unavailable");
        }
        if let Some(shell) = metadata_shell.as_mut() {
            shell.observe_outputs(&outputs)?;
            let reference_was_active=shell.reference_busy();
            let mut revoke_shell_input = false;
            let shell_operational = match shell.poll() {
                Ok(LiveMetadataShellPoll::Healthy) => true,
                Ok(LiveMetadataShellPoll::Connected { .. } | LiveMetadataShellPoll::Reconnected { .. }) => {
                    revoke_shell_input = true;
                    true
                }
                Ok(LiveMetadataShellPoll::Unavailable) => {
                    revoke_shell_input = true;
                    false
                }
                Err(error) => {
                    crate::session_eprintln!(
                        "sophia_live_metadata_shell schema=1 status=transport_failed stage=poll reason={error}"
                    );
                    shell.recover_transport("poll_failure")?;
                    revoke_shell_input = true;
                    false
                }
            };
            settle_revoked_shell_content_claims!(shell, "transport_disconnect");
            if shell_operational {
            if let (Some(runtime),Some(broker))=(runtime.as_mut(),metadata_broker.as_ref()) {
                let service=(||->Result<(),Box<dyn std::error::Error>> {
                    if let Some((surface,shell_output,activation))=shell.poll_activation(broker)? {
                        let output_bounds=wm_output_bounds(&outputs);
                        if let Some(output)=outputs.iter().find(|o|o.id==shell_output).copied() {
                            let activation_surfaces=live_shell_activation_surfaces(&layout.layers,&layout.presentation_roles);
                            if let Some(surface)=surface.filter(|s|activation_surfaces.contains(s))
                                && let Some(wm)=wm_session.as_mut(){
                                    let admitted=wm.enqueue_focus(surface,&layout,output)?;
                                    crate::session_println!("sophia_live_metadata_shell schema=1 status=activation_admitted activation={activation} outcome={admitted:?} target=redacted");
                                }
                            let bounds=output_bounds.iter().find(|(o,_)|*o==shell_output).map(|(_,b)|*b).ok_or("shell output bounds missing")?;
                            let root=wm_root_bounds(&output_bounds).ok_or("shell root bounds missing")?;
                            shell.request_candidate(broker,output,bounds,root,&output_bounds,&activation_surfaces)?;
                        }
                    }
                    if let Some(overlay)=shell.poll_candidate(broker)?
                        && let Err(error)=runtime.set_descriptor_overlay(overlay,scene,native_scanout.as_mut()) {
                            shell.reject_pending()?;return Err(error);
                        }
                    Ok(())
                })();
                if let Err(error)=service {
                    crate::session_eprintln!("sophia_live_metadata_shell schema=1 status=transport_failed stage=candidate reason={error}");
                    shell.recover_transport("candidate_failure")?;revoke_shell_input=true;
                }
            }
            if let (Some(runtime), Some(broker)) = (runtime.as_mut(), metadata_broker.as_ref()) {
                let publication=wm_session.as_ref().and_then(LiveWmSession::indicator_publication);
                let active_output=wm_session.as_ref().and_then(LiveWmSession::active_output);
                if let Err(error)=shell.service_indicators(publication.as_ref(),active_output) {
                    crate::session_eprintln!("sophia_shell_indicators status=unavailable error={error}");
                    shell.recover_transport("indicator_failure")?;revoke_shell_input=true;
                }
                let presented_content = runtime
                    .input_projections()
                    .iter()
                    .flat_map(|projection| projection.content.iter().cloned())
                    .collect::<Vec<_>>();
                if let Err(error) = shell.service_content_actions(&presented_content) {
                    crate::session_eprintln!(
                        "sophia_live_shell_content schema=1 status=action_service_failed reason={error}"
                    );
                    shell.recover_transport("content_action_failure")?;
                    revoke_shell_input = true;
                }
                match shell.service_indicator_activation(|action, output| {
                    wm_session.as_mut().map_or(
                        Ok(LiveIndicatorAdmissionResult::unavailable()),
                        |wm| wm.enqueue_indicator_action(action, output),
                    )
                }) {
                    Ok(_) => {},
                    Err(metadata_shell::indicators::IndicatorServiceError::Completion(error)) => return Err(error),
                    Err(metadata_shell::indicators::IndicatorServiceError::Poll(error)) => {
                        crate::session_eprintln!("sophia_shell_indicators status=activation_failed error={error}");
                        shell.recover_transport("indicator_activation_failure")?;revoke_shell_input=true;
                    }
                }
                match shell.service_tabs(publication,broker,runtime,scene,native_scanout.as_mut()) {
                    Ok(focus)=>for(surface,output) in focus {
                        if let (Some(wm),Some(output))=(wm_session.as_mut(),outputs.iter().find(|o|o.id==output).copied()) {
                            wm.enqueue_tab_focus(surface,output.id)?;
                        }
                    },
                    Err(error)=>{crate::session_eprintln!("sophia_tabs status=unavailable error={error}");shell.recover_transport("tab_failure")?;revoke_shell_input=true;}
                }
            }
            if let Some(runtime)=runtime.as_mut() {
                if let Err(error)=shell.service_launcher(config,xauthority,session_launches,secondary_children,&mut launch_admission_started_at,runtime,scene,native_scanout.as_mut()){
                    crate::session_eprintln!("sophia_launcher status=unavailable error={error}");
                    shell.cancel_launcher()?;
                    shell.recover_transport("launcher_failure")?;
                    runtime.set_descriptor_overlay(None,scene,native_scanout.as_mut())?;
                    revoke_shell_input=true;
                }
                shell.update_launcher_capture(&mut launcher_capture);
                if launcher_capture.active(){key_repeat.cancel_all();}
                let shortcuts=wm_session.as_ref().and_then(LiveWmSession::reference_shortcuts);
                let reference_output=wm_session.as_ref().and_then(LiveWmSession::reference_output).unwrap_or(output.id);
                if let Err(error)=shell.service_reference(shortcuts,reference_output,runtime,scene,native_scanout.as_mut()) {
                    crate::session_eprintln!("sophia_reference status=unavailable error={error}");
                    shell.recover_transport("reference_failure")?;
                    runtime.set_descriptor_overlay(None,scene,native_scanout.as_mut())?;
                    revoke_shell_input=true;
                }
                let reference_input=shell.reference_input();
                if reference_input.is_some() {
                    key_repeat.cancel_all();
                }
                reference_capture.present(reference_input);
            }
            if let Some(runtime) = runtime.as_mut() {
                let output_bounds = wm_output_bounds(&outputs);
                let root = wm_root_bounds(&output_bounds)
                    .ok_or("shell content output topology has no root bounds")?;
                if let Err(error) = shell.service_content(
                    runtime,
                    scene,
                    native_scanout.as_mut(),
                    &outputs,
                    &output_bounds,
                    root,
                ) {
                    crate::session_eprintln!(
                        "sophia_live_shell_content schema=2 status=transport_failed stage={} reason={error}",
                        shell.content_service_stage(),
                    );
                    shell.recover_transport("content_failure")?;
                    revoke_shell_input = true;
                }
            }
            if let Some(runtime) = runtime.as_ref() {
                match shell.observe_presentation(runtime) {
                    Ok(true) if shell.interaction_presented() => {
                        shell_proof_visible_presentations =
                            shell_proof_visible_presentations.saturating_add(1);
                        if !shell_proof_restart_triggered
                            && config.shell_proof_restart_after_visible
                                == Some(shell_proof_visible_presentations)
                        {
                            shell_proof_restart_triggered = true;
                            shell_proof_waiting_for_inert_click = true;
                            crate::session_println!(
                                "sophia_live_metadata_shell schema=1 status=proof_restart_triggered visible_presentation={} retained_pixels=true",
                                shell_proof_visible_presentations,
                            );
                            shell.recover_transport("proof_visible_restart")?;
                            revoke_shell_input = true;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        crate::session_eprintln!(
                            "sophia_live_metadata_shell schema=1 status=transport_failed stage=presentation reason={error}"
                        );
                        shell.recover_transport("presentation_failure")?;
                        revoke_shell_input = true;
                    }
                }
                if let Err(error) = shell.observe_content_presentation(runtime) {
                    crate::session_eprintln!(
                        "sophia_live_shell_content schema=1 status=presentation_failed reason={error}"
                    );
                    shell.recover_transport("content_presentation_failure")?;
                    revoke_shell_input = true;
                }
            }
            }
            if revoke_shell_input {
                launcher_capture.present(None,0,&[],true);
                reference_capture.present(None);
                shell.revoke_interaction();
                descriptor_captures.cancel_all();
                content_captures.revoke_targets();
                if let Some(runtime) = runtime.as_mut() {
                    runtime.revoke_descriptor_overlay_interaction();
                    if reference_was_active {runtime.set_descriptor_overlay(None,scene,native_scanout.as_mut())?;}
                }
            }
            shell_work_area_bands = Some(shell.work_area_bands());
        }
        if let (Some(components), Some(runtime)) = (shell_components.as_mut(), runtime.as_mut()) {
            component_service::service_components(components, component_catalog, runtime, scene, native_scanout.as_mut(),
                &outputs, wm_session, shell_presentation_available, session_launches, secondary_children, config, xauthority, &mut launch_admission_started_at)?;
            shell_work_area_bands = Some(components.work_area_bands());
        }
        if let Some(wm) = wm_session.as_mut() {
            // The shell's committed claim reaches the reduction here. Only a
            // change reprojects: an unchanged claim every tick would relayout
            // the desktop forever.
            if let Some(bands) = shell_work_area_bands.take()
                && wm.set_shell_reservation_bands(bands)
            {
                let primary = outputs
                    .first()
                    .copied()
                    .ok_or("shell reservation change has no primary output")?;
                crate::session_println!(
                    "sophia_live_metadata_shell schema=1 status=reservation_reduced bands={}",
                    wm.shell_reservation_band_count(),
                );
                wm.update_output_work_areas(&layout, &outputs, primary)?;
            }
        }
        service_core_config_reload!();
        service_session_controls!();
        // Deadlines and acknowledgments belong to the session, not to DRM.
        // Service them before any seat wait or renderer replacement can continue.
        InputDeliveryPhase {
                    sender: Some(input_sender),
            receiver: input_delivery_receiver,
            state: &mut input_delivery,
            client_key_release_barrier: &mut client_key_release_barrier,
            proof_started_at: &mut input_proof_started_at,
            post_input_deadline: &mut post_input_deadline,
        }.drain()?;
        if (post_input_deadline.is_none() || input_presented_latency.is_some())
            && deadline.is_some_and(|deadline| Instant::now() >= deadline)
        {
            if config.input_proof_requested() && injection_checksum.is_none() {
                return Err(
                    "persistent live session startup budget elapsed before a focused terminal frame was ready for input proof"
                        .into(),
                );
            }
            // The global runtime budget bounds startup. Once input has been
            // injected, its delivery and pixel/semantic stages own narrower
            // explicit deadlines. Ending here can strand already-routed keys
            // without giving the frontend a chance to acknowledge them.
            if global_runtime_deadline_ends_session(config.input_proof_requested()) {
                service_runtime_deadline_key_drain!();
            }
        }
        let native_shutdown_started = session_quiescence.is_some()
            || runtime_deadline_key_drain.is_draining();
        if !native_shutdown_started {
            *failure_phase = crate::diagnostics::SessionFailurePhase::Topology;
            include!("topology_phase.rs");
        }
        *failure_phase = crate::diagnostics::SessionFailurePhase::Lifecycle;
        include!("lifecycle.rs");
        // A pending VT switch must not park the owner which still holds the
        // final seat-device lease. This visit never performs KMS operations.
        let _ = native_retirement.poll()?;
        *failure_phase = crate::diagnostics::SessionFailurePhase::WindowManagement;
        include!("wm_phase.rs");
        *failure_phase = crate::diagnostics::SessionFailurePhase::Authority;
        include!("authority.rs");
        *failure_phase = crate::diagnostics::SessionFailurePhase::InputProof;
        include!("input_proof.rs");
        *failure_phase = crate::diagnostics::SessionFailurePhase::Control;
        service_session_controls!();
        *failure_phase = crate::diagnostics::SessionFailurePhase::Quiescence;
        // Reduce primary retirement once after every independently scheduled
        // service phase. Branch-local latency sampling stays at the event
        // source, while CPU settlement observes one coherent owner-loop state.
        if let Some(native_scanout) = native_scanout.as_ref() {
            cpu_visual_progress.observe_native_scanout(native_scanout, Instant::now());
        }
        if let Some(quiescence) = session_quiescence.as_ref() {
            let now = Instant::now();
            let native_work_pending = match (runtime.as_ref(), native_scanout.as_ref()) {
                (Some(runtime), Some(native_scanout)) => native_frame_service_requires_owner_progress(
                    &runtime.native_output_service_request(native_scanout)?,
                ),
                _ => false,
            };
            let snapshot = SessionQuiescenceSnapshot {
                pending_authority_batches: pending_authority_batches
                    .len()
                    .saturating_add(usize::from(initial_authority_batch.is_some())),
                pending_coordinator_work: usize::from(pending_wm_update.is_some())
                    .saturating_add(usize::from(layout.pending.is_some()))
                    .saturating_add(wm_session.as_ref().map_or(
                        0,
                        LiveWmSession::in_flight_request_count,
                    ))
                    .saturating_add(usize::from(runtime.as_ref().is_some_and(
                        LiveProductionVisualRuntime::has_released_surface_content,
                    ))),
                // Control servicing can dispatch a final command or produce
                // more layout work; take the snapshot after both have run.
                pending_controls: session_controls.pending_len(),
                cpu_update_pending: !cpu_visual_progress.is_settled(),
                native_work_pending,
            };
            match quiescence.decision(now, snapshot) {
                SessionQuiescenceDecision::Pending => {}
                SessionQuiescenceDecision::Complete => {
                    crate::session_println!(
                        "sophia_live_session_quiescence schema=3 status=complete reason={} elapsed_msec={} authority_pending=0 coordinator_pending=0 pending_control_count=0 cpu_pending=0 native_pending=false pending_transaction=none pending_surface=none pending_handle=none pending_generation=none pending_target_checksum=none",
                        quiescence.reason,
                        quiescence.elapsed(now).as_millis(),
                    );
                    break 'session;
                }
                SessionQuiescenceDecision::TimedOut => {
                    // Quiescence normally remains pending for many owner turns.
                    // Materialize diagnostic strings only on its terminal
                    // failure path, not in the steady drain loop.
                    let pending_identity = cpu_visual_progress.pending_identity();
                    let oldest_authority_transaction = initial_authority_batch
                        .as_ref()
                        .or_else(|| pending_authority_batches.front())
                        .map_or_else(
                            || "none".to_owned(),
                            |batch| batch.transaction.raw().to_string(),
                        );
                    let pending_transaction = pending_identity.map_or_else(
                        || "none".to_owned(),
                        |identity| identity.transaction.raw().to_string(),
                    );
                    let pending_surface = pending_identity.map_or_else(
                        || "none".to_owned(),
                        |identity| identity.surface.index().to_string(),
                    );
                    let pending_handle = pending_identity.map_or_else(
                        || "none".to_owned(),
                        |identity| identity.handle.to_string(),
                    );
                    let pending_generation = pending_identity.map_or_else(
                        || "none".to_owned(),
                        |identity| identity.generation.to_string(),
                    );
                    let pending_target_checksum = cpu_visual_progress
                        .pending_target_checksum()
                        .map_or_else(
                            || "none".to_owned(),
                            |checksum| checksum.to_string(),
                        );
                    let cancellation = match frontend_service_sender
                        .send(XServerFrontendServiceCommand::StopAndDisconnect)
                    {
                        Ok(()) => "requested",
                        Err(_) => "frontend_already_stopped",
                    };
                    crate::session_println!(
                        "sophia_live_session_quiescence schema=3 status=timed_out reason={} elapsed_msec={} authority_pending={} cpu_pending={} native_pending={} cancellation={} pending_transaction={} pending_surface={} pending_handle={} pending_generation={} pending_target_checksum={} coordinator_pending={} authority_initial={} authority_queued={} oldest_authority_transaction={} pending_control_count={}",
                        quiescence.reason,
                        quiescence.elapsed(now).as_millis(),
                        snapshot.pending_authority_batches,
                        cpu_visual_progress.pending_updates(),
                        snapshot.native_work_pending,
                        cancellation,
                        pending_transaction,
                        pending_surface,
                        pending_handle,
                        pending_generation,
                        pending_target_checksum,
                        snapshot.pending_coordinator_work,
                        usize::from(initial_authority_batch.is_some()),
                        pending_authority_batches.len(),
                        oldest_authority_transaction,
                        snapshot.pending_controls,
                    );
                    return Err(format!(
                        "session quiescence timed out: reason={} frontend_drained={} authority_pending={} cpu_pending={} native_pending={} pending_transaction={} pending_surface={} pending_handle={} pending_generation={} pending_target_checksum={} coordinator_pending={} oldest_authority_transaction={} pending_control_count={}",
                        quiescence.reason,
                        quiescence.frontend_authority_drained,
                        snapshot.pending_authority_batches,
                        cpu_visual_progress.pending_updates(),
                        snapshot.native_work_pending,
                        pending_transaction,
                        pending_surface,
                        pending_handle,
                        pending_generation,
                        pending_target_checksum,
                        snapshot.pending_coordinator_work,
                        oldest_authority_transaction,
                        snapshot.pending_controls,
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
})();
if let Err(error) = session_loop_result {
    let failure_code = crate::diagnostics::failure_code(error.as_ref());
    let original = error.to_string();
    terminal_runtime_error = Some(original.clone());
    if let Err(error) = stop_frontend_intake(
        frontend_service_sender,
        &mut terminal_client_intake_stopped,
    ) {
        terminal_client_cleanup_failures.push(format!("frontend intake stop failed: {error}"));
    }
    crate::session_println!(
        "sophia_live_session_runtime_fatal schema=1 status=detected source=owner_loop action=bounded_cleanup failure_code={failure_code} error={original:?}"
    );
}

include!("completion.rs")
}
