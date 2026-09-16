{
        if let Some(controller) = seat_controller.as_mut() {
            render_owners.seat_active = false;
            if let Some(event) = controller.dispatch()? {
                seat_state = seat_state.observe(event);
            }
            render_owners.seat_active = seat_state == sophia_backend_live::LiveSeatState::Active;
            if seat_state == sophia_backend_live::LiveSeatState::Active
                && native_recovery_allowed!()
                && let Some((terminal, queued_at)) = pending_virtual_terminal
            {
                InputDeliveryPhase {
                    sender: Some(input_sender),
                    receiver: input_delivery_receiver,
                    state: &mut input_delivery,
                    client_key_release_barrier: &mut client_key_release_barrier,
                    proof_started_at: &mut input_proof_started_at,
                    post_input_deadline: &mut post_input_deadline,
                }
                .drain()?;
                if !input_delivery.pending.is_empty() && queued_at.elapsed() >= Duration::from_millis(500) {
                    for ticket in input_sender.recover_input_deliveries(Instant::now(), true)? {
                        crate::session_println!(
                            "sophia_live_session_input_recovery schema=1 status=revoked delivery={} client={} surface={} generation={} age_msec={} reason=seat_handoff content=redacted",
                            ticket.delivery.raw(), ticket.client.map_or(0, |client| client.raw()),
                            ticket.surface.index(), ticket.surface.generation(), ticket.admitted_at.elapsed().as_millis(),
                        );
                    }
                    InputDeliveryPhase {
                        sender: Some(input_sender), receiver: input_delivery_receiver,
                        state: &mut input_delivery,
                        client_key_release_barrier: &mut client_key_release_barrier,
                        proof_started_at: &mut input_proof_started_at,
                        post_input_deadline: &mut post_input_deadline,
                    }.drain()?;
                }
                if !input_delivery.pending.is_empty() {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                pending_virtual_terminal = None;
                crate::session_println!(
                    "sophia_live_session_vt schema=4 status=preparing target={terminal}"
                );
                std::io::stdout().flush()?;
                let revoked_input_leases = advance_application_input_security_epoch(
                    &mut application_route_leases,
                    input_sender,
                    &layout.client_routes,
                    route_lease_release_sender,
                )?;
                revoke_floating_pointer_interaction!("virtual_terminal");
                revoke_chrome_captures!("virtual_terminal");
                keyboard_focus_handoff = KeyboardFocusHandoffState::default();
                deferred_physical_key_timings.clear();
                crate::session_println!(
                    "sophia_live_input_epoch schema=1 reason=virtual_terminal epoch={} revoked_leases={revoked_input_leases}",
                    application_route_leases.control_epoch(),
                );
                physical_input.take();
                pause_metadata_shell_presentation!("seat_release");
                let quiesced = if let (Some(runtime), Some(native)) =
                    (runtime.as_mut(), native_scanout.as_mut())
                {
                    runtime
                        .suspend_native_scanout(native, &outputs, Duration::from_secs(2))
                } else {
                    Ok(Default::default())
                };
                match quiesced {
                    Ok(report) => {
                        native_evidence.observe_settlement(report.outcome.drained(), report.abandoned_scanouts);
                        *suspended_renderer_images = match (runtime.as_ref(), native_scanout.as_mut())
                        {
                            (Some(runtime), Some(native)) => {
                                Some(capture_renderer_image_handoff(runtime, native)?)
                            }
                            _ => None,
                        };
                        crate::session_println!(
                            "sophia_live_renderer_handoff schema=1 status=captured images={}",
                            suspended_renderer_images.as_ref().map_or(0, |handoff| handoff.len()),
                        );
                        close_native_owner!("seat_release", RetirementMode::from_suspend(report.outcome));
                        seat_release_prepared = true;
                        crate::session_println!(
                            "sophia_live_session_vt schema=6 status=quiesced target={terminal} outcome={} drained={} abandoned_scanouts={} skipped_present={}",
                            report.outcome.reduced_name(),
                            report.outcome.drained(),
                            report.abandoned_scanouts,
                            report
                                .skipped_present
                                .map_or_else(|| "none".to_owned(), |transaction| transaction.raw().to_string()),
                        );
                        match controller.switch_session(terminal) {
                            Ok(()) => {
                                requested_virtual_terminal =
                                    Some((terminal, Instant::now()));
                                crate::session_println!(
                                    "sophia_live_session_vt schema=4 status=requested target={terminal}"
                                );
                                std::io::stdout().flush()?;
                                continue;
                            }
                            Err(error) => {
                                seat_release_prepared = false;
                                if !native_recovery_allowed!() { continue; }
                                native_owner_retirement::finish_before_replacement(runtime.as_ref(), native_retirement)?;
                let resumed =
                                    LiveProductionNativeScanout::new_with_seat_mirroring_mapping_and_cursor(
                                        &controller.device_opener(),
                                        mirror_grouping,
                                        initial_head_mapping,
                                        config.cursor_resolution.asset.clone(),
                                    )?;
                                *native_scanout = Some(resumed);
                                native_retirement.admit(native_scanout.as_ref().expect("just adopted"))?;
                                let resumed = native_scanout.as_mut().expect("just adopted");
                                if resumed.outputs() != outputs {
                                    schedule_output_topology_rebuild!("switch_rejected", true);
                                    close_native_owner!("replacement_mismatch");
                                } else {
                                    let restored = resume_native_scanout_from_scene(
                                        runtime.as_mut().ok_or(
                                            "seat switch rejection lost the visual runtime",
                                        )?,
                                        resumed,
                                        &outputs,
                                        scene,
                                        suspended_renderer_images,
                                    )?;
                                    publish_resumed_topology_transport!(resumed);
                                    native_evidence.open("seat_resume");

                    native_presentation_admitted = false;
                                    crate::session_println!(
                                        "sophia_live_renderer_handoff schema=1 status=restored images={restored} source=switch_rejected"
                                    );
                                }
                                let device_map =
                                    sophia_backend_live::NativeLibinputDeviceMap::new(
                                        SeatId::from_raw(SESSION_SEAT_RAW),
                                    )
                                    .with_keyboard_device(DeviceId::from_raw(
                                        SESSION_KEYBOARD_DEVICE_RAW,
                                    ))
                                    .with_pointer_device(DeviceId::from_raw(
                                        SESSION_POINTER_DEVICE_RAW,
                                    ));
                                *physical_input = open_session_physical_input(
                                    config,
                                    device_map,
                                    Some(controller.device_opener()),
                                )?;
                                modifiers = config.keyboard_mapper();
                                virtual_terminal_chord = VirtualTerminalChordState::default();
                                emergency_chord = EmergencyChordState::armed();
                                cursor_updates =
                                    CursorUpdateState::new(pointer.position().is_some());
                                crate::session_eprintln!(
                                    "sophia_live_session_vt schema=4 status=rejected target={terminal} phase=request error={error}"
                                );
                            }
                        }
                    }
                    Err(error) => {
                        native_evidence.observe_settlement(false, 0);
                        let device_map = sophia_backend_live::NativeLibinputDeviceMap::new(
                            SeatId::from_raw(SESSION_SEAT_RAW),
                        )
                        .with_keyboard_device(DeviceId::from_raw(SESSION_KEYBOARD_DEVICE_RAW))
                        .with_pointer_device(DeviceId::from_raw(SESSION_POINTER_DEVICE_RAW));
                        *physical_input = open_session_physical_input(
                            config,
                            device_map,
                            Some(controller.device_opener()),
                        )?;
                        modifiers = config.keyboard_mapper();
                        virtual_terminal_chord = VirtualTerminalChordState::default();
                        emergency_chord = EmergencyChordState::armed();
                        crate::session_eprintln!(
                            "sophia_live_session_vt schema=4 status=rejected target={terminal} phase=quiesce error={error}"
                        );
                    }
                }
                std::io::stdout().flush()?;
            }
            if seat_state == sophia_backend_live::LiveSeatState::Active
                && native_recovery_allowed!()
                && let Some((terminal, requested_at)) = requested_virtual_terminal
                && requested_at.elapsed() >= Duration::from_secs(2)
            {
                requested_virtual_terminal = None;
                seat_release_prepared = false;
                native_owner_retirement::finish_before_replacement(runtime.as_ref(), native_retirement)?;
                let resumed =
                    LiveProductionNativeScanout::new_with_seat_mirroring_mapping_and_cursor(
                        &controller.device_opener(),
                        mirror_grouping,
                        initial_head_mapping,
                        config.cursor_resolution.asset.clone(),
                    )?;
                *native_scanout = Some(resumed);
                native_retirement.admit(native_scanout.as_ref().expect("just adopted"))?;
                let resumed = native_scanout.as_mut().expect("just adopted");
                if resumed.outputs() != outputs {
                    schedule_output_topology_rebuild!("switch_timeout", true);
                    close_native_owner!("replacement_mismatch");
                } else {
                    let restored = resume_native_scanout_from_scene(
                        runtime
                            .as_mut()
                            .ok_or("seat switch timeout lost the visual runtime")?,
                        resumed,
                        &outputs,
                        scene,
                        suspended_renderer_images,
                    )?;
                    publish_resumed_topology_transport!(resumed);
                    native_evidence.open("seat_resume");

                    native_presentation_admitted = false;
                    crate::session_println!(
                        "sophia_live_renderer_handoff schema=1 status=restored images={restored} source=disable_timeout"
                    );
                }
                let device_map = sophia_backend_live::NativeLibinputDeviceMap::new(
                    SeatId::from_raw(SESSION_SEAT_RAW),
                )
                .with_keyboard_device(DeviceId::from_raw(SESSION_KEYBOARD_DEVICE_RAW))
                .with_pointer_device(DeviceId::from_raw(SESSION_POINTER_DEVICE_RAW));
                *physical_input = open_session_physical_input(
                    config,
                    device_map,
                    Some(controller.device_opener()),
                )?;
                modifiers = config.keyboard_mapper();
                key_repeat.cancel_seat(seat);
                virtual_terminal_chord = VirtualTerminalChordState::default();
                emergency_chord = EmergencyChordState::armed();
                if let Some(wm) = wm_session.as_mut()
                    && let Some(shortcuts) = wm.shortcuts.as_mut()
                {
                    let _ = shortcuts.clear_seat(seat);
                }
                cursor_updates = CursorUpdateState::new(pointer.position().is_some());
                crate::session_eprintln!(
                    "sophia_live_session_vt schema=4 status=rejected target={terminal} phase=disable_timeout"
                );
                std::io::stdout().flush()?;
            }
            if seat_state == sophia_backend_live::LiveSeatState::Active
                && native_recovery_allowed!()
                && requested_virtual_terminal.is_some()
            {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            if seat_state == sophia_backend_live::LiveSeatState::ReleasePending {
                crate::session_println!("sophia_live_seat schema=1 status=release_pending");
                if !seat_release_prepared {
                    let revoked_input_leases = advance_application_input_security_epoch(
                        &mut application_route_leases,
                        input_sender,
                        &layout.client_routes,
                        route_lease_release_sender,
                    )?;
                    revoke_floating_pointer_interaction!("seat_release");
                    revoke_chrome_captures!("seat_release");
                    keyboard_focus_handoff = KeyboardFocusHandoffState::default();
                    deferred_physical_key_timings.clear();
                    crate::session_println!(
                        "sophia_live_input_epoch schema=1 reason=seat_release epoch={} revoked_leases={revoked_input_leases}",
                        application_route_leases.control_epoch(),
                    );
                }
                if let Some(surface) = applied_client_focus {
                    flush_client_keys!(surface, "seat_release");
                }
                physical_input.take();
                if !seat_release_prepared
                    && let Some(runtime) = runtime.as_mut()
                {
                    let report = runtime.suspend_revoked_native_scanout(&outputs)?;
                    native_evidence.observe_settlement(report.outcome.drained(), report.abandoned_scanouts);
                    let discarded_renderer_images = runtime.discard_retained_renderer_images();
                    *suspended_renderer_images = None;
                    crate::session_println!(
                        "sophia_live_seat schema=2 status=forced_detach abandoned_scanouts={} skipped_present={}",
                        report.abandoned_scanouts,
                        report
                            .skipped_present
                            .map_or_else(|| "none".to_owned(), |transaction| transaction.raw().to_string()),
                    );
                    crate::session_println!(
                        "sophia_live_renderer_handoff schema=1 status=discarded images={discarded_renderer_images} source=forced_detach"
                    );
                }
                close_native_owner!("seat_release", RetirementMode::DeviceRevoked);
                controller.acknowledge_disable()?;
                seat_state = seat_state.released();
                render_owners.seat_active = false;
                seat_release_prepared = false;
                requested_virtual_terminal = None;
                modifiers = config.keyboard_mapper();
                key_repeat.cancel_seat(seat);
                virtual_terminal_chord = VirtualTerminalChordState::default();
                emergency_chord = EmergencyChordState::armed();
                crate::session_println!("sophia_live_seat schema=1 status=suspended");
                std::io::stdout().flush()?;
            }
            if seat_state == sophia_backend_live::LiveSeatState::AcquirePending
                && native_recovery_allowed!()
            {
                crate::session_println!("sophia_live_seat schema=1 status=acquire_pending");
                native_owner_retirement::finish_before_replacement(runtime.as_ref(), native_retirement)?;
                let resumed =
                    LiveProductionNativeScanout::new_with_seat_mirroring_mapping_and_cursor(
                        &controller.device_opener(),
                        mirror_grouping,
                        initial_head_mapping,
                        config.cursor_resolution.asset.clone(),
                    )?;
                *native_scanout = Some(resumed);
                native_retirement.admit(native_scanout.as_ref().expect("just adopted"))?;
                let resumed = native_scanout.as_mut().expect("just adopted");
                if resumed.outputs() != outputs {
                    schedule_output_topology_rebuild!("seat_resume", true);
                    close_native_owner!("replacement_mismatch");
                } else {
                    let frames = scene.frames_for_outputs(&outputs)?;
                    let scene_outputs = frames.len();
                    let nonzero_scene_outputs = frames
                        .iter()
                        .filter(|frame| frame.nonzero_pixel_bytes > 0)
                        .count();
                    let primary_nonzero_pixel_bytes = frames
                        .first()
                        .map_or(0, |frame| frame.nonzero_pixel_bytes);
                    let restored = resume_native_scanout_from_scene(
                        runtime
                            .as_mut()
                            .ok_or("seat resume lost the visual runtime")?,
                        resumed,
                        &outputs,
                        scene,
                        suspended_renderer_images,
                    )?;
                    publish_resumed_topology_transport!(resumed);
                    native_evidence.open("seat_resume");

                    native_presentation_admitted = false;
                    // CPU snapshots live in the Engine scene, outside the imported
                    // renderer-image table. Record both recovery paths separately.
                    crate::session_println!(
                        "sophia_live_scene_handoff schema=1 status=rehydrated outputs={scene_outputs} nonzero_outputs={nonzero_scene_outputs} primary_nonzero_pixel_bytes={primary_nonzero_pixel_bytes} source=seat_resume"
                    );
                    crate::session_println!(
                        "sophia_live_renderer_handoff schema=1 status=restored images={restored} source=seat_resume"
                    );
                }
                let device_map = sophia_backend_live::NativeLibinputDeviceMap::new(
                    SeatId::from_raw(SESSION_SEAT_RAW),
                )
                .with_keyboard_device(DeviceId::from_raw(SESSION_KEYBOARD_DEVICE_RAW))
                .with_pointer_device(DeviceId::from_raw(SESSION_POINTER_DEVICE_RAW));
                *physical_input = open_session_physical_input(
                    config,
                    device_map,
                    Some(controller.device_opener()),
                )?;
                cursor_updates = CursorUpdateState::new(pointer.position().is_some());
                seat_state = seat_state.acquired();
                render_owners.seat_active = seat_state == sophia_backend_live::LiveSeatState::Active;
                crate::session_println!("sophia_live_seat schema=1 status=active source=resume");
                std::io::stdout().flush()?;
            }
            if seat_state == sophia_backend_live::LiveSeatState::Failed {
                return Err("invalid libseat lifecycle transition".into());
            }
        }
        let child_reap_started = Instant::now();
        if !primary_child_exited
            && let Some(primary_child) = child.as_deref_mut()
            && let Some(status) = primary_child.try_wait()?
        {
            primary_exit_status = Some(status);
            if !status.success() && !config.normal_session {
                let error =
                    format!("session client exited during live session with status {status}");
                terminal_client_error = Some(("primary", error));
                if let Err(error) = stop_frontend_intake(
                    frontend_service_sender,
                    &mut terminal_client_intake_stopped,
                ) {
                    terminal_client_cleanup_failures
                        .push(format!("frontend intake stop failed: {error}"));
                }
                crate::session_println!(
                    "sophia_live_session_client_fatal schema=1 status=detected source=primary exit_status={status} action=bounded_cleanup"
                );
                break 'session;
            }
            if status.success()
                && config.expect_physical_pointer
                && metrics.physical_pointer_buttons_routed == 0
            {
                return Err(
                    "session client exited before the required physical pointer selection".into(),
                );
            }
            if config.application_proof_requested() {
                client_stdout = client_stdout_capture
                    .ok_or("application stdout capture is missing")?
                    .read_bounded()?;
                if client_stdout.len() > 4_096 {
                    return Err("application stdout exceeded the 4096-byte evidence bound".into());
                }
                if let (Some(text), Some(expected)) = (
                    config.inject_text.as_deref(),
                    config.expect_client_stdout.as_deref(),
                ) && client_stdout == expected.as_bytes()
                {
                    input_text_match = true;
                    crate::session_println!(
                        "sophia_live_session_input schema=3 status=semantic_complete source=synthetic text_match=true bytes={}",
                        text.len()
                    );
                }
            }
            if config.normal_session {
                if let Some(id) = config.applications.startup.first() {
                    crate::session_println!(
                        "sophia_session_app schema=1 status=exited id={id} source=startup exit_status={status}",
                    );
                }
                primary_child_exited = true;
                if config.exit_when_startup_exits {
                    begin_session_quiescence!("startup_application_exit");
                }
            } else {
                if status.success()
                    && successful_primary_exit_ends_session(config.input_proof_requested())
                {
                    begin_session_quiescence!("successful_primary_exit");
                }
                // The proof helper intentionally exits after displaying its
                // received text. Keep the session and secondary terminal alive so
                // the final native frame can retire and pointer evidence can run.
                primary_child_exited = true;
            }
        }
        if config.application_proof_requested()
            && !input_text_match
            && physical_text_proof
                .as_ref()
                .is_some_and(PhysicalTextProof::is_complete)
        {
            input_text_match = true;
            crate::session_println!(
                "sophia_live_session_input schema=3 status=semantic_complete source=physical text_match=true bytes={}",
                config.expect_physical_text.as_ref().map_or(0, String::len)
            );
        }
        let mut secondary_index = 0;
        while secondary_index < secondary_children.len() {
            if let Some(status) = secondary_children[secondary_index].child.try_wait()? {
                if managed_child_exit_is_nonfatal(
                    config.normal_session,
                    secondary_children[secondary_index].launch_transaction,
                ) {
                    terminate_session_child(&mut secondary_children[secondary_index].child, true)?;
                    let launch_transaction =
                        secondary_children[secondary_index].launch_transaction;
                    let id = secondary_children[secondary_index]
                        .id
                        .as_deref()
                        .unwrap_or("untracked");
                    crate::session_println!(
                        "sophia_session_app schema=1 status=exited id={id} source=managed exit_status={status}",
                    );
                    let exiting_admission = launch_transaction.is_some_and(|transaction| {
                        session_launches
                            .admission()
                            .is_some_and(|admission| admission.intent.transaction == transaction && session_launches.catalog_admission(transaction)==secondary_children[secondary_index].catalog_launch)
                    });
                    if exiting_admission
                        && status.success()
                        && let Some(admission) = session_launches.complete_successful_exit(
                            launch_transaction.expect("exiting admission has a transaction"),
                            secondary_children[secondary_index].catalog_launch,
                        )
                    {
                        launch_admission_started_at = None;
                        crate::session_println!(
                            "sophia_session_app schema=2 status=completed id={id} source=action transaction={} reason=normal_exit exit_status={status}",
                            admission.intent.transaction.raw(),
                        );
                    } else if exiting_admission
                        && let Some(admission) = session_launches.fail_current()
                    {
                        launch_admission_started_at = None;
                        crate::session_eprintln!(
                            "sophia_session_app schema=2 status=failed id={id} source=action transaction={} application={} reason=exit_before_admission exit_status={status}",
                            admission.intent.transaction.raw(),
                            admission.intent.application.raw(),
                        );
                    }
                    secondary_children.remove(secondary_index);
                } else {
                    let error = format!(
                        "secondary xterm {} exited during live session with status {status}",
                        secondary_index + 1
                    );
                    terminal_client_error = Some(("secondary", error));
                    if let Err(error) = stop_frontend_intake(
                        frontend_service_sender,
                        &mut terminal_client_intake_stopped,
                    ) {
                        terminal_client_cleanup_failures
                            .push(format!("frontend intake stop failed: {error}"));
                    }
                    crate::session_println!(
                        "sophia_live_session_client_fatal schema=1 status=detected source=secondary index={} exit_status={status} action=bounded_cleanup",
                        secondary_index + 1,
                    );
                    break 'session;
                }
            } else {
                secondary_index += 1;
            }
        }
        metrics.max_child_reap = metrics.max_child_reap.max(child_reap_started.elapsed());
        InputDeliveryPhase {
                    sender: Some(input_sender),
            receiver: input_delivery_receiver,
            state: &mut input_delivery,
            client_key_release_barrier: &mut client_key_release_barrier,
            proof_started_at: &mut input_proof_started_at,
            post_input_deadline: &mut post_input_deadline,
        }
        .drain()?;
        if emergency_exit_requested && input_delivery.pending.is_empty() {
            break;
        }
        if !input_text_match
            && let (Some(expected), Some(result)) = (
                config
                    .inject_text
                    .as_deref()
                    .or(config.expect_physical_text.as_deref()),
                input_proof_result,
            )
            && let Some(received) = result.received()?
        {
            if received != expected.as_bytes() {
                return Err(format!(
                    "persistent live session terminal received incorrect input: expected_bytes={} received_bytes={}",
                    expected.len(),
                    received.len(),
                )
                .into());
            }
            input_text_match = true;
            crate::session_println!(
                "sophia_live_session_input schema=3 status=semantic_complete source={} text_match=true bytes={}",
                if config.inject_text.is_some() {
                    "synthetic"
                } else {
                    "physical"
                },
                received.len(),
            );
            std::io::stdout().flush()?;
        }
        if let Some(post_input_deadline) = post_input_deadline
            && Instant::now() >= post_input_deadline
            && !input_text_match
        {
            return Err(
                "persistent live session timed out waiting for the terminal to receive exact text and Return"
                    .into(),
            );
        }
        if input_presented_latency.is_none()
            && let Some(post_input_deadline) = post_input_deadline
            && Instant::now() >= post_input_deadline
        {
            if !input_pixel_change {
                return Err(format!(
                    "persistent live session timed out waiting for pixels after flushed X11 input: expected={} flushed={} authority_batches_after_input={} cpu_updates_after_input={} baseline_checksum={injection_checksum:?} final_checksum={:?} baseline_generation={input_surface_generation:?} final_generation={:?} input_surface_pixel_change={input_surface_pixel_change} native_submission_baseline={input_change_submission_baseline:?} native_submissions={} native_callbacks={}",
                    input_delivery.events_expected,
                    input_delivery.events_flushed,
                    metrics.batches.saturating_sub(input_batch_baseline.unwrap_or(metrics.batches)),
                    metrics.cpu_buffer_updates.saturating_sub(input_cpu_update_baseline.unwrap_or(metrics.cpu_buffer_updates)),
                    scene.last_report().map(|report| report.checksum),
                    input_surface.and_then(|surface| {
                        runtime.as_ref().and_then(|runtime| {
                            scene.surface_buffer_generation(runtime.committed_surfaces(), surface)
                        })
                    }),
                    native_scanout.as_ref().map_or(0, |native| native.submissions),
                    native_scanout.as_ref().map_or(0, |native| native.callback_accepted),
                )
                .into());
            }
            return Err("persistent live session input pixels were not presented within the post-flush proof window".into());
        }
        if matches!(seat_state, sophia_backend_live::LiveSeatState::Suspended
            | sophia_backend_live::LiveSeatState::AcquirePending)
            && session_quiescence.is_none()
        {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        if let (Some(runtime), Some(native_scanout)) = (runtime.as_mut(), native_scanout.as_mut())
            && native_scanout.output_topology_allows_frame_service()
        {
            if layout.pending.is_none() {
                runtime.release_layout_deferred_presentations();
            }
            // Deliberately outside the guard above. A first candidate is
            // parked while admission waits for it, and admission is what
            // makes a layout pending, so servicing parked candidates only
            // when nothing is pending cannot release the ones that matter.
            runtime.service_first_visibility_presentations(Instant::now());
            // A frame presented before its window mapped escaped admission
            // entirely, so waiting on visibility for it only delays the redraw
            // that can actually be admitted. Skip it as soon as the authority
            // has confirmed the map, and only once production still holds it:
            // a request dropped on a queue miss would strand the client the
            // way the unbounded wait used to.
            let skippable = layout.skippable_escaped_presents();
            for key in skippable {
                // Consumed once production has taken the candidate, settled or
                // not: it is out of the queue either way, and asking again for
                // a candidate that is gone would spin forever. A queue miss is
                // different -- intake may not have reached it -- so that record
                // stands.
                if runtime.skip_escaped_pre_admission(key).is_some() {
                    layout.consume_escaped_present(key);
                }
            }
            let service = match runtime.service_native(native_scanout, scene) {
                Ok(service) => Some(service),
                Err(error) => {
                    let Some(execution) = active_output_topology_preparation.as_mut() else {
                        return Err(error);
                    };
                    let transaction = execution.effect.transaction;
                    let failure = error.to_string();
                    let recovered = begin_output_topology_first_presentation_rollback(
                        &mut execution.phase,
                        transaction,
                        &failure,
                        |reason| native_scanout.request_output_topology_rollback(reason),
                        |transaction| {
                            wm_session
                                .as_mut()
                                .ok_or_else(|| {
                                    Box::<dyn std::error::Error>::from(
                                        "first-presentation rollback lost its WM owner",
                                    )
                                })?
                                .reject_output_topology_effect(
                                    transaction,
                                    sophia_engine::OutputTopologyTransactionFailure::FirstPresentation,
                                )
                        },
                    )?;
                    if !recovered {
                        return Err(error);
                    }
                    tracing::warn!(
                        "sophia_live_output_authority schema=2 status=rollback_started transaction={} reason=first_presentation_service error={error} published=false",
                        transaction.raw(),
                    );
                    None
                }
            };
            if let Some(service) = service {
                for retired in service.retired_software_presents {
                    record_native_software_present_retirement(&mut layout, retired);
                }
                if let Some(retired) = service.retired_present {
                    let NativePresentRetirementObservation {
                        surface,
                        stable,
                        ust_usec: _,
                        msc: _,
                    } = record_native_present_retirement(
                        &mut layout,
                        runtime,
                        native_scanout,
                        retired,
                        &mut retired_present_surfaces,
                        &mut startup_surface_presentations,
                        &mut startup_readiness,
                    );
                    if stable_gpu_frame_proves_post_input_pixels(
                        input_proof_started_at.is_some(),
                        input_surface,
                        surface,
                        stable,
                    ) {
                        input_pixel_change = true;
                    }
                }
            }
            correlate_physical_input_page_flip(
                input_proof_started_at.is_some(),
                input_pixel_change,
                input_raw_ingress_msec,
                input_change_submission_baseline,
                input_change_frame_baseline,
                native_scanout,
                &mut input_presented_ust_usec,
                &mut input_submit_to_page_flip,
            );
            if let Some(head) = native_scanout.heads.first() {
                input_latency_samples.observe_page_flip(
                    head.presented_submissions,
                    head.presented_content
                        .map_or(0, |content| content.frame().raw()),
                    head.presented_submission_ust_usec,
                    head.presented_page_flip_ust_usec,
                );
            }
            metrics.runtime_surfaces =
                u64::try_from(runtime.committed_surfaces().len()).unwrap_or(u64::MAX);
            reconcile_initial_session_focus(InitialSessionFocusContext {
                runtime,
                focus: &mut focus,
                seat,
                wm_session_present: wm_session.is_some(),
                layout: &layout,
                session_controls: &mut session_controls,
                next_focus_control_transaction: &mut next_focus_control_transaction,
            })?;
            // Admission focus can become eligible on a page-flip retirement.
            // Reconcile it here so an idle client does not need to emit another
            // authority batch before it can receive focus.
            reconcile_pending_wm_focus!(runtime);
        }
        // Retirement may leave the native request idle before its service
        // deadline is armed. Deliver its feedback before an authority-only
        // batch can skip the remainder of this owner turn.
        if let Some(runtime) = runtime.as_mut() {
            present_observer.drain_pending_feedback_with_allocation(
                runtime,
                &mut present_feedback,
                Some(window_allocation::LiveWindowAllocationView {
                    publisher: &window_allocation_publisher,
                    native: native_scanout.as_ref(),
                    layout: &layout,
                    outputs: &outputs,
                    topology_generation: output_topology_owner.publication_generation,
                }),
            )?;
            visual_progress.observe_committed(runtime.committed_surfaces());
        }
        if let Some(native) = native_scanout.as_ref() {
            visual_progress.observe_native(native);
        }
        let mut input_routing_mode = physical_input_routing_mode(
            primary_child_exited,
            focus.focused_surface(seat),
            input_surface,
            wm_session.as_ref().is_some_and(|wm| wm.shortcuts.is_some()),
        );
        if config.expect_physical_text.is_some() && physical_input_ready_at.is_none() {
            input_routing_mode = PhysicalInputRoutingMode::CursorOnly;
        }
        if input_routing_mode != PhysicalInputRoutingMode::Suppressed
            && focus.focused_surface(seat) != applied_client_focus
        {
            input_routing_mode = PhysicalInputRoutingMode::ControlPlaneOnly;
        }
        if output_topology_owner.input_quarantined() {
            input_routing_mode = PhysicalInputRoutingMode::ShortcutsOnly;
        }
        if runtime_deadline_key_drain.is_draining() || session_quiescence.is_some() {
            input_routing_mode = PhysicalInputRoutingMode::Suppressed;
        }
        let explicit_controls = drain_explicit_pointer_grab_controls(
            explicit_pointer_grabs,
            &mut application_route_leases,
            &mut explicit_grab_queue,
            &layout,
            &mut pending_lease_input,
            route_lease_release_sender,
            chrome_captures.capture(seat).is_some() || descriptor_captures.capture(seat).is_some() || reference_capture.active() || launcher_capture.active() || pointer_focus_handoff.target().is_some(),
            &focus,
            seat,
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        )?;
        if explicit_controls.prepared + explicit_controls.activated + explicit_controls.released
            + explicit_controls.aborted + explicit_controls.rejected + explicit_controls.cancelled != 0 {
            crate::session_println!(
                "sophia_live_explicit_pointer_grab schema=1 prepared={} activated={} released={} aborted={} rejected={} deferred={} cancelled={}",
                explicit_controls.prepared,
                explicit_controls.activated,
                explicit_controls.released,
                explicit_controls.aborted,
                explicit_controls.rejected,
                explicit_controls.deferred,
                explicit_controls.cancelled,
            );
        }
        reconcile_lease_presentation(
            &mut application_route_leases, &layout.client_routes, route_lease_release_sender,
            &mut pending_lease_input, seat,
            pointer.output_index().and_then(|index| outputs.get(index)).map(|output| output.id),
            runtime.as_ref().map_or(&[][..], |runtime| runtime.input_projections()),
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        )?;
        let input_phase_started = Instant::now();
        let input_requested_exit = input_routing_mode != PhysicalInputRoutingMode::Suppressed
            && drain_physical_input!(input_routing_mode);
        metrics.max_input_phase = metrics.max_input_phase.max(input_phase_started.elapsed());
        if input_requested_exit {
            break;
        }
        // `Queued` transfers ownership to the backend. Observe completion on
        // the following owner pass instead of claiming visible pixels merely
        // because the desired position entered the latest-wins cell.
        if !cursor_updates.dirty
            && cursor_updates.dirty_since.is_some()
            && native_scanout
                .as_ref()
                .is_some_and(|native| native.pending_atomic_cursor_count() == 0)
        {
            pointer_pixel_change |= metrics.physical_pointer_routed > 0;
            if let Some(started) = cursor_updates.dirty_since.take() {
                metrics.cursor_max_motion_to_submit =
                    metrics.cursor_max_motion_to_submit.max(started.elapsed());
            }
            if !cursor_visible_reported {
                crate::session_println!(
                    "sophia_live_session_pointer schema=2 status=visible source=hardware_cursor"
                );
                cursor_visible_reported = true;
            }
            if config.expect_physical_pointer
                && physical_input_completion_reported
                && input_pixel_change
                && pointer_phase_started_at.is_none()
            {
                pointer_checksum = Some(0);
                pointer_phase_started_at = Some(Instant::now());
                crate::session_println!(
                    "sophia_live_session_pointer schema=1 status=visible source=physical position=center"
                );
                crate::session_println!(
                    "sophia_live_session_pointer schema=1 status=ready source=physical action=select"
                );
                std::io::stdout().flush()?;
            }
        }
        // Shake-to-find, before the move is submitted: the gesture is read from
        // the same positions the hardware cursor is about to be placed at, so
        // there is no second source of pointer truth to keep in step.
        //
        // The pointer is watched whatever input routing is doing. A client
        // that has grabbed the pointer has not taken the cursor -- the Engine
        // owns that -- and someone who cannot find it is exactly as lost
        // either way.
        if config.cursor_shake_resolution.is_some() && !cursor_shake_refused {
            let now_msec =
                u64::try_from(cursor_shake_epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
            let at = pointer
                .position()
                .map(|position| (position.x as i32, position.y as i32));
            let action = match at {
                Some(at) if Some(at) != cursor_shake_seen => {
                    cursor_shake_seen = Some(at);
                    cursor_shake.observe_motion(true, at.0, at.1, now_msec)
                }
                _ => cursor_shake.tick(true, now_msec),
            };
            if let Some(action) = action {
                let resolution = match action {
                    sophia_engine::CursorShakeAction::Enlarge => config
                        .cursor_shake_resolution
                        .as_ref()
                        .unwrap_or(&config.cursor_resolution),
                    sophia_engine::CursorShakeAction::Restore => &config.cursor_resolution,
                };
                scene.set_cursor_asset(resolution.asset.clone());
                if let Some(native) = native_scanout.as_mut() {
                    // A refusal here is not a session failure. The cursor the
                    // operator has stays on screen, which is worse than the
                    // one they asked for and much better than no desktop.
                    if let Err(error) =
                        native.replace_hardware_cursor_asset(resolution.asset.clone())
                    {
                        crate::session_eprintln!(
                            "sophia_live_cursor schema=1 status=shake_declined action={action:?} detail={error}"
                        );
                        // Leave the base cursor installed rather than whatever
                        // partial state a multi-head refusal stopped at.
                        let _ = native
                            .replace_hardware_cursor_asset(config.cursor_resolution.asset.clone());
                        scene.set_cursor_asset(config.cursor_resolution.asset.clone());
                        cursor_shake_refused = true;
                    }
                }
                crate::session_println!(
                    "sophia_live_cursor schema=1 status=shake action={action:?} size={}",
                    resolution.effective_nominal_size,
                );
                cursor_updates.dirty = pointer.position().is_some();
            }
        }
        if cursor_updates.dirty
            && let (Some(native_scanout), Some(runtime), Some(position)) =
                (native_scanout.as_mut(), runtime.as_ref(), pointer.position())
        {
            let logical_viewports = runtime.logical_viewports();
            match native_scanout
                .update_classic_hardware_cursor(position, &logical_viewports)
            {
                Ok(ClassicHardwareCursorUpdate::Visible) => {
                    pointer_pixel_change |= metrics.physical_pointer_routed > 0;
                    if let Some(started) = cursor_updates.dirty_since.take() {
                        metrics.cursor_max_motion_to_submit =
                            metrics.cursor_max_motion_to_submit.max(started.elapsed());
                    }
                    cursor_updates.dirty = false;
                    if !cursor_visible_reported {
                        crate::session_println!(
                            "sophia_live_session_pointer schema=2 status=visible source=hardware_cursor"
                        );
                        cursor_visible_reported = true;
                    }
                    if config.expect_physical_pointer
                        && physical_input_completion_reported
                        && input_pixel_change
                        && pointer_phase_started_at.is_none()
                    {
                        pointer_checksum = Some(0);
                        pointer_phase_started_at = Some(Instant::now());
                        crate::session_println!(
                            "sophia_live_session_pointer schema=1 status=visible source=physical position=center"
                        );
                        crate::session_println!(
                            "sophia_live_session_pointer schema=1 status=ready source=physical action=select"
                        );
                        std::io::stdout().flush()?;
                    }
                }
                Ok(ClassicHardwareCursorUpdate::Hidden) => {
                    cursor_updates.dirty = false;
                }
                Ok(ClassicHardwareCursorUpdate::Queued) => {
                    // The backend owns progress from here. Leaving `dirty`
                    // set would resubmit the same position on every owner
                    // pass and defeat latest-wins coalescing.
                    cursor_updates.dirty = false;
                }
                Ok(ClassicHardwareCursorUpdate::Deferred) => {}
                Err(error) => {
                    crate::session_eprintln!(
                        "sophia_live_session_pointer schema=2 status=unavailable source=hardware_cursor error={error}"
                    );
                    return Err(format!(
                        "native session cannot provide an owned atomic cursor: {error}"
                    )
                    .into());
                }
            }
        }
        if let Some(candidate) = pointer_cursor_checksum
            && native_scanout.as_ref().is_none_or(|native| {
                native.heads.first().is_some_and(|head| {
                    head.presented_logical_checksum == candidate && head.nonzero_exports > 0
                })
            })
        {
            pointer_checksum = Some(candidate);
            pointer_cursor_checksum = None;
            pointer_phase_started_at = Some(Instant::now());
            crate::session_println!(
                "sophia_live_session_pointer schema=1 status=visible source=physical position=center"
            );
            crate::session_println!(
                "sophia_live_session_pointer schema=1 status=ready source=physical action=select"
            );
            std::io::stdout().flush()?;
        }

        if let Some(surface) = focus.focused_surface(seat) {
            let cpu_visual_detail = runtime.as_ref().and_then(|runtime| {
                runtime
                    .committed_surfaces()
                    .iter()
                    .any(|committed| committed.surface == surface)
                    .then(|| {
                        scene.surface_has_visual_detail(runtime.committed_surfaces(), surface)
                    })
            });
            let stable_gpu_pixels = startup_surface_presentations.nonzero_rgb_pixels(surface);
            let stable_gpu_detail = startup_surface_presentations.visual_detail(surface);
            let visual_detail =
                startup_surface_visual_detail(cpu_visual_detail, stable_gpu_pixels);
            if visual_detail {
                let _ = reduce_session_startup(
                    &mut startup_readiness,
                    SessionStartupEvent::VisualDetail(surface),
                );
            }
            if stable_gpu_detail {
                let _ = reduce_session_startup(
                    &mut startup_readiness,
                    SessionStartupEvent::StablePresented(surface),
                );
            }
            if input_content_surface != Some(surface) && visual_detail {
                input_content_surface = Some(surface);
                crate::session_println!(
                    "sophia_live_session_input_pipeline schema=2 status=content_ready source={}",
                    if stable_gpu_detail {
                        "stable_present_scanout"
                    } else {
                        "cpu_visual_detail"
                    }
                );
                std::io::stdout().flush()?;
            }
            if !startup_content_ready && visual_detail {
                startup_content_ready = true;
                crate::session_println!(
                    "sophia_live_session_startup schema=2 status=content_ready source={} nonzero_rgb_pixels={stable_gpu_pixels}",
                    if stable_gpu_detail {
                        "stable_present_scanout"
                    } else {
                        "cpu_visual_detail"
                    }
                );
                std::io::stdout().flush()?;
            }
        }
        let focused_surface = focus.focused_surface(seat);
        let focused_client_ready =
            focused_surface.is_some() && applied_client_focus == focused_surface;
        let missing_output_callback = native_scanout.as_ref().is_some_and(|native| {
            native
                .heads
                .iter()
                .any(|head| {
                    head.callback_accepted == 0 && head.initial_modeset_submission.is_none()
                })
        });
        if !startup_outputs_ready_reported
            && let Some(native) = native_scanout.as_ref()
            && startup_output_evidence(native, None)
                .is_some_and(|outputs| all_startup_outputs_presented(&outputs))
        {
            startup_outputs_ready_reported = true;
            for record in logical_synchronous_modeset_records(native.heads.iter().map(|head| {
                (head.output.id, head.initial_modeset_submission)
            })) {
                crate::session_println!("{record}");
            }
            let _ = reduce_session_startup(
                &mut startup_readiness,
                SessionStartupEvent::OutputsPresented,
            );
            let (ready_outputs, output_count) = logical_startup_output_progress(
                native.heads.iter().map(|head| {
                    (
                        head.output.id,
                        head.callback_accepted > 0 || head.initial_modeset_submission.is_some(),
                    )
                }),
            );
            crate::session_println!(
                "sophia_live_session_startup schema=2 status=output_baseline_ready outputs={}/{}",
                ready_outputs,
                output_count,
            );
            // The profile's topology is applied by the output-authority
            // transaction, not from here: it quiesces presentation, prepares a
            // framebuffer per head and retains a rollback, which is what a
            // modeset needs and what this point in startup cannot offer.
            //
            // A second apply used to be attempted here, judged against the
            // legacy CRTC framebuffer. Atomic commits leave that field unset,
            // so it declined every session with `reason=heads` -- harmless in
            // itself, but it read as "output configuration does nothing" for
            // long enough to be worth removing, and had the read ever
            // succeeded it would have modeset without quiescing first.
            crate::session_println!(
                "sophia_live_native_topology_apply schema=2 status=owned_by_output_authority"
            );
            std::io::stdout().flush()?;
        }
        // Pixel content is application-readiness evidence, not transport
        // liveness. A valid black Present may have more client work queued;
        // rebuilding its renderer here would invalidate retained snapshots.
        let recovery_reason =
            startup_native_recovery_reason(missing_output_callback, started.elapsed());
        if !native_presentation_admitted
            && !startup_native_recovery_attempted
            && let Some(recovery_reason) = recovery_reason
            && runtime.is_some()
            && native_scanout.is_some()
            && seat_controller.is_some()
        {
            include!("startup_native_recovery.rs");
        }
        let startup_frame_presented = native_scanout.as_ref().map_or(
            !output_topology_owner.input_quarantined(),
            |native| {
            let all_outputs_presented = startup_required_submissions
                .as_ref()
                .and_then(|required| startup_output_evidence(native, Some(required)))
                .is_some_and(|outputs| all_startup_outputs_presented(&outputs));
            let focused_mixed_presented = startup_readiness.surface.is_some_and(|surface| {
                startup_surface_presentations.stable_presented(surface)
            });
            let every_output_has_retired = startup_output_evidence(native, None)
                .is_some_and(|outputs| all_startup_outputs_presented(&outputs));
                (focused_mixed_presented && every_output_has_retired) || all_outputs_presented
            },
        );
        if !startup_ready_reported
            && startup_readiness.surface.is_some()
            && startup_readiness.client_focus_applied
            && startup_readiness.visual_detail
            && startup_frame_presented
        {
            if startup_frame_presented
                && let Some(surface) = startup_readiness.surface
            {
                let _ = reduce_session_startup(
                    &mut startup_readiness,
                    SessionStartupEvent::StablePresented(surface),
                );
            }
            if startup_outputs_ready_reported || native_scanout.is_none() {
                let _ = reduce_session_startup(
                    &mut startup_readiness,
                    SessionStartupEvent::OutputsPresented,
                );
            }
        }
        // The cursor proof, when the session asked for it. Moves the pointer
        // through `place`, the same entry physical input uses, so what it
        // exercises is the cursor path the product runs rather than a route
        // built for the proof.
        if let Some(native) = native_scanout.as_mut() {
            let flips = native.direct_scanout_totals().flips;
            let flipping_output = native.direct_scanout_output();
            match direct_cursor_proof.tick(flips, flipping_output) {
                crate::live_session::direct_cursor_proof::DirectCursorAction::Move {
                    output,
                    step,
                } => {
                    if let Some(runtime) = runtime.as_ref() {
                        let head = runtime
                            .logical_viewports()
                            .into_iter()
                            .find(|(candidate, _)| *candidate == output)
                            .map(|(_, logical)| logical)
                            .ok_or("the cursor proof's output has no logical viewport")?;
                        let position =
                            crate::live_session::direct_cursor_proof::cursor_position(head, step);
                        pointer.place(position, None);
                        cursor_updates.dirty_since.get_or_insert_with(Instant::now);
                        cursor_updates.dirty = true;
                        if step == 0 {
                            crate::session_println!(
                                "sophia_live_direct_scanout_cursor_proof schema=1 status=started output={} flips_before={flips}",
                                output.raw()
                            );
                        }
                    }
                }
                crate::live_session::direct_cursor_proof::DirectCursorAction::Finished {
                    moves,
                } => {
                    crate::session_println!(
                        "sophia_live_direct_scanout_cursor_proof schema=1 status=finished moves={moves} flips_after={flips}"
                    );
                }
                crate::live_session::direct_cursor_proof::DirectCursorAction::Idle => {}
            }
        }
        // The overlay proof, when the session asked for it. Placed on the tick
        // rather than in the input phase because nothing here is driven by
        // input: the shell would open this overlay from a shortcut, and this
        // session has no shell to press one in.
        if let Some(native) = native_scanout.as_mut() {
            let flips = native.direct_scanout_totals().flips;
            let flipping_output = native.direct_scanout_output();
            match direct_overlay_proof.tick(flips, flipping_output) {
                crate::live_session::direct_overlay_proof::DirectOverlayAction::Activate(
                    output,
                ) => {
                    let head = runtime
                        .as_ref()
                        .and_then(|runtime| {
                            runtime
                                .logical_viewports()
                                .into_iter()
                                .find(|(id, _)| *id == output)
                        })
                        .map(|(_, viewport)| viewport);
                    if let (Some(runtime), Some(head)) = (runtime.as_mut(), head) {
                        let overlay =
                            crate::live_session::direct_overlay_proof::overlay_projection(
                                output,
                                direct_overlay_generation,
                                head,
                            );
                        // The same entry the shell uses. A proof that drove a
                        // private path would prove nothing about the product.
                        match runtime.set_descriptor_overlay(
                            Some(overlay),
                            scene,
                            native_scanout.as_mut(),
                        ) {
                            Ok(_) => crate::session_println!(
                                "sophia_live_direct_scanout_overlay_proof schema=1 status=activated output={} flips_before={flips}",
                                output.raw(),
                            ),
                            Err(error) => {
                                return Err(format!(
                                    "direct-scanout overlay proof could not activate: {error}"
                                )
                                .into());
                            }
                        }
                    }
                }
                crate::live_session::direct_overlay_proof::DirectOverlayAction::Withdraw => {
                    if let Some(runtime) = runtime.as_mut() {
                        match runtime.set_descriptor_overlay(
                            None,
                            scene,
                            native_scanout.as_mut(),
                        ) {
                            Ok(_) => crate::session_println!(
                                "sophia_live_direct_scanout_overlay_proof schema=1 status=withdrawn output=0 flips_before={flips}",
                            ),
                            Err(error) => {
                                return Err(format!(
                                    "direct-scanout overlay proof could not withdraw: {error}"
                                )
                                .into());
                            }
                        }
                    }
                }
                crate::live_session::direct_overlay_proof::DirectOverlayAction::Idle => {}
            }
        }
        if startup_proof_requested && !startup_ready_reported && startup_readiness.ready {
            startup_ready_reported = true;
            startup_ready_msec.get_or_insert_with(|| started.elapsed().as_millis());
            let (presented_submissions, presented_checksum, refresh_millihz) = native_scanout
                .as_ref()
                .and_then(|native| native.heads.first())
                .map_or((0, None, 0), |head| {
                    (
                        head.presented_submissions,
                        presented_logical_checksum(head.presented_content),
                        head.refresh_millihz,
                    )
                });
            cpu_visual_progress.observe_ready(
                Instant::now(), presented_submissions, presented_checksum, refresh_millihz,
            );
            let logical_output_progress = native_scanout.as_ref().map(|native| {
                logical_startup_output_progress(native.heads.iter().map(|head| {
                    (
                        head.output.id,
                        head.callback_accepted > 0 || head.initial_modeset_submission.is_some(),
                    )
                }))
            });
            crate::session_println!(
                "sophia_live_session_startup schema=2 status=ready elapsed_msec={} surface=true visual_detail=true presented=true outputs_ready={}/{} recovery_attempts={}",
                started.elapsed().as_millis(),
                logical_output_progress.map_or(1, |progress| progress.0),
                logical_output_progress.map_or(1, |progress| progress.1),
                usize::from(startup_native_recovery_attempted),
            );
            std::io::stdout().flush()?;
        }
        // Rendering eligibility follows completed presentation, not whether
        // the user chose to open a focusable application. Explicit proofs
        // still require their exact surface before bypassing composition.
        let native_outputs_presented = native_scanout.as_ref().is_some_and(|native| {
            startup_output_evidence(native, None)
                .is_some_and(|outputs| all_startup_outputs_presented(&outputs))
        }) && !output_topology_owner.input_quarantined();
        if !native_presentation_admitted
            && native_outputs_presented
            && (!startup_proof_requested || startup_readiness.ready)
        {
            native_presentation_admitted = true;
            if let Some(native) = native_scanout.as_mut() {
                native.admit_direct_scanout();
                // The cursor moves atomically from here, if the session asked
                // and the card agreed. Chosen at readiness rather than at
                // setup because the probe runs when the cursor is first
                // prepared, which is after the first frames.
                //
                // Recorded either way, and with what was asked beside what was
                // taken. A session that opted out records the legacy path
                // rather than nothing, because an absent record cannot be told
                // apart from a session that never reached readiness; and a card
                // that refused the plane is a different run from an operator
                // who asked for the ioctl, which `path` alone cannot say.
                let requested = if config.atomic_cursor {
                    "atomic_plane"
                } else {
                    "legacy_ioctl"
                };
                let path = if config.atomic_cursor {
                    match native.use_atomic_cursor_plane() {
                        sophia_backend_live::HardwareCursorPath::AtomicPlane => "atomic_plane",
                        sophia_backend_live::HardwareCursorPath::LegacyIoctl => "legacy_ioctl",
                    }
                } else {
                    "legacy_ioctl"
                };
                crate::session_println!(
                    "sophia_live_cursor_path schema=2 status=selected requested={requested} path={path}"
                );
            }
        }
        include!("startup_watchdog.rs");

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
        if require_startup_focus
            && focus.focused_surface(seat).is_none()
            && focus_deadline_started_at
                .is_some_and(|started: Instant| started.elapsed() >= Duration::from_secs(5))
        {
            return Err(
                "live-session input focus was not ready within five seconds of the first presented frame"
                    .into(),
            );
        }
        let physical_sequence_complete = physical_text_proof
            .as_ref()
            .is_none_or(PhysicalTextProof::is_complete);
        let waiting_for_keyboard_sequence =
            physical_input_ready_at.is_some() && !physical_sequence_complete;
        let waiting_for_pointer_selection = crate::input_proof::pointer_selection_waiting(
            config.expect_physical_pointer,
            physical_sequence_complete,
            input_pixel_change,
            pointer_checksum.is_some(),
            metrics.physical_pointer_buttons_routed,
            pointer_pixel_change,
        );
        if waiting_for_keyboard_sequence {
            let ready_at = physical_input_ready_at.expect("checked above");
            if ready_at.elapsed()
                >= Duration::from_millis(config.physical_sequence_timeout_msec)
            {
                let proof = physical_text_proof.as_ref().expect("checked above");
                return Err(format!(
                    "persistent live session timed out waiting for exact physical input sequence: matched_events={} expected_events={} keyboard_routed={physical_keys_routed}",
                    proof.matched_events(),
                    proof.expected_events(),
                    physical_keys_routed = metrics.physical_keys_routed,
                )
                .into());
            }
        } else if waiting_for_pointer_selection {
            let started_at = pointer_phase_started_at.expect("set above");
            if started_at.elapsed()
                >= Duration::from_millis(config.physical_sequence_timeout_msec)
            {
                return Err(format!(
                    "persistent live session timed out waiting for a routed physical pointer button: pointer_observed={physical_pointer_events} pointer_routed={physical_pointer_routed} pointer_buttons={physical_pointer_buttons_routed} pointer_pixels={pointer_pixel_change}",
                    physical_pointer_events = metrics.physical_pointer_events,
                    physical_pointer_routed = metrics.physical_pointer_routed,
                    physical_pointer_buttons_routed =
                        metrics.physical_pointer_buttons_routed,
                )
                .into());
            }
        } else if input_delivery.wait_started_at.is_none()
            && (input_proof_started_at.is_none() || input_presented_latency.is_some())
        {
            if config
                .max_ticks
                .is_some_and(|max_ticks| metrics.session_ticks >= max_ticks)
            {
                begin_session_quiescence!("tick_limit");
            }
            metrics.session_ticks = metrics.session_ticks.saturating_add(1);
        }

        
}
