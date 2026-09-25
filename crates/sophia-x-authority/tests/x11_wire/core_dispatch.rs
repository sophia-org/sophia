#[test]
fn x11_dispatch_reports_root_input_focus_for_minimal_server() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = decode_x11_core_request(
        context(namespace, 522, XByteOrder::LittleEndian),
        &[43, 0, 1, 0],
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 43),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][1], 1);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]),
        X_SETUP_DEFAULT_ROOT
    );
}

#[test]
fn override_redirect_window_is_reported_as_client_positioned() {
    let namespace = NamespaceId::from_raw(45);
    let window = 0x220901;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let create = decode_x11_core_request(
        context(namespace, 540, XByteOrder::LittleEndian),
        &create_window_override_redirect_request(
            XByteOrder::LittleEndian,
            window,
            0,
            0,
            1920,
            24,
        ),
    )
    .unwrap();
    let created = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        created.response.as_ref().unwrap().surfaces.as_slice(),
        [surface]
            if surface.presentation == SurfacePresentationRole::ClientPositioned
    ));
    assert!(matches!(
        created.outputs.as_slice(),
        [XClientOutput::Event(XClientEvent::CreateNotify {
            override_redirect: true,
            ..
        })]
    ));
    let observed = XAuthorityObservedTransactionBatch::from_dispatch_result(&created).unwrap();
    assert!(matches!(
        observed.surface_presentations.as_slice(),
        [presentation]
            if presentation.role == SurfacePresentationRole::ClientPositioned
                && presentation.geometry.width == 1920
                && presentation.geometry.height == 24
    ));

    let attributes = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 3),
        XWireRequest::GetWindowAttributes {
            window: XResourceId::new(u64::from(window), 1),
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        attributes.outputs.as_slice(),
        [XClientOutput::Reply(XClientReply::GetWindowAttributes {
            override_redirect: true,
            map_state: 0,
            ..
        })]
    ));
}

