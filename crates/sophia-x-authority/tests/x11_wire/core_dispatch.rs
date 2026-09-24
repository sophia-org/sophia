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

#[test]
fn dri3_open_decodes_default_provider_and_encodes_one_fd_reply() {
    let namespace = NamespaceId::from_raw(45);
    let request = decode_x11_core_request(
        context(namespace, 529, XByteOrder::LittleEndian),
        &dri3_open_request(XByteOrder::LittleEndian, X_SETUP_DEFAULT_ROOT, 0),
    )
    .unwrap();
    assert_eq!(request.required_fd_count(), 0);
    assert_eq!(
        request,
        XWireRequest::Dri3Open {
            drawable: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            provider: 0,
        }
    );

    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 7, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][1], 1);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][2..4]), 7);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 0);
}

#[test]
fn dri3_open_rejects_nondefault_provider() {
    let namespace = NamespaceId::from_raw(45);
    let request = decode_x11_core_request(
        context(namespace, 529, XByteOrder::LittleEndian),
        &dri3_open_request(XByteOrder::LittleEndian, X_SETUP_DEFAULT_ROOT, 99),
    )
    .unwrap();
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 8, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 0);
    assert_eq!(encoded[0][1], XErrorCode::BadValue.wire_code());
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 99);
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &encoded[0][8..10]),
        u16::from(X_DRI3_OPEN_MINOR_OPCODE)
    );
}

