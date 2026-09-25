// KillClient and a departing client's save-set, acted on by the socket
// layer, which owns the leases and the sockets (t166).

/// KillClient. A live owner of `resource` is disconnected: its own thread
/// tears its resources down, honouring its own close-down mode. A resource
/// in a retained range frees that range now. AllTemporary frees every
/// retained temporary range. A resource nobody holds is the Value error the
/// protocol names. Returns whether the requester ended itself.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn apply_x11_kill_client(
    state: &X11CoreSocketServerState,
    routing: Option<&XServerFrontendRouteRegistry>,
    client: XServerFrontendClientId,
    resource: Option<crate::XResourceId>,
    transaction: TransactionId,
    sequence: u16,
    major_opcode: u8,
    output: &mut XDispatchResult,
    released_dma_bufs: &mut Vec<sophia_protocol::BufferHandle>,
    released_fences: &mut Vec<sophia_protocol::FenceHandle>,
) -> Result<bool, X11SetupSocketError> {
    let Some(resource) = resource else {
        for retained in state.take_retained_temporary_ranges()? {
            free_x11_retained_range(state, routing, retained, transaction, output, released_dma_bufs, released_fences)?;
        }
        return Ok(false);
    };
    if let Some(owner) = state.client_for_resource(resource)? {
        if owner == client {
            // Ending oneself: the connection's own teardown follows this
            // request; nothing else is owed.
            return Ok(true);
        }
        // The standalone server serves one connection: a live owner other
        // than the requester exists only on the routed service.
        if let Some(routing) = routing {
            routing
                .input_recovery
                .disconnect(owner, XAuthorityInputDeliveryOutcome::ClientDisconnected)
                .map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to kill X11 client {}: {error}",
                        owner.raw()
                    ))
                })?;
        }
        return Ok(false);
    }
    if let Some(retained) = state.take_retained_range_for_resource(resource)? {
        free_x11_retained_range(state, routing, retained, transaction, output, released_dma_bufs, released_fences)?;
        return Ok(false);
    }
    output.outputs.push(crate::XClientOutput::Error(crate::XClientError {
        code: crate::XErrorCode::BadValue,
        sequence,
        resource_id: u32::try_from(resource.local.raw()).unwrap_or(0),
        minor_code: 0,
        major_code: major_opcode,
    }));
    Ok(false)
}

/// Frees a retained range as the departing client's teardown would have:
/// the same release, its surfaces shipped with this request's response and
/// its destroys routed to whoever watched them.
#[cfg(unix)]
fn free_x11_retained_range(
    state: &X11CoreSocketServerState,
    routing: Option<&XServerFrontendRouteRegistry>,
    retained: XRetainedClientRange,
    transaction: TransactionId,
    output: &mut XDispatchResult,
    released_dma_bufs: &mut Vec<sophia_protocol::BufferHandle>,
    released_fences: &mut Vec<sophia_protocol::FenceHandle>,
) -> Result<(), X11SetupSocketError> {
    let lease = XServerFrontendClientLease {
        client: retained.client,
        resource_id_range: retained.range,
        close_down_mode: crate::XCloseDownMode::Destroy,
    };
    let release = release_x11_client_lease_with_control(state, retained.namespace, lease, &[], None)?;
    released_dma_bufs.extend(release.released_dma_bufs.iter().copied());
    released_fences.extend(release.released_fences.iter().copied());
    if !release.removed_surfaces.is_empty() {
        let response = output
            .response
            .get_or_insert_with(|| crate::XAuthorityResponsePacket::accepted(transaction));
        response.removed_surfaces.extend(release.removed_surfaces.iter().copied());
    }
    if let Some(routing) = routing {
        route_x11_colormap_changes(routing, retained.namespace, &release.colormap_changes)?;
        route_x11_retained_destroys(routing, &release.destroyed_windows)?;
    }
    Ok(())
}