#[test]
fn reparent_reports_policy_role_transition_to_session_observer() {
    let namespace = NamespaceId::from_raw(45);
    let parent = 0x220911;
    let child = 0x220912;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    for (sequence, window) in [(1, parent), (2, child)] {
        let create = decode_x11_core_request(
            context(namespace, 540, XByteOrder::LittleEndian),
            &create_window_request(
                XByteOrder::LittleEndian,
                window,
                0,
                0,
                640,
                480,
            ),
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

    let reparented = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 7),
        XWireRequest::ReparentWindow {
            window: XResourceId::new(u64::from(child), 1),
            parent: XResourceId::new(u64::from(parent), 1),
            x: 12,
            y: 24,
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(matches!(
        reparented.response.as_ref().unwrap().surfaces.as_slice(),
        [surface]
            if surface.surface == SurfaceId::new(child, 1)
                && surface.presentation == SurfacePresentationRole::ClientPositioned
                && surface.geometry.x == 12
                && surface.geometry.y == 24
    ));
}

#[test]
fn x11_dispatch_reports_window_lifecycle_map_state() {
    let namespace = NamespaceId::from_raw(45);
    let window = XResourceId::new(0x220902, 1);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let create = XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(1),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window,
            surface: SurfaceId::new(0x220902, 1),
            geometry: Rect {
                x: 0,
                y: 0,
                width: 640,
                height: 480,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    };
    assert_eq!(
        runtime.apply(create).outcome,
        XAuthorityResponseOutcome::Accepted
    );

    let attributes = |runtime: &mut XAuthorityRuntime,
                      atoms: &mut XAtomTable,
                      properties: &mut XPropertyTable| {
        dispatch_x11_wire_request(
            dispatch_context(namespace, 2, XByteOrder::LittleEndian, 3),
            XWireRequest::GetWindowAttributes { window },
            runtime,
            atoms,
            properties,
        )
    };
    assert!(matches!(
        attributes(&mut runtime, &mut atoms, &mut properties)
            .outputs
            .as_slice(),
        [XClientOutput::Reply(XClientReply::GetWindowAttributes {
            map_state: 0,
            ..
        })]
    ));

    runtime.set_policy_map_deferred(true);
    assert_eq!(
        runtime
            .apply(XAuthorityRequestPacket {
                transaction: TransactionId::from_raw(2),
                namespace,
                kind: XAuthorityRequestKind::MapWindow {
                    window,
                    generation: 2,
                },
            })
            .outcome,
        XAuthorityResponseOutcome::Accepted
    );
    assert!(matches!(
        attributes(&mut runtime, &mut atoms, &mut properties)
            .outputs
            .as_slice(),
        [XClientOutput::Reply(XClientReply::GetWindowAttributes {
            map_state: 0,
            ..
        })]
    ));

    runtime
        .admit_window_from_engine(
            namespace,
            window,
            Rect {
                x: 10,
                y: 20,
                width: 640,
                height: 480,
            },
        )
        .unwrap();
    assert!(matches!(
        attributes(&mut runtime, &mut atoms, &mut properties)
            .outputs
            .as_slice(),
        [XClientOutput::Reply(XClientReply::GetWindowAttributes {
            map_state: 2,
            ..
        })]
    ));
}

#[test]
fn mapped_policy_toplevel_cannot_overwrite_engine_geometry() {
    let namespace = NamespaceId::from_raw(45);
    let window = XResourceId::new(0x220903, 1);
    let surface = SurfaceId::new(0x220903, 1);
    let engine_geometry = Rect {
        x: 2,
        y: 16,
        width: 1276,
        height: 1422,
    };
    let mut runtime = XAuthorityRuntime::new();
    runtime.set_policy_map_deferred(true);
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    assert_eq!(
        runtime
            .apply(XAuthorityRequestPacket {
                transaction: TransactionId::from_raw(1),
                namespace,
                kind: XAuthorityRequestKind::CreateWindow {
                    window,
                    surface,
                    geometry: Rect {
                        x: 10,
                        y: 10,
                        width: 1280,
                        height: 1040,
                    },
                    constraints: SurfaceConstraints {
                        min_size: None,
                        max_size: None,
                    },
                    generation: 1,
                },
            })
            .outcome,
        XAuthorityResponseOutcome::Accepted
    );
    assert_eq!(
        runtime
            .apply(XAuthorityRequestPacket {
                transaction: TransactionId::from_raw(2),
                namespace,
                kind: XAuthorityRequestKind::MapWindow {
                    window,
                    generation: 2,
                },
            })
            .outcome,
        XAuthorityResponseOutcome::Accepted
    );
    runtime
        .admit_window_from_engine(namespace, window, engine_geometry)
        .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 12),
        XWireRequest::ConfigureWindow {
            window,
            value_mask: 0x000f,
            x: Some(10),
            y: Some(10),
            width: Some(1280),
            height: Some(1040),
            border_width: None,
            sibling: None,
            stack_mode: None,
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert_eq!(runtime.window_geometry(namespace, window), Ok(engine_geometry));
    assert!(matches!(
        result.outputs.as_slice(),
        [XClientOutput::Event(XClientEvent::ConfigureNotify {
            x: 2,
            y: 16,
            width: 1276,
            height: 1422,
            ..
        })]
    ));
}

#[test]
fn x11_dispatch_reports_core_modifier_mapping() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = decode_x11_core_request(
        context(namespace, 523, XByteOrder::LittleEndian),
        &[119, 0, 1, 0],
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 119),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0].len(), 48);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][1], 2);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][2..4]), 2);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 4);
    assert_eq!(&encoded[0][32..36], &[50, 62, 66, 0]);
}

#[test]
fn x11_dispatch_reports_an_identity_pointer_mapping_over_the_advertised_buttons() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = decode_x11_core_request(
        context(namespace, 524, XByteOrder::LittleEndian),
        &[117, 0, 1, 0],
    )
    .unwrap();

    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 117),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    // The mapping is an identity over however many buttons Sophia advertises, so
    // the reply's length fields follow the count rather than restating it.
    let mapping = sophia_x_authority::x_pointer_button_mapping();
    let padded = mapping.len().next_multiple_of(4);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0].len(), 32 + padded);
    assert_eq!(encoded[0][1], u8::try_from(mapping.len()).unwrap());
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]),
        u32::try_from(padded / 4).unwrap()
    );
    assert_eq!(&encoded[0][32..32 + mapping.len()], &mapping[..]);
}