#[test]
fn dri3_get_supported_modifiers_without_measurements_reports_no_explicit_layouts() {
    let namespace = NamespaceId::from_raw(45);
    let request = decode_x11_core_request(
        context(namespace, 529, XByteOrder::LittleEndian),
        &dri3_get_supported_modifiers_request(
            XByteOrder::LittleEndian,
            X_SETUP_DEFAULT_ROOT,
            24,
            32,
        ),
    )
    .unwrap();
    assert_eq!(
        request,
        XWireRequest::Dri3GetSupportedModifiers {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            depth: 24,
            bits_per_pixel: 32,
        }
    );
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 9, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0].len(), 32);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][12..16]), 0);

    let argb = dispatch_x11_wire_request(
        dispatch_context(namespace, 10, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        XWireRequest::Dri3GetSupportedModifiers {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            depth: 32,
            bits_per_pixel: 32,
        },
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(argb[0][0], 1);

    let invalid = decode_x11_core_request(
        context(namespace, 530, XByteOrder::LittleEndian),
        &dri3_get_supported_modifiers_request(
            XByteOrder::LittleEndian,
            X_SETUP_DEFAULT_ROOT,
            16,
            16,
        ),
    )
    .unwrap();
    let invalid = dispatch_x11_wire_request(
        dispatch_context(namespace, 10, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        invalid,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(invalid[0][0], 0);
    assert_eq!(invalid[0][1], XErrorCode::BadValue.wire_code());
}

#[test]
fn dri3_pixmap_from_buffer_requires_one_fd_and_preserves_bounded_metadata() {
    let namespace = NamespaceId::from_raw(45);
    let request = decode_x11_core_request(
        context(namespace, 530, XByteOrder::LittleEndian),
        &dri3_pixmap_from_buffer_request(
            XByteOrder::LittleEndian,
            0x220801,
            X_SETUP_DEFAULT_ROOT,
            64 * 48 * 4,
            64,
            48,
            256,
            24,
            32,
        ),
    )
    .unwrap();
    assert_eq!(request.required_fd_count(), 1);
    assert_eq!(
        request,
        XWireRequest::Dri3PixmapFromBuffer {
            pixmap: XResourceId::new(0x220801, 1),
            drawable: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            size_bytes: 64 * 48 * 4,
            width: 64,
            height: 48,
            stride: 256,
            depth: 24,
            bits_per_pixel: 32,
        }
    );
}

#[test]
fn dri3_pixmap_from_buffers_preserves_modifier_and_plane_metadata() {
    let namespace = NamespaceId::from_raw(45);
    let pixmap = XResourceId::new(0x220803, 1);
    let request = decode_x11_core_request(
        context(namespace, 531, XByteOrder::LittleEndian),
        &dri3_pixmap_from_buffers_request(
            XByteOrder::LittleEndian,
            0x220803,
            X_SETUP_DEFAULT_ROOT,
            1,
            64,
            48,
            [256, 0, 0, 0],
            [0, 0, 0, 0],
            24,
            32,
            0,
        ),
    )
    .unwrap();
    assert_eq!(request.required_fd_count(), 1);
    assert_eq!(
        request,
        XWireRequest::Dri3PixmapFromBuffers {
            pixmap,
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            num_buffers: 1,
            width: 64,
            height: 48,
            strides: [256, 0, 0, 0],
            offsets: [0, 0, 0, 0],
            depth: 24,
            bits_per_pixel: 32,
            modifier: 0,
        }
    );

    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty());
    let descriptor = runtime.dri3_pixmap_descriptor(namespace, pixmap).unwrap();
    assert_eq!(
        descriptor.size,
        Size {
            width: 64,
            height: 48,
        }
    );
    assert_eq!(descriptor.format, sophia_protocol::DRM_FORMAT_XRGB8888);
    assert_eq!(descriptor.modifier, 0);
    assert_eq!(descriptor.plane_count, 1);
    assert_eq!(descriptor.planes[0].unwrap().stride, 256);
    assert_eq!(descriptor.planes[0].unwrap().offset, 0);
}

#[test]
fn dri3_fence_from_fd_requires_one_fd_and_registers_authority_identity() {
    let namespace = NamespaceId::from_raw(45);
    let request = decode_x11_core_request(
        context(namespace, 531, XByteOrder::LittleEndian),
        &dri3_fence_from_fd_request(
            XByteOrder::LittleEndian,
            X_SETUP_DEFAULT_ROOT,
            0x220802,
            false,
        ),
    )
    .unwrap();
    assert_eq!(request.required_fd_count(), 1);
    assert_eq!(
        request,
        XWireRequest::Dri3FenceFromFd {
            drawable: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            fence: XResourceId::new(0x220802, 1),
            initially_triggered: false,
        }
    );

    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty());
    runtime
        .validate_dri3_fence_access(namespace, XResourceId::new(0x220802, 1))
        .unwrap();
}

#[test]
fn standard_present_pixmap_reduces_dri3_pixmap_to_dmabuf_transaction() {
    let namespace = NamespaceId::from_raw(45);
    let window = XResourceId::new(0x220810, 1);
    let pixmap = XResourceId::new(0x220811, 1);
    let mut runtime = XAuthorityRuntime::new();
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(1),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window,
            surface: SurfaceId::new(45, 1),
            geometry: Rect {
                x: 0,
                y: 0,
                width: 64,
                height: 48,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    let descriptor = runtime
        .create_dri3_pixmap(namespace, pixmap, 2, 64 * 48 * 4, 64, 48, 256, 24, 32)
        .unwrap();

    let request = decode_x11_core_request(
        context(namespace, 532, XByteOrder::LittleEndian),
        &present_pixmap_request(XByteOrder::LittleEndian, window, pixmap, 77),
    )
    .unwrap();
    assert_eq!(request.required_fd_count(), 0);
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(
            namespace,
            2,
            XByteOrder::LittleEndian,
            X_PRESENT_MAJOR_OPCODE,
        ),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty());
    let response = result.response.unwrap();
    assert_eq!(response.transaction, TransactionId::from_raw(532));
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].transaction,
        TransactionId::from_raw(532)
    );
    assert_eq!(
        response.transactions[0].target_buffer(),
        BufferSource::DmaBuf {
            handle: descriptor.handle.raw()
        }
    );
    assert_eq!(response.transactions[0].damage.rects[0].width, 64);
    assert_eq!(response.transactions[0].damage.rects[0].height, 48);
}

