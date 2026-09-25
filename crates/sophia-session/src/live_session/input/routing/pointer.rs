{
                let is_button =
                    matches!(kind, sophia_protocol::InputEventKind::PointerButton { .. });
                let is_axis =
                    matches!(kind, sophia_protocol::InputEventKind::PointerAxis { .. });
                // Modal capture prevents application delivery, but the owner
                // still needs observed motion to update the visible cursor.
                report.pointer_events = report.pointer_events.saturating_add(1);
                if is_button {
                    report.pointer_buttons_observed =
                        report.pointer_buttons_observed.saturating_add(1);
                }
                if is_axis {
                    report.pointer_axes_observed =
                        report.pointer_axes_observed.saturating_add(1);
                }
                if !control_plane_applied && let Some((capture,_))=launcher.as_mut() {
                    if capture.active() && !capture.native_active() && matches!(kind,sophia_protocol::InputEventKind::PointerMotion){
                        let focused=focus.focused_surface(event.seat);
                        let (_, placement)=place_pointer_event_for_routing(&mut event,focused,input_layers,pointer,false);
                        record_pointer_boundary_placement(&mut report, kind, placement);
                    }
                    let(consumed,input)=capture.route(&event,None,pointer.position(),false,false);
                    report.launcher_events.extend(input);
                    if consumed{discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;}
                }
                if !control_plane_applied && let Some(capture)=reference_capture.as_deref_mut() {
                    let (consumed,operation)=capture.route(&event);
                    report.reference_operations.extend(operation);
                    if consumed {
                        if matches!(kind,sophia_protocol::InputEventKind::PointerMotion) {
                            let focused=focus.focused_surface(event.seat);
                            let (_, placement)=place_pointer_event_for_routing(&mut event,focused,input_layers,pointer,false);
                            record_pointer_boundary_placement(&mut report, kind, placement);
                        }
                        discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                    }
                }
                if matches!(
                    routing_mode,
                    PhysicalInputRoutingMode::Suppressed | PhysicalInputRoutingMode::ShortcutsOnly
                ) {
                    if is_button {
                        report.pointer_buttons_suppressed_by_policy = report
                            .pointer_buttons_suppressed_by_policy
                            .saturating_add(1);
                    }
                    discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                }
                if matches!(
                    routing_mode,
                    PhysicalInputRoutingMode::CursorOnly
                        | PhysicalInputRoutingMode::ControlPlaneOnly
                ) {
                    if is_button {
                        report.pointer_buttons_suppressed_by_policy = report
                            .pointer_buttons_suppressed_by_policy
                            .saturating_add(1);
                    }
                    if !is_button {
                        let focused_surface = focus.focused_surface(event.seat);
                        let (_, placement) = place_pointer_event_for_routing(
                            &mut event,
                            focused_surface,
                            input_layers,
                            pointer,
                            false,
                        );
                        record_pointer_boundary_placement(&mut report, kind, placement);
                    }
                    discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                }
                let focused_surface = focus.focused_surface(event.seat);
                let (route_event, placement) = place_pointer_event_for_routing(
                    &mut event,
                    focused_surface,
                    input_layers,
                    pointer,
                    pointer_buttons_only,
                );
                record_pointer_boundary_placement(&mut report, kind, placement);
                if !route_event {
                    discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                }
                let output_index = placement
                    .and_then(|placement| placement.output_index)
                    .or_else(|| pointer.output_index());
                let (
                    input_layers,
                    chrome_targets,
                    chrome_occlusion,
                    descriptor_targets,
                    descriptor_occlusion,
                    content_binding,
                    input_output,
                    input_presentation_epoch,
                ) =
                    input_projection_for_pointer(
                        input_projections,
                        pointer_outputs,
                        output_index,
                        input_layers,
                        input_output,
                        input_presentation_epoch,
                    );
                let native_focus = launcher.as_ref().and_then(|(capture, _)| capture.native_binding());
                let content_binding = if let Some(native) = native_focus {
                    content_binding.iter().find(|binding| binding.grant == native.grant
                        && binding.output == native.output
                        && binding.candidate_generation == native.candidate_generation
                        && binding.presentation_epoch == native.presentation_epoch
                        && binding.interaction_generation == native.interaction_generation)
                        .map(std::slice::from_ref).unwrap_or(&[])
                } else { content_binding };
                let descriptor_occlusion = descriptor_occlusion.or_else(|| {
                    input_projections.into_iter().flatten().filter(|p|Some(p.output)==input_output).flat_map(|p|p.tab_occlusions.iter()).find(|r| {
                        event.global_position.is_some_and(|p| p.x >= f64::from(r.x) && p.y >= f64::from(r.y) && p.x < f64::from(r.x)+f64::from(r.width) && p.y < f64::from(r.y)+f64::from(r.height) && !input_layers.iter().any(|l| p.x >= f64::from(l.geometry.x) && p.y >= f64::from(l.geometry.y) && p.x < f64::from(l.geometry.x)+f64::from(l.geometry.width) && p.y < f64::from(l.geometry.y)+f64::from(l.geometry.height)))
                    }).copied()
                });
                let application_owned = application_route_leases
                    .as_deref()
                    .and_then(|state| state.lease(event.seat))
                    .is_some()
                    || pointer_focus_handoff
                        .as_deref()
                        .and_then(PointerFocusHandoffState::target)
                    .is_some();
                if let Some(policy) = policy_presentation.as_mut()
                    && (native_focus.is_some()
                        || sophia_engine::content_binding_at_point(content_binding, event.global_position).is_some()
                        || [chrome_occlusion, descriptor_occlusion].into_iter().flatten().any(|rect|
                            event.global_position.is_some_and(|point| point.x >= f64::from(rect.x)
                                && point.y >= f64::from(rect.y)
                                && point.x < f64::from(rect.x) + f64::from(rect.width)
                                && point.y < f64::from(rect.y) + f64::from(rect.height))))
                {
                    policy.capture.revoke();
                }
                if pointer_routing_enabled && native_focus.is_none()
                    && let Some(state) = descriptor_captures.as_deref_mut()
                {
                    let disposition = sophia_engine::resolve_presented_chrome_pointer_event(
                        state,
                        event.seat,
                        event.device,
                        kind,
                        event.global_position,
                        input_output,
                        input_presentation_epoch,
                        descriptor_targets,
                        descriptor_occlusion,
                        application_owned,
                    )
                    .map_err(|error| {
                        format!("failed to route descriptor switcher input: {error:?}")
                    })?;
                    match disposition {
                        sophia_engine::PresentedChromePointerDisposition::Pass => {}
                        sophia_engine::PresentedChromePointerDisposition::Captured => {
                            report.chrome_captures_started =
                                report.chrome_captures_started.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                        sophia_engine::PresentedChromePointerDisposition::Activated {
                            action,
                            activation,
                        } => {
                            report.chrome_actions_activated =
                                report.chrome_actions_activated.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            report.descriptor_activations.push((action, activation));
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                        sophia_engine::PresentedChromePointerDisposition::Cancelled => {
                            report.chrome_captures_cancelled =
                                report.chrome_captures_cancelled.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                        sophia_engine::PresentedChromePointerDisposition::Consumed => {
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                    }
                }
                if pointer_routing_enabled && native_focus.is_none()
                    && let Some(state) = chrome_captures.as_deref_mut()
                {
                    let disposition = sophia_engine::resolve_chrome_pointer_event(
                        state,
                        event.seat,
                        event.device,
                        kind,
                        event.global_position,
                        input_output,
                        input_presentation_epoch,
                        chrome_targets,
                        chrome_occlusion,
                        application_owned,
                    )
                    .map_err(|error| format!("failed to route indicator input: {error:?}"))?;
                    match disposition {
                        sophia_engine::ChromePointerDisposition::Pass => {}
                        sophia_engine::ChromePointerDisposition::Captured => {
                            report.chrome_captures_started =
                                report.chrome_captures_started.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                        sophia_engine::ChromePointerDisposition::Activated { output, action } => {
                            report.chrome_actions_activated =
                                report.chrome_actions_activated.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            report.chrome_activations.push((output, action));
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                        sophia_engine::ChromePointerDisposition::Cancelled => {
                            report.chrome_captures_cancelled =
                                report.chrome_captures_cancelled.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                        sophia_engine::ChromePointerDisposition::Consumed => {
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                    }
                }
                if pointer_routing_enabled
                    && let Some(state) = content_captures.as_deref_mut()
                {
                    let disposition = sophia_engine::resolve_content_pointer_stack(
                        state,
                        event.seat,
                        event.device,
                        kind,
                        event.global_position,
                        content_binding,
                        application_owned,
                    );
                    if let (sophia_protocol::InputEventKind::PointerButton { pressed, .. }, Some(binding)) = (kind, sophia_engine::content_binding_at_point(content_binding, event.global_position))
                        && !matches!(disposition, sophia_engine::ContentPointerDisposition::Pass)
                    {
                        let status = match &disposition {
                            sophia_engine::ContentPointerDisposition::Captured => "captured",
                            sophia_engine::ContentPointerDisposition::Activated(_) => "activated",
                            sophia_engine::ContentPointerDisposition::Cancelled => "cancelled",
                            _ => "consumed",
                        };
                        let reason = if matches!(disposition, sophia_engine::ContentPointerDisposition::Cancelled) { "target_continuity_lost" } else { "none" };
                        crate::session_println!(
                            "sophia_shell_pointer_binding schema=1 status={} reason={} pressed={} observed_output={} output_generation={} candidate={} presentation={} layout_generation={} authority_current={}",
                            status, reason, pressed, binding.output.id, binding.output.generation,
                            binding.candidate_generation, binding.presentation_epoch,
                            binding.transform.layout_generation, binding.authority_current,
                        );
                    }
                    match disposition {
                        sophia_engine::ContentPointerDisposition::Pass => {}
                        sophia_engine::ContentPointerDisposition::OutsideDismiss(popout) => {
                            report.content_dismissals.push(popout);
                            report.chrome_events_consumed = report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                        sophia_engine::ContentPointerDisposition::Activated(target) => {
                            report.content_activations.push(target);
                            report.chrome_actions_activated =
                                report.chrome_actions_activated.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                        sophia_engine::ContentPointerDisposition::Captured => {
                            report.chrome_captures_started =
                                report.chrome_captures_started.saturating_add(1);
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                        sophia_engine::ContentPointerDisposition::Cancelled
                        | sophia_engine::ContentPointerDisposition::Consumed => {
                            report.chrome_events_consumed =
                                report.chrome_events_consumed.saturating_add(1);
                            discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                        }
                    }
                }
                if !control_plane_applied && !application_owned && let Some((capture, _)) = launcher.as_mut() {
                    let (consumed, input) = capture.route_native_pointer_fallback(&event)?;
                    report.launcher_events.extend(input);
                    if consumed {
                        report.chrome_events_consumed = report.chrome_events_consumed.saturating_add(1);
                        discard_preempted_policy_release(&mut policy_presentation, event.seat, event.device, event.kind); continue;
                    }
                }
                if !control_plane_applied && let Some(policy) = policy_presentation.as_mut() {
                    let hit = policy.pointer_hit(input_output, event.global_position, input_projections);
                    let disposition = if let sophia_protocol::InputEventKind::PointerButton { button, pressed } = kind {
                        policy.capture.pointer(policy.state, event.seat, event.device, button, pressed, hit, application_owned)
                    } else if !application_owned && hit != sophia_engine::PolicyPointerHit::Pass {
                        sophia_engine::PolicyInputDisposition::Consumed
                    } else { sophia_engine::PolicyInputDisposition::Pass };
                    if record_policy_input(disposition, &mut report) { continue; }
                }
                if let Some(gesture) = floating_gesture.as_deref_mut() {
                    let position = event.global_position.map(|global| {
                        sophia_protocol::WmPointerPosition {
                            x: global.x.round() as i32,
                            y: global.y.round() as i32,
                        }
                    });
                    let super_held = shortcuts.as_deref().is_some_and(|shortcuts| {
                        shortcuts.modifier_mask(event.seat).bits
                            & sophia_protocol::WmModifierMask::SUPER
                            != 0
                    });
                    let route =
                        sophia_engine::hit_test_scene_surface_for_input(&event, input_layers);
                    let observation = observe_floating_pointer_gesture(
                        gesture,
                        kind,
                        position,
                        route.target_surface,
                        route
                            .target_surface
                            .and_then(|surface| surface_roles.get(&surface).copied()),
                        route.target_surface.and_then(|surface| {
                            input_layers
                                .iter()
                                .find(|layer| layer.surface == surface)
                                .map(|layer| layer.geometry)
                        }),
                        super_held,
                    );
                    if let Some(completed) = observation.completed {
                        report.wm_pointer_gestures.push(completed);
                    }
                    if let Some(interaction) = observation.interaction {
                        report.wm_pointer_interactions.push(interaction);
                    }
                    if observation.outline != FloatingPointerOutlineUpdate::Unchanged {
                        report.floating_outline = observation.outline;
                    }
                    if observation.consumed {
                        continue;
                    }
                }
                if !pointer_routing_enabled {
                    if is_button {
                        report.pointer_buttons_suppressed_by_policy = report
                            .pointer_buttons_suppressed_by_policy
                            .saturating_add(1);
                    }
                    continue;
                }
                // Capacity failure clears the entire held sequence. Suppress
                // the rest of this already-polled pointer batch as part of
                // the same atomic drop; fresh input resumes next owner turn.
                if report.pointer_focus_handoff_capacity_drops != 0 {
                    continue;
                }
                let pending_target = pointer_focus_handoff
                    .as_deref()
                    .and_then(PointerFocusHandoffState::target);
                let held_lease = application_route_leases
                    .as_deref()
                    .and_then(|state| state.lease(event.seat));
                let tab_occlusions = input_projections.into_iter().flatten()
                    .find(|projection| Some(projection.output) == input_output)
                    .map_or(&[][..], |projection| projection.tab_occlusions.as_slice());
                let route = if let Some(mut lease) = held_lease {
                    let Some(state) = application_route_leases.as_deref_mut() else { unreachable!() };
                    if state.routing_readiness(event.seat) == Some(sophia_engine::ApplicationRouteLeaseReadiness::Releasing) {
                        report.pointer_lease_waits += 1;
                        continue;
                    }
                    let scope = presented_application_scope(&event, input_layers, chrome_targets, chrome_occlusion,
                        descriptor_targets, descriptor_occlusion, tab_occlusions, client_routes);
                    let owner = client_routes.admission_for_surface(lease.target_surface);
                    let authorized_scope = scope.is_some_and(|scope| lease.scope.covers(scope))
                        && owner.is_some_and(|owner| owner.client_id == lease.admission
                            && owner.auth_provenance.session_generation == lease.authority_session_epoch)
                        && lease.identity.control_epoch == state.control_epoch()
                        && lease.initiating_device.is_none_or(|device| device == event.device);
                    let pinned = input_output.is_some_and(|output| match lease.binding() {
                        sophia_engine::ApplicationRouteLeaseBinding::Bound { output: bound, .. } => bound == output,
                        sophia_engine::ApplicationRouteLeaseBinding::AwaitingPresentation { .. } =>
                            state.pin_output(lease.identity, output).is_ok(),
                    });
                    if !authorized_scope || !pinned {
                        report.pointer_lease_rejections += 1;
                        record_application_lease_refusal(if !pinned { "output" } else { "outside_scope" });
                        if let Some(sender) = route_lease_release_sender {
                            if let Some(held) = pending_lease_input.as_deref_mut() {
                                cancel_application_lease(state, client_routes, sender, held, lease.identity, now_msec)?;
                            } else {
                                request_application_route_lease_release(state, client_routes, sender, event.seat, now_msec)?;
                            }
                        }
                        continue;
                    }
                    let output = input_output.expect("validated output");
                    lease = state.lease(event.seat).expect("pinned current lease");
                    if matches!(lease.binding(), sophia_engine::ApplicationRouteLeaseBinding::AwaitingPresentation { .. })
                        && sophia_engine::scene_contains_input_surface(input_layers, lease.target_surface)
                    {
                        lease = state.bind_presentation(lease.identity, output, input_presentation_epoch)
                            .map_err(|error| format!("failed to bind presented input: {error:?}"))?;
                    }
                    match state.routing_readiness(event.seat) {
                        Some(sophia_engine::ApplicationRouteLeaseReadiness::WaitForActivation
                            | sophia_engine::ApplicationRouteLeaseReadiness::WaitForPresentation) => {
                            report.pointer_lease_waits += 1;
                            if let Some(held) = pending_lease_input.as_deref_mut()
                                && let Err(error) = held.defer(lease, output, now_msec, event)
                                && let Some(sender) = route_lease_release_sender
                            {
                                record_application_lease_refusal(match error {
                                    HeldLeaseInputError::OutputChanged => "output",
                                    HeldLeaseInputError::Expired => "binding_timeout",
                                    HeldLeaseInputError::Capacity => "capacity",
                                });
                                cancel_application_lease(state, client_routes, sender, held, lease.identity, now_msec)?;
                                report.pointer_focus_handoff_capacity_drops += 1;
                            }
                            continue;
                        }
                        Some(sophia_engine::ApplicationRouteLeaseReadiness::ReadyForEvidenceValidation) => {}
                        _ => { report.pointer_lease_waits += 1; continue; }
                    }
                    if let Err(reason) = authorize_presented_lease(state, lease, &event, client_routes,
                        scope.expect("validated scope"), output, input_presentation_epoch, input_layers)
                    {
                        report.pointer_lease_rejections += 1;
                        record_application_lease_refusal(application_lease_refusal_reason(reason));
                        if let Some(sender) = route_lease_release_sender {
                            if let Some(held) = pending_lease_input.as_deref_mut() {
                                cancel_application_lease(state, client_routes, sender, held, lease.identity, now_msec)?;
                            } else {
                                request_application_route_lease_release(state, client_routes, sender, event.seat, now_msec)?;
                            }
                        }
                        continue;
                    }
                    // Owner_events: the surface under the pointer when it is the
                    // grabbing client's, the anchor otherwise. input/grab_routing.rs
                    // carries why naming the anchor alone was wrong.
                    let hit = sophia_engine::hit_test_scene_surface_for_input(&event, input_layers).target_surface;
                    let grabbed = grab_routed_surface(hit, lease.target_surface, lease.admission, |surface| {
                        client_routes.admission_for_surface(surface).map(|admission| admission.client_id)
                    });
                    sophia_engine::route_scene_surface_for_input(&event, input_layers, grabbed)
                } else if let Some(target) = pending_target {
                    sophia_engine::route_scene_surface_for_input(&event, input_layers, target)
                } else {
                    sophia_engine::hit_test_scene_surface_for_input(&event, input_layers)
                };
                if matches!(kind, sophia_protocol::InputEventKind::PointerMotion)
                    && held_lease.is_none() && pending_target.is_none()
                    && input_presentation_epoch != 0
                    && let (Some(output), Some(position)) = (input_output, event.global_position)
                {
                    let occluded = chrome_occlusion.into_iter().chain(descriptor_occlusion)
                        .chain(tab_occlusions.iter().copied()).any(|r| point_is_inside_rect(position, r));
                    let popup = route.target_surface.is_some_and(|surface|
                        surface_roles.get(&surface) == Some(&sophia_protocol::SurfacePresentationRole::ClientPositioned));
                    if !occluded && !popup {
                        report.policy_inputs.push(PhysicalPolicyInput::Hover(PresentedPointerFocus {
                            output, target: route.target_surface,
                        }));
                    }
                }
                if is_button && route.target_surface.is_none() {
                    report.pointer_buttons_suppressed_no_target = report
                        .pointer_buttons_suppressed_no_target
                        .saturating_add(1);
                }
                let (Some(global), Some(local)) = (event.global_position, route.local_position)
                else {
                    continue;
                };
                let Some(surface) = route.target_surface else {
                    continue;
                };
                let focus_surface = pointer_focus_surface(
                    surface,
                    global,
                    input_layers,
                    surface_roles,
                    client_routes,
                );
                let request = sophia_protocol::RoutedInputRequest {
                    serial: event.serial,
                    seat: event.seat,
                    device: event.device,
                    time_msec: event.time_msec,
                    target_surface: surface,
                    global_position: global,
                    local_position: local,
                    kind,
                };
                let starts_focus_handoff = held_lease.is_none() && pointer_press_starts_focus_handoff(
                    &kind,
                    applied_client_focus,
                    focus_surface,
                    surface_roles.get(&focus_surface).copied(),
                    pointer_focus_handoff
                        .as_deref()
                        .is_some_and(|handoff| handoff.target().is_none()),
                );
                // A click that begins or defers a focus handoff must land
                // after the motion that positioned the pointer, so buffered
                // motion is released before the handoff takes the event.
                let handoff_takes_event = starts_focus_handoff
                    || pointer_focus_handoff
                        .as_deref()
                        .is_some_and(|handoff| handoff.target().is_some());
                if handoff_takes_event
                    && let Some(flush) = routed_input_coalescer
                        .flush_barrier(sophia_engine::RoutedInputFlushReason::FocusChanged)
                {
                    *motion_held_since = None;
                    deliver_coalesced_inputs(
                        flush,
                        input_sender,
                        next_input_delivery,
                        application_route_leases.as_deref_mut(),
                        client_routes,
                        input_output,
                        input_presentation_epoch,
                        &mut report,
                    )?;
                }
                if let Some(handoff) = pointer_focus_handoff.as_deref_mut() {
                    if starts_focus_handoff {
                        handoff.begin(focus_surface, now_msec, request)?;
                        report.pointer_focus_targets.push(focus_surface);
                        report.policy_inputs.push(PhysicalPolicyInput::ClickFocus(focus_surface));
                        continue;
                    }
                    if handoff.target().is_some() {
                        if handoff.defer(request).is_err() {
                            report.pointer_focus_handoff_capacity_drops = report
                                .pointer_focus_handoff_capacity_drops
                                .saturating_add(1);
                        }
                        continue;
                    }
                }
                // Motion reaches the frontend at the composition cadence
                // rather than once per event. Every motion packet used to mint
                // its own delivery, route lease and ordered acknowledgement,
                // which kept authority work continuously available to the owner
                // loop and let a moving pointer hold composition below its rate.
                // The coalescer keeps the latest motion per target surface and
                // releases it on the frame boundary; anything that changes
                // state releases it first, so a click still lands after the
                // motion that positioned the pointer.
                //
                // The route is restated rather than forwarded so the packet
                // rebuilt at release is exactly the one this path would have
                // sent: the engine takes `global_position` from the event, and
                // `surface` and `local` are the values already unwrapped above.
                let coalesced_route = sophia_protocol::InputRoute {
                    input_serial: event.serial,
                    target_surface: Some(surface),
                    global_position: global,
                    local_position: Some(local),
                    transform: route.transform,
                    outcome: sophia_protocol::InputRouteOutcome::Routed,
                };
                match routed_input_coalescer.push(event.clone(), coalesced_route) {
                    sophia_engine::RoutedInputQueueAction::BufferedMotion => {
                        motion_held_since.get_or_insert_with(std::time::Instant::now);
                    }
                    sophia_engine::RoutedInputQueueAction::Flushed(flush) => {
                        *motion_held_since = None;
                        deliver_coalesced_inputs(
                            flush,
                            input_sender,
                            next_input_delivery,
                            application_route_leases.as_deref_mut(),
                            client_routes,
                            input_output,
                            input_presentation_epoch,
                            &mut report,
                        )?;
                    }
                }
            }
