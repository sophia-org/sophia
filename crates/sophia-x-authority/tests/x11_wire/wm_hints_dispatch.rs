// ICCCM and EWMH hints as properties: normal hints, transients, dialog
// types, the manager advertisement and the active window. Included from
// x11_wire.rs beside properties_dispatch.rs (t026).

#[test]
fn icccm_normal_hints_reduce_to_protocol_neutral_minimum_and_maximum() {
    let namespace = NamespaceId::from_raw(45);
    let atoms = XAtomTable::new();
    let mut values = [0_u32; 18];
    values[0] = (1 << 4) | (1 << 5);
    values[5] = 320;
    values[6] = 200;
    values[7] = 1920;
    values[8] = 1080;
    let record = XPropertyRecord {
        namespace,
        window: XResourceId::new(0x220010, 1),
        property: 40,
        property_type: 41,
        format: 32,
        bytes: values.into_iter().flat_map(u32::to_le_bytes).collect(),
        generation: 1,
    };

    assert_eq!(
        decode_x_size_hints(&record, &atoms, XByteOrder::LittleEndian),
        Some(Ok(SurfaceConstraints {
            min_size: Some(Size {
                width: 320,
                height: 200,
            }),
            max_size: Some(Size {
                width: 1920,
                height: 1080,
            }),
        }))
    );
}

#[test]
fn wm_transient_for_attaches_dialog_and_unmap_publishes_lifecycle_snapshot() {
    let namespace = NamespaceId::from_raw(45);
    let owner = 0x220020;
    let dialog = 0x220021;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    for (sequence, window) in [(1_u16, owner), (2_u16, dialog)] {
        let create = decode_x11_core_request(
            context(
                namespace,
                540 + u64::from(sequence),
                XByteOrder::LittleEndian,
            ),
            &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 640, 480),
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 1),
            create,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
    }
    runtime
        .apply(XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(3),
            namespace,
            kind: XAuthorityRequestKind::MapWindow {
                window: XResourceId::new(u64::from(dialog), 1),
                generation: 3,
            },
        });

    let transient_for = atoms.intern("WM_TRANSIENT_FOR", false).unwrap().unwrap();
    let window_type = atoms.intern("WINDOW", false).unwrap().unwrap();
    let change = decode_x11_core_request(
        context(namespace, 544, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            dialog,
            transient_for,
            window_type,
            32,
            &owner.to_le_bytes(),
        ),
    )
    .unwrap();
    let attached = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 18),
        change,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        attached.response.as_ref().unwrap().surfaces.as_slice(),
        [surface]
            if surface.surface == SurfaceId::new(dialog, 1)
                && surface.presentation == SurfacePresentationRole::PolicyManaged
                && surface.presentation_owner == Some(SurfaceId::new(owner, 1))
                && surface.kind == LayoutNodeKind::Dialog
                && surface.placement_preference == SurfacePlacementPreference::Floating
                && surface.mapped
    ));

    let unmapped = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 10),
        XWireRequest::UnmapWindow {
            window: XResourceId::new(u64::from(dialog), 1),
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        unmapped.response.as_ref().unwrap().surfaces.as_slice(),
        [surface]
            if surface.surface == SurfaceId::new(dialog, 1)
                && surface.presentation_owner == Some(SurfaceId::new(owner, 1))
                && !surface.mapped
    ));
}

#[test]
fn root_transient_stays_policy_managed_without_a_surface_owner() {
    let namespace = NamespaceId::from_raw(46);
    let dialog = 0x220022;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let create = decode_x11_core_request(
        context(namespace, 550, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, dialog, 0, 0, 480, 240),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    runtime
        .apply(XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(2),
            namespace,
            kind: XAuthorityRequestKind::MapWindow {
                window: XResourceId::new(u64::from(dialog), 1),
                generation: 2,
            },
        });

    let transient_for = atoms.intern("WM_TRANSIENT_FOR", false).unwrap().unwrap();
    let window_type = atoms.intern("WINDOW", false).unwrap().unwrap();
    let change = decode_x11_core_request(
        context(namespace, 551, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            dialog,
            transient_for,
            window_type,
            32,
            &X_SETUP_DEFAULT_ROOT.to_le_bytes(),
        ),
    )
    .unwrap();
    let attached = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 18),
        change,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        attached.response.as_ref().unwrap().surfaces.as_slice(),
        [surface]
            if surface.surface == SurfaceId::new(dialog, 1)
                && surface.presentation == SurfacePresentationRole::PolicyManaged
                && surface.presentation_owner.is_none()
                && surface.kind == LayoutNodeKind::Dialog
                && surface.placement_preference == SurfacePlacementPreference::Floating
                && surface.mapped
    ));

    let detached = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 19),
        XWireRequest::DeleteProperty {
            window: XResourceId::new(u64::from(dialog), 1),
            property: transient_for,
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        detached.response.as_ref().unwrap().surfaces.as_slice(),
        [surface]
            if surface.presentation == SurfacePresentationRole::PolicyManaged
                && surface.presentation_owner.is_none()
                && surface.mapped
    ));
}