/// DestroyNotify for each window a freed retained range destroyed, to the
/// window's StructureNotify selectors and its parent's SubstructureNotify
/// ones, before their subscriptions are retired.
#[cfg(unix)]
fn route_x11_retained_destroys(
    routing: &XServerFrontendRouteRegistry,
    destroyed: &[crate::XResourceId],
) -> Result<(), X11SetupSocketError> {
    const STRUCTURE_NOTIFY_MASK: u32 = 1 << 17;
    const SUBSTRUCTURE_NOTIFY_MASK: u32 = 1 << 19;
    for window in destroyed {
        let parent = routing.window_parent(*window).map_err(|error| {
            X11SetupSocketError::new(format!("failed to resolve a freed X11 window's parent: {error}"))
        })?;
        for (target, mask) in [(Some(*window), STRUCTURE_NOTIFY_MASK), (parent, SUBSTRUCTURE_NOTIFY_MASK)] {
            let Some(target) = target else { continue };
            for recipient in routing.core_event_subscribers(target, mask).map_err(|error| {
                X11SetupSocketError::new(format!("failed to inspect freed X11 subscriptions: {error}"))
            })? {
                routing
                    .route_protocol_contained(
                        recipient,
                        crate::XClientEvent::DestroyNotify { sequence: 0, event: target, window: *window },
                    )
                    .map_err(|error| X11SetupSocketError::new(format!("failed to route a freed X11 destroy: {error}")))?;
            }
        }
    }
    for window in destroyed {
        routing.remove_xfixes_selection_window(*window).map_err(|error| {
            X11SetupSocketError::new(format!("failed to retire a freed window's XFixes subscriptions: {error}"))
        })?;
        routing.remove_core_event_window(*window).map_err(|error| {
            X11SetupSocketError::new(format!("failed to retire a freed window's subscriptions: {error}"))
        })?;
    }
    Ok(())
}

/// The events a save-set window is owed: for a reparented window UnmapNotify
/// if it was mapped, then ReparentNotify to it and both parents'
/// SubstructureNotify selectors; then MapNotify, since the walk leaves every
/// saved window mapped, with VisibilityNotify and Expose when it became
/// viewable, as MapWindow reports them. The registry's parent map follows.
#[cfg(unix)]
fn route_x11_save_set_reparents(
    routing: &XServerFrontendRouteRegistry,
    reparents: &[crate::XSaveSetReparent],
) -> Result<(), X11SetupSocketError> {
    const EXPOSURE_MASK: u32 = 1 << 15;
    const VISIBILITY_CHANGE_MASK: u32 = 1 << 16;
    const STRUCTURE_NOTIFY_MASK: u32 = 1 << 17;
    const SUBSTRUCTURE_NOTIFY_MASK: u32 = 1 << 19;
    let deliver = |targets: &[(crate::XResourceId, u32)], event: crate::XClientEvent| -> Result<(), X11SetupSocketError> {
        for (target, mask) in targets {
            for recipient in routing.core_event_subscribers(*target, *mask).map_err(|error| {
                X11SetupSocketError::new(format!("failed to inspect save-set subscriptions: {error}"))
            })? {
                routing.route_protocol_contained(recipient, event).map_err(|error| {
                    X11SetupSocketError::new(format!("failed to route a save-set event: {error}"))
                })?;
            }
        }
        Ok(())
    };
    for reparent in reparents {
        let window = reparent.window;
        let reparented = reparent.new_parent != reparent.old_parent;
        if reparented {
            if reparent.was_mapped {
                deliver(
                    &[(window, STRUCTURE_NOTIFY_MASK), (reparent.old_parent, SUBSTRUCTURE_NOTIFY_MASK)],
                    crate::XClientEvent::UnmapNotify { sequence: 0, event: window, window, from_configure: false },
                )?;
            }
            routing.update_window_parent(window, reparent.new_parent).map_err(|error| {
                X11SetupSocketError::new(format!("failed to record a save-set reparent: {error}"))
            })?;
            deliver(
                &[
                    (window, STRUCTURE_NOTIFY_MASK),
                    (reparent.old_parent, SUBSTRUCTURE_NOTIFY_MASK),
                    (reparent.new_parent, SUBSTRUCTURE_NOTIFY_MASK),
                ],
                crate::XClientEvent::ReparentNotify {
                    sequence: 0,
                    event: window,
                    window,
                    parent: reparent.new_parent,
                    x: reparent.x,
                    y: reparent.y,
                    override_redirect: reparent.override_redirect,
                },
            )?;
        }
        deliver(
            &[(window, STRUCTURE_NOTIFY_MASK), (reparent.new_parent, SUBSTRUCTURE_NOTIFY_MASK)],
            crate::XClientEvent::MapNotify { sequence: 0, event: window, window, override_redirect: reparent.override_redirect },
        )?;
        if let Some(surface) = reparent.surface.as_ref().filter(|_| !reparent.input_only) {
            let clamp = |value: i32| u16::try_from(value).unwrap_or(u16::MAX);
            deliver(
                &[(window, VISIBILITY_CHANGE_MASK)],
                crate::XClientEvent::VisibilityNotify { sequence: 0, window, state: 0 },
            )?;
            deliver(
                &[(window, EXPOSURE_MASK)],
                crate::XClientEvent::Expose {
                    sequence: 0,
                    window,
                    x: 0,
                    y: 0,
                    width: clamp(surface.geometry.width),
                    height: clamp(surface.geometry.height),
                    count: 0,
                },
            )?;
        }
    }
    Ok(())
}
