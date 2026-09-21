// `_NET_ACTIVE_WINDOW`, after the focus moved.
//
// The runtime notes each focus change and holds no property table; whoever
// holds the table calls this once the runtime guard's work is done. Three
// callers: request dispatch after every request, the connection thread after
// a private focus claim, and the control writer after a session focus
// command, under its own locks. A publication that fails is warned about and
// never fails the request: the focus already moved, and a root that says the
// old window is a lesser wrong than a client refused for it.

pub(crate) fn publish_noted_focus(
    runtime: &mut XAuthorityRuntime,
    properties: &mut XPropertyTable,
    atoms: &mut XAtomTable,
    byte_order: XByteOrder,
) {
    for (namespace, window) in runtime.take_active_window_changes() {
        if let Err(error) =
            crate::publish_active_window(properties, atoms, namespace, byte_order, window)
        {
            tracing::warn!(
                "sophia_x11_active_window status=unpublished namespace={} error={error:?}",
                namespace.raw()
            );
        }
    }
}
