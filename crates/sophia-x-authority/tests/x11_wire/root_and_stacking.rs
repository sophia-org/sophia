// What the root and a window's stacking mean for pixels: a namespace's
// private root and its readback (t181, t185), and what the screen shows
// where windows overlap (t188).

/// A root readback shows what covers the root for the reader's namespace:
/// its own windows, never another namespace's. Before t185 the readback
/// composited every namespace's windows, so a confined client could read
/// another's pixels off the root.
#[test]
fn a_root_readback_shows_only_the_readers_namespace() {
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let reader = NamespaceId::from_raw(0x5001);
    let owner = NamespaceId::from_raw(0x5002);
    let order = XByteOrder::LittleEndian;
    let mut send = |ns: NamespaceId, seq: u16, op: u8, bytes: Vec<u8>, runtime: &mut XAuthorityRuntime| {
        let request = decode_x11_core_request(context(ns, u64::from(seq), order), &bytes).unwrap();
        dispatch_x11_wire_request(
            dispatch_context(ns, seq, order, op),
            request,
            runtime,
            &mut atoms,
            &mut properties,
        )
    };
    send(owner, 1, 1, create_window_request(order, 0x400001, 10, 10, 20, 20), &mut runtime);
    send(owner, 2, 8, map_window_request(order, 0x400001), &mut runtime);
    send(
        owner,
        3,
        55,
        create_gc_values_request(order, 0x400002, 0x400001, 3, u32::MAX, 0x00c0_ffee, 0, 0, 0),
        &mut runtime,
    );
    send(
        owner,
        4,
        70,
        poly_fill_rectangle_request(order, 0x400001, 0x400002, &[(0, 0, 20, 20)]),
        &mut runtime,
    );
    let pixel = |result: XDispatchResult| match result.outputs.as_slice() {
        [XClientOutput::Reply(XClientReply::GetImage { data, .. })] => {
            u32::from_le_bytes(data[..4].try_into().unwrap())
        }
        other => panic!("unexpected GetImage result: {other:?}"),
    };
    let own = send(owner, 5, 73, get_image_request(order, 2, 0x20, 15, 15, 1, 1, u32::MAX), &mut runtime);
    assert_eq!(pixel(own), 0x00c0_ffee, "a namespace reads its own window off the root");
    let other = send(reader, 6, 73, get_image_request(order, 2, 0x20, 15, 15, 1, 1, u32::MAX), &mut runtime);
    assert_eq!(pixel(other), 0, "and another namespace's window is not there to read");
}

/// Drawing on the root lands in a root private to the drawing client's
/// namespace: read back there, invisible to other namespaces, and never
/// presented, because the Engine and the shell own the desktop (t181).
#[test]
fn a_draw_on_the_root_lands_in_the_namespaces_private_root() {
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let drawer = NamespaceId::from_raw(0x5003);
    let other = NamespaceId::from_raw(0x5004);
    let order = XByteOrder::LittleEndian;
    let mut send = |ns: NamespaceId, seq: u16, op: u8, bytes: Vec<u8>, runtime: &mut XAuthorityRuntime| {
        let request = decode_x11_core_request(context(ns, u64::from(seq), order), &bytes).unwrap();
        dispatch_x11_wire_request(
            dispatch_context(ns, seq, order, op),
            request,
            runtime,
            &mut atoms,
            &mut properties,
        )
    };
    send(
        drawer,
        1,
        55,
        create_gc_values_request(order, 0x500002, 0x20, 3, u32::MAX, 0x0012_3456, 0, 0, 0),
        &mut runtime,
    );
    let fill = send(
        drawer,
        2,
        70,
        poly_fill_rectangle_request(order, 0x20, 0x500002, &[(4, 4, 8, 8)]),
        &mut runtime,
    );
    assert!(fill.outputs.is_empty(), "no error: {:?}", fill.outputs);
    let response = fill.response.expect("a response");
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert!(response.transactions.is_empty(), "the private root is never presented");
    let pixel = |result: XDispatchResult| match result.outputs.as_slice() {
        [XClientOutput::Reply(XClientReply::GetImage { data, .. })] => {
            u32::from_le_bytes(data[..4].try_into().unwrap())
        }
        other => panic!("unexpected GetImage result: {other:?}"),
    };
    let own = send(drawer, 3, 73, get_image_request(order, 2, 0x20, 6, 6, 1, 1, u32::MAX), &mut runtime);
    assert_eq!(pixel(own), 0x0012_3456, "the drawer reads its root back");
    let theirs = send(other, 4, 73, get_image_request(order, 2, 0x20, 6, 6, 1, 1, u32::MAX), &mut runtime);
    assert_eq!(pixel(theirs), 0, "another namespace's root is its own");
}