#[test]
fn x11_dispatch_reports_us_keyboard_mapping_for_minimal_server() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = decode_x11_core_request(
        context(namespace, 524, XByteOrder::LittleEndian),
        &[101, 0, 2, 0, 8, 4, 0, 0],
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 101),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0].len(), 64);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][1], 2);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][2..4]), 3);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 8);
    let keysyms = encoded[0][32..64]
        .chunks_exact(4)
        .map(|bytes| read_u32(XByteOrder::LittleEndian, bytes))
        .collect::<Vec<_>>();
    assert_eq!(
        keysyms,
        vec![
            0,
            0,
            0xff1b,
            0xff1b,
            b'1' as u32,
            b'!' as u32,
            b'2' as u32,
            b'@' as u32
        ]
    );
}

#[test]
fn x11_dispatch_reports_evdev_navigation_keysyms() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = decode_x11_core_request(
        context(namespace, 525, XByteOrder::LittleEndian),
        &[101, 0, 2, 0, 111, 6, 0, 0],
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 101),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][1], 2);
    let keysyms = encoded[0][32..]
        .chunks_exact(4)
        .map(|bytes| read_u32(XByteOrder::LittleEndian, bytes))
        .collect::<Vec<_>>();
    assert_eq!(
        keysyms,
        vec![
            0xff52, 0xff52, 0xff55, 0xff55, 0xff51, 0xff51, 0xff53, 0xff53, 0xff57, 0xff57, 0xff54,
            0xff54,
        ]
    );
}

#[test]
fn x11_dispatch_replies_to_atom_requests_and_rejects_unknown_names() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let intern = decode_x11_core_request(
        context(namespace, 508, XByteOrder::LittleEndian),
        &intern_atom_request(XByteOrder::LittleEndian, false, X_ATOM_NAME_NET_WM_NAME),
    )
    .unwrap();
    let intern = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 16),
        intern,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = intern.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 1);
    let net_wm_name = read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]);
    assert_ne!(net_wm_name, 0);

    let missing = decode_x11_core_request(
        context(namespace, 509, XByteOrder::LittleEndian),
        &intern_atom_request(XByteOrder::LittleEndian, true, "SOPHIA_MISSING"),
    )
    .unwrap();
    let missing = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 16),
        missing,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = missing.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 0);

    let get_name = decode_x11_core_request(
        context(namespace, 510, XByteOrder::LittleEndian),
        &get_atom_name_request(XByteOrder::LittleEndian, net_wm_name),
    )
    .unwrap();
    let get_name = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 17),
        get_name,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = get_name.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][8..10]), 12);
    assert_eq!(&encoded[0][32..44], X_ATOM_NAME_NET_WM_NAME.as_bytes());

    let unknown = decode_x11_core_request(
        context(namespace, 511, XByteOrder::LittleEndian),
        &get_atom_name_request(XByteOrder::LittleEndian, 0x00ff_ffff),
    )
    .unwrap();
    let unknown = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 17),
        unknown,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = unknown.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 0);
    assert_eq!(encoded[0][1], XErrorCode::BadAtom.wire_code());
}

#[test]
fn x11_dispatch_reports_extensions_absent_until_explicitly_supported() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let query = decode_x11_core_request(
        context(namespace, 521, XByteOrder::LittleEndian),
        &query_extension_request(XByteOrder::LittleEndian, "SOPHIA-UNKNOWN"),
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
        query,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][8], 0);
    assert_eq!(encoded[0][9], 0);
}

#[test]
fn x11_dispatch_advertises_sophia_present_extension() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let query = decode_x11_core_request(
        context(namespace, 524, XByteOrder::LittleEndian),
        &query_extension_request(XByteOrder::LittleEndian, X_SOPHIA_PRESENT_EXTENSION_NAME),
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
        query,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][8], 1);
    assert_eq!(encoded[0][9], X_SOPHIA_PRESENT_MAJOR_OPCODE);
    assert_eq!(encoded[0][10], 0);
    assert_eq!(encoded[0][11], 0);
}

