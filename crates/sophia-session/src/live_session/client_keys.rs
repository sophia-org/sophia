// Routing key repeats and flushing the keys a client is believed to hold.
//
// These share one concern: every record here is a key the compositor already
// told a client about, so losing one silently changes what that client thinks
// is held down. They live beside the routing loop rather than inside it because
// the loop file is already past its cohesion budget.

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct KeyRepeatRouteReport {
    routed: usize,
    delivery: Option<XAuthorityInputDeliveryId>,
    ingress_saturation: RoutedInputIngressSaturation,
}

#[allow(clippy::too_many_arguments)]
/// Routes a due key repeat and folds its ingress accounting into the session's,
/// so a saturated repeat is reported alongside every other lost record rather
/// than in a report of its own.
fn route_due_key_repeat_with_saturation(
    key_repeat: &mut KeyRepeatState,
    seat: SeatId,
    now_msec: u64,
    routing_mode: PhysicalInputRoutingMode,
    focus: &InputFocusState,
    committed_surfaces: &[CommittedSurfaceState],
    client_keys: &SessionClientKeyState,
    input_sender: &impl RoutedInputIngress,
    ingress_saturation: &mut RoutedInputIngressSaturation,
    next_input_delivery: &mut u64,
) -> Result<KeyRepeatRouteReport, Box<dyn std::error::Error>> {
    let report = route_due_key_repeat(
        key_repeat,
        seat,
        now_msec,
        routing_mode,
        focus,
        committed_surfaces,
        client_keys,
        input_sender,
        next_input_delivery,
    )?;
    ingress_saturation.merge(report.ingress_saturation);
    Ok(report)
}

fn route_due_key_repeat(
    key_repeat: &mut KeyRepeatState,
    seat: SeatId,
    now_msec: u64,
    routing_mode: PhysicalInputRoutingMode,
    focus: &InputFocusState,
    committed_surfaces: &[CommittedSurfaceState],
    client_keys: &SessionClientKeyState,
    input_sender: &impl RoutedInputIngress,
    next_input_delivery: &mut u64,
) -> Result<KeyRepeatRouteReport, Box<dyn std::error::Error>> {
    let mut report = KeyRepeatRouteReport::default();
    if routing_mode != PhysicalInputRoutingMode::Full {
        return Ok(report);
    }
    let Some(target) = key_repeat.active_target(seat) else {
        return Ok(report);
    };
    let pressed = SessionClientPressedKey {
        surface: target.surface,
        seat: target.seat,
        device: target.device,
        keycode: target.keycode,
    };
    let target_is_current = focus.focused_surface(seat) == Some(target.surface)
        && committed_surfaces
            .iter()
            .any(|committed| committed.surface == target.surface)
        && client_keys.is_pressed(pressed);
    if !target_is_current {
        key_repeat.cancel_seat(seat);
        return Ok(report);
    }
    let Some(pulse) = key_repeat.take_due(seat, now_msec) else {
        return Ok(report);
    };
    let delivery = XAuthorityInputDeliveryId::from_raw(*next_input_delivery);
    *next_input_delivery = next_input_delivery
        .checked_add(1)
        .ok_or("live-session input delivery ID exhausted")?;
    if route_bounded_input(
        input_sender,
        XAuthorityRoutedInput {
            request: sophia_protocol::RoutedInputRequest {
                serial: delivery.raw(),
                seat: pulse.target.seat,
                device: pulse.target.device,
                time_msec: pulse.time_msec,
                target_surface: pulse.target.surface,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: sophia_protocol::InputEventKind::Key {
                    keycode: pulse.target.keycode,
                    pressed: true,
                },
            },
            route_lease: None,
            delivery: Some(delivery),
            mode: XAuthorityRoutedInputMode::Repeat,
            origin: sophia_x_authority::XAuthorityRoutedInputOrigin::Physical,
        },
        sophia_protocol::CapacityClass::Ordered,
        &mut report.ingress_saturation,
    )? {
        report.routed = 1;
        report.delivery = Some(delivery);
    }
    Ok(report)
}

fn flush_client_pressed_keys(
    surface: SurfaceId,
    client_keys: &mut SessionClientKeyState,
    scratch: &mut Vec<SessionClientPressedKey>,
    deliveries: &mut Vec<XAuthorityInputDeliveryId>,
    input_sender: &impl RoutedInputIngress,
    ingress_saturation: &mut RoutedInputIngressSaturation,
    modifiers: &mut XCoreKeyboardMapper,
    next_input_delivery: &mut u64,
    time_msec: u64,
) -> Result<usize, Box<dyn std::error::Error>> {
    client_keys.copy_surface_keys(surface, scratch);
    flush_copied_client_pressed_keys(
        client_keys,
        scratch,
        deliveries,
        input_sender,
        ingress_saturation,
        modifiers,
        next_input_delivery,
        time_msec,
    )
}

