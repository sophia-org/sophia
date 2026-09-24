#[cfg(unix)]
fn route_x11_dispatch_protocol_outputs(
    state: &X11CoreSocketServerState,
    routing: &XServerFrontendRouteRegistry,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    output: &mut XDispatchResult,
) -> Result<(), X11SetupSocketError> {
    capture_clipboard_proxy_payload(state, routing, namespace, output)?;
    route_selection_event(state, routing, client, output)?;
    route_property_events(routing, client, output)?;
    route_mapping_notify_events(routing, namespace, client, output)?;
    route_core_lifecycle_events(routing, client, output)
}

#[cfg(unix)]
/// MappingNotify reaches every client of the namespace; the requester keeps
/// its own copy, sequenced with its request, and the others are sequenced by
/// their writers.
#[cfg(unix)]
fn route_mapping_notify_events(
    routing: &XServerFrontendRouteRegistry,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    output: &XDispatchResult,
) -> Result<(), X11SetupSocketError> {
    for item in &output.outputs {
        let crate::XClientOutput::Event(event @ XClientEvent::MappingNotify { .. }) = item else {
            continue;
        };
        let mut copy = *event;
        if let XClientEvent::MappingNotify { sequence, .. } = &mut copy {
            *sequence = 0;
        }
        routing
            .broadcast_protocol_event(namespace, client, copy)
            .map_err(|error| {
                X11SetupSocketError::new(format!("failed to broadcast a MappingNotify: {error}"))
            })?;
    }
    Ok(())
}

#[cfg(unix)]
fn route_core_lifecycle_events(
    routing: &XServerFrontendRouteRegistry,
    client: XServerFrontendClientId,
    output: &mut XDispatchResult,
) -> Result<(), X11SetupSocketError> {
    route_core_lifecycle_events_with_control(routing, client, output, None)
}

