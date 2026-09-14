#[cfg(unix)]
fn x11_focus_event_record(
    byte_order: XByteOrder,
    sequence: u16,
    window: XResourceId,
    focused: bool,
) -> Vec<u8> {
    encode_x_client_event(
        byte_order,
        XClientEvent::Focus {
            sequence,
            focused,
            detail: 3,
            event: window,
            mode: 0,
        },
    )
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct X11FocusEventState {
    time_msec: u32,
    root_x: i16,
    root_y: i16,
    event_x: i16,
    event_y: i16,
    state: u16,
}

#[cfg(unix)]
fn x11_focus_event_state(
    selections: &XCoreEventSelectionState,
    window: XResourceId,
    modifiers: u16,
    time_msec: u32,
) -> X11FocusEventState {
    let pointer = selections.query_pointer(window);
    X11FocusEventState {
        time_msec,
        root_x: pointer.map_or(0, |pointer| pointer.root_x),
        root_y: pointer.map_or(0, |pointer| pointer.root_y),
        event_x: pointer.map_or(0, |pointer| pointer.win_x),
        event_y: pointer.map_or(0, |pointer| pointer.win_y),
        state: pointer.map_or(modifiers & 0xff, |pointer| {
            (pointer.mask & !0xff) | (modifiers & 0xff)
        }),
    }
}

#[cfg(unix)]
fn encode_xi_focus_event(
    byte_order: XByteOrder,
    sequence: u16,
    window: XResourceId,
    focused: bool,
    state: X11FocusEventState,
) -> Vec<u8> {
    let mut out = vec![0; 76];
    out[0] = 35;
    out[1] = crate::X_INPUT_MAJOR_OPCODE;
    write_xi_u16(byte_order, &mut out[2..4], sequence);
    write_xi_u32(byte_order, &mut out[4..8], 11);
    write_xi_u16(byte_order, &mut out[8..10], if focused { 9 } else { 10 });
    write_xi_u16(byte_order, &mut out[10..12], 3);
    write_xi_u32(byte_order, &mut out[12..16], state.time_msec);
    write_xi_u16(byte_order, &mut out[16..18], 3);
    out[18] = 0;
    out[19] = 3;
    write_xi_u32(byte_order, &mut out[20..24], X_SETUP_DEFAULT_ROOT);
    write_xi_u32(
        byte_order,
        &mut out[24..28],
        u32::try_from(window.local.raw()).unwrap_or_default(),
    );
    write_xi_u32(
        byte_order,
        &mut out[32..36],
        (i32::from(state.root_x) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out[36..40],
        (i32::from(state.root_y) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out[40..44],
        (i32::from(state.event_x) << 16) as u32,
    );
    write_xi_u32(
        byte_order,
        &mut out[44..48],
        (i32::from(state.event_y) << 16) as u32,
    );
    out[48] = 1;
    out[49] = 1;
    write_xi_u16(byte_order, &mut out[50..52], 1);
    let modifiers = u32::from(state.state & 0xff);
    write_xi_u32(byte_order, &mut out[52..56], modifiers);
    write_xi_u32(byte_order, &mut out[64..68], modifiers);
    let buttons = (1_u8..=5).fold(0_u32, |buttons, button| {
        let core_mask = 1_u16 << (u32::from(button) + 7);
        if state.state & core_mask == 0 {
            buttons
        } else {
            buttons | (1_u32 << button)
        }
    });
    write_xi_u32(byte_order, &mut out[72..76], buttons);
    out
}

#[cfg(unix)]
struct X11FocusRecordContext<'a> {
    byte_order: XByteOrder,
    sequence: u16,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    selections: &'a XCoreEventSelectionState,
    input_authority: Option<&'a crate::XInputAuthorityState>,
    modifiers: u16,
    time_msec: u32,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
enum X11FocusRecordRequest {
    Event {
        window: XResourceId,
        focused: bool,
        time_msec: u32,
    },
    Surface {
        window: XResourceId,
        previous_authority: XResourceId,
        previous_routed: XResourceId,
        transition: Option<X11FocusTransition>,
    },
    Clear {
        root: XResourceId,
        previous_routed: XResourceId,
        transition: Option<X11FocusTransition>,
    },
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn x11_focus_records(
    byte_order: XByteOrder,
    sequence: u16,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    core_event_selections: &Mutex<XCoreEventSelectionState>,
    input_authority: Option<&Arc<Mutex<crate::XInputAuthorityState>>>,
    modifiers: u16,
    request: X11FocusRecordRequest,
) -> Result<Vec<Vec<u8>>, X11SetupSocketError> {
    let input_authority = input_authority
        .map(|authority| {
            authority
                .lock()
                .map_err(|_| X11SetupSocketError::new("X11 input authority lock poisoned"))
        })
        .transpose()?;
    // The input writer reads XI source selections under this same order.
    // Holding selections while waiting for input authority forms a cycle
    // with that writer, which can already own input authority and need these
    // selections. Keep the coherent snapshot, acquiring authority first.
    let selections = core_event_selections
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 core event selection lock poisoned"))?;
    let context = X11FocusRecordContext {
        byte_order,
        sequence,
        namespace,
        client,
        selections: &selections,
        input_authority: input_authority.as_deref(),
        modifiers,
        time_msec: x11_server_time_msec(),
    };
    match request {
        X11FocusRecordRequest::Event {
            window,
            focused,
            time_msec,
        } => Ok(x11_selected_focus_records(
            &X11FocusRecordContext {
                time_msec,
                ..context
            },
            window,
            focused,
        )),
        X11FocusRecordRequest::Surface {
            window,
            previous_authority,
            previous_routed,
            transition,
        } => x11_focus_surface_records(
            context,
            window,
            previous_authority,
            previous_routed,
            transition,
        ),
        X11FocusRecordRequest::Clear {
            root,
            previous_routed,
            transition,
        } => x11_clear_focus_records(context, root, previous_routed, transition),
    }
}

#[cfg(unix)]
fn x11_selected_focus_records(
    context: &X11FocusRecordContext<'_>,
    window: XResourceId,
    focused: bool,
) -> Vec<Vec<u8>> {
    let core_selected = context.selections.focus_selected(window);
    let event_type = if focused { 9 } else { 10 };
    let xi_selected = context.input_authority.is_some_and(|authority| {
        authority.xi_event_selected(
            context.namespace,
            context.client.raw(),
            window,
            3,
            event_type,
        )
    });
    tracing::info!(
        "sophia_x11_focus_delivery schema=1 client={} window={} focused={} core_selected={} xi2_selected={} content=redacted",
        context.client.raw(),
        window.local.raw(),
        focused,
        core_selected,
        xi_selected,
    );
    let mut records = Vec::with_capacity(2);
    if core_selected {
        records.push(x11_focus_event_record(
            context.byte_order,
            context.sequence,
            window,
            focused,
        ));
    }
    if xi_selected {
        records.push(encode_xi_focus_event(
            context.byte_order,
            context.sequence,
            window,
            focused,
            x11_focus_event_state(
                context.selections,
                window,
                context.modifiers,
                context.time_msec,
            ),
        ));
    }
    records
}

#[cfg(unix)]
fn x11_focus_surface_records(
    mut context: X11FocusRecordContext<'_>,
    window: XResourceId,
    previous_authority: XResourceId,
    previous_routed: XResourceId,
    transition: Option<X11FocusTransition>,
) -> Result<Vec<Vec<u8>>, X11SetupSocketError> {
    let transition = transition.unwrap_or_else(|| {
        if previous_authority == window && previous_routed == window {
            X11FocusTransition::Unchanged
        } else {
            X11FocusTransition::Enter {
                previous: (previous_routed != window
                    && previous_routed.local.raw() != u64::from(X_SETUP_DEFAULT_ROOT))
                .then_some(previous_routed),
                time_msec: x11_server_time_msec(),
            }
        }
    });
    match transition {
        X11FocusTransition::Unchanged => Ok(Vec::new()),
        X11FocusTransition::Enter {
            previous,
            time_msec,
        } => {
            context.time_msec = time_msec;
            let mut records = Vec::with_capacity(4);
            if let Some(previous) = previous {
                records.extend(x11_selected_focus_records(&context, previous, false));
            }
            records.extend(x11_selected_focus_records(&context, window, true));
            Ok(records)
        }
        X11FocusTransition::Clear { .. } => Err(X11SetupSocketError::new(
            "X11 routed focus transition mismatched FocusSurface",
        )),
    }
}

#[cfg(unix)]
fn x11_clear_focus_records(
    mut context: X11FocusRecordContext<'_>,
    root: XResourceId,
    previous_routed: XResourceId,
    transition: Option<X11FocusTransition>,
) -> Result<Vec<Vec<u8>>, X11SetupSocketError> {
    let (previous, time_msec) = match transition {
        Some(X11FocusTransition::Clear {
            previous,
            time_msec,
        }) => (previous, time_msec),
        None if previous_routed != root => (Some(previous_routed), x11_server_time_msec()),
        None => (None, x11_server_time_msec()),
        Some(_) => {
            return Err(X11SetupSocketError::new(
                "X11 routed focus transition mismatched ClearFocus",
            ));
        }
    };
    context.time_msec = time_msec;
    Ok(previous
        .map(|previous| x11_selected_focus_records(&context, previous, false))
        .unwrap_or_default())
}

#[cfg(unix)]
fn write_x11_control_records(
    stream: &Arc<Mutex<UnixStream>>,
    byte_order: XByteOrder,
    sequence: &AtomicU16,
    records: Vec<Vec<u8>>,
) -> Result<(), X11SetupSocketError> {
    let mut stream = stream
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 output socket lock poisoned"))?;
    let event_sequence = sequence.load(Ordering::Acquire);
    for mut record in records {
        write_xi_u16(byte_order, &mut record[2..4], event_sequence);
        if std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some() {
            tracing::trace!(
                "sophia_x11_socket_write schema=1 writer=control bytes={} payload_redacted=true",
                record.len(),
            );
        }
        stream
            .write_all(&record)
            .map_err(|error| x11_peer_write_error("failed to write X11 control event", error))?;
    }
    stream
        .flush()
        .map_err(|error| x11_peer_write_error("failed to flush X11 control event", error))
}
