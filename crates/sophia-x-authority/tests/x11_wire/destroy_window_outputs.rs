// DestroyWindow and DestroySubwindows through wire dispatch: what is destroyed,
// and the order the protocol reports it in.

#[test]
fn x11_dispatch_accepts_destroy_window_for_known_namespace_window() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let create = decode_x11_core_request(
        context(namespace, 601, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, 0x220101, 10, 20, 640, 480),
    )
    .unwrap();
    let create = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let surface = create
        .response
        .as_ref()
        .expect("CreateWindow should produce an authority response")
        .surfaces
        .first()
        .expect("CreateWindow should create one surface")
        .surface;
    assert_eq!(runtime.window_count(), 1);
    assert_eq!(runtime.resource_count(), 1);

    let destroy = decode_x11_core_request(
        context(namespace, 602, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 4, 0x220101),
    )
    .unwrap();
    let destroy = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 4),
        destroy,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    // A destroy now tells its selectors. The handler emits the record
    // addressed to the window; the router adds the parent-addressed copy for
    // SubstructureNotify selectors, as it does for map and unmap.
    assert!(matches!(
        destroy.outputs.as_slice(),
        [XClientOutput::Event(XClientEvent::DestroyNotify { event, window, .. })]
            if event == window
    ));
    assert_eq!(
        destroy.response.as_ref().unwrap().removed_surfaces,
        vec![surface]
    );
    assert_eq!(runtime.window_count(), 0);
    assert_eq!(runtime.resource_count(), 0);
    assert_eq!(
        XAuthorityObservedTransactionBatch::from_dispatch_result(&destroy),
        Some(XAuthorityObservedTransactionBatch {
            client: None,
            admission: None,
            surface_routes: Vec::new(),
            transaction: TransactionId::from_raw(2),
            transactions: Vec::new(),
            surface_presentations: Vec::new(),
            presentation_intents: Vec::new(),
            removed_surfaces: vec![surface],
            surface_output_reservations: Vec::new(),
            cpu_buffer_updates: Vec::new(),
            raster_responses: Vec::new(),
            dma_buf_registrations: Vec::new(),
            fence_registrations: Vec::new(),
            present_submissions: Vec::new(),
            software_present_submissions: Vec::new(),
            released_dma_bufs: Vec::new(),
            released_fences: Vec::new(),
            protocol_errors: Vec::new(),
            expected_protocol_errors: Vec::new(),
            metadata: Vec::new(),
            selection_owner_change: false,
            selection_conversion: false,
        })
    );
}

/// Destroying a parent destroys what it contained, and reports it in the order
/// the protocol requires: a child before the parent that held it. Reporting the
/// parent first would announce a container gone while its contents still looked
/// live, and until this landed the descendants were not destroyed at all.
#[test]
fn x11_destroy_window_reports_descendants_before_their_parent() {
    let namespace = NamespaceId::from_raw(908);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let parent = 0x0090_0201;
    let child = 0x0090_0202;
    for request in [
        create_window_request(XByteOrder::LittleEndian, parent, 0, 0, 64, 64),
        create_window_request_with_parent(XByteOrder::LittleEndian, child, parent, 0, 0, 32, 32),
    ] {
        let create =
            decode_x11_core_request(context(namespace, 700, XByteOrder::LittleEndian), &request)
                .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
            create,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
    }
    assert_eq!(runtime.window_count(), 2);

    let destroy = decode_x11_core_request(
        context(namespace, 702, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 4, parent),
    )
    .unwrap();
    let destroy = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 4),
        destroy,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let reported: Vec<u32> = destroy
        .outputs
        .iter()
        .filter_map(|output| match output {
            XClientOutput::Event(XClientEvent::DestroyNotify { event, window, .. })
                if event == window =>
            {
                Some(window.local.raw() as u32)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        reported,
        vec![child, parent],
        "the child is reported before the parent that contained it"
    );
    assert_eq!(
        runtime.window_count(),
        0,
        "the subtree is destroyed, not just the named window"
    );
}

/// DestroySubwindows empties a window without destroying it, reporting each
/// child's descendants before that child.
///
/// DestroySubwindows empties a window without destroying it, walking children
/// bottom to top and each child's descendants before itself.
///
/// The restack is what makes the ordering claim real. Ids are allocated
/// ascending, so id order and `stack_rank` order agree until something moves a
/// window; without it this passes whether or not the implementation consults
/// stacking at all.
#[test]
fn x11_destroy_subwindows_empties_the_parent_bottom_to_top() {
    let namespace = NamespaceId::from_raw(909);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let parent = 0x0091_0301;
    let lower = 0x0091_0302;
    let upper = 0x0091_0303;
    let grandchild = 0x0091_0304;
    let creates = [
        create_window_request(XByteOrder::LittleEndian, parent, 0, 0, 64, 64),
        create_window_request_with_parent(XByteOrder::LittleEndian, lower, parent, 0, 0, 32, 32),
        create_window_request_with_parent(XByteOrder::LittleEndian, upper, parent, 0, 0, 32, 32),
        create_window_request_with_parent(
            XByteOrder::LittleEndian,
            grandchild,
            lower,
            0,
            0,
            16,
            16,
        ),
    ];
    for request in creates {
        let create =
            decode_x11_core_request(context(namespace, 800, XByteOrder::LittleEndian), &request)
                .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
            create,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
    }
    assert_eq!(runtime.window_count(), 4);

    // Ids are allocated ascending, so id order and stacking order coincide
    // until something restacks. Lower `upper` beneath `lower` so the two
    // disagree -- otherwise this test passes whether or not the
    // implementation consults `stack_rank` at all.
    runtime
        .restack_window(
            namespace,
            XResourceId::new(u64::from(upper), 1),
            None,
            Some(1),
        )
        .expect("restack should place the window at the bottom");

    let request = decode_x11_core_request(
        context(namespace, 802, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 5, parent),
    )
    .unwrap();
    let destroyed = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 5),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let reported: Vec<u32> = destroyed
        .outputs
        .iter()
        .filter_map(|output| match output {
            XClientOutput::Event(XClientEvent::DestroyNotify { event, window, .. })
                if event == window =>
            {
                Some(window.local.raw() as u32)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        reported,
        vec![upper, grandchild, lower],
        "bottom to top after the restack: the lowered child first, then the \
         other child's descendants before itself"
    );
    assert_eq!(
        runtime.window_count(),
        1,
        "the named window survives having its children destroyed"
    );
}

/// An unknown window is an error, and an error owes no notification. A phantom
/// event here would tell a client its window went away when nothing happened.
#[test]
fn x11_destroy_subwindows_rejects_an_unknown_window_without_notifying() {
    let namespace = NamespaceId::from_raw(910);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let request = decode_x11_core_request(
        context(namespace, 804, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 5, 0x0091_0999),
    )
    .unwrap();
    let rejected = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 5),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(
        rejected
            .outputs
            .iter()
            .all(|output| !matches!(
                output,
                XClientOutput::Event(XClientEvent::DestroyNotify { .. })
            )),
        "a rejected destroy owes no DestroyNotify"
    );
    assert!(matches!(
        rejected.outputs.as_slice(),
        [XClientOutput::Error(_)]
    ));
}