#[cfg(unix)]
fn route_core_lifecycle_events_with_control(
    routing: &XServerFrontendRouteRegistry,
    client: XServerFrontendClientId,
    output: &mut XDispatchResult,
    execution: Option<&Arc<Mutex<PrivateControlExecution>>>,
) -> Result<(), X11SetupSocketError> {
    retain_private_control_events(execution, output.outputs.iter().filter_map(|item| match item {
        crate::XClientOutput::Event(event) => Some((None, *event)),
        _ => None,
    }))?;
    const EXPOSURE_MASK: u32 = 1 << 15;
    const VISIBILITY_CHANGE_MASK: u32 = 1 << 16;
    const STRUCTURE_NOTIFY_MASK: u32 = 1 << 17;
    const SUBSTRUCTURE_NOTIFY_MASK: u32 = 1 << 19;
    const SUBSTRUCTURE_REDIRECT_MASK: u32 = 1 << 20;
    const FOCUS_CHANGE_MASK: u32 = 1 << 21;

    // A focus transition is reported to whoever asked about the window it
    // names, and asking is selecting FocusChange on that window. Making the
    // request that moved the focus is not asking: a client that moves the
    // focus and did not select is exactly as uninterested as one that did
    // neither, and until now it was told anyway, twice, about a window it
    // had said nothing about.
    //
    // Every client that selected on the window is delivered the transition,
    // not only the one whose request caused it. The chain itself is resolved
    // before this, by whichever layer applied the focus; what happens here is
    // fanning each resolved event out to its subscribers, which is the same
    // treatment the substructure events below get. The focus producer reaches
    // only the requesting connection's own stream, so without this a second
    // client watching a window learns nothing when the focus enters it.
    let mut unselected_focus = Vec::new();
    for (index, item) in output.outputs.iter().enumerate() {
        let crate::XClientOutput::Event(event @ XClientEvent::Focus { event: window, .. }) = item
        else {
            continue;
        };
        let selectors = routing
            .core_event_subscribers(*window, FOCUS_CHANGE_MASK)
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to inspect X11 focus subscriptions: {error}"
                ))
            })?;
        for recipient in selectors
            .iter()
            .copied()
            .filter(|recipient| *recipient != client)
        {
            retain_private_control_events(execution, [(Some(recipient), *event)])?;
            routing
                .route_control_protocol(recipient, *event, execution)
                .map_err(|error| {
                    X11SetupSocketError::new(format!("failed to route X11 focus event: {error}"))
                })?;
        }
        if !selectors.contains(&client) {
            unselected_focus.push(index);
        }
    }
    for index in unselected_focus.into_iter().rev() {
        output.outputs.remove(index);
    }

    let mut candidates = Vec::new();
    for (index, output) in output.outputs.iter().enumerate() {
        let crate::XClientOutput::Event(event) = output else {
            continue;
        };
        let candidate = match *event {
            XClientEvent::CreateNotify { parent, .. } => {
                Some((index, parent, SUBSTRUCTURE_NOTIFY_MASK, *event))
            }
            XClientEvent::MapNotify { event: target, .. }
            | XClientEvent::UnmapNotify { event: target, .. }
            | XClientEvent::CirculateNotify { event: target, .. }
            | XClientEvent::ConfigureNotify {
                synthetic: false,
                event: target,
                ..
            } => {
                Some((index, target, STRUCTURE_NOTIFY_MASK, *event))
            }
            XClientEvent::CirculateRequest { parent, .. } => {
                Some((index, parent, SUBSTRUCTURE_REDIRECT_MASK, *event))
            }
            // A destroy addressed to its own window is a StructureNotify
            // record; one addressed elsewhere is the parent-addressed form the
            // handler produced, and belongs to SubstructureNotify selectors on
            // that parent. Both may exist for one destroy, and a client holding
            // both masks is entitled to both.
            XClientEvent::DestroyNotify {
                event: target,
                window,
                ..
            } => Some((
                index,
                target,
                if target == window {
                    STRUCTURE_NOTIFY_MASK
                } else {
                    SUBSTRUCTURE_NOTIFY_MASK
                },
                *event,
            )),
            // A redirected map is addressed to the parent and belongs to
            // whoever asked to manage that parent's children.
            XClientEvent::MapRequest { parent, .. } => {
                Some((index, parent, SUBSTRUCTURE_REDIRECT_MASK, *event))
            }
            XClientEvent::VisibilityNotify { window, .. } => {
                Some((index, window, VISIBILITY_CHANGE_MASK, *event))
            }
            XClientEvent::Expose { window, .. } => {
                Some((index, window, EXPOSURE_MASK, *event))
            }
            _ => None,
        };
        if let Some(candidate) = candidate {
            candidates.push(candidate);
        }
    }

    let structure_events = candidates
        .iter()
        .filter_map(|(_, _, _, event)| match event {
            event @ (XClientEvent::MapNotify { window, .. }
            | XClientEvent::UnmapNotify { window, .. }
            | XClientEvent::CirculateNotify { window, .. }
            | XClientEvent::DestroyNotify { window, .. }) => Some((*window, *event)),
            event @ XClientEvent::ConfigureNotify {
                synthetic: false,
                window,
                ..
            } => Some((*window, *event)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut remove = Vec::new();
    for (index, target, required_mask, event) in candidates {
        let subscribers = routing
            .core_event_subscribers(target, required_mask)
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to inspect X11 lifecycle subscriptions: {error}"
                ))
            })?;
        for recipient in subscribers.iter().copied().filter(|recipient| *recipient != client) {
            retain_private_control_events(execution, [(Some(recipient), event)])?;
            routing.route_control_protocol(recipient, event, execution).map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to route X11 lifecycle event: {error}"
                ))
            })?;
        }
        if !subscribers.contains(&client) {
            remove.push(index);
        }
    }
    for index in remove.into_iter().rev() {
        output.outputs.remove(index);
    }

    // Core structure events are also delivered to SubstructureNotify
    // selectors on the immediate parent. The direct event above remains
    // addressed to the window itself.
    for (window, event) in structure_events {
        let Some(parent) = routing.window_parent(window).map_err(|error| {
            X11SetupSocketError::new(format!("failed to resolve X11 lifecycle parent: {error}"))
        })? else {
            continue;
        };
        let subscribers = routing
            .core_event_subscribers(parent, SUBSTRUCTURE_NOTIFY_MASK)
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to inspect X11 parent lifecycle subscriptions: {error}"
                ))
            })?;
        for recipient in subscribers {
            let parent_event = lifecycle_event_for_parent(event, parent);
            retain_private_control_events(execution, [(Some(recipient), parent_event)])?;
            if recipient == client {
                output.outputs.push(crate::XClientOutput::Event(parent_event));
            } else {
                routing.route_control_protocol(recipient, parent_event, execution).map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to route X11 parent lifecycle event: {error}"
                    ))
                })?;
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn filter_local_core_lifecycle_events(
    selections: &XCoreEventSelectionState,
    output: &mut XDispatchResult,
) {
    const EXPOSURE_MASK: u32 = 1 << 15;
    const VISIBILITY_CHANGE_MASK: u32 = 1 << 16;
    const STRUCTURE_NOTIFY_MASK: u32 = 1 << 17;
    const SUBSTRUCTURE_NOTIFY_MASK: u32 = 1 << 19;
    const SUBSTRUCTURE_REDIRECT_MASK: u32 = 1 << 20;
    const FOCUS_CHANGE_MASK: u32 = 1 << 21;

    let structure_events = output
        .outputs
        .iter()
        .filter_map(|output| match output {
            crate::XClientOutput::Event(
                event @ (XClientEvent::MapNotify { window, .. }
                | XClientEvent::UnmapNotify { window, .. }
                | XClientEvent::CirculateNotify { window, .. }),
            ) => Some((*window, *event)),
            crate::XClientOutput::Event(
                event @ XClientEvent::ConfigureNotify {
                    synthetic: false,
                    window,
                    ..
                },
            ) => Some((*window, *event)),
            _ => None,
        })
        .collect::<Vec<_>>();

    output.outputs.retain(|output| {
        let crate::XClientOutput::Event(event) = output else {
            return true;
        };
        match *event {
            XClientEvent::CreateNotify { parent, .. } => {
                selections.selects(parent, SUBSTRUCTURE_NOTIFY_MASK)
            }
            XClientEvent::MapNotify { event: target, .. }
            | XClientEvent::UnmapNotify { event: target, .. }
            | XClientEvent::CirculateNotify { event: target, .. }
            | XClientEvent::ConfigureNotify {
                synthetic: false,
                event: target,
                ..
            } => selections.selects(target, STRUCTURE_NOTIFY_MASK),
            // A redirected map or circulate reaches this client only if it
            // is the one managing the parent, which is the usual case for a
            // window manager acting through its own connection.
            XClientEvent::MapRequest { parent, .. }
            | XClientEvent::CirculateRequest { parent, .. } => {
                selections.selects(parent, SUBSTRUCTURE_REDIRECT_MASK)
            }
            XClientEvent::VisibilityNotify { window, .. } => {
                selections.selects(window, VISIBILITY_CHANGE_MASK)
            }
            XClientEvent::Expose { window, .. } => selections.selects(window, EXPOSURE_MASK),
            // A focus transition is reported to the windows that asked about
            // it, and asking is selecting FocusChange on the window named.
            // The request that caused the transition earns its client
            // nothing: a client that moves the focus and did not select is
            // exactly as uninterested as one that did neither.
            XClientEvent::Focus { event: window, .. } => {
                selections.selects(window, FOCUS_CHANGE_MASK)
            }
            // A synthetic ConfigureNotify is the protocol response to a
            // managed ConfigureWindow request, not a selected structure event.
            XClientEvent::ConfigureNotify {
                synthetic: true, ..
            } => true,
            _ => true,
        }
    });

    for (window, event) in structure_events {
        let Some(parent) = selections.parent(window) else {
            continue;
        };
        if selections.selects(parent, SUBSTRUCTURE_NOTIFY_MASK) {
            output.outputs.push(crate::XClientOutput::Event(
                lifecycle_event_for_parent(event, parent),
            ));
        }
    }
}

