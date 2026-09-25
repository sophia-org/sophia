// The crossing events of one pointer move, written to a client: the core
// EnterNotify and LeaveNotify the protocol owes along the path, each where
// the client selected it, and the XI2 crossings it selected. Included by
// control_writer.rs beside writers/input.rs; one module with it (t211).

/// What a pointer move's crossings are written from.
struct X11PointerCrossingWrite<'a, W: std::io::Write> {
    stream: &'a mut W,
    byte_order: XByteOrder,
    sequence: u16,
    /// The window the pointer was in, None before anything was observed.
    previous: Option<XResourceId>,
    /// The window the pointer is in now.
    to: XResourceId,
    out_type: u16,
    in_type: u16,
    mode: u8,
    event: XAuthorityInputEvent,
    input_authority: Option<&'a Arc<Mutex<crate::XInputAuthorityState>>>,
    namespace: NamespaceId,
    core_event_selections: &'a Arc<Mutex<XCoreEventSelectionState>>,
    focused_surface_window: &'a AtomicU64,
    root: XResourceId,
    xi_pointer_crossing_mask: u16,
}

fn write_x11_pointer_crossings<W: std::io::Write>(
    write: X11PointerCrossingWrite<'_, W>,
) -> Result<(), X11SetupSocketError> {
    let X11PointerCrossingWrite {
        stream,
        byte_order,
        sequence,
        previous,
        to,
        out_type,
        in_type,
        mode,
        event,
        input_authority,
        namespace,
        core_event_selections,
        focused_surface_window,
        root,
        xi_pointer_crossing_mask,
    } = write;
        if let XAuthorityInputEvent::Pointer(pointer) = event {
            // Read before the selections are held: the focus
            // records take the authority first and then the
            // selections, and this writer must not hold them in
            // the other order.
            let pressed_keys = match input_authority.as_ref() {
                Some(authority) => authority
                    .lock()
                    .map_err(|_| {
                        X11SetupSocketError::new("X11 input authority lock poisoned")
                    })?
                    .pressed_keys(namespace),
                None => [0; 32],
            };
            let selections = core_event_selections.lock().map_err(|_| {
                X11SetupSocketError::new("X11 core event selection lock poisoned")
            })?;
            // The protocol's crossings for the move, from the
            // window the pointer was in (the root before anything
            // was observed) to the one it is in now, each written
            // to the client only where it selected that half, with
            // its detail, its child and coordinates in its own
            // window; a KeymapNotify follows each EnterNotify for a
            // KeymapState selector (XTS Xlib11 EnterNotify,
            // LeaveNotify, KeymapNotify 1).
            // A window this table no longer knows (destroyed since)
            // was left for its parent when it went; the move is
            // from the root, the nearest thing still standing.
            let from = previous
                .filter(|window| selections.knows_window(*window))
                .unwrap_or(root);
            // The focus flag: whether the event window is the focus
            // window or one of its inferiors. The root stands for
            // PointerRoot and an unset focus, so everything is its
            // inferior; a focus of None makes nothing so.
            let focus_raw = focused_surface_window.load(Ordering::Acquire);
            for step in selections.pointer_crossings(from, to) {
                if !selections.crossing_selected(step.window, step.entered) {
                    continue;
                }
                let focus = focus_raw != u64::from(crate::X_FOCUS_NONE)
                    && selections
                        .ancestry_including(step.window)
                        .contains(&XResourceId::new(focus_raw, 1));
                let (event_x, event_y) = selections
                    .root_origin(step.window)
                    .map_or((pointer.root_x, pointer.root_y), |(origin_x, origin_y)| {
                        (
                            clamp_engine_i16(i32::from(pointer.root_x) - origin_x),
                            clamp_engine_i16(i32::from(pointer.root_y) - origin_y),
                        )
                    });
                let mut crossing = encode_x_client_event(
                    byte_order,
                    XClientEvent::PointerCrossing {
                        sequence,
                        entered: step.entered,
                        detail: step.detail,
                        time: pointer.time_msec,
                        root,
                        event: step.window,
                        root_x: pointer.root_x,
                        root_y: pointer.root_y,
                        event_x,
                        event_y,
                        state: pointer.state,
                        mode,
                        focus,
                    },
                );
                write_xi_u32(
                    byte_order,
                    &mut crossing[16..20],
                    u32::try_from(step.child.local.raw()).unwrap_or(0),
                );
                stream.write_all(&crossing).map_err(|error| {
                    x11_peer_write_error(
                        if step.entered {
                            "failed to write X11 EnterNotify event"
                        } else {
                            "failed to write X11 LeaveNotify event"
                        },
                        error,
                    )
                })?;
                if step.entered && selections.keymap_state_selected(step.window) {
                    stream
                        .write_all(&encode_x_client_event(
                            byte_order,
                            keymap_notify_event(pressed_keys),
                        ))
                        .map_err(|error| {
                            x11_peer_write_error("failed to write X11 KeymapNotify event", error)
                        })?;
                }
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
                    to,
                ))
                .map_err(|error| {
                    x11_peer_write_error("failed to write XI2 enter/focus-in event", error)
                })?;
        }
    Ok(())
}
