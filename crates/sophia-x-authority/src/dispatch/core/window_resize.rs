// What a resize owes beyond the resized window's own ConfigureNotify: its
// children moved by their win-gravity (t199), and its contents under its
// bit-gravity (t215). Included by dispatch.rs beside the other core
// families.

/// The GravityNotify and UnmapNotify a resize owes its children: each child
/// moved by its win-gravity is told where it is now, and each with
/// UnmapGravity is unmapped, as dix does it (t199). The parent's copy of
/// each is derived by the routing, as for the other structure events.
fn gravity_outputs(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    parent: XResourceId,
    before: Rect,
    after: Rect,
    outputs: &mut Vec<XClientOutput>,
    surfaces: &mut Vec<sophia_protocol::AuthoritySurface>,
) {
    for (child, outcome) in
        runtime.apply_win_gravity(context.namespace, parent, before, after, u64::from(context.sequence))
    {
        match outcome {
            crate::XGravityOutcome::Moved { x, y } => {
                outputs.push(XClientOutput::Event(XClientEvent::GravityNotify {
                    sequence: context.sequence,
                    event: child,
                    window: child,
                    x: clamp_i16(x),
                    y: clamp_i16(y),
                }));
            }
            crate::XGravityOutcome::Unmap => {
                if let Ok(surface) = runtime.unmap_window(context.namespace, child) {
                    surfaces.extend(surface);
                    outputs.push(XClientOutput::Event(XClientEvent::UnmapNotify {
                        sequence: context.sequence,
                        event: child,
                        window: child,
                        from_configure: true,
                    }));
                }
            }
        }
    }
}

/// The resized window's contents under its bit-gravity, presented, and an
/// Expose for each rectangle the old contents no longer cover, as dix does
/// for a viewable window (t215). The presentation's packet is returned for
/// the caller's response.
fn contents_outputs(
    context: XDispatchContext,
    runtime: &mut XAuthorityRuntime,
    window: XResourceId,
    before: Rect,
    after: Rect,
    outputs: &mut Vec<XClientOutput>,
) -> Option<XAuthorityResponsePacket> {
    let (uncovered, presented) =
        runtime.resize_window_contents(context.transaction, context.namespace, window, before, after);
    if runtime.window_map_state(context.namespace, window) == Ok(crate::XMapState::Viewable) {
        let count = uncovered.len();
        for (index, rect) in uncovered.into_iter().enumerate() {
            outputs.push(XClientOutput::Event(XClientEvent::Expose {
                sequence: context.sequence,
                window,
                x: clamp_u16(rect.x),
                y: clamp_u16(rect.y),
                width: clamp_u16(rect.width),
                height: clamp_u16(rect.height),
                count: clamp_u16(i32::try_from(count - 1 - index).unwrap_or(0)),
            }));
        }
    }
    presented
}