#[cfg(unix)]
fn lifecycle_event_for_parent(event: XClientEvent, parent: XResourceId) -> XClientEvent {
    match event {
        XClientEvent::MapNotify {
            sequence,
            window,
            override_redirect,
            ..
        } => XClientEvent::MapNotify {
            sequence,
            event: parent,
            window,
            override_redirect,
        },
        XClientEvent::DestroyNotify {
            sequence, window, ..
        } => XClientEvent::DestroyNotify {
            sequence,
            event: parent,
            window,
        },
        XClientEvent::CirculateNotify {
            sequence,
            window,
            place,
            ..
        } => XClientEvent::CirculateNotify {
            sequence,
            event: parent,
            window,
            place,
        },
        XClientEvent::UnmapNotify {
            sequence,
            window,
            from_configure,
            ..
        } => XClientEvent::UnmapNotify {
            sequence,
            event: parent,
            window,
            from_configure,
        },
        XClientEvent::ConfigureNotify {
            sequence,
            synthetic,
            window,
            above_sibling,
            x,
            y,
            width,
            height,
            border_width,
            override_redirect,
            ..
        } => XClientEvent::ConfigureNotify {
            sequence,
            synthetic,
            event: parent,
            window,
            above_sibling,
            x,
            y,
            width,
            height,
            border_width,
            override_redirect,
        },
        _ => event,
    }
}

