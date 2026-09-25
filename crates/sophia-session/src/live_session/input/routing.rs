#[allow(clippy::too_many_arguments)]
fn route_input_events_with_launcher(
    events: Vec<sophia_protocol::InputEventPacket>,
    focus: &InputFocusState,
    committed_surfaces: &[CommittedSurfaceState],
    input_layers: &[LayerSnapshot],
    surface_roles: &BTreeMap<SurfaceId, sophia_protocol::SurfacePresentationRole>,
    client_routes: &XAuthorityClientSurfaceRoutes,
    input_sender: &impl RoutedInputIngress,
    modifiers: &mut XCoreKeyboardMapper,
    key_repeat: &mut KeyRepeatState,
    key_repeat_map: &XkbKeymapSnapshot,
    client_keys: &mut SessionClientKeyState,
    emergency_chord: &mut EmergencyChordState,
    virtual_terminal_chord: &mut VirtualTerminalChordState,
    keyboard_coverage: &mut PhysicalKeyboardCoverage,
    mut shortcuts: Option<&mut WmShortcutRouter>,
    pointer: &mut SessionPointerPlacement,
    pointer_routing_enabled: bool,
    pointer_proof_required: bool,
    pointer_buttons_only: bool,
    routing_mode: PhysicalInputRoutingMode,
    next_input_delivery: &mut u64,
    now_msec: u64,
    mut physical_text_proof: Option<&mut PhysicalTextProof>,
    mut keyboard_focus_handoff: Option<&mut KeyboardFocusHandoffState>,
    mut pointer_focus_handoff: Option<&mut PointerFocusHandoffState>,
    applied_client_focus: Option<SurfaceId>,
    mut floating_gesture: Option<&mut FloatingPointerGestureState>,
    mut application_route_leases: Option<&mut ApplicationRouteLeaseState>,
    mut chrome_captures: Option<&mut sophia_engine::ChromeCaptureState>,
    mut descriptor_captures: Option<&mut sophia_engine::PresentedChromeCaptureState>,
    mut content_captures: Option<&mut sophia_engine::ContentCaptureState>,
    route_lease_release_sender: Option<&SyncSender<XAuthorityRouteLeaseRelease>>,
    input_output: Option<sophia_protocol::OutputId>,
    input_presentation_epoch: u64,
    input_projections: Option<&[sophia_backend_live::LivePresentedInputProjection]>,
    pointer_outputs: Option<&[sophia_engine::HeadlessOutput]>,
    mut reference_capture: Option<&mut sophia_engine::ReferenceSheetCapture>,
    mut launcher: Option<(&mut sophia_engine::LauncherCapture, &mut sophia_engine::LauncherKeyboard)>,
    mut pending_lease_input: Option<&mut PendingLeaseInput>,
    routed_input_coalescer: &mut sophia_engine::RoutedInputCoalescer,
    repaint_due: bool,
    motion_held_since: &mut Option<std::time::Instant>,
    frame_interval: std::time::Duration,
) -> Result<PhysicalInputRouteReport, Box<dyn std::error::Error>> {
    let mut report = PhysicalInputRouteReport {
        ingress_saturation: RoutedInputIngressSaturation::default(),
        events: events.len(),
        wm_actions: Vec::new(),
        policy_inputs: Vec::new(),
        reference_operations: Vec::new(),
        launcher_events: Vec::new(),
        chrome_activations: Vec::new(),
        descriptor_activations: Vec::new(),
        content_activations: Vec::new(),
        content_dismissals: Vec::new(),
        chrome_captures_started: 0,
        chrome_actions_activated: 0,
        chrome_captures_cancelled: 0,
        chrome_events_consumed: 0,
        wm_pointer_gestures: Vec::new(),
        wm_pointer_interactions: Vec::new(),
        floating_outline: FloatingPointerOutlineUpdate::Unchanged,
        keys_observed: 0,
        keys_suppressed_no_focus: 0,
        keys_suppressed_stale_focus: 0,
        keys_routed: 0,
        key_targets: Vec::new(),
        routed_key_presses: Vec::new(),
        deferred_key_presses: Vec::new(),
        pointer_events: 0,
        pointer_buttons_observed: 0,
        pointer_buttons_suppressed_no_target: 0,
        pointer_buttons_suppressed_by_policy: 0,
        pointer_axes_observed: 0,
        pointer_routed: 0,
        pointer_buttons_routed: 0,
        pointer_lease_waits: 0,
        pointer_lease_rejections: 0,
        pointer_button_targets: Vec::new(),
        pointer_focus_targets: Vec::new(),
        pointer_axes_routed: 0,
        pointer_axis_targets: Vec::new(),
        deliveries: Vec::new(),
        emergency_exit: false,
        return_suppressed: false,
        virtual_terminal: None,
        virtual_terminal_trigger_keycode: None,
        virtual_terminal_modifier_keycodes: [None; 4],
        virtual_terminal_modifier_releases: 0,
        pointer_focus_handoff_expired: false,
        pointer_focus_handoff_stale_drops: 0,
        pointer_focus_handoff_capacity_drops: 0,
        pointer_focus_handoff_released: None,
        keyboard_focus_handoff_expired: false,
        keyboard_focus_handoff_stale_drops: 0,
        keyboard_focus_handoff_capacity_drops: 0,
        keyboard_focus_handoff_released: None,
        pointer_boundary_entries: Vec::new(),
        pointer_boundary_reversals: Vec::new(),
        pointer_output_transitions: Vec::new(),
        devices_added: Vec::new(),
        devices_removed: Vec::new(),
        devices_keyed: Vec::new(),
        device_release_deliveries: Vec::new(),
    };
    if routing_mode == PhysicalInputRoutingMode::Full
        && let (Some(held), Some(state), Some(projections), Some(release_sender)) = (
            pending_lease_input.as_deref_mut(), application_route_leases.as_deref_mut(), input_projections, route_lease_release_sender,
        )
    {
        flush_held_lease_input(held, state, client_routes, projections, input_sender, release_sender,
            next_input_delivery, now_msec, &mut report)?;
    }
    let mut routed_events = VecDeque::new();
    if let Some(handoff) = keyboard_focus_handoff.as_deref_mut() {
        if handoff.cancel_if_target_stale(|target| {
            committed_surfaces
                .iter()
                .any(|committed| committed.surface == target)
                && client_routes.client_for_surface(target).is_some()
        }) {
            report.keyboard_focus_handoff_stale_drops = 1;
        } else {
            report.keyboard_focus_handoff_expired = handoff.expire(now_msec);
        }
        if routing_mode == PhysicalInputRoutingMode::Full
            && let Some(mut ready) = handoff.take_ready(applied_client_focus)
        {
            let released_target = applied_client_focus;
            let released_count = ready.len();
            routed_events.extend(ready.drain(..).map(|event| (event, true)));
            report.keyboard_focus_handoff_released =
                released_target.map(|surface| (surface, released_count));
        }
    }
    routed_events.extend(events.into_iter().map(|event| (event, false)));
    if let Some(handoff) = pointer_focus_handoff.as_deref_mut() {
        if handoff.cancel_if_target_stale(|target| {
            let present = sophia_engine::scene_contains_input_surface(input_layers, target)
                || input_projections.is_some_and(|projections| {
                    projections.iter().any(|projection| {
                        sophia_engine::scene_contains_input_surface(&projection.layers, target)
                    })
                });
            present
                && client_routes.client_for_surface(target).is_some()
        }) {
            report.pointer_focus_handoff_stale_drops = 1;
        } else {
            report.pointer_focus_handoff_expired = handoff.expire(now_msec);
        }
        if let Some(mut ready) = handoff.take_ready(applied_client_focus) {
            let released_target = applied_client_focus;
            let released_count = ready.len();
            while let Some(request) = ready.pop_front() {
                let is_button = matches!(
                    request.kind,
                    sophia_protocol::InputEventKind::PointerButton { .. }
                );
                let is_axis = matches!(
                    request.kind,
                    sophia_protocol::InputEventKind::PointerAxis { .. }
                );
                let target = request.target_surface;
                let delivery = XAuthorityInputDeliveryId::from_raw(*next_input_delivery);
                *next_input_delivery = next_input_delivery
                    .checked_add(1)
                    .ok_or("live-session input delivery ID exhausted")?;
                let route_lease = match application_route_leases.as_deref_mut() {
                    Some(state) => application_route_lease_for_request(
                        &request,
                        client_routes,
                        state,
                        input_output,
                        input_presentation_epoch,
                    )?,
                    None => None,
                };
                if !route_bounded_input(
                    input_sender,
                    XAuthorityRoutedInput {
                        request,
                        route_lease,
                        delivery: Some(delivery),
                        mode: XAuthorityRoutedInputMode::Deliver,
                        origin: sophia_x_authority::XAuthorityRoutedInputOrigin::Physical,
                    },
                    sophia_protocol::CapacityClass::Ordered,
                    &mut report.ingress_saturation,
                )? {
                    continue;
                }
                report.pointer_routed = report.pointer_routed.saturating_add(1);
                if is_button {
                    report.pointer_buttons_routed =
                        report.pointer_buttons_routed.saturating_add(1);
                    report.pointer_button_targets.push(target);
                }
                if is_axis {
                    report.pointer_axes_routed = report.pointer_axes_routed.saturating_add(1);
                    report.pointer_axis_targets.push(target);
                }
                report.deliveries.push(delivery);
            }
            report.pointer_focus_handoff_released =
                released_target.map(|surface| (surface, released_count));
        }
    }
    for (mut event, control_plane_applied) in routed_events {
        match event.kind {
            sophia_protocol::InputEventKind::Key { keycode, pressed } => include!("routing/key.rs"),
            sophia_protocol::InputEventKind::DeviceAdded {
                keyboard,
                pointer,
                touch,
                virtual_bus,
            } => {
                report.devices_added.push(DeviceArrival {
                    device: event.device,
                    keyboard,
                    pointer,
                    touch,
                    virtual_bus,
                });
            }
            sophia_protocol::InputEventKind::DeviceRemoved => {
                let removal = release_departed_device(
                    event.device,
                    client_keys,
                    input_sender,
                    &mut report.ingress_saturation,
                    modifiers,
                    key_repeat,
                    virtual_terminal_chord,
                    emergency_chord,
                    keyboard_coverage,
                    next_input_delivery,
                    now_msec,
                    &mut report.device_release_deliveries,
                )?;
                report.devices_removed.push(removal);
            }
            kind @ (sophia_protocol::InputEventKind::PointerMotion
            | sophia_protocol::InputEventKind::PointerButton { .. }
            | sophia_protocol::InputEventKind::PointerAxis { .. }) => include!("routing/pointer.rs"),
        }
    }
    // The frame boundary, which caps motion delivery at the composition
    // cadence: a client sees the pointer's current position once per composed
    // frame instead of once per packet the device produced.
    //
    // The pacer alone cannot decide this. It reports a repaint due only when
    // one was *requested*, and a session composing from client Present
    // submissions asks for almost none -- one measured run took 870 frames and
    // requested two. Waiting only on the pacer buffered motion that was then
    // coalesced away, and the client received no pointer input at all. So the
    // wait is bounded by the frame interval as well: whatever drives
    // composition, motion is never held longer than the frame it belongs to.
    let motion_waited_a_frame = motion_held_since
        .is_some_and(|since| since.elapsed() >= frame_interval);
    if (repaint_due || motion_waited_a_frame)
        && let Some(flush) = routed_input_coalescer.flush_frame()
    {
        *motion_held_since = None;
        deliver_coalesced_inputs(
            flush,
            input_sender,
            next_input_delivery,
            application_route_leases,
            client_routes,
            input_output,
            input_presentation_epoch,
            &mut report,
        )?;
    }
    Ok(report)
}