#[allow(clippy::too_many_arguments)]
fn flush_all_client_pressed_keys(
    client_keys: &mut SessionClientKeyState,
    scratch: &mut Vec<SessionClientPressedKey>,
    deliveries: &mut Vec<XAuthorityInputDeliveryId>,
    input_sender: &impl RoutedInputIngress,
    ingress_saturation: &mut RoutedInputIngressSaturation,
    modifiers: &mut XCoreKeyboardMapper,
    next_input_delivery: &mut u64,
    time_msec: u64,
) -> Result<usize, Box<dyn std::error::Error>> {
    client_keys.copy_all_keys(scratch);
    flush_copied_client_pressed_keys(
        client_keys,
        scratch,
        deliveries,
        input_sender,
        ingress_saturation,
        modifiers,
        next_input_delivery,
        time_msec,
    )
}

#[allow(clippy::too_many_arguments)]
fn flush_copied_client_pressed_keys(
    client_keys: &mut SessionClientKeyState,
    scratch: &[SessionClientPressedKey],
    deliveries: &mut Vec<XAuthorityInputDeliveryId>,
    input_sender: &impl RoutedInputIngress,
    ingress_saturation: &mut RoutedInputIngressSaturation,
    modifiers: &mut XCoreKeyboardMapper,
    next_input_delivery: &mut u64,
    time_msec: u64,
) -> Result<usize, Box<dyn std::error::Error>> {
    deliveries.clear();
    for key in scratch.iter().copied() {
        let Some((_x_keycode, _state)) = modifiers.map_evdev_key(key.keycode, false) else {
            return Err("pressed-key ledger contains an unmappable key".into());
        };
        let delivery = XAuthorityInputDeliveryId::from_raw(*next_input_delivery);
        *next_input_delivery = next_input_delivery
            .checked_add(1)
            .ok_or("live-session input delivery ID exhausted")?;
        // Losing a release leaves the client believing the key is still down,
        // so leave it recorded as pressed and let the epoch close be the
        // barrier that clears it.
        if !route_bounded_input(
            input_sender,
            XAuthorityRoutedInput {
                request: sophia_protocol::RoutedInputRequest {
                    serial: delivery.raw(),
                    seat: key.seat,
                    device: key.device,
                    time_msec,
                    target_surface: key.surface,
                    global_position: Point::default(),
                    local_position: Point::default(),
                    kind: sophia_protocol::InputEventKind::Key {
                        keycode: key.keycode,
                        pressed: false,
                    },
                },
                route_lease: None,
                delivery: Some(delivery),
                mode: XAuthorityRoutedInputMode::Deliver,
                origin: sophia_x_authority::XAuthorityRoutedInputOrigin::Physical,
            },
            sophia_protocol::CapacityClass::TerminatingBoundary,
            ingress_saturation,
        )? {
            continue;
        }
        deliveries.push(delivery);
        client_keys.record_synthetic_release(key);
    }
    Ok(scratch.len())
}

fn clear_client_pressed_keys_state_only(
    surface: SurfaceId,
    client_keys: &mut SessionClientKeyState,
    scratch: &mut Vec<SessionClientPressedKey>,
    modifiers: &mut XCoreKeyboardMapper,
    input_sender: &impl RoutedInputIngress,
    ingress_saturation: &mut RoutedInputIngressSaturation,
    next_input_delivery: &mut u64,
    time_msec: u64,
) -> Result<usize, Box<dyn std::error::Error>> {
    client_keys.copy_surface_keys(surface, scratch);
    for key in scratch.iter().copied() {
        let _ = modifiers.map_evdev_key(key.keycode, false);
        let serial = *next_input_delivery;
        *next_input_delivery = next_input_delivery
            .checked_add(1)
            .ok_or("live-session input delivery ID exhausted")?;
        if !route_bounded_input(
            input_sender,
            XAuthorityRoutedInput {
                request: sophia_protocol::RoutedInputRequest {
                    serial,
                    seat: key.seat,
                    device: key.device,
                    time_msec,
                    target_surface: key.surface,
                    global_position: Point::default(),
                    local_position: Point::default(),
                    kind: sophia_protocol::InputEventKind::Key {
                        keycode: key.keycode,
                        pressed: false,
                    },
                },
                route_lease: None,
                delivery: None,
                mode: XAuthorityRoutedInputMode::StateOnly,
                origin: sophia_x_authority::XAuthorityRoutedInputOrigin::Physical,
            },
            sophia_protocol::CapacityClass::TerminatingBoundary,
            ingress_saturation,
        )? {
            continue;
        }
        client_keys.record_state_only_release(key);
    }
    Ok(scratch.len())
}
