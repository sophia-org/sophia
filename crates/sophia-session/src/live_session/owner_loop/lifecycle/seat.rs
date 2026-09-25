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
                                // A reopened seat announces its devices under new identities. What
                                // the old ones still held is released now, so no key outlives the
                                // device that pressed it.
                                flush_all_client_keys!("input_reopened");
                                keyboard_coverage.forget_all_devices();
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
                        flush_all_client_keys!("input_reopened");
                        keyboard_coverage.forget_all_devices();
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
                flush_all_client_keys!("input_reopened");
                keyboard_coverage.forget_all_devices();
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
                let _ = native_retirement.poll()?;
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            if seat_state == sophia_backend_live::LiveSeatState::ReleasePending {
                if seat_release_started.is_none() {
                    seat_release_started = Some(Instant::now());
                    crate::session_println!("sophia_live_seat schema=1 status=release_pending");
                }
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
                seat_release_prepared = true;
                // Poll before asking the broker: destruction queues the exact
                // lease close. Neither a pending owner nor a broker delay is
                // permission to discard custody or acknowledge early.
                let outcome = native_retirement.poll_seat_release(|| controller.acknowledge_disable())?;
                if !matches!(outcome, Some(sophia_backend_live::LiveSeatDisableOutcome::Acknowledged)) {
                    if seat_release_started.expect("release started").elapsed() >= Duration::from_secs(2) {
                        return Err(format!("seat release timed out: broker={outcome:?}").into());
                    }
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                seat_state = seat_state.released();
                seat_release_started = None;
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
                flush_all_client_keys!("input_reopened");
                keyboard_coverage.forget_all_devices();
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