#[cfg(unix)]
fn route_x11_present_configure(
    routing: &XServerFrontendRouteRegistry,
    client: XServerFrontendClientId,
    sequence: u16,
    window: XResourceId,
    geometry: Rect,
) -> Result<Vec<XClientEvent>, X11SetupSocketError> {
    route_x11_present_configure_with_control(routing, client, sequence, window, geometry, None)
}

#[cfg(unix)]
fn route_x11_present_configure_with_control(
    routing: &XServerFrontendRouteRegistry,
    client: XServerFrontendClientId,
    sequence: u16,
    window: XResourceId,
    geometry: Rect,
    execution: Option<&Arc<Mutex<PrivateControlExecution>>>,
) -> Result<Vec<XClientEvent>, X11SetupSocketError> {
    let width = crate::dispatch::clamp_u16(geometry.width);
    let height = crate::dispatch::clamp_u16(geometry.height);
    let subscribers = routing
        .present_configure_subscribers(window)
        .map_err(|error| {
            X11SetupSocketError::new(format!(
                "failed to resolve Present ConfigureNotify subscriptions: {error}"
            ))
        })?;
    if crate::x11_authority_trace_enabled() {
        // A drawable's size reaches a GL client through this event, and a client
        // that never receives one can wait forever without erroring. Saying how
        // many subscribers it reached separates "nobody asked" from "asked and
        // was not told".
        tracing::info!(
            "sophia_x11_present_configure schema=1 status=routed window={:#x} width={width} height={height} subscribers={}",
            window.local.raw(),
            subscribers.len(),
        );
    }
    let events: Vec<_> = subscribers.into_iter().map(|(target, event_id)| (target, XClientEvent::PresentConfigureNotify {
            sequence,
            event_id,
            window,
            x: crate::dispatch::clamp_i16(geometry.x),
            y: crate::dispatch::clamp_i16(geometry.y),
            width,
            height,
            pixmap_width: width,
            pixmap_height: height,
            pixmap_flags: 0,
        })).collect();
    retain_private_control_events(execution, events.iter().map(|(target, event)| (Some(*target), *event)))?;
    let mut local_events = Vec::new();
    for (target, mut event) in events {
        if target == client {
            local_events.push(event);
        } else {
            set_x11_protocol_event_sequence(&mut event, 0);
            routing.route_control_protocol(target, event, execution).map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to route Present ConfigureNotify: {error}"
                ))
            })?;
        }
    }
    Ok(local_events)
}

#[cfg(unix)]
fn capture_clipboard_proxy_payload(
    state: &X11CoreSocketServerState,
    routing: &XServerFrontendRouteRegistry,
    namespace: NamespaceId,
    output: &mut XDispatchResult,
) -> Result<(), X11SetupSocketError> {
    let Some((index, requestor, property)) =
        output
            .outputs
            .iter()
            .enumerate()
            .find_map(|(index, output)| match output {
                crate::XClientOutput::Event(XClientEvent::SelectionNotify {
                    requestor,
                    property,
                    ..
                }) => Some((index, *requestor, *property)),
                _ => None,
            })
    else {
        return Ok(());
    };
    let mut runtime = state
        .runtime
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))?;
    if !runtime.is_clipboard_proxy(namespace, requestor) {
        return Ok(());
    }
    let mut properties = state
        .properties
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 property table lock poisoned"))?;
    let payload = runtime
        .capture_clipboard_source_payload(requestor, property, &mut properties)
        .map_err(|error| {
            X11SetupSocketError::new(format!(
                "failed to capture clipboard source payload: {error:?}"
            ))
        })?;
    routing
        .source_payload_sender
        .try_send(payload)
        .map_err(|error| match error {
            TrySendError::Full(_) => {
                X11SetupSocketError::new("clipboard source payload queue is full")
            }
            TrySendError::Disconnected(_) => {
                X11SetupSocketError::new("clipboard source payload queue is disconnected")
            }
        })?;
    trace_selection_transfer("portal_source_captured", true);
    output.outputs.remove(index);
    Ok(())
}

