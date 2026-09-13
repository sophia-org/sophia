{
    if let Some(devices) = client_render_devices.as_mut() {
        let now = Instant::now();
        if let Some(monitor) = output_topology_monitor.as_mut() {
            match monitor.poll_render_inventory_notice() {
                Ok(true) => {
                    if let Some(snapshot) = monitor.render_inventory_snapshot() {
                        devices.observe_inventory(snapshot, now)?;
                        let (generation, inventory) = devices.observed_inventory();
                        tracing::info!(generation, devices=inventory.len(), "render-device inventory changed");
                    }
                }
                Ok(false) => {}
                Err(error) => tracing::warn!(%error, "render inventory comparison deferred"),
            }
        }
        devices.poll(now, frontend_service_sender)?;
        if let Some(shell) = metadata_shell.as_mut() {
            // Only the coordinator's fully admitted active identity may replace
            // the shell grant. An unavailable identity revokes immediately;
            // replacement waits for the existing frontend acknowledgement.
            let admitted = devices.shell_gpu_device().ok();
            let _ = shell.observe_gpu_device(admitted)?;
        }
        if now >= render_inventory_service_at {
            render_inventory_service_at = now + Duration::from_millis(250);
            if let (Some(native), Some((generation, inventory))) =
                (native_scanout.as_mut(), devices.admitted_inventory())
            {
                if generation > native.image_import_inventory_generation() {
                    let files = inventory.iter().map(|device| device.file.try_clone())
                        .collect::<std::io::Result<Vec<_>>>();
                    match files.and_then(|files| native.request_image_import_inventory(generation, files)) {
                        Ok(()) => {}
                        Err(error) => tracing::warn!(generation, %error, "renderer device inventory handoff deferred"),
                    }
                }
                if let Err(error) = native.poll_image_import_inventory() {
                    tracing::warn!(%error, "renderer device inventory acknowledgement deferred");
                }
            }
        }
    }
    let polled_monitor_notice = output_topology_monitor
        .as_mut()
        .map(sophia_backend_live::LiveDrmTopologyMonitor::poll_notice)
        .transpose()?
        .flatten();
    let monitor_notice = if active_output_topology_preparation.is_some() {
        if let Some(notice) = polled_monitor_notice
            && deferred_output_topology_notice
                .is_none_or(|deferred| notice.sequence > deferred.sequence)
            {
                deferred_output_topology_notice = Some(notice);
            }
        None
    } else {
        polled_monitor_notice.or_else(|| deferred_output_topology_notice.take())
    };
    let retry_due = output_topology_retry_at.is_some_and(|deadline| Instant::now() >= deadline);
    if let Some(notice) = monitor_notice {
        let advance_security_epoch = output_topology_owner.begin_rescan(notice.sequence)?;
        output_topology_retry_at = None;
        if advance_security_epoch {
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
            deferred_physical_key_timings.clear();
            key_repeat.cancel_seat(seat);
            crate::session_println!(
                "sophia_live_input_epoch schema=1 reason=output_topology transition={} epoch={} revoked_leases={revoked_input_leases}",
                output_topology_owner.transition,
                application_route_leases.control_epoch(),
            );
        }
    }

    // Only a hotplug quarantine is this path's to consume. Arming the retry on
    // a policy quarantine is what let a rescan tear down the scanout a
    // candidate was mid-apply on, and release the quarantine it was holding.
    let hotplug_quarantined = output_topology_owner.phase
        == LiveOutputTopologyPhase::Quarantined(LiveOutputTopologyQuarantine::Hotplug);
    let rebuild_requested = (monitor_notice.is_some() || retry_due)
        && hotplug_quarantined
        && seat_state == sophia_backend_live::LiveSeatState::Active
        && runtime.is_some();
    if (hotplug_quarantined || output_topology_owner.take_deferred_hotplug_notice())
        && output_topology_retry_at.is_none()
    {
        output_topology_retry_at = Some(Instant::now() + Duration::from_millis(250));
    }
    if rebuild_requested {
        output_topology_retry_at = None;
        let mut renderer_handoff = suspended_renderer_images.take();
        if let (Some(runtime), Some(native)) = (runtime.as_mut(), native_scanout.as_mut()) {
            match runtime.suspend_native_scanout(native, &outputs, Duration::from_secs(2)) {
                Ok(report) => {
                        native_evidence.observe_settlement(report.outcome.drained(), report.abandoned_scanouts);
                    renderer_handoff = Some(capture_renderer_image_handoff(
                        runtime,
                        native,
                    )?);
                    tracing::info!(
                        "sophia_live_output_topology schema=1 status=quiesced transition={} outcome={} abandoned_scanouts={}",
                        output_topology_owner.transition,
                        report.outcome.reduced_name(),
                        report.abandoned_scanouts,
                    );
                }
                Err(error) => {
                    native_evidence.observe_settlement(false, 0);
                    let report = runtime.suspend_revoked_native_scanout(&outputs)?;
                    native_evidence.observe_settlement(report.outcome.drained(), report.abandoned_scanouts);
                    let discarded = runtime.discard_retained_renderer_images();
                    renderer_handoff = None;
                    tracing::warn!(
                        "sophia_live_output_topology schema=1 status=forced_detach transition={} error={error} abandoned_scanouts={} discarded_images={discarded}",
                        output_topology_owner.transition,
                        report.abandoned_scanouts,
                    );
                }
            }
        }
        close_native_owner!("topology_rebuild");

        if !native_recovery_allowed!() { continue; }
        let replacement = match seat_controller.as_ref() {
            Some(controller) => {
                LiveProductionNativeScanout::new_with_seat_mirroring_mapping_and_cursor(
                    &controller.device_opener(),
                    mirror_grouping,
                    initial_head_mapping,
                    config.cursor_resolution.asset.clone(),
                )
                .map_err(|error| error.to_string())
            }
            None => Err("DRM topology rescan lost its seat controller".to_owned()),
        };
        match replacement {
            Err(error) => {
                let _ = output_topology_owner.observe_rebuild(Vec::new(), Vec::new())?;
                suspended_renderer_images = renderer_handoff;
                output_topology_retry_at = Some(Instant::now() + Duration::from_millis(250));
                tracing::warn!(
                    "sophia_live_output_topology schema=1 status=unavailable transition={} retry_msec=250 error={error}",
                    output_topology_owner.transition,
                );
            }
            Ok(mut replacement) => {
                let replacement_outputs = replacement.outputs();
                let rebuild = output_topology_owner
                    .observe_rebuild(replacement_outputs.clone(), replacement.head_fingerprint())?;
                let topology_changed = rebuild == LiveOutputTopologyRebuild::TopologyChanged;
                physical_output_topology_replaced |= topology_changed;
                let replacement_capabilities = replacement.output_capabilities()?;
                let replacement_authority = replacement.output_authority_snapshot(
                    output_topology_owner.topology_epoch,
                )?;
                let replacement_primary = replacement_outputs[0];
                if scene.reconfigure_output_size(replacement_primary.size)? {
                    let committed = runtime
                        .as_ref()
                        .map(|runtime| runtime.committed_surfaces().to_vec())
                        .unwrap_or_default();
                    scene.compose(&committed, None, pointer.position())?;
                }
                let runtime = runtime
                    .as_mut()
                    .ok_or("DRM topology rescan lost the visual runtime")?;
                let restored = resume_native_scanout_from_scene(
                    runtime,
                    &mut replacement,
                    &replacement_outputs,
                    &mut scene,
                    renderer_handoff,
                )?;

                if topology_changed {
                    let snapshot = output_topology_from_engine_outputs_at_generation(
                        &replacement_outputs,
                        output_topology_owner.publication_generation,
                    )?;
                    let (ack_sender, ack_receiver) = sync_channel(1);
                    frontend_service_sender.send(
                        XServerFrontendServiceCommand::UpdateOutputTopology {
                            snapshot,
                            acknowledgement: ack_sender,
                        },
                    )?;
                    match ack_receiver.recv_timeout(Duration::from_secs(1))? {
                        sophia_x_authority::XAuthorityOutputUpdateOutcome::Applied { .. } => {}
                        outcome => {
                            return Err(format!(
                                "X frontend rejected owner topology publication: {outcome:?}"
                            )
                            .into());
                        }
                    }
                }

                outputs = replacement_outputs;
                output = replacement_primary;
                pointer.set_output_bounds(
                    wm_output_bounds(&outputs)
                        .into_iter()
                        .map(|(_, bounds)| bounds)
                        .collect(),
                );
                cursor_updates.dirty = pointer.position().is_some();
                cursor_updates.dirty_since = cursor_updates.dirty.then(Instant::now);

                let mut policy_required = false;
                if topology_changed
                    && let Some(wm) = wm_session.as_mut()
                {
                    let admission = wm.update_output_work_areas(&layout, &outputs, output)?;
                    if admission == LiveWmRequestAdmission::RejectedCapacity {
                        return Err("output topology relayout exceeded WM owner capacity".into());
                    }
                    policy_required = admission == LiveWmRequestAdmission::Admitted
                        || wm.has_current_relayout_request(&layout);
                    output_topology_policy_commit_baseline =
                        wm.topology_policy_commit_serial();
                }
                let presentation_baseline = replacement.retirements;
                output_topology_owner
                    .mark_published(presentation_baseline, policy_required)?;
                pending_hardware_output_publication =
                    Some((replacement_authority, replacement_capabilities));
                // A replacement snapshot owes its own presentation before it
                // may be published, so it does not inherit the previous one's.
                hardware_output_publication_presented = false;
                if !startup_ready_reported {
                    startup_required_submissions = startup_readiness.surface.and_then(|surface| {
                        let geometry = runtime
                            .committed_surfaces()
                            .iter()
                            .find(|committed| committed.surface == surface)?
                            .geometry;
                        let bounds = wm_output_bounds(&outputs);
                        Some(
                            replacement
                                .heads
                                .iter()
                                .map(|head| {
                                    let intersects = bounds
                                        .iter()
                                        .find(|(output, _)| *output == head.output.id)
                                        .is_some_and(|(_, bounds)| {
                                            rects_intersect(geometry, *bounds)
                                        });
                                    (
                                        head.head,
                                        StartupHeadRequirement {
                                            submission: startup_submission_requirement(
                                                head.submissions,
                                                head.presented_submissions,
                                                intersects,
                                            ),
                                            content_frame: newest_head_composition_frame(
                                                [
                                                    head.pending_content,
                                                    head.rendering_content,
                                                    head.submitted_content,
                                                    head.presented_content,
                                                ]
                                                .map(|content| {
                                                    content
                                                        .map(|content| content.frame().raw())
                                                }),
                                            ),
                                        },
                                    )
                                })
                                .collect(),
                        )
                    });
                }
                native_evidence.open("topology_rebuild");
                *native_scanout = Some(replacement);
                native_presentation_admitted = false;
                tracing::info!(
                    "sophia_live_output_topology schema=1 status=published transition={} topology_epoch={} generation={} outputs={} changed={} restored_images={} policy_required={} input=quarantined",
                    output_topology_owner.transition,
                    output_topology_owner.topology_epoch,
                    output_topology_owner.publication_generation,
                    outputs.len(),
                    topology_changed,
                    restored,
                    policy_required,
                );
            }
        }
    }

    if output_topology_owner.phase == LiveOutputTopologyPhase::Published
        && wm_session.as_ref().is_some_and(|wm| {
            wm.topology_policy_commit_serial() > output_topology_policy_commit_baseline
        })
    {
        let presentation_baseline = native_scanout
            .as_ref()
            .map_or(0, |native| native.retirements);
        output_topology_owner.mark_policy_committed(presentation_baseline)?;
        if let (Some(runtime), Some(native)) = (runtime.as_mut(), native_scanout.as_mut()) {
            let focused = runtime.focused_surface();
            scene.force_full_repaint();
            let forced = runtime.run_cpu_repaint(
                &mut scene,
                focused,
                focused,
                LiveProductionCursorPresentation::HardwarePlane,
                &outputs,
                native,
            )?;
            primary_frame_pacer.observe_repaint(Instant::now());
            tracing::info!(
                "sophia_live_output_topology schema=2 status=repaint_forced transition={} presentation_baseline={presentation_baseline} checksum={} reason=policy_committed",
                output_topology_owner.transition,
                forced.composition.checksum,
            );
        }
        topology_presentation_deadline =
            Some(Instant::now() + OUTPUT_TOPOLOGY_PRESENTATION_TIMEOUT);
        tracing::info!(
            "sophia_live_output_topology schema=1 status=policy_committed transition={} presentation_baseline={presentation_baseline}",
            output_topology_owner.transition,
        );
    }
    if let Some(retirements) = native_scanout.as_ref().map(|native| native.retirements)
        && output_topology_owner.observe_presentation(retirements)
    {
        // Arm rather than publish. A snapshot must not reach policy before the
        // topology it describes has presented, but it must also not reach a
        // live candidate, and those two conditions do not become true in the
        // same pass. `observe_presentation` reports the edge exactly once, so
        // publishing from here is the only chance the snapshot ever gets.
        hardware_output_publication_presented = true;
        if startup_topology_recovery_pending {
            let _ = reduce_session_startup(
                &mut startup_readiness,
                SessionStartupEvent::NativeRecovered,
            );
            startup_topology_recovery_pending = false;
        }
        topology_presentation_deadline = None;
        tracing::info!(
            "sophia_live_output_topology schema=1 status=settled transition={} retirements={retirements} input=enabled",
            output_topology_owner.transition,
        );
    }
    // A relayout that moves nothing produces no damage and so no flip, which is
    // indistinguishable from a slow client. In that case the displayed layout is
    // already the committed one, so continuing to wait protects nothing and
    // holds input at shortcuts-only indefinitely. Say what was missing and
    // restore input.
    if let Some(deadline) = topology_presentation_deadline
        && Instant::now() >= deadline
    {
        topology_presentation_deadline = None;
        let retirements = native_scanout
            .as_ref()
            .map_or(0, |native| native.retirements);
        if output_topology_owner.release_presentation_wait() {
            tracing::warn!(
                "sophia_live_output_topology schema=2 status=presentation_timed_out transition={} retirements={retirements} presentation_baseline={} timeout_msec={} input=enabled",
                output_topology_owner.transition,
                output_topology_owner.presentation_baseline,
                OUTPUT_TOPOLOGY_PRESENTATION_TIMEOUT.as_millis(),
            );
        }
    }
    // Retried every pass, because the candidate that blocks publication clears
    // on its own schedule. One slot is enough: a newer hardware snapshot
    // supersedes an older unpublished one rather than queueing behind it.
    if hardware_output_publication_presented
        && let Some(wm) = wm_session.as_mut()
        && !wm.output_candidate_active()
        && let Some((snapshot, capabilities)) = pending_hardware_output_publication.take()
    {
        hardware_output_publication_presented = false;
        if wm.output_authority_topology_epoch().is_some_and(|current| {
            hardware_output_snapshot_is_stale(snapshot.topology_epoch, current)
        }) {
            tracing::warn!(
                "sophia_live_output_authority schema=2 status=hardware_snapshot_dropped snapshot_epoch={} current_epoch={} reason=stale_after_candidate",
                snapshot.topology_epoch,
                wm.output_authority_topology_epoch().unwrap_or(0),
            );
        } else {
            let _ = wm.publish_output_authority_snapshot(snapshot, capabilities)?;
        }
    }
}
