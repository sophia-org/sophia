// Where a routed event is delivered, for the input writer: the pointer's
// coordinates for a key, the selection a pointer event answers to, the
// registry's propagation stop, and a core event's child. Included by
// control_writer.rs beside writers/input.rs; one module with it (t220).

/// Where the pointer is when a key is delivered: a key event carries the
/// pointer's root and event-window coordinates (XTS Xlib11 KeyPress 1,
/// KeyRelease 1). Nothing observed yet reads as the origin.
#[cfg(unix)]
fn key_pointer_coordinates(
    input_authority: Option<&Arc<Mutex<crate::XInputAuthorityState>>>,
    core_event_selections: &Arc<Mutex<XCoreEventSelectionState>>,
    namespace: NamespaceId,
    delivered_window: XResourceId,
) -> Result<(i16, i16, i16, i16), X11SetupSocketError> {
    let Some(authority) = input_authority else {
        return Ok((0, 0, 0, 0));
    };
    let position = authority
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 input authority lock poisoned"))?
        .pointer_query_state(namespace)
        .position;
    let Some(pointer) = position else {
        return Ok((0, 0, 0, 0));
    };
    let (event_x, event_y) = core_event_selections
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 core event selection lock poisoned"))?
        .pointer_event_coordinates(
            pointer.surface_window,
            delivered_window,
            i16::try_from(pointer.local_x).unwrap_or(i16::MAX),
            i16::try_from(pointer.local_y).unwrap_or(i16::MAX),
        );
    Ok((pointer.root_x, pointer.root_y, event_x, event_y))
}

/// Whether this client's selection at `target` is told a button event the
/// registry stopped at `stop`: at the stop, or nearer the pointer than it.
/// The stop is decided from the pointer window the owner's writer last
/// resolved, so a press routed before the motion that moved the pointer
/// was resolved carries a stop from higher up, or off the path; a nearer
/// selection of this client's is still nearer, and a stop off the path
/// decides nothing (t220).
fn within_propagation_stop(
    ancestry: &[XResourceId],
    stop: Option<XResourceId>,
    target: XResourceId,
) -> bool {
    let Some(stop_depth) = stop.and_then(|stop| ancestry.iter().position(|window| *window == stop))
    else {
        return true;
    };
    ancestry
        .iter()
        .position(|window| *window == target)
        .is_none_or(|depth| depth <= stop_depth)
}

/// The selection half a core pointer event answers to.
fn pointer_selection(kind: XAuthorityPointerEventKind) -> XPointerSelection {
    match kind {
        XAuthorityPointerEventKind::Motion => XPointerSelection::Motion,
        XAuthorityPointerEventKind::Button { pressed: true, .. }
        | XAuthorityPointerEventKind::Axis { pressed: true, .. } => XPointerSelection::Press,
        XAuthorityPointerEventKind::Button { pressed: false, .. }
        | XAuthorityPointerEventKind::Axis { pressed: false, .. } => XPointerSelection::Release,
    }
}

/// The `child` of a core event: the element of the source window's ancestry
/// (source first, then up) directly below the event window, or None when the
/// source is the event window itself or not an inferior of it.
fn x11_event_subwindow(source_ancestry: &[XResourceId], event_window: XResourceId) -> XResourceId {
    match source_ancestry.iter().position(|window| *window == event_window) {
        Some(0) | None => XResourceId::NONE,
        Some(depth) => source_ancestry[depth - 1],
    }
}