#[cfg(unix)]
fn route_selection_event(
    state: &X11CoreSocketServerState,
    routing: &XServerFrontendRouteRegistry,
    client: XServerFrontendClientId,
    output: &mut XDispatchResult,
) -> Result<(), X11SetupSocketError> {
    let Some((index, destination, event, stage, property_present)) =
        output
            .outputs
            .iter()
            .enumerate()
            .find_map(|(index, output)| match output {
                crate::XClientOutput::Event(
                    event @ XClientEvent::SelectionNotify {
                        requestor,
                        property,
                        ..
                    },
                ) => Some((
                    index,
                    *requestor,
                    *event,
                    "notify_routed",
                    *property != crate::X_ATOM_NONE,
                )),
                crate::XClientOutput::Event(
                    event @ XClientEvent::SelectionRequest { owner, .. },
                ) => Some((index, *owner, *event, "request_routed", true)),
                crate::XClientOutput::Event(
                    event @ XClientEvent::SelectionClear { owner, .. },
                ) => Some((index, *owner, *event, "owner_clear_routed", false)),
                _ => None,
            })
    else {
        return Ok(());
    };
    let Some(target) = state.client_for_resource(destination)? else {
        trace_selection_transfer("destination_missing", property_present);
        return Ok(());
    };
    if target == client {
        trace_selection_transfer("notify_local", property_present);
        return Ok(());
    }
    routing.route_protocol(target, event).map_err(|error| {
        X11SetupSocketError::new(format!("failed to route X11 protocol event: {error}"))
    })?;
    trace_selection_transfer(stage, property_present);
    output.outputs.remove(index);
    Ok(())
}

#[cfg(unix)]
fn route_property_events(
    routing: &XServerFrontendRouteRegistry,
    client: XServerFrontendClientId,
    output: &mut XDispatchResult,
) -> Result<(), X11SetupSocketError> {
    let property_events = output
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| match output {
            crate::XClientOutput::Event(event @ XClientEvent::PropertyNotify { window, .. }) => {
                Some((index, *window, *event))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut remove = Vec::new();
    for (index, window, event) in property_events {
        let subscribers = routing
            .property_change_subscribers(window)
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to inspect X11 property subscriptions: {error}"
                ))
            })?;
        for target in subscribers.iter().copied().filter(|target| *target != client) {
            routing.route_protocol(target, event).map_err(|error| {
                X11SetupSocketError::new(format!("failed to route X11 property event: {error}"))
            })?;
        }
        if subscribers.iter().any(|target| *target != client) {
            trace_selection_transfer("property_notify_routed", true);
        }
        if !subscribers.contains(&client) {
            remove.push(index);
        }
    }
    for index in remove.into_iter().rev() {
        output.outputs.remove(index);
    }
    Ok(())
}

#[cfg(unix)]
#[derive(Clone, Copy)]
struct XSelectionPropertyReadTrace {
    delete: bool,
    any_type: bool,
    complete_ceiling: bool,
}

#[cfg(unix)]
fn selection_property_read_trace(
    request: &crate::XWireRequest,
) -> Option<XSelectionPropertyReadTrace> {
    let crate::XWireRequest::GetProperty(read) = request else {
        return None;
    };
    (read.delete && read.long_offset == 0).then_some(XSelectionPropertyReadTrace {
        delete: read.delete,
        any_type: read.property_type == crate::X_PROPERTY_ANY_TYPE,
        complete_ceiling: read.long_length > crate::X_PROPERTY_MAX_VALUE_BYTES as u32,
    })
}

#[cfg(unix)]
fn trace_selection_property_read_result(
    trace: Option<XSelectionPropertyReadTrace>,
    output: &XDispatchResult,
) {
    if std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_none() {
        return;
    }
    let Some(trace) = trace else {
        return;
    };
    let reply = output.outputs.iter().find_map(|output| match output {
        crate::XClientOutput::Reply(crate::XClientReply::GetProperty {
            property_type,
            format,
            bytes_after,
            item_count,
            ..
        }) => Some((*property_type, *format, *bytes_after, *item_count)),
        _ => None,
    });
    let error = output
        .outputs
        .iter()
        .any(|output| matches!(output, crate::XClientOutput::Error(_)));
    match reply {
        Some((property_type, format, bytes_after, item_count)) => tracing::info!(
            "sophia_x11_selection_transfer schema=1 stage=property_read result=reply delete={} any_type={} complete_ceiling={} type_present={} format={} items={} bytes_after={} content=redacted",
            trace.delete,
            trace.any_type,
            trace.complete_ceiling,
            property_type != crate::X_PROPERTY_ANY_TYPE,
            format,
            item_count,
            bytes_after,
        ),
        None => tracing::info!(
            "sophia_x11_selection_transfer schema=1 stage=property_read result={} delete={} any_type={} complete_ceiling={} content=redacted",
            if error { "error" } else { "missing" },
            trace.delete,
            trace.any_type,
            trace.complete_ceiling,
        ),
    }
}

#[cfg(unix)]
fn trace_selection_transfer(stage: &str, property_present: bool) {
    if std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_some() {
        tracing::info!(
            "sophia_x11_selection_transfer schema=1 stage={} property_present={} content=redacted",
            stage,
            property_present,
        );
    }
}
