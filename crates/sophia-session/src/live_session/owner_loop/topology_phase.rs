{
    if let Some(wm) = wm_session.as_ref()
        && let Some(focused) = wm.reference_output()
        && let Some(public) = wm.public.as_ref()
        && pending_hardware_output_publication.is_none()
    {
        output_realization.observe_focus(focused, public.output_policy_capabilities.as_deref().unwrap_or(&public.output_capabilities));
    }
    // The initial owner has no hotplug publication to settle. Its ordinary
    // startup presentation barrier supplies the same evidence, before a later
    // rescan can use it as the committed fallback and focus preference.
    if native_presentation_admitted
        && pending_hardware_output_publication.is_none()
        && output_topology_owner.phase == LiveOutputTopologyPhase::Stable
        && active_output_topology_preparation.is_none()
        && wm_session.as_ref().is_none_or(|wm| !wm.output_candidate_active())
        && let Some(native) = native_scanout.as_ref()
    {
        let initial_binding = output_realization::OutputRealizationBinding {
                transition: 0,
                notice_sequence: 0,
                native_owner: native.retirement_owner_identity(),
        };
        if let Some(initial) = output_realization.pending(initial_binding, config.output_profile.current()) {
            let capabilities = native.output_capabilities()?;
            let snapshot = wm_session.as_ref().and_then(|wm| wm.published_output_snapshot())
                .map(Ok).unwrap_or_else(|| native.output_authority_snapshot(output_topology_owner.topology_epoch))?;
            let settings_match = initial.outputs.iter().filter(|state| state.enabled).all(|state| {
                capabilities.iter().find(|cap| cap.connector_key() == state.connector)
                    .and_then(|cap| cap.head())
                    .and_then(|head| native.heads.iter().find(|candidate| candidate.head == head))
                    .is_some_and(|head| head.transform == sophia_protocol::OutputTransform::Normal
                        && state.transform == sophia_config::DesktopOutputTransform::Normal
                        && head.vrr == match state.vrr {
                            sophia_config::DesktopOutputVrrMode::Disabled => sophia_protocol::OutputVrrPolicy::Disabled,
                            sophia_config::DesktopOutputVrrMode::Automatic => sophia_protocol::OutputVrrPolicy::Automatic,
                            sophia_config::DesktopOutputVrrMode::Always => sophia_protocol::OutputVrrPolicy::Always,
                        })
            });
            if settings_match && output_realization::matches_presented(initial, &capabilities, &outputs, &snapshot, initial_head_mapping)? {
                output_realization.commit(initial_binding, config.output_profile.current());
            } else {
                // One observation per owner: a refused startup candidate must
                // not turn every idle pass into another capability read.
                output_realization.abandon();
                tracing::warn!(target: "sophia_scanout_evidence", "sophia_live_output_resolution schema=1 phase=startup status=uncommitted reason=presented_settings_differ");
            }
        }
    }
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
        // A new notice starts a new bounded rescan series.
        output_topology_retry_attempts = 0;
        output_recovery = output_replacement::OutputRecovery::default();
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
        && output_recovery != output_replacement::OutputRecovery::Exhausted
        && hotplug_quarantined
        && seat_state == sophia_backend_live::LiveSeatState::Active
        && runtime.is_some();
    // A deferred notice starts a new series. Once a series is spent, only a
    // new notice or seat enable rescans: a quarantined owner is never polled
    // on a timer.
    let deferred_notice =
        !hotplug_quarantined && output_topology_owner.take_deferred_hotplug_notice();
    if deferred_notice {
        output_topology_retry_attempts = 0;
        output_recovery = output_replacement::OutputRecovery::default();
    }
    if (hotplug_quarantined || deferred_notice)
        && output_recovery != output_replacement::OutputRecovery::Exhausted
        && output_topology_retry_at.is_none()
        && let Some(delay) = output_replacement::runtime_output_retry_delay(output_topology_retry_attempts)
    {
        output_topology_retry_at = Some(Instant::now() + delay);
    }
    if rebuild_requested {
        // A dispatched authority effect owns Policy quarantine, which cannot
        // enter this branch; its monitor notice waits for rollback/settlement.
        debug_assert!(active_output_topology_preparation.is_none());
        output_topology_retry_at = None;
        if let Some(wm) = wm_session.as_mut() {
            wm.abandon_unstarted_output_topology_for_rebuild()?;
        }
        output_realization.abandon();
        pending_hardware_output_publication = None;
        hardware_output_publication_presented = false;
        pause_metadata_shell_presentation!("topology_rebuild");
        let mut retirement_mode = RetirementMode::Abandoned;
        if let (Some(runtime), Some(native)) = (runtime.as_mut(), native_scanout.as_mut()) {
            match runtime.suspend_native_scanout(native, &outputs, Duration::from_secs(2)) {
                Ok(report) => {
                    retirement_mode = RetirementMode::from_suspend(report.outcome);
                        native_evidence.observe_settlement(report.outcome.drained(), report.abandoned_scanouts);
                    *suspended_renderer_images = Some(capture_renderer_image_handoff(
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
                    *suspended_renderer_images = None;
                    tracing::warn!(
                        "sophia_live_output_topology schema=1 status=forced_detach transition={} error={error} abandoned_scanouts={} discarded_images={discarded}",
                        output_topology_owner.transition,
                        report.abandoned_scanouts,
                    );
                }
            }
        }
        close_native_owner!("topology_rebuild", retirement_mode);

        if !native_recovery_allowed!() { continue; }
        native_owner_retirement::finish_before_replacement(runtime.as_ref(), native_retirement)?;
        // Policy resolves against admitted probes only; the suspended images
        // and the hotplug quarantine stay as they are until a replacement is
        // constructed, and waiting or a refusal is unavailability, never a
        // session failure.
        let replacement_head_mapping = output_replacement::profile_head_mapping(config.output_profile.current());
        let replacement = match seat_controller.as_ref() {
            Some(controller) => output_replacement::resolve_runtime_output_replacement(
                controller,
                config.output_profile.current(),
                output_realization.committed(),
                replacement_head_mapping,
                &config.cursor_resolution.asset,
                output_recovery,
            ),
            None => output_replacement::RuntimeOutputReplacement::Refused(
                "DRM topology rescan lost its seat controller".to_owned(),
            ),
        };
        match replacement {
            output_replacement::RuntimeOutputReplacement::Waiting
            | output_replacement::RuntimeOutputReplacement::Refused(_) => {
                let _ = output_topology_owner.observe_rebuild(Vec::new(), Vec::new())?;
                let attempt = output_topology_retry_attempts;
                let retry = if matches!(replacement, output_replacement::RuntimeOutputReplacement::Refused(_)) {
                    output_recovery.refused(config.output_profile.current()).then_some(Duration::ZERO)
                } else {
                    output_replacement::runtime_output_retry_delay(attempt)
                };
                output_topology_retry_attempts = attempt.saturating_add(1);
                output_topology_retry_at = retry.map(|delay| Instant::now() + delay);
                output_recovery.record_exhausted("runtime", config.output_profile.current());
                let retry_msec = retry.map_or("none".to_owned(), |delay| delay.as_millis().to_string());
                let status = if matches!(replacement, output_replacement::RuntimeOutputReplacement::Waiting) { "waiting" } else { "refused" };
                tracing::info!(target: "sophia_scanout_evidence",
                    "sophia_live_output_resolution schema=1 phase=runtime status={status} reason=unavailable generation={} transition={} notice={} attempt={}",
                    config.output_profile.current().generation.raw(), output_topology_owner.transition,
                    output_topology_owner.notice_sequence, attempt + 1);
                match replacement {
                    output_replacement::RuntimeOutputReplacement::Refused(error) => tracing::warn!(
                        "sophia_live_output_topology schema=1 status=unavailable transition={} attempt={} retry_msec={retry_msec} error={error}",
                        output_topology_owner.transition,
                        attempt + 1,
                    ),
                    _ => tracing::info!(
                        "sophia_live_output_topology schema=1 status=waiting transition={} attempt={} retry_msec={retry_msec}",
                        output_topology_owner.transition,
                        attempt + 1,
                    ),
                }
            }
            output_replacement::RuntimeOutputReplacement::Active(replacement, realization, policy_layout) => {
                output_topology_retry_attempts = 0;
                *native_scanout = Some(*replacement);
                native_retirement.admit(native_scanout.as_ref().expect("just adopted"))?;
                // Open the adopted owner's evidence before resume: a refused
                // resume closes it through close_native_owner!. This names the
                // adopted owner, not a presentation; readiness follows resume.
                let epoch = native_evidence.open("topology_rebuild");
                native_evidence.record_owner_heads(
                    epoch,
                    native_scanout.as_ref().expect("just adopted").output_capabilities(),
                );
                let replacement = native_scanout.as_mut().expect("just adopted");
                let replacement_outputs = replacement.outputs();
                let replacement_capabilities = policy_layout.capabilities.clone();
                let realization_changed = output_realization.committed().is_none_or(|before| {
                    before.outputs != realization.outputs || before.policy_keys != realization.policy_keys
                });
                let binding = output_realization::OutputRealizationBinding {
                    transition: output_topology_owner.transition,
                    notice_sequence: output_topology_owner.notice_sequence,
                    native_owner: replacement.retirement_owner_identity(),
                };
                // Staged for the realization owner before anything downstream
                // publishes; the owner decides commit and abandonment.
                output_realization.stage(
                    binding,
                    *realization,
                )?;
                // Keep the old published topology until resume succeeds. A
                // partially applied replacement still belongs to quarantine.
                let mut replacement_owner = output_topology_owner.clone();
                let rebuild = replacement_owner
                    .observe_resolved_rebuild(replacement_outputs.clone(), replacement.head_fingerprint(), realization_changed)?;
                let topology_changed = rebuild == LiveOutputTopologyRebuild::TopologyChanged;
                let mut replacement_authority = replacement.output_authority_snapshot(
                    replacement_owner.topology_epoch,
                )?;
                policy_layout.apply_authority_geometry(&mut replacement_authority)?;
                let replacement_primary = replacement_outputs.iter()
                    .find(|output| output.id == policy_layout.primary).copied()
                    .ok_or("replacement lost its resolved primary output")?;
                if scene.reconfigure_output_descriptors(&replacement_outputs)? {
                    let committed = runtime
                        .as_ref()
                        .map(|runtime| runtime.committed_surfaces().to_vec())
                        .unwrap_or_default();
                    scene.compose(&committed, None, pointer.position())?;
                }
                let attempt = try_resume_native_scanout_from_scene_at(
                    runtime.as_mut().ok_or("DRM topology rescan lost the visual runtime")?,
                    replacement,
                    &replacement_outputs,
                    scene,
                    suspended_renderer_images,
                    &policy_layout.bounds,
                )?;
                let restored = match attempt {
                    ResumeAttempt::Resumed(restored) => restored,
                    ResumeAttempt::Abandoned { error, mode } => {
                        output_realization.abandon();
                        close_native_owner!("replacement_refused", mode);
                        native_owner_retirement::finish_before_replacement(runtime.as_ref(), native_retirement)?;
                        if scene.reconfigure_output_descriptors(&outputs)? {
                            let committed = runtime.as_ref().expect("retained runtime").committed_surfaces().to_vec();
                            scene.compose(&committed, None, pointer.position())?;
                        }
                        output_topology_retry_at = output_recovery.refused(config.output_profile.current()).then(Instant::now);
                        output_recovery.record_exhausted("runtime", config.output_profile.current());
                        tracing::warn!(target: "sophia_scanout_evidence",
                            "sophia_live_output_resolution schema=1 phase=runtime status=refused reason=hardware transition={} notice={} owner={}",
                            binding.transition, binding.notice_sequence, binding.native_owner);
                        tracing::warn!(%error, "replacement resume refused; retained handoff kept");
                        continue;
                    }
                };
                let runtime = runtime.as_mut().expect("retained runtime");
                output_topology_owner = replacement_owner;
                physical_output_topology_replaced |= topology_changed;
                output_startup_activation::record_ready_heads(&replacement_capabilities)?;

                if topology_changed {
                    let snapshot = policy_layout.frontend_snapshot(
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
                    policy_layout.bounds.iter()
                        .map(|(_, bounds)| *bounds)
                        .collect(),
                );
                cursor_updates.dirty = pointer.position().is_some();
                cursor_updates.dirty_since = cursor_updates.dirty.then(Instant::now);

                let mut policy_required = false;
                if let Some(wm) = wm_session.as_mut()
                {
                    let admission = wm.update_output_work_areas_for_realization(&layout, &outputs, &policy_layout)?;
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
                if !policy_required {
                    // A blocking initial modeset retires no flip. Request an
                    // observation frame even when the WM has no changed scene,
                    // so this replacement can cross the publication barrier.
                    let focused = runtime.focused_surface();
                    scene.force_full_repaint();
                    runtime.run_cpu_repaint(scene, focused, focused,
                        LiveProductionCursorPresentation::HardwarePlane, &outputs, replacement)?;
                    primary_frame_pacer.observe_repaint(Instant::now());
                }
                // The replacement's first frame is a blocking modeset, which
                // retires no page flip. With no policy commit to force another,
                // a still screen would hold input quarantined until some client
                // repaints, so wait the same bounded time a policy commit does.
                topology_presentation_deadline = (output_topology_owner.phase
                    == LiveOutputTopologyPhase::AwaitingPresentation)
                    .then(|| Instant::now() + OUTPUT_TOPOLOGY_PRESENTATION_TIMEOUT);
                pending_hardware_output_publication = Some(output_realization::PendingOutputPublication {
                    binding,
                    snapshot: replacement_authority,
                    capabilities: replacement_capabilities,
                    already_published: false,
                });
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
                        let bounds = &policy_layout.bounds;
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
                scene,
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
            if startup_topology_recovery_pending {
                // Invalidate the retired owner's readiness even if no new
                // flip arrived. This does not mark the replacement ready.
                let _ = reduce_session_startup(&mut startup_readiness, SessionStartupEvent::NativeRecovered);
                startup_topology_recovery_pending = false;
            }
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
        && wm_session.as_ref().is_none_or(|wm| !wm.output_candidate_active())
        && let Some(publication) = pending_hardware_output_publication.take()
    {
        hardware_output_publication_presented = false;
        let current_owner = native_scanout.as_ref().map(|native| native.retirement_owner_identity());
        let binding = publication.binding;
        let profile = config.output_profile.current();
        let current_binding = binding.transition == output_topology_owner.transition
            && binding.notice_sequence == output_topology_owner.notice_sequence
            && Some(binding.native_owner) == current_owner;
        let current_profile = output_realization.pending(binding, profile).is_some();
        let current_epoch = wm_session.as_ref().and_then(|wm| wm.output_authority_topology_epoch());
        let stale_epoch = if publication.already_published {
            current_epoch != Some(publication.snapshot.topology_epoch)
        } else {
            current_epoch.is_some_and(|current| hardware_output_snapshot_is_stale(publication.snapshot.topology_epoch, current))
        };
        if !current_binding || stale_epoch {
            tracing::info!(target: "sophia_scanout_evidence",
                "sophia_live_output_resolution schema=1 phase=runtime status=uncommitted reason=stale transition={} notice={} owner={}",
                binding.transition, binding.notice_sequence, binding.native_owner);
            tracing::warn!(
                "sophia_live_output_authority schema=2 status=hardware_snapshot_dropped snapshot_epoch={} current_epoch={} reason=stale_replacement",
                publication.snapshot.topology_epoch,
                current_epoch.unwrap_or(0),
            );
        } else {
            if !publication.already_published && let Some(wm) = wm_session.as_mut() {
                let _ = wm.publish_output_authority_snapshot(publication.snapshot, publication.capabilities)?;
            }
            if current_profile {
                if !output_realization.commit(binding, profile) {
                    return Err("presented output realization lost its publication binding".into());
                }
            } else {
                // This owner really presented, so publish its hardware facts.
                // A newer desired profile needs a fresh resolution; it cannot
                // rewrite the generation of the realization just presented.
                output_realization.abandon();
                tracing::info!(target: "sophia_scanout_evidence",
                    "sophia_live_output_resolution schema=1 phase=runtime status=uncommitted reason=profile_changed generation={} transition={} notice={} owner={}",
                    profile.generation.raw(), binding.transition, binding.notice_sequence, binding.native_owner);
                schedule_output_topology_rebuild!("profile_changed_during_rebuild", false);
            }
        }
    }
}