#[test]
fn child_dri3_present_projects_onto_the_managed_toplevel() {
    let namespace = NamespaceId::from_raw(46);
    let parent = XResourceId::new(0x220820, 1);
    let child = XResourceId::new(0x220821, 1);
    let pixmap = XResourceId::new(0x220822, 1);
    let parent_surface = SurfaceId::new(46, 1);
    let mut runtime = XAuthorityRuntime::new();
    for (window, surface, geometry) in [
        (
            parent,
            parent_surface,
            Rect {
                x: 100,
                y: 200,
                width: 64,
                height: 80,
            },
        ),
        (
            child,
            SurfaceId::new(47, 1),
            Rect {
                x: 5,
                y: 7,
                width: 64,
                height: 48,
            },
        ),
    ] {
        runtime.apply(XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(u64::from(surface.index())),
            namespace,
            kind: XAuthorityRequestKind::CreateWindow {
                window,
                surface,
                geometry,
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            },
        });
    }
    runtime.set_window_parent(namespace, child, parent).unwrap();
    let descriptor = runtime
        .create_dri3_pixmap(namespace, pixmap, 1, 64 * 48 * 4, 64, 48, 256, 24, 32)
        .unwrap();

    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(548),
        namespace,
        child,
        pixmap,
        2,
        3,
        None,
        None,
    );

    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions.len(), 1);
    let transaction = &response.transactions[0];
    assert_eq!(transaction.surface, parent_surface);
    assert_eq!(transaction.target_geometry.x, 100);
    assert_eq!(transaction.target_geometry.y, 200);
    assert_eq!(transaction.target_geometry.width, 64);
    assert_eq!(transaction.target_geometry.height, 80);
    assert_eq!(
        transaction.raster_extent(),
        Size {
            width: 64,
            height: 48,
        }
    );
    assert_eq!(
        transaction.target_buffer(),
        BufferSource::DmaBuf {
            handle: descriptor.handle.raw()
        }
    );
    assert_eq!(
        transaction.damage,
        Region::single(Rect {
            x: 7,
            y: 10,
            width: 64,
            height: 48,
        })
    );
    assert_eq!(
        runtime
            .window_presentation_root_and_offset(namespace, child)
            .unwrap(),
        (parent, parent_surface, 5, 7)
    );
}

fn test_plane_descriptor() -> std::sync::Arc<std::os::fd::OwnedFd> {
    let file = std::fs::File::open("/dev/null").expect("a plane descriptor stand-in");
    std::sync::Arc::new(std::os::fd::OwnedFd::from(file))
}

fn import_dri3_pixmap(
    runtime: &mut XAuthorityRuntime,
    namespace: NamespaceId,
    pixmap: XResourceId,
    raw_pixmap: u32,
) {
    let request = decode_x11_core_request(
        context(namespace, 531, XByteOrder::LittleEndian),
        &dri3_pixmap_from_buffers_request(
            XByteOrder::LittleEndian,
            raw_pixmap,
            X_SETUP_DEFAULT_ROOT,
            1,
            64,
            48,
            [256, 0, 0, 0],
            [0, 0, 0, 0],
            24,
            32,
            9,
        ),
    )
    .unwrap();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        request,
        runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty(), "import should not error");
    // The socket layer records these; there is no socket in a dispatch test.
    runtime
        .attach_dri3_plane_fds(namespace, pixmap, vec![test_plane_descriptor()])
        .expect("plane descriptors are recorded against the imported pixmap");
}

fn dispatch_dri3(
    runtime: &mut XAuthorityRuntime,
    namespace: NamespaceId,
    bytes: &[u8],
) -> Vec<XClientOutput> {
    let request = decode_x11_core_request(
        context(namespace, 532, XByteOrder::LittleEndian),
        bytes,
    )
    .unwrap();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, X_DRI3_MAJOR_OPCODE),
        request,
        runtime,
        &mut atoms,
        &mut properties,
    )
    .outputs
}

