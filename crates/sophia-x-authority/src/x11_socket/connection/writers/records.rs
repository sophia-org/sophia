// Property and configure records describing one surface to one client.
//
// Split from the writer threads that emit them: these are pure functions from
// Engine-visible state to X11 wire records, with no socket, no lock, and no
// thread. Included rather than declared as a module so the enclosing scope's
// imports and helpers stay reachable, which is the same arrangement
// `writers/input.rs` already uses.

#[cfg(unix)]
#[expect(clippy::too_many_arguments, reason = "the existing geometry projection needs its original control custody through fallible peer routing")]
fn x11_surface_geometry_records(
    byte_order: XByteOrder,
    event_sequence: u16,
    client: XServerFrontendClientId,
    window: XResourceId,
    geometry: Rect,
    admit: bool,
    map_transition: Option<&XCoreMapTransition>,
    present_configure: bool,
    selections: &XCoreEventSelectionState,
    protocol_routing: Option<&XServerFrontendRouteRegistry>,
    execution: Option<&Arc<Mutex<PrivateControlExecution>>>,
) -> Result<Vec<Vec<u8>>, X11SetupSocketError> {
    let width = u16::try_from(geometry.width)
        .map_err(|_| X11SetupSocketError::new("X11 control geometry width is invalid"))?;
    let height = u16::try_from(geometry.height)
        .map_err(|_| X11SetupSocketError::new("X11 control geometry height is invalid"))?;
    let present_events = protocol_routing
        .filter(|_| present_configure)
        .map(|routing| route_x11_present_configure_with_control(routing, client, event_sequence, window, geometry, execution))
        .transpose()?
        .unwrap_or_default();
    let mut records = Vec::with_capacity(
        present_events.len()
            + if admit {
                4 + map_transition.map_or(0, |transition| {
                    transition.promoted_descendants.len().saturating_mul(2)
                })
            } else {
                1
            },
    );
    records.extend(
        present_events
            .into_iter()
            .map(|event| encode_x_client_event(byte_order, event)),
    );
    let mut core_events = Vec::new();
    const EXPOSURE_MASK: u32 = 1 << 15;
    const VISIBILITY_CHANGE_MASK: u32 = 1 << 16;
    const STRUCTURE_NOTIFY_MASK: u32 = 1 << 17;
    if protocol_routing.is_some() || selections.selects(window, STRUCTURE_NOTIFY_MASK) {
        core_events.push(XClientEvent::ConfigureNotify {
            sequence: event_sequence,
            synthetic: false,
            event: window,
            window,
            above_sibling: None,
            x: clamp_engine_i16(geometry.x),
            y: clamp_engine_i16(geometry.y),
            width,
            height,
            border_width: 0,
            override_redirect: false,
        });
    }
    if admit {
        if protocol_routing.is_some() || selections.selects(window, STRUCTURE_NOTIFY_MASK) {
            core_events.push(XClientEvent::MapNotify {
                sequence: event_sequence,
                event: window,
                window,
                override_redirect: false,
            });
        }
        let viewable_windows = std::iter::once(window).chain(
            map_transition
                .into_iter()
                .flat_map(|transition| transition.promoted_descendants.iter().copied()),
        );
        for candidate in viewable_windows.clone() {
            if protocol_routing.is_some() || selections.selects(candidate, VISIBILITY_CHANGE_MASK) {
                core_events.push(XClientEvent::VisibilityNotify {
                    sequence: event_sequence,
                    window: candidate,
                    state: 0,
                });
            }
        }
        for candidate in viewable_windows {
            if protocol_routing.is_none() && !selections.selects(candidate, EXPOSURE_MASK) {
                continue;
            }
            let candidate_geometry = selections.geometry(candidate).unwrap_or(geometry);
            core_events.push(XClientEvent::Expose {
                sequence: event_sequence,
                window: candidate,
                x: 0,
                y: 0,
                width: crate::dispatch::clamp_u16(candidate_geometry.width),
                height: crate::dispatch::clamp_u16(candidate_geometry.height),
                count: 0,
            });
        }
    }
    if let Some(routing) = protocol_routing {
        let mut output = crate::XDispatchResult {
            response: None,
            outputs: core_events
                .into_iter()
                .map(crate::XClientOutput::Event)
                .collect(),
            metadata_candidates: Vec::new(),
        };
        route_core_lifecycle_events_with_control(routing, client, &mut output, execution)?;
        core_events = output
            .outputs
            .into_iter()
            .filter_map(|output| match output {
                crate::XClientOutput::Event(event) => Some(event),
                _ => None,
            })
            .collect();
    }
    records.extend(
        core_events
            .into_iter()
            .map(|event| encode_x_client_event(byte_order, event)),
    );
    Ok(records)
}

#[cfg(unix)]
fn x11_presentation_property_records(
    byte_order: XByteOrder,
    sequence: u16,
    client: XServerFrontendClientId,
    window: XResourceId,
    changed: &[crate::XAtom],
    selections: &XCoreEventSelectionState,
    protocol_routing: Option<&XServerFrontendRouteRegistry>,
    execution: Option<&Arc<Mutex<PrivateControlExecution>>>,
) -> Result<Vec<Vec<u8>>, X11SetupSocketError> {
    const PROPERTY_CHANGE_MASK: u32 = 1 << 22;
    let mut records = Vec::with_capacity(changed.len());
    retain_private_control_events(execution, changed.iter().map(|atom| (None, XClientEvent::PropertyNotify {
        sequence, window, atom: *atom, time: 0, new_value: true,
    })))?;
    for atom in changed {
        let event = XClientEvent::PropertyNotify {
            sequence,
            window,
            atom: *atom,
            time: 0,
            new_value: true,
        };
        let local_selected = if let Some(routing) = protocol_routing {
            let subscribers = routing.property_change_subscribers(window).map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to inspect presentation property subscriptions: {error:?}"
                ))
            })?;
            for target in subscribers.iter().copied().filter(|target| *target != client) {
                retain_private_control_events(execution, [(Some(target), event)])?;
                routing.route_protocol(target, event).map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to route presentation property notification: {error:?}"
                    ))
                })?;
            }
            subscribers.contains(&client)
        } else {
            selections.selects(window, PROPERTY_CHANGE_MASK)
        };
        if local_selected {
            records.push(encode_x_client_event(byte_order, event));
        }
    }
    Ok(records)
}
