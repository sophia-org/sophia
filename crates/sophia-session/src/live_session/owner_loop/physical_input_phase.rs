{
macro_rules! drain_physical_input {
    ($routing_mode:expr) => {{
        synchronize_wm_pointer_epoch!();
        let emergency_exit = false;
        let lease_updates = drain_application_route_lease_updates(
            route_lease_update_receiver,
            &mut application_route_leases,
        );
        if lease_updates.confirmed != 0
            || lease_updates.rejected != 0
            || lease_updates.released != 0
            || lease_updates.stale != 0
        {
            crate::session_println!(
                "sophia_live_input_lease schema=1 confirmed={} rejected={} released={} stale={}",
                lease_updates.confirmed,
                lease_updates.rejected,
                lease_updates.released,
                lease_updates.stale,
            );
        }
        if let sophia_engine::ApplicationRouteLeaseTimeout::Quarantine(lease) =
            application_route_leases.observe_timeout(
                seat,
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            )
        {
            frontend_service_sender.try_send(XServerFrontendServiceCommand::RevokeAdmission {
                admission: lease.admission,
            })?;
            crate::session_eprintln!(
                "sophia_live_input_lease schema=1 status=quarantined reason=release_timeout admission={}",
                lease.admission.raw(),
            );
        }
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
                for target in report.content_activations.iter().cloned() {
                    if shell.issue_content_activation(target)?.is_none() {
                        crate::session_eprintln!(
                            "sophia_live_shell_content schema=1 status=input_rejected reason=capacity"
                        );
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
                        let wm = wm_session
                            .as_mut()
                            .ok_or("pointer focus requested without a live WM session")?;
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
                        wm.enqueue_command_shortcut(action, &mut session_launches, secondary_children.len())?;
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
let primary_refresh_millihz = native_scanout
    .as_ref()
    .and_then(|native| native.heads.first())
    .map_or(60_000, |head| head.refresh_millihz)
    .max(1);
let mut primary_frame_interval = Duration::from_micros(
    (1_000_000_000_u64 / u64::from(primary_refresh_millihz)).max(1),
);
let mut primary_frame_pacer = sophia_engine::PrimaryFramePacer::new(primary_frame_interval);
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
            .and_then(|native| native.heads.first())
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
                log_cpu_surface_sample(&scene, runtime.committed_surfaces(), surface_samples);
            }
        }
        if resource_sampler.is_due(sample_now) {
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
        if let Some(shell) = metadata_shell.as_mut() {
            shell.observe_outputs(&outputs)?;
            let reference_was_active=shell.reference_busy();
            let mut revoke_shell_input = false;
            match shell.poll() {
                Ok(LiveMetadataShellPoll::Healthy) => {}
                Ok(LiveMetadataShellPoll::Reconnected { .. }) => {
                    revoke_shell_input = true;
                }
                Ok(LiveMetadataShellPoll::Unavailable) => {
                    revoke_shell_input = true;
                }
                Err(error) => {
                    crate::session_eprintln!(
                        "sophia_live_metadata_shell schema=1 status=transport_failed stage=poll reason={error}"
                    );
                    shell.recover_transport("poll_failure")?;
                    revoke_shell_input = true;
                }
            }
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
                        && let Err(error)=runtime.set_descriptor_overlay(overlay,&scene,native_scanout.as_mut()) {
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
                    .filter_map(|projection| projection.content.clone())
                    .collect::<Vec<_>>();
                if let Err(error) = shell.service_content_actions(&presented_content) {
                    crate::session_eprintln!(
                        "sophia_live_shell_content schema=1 status=action_service_failed reason={error}"
                    );
                    shell.recover_transport("content_action_failure")?;
                    revoke_shell_input = true;
                }
                match shell.take_indicator_activation() {
                    Ok(Some(request)) => {
                        use sophia_protocol::ShellIndicatorActivationStatus as Status;
                        let mut status = request.status;
                        let mut reason = 0_u16;
                        if status == Status::Accepted
                            && !shell.content_indicator_admitted_by_ledger(&request.activation)
                        {
                            status = Status::Stale;
                            reason = sophia_protocol::ContentReason::Stale as u16;
                        }
                        if status == Status::Accepted {
                            let action = sophia_protocol::WmActionId::from_raw(request.activation.action);
                            let admission = wm_session.as_mut().map_or(
                                Ok(LiveWmRequestAdmission::Duplicate),
                                |wm| wm.enqueue_indicator_action(action, request.activation.output),
                            )?;
                            match admission {
                                LiveWmRequestAdmission::Admitted => {
                                    shell.content_indicator_admitted(request.activation.event_id);
                                }
                                LiveWmRequestAdmission::RejectedCapacity => {
                                    shell.content_indicator_rejected(request.activation.event_id);
                                    status = Status::Unknown;
                                    reason = sophia_protocol::ContentReason::Budget as u16;
                                }
                                LiveWmRequestAdmission::Duplicate => {
                                    shell.content_indicator_rejected(request.activation.event_id);
                                    status = Status::Stale;
                                    reason = sophia_protocol::ContentReason::Stale as u16;
                                }
                            }
                        }
                        shell.finish_indicator_activation(request, status, reason)?;
                    }
                    Ok(None)=>{},
                    Err(error)=>{
                        crate::session_eprintln!("sophia_shell_indicators status=activation_failed error={error}");
                        shell.recover_transport("indicator_activation_failure")?;revoke_shell_input=true;
                    }
                }
                match shell.service_tabs(publication,broker,runtime,&scene,native_scanout.as_mut()) {
                    Ok(focus)=>for(surface,output) in focus {
                        if let (Some(wm),Some(output))=(wm_session.as_mut(),outputs.iter().find(|o|o.id==output).copied()) {
                            wm.enqueue_tab_focus(surface,output.id)?;
                        }
                    },
                    Err(error)=>{crate::session_eprintln!("sophia_tabs status=unavailable error={error}");shell.recover_transport("tab_failure")?;revoke_shell_input=true;}
                }
            }
            if let Some(runtime)=runtime.as_mut() {
                if let Err(error)=shell.service_launcher(config,xauthority,&mut session_launches,secondary_children,&mut launch_admission_started_at,runtime,&scene,native_scanout.as_mut()){
                    crate::session_eprintln!("sophia_launcher status=unavailable error={error}");
                    shell.cancel_launcher()?;
                    shell.recover_transport("launcher_failure")?;
                    runtime.set_descriptor_overlay(None,&scene,native_scanout.as_mut())?;
                    revoke_shell_input=true;
                }
                shell.update_launcher_capture(&mut launcher_capture);
                if launcher_capture.active(){key_repeat.cancel_all();}
                let shortcuts=wm_session.as_ref().and_then(LiveWmSession::reference_shortcuts);
                let reference_output=wm_session.as_ref().and_then(LiveWmSession::reference_output).unwrap_or(output.id);
                if let Err(error)=shell.service_reference(shortcuts,reference_output,runtime,&scene,native_scanout.as_mut()) {
                    crate::session_eprintln!("sophia_reference status=unavailable error={error}");
                    shell.recover_transport("reference_failure")?;
                    runtime.set_descriptor_overlay(None,&scene,native_scanout.as_mut())?;
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
                    &scene,
                    native_scanout.as_mut(),
                    &outputs,
                    &output_bounds,
                    root,
                ) {
                    crate::session_eprintln!(
                        "sophia_live_shell_content schema=1 status=transport_failed reason={error}"
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
            if revoke_shell_input {
                launcher_capture.present(None,0,&[],true);
                reference_capture.present(None);
                shell.revoke_interaction();
                descriptor_captures.cancel_all();
                content_captures.revoke_targets();
                if let Some(runtime) = runtime.as_mut() {
                    runtime.revoke_descriptor_overlay_interaction();
                    if reference_was_active {runtime.set_descriptor_overlay(None,&scene,native_scanout.as_mut())?;}
                }
            }
            shell_work_area_bands = Some(shell.work_area_bands());
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
