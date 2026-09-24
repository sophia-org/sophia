struct X11InputWriterState {
    stream: Arc<Mutex<X11ClientOutput>>,
    output_control_pending: Arc<AtomicUsize>,
    output_wire: Arc<X11WirePermission>,
    byte_order: XByteOrder,
    sequence: Arc<AtomicU16>,
    focused_surface_window: Arc<AtomicU64>,
    core_event_selections: Arc<Mutex<XCoreEventSelectionState>>,
    xkb_state_details: Arc<AtomicU16>,
    xkb_modifiers: Arc<AtomicU16>,
    surface_windows: Arc<Mutex<BTreeMap<SurfaceId, XResourceId>>>,
    input_authority: Option<Arc<Mutex<crate::XInputAuthorityState>>>,
    standalone_query_authority: Option<Arc<Mutex<crate::XInputAuthorityState>>>,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
}

/// What one attempt to write an input event achieved.
///
/// Cancellation and a completed write were the same `Ok(())`, and every
/// `Ok(())` reported the delivery flushed -- so a delivery this writer was
/// told to abandon before touching the socket was recorded as having reached
/// its client. Success and cancellation do not share a return value.
#[cfg(unix)]
enum X11InputWriteOutcome {
    /// The event reached the socket and was flushed.
    Flushed,
    /// The delivery was no longer live before anything was written. Settled as
    /// it was before: the ledger has already moved on from it.
    Inactive,
    /// This writer was told to stop before anything was written.
    Cancelled,
}