#[test]
fn an_imported_pixmap_answers_with_the_facts_it_was_imported_with() {
    // DRI3 lets a client recover the buffer it gave the authority. The answer
    // must be the same buffer, not a plausible one: a client that imports at
    // one stride and recovers at another renders a sheared image.
    let namespace = NamespaceId::from_raw(45);
    let pixmap = XResourceId::new(0x220803, 1);
    let mut runtime = XAuthorityRuntime::new();
    import_dri3_pixmap(&mut runtime, namespace, pixmap, 0x220803);

    let outputs = dispatch_dri3(
        &mut runtime,
        namespace,
        &dri3_buffers_from_pixmap_request(XByteOrder::LittleEndian, 0x220803),
    );

    assert_eq!(outputs.len(), 1);
    let XClientOutput::Reply(XClientReply::Dri3BuffersFromPixmap {
        width,
        height,
        modifier,
        depth,
        bits_per_pixel,
        strides,
        offsets,
        ..
    }) = &outputs[0]
    else {
        panic!("expected a BuffersFromPixmap reply, got {:?}", outputs[0]);
    };
    assert_eq!((*width, *height), (64, 48));
    assert_eq!(*modifier, 9);
    assert_eq!((*depth, *bits_per_pixel), (24, 32));
    // The list length is the `nfd` the reply header promises.
    assert_eq!(strides, &vec![256]);
    assert_eq!(offsets, &vec![0]);
}

#[test]
fn the_single_plane_recovery_reports_the_same_buffer() {
    let namespace = NamespaceId::from_raw(45);
    let pixmap = XResourceId::new(0x220803, 1);
    let mut runtime = XAuthorityRuntime::new();
    import_dri3_pixmap(&mut runtime, namespace, pixmap, 0x220803);

    let outputs = dispatch_dri3(
        &mut runtime,
        namespace,
        &dri3_buffer_from_pixmap_request(XByteOrder::LittleEndian, 0x220803),
    );

    assert_eq!(outputs.len(), 1);
    let XClientOutput::Reply(XClientReply::Dri3BufferFromPixmap {
        size_bytes,
        width,
        height,
        stride,
        depth,
        bits_per_pixel,
        ..
    }) = &outputs[0]
    else {
        panic!("expected a BufferFromPixmap reply, got {:?}", outputs[0]);
    };
    assert_eq!((*width, *height, *stride), (64, 48, 256));
    assert_eq!(*size_bytes, 256 * 48);
    assert_eq!((*depth, *bits_per_pixel), (24, 32));
}

