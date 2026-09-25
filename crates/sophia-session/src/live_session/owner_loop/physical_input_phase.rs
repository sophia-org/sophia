{
macro_rules! drain_physical_input {
    ($routing_mode:expr, $routed_input_coalescer:expr, $repaint_due:expr,
     $motion_held_since:expr, $frame_interval:expr) => {{
        synchronize_wm_pointer_epoch!();
        if let Some(components) = shell_components.as_mut() {
            component_service::synchronize_native_capture(components, &mut launcher_capture, &mut launcher_keyboard)?;
            if launcher_capture.active() { key_repeat.cancel_all(); }
        }
        let emergency_exit = false;
        service_application_route_leases(route_lease_update_receiver, &mut application_route_leases, seat, started, frontend_service_sender)?;
        if let Some(poller) = physical_input.as_mut() {
            let empty_committed = [];
            let committed_surfaces = runtime
                .as_ref()
                .map_or(&empty_committed[..], |runtime| runtime.committed_surfaces());
            let empty_layers = [];
            let input_output = runtime.as_ref().and_then(|runtime| runtime.input_output());
            let input_presentation_epoch = runtime
                .as_ref()
                .map_or(0, |runtime| runtime.input_presentation_epoch());
            let input_layers = runtime
                .as_ref()
                .map_or(&empty_layers[..], |runtime| runtime.input_layers());
            let empty_projections = [];
            let input_projections = runtime.as_ref().map_or(
                &empty_projections[..],
                |runtime| runtime.input_projections(),
            );
            // Read before the context borrows `wm_session` mutably for
            // shortcuts; this only asks whether one exists.
            let pointer_focus_policy_available = wm_session.is_some();
            let report = route_physical_input(
                poller,
                PhysicalInputRoutingContext {
                    focus: &focus,
                    committed_surfaces,
                    input_layers,
                    input_projections,
                    pointer_outputs: &outputs,
                    surface_roles: &layout.presentation_roles,
                    client_routes: &layout.client_routes,
                    shortcuts: wm_session
                        .as_mut()
                        .and_then(|wm_session| wm_session.shortcuts.as_mut()),
                    input_sender,
                    modifiers: &mut modifiers,
                    key_repeat: &mut key_repeat,
                    key_repeat_map: &key_repeat_map,
                    client_keys: &mut client_keys,
                    emergency_chord: &mut emergency_chord,
                    virtual_terminal_chord: &mut virtual_terminal_chord,
                    keyboard_coverage: &mut keyboard_coverage,
                    pointer: &mut pointer,
                    pointer_routing_enabled: !config.expect_physical_pointer
                        || pointer_checksum.is_some(),
                    pointer_proof_required: crate::input_proof::pointer_selection_pending(
                        config.expect_physical_pointer,
                        metrics.physical_pointer_buttons_routed,
                    ),
                    pointer_buttons_only: false,
                    routing_mode: $routing_mode,
                    next_input_delivery: &mut input_delivery.next,
                    now_msec: u64::try_from(started.elapsed().as_millis())
                        .unwrap_or(u64::MAX),
                    physical_text_proof: physical_text_proof.as_mut(),
                    keyboard_focus_handoff: &mut keyboard_focus_handoff,
                    pointer_focus_handoff: &mut pointer_focus_handoff,
                    pointer_focus_policy_available,
                    applied_client_focus,
                    floating_gesture: &mut floating_pointer_gesture,
                    application_route_leases: &mut application_route_leases,
                    pending_lease_input: &mut pending_lease_input,
                    chrome_captures: &mut chrome_captures,
                    descriptor_captures: &mut descriptor_captures,
                    content_captures: &mut content_captures,
                    reference_capture: &mut reference_capture,
                    launcher_capture: &mut launcher_capture,
                    launcher_keyboard: &mut launcher_keyboard,
                    route_lease_release_sender,
                    input_output,
                    input_presentation_epoch,
                    routed_input_coalescer: $routed_input_coalescer,
                    repaint_due: $repaint_due,
                    motion_held_since: $motion_held_since,
                    frame_interval: $frame_interval,
                },
            )?;
            routed_input_saturation.merge(report.ingress_saturation);
            let event_timings = poller.drain_event_timings();
            // Acquisition saturation costs events rather than the session, so
            // it has to be audible. The count is cumulative, which is what lets
            // one replaceable slot carry every occurrence since the last tick.
            if let Some(saturation) = poller.take_acquisition_saturation() {
                print_capacity_saturation(&saturation);
            }
            if report.keyboard_focus_handoff_expired
                || report.keyboard_focus_handoff_stale_drops != 0
                || report.keyboard_focus_handoff_capacity_drops != 0
            {
                deferred_physical_key_timings.clear();
            }
            if physical_input_ready_at.is_some() && input_proof_started_at.is_none() {
                let mut rejects = PhysicalKeyTimingRejects::default();
                for (serial, event_time_msec) in &report.deferred_key_presses {
                    // An absent sidecar is a lost measurement, not a lost key:
                    // the event itself was already routed. Consuming it keeps a
                    // diagnostic from being able to end the session.
                    let Some(timing) = event_timings
                        .iter()
                        .find(|timing| timing.serial == *serial)
                        .copied()
                    else {
                        rejects.absent = rejects.absent.saturating_add(1);
                        continue;
                    };
                    // A sidecar that disagrees with its event is different in
                    // kind. It means the serial-to-timing association is wrong,
                    // which would make every latency number untrustworthy, so
                    // this one stays fatal.
                    if timing.event_time_msec != *event_time_msec {
                        return Err(
                            "deferred physical key timing sidecar did not match event".into()
                        );
                    }
                    if deferred_physical_key_timings.len()
                        >= sophia_engine::KEYBOARD_FOCUS_HANDOFF_CAPACITY
                        && !deferred_physical_key_timings.contains_key(serial)
                    {
                        rejects.overflow = rejects.overflow.saturating_add(1);
                        continue;
                    }
                    deferred_physical_key_timings.insert(*serial, timing);
                }
                if !rejects.is_empty() {
                    rejects.report(deferred_physical_key_timings.len());
                }
            }
            // Every routed press is a latency sample, not only the one the
            // proof latched. The proof needs one correlation; a percentile
            // needs a population, and the presses are already timestamped.
            if physical_input_ready_at.is_some() {
                for (serial, event_time_msec) in report.routed_key_presses.iter().copied() {
                    let Some(timing) = event_timings
                        .iter()
                        .find(|timing| timing.serial == serial)
                        .copied()
                    else {
                        continue;
                    };
                    let Some(ingress_ust_usec) = event_time_msec.checked_mul(1_000) else {
                        continue;
                    };
                    input_latency_samples.observe_press(
                        crate::input_latency_samples::PendingInputLatencySample {
                            serial,
                            ingress_ust_usec,
                            baseline_submission: native_scanout
                                .as_ref()
                                .and_then(|native| native.heads.first())
                                .map_or(0, |head| head.presented_submissions),
                            baseline_frame: native_scanout
                                .as_ref()
                                .and_then(|native| native.heads.first())
                                .map_or(0, |head| {
                                    newest_head_composition_frame(
                                        [
                                            head.pending_content,
                                            head.rendering_content,
                                            head.submitted_content,
                                            head.presented_content,
                                        ]
                                        .map(|content| {
                                            content.map(|content| content.frame().raw())
                                        }),
                                    )
                                }),
                            queue_dwell_usec: u64::try_from(timing.queue_dwell_msec)
                                .unwrap_or(u64::MAX)
                                .saturating_mul(1_000),
                        },
                    );
                }
            }
            if physical_input_ready_at.is_some()
                && input_proof_started_at.is_none()
                && let Some((serial, event_time_msec)) = report.routed_key_presses.last().copied()
                && let Some(timing) = event_timings
                    .iter()
                    .find(|timing| timing.serial == serial)
                    .copied()
                    .or_else(|| deferred_physical_key_timings.remove(&serial))
            {
                if timing.event_time_msec != event_time_msec {
                    return Err("physical input timing sidecar did not match routed event".into());
                }
                input_raw_ingress_msec = Some(event_time_msec);
                input_queue_dwell = Some(Duration::from_millis(
                    u64::try_from(timing.queue_dwell_msec).unwrap_or(u64::MAX),
                ));
                crate::session_println!(
                    "sophia_live_input_latency schema=1 status=ingress source=libinput_kernel event_serial={} ingress_msec={} queue_dwell_msec={}",
                    serial,
                    event_time_msec,
                    timing.queue_dwell_msec,
                );
                std::io::stdout().flush()?;
                deferred_physical_key_timings.clear();
            }
            metrics.physical_events = metrics.physical_events.saturating_add(report.events);
            metrics.physical_keys_routed = metrics
                .physical_keys_routed
                .saturating_add(report.keys_routed);
            metrics.physical_pointer_events = metrics
                .physical_pointer_events
                .saturating_add(report.pointer_events);
            metrics.physical_pointer_routed = metrics
                .physical_pointer_routed
                .saturating_add(report.pointer_routed);
            metrics.physical_pointer_buttons_routed = metrics
                .physical_pointer_buttons_routed
                .saturating_add(report.pointer_buttons_routed);
            if shell_proof_waiting_for_inert_click && report.pointer_buttons_observed != 0 {
                if !report.descriptor_activations.is_empty() {
                    return Err("retained shell pixels remained interactive after restart".into());
                }
                shell_proof_waiting_for_inert_click = false;
                crate::session_println!(
                    "sophia_live_metadata_shell schema=1 status=proof_inert_click observed=true activation=false"
                );
            }
            if report.pointer_focus_handoff_expired {
                crate::session_eprintln!(
                    "sophia_live_session_pointer schema=5 status=focus_handoff_dropped reason=timeout"
                );
            }
            if report.keyboard_focus_handoff_expired {
                crate::session_eprintln!(
                    "sophia_live_session_keyboard schema=1 status=focus_handoff_dropped reason=timeout"
                );
            }
            if report.keyboard_focus_handoff_stale_drops != 0 {
                crate::session_eprintln!(
                    "sophia_live_session_keyboard schema=1 status=focus_handoff_dropped reason=stale_target count={}",
                    report.keyboard_focus_handoff_stale_drops,
                );
            }
            if report.keyboard_focus_handoff_capacity_drops != 0 {
                crate::session_eprintln!(
                    "sophia_live_session_keyboard schema=1 status=focus_handoff_dropped reason=capacity count={}",
                    report.keyboard_focus_handoff_capacity_drops,
                );
            }
            if let Some((surface, count)) = report.keyboard_focus_handoff_released {
                crate::session_println!(
                    "sophia_live_session_keyboard schema=1 status=focus_handoff_released surface={} count={count}",
                    surface.index(),
                );
            }
            if report.pointer_focus_handoff_stale_drops != 0 {
                crate::session_eprintln!(
                    "sophia_live_session_pointer schema=5 status=focus_handoff_dropped reason=stale_target count={}",
                    report.pointer_focus_handoff_stale_drops,
                );
            }
            if report.pointer_focus_handoff_capacity_drops != 0 {
                crate::session_eprintln!(
                    "sophia_live_session_pointer schema=5 status=focus_handoff_dropped reason=capacity count={}",
                    report.pointer_focus_handoff_capacity_drops,
                );
            }
            if let Some((surface, count)) = report.pointer_focus_handoff_released {
                crate::session_println!(
                    "sophia_live_session_pointer schema=5 status=focus_handoff_released surface={} count={count}",
                    surface.index(),
                );
                input_observations.pointer_focus_target = Some(surface);
                input_observations.pointer_focus_key_routed = false;
            }
            if !input_observations.pointer_focus_key_routed
                && let Some(surface) = input_observations.pointer_focus_target
                && report.key_targets.contains(&surface)
            {
                crate::session_println!(
                    "sophia_live_session_pointer schema=6 status=focused_key_routed surface={}",
                    surface.index(),
                );
                input_observations.pointer_focus_key_routed = true;
            }
            input_delivery.events_expected = input_delivery
                .events_expected
                .saturating_add(report.deliveries.len());
            input_delivery.track(input_sender, report.deliveries.iter().copied(), false)?;
            // A departed device's releases are releases the seat owes, so
            // they are tracked the way a flush is: behind the release barrier.
            input_delivery.events_expected = input_delivery
                .events_expected
                .saturating_add(report.device_release_deliveries.len());
            input_delivery.track(
                input_sender,
                report.device_release_deliveries.iter().copied(),
                true,
            )?;
            client_key_release_barrier.extend(report.device_release_deliveries.iter().copied());
            announce_device_lifecycle(
                &report,
                poller.policy_report().udev_managed,
                &mut input_observations.devices_keyed,
            )?;
            let repeat_report = route_due_key_repeat_with_saturation(
                &mut key_repeat,
                seat,
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                $routing_mode,
                &focus,
                committed_surfaces,
                &client_keys,
                input_sender,
                &mut routed_input_saturation,
                &mut input_delivery.next,
            )?;
            metrics.key_repeats_routed = metrics
                .key_repeats_routed
                .saturating_add(repeat_report.routed);
            input_delivery.events_expected = input_delivery
                .events_expected
                .saturating_add(usize::from(repeat_report.delivery.is_some()));
            if let Some(delivery) = repeat_report.delivery {
                input_delivery.track(input_sender, [delivery], false)?;
            }
            match report.floating_outline {
                FloatingPointerOutlineUpdate::Unchanged => {}
                FloatingPointerOutlineUpdate::Set(outline) => {
                    let outline = clamp_floating_pointer_outline(
                        outline,
                        &wm_output_bounds(&outputs),
                    )
                    .ok_or("floating outline started outside every Engine output")?;
                    if let Some(runtime) = runtime.as_mut()
                        && runtime.set_floating_outline(
                            Some(sophia_backend_live::LiveFloatingOutline {
                                surface: outline.surface,
                                geometry: outline.geometry,
                            }),
                            &scene,
                            native_scanout.as_mut(),
                        )?
                    {
                        crate::session_println!(
                            "sophia_live_wm_pointer schema=1 status=outline_presented surface={} geometry={}x{}_{}_{}",
                            outline.surface.index(),
                            outline.geometry.width,
                            outline.geometry.height,
                            outline.geometry.x,
                            outline.geometry.y,
                        );
                    }
                }
                FloatingPointerOutlineUpdate::Clear => {
                    if let Some(runtime) = runtime.as_mut()
                        && runtime.set_floating_outline(
                            None,
                            &scene,
                            native_scanout.as_mut(),
                        )?
                    {
                        crate::session_println!(
                            "sophia_live_wm_pointer schema=1 status=outline_retired atomic_request=true"
                        );
                    }
                }
            }
            if !report.deliveries.is_empty() && input_proof_started_at.is_some() {
                input_delivery
                    .wait_started_at
                    .get_or_insert_with(Instant::now);
            }
            let pointer_motions_observed = report
                .pointer_events
                .saturating_sub(report.pointer_buttons_observed)
                .saturating_sub(report.pointer_axes_observed);
            for (status, contacts) in [
                (
                    "output_edge_confined",
                    &report.pointer_boundary_entries,
                ),
                (
                    "edge_reverse_immediate",
                    &report.pointer_boundary_reversals,
                ),
            ] {
                for (contact, output_index) in contacts {
                    for (axis, side) in [
                        ("horizontal", contact.horizontal),
                        ("vertical", contact.vertical),
                    ] {
                        let Some(side) = side else {
                            continue;
                        };
                        let side = match side {
                            sophia_engine::PointerBoundarySide::Minimum => "minimum",
                            sophia_engine::PointerBoundarySide::Maximum => "maximum",
                        };
                        crate::session_println!(
                            "sophia_live_session_pointer schema=7 status={status} axis={axis} side={side}"
                        );
                        if let Some(output_slot) = output_index {
                            crate::session_println!(
                                "sophia_live_session_pointer schema=8 status={status} axis={axis} side={side} output_slot={output_slot}"
                            );
                        }
                    }
                }
            }
            for (transition, boundary_free) in &report.pointer_output_transitions {
                let boundary = if *boundary_free { "free" } else { "projected" };
                crate::session_println!(
                    "sophia_live_session_pointer schema=8 status=output_transition from_slot={} to_slot={} boundary={boundary}",
                    transition.from, transition.to
                );
            }
            if !post_startup_exit_pointer_reported
                && config.normal_session
                && primary_child_exited
                && focus.focused_surface(seat).is_none()
                && wm_session.is_some()
                && pointer_motions_observed > 0
            {
                crate::session_println!(
                    "sophia_live_session_input_pipeline schema=1 status=desktop_pointer_active source=post_startup_exit"
                );
                std::io::stdout().flush()?;
                post_startup_exit_pointer_reported = true;
            }
            if pointer_motions_observed > 0 && pointer.position().is_some() {
                if cursor_updates.dirty {
                    metrics.cursor_moves_coalesced = metrics
                        .cursor_moves_coalesced
                        .saturating_add(pointer_motions_observed as u64);
                } else {
                    cursor_updates.dirty_since = Some(Instant::now());
                }
                cursor_updates.dirty = true;
            }
            if report.chrome_captures_started != 0
                || report.chrome_actions_activated != 0
                || report.chrome_captures_cancelled != 0
                || report.chrome_events_consumed != 0
            {
                crate::session_println!(
                    "sophia_live_indicator_input schema=1 status=batch captures={} activated={} cancelled={} consumed={}",
                    report.chrome_captures_started,
                    report.chrome_actions_activated,
                    report.chrome_captures_cancelled,
                    report.chrome_events_consumed,
                );
            }
            for (indicator_output, action) in report.chrome_activations.iter().copied() {
                crate::session_println!(
                    "sophia_live_indicator_input schema=1 status=activated output={} action={}",
                    indicator_output.raw(),
                    action.raw(),
                );
                let wm = wm_session
                    .as_mut()
                    .ok_or("indicator activated without a live WM session")?;
                let action_output = outputs
                    .iter()
                    .find(|output| output.id == indicator_output)
                    .copied()
                    .ok_or("indicator activation targets an unavailable output")?;
                match wm.enqueue_action(action, &layout, action_output)? {
                    LiveOrderedWmActionAdmission::Admitted => {
                        crate::session_println!(
                            "sophia_live_wm schema=1 status=physical_action_admitted action={}",
                            action.raw(),
                        );
                    }
                    LiveOrderedWmActionAdmission::RejectedCapacity { report } => {
                        if report {
                            crate::session_eprintln!(
                                "sophia_live_wm schema=2 status=request_rejected source=indicator reason=capacity action={}",
                                action.raw(),
                            );
                        }
                    }
                }
            }
            if let Some(shell) = metadata_shell.as_mut() {
                if let Some(runtime) = runtime.as_ref() {
                    for popout in report.content_dismissals.iter().cloned() {
                        shell.issue_content_dismissal(popout, runtime)?;
                    }
                }
                for target in report.content_activations.iter().cloned() {
                    if runtime.as_ref().is_none() || shell.issue_content_activation(
                        target, runtime.as_ref().expect("runtime was checked above")
                    )?.is_none() {
                        crate::session_eprintln!(
                            "sophia_live_shell_content schema=1 status=input_rejected reason=capacity"
                        );
                    }
                }
            }
            if let (Some(components), Some(runtime)) = (shell_components.as_mut(), runtime.as_ref()) {
                for target in report.content_activations.iter().cloned() {
                    if component_service::issue_component_activation(components, target, runtime, component_catalog)?.is_none() {
                        crate::session_eprintln!("sophia_shell_component schema=1 status=input_rejected reason=inactive_or_capacity");
                    }
                }
            }
            for (action, activation) in report.descriptor_activations.iter().copied() {
                let shell=metadata_shell.as_mut().ok_or("descriptor activation has no shell")?;
                let result=if shell.is_tab_action(action){shell.queue_tab_action(action,activation)}else{shell.dispatch_activation(action,activation)};
                if let Err(error)=result {
                    crate::session_eprintln!("sophia_live_metadata_shell schema=1 status=transport_failed stage=activation reason={error}");
                    shell.recover_transport("activation_failure")?;
                }
                if !shell.is_tab_action(action) {
                    shell.revoke_interaction();descriptor_captures.cancel_all();
                    if let Some(runtime)=runtime.as_mut(){runtime.revoke_descriptor_overlay_interaction();}
                }
            }
            if let Some(components) = shell_components.as_mut() {
                for event in &report.launcher_events {
                    component_service::dispatch_native_input(components, component_catalog, event)?;
                }
            }
            if let Some(shell)=metadata_shell.as_mut() {
                for event in &report.launcher_events {shell.launcher_input(event)?;}
                for (output,epoch,operation) in &report.reference_operations {
                    if shell.reference_input()==Some((*output,*epoch)) {shell.queue_reference(*operation,*output);}
                }
            }
            physical_policy_inputs.synchronize(wm_session.as_ref().and_then(|wm| wm.public.as_ref().map(|p| p.connection_epoch)));
            let hover_enabled = wm_session.as_ref().is_some_and(LiveWmSession::pointer_focus_enabled);
            for input in report.policy_inputs.iter().copied() {
                if !physical_policy_inputs.push(input, hover_enabled) {
                    crate::session_eprintln!("sophia_live_wm schema=4 status=input_rejected reason=capacity");
                }
            }
            while let Some(input) = physical_policy_inputs.next(wm_session.as_ref().is_some_and(LiveWmSession::pointer_focus_pending)) {
                let action = match input {
                    PhysicalPolicyInput::Hover(observation) => {
                        if let Some(wm) = wm_session.as_mut() {
                            wm.enqueue_pointer_focus(observation);
                        }
                        continue;
                    }
                    PhysicalPolicyInput::ClickFocus(surface) => {
                        // A SESSION WITHOUT A WM HAS NO FOCUS TO CHANGE. The
                        // hover arm above already reads the absence that way
                        // and continues; this arm made it fatal, which killed
                        // every session configured for a pointer proof with no
                        // window manager. That is the QEMU scenario exactly --
                        // it passes `--expect-physical-pointer` and starts no
                        // WM -- so the session announced `pointer status=ready
                        // action=select`, the harness sent the click it had
                        // just asked for, and the session died on the answer.
                        // Dropping the request costs the proof nothing: focus
                        // policy is not what a pointer proof measures, and the
                        // button still routes to the client through the
                        // authority, which is what moves the pixels it reads.
                        let Some(wm) = wm_session.as_mut() else {
                            crate::session_eprintln!(
                                "sophia_live_wm schema=3 status=request_rejected source=pointer_focus reason=no_wm_session surface={}",
                                surface.index(),
                            );
                            continue;
                        };
                        match wm.enqueue_focus(surface, &layout, output)? {
                            LiveWmRequestAdmission::Admitted => {
                                crate::session_println!(
                                    "sophia_live_wm schema=3 status=focus_requested source=pointer surface={}",
                                    surface.index(),
                                );
                            }
                            LiveWmRequestAdmission::Duplicate => {}
                            LiveWmRequestAdmission::RejectedCapacity => {
                                crate::session_eprintln!(
                                    "sophia_live_wm schema=3 status=request_rejected source=pointer_focus reason=capacity surface={}",
                                    surface.index(),
                                );
                            }
                        }
                        continue;
                    }
                    PhysicalPolicyInput::Action(action) => action,
                };

                if is_reserved_session_action(action)
                    && action != SHELL_HELP_SHORTCUT_ACTION
                    && !is_shell_switcher_shortcut(action)
                {
                    if let Some(wm) = wm_session.as_mut() {
                        wm.enqueue_command_shortcut(action, session_launches, secondary_children.len())?;
                    }
                    continue;
                }
                if action==SHELL_HELP_SHORTCUT_ACTION || is_shell_switcher_shortcut(action){
                    if let Some(shell)=metadata_shell.as_mut() && shell.launcher_busy(){
                        shell.cancel_launcher()?;launcher_capture.present(None,0,&[],true);
                        if let Some(runtime)=runtime.as_mut(){runtime.set_descriptor_overlay(None,&scene,native_scanout.as_mut())?;}
                    }
                }
                if action==SHELL_HELP_SHORTCUT_ACTION {
                    if let Some(shell)=metadata_shell.as_mut(){shell.queue_reference(sophia_protocol::ShellReferenceOperation::Toggle,wm_session.as_ref().and_then(LiveWmSession::reference_output).unwrap_or(output.id));}
                    continue;
                }
                if is_shell_switcher_shortcut(action) {
                    let broker = metadata_broker
                        .as_ref()
                        .ok_or("shell shortcut has no live metadata broker")?;
                    let shell = metadata_shell
                        .as_mut()
                        .ok_or("shell shortcut has no live metadata shell")?;
                    if shell.reference_busy() {
                        shell.cancel_reference()?;
                        reference_capture.present(None);
                        if let Some(runtime)=runtime.as_mut(){runtime.set_descriptor_overlay(None,&scene,native_scanout.as_mut())?;}
                    }
                    if shell.interaction_presented() {
                        crate::session_println!(
                            "sophia_live_metadata_shell schema=1 status=shortcut_consumed outcome=already_open"
                        );
                        continue;
                    }
                    let output_bounds = wm_output_bounds(&outputs);
                    let bounds = output_bounds
                        .iter()
                        .find(|(candidate, _)| *candidate == output.id)
                        .map(|(_, bounds)| *bounds)
                        .ok_or("shell shortcut has no output bounds")?;
                    let root = wm_root_bounds(&output_bounds)
                        .ok_or("shell shortcut has no root bounds")?;
                    let activation_surfaces = live_shell_activation_surfaces(
                        &layout.layers,
                        &layout.presentation_roles,
                    );
                    match shell.request_candidate(
                        broker,
                        output,
                        bounds,
                        root,
                        &output_bounds,
                        &activation_surfaces,
                    ) {
                        Ok(()) => (),
                        Err(error) => {
                            crate::session_eprintln!(
                                "sophia_live_metadata_shell schema=1 status=transport_failed stage=candidate reason={error}"
                            );
                            shell.recover_transport("candidate_failure")?;
                            shell.revoke_interaction();
                            descriptor_captures.cancel_all();
                            runtime
                                .as_mut()
                                .ok_or("shell shortcut has no visual runtime")?
                                .revoke_descriptor_overlay_interaction();
                            continue;
                        }
                    };
                    crate::session_println!(
                        "sophia_live_metadata_shell schema=1 status=shortcut_admitted action=descriptor_switcher"
                    );
                    continue;
                }
                let wm = wm_session
                    .as_mut()
                    .ok_or("WM shortcut activated without a live WM session")?;
                if wm.public.as_ref().is_none_or(|public| !public.configured) {
                    crate::session_println!("sophia_live_wm schema=2 status=physical_action_withheld reason=policy_replacement");
                    continue;
                }
                match wm.enqueue_action(action, &layout, output)? {
                    LiveOrderedWmActionAdmission::Admitted => {
                        crate::session_println!(
                            "sophia_live_wm schema=1 status=physical_action_admitted action={}",
                            action.raw(),
                        );
                    }
                    LiveOrderedWmActionAdmission::RejectedCapacity { report } => {
                        if report {
                            crate::session_eprintln!(
                                "sophia_live_wm schema=2 status=request_rejected source=action reason=capacity action={}",
                                action.raw(),
                            );
                        }
                    }
                }
            }
            for interaction in report.wm_pointer_interactions.iter().copied() {
                let wm = wm_session
                    .as_mut()
                    .ok_or("WM pointer interaction activated without a live WM session")?;
                match LivePhysicalWmActionDisposition::from(
                    wm.enqueue_pointer_interaction(interaction, &layout)?,
                ) {
                    LivePhysicalWmActionDisposition::Admitted => {
                        crate::session_println!(
                            "sophia_live_wm_pointer schema=2 status=interaction_admitted phase={:?} mode={:?} surface={}",
                            interaction.phase,
                            interaction.mode,
                            interaction.surface.index(),
                        );
                    }
                    LivePhysicalWmActionDisposition::RejectedCapacity => {
                        crate::session_eprintln!(
                            "sophia_live_wm_pointer schema=2 status=request_rejected reason=capacity phase={:?} surface={}",
                            interaction.phase,
                            interaction.surface.index(),
                        );
                    }
                    LivePhysicalWmActionDisposition::Coalesced => {}
                }
            }
            for gesture in report.wm_pointer_gestures.iter().copied() {
                let wm = wm_session
                    .as_mut()
                    .ok_or("WM pointer gesture activated without a live WM session")?;
                match LivePhysicalWmActionDisposition::from(wm.enqueue_pointer_gesture(
                    gesture,
                    &layout,
                )?) {
                    LivePhysicalWmActionDisposition::Admitted => {
                        crate::session_println!(
                            "sophia_live_wm_pointer schema=1 status=gesture_released atomic_request=true mode={:?} surface={} start_x={} start_y={} end_x={} end_y={}",
                            gesture.mode,
                            gesture.surface.index(),
                            gesture.start.x,
                            gesture.start.y,
                            gesture.end.x,
                            gesture.end.y,
                        );
                    }
                    LivePhysicalWmActionDisposition::RejectedCapacity => {
                        crate::session_eprintln!(
                            "sophia_live_wm_pointer schema=1 status=request_rejected reason=capacity surface={}",
                            gesture.surface.index(),
                        );
                    }
                    LivePhysicalWmActionDisposition::Coalesced => {}
                }
            }
            if let Some(terminal) = report.virtual_terminal {
                if pending_virtual_terminal.is_none() && requested_virtual_terminal.is_none() {
                    if let Some(surface) = applied_client_focus {
                        flush_client_keys!(surface, "virtual_terminal");
                    }
                    pending_virtual_terminal = Some((terminal, Instant::now()));
                    crate::session_println!(
                        "sophia_live_session_vt schema=5 status=queued target={terminal} trigger_keycode={} modifiers={:?} modifier_releases={}",
                        report.virtual_terminal_trigger_keycode.unwrap_or_default(),
                        report.virtual_terminal_modifier_keycodes,
                        report.virtual_terminal_modifier_releases,
                    );
                }
                std::io::stdout().flush()?;
            }
            if report.return_suppressed && !input_observations.return_suppressed {
                crate::session_println!("sophia_live_session_input_pipeline schema=1 status=return_suppressed");
                std::io::stdout().flush()?;
                input_observations.return_suppressed = true;
            }
            if !input_observations.key_observed && report.keys_observed > 0 {
                crate::session_println!("sophia_live_session_input_pipeline schema=1 status=key_observed");
                std::io::stdout().flush()?;
                input_observations.key_observed = true;
            }
            if !input_observations.key_routed && report.keys_routed > 0 {
                crate::session_println!("sophia_live_session_input_pipeline schema=1 status=key_routed");
                std::io::stdout().flush()?;
                input_observations.key_routed = true;
            }
            if !input_observations.key_suppressed_no_focus
                && report.keys_suppressed_no_focus > 0
            {
                crate::session_println!(
                    "sophia_live_session_input_pipeline schema=2 status=key_suppressed reason=no_focus"
                );
                std::io::stdout().flush()?;
                input_observations.key_suppressed_no_focus = true;
            }
            if report.emergency_exit {
                crate::session_println!("sophia_live_session_input_pipeline schema=1 status=emergency_exit");
                std::io::stdout().flush()?;
                emergency_exit_requested = true;
                flush_all_client_keys!("emergency");
                let requested_at = Instant::now();
                input_delivery.wait_started_at = Some(requested_at);
                input_delivery.source = Some("emergency");
            }
            if physical_sequence_completed_at.is_none()
                && physical_text_proof
                    .as_ref()
                    .is_some_and(|proof| proof.is_complete())
            {
                let completed_at = Instant::now();
                physical_sequence_completed_at = Some(completed_at);
                input_delivery.wait_started_at = Some(completed_at);
                input_delivery.source = Some("physical");
                if physical_input_pixels_already_changed(
                    injection_checksum,
                    scene.last_report().map(|report| report.checksum),
                    input_surface_pixel_change,
                ) {
                    input_pixel_change = true;
                }
            }
            if !input_observations.pointer_motion_observed
                && report.pointer_events
                    > report
                        .pointer_buttons_observed
                        .saturating_add(report.pointer_axes_observed)
            {
                crate::session_println!("sophia_live_session_pointer schema=2 status=motion_observed");
                input_observations.pointer_motion_observed = true;
            }
            if !input_observations.pointer_motion_routed
                && report.pointer_routed
                    > report
                        .pointer_buttons_routed
                        .saturating_add(report.pointer_axes_routed)
            {
                crate::session_println!("sophia_live_session_pointer schema=2 status=motion_routed");
                input_observations.pointer_motion_routed = true;
            }
            // First-use markers cannot explain a later unresponsive window.
            // Retain counts for button batches without coordinates or codes.
            if report.keys_observed > 0 || report.pointer_buttons_observed > 0 {
                crate::session_println!(
                    "sophia_live_session_input_routing schema=1 key_observed_count={} key_routed_count={} key_no_focus_count={} key_stale_focus_count={} wm_action_count={} pointer_button_count={} pointer_routed_count={} chrome_event_count={} lease_wait_count={} lease_rejected_count={}",
                    report.keys_observed,
                    report.keys_routed,
                    report.keys_suppressed_no_focus,
                    report.keys_suppressed_stale_focus,
                    report.wm_actions.len(),
                    report.pointer_buttons_observed,
                    report.pointer_routed,
                    report.chrome_events_consumed,
                    report.pointer_lease_waits,
                    report.pointer_lease_rejections,
                );
            }
            if report.pointer_buttons_observed > 0 {
                crate::session_println!(
                    "sophia_live_session_pointer_batch schema=1 observed_count={} routed_count={} suppressed_no_target_count={} suppressed_policy_count={}",
                    report.pointer_buttons_observed,
                    report.pointer_buttons_routed,
                    report.pointer_buttons_suppressed_no_target,
                    report.pointer_buttons_suppressed_by_policy,
                );
            }
            // Pointer evidence lives in owner_loop/pointer_evidence.rs. The
            // projection is re-read here rather than reused from the phase's
            // early borrow: the runtime is borrowed mutably in between, and
            // this is a short read after those borrows have ended.
            if !report.pointer_button_targets.is_empty() {
                let empty: &[sophia_protocol::LayerSnapshot] = &[];
                let (projection_layers, projection_count, projection_epoch) =
                    runtime.as_ref().map_or((empty, 0usize, 0u64), |runtime| {
                        (
                            runtime.input_layers(),
                            runtime.input_projections().len(),
                            runtime.input_presentation_epoch(),
                        )
                    });
                emit_pointer_evidence(
                    &report.pointer_button_targets,
                    &layout.presentation_roles,
                    projection_layers,
                    projection_count,
                    projection_epoch,
                    pointer.position(),
                );
            }
            if !input_observations.pointer_button_observed
                && report.pointer_buttons_observed > 0
            {
                crate::session_println!(
                    "sophia_live_session_pointer schema=2 status=button_observed count={}",
                    report.pointer_buttons_observed
                );
                input_observations.pointer_button_observed = true;
            }
            if report.pointer_buttons_suppressed_no_target > 0 {
                input_observations.pointer_buttons_suppressed_no_target = input_observations
                    .pointer_buttons_suppressed_no_target
                    .saturating_add(report.pointer_buttons_suppressed_no_target);
                crate::session_println!(
                    "sophia_live_session_pointer schema=8 status=button_suppressed reason=no_target count={} total={}",
                    report.pointer_buttons_suppressed_no_target,
                    input_observations.pointer_buttons_suppressed_no_target
                );
            }
            if report.pointer_buttons_suppressed_by_policy > 0 {
                crate::session_println!(
                    "sophia_live_session_pointer schema=8 status=button_suppressed reason=policy mode={} count={}",
                    physical_input_routing_mode_label($routing_mode),
                    report.pointer_buttons_suppressed_by_policy
                );
            }
            if !input_observations.pointer_button_routed && report.pointer_buttons_routed > 0 {
                crate::session_println!(
                    "sophia_live_session_pointer schema=2 status=button_routed count={}",
                    metrics.physical_pointer_buttons_routed
                );
                input_observations.pointer_button_routed = true;
            }
            if config.firefox_m10_dialog_proof && report.pointer_buttons_routed > 0 {
                crate::session_println!(
                    "sophia_firefox_dialog schema=1 status=pointer_batch routed={} total={} content=redacted",
                    report.pointer_buttons_routed,
                    metrics.physical_pointer_buttons_routed,
                );
            }
            if !input_observations.client_positioned_pointer_button_routed
                && report
                    .pointer_button_targets
                    .iter()
                    .copied()
                    .any(|surface| layout.is_client_positioned(surface))
            {
                crate::session_println!(
                    "sophia_live_session_pointer schema=4 status=target_routed role=client_positioned kind=button"
                );
                input_observations.client_positioned_pointer_button_routed = true;
            }
            if !input_observations.pointer_axis_observed && report.pointer_axes_observed > 0 {
                crate::session_println!("sophia_live_session_pointer schema=3 status=axis_observed");
                input_observations.pointer_axis_observed = true;
            }
            if !input_observations.pointer_axis_routed && report.pointer_axes_routed > 0 {
                crate::session_println!("sophia_live_session_pointer schema=3 status=axis_routed");
                input_observations.pointer_axis_routed = true;
            }
            if report.pointer_axes_observed > 0 || report.pointer_axes_routed > 0 {
                crate::session_println!(
                    "sophia_live_session_pointer schema=9 status=axis_batch observed={} routed={}",
                    report.pointer_axes_observed, report.pointer_axes_routed,
                );
            }
            if !input_observations.client_positioned_pointer_axis_routed
                && report
                    .pointer_axis_targets
                    .iter()
                    .copied()
                    .any(|surface| layout.is_client_positioned(surface))
            {
                crate::session_println!(
                    "sophia_live_session_pointer schema=4 status=target_routed role=client_positioned kind=axis"
                );
                input_observations.client_positioned_pointer_axis_routed = true;
            }
            if input_observations.pointer_motion_observed
                || input_observations.pointer_button_observed
                || input_observations.pointer_button_routed
                || input_observations.pointer_axis_observed
                || input_observations.pointer_axis_routed
            {
                std::io::stdout().flush()?;
            }
        }
        // A full ingress queue costs the records it could not take, and the
        // epoch close is what keeps that from leaving latched state in a
        // client. Closing it here rather than at the seven send sites keeps the
        // policy in one place, and keeps it outside the borrow that routing
        // holds on the runtime.
        if !routed_input_saturation.is_empty() {
            routed_input_saturation
                .report(input_sender.capacity(), &mut routed_input_saturation_ledger);
            routed_input_saturation = RoutedInputIngressSaturation::default();
            let revoked_input_leases = advance_application_input_security_epoch(
                &mut application_route_leases,
                input_sender,
                &layout.client_routes,
                route_lease_release_sender,
            )?;
            revoke_floating_pointer_interaction!("routed_input_saturation");
            revoke_chrome_captures!("routed_input_saturation");
            pointer_focus_handoff = PointerFocusHandoffState::default();
            keyboard_focus_handoff = KeyboardFocusHandoffState::default();
            // Flushing is what makes the close a terminating boundary rather
            // than an amnesty: every key the ledger still holds is released,
            // which both keeps clients from latching one down and lets the
            // ledger drain, so the next press is not refused for the same
            // reason forever.
            flush_all_client_keys!("routed_input_saturation");
            crate::session_println!(
                "sophia_live_input_epoch schema=1 reason=routed_input_saturation epoch={} revoked_leases={revoked_input_leases}",
                application_route_leases.control_epoch(),
            );
        }
        emergency_exit
    }};
}

include!("physical_input_loop.rs")
}