#[test]
fn x11_dispatch_advertises_mit_shm_and_replies_to_query_version() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let query = decode_x11_core_request(
        context(namespace, 526, XByteOrder::LittleEndian),
        &query_extension_request(XByteOrder::LittleEndian, X_MIT_SHM_EXTENSION_NAME),
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
        query,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][8], 1);
    assert_eq!(encoded[0][9], X_MIT_SHM_MAJOR_OPCODE);

    let version = decode_x11_core_request(
        context(namespace, 527, XByteOrder::LittleEndian),
        &mit_shm_query_version_request(XByteOrder::LittleEndian),
    )
    .unwrap();
    let version = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            2,
            XByteOrder::LittleEndian,
            X_MIT_SHM_MAJOR_OPCODE,
        ),
        version,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = version.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][1], 0);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][8..10]), 1);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][10..12]), 2);
}

#[test]
fn x11_dispatch_negotiates_dri3_1_3_and_present_1_2() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    for (name, opcode, first_event) in [
        (X_DRI3_EXTENSION_NAME, X_DRI3_MAJOR_OPCODE, 0),
        (
            X_PRESENT_EXTENSION_NAME,
            X_PRESENT_MAJOR_OPCODE,
            X_PRESENT_FIRST_EVENT,
        ),
    ] {
        let query = decode_x11_core_request(
            context(namespace, 528, XByteOrder::LittleEndian),
            &query_extension_request(XByteOrder::LittleEndian, name),
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
            query,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
        assert_eq!(encoded[0][8], 1);
        assert_eq!(encoded[0][9], opcode);
        assert_eq!(encoded[0][10], first_event);

        let version = decode_x11_core_request(
            context(namespace, 529, XByteOrder::LittleEndian),
            &extension_query_version_request(XByteOrder::LittleEndian, opcode, 1, 4),
        )
        .unwrap();
        let version = dispatch_x11_wire_request(
            dispatch_context(namespace, 2, XByteOrder::LittleEndian, opcode),
            version,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let encoded = version.encoded_outputs(XByteOrder::LittleEndian);
        assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 1);
        assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][12..16]),
            if opcode == X_DRI3_MAJOR_OPCODE { 3 } else { 2 });
    }
}

/// ChangeActivePointerGrab narrows the mask of a grab this client holds,
/// and leaves another client's grab alone.
#[test]
fn x11_dispatch_change_active_pointer_grab_rewrites_only_the_holders_mask() {
    let namespace = NamespaceId::from_raw(1266);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let order = XByteOrder::LittleEndian;
    let mut grab = vec![26u8, 0];
    push_u16(&mut grab, order, 6);
    push_u32(&mut grab, order, X_SETUP_DEFAULT_ROOT);
    push_u16(&mut grab, order, 0x0004 | 0x0008);
    grab.extend_from_slice(&[1, 1]);
    for _ in 0..3 {
        push_u32(&mut grab, order, 0);
    }
    let decoded = decode_x11_core_request(context(namespace, 1, order), &grab).unwrap();
    let mut holder = dispatch_context(namespace, 1, order, 26);
    holder.client_id = 7;
    let result = dispatch_x11_wire_request(holder, decoded, &mut runtime, &mut atoms, &mut properties);
    assert!(matches!(result.outputs.as_slice(), [XClientOutput::Reply(XClientReply::GrabStatus { status: 0, .. })]));

    let mut change = vec![30u8, 0];
    push_u16(&mut change, order, 4);
    push_u32(&mut change, order, 0);
    push_u32(&mut change, order, 0);
    push_u16(&mut change, order, 0x0004);
    push_u16(&mut change, order, 0);
    let decoded = decode_x11_core_request(context(namespace, 2, order), &change).unwrap();
    let mut other = dispatch_context(namespace, 2, order, 30);
    other.client_id = 8;
    let result = dispatch_x11_wire_request(other, decoded.clone(), &mut runtime, &mut atoms, &mut properties);
    assert!(result.outputs.is_empty());
    assert_eq!(runtime.input_authority_mut().pointer_grab(namespace).map(|g| g.event_mask), Some(0x000c), "another client's request changes nothing");
    let mut holder = dispatch_context(namespace, 3, order, 30);
    holder.client_id = 7;
    let result = dispatch_x11_wire_request(holder, decoded, &mut runtime, &mut atoms, &mut properties);
    assert!(result.outputs.is_empty());
    assert_eq!(runtime.input_authority_mut().pointer_grab(namespace).map(|g| g.event_mask), Some(0x0004), "the holder's mask is rewritten");
}