#[test]
fn legacy_dri3_exports_preserve_exact_layout_or_refuse() {
    for (stride, offset, legacy_admitted) in [
        (256, 0, true),
        (u32::from(u16::MAX), 0, true),
        (u32::from(u16::MAX) + 1, 0, false),
        (256, 4096, false),
    ] {
        let namespace = NamespaceId::from_raw(45);
        let raw_pixmap = 0x220803;
        let pixmap = XResourceId::new(u64::from(raw_pixmap), 1);
        let mut runtime = XAuthorityRuntime::new();
        let imported = dispatch_dri3(
            &mut runtime,
            namespace,
            &dri3_pixmap_from_buffers_request(
                XByteOrder::LittleEndian,
                raw_pixmap,
                X_SETUP_DEFAULT_ROOT,
                1,
                64,
                48,
                [stride, 0, 0, 0],
                [offset, 0, 0, 0],
                32,
                32,
                9,
            ),
        );
        assert!(imported.is_empty(), "stride={stride} offset={offset}");
        // Descriptor stand-ins exercise protocol metadata, not GPU importability.
        runtime
            .attach_dri3_plane_fds(namespace, pixmap, vec![test_plane_descriptor()])
            .unwrap();

        let legacy = dispatch_dri3(
            &mut runtime,
            namespace,
            &dri3_buffer_from_pixmap_request(XByteOrder::LittleEndian, raw_pixmap),
        );
        assert_eq!(legacy.len(), 1);
        let encoded = encode_x_client_output(XByteOrder::LittleEndian, legacy[0].clone());
        assert_eq!(encoded.len(), X_CLIENT_OUTPUT_RECORD_LEN);
        if legacy_admitted {
            assert_eq!(encoded[0], 1);
            assert_eq!(encoded[1], 1);
            assert_eq!(
                read_u32(XByteOrder::LittleEndian, &encoded[8..12]),
                stride * 48
            );
            assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[12..14]), 64);
            assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[14..16]), 48);
            assert_eq!(
                u32::from(read_u16(XByteOrder::LittleEndian, &encoded[16..18])),
                stride
            );
            assert_eq!(&encoded[18..20], &[32, 32]);
        } else {
            let XClientOutput::Error(error) = &legacy[0] else {
                panic!("legacy reply cannot represent stride={stride} offset={offset}");
            };
            assert_eq!(encoded[0], 0);
            assert_eq!(encoded[1], XErrorCode::BadPixmap.wire_code());
            assert_eq!(error.resource_id, raw_pixmap);
            assert_eq!(error.major_code, X_DRI3_MAJOR_OPCODE);
            assert_eq!(
                error.minor_code,
                u16::from(X_DRI3_BUFFER_FROM_PIXMAP_MINOR_OPCODE)
            );
        }

        // Legacy refusal leaves the exact modern export available.
        let modern = dispatch_dri3(
            &mut runtime,
            namespace,
            &dri3_buffers_from_pixmap_request(XByteOrder::LittleEndian, raw_pixmap),
        );
        assert_eq!(modern.len(), 1);
        let XClientOutput::Reply(XClientReply::Dri3BuffersFromPixmap {
            width,
            height,
            modifier,
            depth,
            bits_per_pixel,
            strides,
            offsets,
            ..
        }) = &modern[0]
        else {
            panic!("modern export must retain stride={stride} offset={offset}");
        };
        assert_eq!(
            (*width, *height, *modifier, *depth, *bits_per_pixel),
            (64, 48, 9, 32, 32)
        );
        assert_eq!(strides, &[stride]);
        assert_eq!(offsets, &[offset]);
    }
}

#[test]
fn a_pixmap_that_was_never_imported_is_refused_by_name() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();

    let outputs = dispatch_dri3(
        &mut runtime,
        namespace,
        &dri3_buffers_from_pixmap_request(XByteOrder::LittleEndian, 0x220999),
    );

    assert_eq!(outputs.len(), 1);
    let XClientOutput::Error(error) = &outputs[0] else {
        panic!("expected an error, got {:?}", outputs[0]);
    };
    // Named against the request that failed, so a tally can attribute it, and
    // reported as a pixmap fault because a PIXMAP is what the request named.
    assert_eq!(
        error.minor_code,
        u16::from(X_DRI3_BUFFERS_FROM_PIXMAP_MINOR_OPCODE)
    );
    assert_eq!(error.code, XErrorCode::BadPixmap);
    assert_eq!(error.resource_id, 0x220999);
}

#[test]
fn a_dri3_request_sophia_does_not_implement_is_a_client_visible_error() {
    // Not a parse failure. An unsupported request must stay a normal X11 error
    // so the client keeps a sequence number to attribute it to, and so a tally
    // can name the minor rather than reporting an undecodable request.
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut bytes = vec![X_DRI3_MAJOR_OPCODE, 5u8];
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let outputs = dispatch_dri3(&mut runtime, namespace, &bytes);

    assert_eq!(outputs.len(), 1);
    let XClientOutput::Error(error) = &outputs[0] else {
        panic!("expected an error, got {:?}", outputs[0]);
    };
    assert_eq!(error.code, XErrorCode::BadImplementation);
    assert_eq!(error.minor_code, 5);
}

