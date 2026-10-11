// A selection requestor is an authority-owned endpoint, not a drawable in
// the Engine scene. Answer its inspection here without teaching renderers
// that a proxy is a client window or granting generic drawable access.
fn validate_clipboard_property_window(context: XDispatchContext,
    runtime: &XAuthorityRuntime, window: XResourceId) -> Result<(), XAuthorityRuntimeError> {
    if runtime.clipboard_proxy(window).is_some_and(|proxy|
        proxy.namespace == context.namespace && proxy.client == Some(context.client_id)) {
        return Ok(());
    }
    runtime.validate_window_access(context.namespace, window)
}

fn dispatch_clipboard_proxy(
    context: XDispatchContext,
    request: XWireRequest,
    runtime: &XAuthorityRuntime,
    properties: &XPropertyTable,
) -> XDispatchFamilyResult {
    use crate::XCoreRequest as Core;
    let window = match &request {
        XWireRequest::Core(
            Core::GetWindowAttributes { window }
            | Core::QueryTree { window }
            | Core::ChangeWindowAttributes { window, .. }
            | Core::DestroyWindow { window }
            | Core::ReparentWindow { window, .. }
            | Core::UnmapWindow { window }
            | Core::ConfigureWindow { window, .. }
            | Core::DeleteProperty { window, .. }
            | Core::ListProperties { window },
        ) => *window,
        XWireRequest::Core(Core::GetGeometry { drawable }) => *drawable,
        XWireRequest::Core(Core::ChangeProperty(change)) => change.window,
        XWireRequest::Core(Core::GetProperty(read)) => read.window,
        XWireRequest::Core(Core::SendSelectionNotify { destination, .. }) => *destination,
        _ => return Unhandled(request),
    };
    let Some(proxy) = runtime.clipboard_proxy(window) else {
        return Unhandled(request);
    };
    let result = |outputs| Handled(XDispatchResult {
        response: None,
        outputs,
        metadata_candidates: Vec::new(),
    });
    let refuse = || result(vec![XClientOutput::Error(crate::XClientError {
        code: XErrorCode::BadAccess,
        sequence: context.sequence,
        resource_id: window.local.raw() as u32,
        minor_code: 0,
        major_code: context.major_opcode,
    })]);
    if proxy.namespace != context.namespace || proxy.client != Some(context.client_id) {
        return refuse();
    }
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let reply = match &request {
        XWireRequest::Core(Core::GetGeometry { .. }) => XClientReply::GetGeometry {
            sequence: context.sequence,
            depth: 24,
            root,
            geometry: Rect { x: 0, y: 0, width: 1, height: 1 },
            border_width: 0,
        },
        XWireRequest::Core(Core::QueryTree { .. }) => XClientReply::QueryTree {
            sequence: context.sequence,
            root,
            parent: root,
            children: Vec::new(),
        },
        XWireRequest::Core(Core::GetWindowAttributes { .. }) => XClientReply::GetWindowAttributes {
            sequence: context.sequence,
            visual: X_SETUP_DEFAULT_VISUAL,
            colormap: XResourceId::new(u64::from(X_SETUP_DEFAULT_COLORMAP), 1),
            colormap_installed: false,
            map_state: 0,
            override_redirect: true,
            bit_gravity: 0,
            win_gravity: 1,
        },
        XWireRequest::Core(Core::ChangeWindowAttributes {
            background_pixmap: None, background_pixel: None, border_pixmap: None,
            border_pixel: None, override_redirect: None, cursor: None,
            bit_gravity: None, win_gravity: None, colormap: None,
            event_mask, do_not_propagate_mask,
            ..
        }) if event_mask.is_none_or(|mask| mask & !((1 << 22) | (1 << 17)) == 0)
            && do_not_propagate_mask.is_none_or(|mask| mask == 0) => {
                // The socket owner records the accepted per-client event mask.
                return result(Vec::new());
            }
        XWireRequest::Core(Core::ChangeProperty(change)) if change.property == proxy.property => {
            let retained = if change.mode == crate::XPropertyMode::Replace { 0 } else {
                properties.get(proxy.namespace, window, proxy.property).map_or(0, |p| p.bytes.len())
            };
            if retained.checked_add(change.bytes.len())
                .is_none_or(|size| size > crate::MAX_CLIPBOARD_TEXT_HANDOFF_BYTES) {
                return refuse();
            }
            return Unhandled(request);
        }
        XWireRequest::Core(Core::GetProperty(read)) if read.property == proxy.property => {
            return Unhandled(request);
        }
        XWireRequest::Core(Core::DeleteProperty { property, .. }) if *property == proxy.property => {
            return Unhandled(request);
        }
        XWireRequest::Core(Core::SendSelectionNotify { event: XClientEvent::SelectionNotify {
            requestor, selection, target, time, property, ..
        }, event_mask: 0, .. }) if *requestor == window && *selection == proxy.selection
            && *target == proxy.target && *time == proxy.time
            && (*property == proxy.property || *property == X_ATOM_NONE) => {
                return Unhandled(request);
            }
        _ => return refuse(),
    };
    result(vec![XClientOutput::Reply(reply)])
}