#[test]
fn ewmh_dialog_type_is_policy_managed_and_requests_floating_placement() {
    let namespace = NamespaceId::from_raw(47);
    let dialog = 0x220023;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let create = decode_x11_core_request(
        context(namespace, 560, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, dialog, 30, 40, 480, 281),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let window_type = atoms
        .intern("_NET_WM_WINDOW_TYPE", false)
        .unwrap()
        .unwrap();
    let extension_type = atoms.intern("_SOPHIA_TEST_TYPE", false).unwrap().unwrap();
    let dialog_type = atoms
        .intern("_NET_WM_WINDOW_TYPE_DIALOG", false)
        .unwrap()
        .unwrap();
    let mut types = Vec::new();
    types.extend_from_slice(&extension_type.to_le_bytes());
    types.extend_from_slice(&dialog_type.to_le_bytes());
    let change = decode_x11_core_request(
        context(namespace, 561, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            dialog,
            window_type,
            crate::X_ATOM_ATOM,
            32,
            &types,
        ),
    )
    .unwrap();
    let typed = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 18),
        change,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        typed.response.as_ref().unwrap().surfaces.as_slice(),
        [surface]
            if surface.presentation == SurfacePresentationRole::PolicyManaged
                && surface.kind == LayoutNodeKind::Dialog
                && surface.placement_preference == SurfacePlacementPreference::Floating
                && !surface.mapped
    ));

    let mapped = runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(3),
        namespace,
        kind: XAuthorityRequestKind::MapWindow {
            window: XResourceId::new(u64::from(dialog), 1),
            generation: 3,
        },
    });
    assert!(matches!(
        mapped.surfaces.as_slice(),
        [surface]
            if surface.presentation == SurfacePresentationRole::PolicyManaged
                && surface.kind == LayoutNodeKind::Dialog
                && surface.placement_preference == SurfacePlacementPreference::Floating
                && surface.mapped
                && surface.geometry == Rect { x: 30, y: 40, width: 480, height: 281 }
    ));

    let deleted = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 19),
        XWireRequest::DeleteProperty {
            window: XResourceId::new(u64::from(dialog), 1),
            property: window_type,
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        deleted.response.as_ref().unwrap().surfaces.as_slice(),
        [surface]
            if surface.presentation == SurfacePresentationRole::PolicyManaged
                && surface.mapped
    ));
}

fn read_seeded_property(
    properties: &mut XPropertyTable,
    atoms: &mut XAtomTable,
    namespace: NamespaceId,
    window: u32,
    name: &str,
) -> Vec<u8> {
    let property = atoms.intern(name, false).unwrap().unwrap();
    let property_type = X_PROPERTY_ANY_TYPE;
    properties
        .read_property(
            namespace,
            XPropertyRead {
                delete: false,
                window: XResourceId::new(u64::from(window), 1),
                property,
                property_type,
                long_offset: 0,
                long_length: 64,
            },
        )
        .unwrap()
        .reply
        .bytes
}

#[test]
fn a_client_asking_whether_a_manager_runs_is_answered() {
    // The three-step handshake a toolkit performs at startup, and the one the
    // browser trace showed it performing: read the check window from the root,
    // read it again from the window that names to prove the manager is live
    // rather than a stale property, then read the manager's name.
    let namespace = NamespaceId::from_raw(45);
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    seed_wm_advertisement(
        &mut properties,
        &mut atoms,
        namespace,
        XByteOrder::LittleEndian,
    )
    .unwrap();

    let expected = X_SETUP_WM_CHECK_WINDOW.to_le_bytes().to_vec();
    assert_eq!(
        read_seeded_property(
            &mut properties,
            &mut atoms,
            namespace,
            X_SETUP_DEFAULT_ROOT,
            "_NET_SUPPORTING_WM_CHECK",
        ),
        expected,
    );
    assert_eq!(
        read_seeded_property(
            &mut properties,
            &mut atoms,
            namespace,
            X_SETUP_WM_CHECK_WINDOW,
            "_NET_SUPPORTING_WM_CHECK",
        ),
        expected,
        "the self-reference is what separates a live manager from a stale root property",
    );
    assert_eq!(
        read_seeded_property(
            &mut properties,
            &mut atoms,
            namespace,
            X_SETUP_WM_CHECK_WINDOW,
            "_NET_WM_NAME",
        ),
        b"Sophia".to_vec(),
    );
}