#[test]
fn a_pixmap_the_client_did_not_allocate_asks_to_be_backed() {
    // The other half of DRI3. A client that expects the server to own the
    // storage never sends a buffer, so the authority has to originate one --
    // and it needs the extent and depth of the pixmap the client did create.
    let namespace = NamespaceId::from_raw(45);
    let pixmap = XResourceId::new(0x220805, 1);
    let size = Size {
        width: 64,
        height: 48,
    };
    let mut runtime = XAuthorityRuntime::new();
    runtime.create_pixmap(namespace, pixmap, size, 24, 1).unwrap();

    let request = runtime
        .dri3_pixmap_backing_request(namespace, pixmap)
        .expect("a pixmap with no buffer of its own asks to be backed");
    assert_eq!(request.size, size);
    assert_eq!(request.depth, 24);

    // Backing it makes the recovery answer, in the same shape an import leaves.
    let descriptor = sophia_protocol::DmaBufDescriptor {
        handle: sophia_protocol::BufferHandle::from_raw(runtime.next_dma_buf_handle()),
        size,
        format: sophia_protocol::DRM_FORMAT_XRGB8888,
        modifier: 0,
        plane_count: 1,
        planes: [
            Some(sophia_protocol::DmaBufPlaneDescriptor {
                offset: 0,
                stride: 256,
            }),
            None,
            None,
            None,
        ],
    };
    runtime
        .adopt_dri3_pixmap_backing(namespace, pixmap, descriptor, vec![test_plane_descriptor()])
        .unwrap();

    let (recovered, fds) = runtime.dri3_pixmap_buffers(namespace, pixmap).unwrap();
    assert_eq!(recovered.size, size);
    assert_eq!(recovered.planes[0].unwrap().stride, 256);
    assert_eq!(fds.len(), 1);

    // Backed once: a second recovery must not ask for another allocation.
    assert!(runtime.dri3_pixmap_backing_request(namespace, pixmap).is_none());

    // The lifetime is still the pixmap's.
    runtime.free_pixmap(namespace, pixmap).unwrap();
    assert!(runtime.dri3_pixmap_buffers(namespace, pixmap).is_err());
}

#[test]
fn a_pixmap_holding_drawn_pixels_is_not_backed_underneath_the_client() {
    // A buffer allocated now would be empty, and adopting it would silently
    // replace what the client already drew through the CPU path. Refuse instead.
    let namespace = NamespaceId::from_raw(45);
    let pixmap = XResourceId::new(0x220806, 1);
    let size = Size {
        width: 32,
        height: 32,
    };
    let mut runtime = XAuthorityRuntime::new();
    runtime.create_pixmap(namespace, pixmap, size, 24, 1).unwrap();
    assert!(runtime.dri3_pixmap_backing_request(namespace, pixmap).is_some());

    let marker = [0x11, 0x22, 0x33, 0x00];
    let response = runtime.apply_put_image(
        TransactionId::from_raw(7),
        namespace,
        pixmap,
        Region::single(Rect {
            x: 1,
            y: 1,
            width: 1,
            height: 1,
        }),
        Some(&marker),
        None,
    );
    assert!(
        matches!(response.outcome, XAuthorityResponseOutcome::Accepted),
        "the CPU upload must land for this test to mean anything",
    );

    assert!(
        runtime.dri3_pixmap_backing_request(namespace, pixmap).is_none(),
        "a pixmap with drawn pixels must not be backed underneath the client",
    );
}