/// Replays the presentation updates into one image per drawable and reads a
/// pixel of `drawable` back from it.
fn presented_pixel(
    presented: &mut std::collections::BTreeMap<XResourceId, (i32, Vec<u8>)>,
    updates: Vec<sophia_x_authority::XAuthorityCpuBufferUpdate>,
    drawable: XResourceId,
    x: i32,
    y: i32,
) -> u32 {
    use sophia_x_authority::XAuthorityCpuBufferUpdate as Update;
    let patch = |presented: &mut std::collections::BTreeMap<XResourceId, (i32, Vec<u8>)>,
                 drawable: XResourceId,
                 rect: Rect,
                 bytes: &[u8]| {
        let (width, image) = presented.get_mut(&drawable).expect("a replace precedes a patch");
        for row in 0..rect.height {
            let from = (row * rect.width * 4) as usize;
            let to = (((rect.y + row) * *width + rect.x) * 4) as usize;
            let len = (rect.width * 4) as usize;
            image[to..to + len].copy_from_slice(&bytes[from..from + len]);
        }
    };
    for update in updates {
        match update {
            Update::Replace(snapshot) => {
                presented.insert(snapshot.drawable, (snapshot.size.width, snapshot.bytes.to_vec()));
            }
            Update::Patch(one) => patch(presented, one.drawable, one.rect, &one.bytes),
            Update::PatchBatch(batch) => {
                for region in &batch.patches {
                    patch(presented, batch.drawable, region.rect, &region.bytes);
                }
            }
        }
    }
    let (width, image) = &presented[&drawable];
    let at = ((y * width + x) * 4) as usize;
    u32::from_le_bytes(image[at..at + 4].try_into().unwrap()) & 0x00ff_ffff
}

/// A mapped child covers its parent on screen, so a later draw on the parent
/// shows only around the child, never over it (t188).
#[test]
fn a_draw_on_a_parent_leaves_its_mapped_child_on_top() {
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let ns = NamespaceId::from_raw(0x5005);
    let order = XByteOrder::LittleEndian;
    let mut send = |seq: u16, op: u8, bytes: Vec<u8>, runtime: &mut XAuthorityRuntime| {
        let request = decode_x11_core_request(context(ns, u64::from(seq), order), &bytes).unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(ns, seq, order, op),
            request,
            runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(
            !result.outputs.iter().any(|output| matches!(output, XClientOutput::Error(_))),
            "no error: {:?}",
            result.outputs
        );
    };
    send(1, 1, create_window_request(order, 0x600001, 0, 0, 40, 40), &mut runtime);
    send(2, 1, create_window_request_with_parent(order, 0x600002, 0x600001, 10, 10, 10, 10), &mut runtime);
    send(3, 8, map_window_request(order, 0x600002), &mut runtime);
    send(4, 8, map_window_request(order, 0x600001), &mut runtime);
    send(5, 55, create_gc_values_request(order, 0x600003, 0x600001, 3, u32::MAX, 0x0000_ff00, 0, 0, 0), &mut runtime);
    send(6, 55, create_gc_values_request(order, 0x600004, 0x600001, 3, u32::MAX, 0x0000_00ff, 0, 0, 0), &mut runtime);
    send(7, 70, poly_fill_rectangle_request(order, 0x600002, 0x600003, &[(0, 0, 10, 10)]), &mut runtime);
    send(8, 70, poly_fill_rectangle_request(order, 0x600001, 0x600004, &[(0, 0, 40, 40)]), &mut runtime);
    let toplevel = XResourceId::new(0x600001, 1);
    let mut presented = std::collections::BTreeMap::new();
    let updates = runtime.take_cpu_buffer_updates();
    assert_eq!(presented_pixel(&mut presented, updates.clone(), toplevel, 15, 15), 0x0000_ff00, "the child stays on top");
    assert_eq!(presented_pixel(&mut presented, Vec::new(), toplevel, 5, 5), 0x0000_00ff, "the parent shows around it");
    // A grandchild overhanging the child shows only inside the child.
    send(9, 1, create_window_request_with_parent(order, 0x600005, 0x600002, 5, 5, 10, 10), &mut runtime);
    send(10, 8, map_window_request(order, 0x600005), &mut runtime);
    send(14, 55, create_gc_values_request(order, 0x600006, 0x600001, 3, u32::MAX, 0x00ff_0000, 0, 0, 0), &mut runtime);
    send(11, 70, poly_fill_rectangle_request(order, 0x600005, 0x600006, &[(0, 0, 10, 10)]), &mut runtime);
    let updates = runtime.take_cpu_buffer_updates();
    assert_eq!(presented_pixel(&mut presented, updates, toplevel, 17, 17), 0x00ff_0000, "inside the child");
    assert_eq!(presented_pixel(&mut presented, Vec::new(), toplevel, 12, 12), 0x0000_ff00, "the child around it");
    send(12, 70, poly_fill_rectangle_request(order, 0x600001, 0x600003, &[(0, 0, 40, 40)]), &mut runtime);
    send(13, 70, poly_fill_rectangle_request(order, 0x600001, 0x600004, &[(0, 0, 40, 40)]), &mut runtime);
    let updates = runtime.take_cpu_buffer_updates();
    assert_eq!(presented_pixel(&mut presented, updates, toplevel, 22, 22), 0x0000_00ff, "nothing past the child's edge");
    assert_eq!(presented_pixel(&mut presented, Vec::new(), toplevel, 17, 17), 0x00ff_0000, "the grandchild stays");
    assert_eq!(presented_pixel(&mut presented, Vec::new(), toplevel, 12, 12), 0x0000_ff00, "and the child");
}