#[cfg(unix)]
fn spawn_x11_input_event_writer(
    state: X11InputWriterState,
    receiver: X11InputEventReceiver,
) -> Result<X11InputEventWriter, X11SetupSocketError> {
    let X11InputWriterState {
        stream,
        output_control_pending,
        output_wire,
        byte_order,
        sequence,
        focused_surface_window,
        core_event_selections,
        xkb_state_details,
        xkb_modifiers,
        surface_windows,
        input_authority,
        standalone_query_authority,
        namespace,
        client,
    } = state;
    let stop = Arc::new(AtomicBool::new(false));
    let writer_stop = stop.clone();
    let thread = std::thread::spawn(move || {
        let _recovery_guard = X11InputWriterRecoveryGuard { receiver: &receiver, client };
        let mut pointer_sent_to = None;
        while !writer_stop.load(Ordering::Acquire) {
            let (
                event,
                target_window,
                mut xi_event_type,
                mut xi_event_window,
                mut xi_emulated_button_type,
                mut xi_emulated_button_window,
                xi_pointer_crossing_mask,
                delivery,
            ) =
                match receiver.recv_timeout(client) {
                    Ok(event) => event,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => return Ok(()),
                };
            let receipt = X11InputDeliveryGuard {
                receiver: &receiver, client, delivery, settled: std::cell::Cell::new(false),
            };
            // A mapped GL client can expose its first frame before its event
            // loop installs KeyPress/KeyReleaseMask. Keep physical keys
            // boundedly pending across that startup race instead of writing
            // core events which the client has not selected and will ignore.
            if !receiver.delivery_active(client, delivery) { continue; }
            let keyboard_wait_started = std::time::Instant::now();
            let keyboard_deadline = keyboard_wait_started + Duration::from_secs(5);
            let (
                focused_window,
                routed_keyboard_window,
                keyboard_selected,
                keyboard_discarded,
                focus_names_a_window,
            ) = loop {
                if writer_stop.load(Ordering::Acquire) || !receiver.delivery_active(client, delivery) {
                    return Ok(());
                }
                let selections = core_event_selections.lock().map_err(|_| {
                    X11SetupSocketError::new("X11 core event selection lock poisoned")
                })?;
                let focused = XResourceId::new(focused_surface_window.load(Ordering::Acquire), 1);
                // The focus projection sits at the root until a client sets a
                // focus, and a client that never sets one is the ordinary
                // case, not a broken one. Only the route knows which surface
                // such a key belongs to, so the route still decides there.
                let focus_names_a_window =
                    focused != XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
                let focused_delivery = selections.keyboard_delivery(focused);
                let routed_delivery =
                    target_window.map(|window| selections.keyboard_delivery(window));
                let focused_fallback = selections.keyboard_target(focused);
                let routed_fallback =
                    target_window.map(|window| selections.keyboard_target(window));
                drop(selections);
                let selected = |delivery: XKeyDelivery| match delivery {
                    XKeyDelivery::Window(window) => Some(window),
                    XKeyDelivery::Discard(_) | XKeyDelivery::Unselected => None,
                };
                let focused_selected = selected(focused_delivery);
                let routed_selected = routed_delivery.and_then(selected);
                // A discard is a decision, not a startup race. Waiting the
                // readiness deadline out would delay the event five seconds
                // and then deliver exactly what the client asked us not to.
                if let XKeyDelivery::Discard(reason) = focused_delivery {
                    break (
                        focused_fallback,
                        None,
                        false,
                        Some(reason),
                        focus_names_a_window,
                    );
                }
                if x11_keyboard_route_ready(
                    matches!(event, XAuthorityInputEvent::Key(_)),
                    xi_event_type.is_some(),
                    focused_selected.is_some() || routed_selected.is_some(),
                    std::time::Instant::now() >= keyboard_deadline,
                ) {
                    break (
                        focused_selected.unwrap_or(focused_fallback),
                        routed_selected.or(routed_fallback),
                        focused_selected.is_some() || routed_selected.is_some(),
                        None,
                        focus_names_a_window,
                    );
                }
                std::thread::sleep(Duration::from_millis(5));
            };
            // The focus decided that this key goes nowhere. The delivery is
            // complete rather than refused: nothing failed, and a refusal
            // record would say a route was rejected when the protocol simply
            // says no event is generated.
            if let Some(reason) = keyboard_discarded
                && matches!(event, XAuthorityInputEvent::Key(_))
            {
                receipt.finish(XAuthorityInputDeliveryOutcome::Flushed)?;
                tracing::debug!(
                    "sophia_x11_key_delivery schema=1 status=discarded reason={} client={} input_redacted=true",
                    match reason {
                        XKeyDiscard::FocusNone => "focus_none",
                        XKeyDiscard::DoNotPropagate => "do_not_propagate",
                    },
                    client.raw(),
                );
                continue;
            }
            if std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some()
                && matches!(event, XAuthorityInputEvent::Key(_))
            {
                tracing::debug!(
                    "sophia_x11_key_delivery schema=2 stage=target_resolved client={} keyboard_selected={} explicit_target={} xi_event={} wait_msec={} input_redacted=true",
                    client.raw(),
                    keyboard_selected,
                    routed_keyboard_window.is_some(),
                    xi_event_type.is_some(),
                    keyboard_wait_started.elapsed().as_millis(),
                );
            }
            if let XAuthorityInputEvent::Key(_) = event
                && routed_keyboard_window.is_some_and(|window| window != focused_window)
            {
                tracing::warn!(
                    "sophia_x11_key_delivery schema=1 target_matches_focus=false explicit_target=true",
                );
            }
            let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
            let (delivered_window, pointer_surface_window, pointer_event_ancestry) = match event {
                // The route says which surface a key belongs to; which window
                // inside it hears the key is the protocol's question, and the
                // focus is the only one of the two that was asked it. Taking
                // the routed window whenever it existed meant a focus on one
                // child with the pointer in its sibling reported the key on
                // the sibling, because the routed window is the surface's own
                // top-level and the rule then ran from there instead of from
                // the focus. When no focus has been set the projection is
                // still the root and the route is all there is, which is the
                // ordinary case for a client that never calls SetInputFocus.
                XAuthorityInputEvent::Key(_) => {
                    let window = if focus_names_a_window {
                        focused_window
                    } else {
                        routed_keyboard_window.unwrap_or(focused_window)
                    };
                    (window, None, None)
                }
                XAuthorityInputEvent::Pointer(pointer) => {
                    let Some(surface_window) = x11_pointer_surface_window(
                        target_window,
                        pointer.surface,
                        &surface_windows,
                    )? else {
                        receipt.finish(XAuthorityInputDeliveryOutcome::TargetGone)?;
                        tracing::debug!(
                            "sophia_x11_input_delivery schema=1 status=target_gone event=pointer client={} input_redacted=true",
                            client.raw(),
                        );
                        continue;
                    };
                    let selections = core_event_selections
                        .lock()
                        .map_err(|_| {
                            X11SetupSocketError::new("X11 core event selection lock poisoned")
                        })?;
                    let event_window = selections.pointer_event_target(
                        surface_window,
                        pointer.event_x,
                        pointer.event_y,
                    );
                    let event_ancestry = selections.ancestry_including(event_window);
                    let delivered_window = selections
                        .selected_pointer_target(
                            surface_window,
                            matches!(pointer.kind, XAuthorityPointerEventKind::Motion),
                            pointer.state,
                            pointer.event_x,
                            pointer.event_y,
                        )
                        .unwrap_or(surface_window);
                    (delivered_window, Some(surface_window), Some(event_ancestry))
                }
            };
            if let Some(authority) = standalone_query_authority.as_ref() {
                authority.lock()
                    .map_err(|_| X11SetupSocketError::new("X11 input authority lock poisoned"))?
                    .observe_query_input(namespace, pointer_surface_window.unwrap_or(focused_window), event);
            }
            if let (Some(input_authority), Some(event_ancestry)) =
                (input_authority.as_ref(), pointer_event_ancestry.as_ref())
            {
                let authority = input_authority.lock().map_err(|_| {
                    X11SetupSocketError::new("X11 input authority lock poisoned")
                })?;
                let (selected_type, emulated_button_selected_type) = match event {
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind: XAuthorityPointerEventKind::Motion,
                        ..
                    }) => (Some(6), None),
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind: XAuthorityPointerEventKind::Button { pressed, .. },
                        ..
                    }) => (Some(if pressed { 4 } else { 5 }), None),
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind:
                            XAuthorityPointerEventKind::Axis {
                                pressed,
                                horizontal_position_v120,
                                vertical_position_v120,
                                ..
                            },
                        ..
                    }) => (
                        (horizontal_position_v120.is_some()
                            || vertical_position_v120.is_some())
                        .then_some(6),
                        Some(if pressed { 4 } else { 5 }),
                    ),
                    XAuthorityInputEvent::Key(_) => (None, None),
                };
                xi_event_window = selected_type.and_then(|event_type| {
                    x11_selected_xi_event_window(
                        &authority,
                        namespace,
                        client.raw(),
                        event_ancestry,
                        2,
                        event_type,
                    )
                });
                xi_event_type = xi_event_window.and(selected_type);
                xi_emulated_button_window =
                    emulated_button_selected_type.and_then(|event_type| {
                        x11_selected_xi_event_window(
                            &authority,
                            namespace,
                            client.raw(),
                            event_ancestry,
                            2,
                            event_type,
                        )
                    });
                xi_emulated_button_type =
                    xi_emulated_button_window.and(emulated_button_selected_type);
            }
            let (wire_event_x, wire_event_y) = match (event, pointer_surface_window) {
                (XAuthorityInputEvent::Pointer(pointer), Some(surface_window)) => {
                    core_event_selections
                        .lock()
                        .map_err(|_| {
                            X11SetupSocketError::new("X11 core event selection lock poisoned")
                        })?
                        .pointer_event_coordinates(
                            surface_window,
                            delivered_window,
                            pointer.event_x,
                            pointer.event_y,
                        )
                }
                _ => (0, 0),
            };
            let (xi_delivery, xi_emulated_button_delivery) = match (
                event,
                pointer_surface_window,
                pointer_event_ancestry.as_deref(),
            ) {
                (
                    XAuthorityInputEvent::Pointer(pointer),
                    Some(surface_window),
                    Some(event_ancestry),
                ) => {
                    let selections = core_event_selections.lock().map_err(|_| {
                        X11SetupSocketError::new("X11 core event selection lock poisoned")
                    })?;
                    (
                        x11_xi_pointer_delivery(
                            &selections,
                            surface_window,
                            event_ancestry,
                            xi_event_window,
                            pointer.event_x,
                            pointer.event_y,
                        ),
                        x11_xi_pointer_delivery(
                            &selections,
                            surface_window,
                            event_ancestry,
                            xi_emulated_button_window,
                            pointer.event_x,
                            pointer.event_y,
                        ),
                    )
                }
                _ => (None, None),
            };
            if std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some()
                && let XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                    kind:
                        XAuthorityPointerEventKind::Axis {
                            pressed,
                            horizontal_position_v120,
                            vertical_position_v120,
                            ..
                        },
                    state,
                    ..
                }) = event
            {
                let descendant_target = pointer_event_ancestry
                    .as_ref()
                    .and_then(|ancestry| ancestry.first())
                    .zip(pointer_surface_window)
                    .is_some_and(|(event_window, surface_window)| *event_window != surface_window);
                tracing::info!(
                    "sophia_x11_axis_delivery schema=2 descendant_target={} smooth_selected={} smooth_child={} smooth_depth={} emulated_button_selected={} emulated_button_child={} emulated_button_depth={} pressed={} horizontal_v120={} vertical_v120={} physical_buttons={} input_redacted=true",
                    descendant_target,
                    xi_event_type == Some(6),
                    xi_delivery.is_some_and(|delivery| delivery.child != XResourceId::NONE),
                    xi_delivery.map_or(0, |delivery| delivery.ancestry_depth),
                    xi_emulated_button_type.is_some(),
                    xi_emulated_button_delivery
                        .is_some_and(|delivery| delivery.child != XResourceId::NONE),
                    xi_emulated_button_delivery.map_or(0, |delivery| delivery.ancestry_depth),
                    pressed,
                    horizontal_position_v120.unwrap_or(0),
                    vertical_position_v120.unwrap_or(0),
                    state >> 8,
                );
            }
            let mut record = encode_x_client_event(
                byte_order,
                match event {
                    XAuthorityInputEvent::Key(event) => XClientEvent::Key {
                        sequence: 0,
                        pressed: event.pressed,
                        keycode: event.keycode,
                        time: event.time_msec,
                        root,
                        event: delivered_window,
                        state: event.state,
                    },
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind: XAuthorityPointerEventKind::Motion,
                        surface: _,
                        root_x,
                        root_y,
                        event_x: _,
                        event_y: _,
                        state,
                        time_msec,
                    }) => XClientEvent::PointerMotion {
                        sequence: 0,
                        time: time_msec,
                        root,
                        event: delivered_window,
                        root_x,
                        root_y,
                        event_x: wire_event_x,
                        event_y: wire_event_y,
                        state,
                    },
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind: XAuthorityPointerEventKind::Button { button, pressed },
                        surface: _,
                        root_x,
                        root_y,
                        event_x: _,
                        event_y: _,
                        state,
                        time_msec,
                    }) => XClientEvent::PointerButton {
                        sequence: 0,
                        pressed,
                        button,
                        time: time_msec,
                        root,
                        event: delivered_window,
                        root_x,
                        root_y,
                        event_x: wire_event_x,
                        event_y: wire_event_y,
                        state,
                    },
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind: XAuthorityPointerEventKind::Axis {
                            button, pressed, ..
                        },
                        surface: _,
                        root_x,
                        root_y,
                        event_x: _,
                        event_y: _,
                        state,
                        time_msec,
                    }) => XClientEvent::PointerButton {
                        sequence: 0,
                        pressed,
                        button,
                        time: time_msec,
                        root,
                        event: delivered_window,
                        root_x,
                        root_y,
                        event_x: wire_event_x,
                        event_y: wire_event_y,
                        state,
                    },
                },
            );
            let mut write_core_record = match (event, input_authority.as_ref()) {
                (XAuthorityInputEvent::Pointer(pointer), Some(authority)) => {
                    // An explicit grab's mask follows the same rule as a
                    // window's: motion with a button down answers to
                    // ButtonMotion and the held button's own mask too.
                    let selected_mask = match pointer.kind {
                        XAuthorityPointerEventKind::Motion => {
                            XCoreEventSelectionState::motion_selection_mask(pointer.state) as u16
                        }
                        XAuthorityPointerEventKind::Button { pressed: true, .. }
                        | XAuthorityPointerEventKind::Axis { pressed: true, .. } => 1_u16 << 2,
                        XAuthorityPointerEventKind::Button { pressed: false, .. }
                        | XAuthorityPointerEventKind::Axis { pressed: false, .. } => 1_u16 << 3,
                    };
                    authority
                        .lock()
                        .map_err(|_| {
                            X11SetupSocketError::new("X11 input authority lock poisoned")
                        })?
                        .pointer_grab(namespace)
                        .filter(|grab| grab.owner == client.raw())
                        .is_none_or(|grab| grab.event_mask & selected_mask != 0)
                }
                _ => true,
            };
            if let (XAuthorityInputEvent::Pointer(pointer), Some(surface_window), Some(ancestry)) =
                (event, pointer_surface_window, pointer_event_ancestry.as_ref())
            {
                let core_target = core_event_selections.lock().map_err(|_| {
                    X11SetupSocketError::new("X11 core event selection lock poisoned")
                })?.selected_pointer_target(
                    surface_window,
                    matches!(pointer.kind, XAuthorityPointerEventKind::Motion),
                    pointer.state,
                    pointer.event_x,
                    pointer.event_y,
                );
                let core_depth = core_target.and_then(|target| ancestry.iter().position(|window| *window == target));
                let xi_depth = [xi_delivery, xi_emulated_button_delivery].into_iter().flatten()
                    .map(|delivery| delivery.ancestry_depth).min();
                // XI master delivery wins over core delivery at the same window.
                // A nearer core subscriber still stops propagation to an XI ancestor.
                if let Some(xi_depth) = xi_depth {
                    if core_depth.is_none_or(|core_depth| xi_depth <= core_depth) {
                        write_core_record = false;
                    } else {
                        xi_event_type = None;
                        xi_emulated_button_type = None;
                    }
                }
            }
            let mut xi_source_records = if let (
                Some(authority),
                Some(surface_window),
                Some(ancestry),
                XAuthorityInputEvent::Pointer(pointer),
            ) = (
                input_authority.as_ref(),
                pointer_surface_window,
                pointer_event_ancestry.as_deref(),
                event,
            ) {
                let authority = authority
                    .lock()
                    .map_err(|_| X11SetupSocketError::new("X11 input authority lock poisoned"))?;
                let selections = core_event_selections
                    .lock()
                    .map_err(|_| X11SetupSocketError::new("X11 core event selection lock poisoned"))?;
                encode_xi_source_pointer_events(X11XiSourceEvent {
                    byte_order,
                    sequence: 0,
                    namespace,
                    client,
                    authority: &authority,
                    selections: &selections,
                    surface_window,
                    ancestry,
                    pointer,
                    crossing: (pointer_sent_to != Some(delivered_window))
                        .then_some((pointer_sent_to, delivered_window)),
                })
            } else {
                [None, None, None, None]
            };
            let write_result = (|| -> Result<X11InputWriteOutcome, X11SetupSocketError> {
                let Some(mut stream) =
                    lock_x11_non_control_output(
                        &stream,
                        &output_wire,
                        &output_control_pending,
                        Some(&writer_stop),
                    )?
                else {
                    return Ok(X11InputWriteOutcome::Cancelled);
                };
                if !receiver.delivery_active(client, delivery) {
                    return Ok(X11InputWriteOutcome::Inactive);
                }
                let sequence = sequence.load(Ordering::Acquire);
                write_xi_u16(byte_order, &mut record[2..4], sequence);
                let transition = match event {
                    XAuthorityInputEvent::Pointer(_)
                        if pointer_sent_to != Some(delivered_window) =>
                    {
                        Some((pointer_sent_to, 8, 7))
                    }
                    _ => None,
                };
                for bytes in xi_source_records.iter_mut().flatten() {
                    write_xi_u16(byte_order, &mut bytes[2..4], sequence);
                    stream.write_all(bytes).map_err(|error| {
                        if is_x11_client_disconnect(&error) {
                            X11SetupSocketError::client_disconnect(format!(
                                "X11 client disconnected while writing XI2 source event: {error}"
                            ))
                        } else {
                            X11SetupSocketError::new(format!("failed to write XI2 source event: {error}"))
                        }
                    })?;
                }
                if let Some((previous, out_type, in_type)) = transition {
                    if let XAuthorityInputEvent::Pointer(pointer) = event {
                        let selections = core_event_selections.lock().map_err(|_| {
                            X11SetupSocketError::new("X11 core event selection lock poisoned")
                        })?;
                        if let Some(previous) = previous
                            && selections.crossing_selected(previous, false)
                        {
                            stream
                                .write_all(&encode_x_client_event(
                                    byte_order,
                                    XClientEvent::PointerCrossing {
                                        sequence,
                                        entered: false,
                                        detail: 3,
                                        time: pointer.time_msec,
                                        root,
                                        event: previous,
                                        root_x: pointer.root_x,
                                        root_y: pointer.root_y,
                                        event_x: wire_event_x,
                                        event_y: wire_event_y,
                                        state: pointer.state,
                                        mode: 0,
                                        focus: true,
                                    },
                                ))
                                .map_err(|error| {
                                    x11_peer_write_error("failed to write X11 LeaveNotify event", error)
                                })?;
                        }
                        if selections.crossing_selected(delivered_window, true) {
                            stream
                                .write_all(&encode_x_client_event(
                                    byte_order,
                                    XClientEvent::PointerCrossing {
                                        sequence,
                                        entered: true,
                                        detail: 3,
                                        time: pointer.time_msec,
                                        root,
                                        event: delivered_window,
                                        root_x: pointer.root_x,
                                        root_y: pointer.root_y,
                                        event_x: wire_event_x,
                                        event_y: wire_event_y,
                                        state: pointer.state,
                                        mode: 0,
                                        focus: true,
                                    },
                                ))
                                .map_err(|error| {
                                    x11_peer_write_error("failed to write X11 EnterNotify event", error)
                                })?;
                        }
                        drop(selections);
                    }
                    if let Some(previous) = previous
                        && xi_pointer_crossing_mask & (1 << out_type) != 0
                    {
                        stream
                            .write_all(&encode_xi_crossing_event(
                                byte_order, sequence, out_type, event, previous,
                            ))
                            .map_err(|error| {
                                x11_peer_write_error("failed to write XI2 leave/focus-out event", error)
                            })?;
                    }
                    if xi_pointer_crossing_mask & (1 << in_type) != 0 {
                        stream
                            .write_all(&encode_xi_crossing_event(
                                byte_order,
                                sequence,
                                in_type,
                                event,
                                delivered_window,
                            ))
                            .map_err(|error| {
                                x11_peer_write_error("failed to write XI2 enter/focus-in event", error)
                            })?;
                    }
                    if matches!(event, XAuthorityInputEvent::Pointer(_)) {
                        pointer_sent_to = Some(delivered_window);
                    }
                }
                if write_core_record {
                    stream.write_all(&record).map_err(|error| {
                        if is_x11_client_disconnect(&error) {
                            X11SetupSocketError::client_disconnect(format!(
                                "X11 client disconnected while writing input: {error}"
                            ))
                        } else {
                            X11SetupSocketError::new(format!(
                                "failed to write X11 input event: {error}"
                            ))
                        }
                    })?;
                }
                if let (
                    Some(surface_window),
                    XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                        kind,
                        root_x,
                        root_y,
                        event_x,
                        event_y,
                        state,
                        ..
                    }),
                ) = (pointer_surface_window, event)
                {
                    let mask = match kind {
                        XAuthorityPointerEventKind::Motion => state,
                        XAuthorityPointerEventKind::Button { button, pressed }
                        | XAuthorityPointerEventKind::Axis {
                            button, pressed, ..
                        } => {
                            let button_mask = if button <= 5 {
                                1_u16 << (u32::from(button) + 7)
                            } else {
                                0
                            };
                            if pressed {
                                state | button_mask
                            } else {
                                state & !button_mask
                            }
                        }
                    };
                    core_event_selections
                        .lock()
                        .map_err(|_| {
                            X11SetupSocketError::new("X11 core event selection lock poisoned")
                        })?
                        .observe_pointer(
                            surface_window,
                            pointer_event_ancestry
                                .as_ref()
                                .and_then(|ancestry| ancestry.first())
                                .copied()
                                .unwrap_or(delivered_window),
                            root_x,
                            root_y,
                            event_x,
                            event_y,
                            mask,
                        );
                }
                if std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some() {
                    tracing::trace!(
                        "sophia_x11_socket_write schema=1 writer=input bytes={} payload_redacted=true",
                        record.len(),
                    );
                }
                if std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some()
                    && matches!(event, XAuthorityInputEvent::Key(_))
                {
                    tracing::debug!(
                        "sophia_x11_key_delivery schema=2 stage=wire_flushed sequence={sequence} input_redacted=true"
                    );
                }
                if let XAuthorityInputEvent::Key(key) = event {
                    let previous = xkb_modifiers
                        .swap(u16::from(key.modifiers_after), Ordering::AcqRel);
                    let changed = previous ^ u16::from(key.modifiers_after);
                    let selected = xkb_state_details.load(Ordering::Acquire);
                    if changed != 0 && selected & 1 != 0 {
                        let state_notify = encode_x_client_event(
                            byte_order,
                            XClientEvent::XkbStateNotify {
                                sequence,
                                time: key.time_msec,
                                modifiers: key.modifiers_after,
                                changed: 1,
                                keycode: key.keycode,
                                event_type: if key.pressed { 2 } else { 3 },
                            },
                        );
                        stream.write_all(&state_notify).map_err(|error| {
                            x11_peer_write_error("failed to write XKB state notification", error)
                        })?;
                    }
                }
                if let Some(event_type) = xi_event_type {
                    let delivery = xi_delivery.unwrap_or(X11XiPointerDelivery {
                        window: xi_event_window.unwrap_or(delivered_window),
                        child: XResourceId::NONE,
                        event_x: wire_event_x,
                        event_y: wire_event_y,
                        ancestry_depth: 0,
                    });
                    // The smooth valuator event is the physical source event. Only
                    // its compatibility button companion is pointer-emulated.
                    let generic = encode_xi_device_event(
                        byte_order,
                        sequence,
                        event_type,
                        event,
                        delivery.window,
                        delivery.child,
                        delivery.event_x,
                        delivery.event_y,
                        0,
                    );
                    stream.write_all(&generic).map_err(|error| {
                        x11_peer_write_error("failed to write XI2 generic event", error)
                    })?;
                }
                if let Some(event_type) = xi_emulated_button_type {
                    let delivery = xi_emulated_button_delivery.unwrap_or(X11XiPointerDelivery {
                        window: xi_emulated_button_window.unwrap_or(delivered_window),
                        child: XResourceId::NONE,
                        event_x: wire_event_x,
                        event_y: wire_event_y,
                        ancestry_depth: 0,
                    });
                    let generic = encode_xi_device_event(
                        byte_order,
                        sequence,
                        event_type,
                        event,
                        delivery.window,
                        delivery.child,
                        delivery.event_x,
                        delivery.event_y,
                        XI_POINTER_EMULATED,
                    );
                    stream.write_all(&generic).map_err(|error| {
                        x11_peer_write_error("failed to write emulated XI2 wheel-button event", error)
                    })?;
                }
                stream
                    .flush()
                    .map(|()| X11InputWriteOutcome::Flushed)
                    .map_err(|error| {
                        x11_peer_write_error("failed to flush X11 input event", error)
                    })
            })();
            match write_result {
                Ok(X11InputWriteOutcome::Flushed | X11InputWriteOutcome::Inactive) => {
                    receipt.finish(XAuthorityInputDeliveryOutcome::Flushed)?
                }
                Ok(X11InputWriteOutcome::Cancelled) => {
                    // Nothing reached the socket. Not flushed, and not the
                    // recipient's doing: this writer was told to stop. The
                    // receipt is left unsettled so the writer's own teardown
                    // settles it for what it is, rather than this reporting an
                    // outcome that did not happen.
                    //
                    // "Nothing written" is not "nothing owed": the delivery was
                    // accepted, and something still has to answer for it.
                    return Ok(());
                }
                Err(error) => {
                    if error.client_disconnect {
                        receipt.finish(XAuthorityInputDeliveryOutcome::ClientDisconnected)?;
                        return Ok(());
                    }
                    let _ = receipt.finish(XAuthorityInputDeliveryOutcome::WriteFailed);
                    return Err(error);
                }
            }
        }
        Ok(())
    });
    Ok(X11InputEventWriter { stop, thread })
}

#[cfg(unix)]
fn x11_keyboard_route_ready(
    is_key: bool,
    xi_selected: bool,
    core_selected: bool,
    deadline_elapsed: bool,
) -> bool {
    !is_key || xi_selected || core_selected || deadline_elapsed
}