#[test]
fn a_gl_drawable_asks_to_be_backed_at_its_own_extent() {
    // What the browser actually names. A GL client asking the server to own the
    // storage names the thing it renders into, and that is a GLX drawable, not a
    // core pixmap -- refusing it as "not a pixmap" is what left the browser
    // retrying a request it could never satisfy.
    let namespace = NamespaceId::from_raw(45);
    let pbuffer = XResourceId::new(0x1a00001, 1);
    let size = Size {
        width: 256,
        height: 128,
    };
    let mut runtime = XAuthorityRuntime::new();
    runtime
        .create_glx_pbuffer(namespace, pbuffer, 1, size)
        .unwrap();

    let request = runtime
        .dri3_pixmap_backing_request(namespace, pbuffer)
        .expect("a GL drawable with no buffer of its own asks to be backed");
    assert_eq!(request.size, size);

    let descriptor = sophia_protocol::DmaBufDescriptor {
        handle: sophia_protocol::BufferHandle::from_raw(runtime.next_dma_buf_handle()),
        size,
        format: sophia_protocol::DRM_FORMAT_XRGB8888,
        modifier: 0,
        plane_count: 1,
        planes: [
            Some(sophia_protocol::DmaBufPlaneDescriptor {
                offset: 0,
                stride: 1024,
            }),
            None,
            None,
            None,
        ],
    };
    runtime
        .adopt_dri3_pixmap_backing(namespace, pbuffer, descriptor, vec![test_plane_descriptor()])
        .unwrap();

    let (recovered, fds) = runtime.dri3_pixmap_buffers(namespace, pbuffer).unwrap();
    assert_eq!(recovered.size, size);
    assert_eq!(fds.len(), 1);
    // Backed once.
    assert!(runtime.dri3_pixmap_backing_request(namespace, pbuffer).is_none());
}

#[test]
fn the_root_is_not_a_drawable_to_hand_storage_for() {
    // The root is the session's own output. A client may name it in a DRI3
    // request, and it must not become a buffer handed to that client.
    let namespace = NamespaceId::from_raw(45);
    let runtime = XAuthorityRuntime::new();
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);

    assert!(runtime.dri3_pixmap_backing_request(namespace, root).is_none());
}

#[test]
fn an_originated_buffer_cannot_take_a_name_another_buffer_holds() {
    // Buffer identity is one space shared with every buffer a client imports.
    // An allocator counting from one of its own would stamp a name the renderer
    // already answers for, and the registry is keyed by exactly that name.
    let namespace = NamespaceId::from_raw(45);
    let imported = XResourceId::new(0x220807, 1);
    let pbuffer = XResourceId::new(0x1a00001, 1);
    let size = Size {
        width: 64,
        height: 48,
    };
    let mut runtime = XAuthorityRuntime::new();
    import_dri3_pixmap(&mut runtime, namespace, imported, 0x220807);
    let taken = runtime
        .dri3_pixmap_buffers(namespace, imported)
        .unwrap()
        .0
        .handle;

    runtime
        .create_glx_pbuffer(namespace, pbuffer, 1, size)
        .unwrap();
    let request = runtime
        .dri3_pixmap_backing_request(namespace, pbuffer)
        .expect("the pbuffer asks to be backed");
    assert_ne!(
        sophia_protocol::BufferHandle::from_raw(request.handle),
        taken,
        "the issued handle must not be one an imported buffer already holds",
    );

    // And a descriptor stamped with anything else is refused outright.
    let forged = sophia_protocol::DmaBufDescriptor {
        handle: taken,
        size,
        format: sophia_protocol::DRM_FORMAT_XRGB8888,
        modifier: 0,
        plane_count: 1,
        planes: [
            Some(sophia_protocol::DmaBufPlaneDescriptor {
                offset: 0,
                stride: 256,
            }),
            None,
            None,
            None,
        ],
    };
    assert!(
        runtime
            .adopt_dri3_pixmap_backing(namespace, pbuffer, forged, vec![test_plane_descriptor()])
            .is_err(),
        "a buffer stamped with a name it was not issued must be refused",
    );
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