#[test]
fn the_supported_claim_lists_only_hints_with_behaviour_behind_them() {
    // A drift guard. Adding an atom to the advertised list without behaviour
    // behind it is the overclaim this advertisement exists to avoid, so the
    // list is pinned here rather than merely being whatever the constant says.
    assert_eq!(
        X_EWMH_SUPPORTED_ATOM_NAMES,
        &[
            "_NET_SUPPORTING_WM_CHECK",
            "_NET_ACTIVE_WINDOW",
            "_NET_WM_NAME",
            "_NET_WM_STATE",
            "_NET_WM_STATE_FULLSCREEN",
            "_NET_WM_STATE_HIDDEN",
            "_NET_WM_STATE_MAXIMIZED_HORZ",
            "_NET_WM_STATE_MAXIMIZED_VERT",
            "_NET_WM_STRUT",
            "_NET_WM_STRUT_PARTIAL",
            "_NET_WM_WINDOW_TYPE",
        ],
    );
    // Hints clients do ask about and Sophia does not honour stay out.
    for withheld in [
        "_NET_CLIENT_LIST",
        "_NET_CURRENT_DESKTOP",
        "_NET_FRAME_EXTENTS",
        "_NET_WM_SYNC_REQUEST",
        "_NET_WM_MOVERESIZE",
    ] {
        assert!(
            !X_EWMH_SUPPORTED_ATOM_NAMES.contains(&withheld),
            "{withheld} is advertised without behaviour behind it",
        );
    }
}

#[test]
fn seeding_the_advertisement_twice_leaves_one_answer() {
    // Seeded per connection, so a second client in the same namespace must not
    // append a second copy or bump the value.
    let namespace = NamespaceId::from_raw(45);
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let order = XByteOrder::LittleEndian;

    seed_wm_advertisement(&mut properties, &mut atoms, namespace, order).unwrap();
    let first = read_seeded_property(
        &mut properties,
        &mut atoms,
        namespace,
        X_SETUP_DEFAULT_ROOT,
        "_NET_SUPPORTED",
    );
    seed_wm_advertisement(&mut properties, &mut atoms, namespace, order).unwrap();
    let second = read_seeded_property(
        &mut properties,
        &mut atoms,
        namespace,
        X_SETUP_DEFAULT_ROOT,
        "_NET_SUPPORTED",
    );

    assert_eq!(first, second);
    assert_eq!(first.len(), X_EWMH_SUPPORTED_ATOM_NAMES.len() * 4);
}

#[test]
fn the_active_window_is_published_from_focus_and_never_reset_by_reseeding() {
    let namespace = NamespaceId::from_raw(46);
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let order = XByteOrder::LittleEndian;
    let read = |properties: &mut XPropertyTable, atoms: &mut XAtomTable| {
        read_seeded_property(properties, atoms, namespace, X_SETUP_DEFAULT_ROOT, "_NET_ACTIVE_WINDOW")
    };

    seed_wm_advertisement(&mut properties, &mut atoms, namespace, order).unwrap();
    // Present from the first connection, as a value: None is 0, not absence.
    // A toolkit that finds the property missing asks for the name of the
    // missing type, which is atom None, and libX11 answers that by exiting it.
    assert_eq!(read(&mut properties, &mut atoms), 0u32.to_le_bytes().to_vec());

    publish_active_window(&mut properties, &mut atoms, namespace, order, 0x0060_0001).unwrap();
    assert_eq!(read(&mut properties, &mut atoms), 0x0060_0001u32.to_le_bytes().to_vec());

    // A later client's seeding re-asserts the advertisement and must leave
    // the focus where it is.
    seed_wm_advertisement(&mut properties, &mut atoms, namespace, order).unwrap();
    assert_eq!(read(&mut properties, &mut atoms), 0x0060_0001u32.to_le_bytes().to_vec());

    publish_active_window(&mut properties, &mut atoms, namespace, order, 0).unwrap();
    assert_eq!(read(&mut properties, &mut atoms), 0u32.to_le_bytes().to_vec());
}
