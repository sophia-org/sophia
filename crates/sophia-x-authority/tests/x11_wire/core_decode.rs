#[test]
fn x11_setup_failure_reply_encodes_native_failure() {
    let reply = encode_x11_setup_failure(
        XByteOrder::BigEndian,
        &XSetupFailure {
            major_version: 11,
            minor_version: 0,
            reason: b"unsupported".to_vec(),
        },
    )
    .unwrap();

    assert_eq!(reply[0], 0);
    assert_eq!(reply[1], b"unsupported".len() as u8);
    assert_eq!(read_u16(XByteOrder::BigEndian, &reply[2..4]), 11);
    assert_eq!(&reply[8..19], b"unsupported");
    assert_eq!(reply.len() % 4, 0);
}

#[test]
fn x11_core_decoder_maps_create_and_map_to_authority_packets() {
    let namespace = NamespaceId::from_raw(41);
    let create = decode_x11_core_request(
        context(namespace, 501, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, 0x220001, 10, 20, 640, 480),
    )
    .unwrap();
    let map = decode_x11_core_request(
        context(namespace, 502, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 8, 0x220001),
    )
    .unwrap();
    let map_subwindows = decode_x11_core_request(
        context(namespace, 503, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 9, X_SETUP_DEFAULT_ROOT),
    )
    .unwrap();
    let unmap = decode_x11_core_request(
        context(namespace, 504, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 10, 0x220001),
    )
    .unwrap();
    let configure = decode_x11_core_request(
        context(namespace, 505, XByteOrder::LittleEndian),
        &configure_window_request(XByteOrder::LittleEndian, 0x220001, 0x000c, &[12, 14]),
    )
    .unwrap();
    let attributes = decode_x11_core_request(
        context(namespace, 506, XByteOrder::LittleEndian),
        &change_window_attributes_request(XByteOrder::LittleEndian, X_SETUP_DEFAULT_ROOT),
    )
    .unwrap();
    let get_attributes = decode_x11_core_request(
        context(namespace, 507, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 3, X_SETUP_DEFAULT_ROOT),
    )
    .unwrap();

    let XWireRequest::CreateWindow {
        packet: create,
        background_pixel,
        event_mask,
        do_not_propagate_mask,
        parent,
        ..
    } = create
    else {
        panic!("expected create-window request");
    };
    assert_eq!(background_pixel, None);
    assert_eq!(event_mask, None);
    assert_eq!(do_not_propagate_mask, None);
    assert_eq!(parent, XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1));
    assert_eq!(create.namespace, namespace);
    assert_eq!(
        create.kind,
        XAuthorityRequestKind::CreateWindow {
            window: XResourceId::new(0x220001, 1),
            surface: SurfaceId::new(0x220001, 1),
            geometry: Rect {
                x: 10,
                y: 20,
                width: 640,
                height: 480,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        }
    );

    let XWireRequest::Authority(map) = map else {
        panic!("expected authority request");
    };
    assert_eq!(
        map.kind,
        XAuthorityRequestKind::MapWindow {
            window: XResourceId::new(0x220001, 1),
            generation: 2,
        }
    );
    assert_eq!(
        map_subwindows,
        XWireRequest::MapSubwindows {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        }
    );
    assert_eq!(
        unmap,
        XWireRequest::UnmapWindow {
            window: XResourceId::new(0x220001, 1),
        }
    );
    assert_eq!(
        configure,
        XWireRequest::ConfigureWindow {
            window: XResourceId::new(0x220001, 1),
            value_mask: 0x000c,
            x: None,
            y: None,
            width: Some(12),
            height: Some(14),
            border_width: None,
            sibling: None,
            stack_mode: None,
        }
    );
    let geometry = decode_x11_core_request(
        context(namespace, 508, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 14, X_SETUP_DEFAULT_ROOT),
    )
    .unwrap();
    let tree = decode_x11_core_request(
        context(namespace, 509, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 15, X_SETUP_DEFAULT_ROOT),
    )
    .unwrap();
    let list_properties = decode_x11_core_request(
        context(namespace, 510, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 21, X_SETUP_DEFAULT_ROOT),
    )
    .unwrap();
    assert_eq!(
        geometry,
        XWireRequest::GetGeometry {
            drawable: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        }
    );
    assert_eq!(
        tree,
        XWireRequest::QueryTree {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        }
    );
    assert_eq!(
        list_properties,
        XWireRequest::ListProperties {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        }
    );
    let translate = decode_x11_core_request(
        context(namespace, 511, XByteOrder::LittleEndian),
        &translate_coordinates_request(
            XByteOrder::LittleEndian,
            X_SETUP_DEFAULT_ROOT,
            X_SETUP_DEFAULT_ROOT,
            12,
            34,
        ),
    )
    .unwrap();
    assert_eq!(
        translate,
        XWireRequest::TranslateCoordinates {
            source: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            destination: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            src_x: 12,
            src_y: 34,
        }
    );
    assert_eq!(
        get_attributes,
        XWireRequest::GetWindowAttributes {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        }
    );
    assert_eq!(
        attributes,
        XWireRequest::ChangeWindowAttributes {
            cursor: None,
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            background_pixmap: None,
            background_pixel: None,
            override_redirect: None,
            event_mask: None,
            do_not_propagate_mask: None,
            colormap: None,
            bit_gravity: None,
            win_gravity: None,
        }
    );
    let modifier_mapping = decode_x11_core_request(
        context(namespace, 512, XByteOrder::LittleEndian),
        &[119, 0, 1, 0],
    )
    .unwrap();
    assert_eq!(modifier_mapping, XWireRequest::GetModifierMapping);
    let pointer_mapping = decode_x11_core_request(
        context(namespace, 513, XByteOrder::LittleEndian),
        &[117, 0, 1, 0],
    )
    .unwrap();
    assert_eq!(pointer_mapping, XWireRequest::GetPointerMapping);
    let keyboard_mapping = decode_x11_core_request(
        context(namespace, 514, XByteOrder::LittleEndian),
        &[101, 0, 2, 0, 8, 4, 0, 0],
    )
    .unwrap();
    assert_eq!(
        keyboard_mapping,
        XWireRequest::GetKeyboardMapping {
            first_keycode: 8,
            count: 4,
        }
    );
}

#[test]
fn x11_core_decoder_preserves_override_redirect_attributes() {
    let namespace = NamespaceId::from_raw(41);
    let create = decode_x11_core_request(
        context(namespace, 508, XByteOrder::LittleEndian),
        &create_window_override_redirect_request(
            XByteOrder::LittleEndian,
            0x220011,
            0,
            0,
            800,
            24,
        ),
    )
    .unwrap();
    assert!(matches!(
        create,
        XWireRequest::CreateWindow {
            override_redirect: true,
            ..
        }
    ));

    let change = decode_x11_core_request(
        context(namespace, 509, XByteOrder::LittleEndian),
        &change_window_override_redirect_request(
            XByteOrder::LittleEndian,
            0x220011,
            false,
        ),
    )
    .unwrap();
    assert!(matches!(
        change,
        XWireRequest::ChangeWindowAttributes {
            override_redirect: Some(false),
            ..
        }
    ));
}

#[test]
fn keyboard_mapping_request_uses_body_keycode_and_count_bytes() {
    let namespace = NamespaceId::from_raw(44);
    for (byte_order, request) in [
        (XByteOrder::LittleEndian, [101, 0, 2, 0, 8, 248, 0, 0]),
        (XByteOrder::BigEndian, [101, 0, 0, 2, 8, 248, 0, 0]),
    ] {
        assert_eq!(
            decode_x11_core_request(context(namespace, 514, byte_order), &request).unwrap(),
            XWireRequest::GetKeyboardMapping {
                first_keycode: 8,
                count: 248,
            }
        );
    }
}

#[test]
fn x11_core_decoder_preserves_window_background_pixel() {
    let namespace = NamespaceId::from_raw(44);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let create = decode_x11_core_request(
            context(namespace, 500, byte_order),
            &create_window_background_request(byte_order, 0x220002, 10, 20, 320, 200, 0x0012_3456),
        )
        .unwrap();
        let XWireRequest::CreateWindow {
            background_pixel, ..
        } = create
        else {
            panic!("expected create-window request");
        };
        assert_eq!(background_pixel, Some(0x0012_3456));
    }
}

#[test]
fn x11_core_decoder_captures_destroy_window_requests() {
    let namespace = NamespaceId::from_raw(41);
    let destroy = decode_x11_core_request(
        context(namespace, 502, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 4, 0x220001),
    )
    .unwrap();

    assert_eq!(
        destroy,
        XWireRequest::DestroyWindow {
            window: XResourceId::new(0x220001, 1),
        }
    );
}

#[test]
fn x11_core_decoder_captures_destroy_subwindows_requests() {
    let namespace = NamespaceId::from_raw(41);
    let destroy = decode_x11_core_request(
        context(namespace, 503, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 5, 0x220001),
    )
    .unwrap();

    // Opcode 5 shares DestroyWindow's 8-byte form, so the decode is only
    // meaningful if it lands on the right variant: the two differ in whether
    // the named window survives.
    assert_eq!(
        destroy,
        XWireRequest::DestroySubwindows {
            window: XResourceId::new(0x220001, 1),
        }
    );
}

#[test]
fn x11_core_decoder_maps_selection_requests_to_authority_packets() {
    let namespace = NamespaceId::from_raw(42);
    let set_owner = decode_x11_core_request(
        context(namespace, 503, XByteOrder::BigEndian),
        &set_selection_owner_request(XByteOrder::BigEndian, 0x220001, 1, 10),
    )
    .unwrap();
    let convert = decode_x11_core_request(
        context(namespace, 504, XByteOrder::BigEndian),
        &convert_selection_request(XByteOrder::BigEndian, 0x220002, 1, 2, 3, 11),
    )
    .unwrap();
    let get_owner = decode_x11_core_request(
        context(namespace, 505, XByteOrder::BigEndian),
        &resource_request(XByteOrder::BigEndian, 23, 1),
    )
    .unwrap();
    let grab_button = decode_x11_core_request(
        context(namespace, 506, XByteOrder::BigEndian),
        &grab_button_request(
            XByteOrder::BigEndian,
            X_SETUP_DEFAULT_ROOT,
            0x001c,
            1,
            0x0040,
        ),
    )
    .unwrap();
    let ungrab_button = decode_x11_core_request(
        context(namespace, 507, XByteOrder::BigEndian),
        &ungrab_button_request(XByteOrder::BigEndian, X_SETUP_DEFAULT_ROOT, 1, 0x0040),
    )
    .unwrap();
    let grab = decode_x11_core_request(
        context(namespace, 508, XByteOrder::BigEndian),
        &[36, 0, 0, 1],
    )
    .unwrap();
    let ungrab = decode_x11_core_request(
        context(namespace, 509, XByteOrder::BigEndian),
        &[37, 0, 0, 1],
    )
    .unwrap();

    let XWireRequest::Authority(set_owner) = set_owner else {
        panic!("expected authority request");
    };
    assert_eq!(
        set_owner.kind,
        XAuthorityRequestKind::SetSelectionOwner {
            selection: 1,
            owner: Some(XResourceId::new(0x220001, 1)),
            timestamp: 10,
            selection_timestamp: 10,
            kind: XSelectionChangeKind::SetOwner,
        }
    );

    let XWireRequest::Authority(convert) = convert else {
        panic!("expected authority request");
    };
    assert_eq!(
        convert.kind,
        XAuthorityRequestKind::RequestSelection {
            requestor: XResourceId::new(0x220002, 1),
            selection: 1,
            target: 2,
            target_name: "atom:2".to_owned(),
            property: 3,
            time: 11,
            transfer: sophia_protocol::PortalTransferId::from_raw(504),
        }
    );
    assert_eq!(get_owner, XWireRequest::GetSelectionOwner { selection: 1 });
    assert_eq!(
        grab_button,
        XWireRequest::GrabButton {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            event_mask: 0x001c,
            button: 1,
            modifiers: 0x0040,
            owner_events: true,
            pointer_mode: 1,
            keyboard_mode: 1,
        }
    );
    assert_eq!(
        ungrab_button,
        XWireRequest::UngrabButton {
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            button: 1,
            modifiers: 0x0040,
        }
    );
    assert_eq!(grab, XWireRequest::GrabServer);
    assert_eq!(ungrab, XWireRequest::UngrabServer);
}

#[test]
fn active_keyboard_pointer_key_and_allow_events_requests_decode() {
    let namespace = NamespaceId::from_raw(45);
    let window = X_SETUP_DEFAULT_ROOT;
    let mut grab_pointer = vec![26, 1, 6, 0];
    grab_pointer.extend_from_slice(&window.to_le_bytes());
    grab_pointer.extend_from_slice(&0x004cu16.to_le_bytes());
    grab_pointer.extend_from_slice(&[1, 0]);
    grab_pointer.extend_from_slice(&[0; 8]);
    grab_pointer.extend_from_slice(&7u32.to_le_bytes());
    assert_eq!(
        decode_x11_core_request(
            context(namespace, 1, XByteOrder::LittleEndian),
            &grab_pointer
        )
        .unwrap(),
        XWireRequest::GrabPointer {
            window: XResourceId::new(u64::from(window), 1),
            event_mask: 0x004c,
            owner_events: true,
            pointer_mode: 1,
            keyboard_mode: 0,
            time: 7,
        }
    );
    let mut grab_keyboard = vec![31, 0, 4, 0];
    grab_keyboard.extend_from_slice(&window.to_le_bytes());
    grab_keyboard.extend_from_slice(&8u32.to_le_bytes());
    grab_keyboard.extend_from_slice(&[0, 1, 0, 0]);
    assert_eq!(
        decode_x11_core_request(
            context(namespace, 2, XByteOrder::LittleEndian),
            &grab_keyboard
        )
        .unwrap(),
        XWireRequest::GrabKeyboard {
            window: XResourceId::new(u64::from(window), 1),
            owner_events: false,
            pointer_mode: 0,
            keyboard_mode: 1,
            time: 8,
        }
    );
    let mut grab_key = vec![33, 1, 4, 0];
    grab_key.extend_from_slice(&window.to_le_bytes());
    grab_key.extend_from_slice(&0x8000u16.to_le_bytes());
    grab_key.extend_from_slice(&[38, 1, 0, 0, 0, 0]);
    assert_eq!(
        decode_x11_core_request(context(namespace, 3, XByteOrder::LittleEndian), &grab_key)
            .unwrap(),
        XWireRequest::GrabKey {
            window: XResourceId::new(u64::from(window), 1),
            key: 38,
            modifiers: 0x8000,
            owner_events: true,
            pointer_mode: 1,
            keyboard_mode: 0,
        }
    );
    let allow = [35, 6, 2, 0, 9, 0, 0, 0];
    assert_eq!(
        decode_x11_core_request(context(namespace, 4, XByteOrder::LittleEndian), &allow).unwrap(),
        XWireRequest::AllowEvents { mode: 6, time: 9 }
    );
}

#[test]
fn x11_core_decoder_captures_change_property_and_table_updates() {
    let namespace = NamespaceId::from_raw(43);
    let decoded = decode_x11_core_request(
        context(namespace, 505, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            0x220003,
            7,
            8,
            8,
            b"hello",
        ),
    )
    .unwrap();
    let XWireRequest::ChangeProperty(change) = decoded else {
        panic!("expected property change");
    };

    let mut properties = XPropertyTable::new();
    let record = properties.apply_change(namespace, change).unwrap();

    assert_eq!(record.window, XResourceId::new(0x220003, 1));
    assert_eq!(record.property, 7);
    assert_eq!(record.property_type, 8);
    assert_eq!(record.format, 8);
    assert_eq!(record.bytes, b"hello");
    assert_eq!(record.generation, 1);
}

#[test]
fn x11_property_table_appends_and_rejects_type_mismatch() {
    let namespace = NamespaceId::from_raw(44);
    let mut properties = XPropertyTable::new();
    let window = XResourceId::new(0x220004, 1);

    properties
        .apply_change(
            namespace,
            XPropertyChange {
                mode: XPropertyMode::Replace,
                window,
                property: 7,
                property_type: 8,
                format: 8,
                bytes: b"hello".to_vec(),
            },
        )
        .unwrap();
    let appended = properties
        .apply_change(
            namespace,
            XPropertyChange {
                mode: XPropertyMode::Append,
                window,
                property: 7,
                property_type: 8,
                format: 8,
                bytes: b" world".to_vec(),
            },
        )
        .unwrap();

    assert_eq!(appended.bytes, b"hello world");
    assert_eq!(appended.generation, 2);
    assert_eq!(
        properties.apply_change(
            namespace,
            XPropertyChange {
                mode: XPropertyMode::Append,
                window,
                property: 7,
                property_type: 9,
                format: 8,
                bytes: b"!".to_vec(),
            },
        ),
        Err(XPropertyError::TypeMismatch)
    );
}

#[test]
fn engine_presentation_state_materializes_exact_icccm_and_ewmh_properties() {
    let namespace = NamespaceId::from_raw(7);
    let window = XResourceId::new(0x210, 1);
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let changed = apply_engine_presentation_state(
        &mut properties,
        &mut atoms,
        namespace,
        window,
        XByteOrder::LittleEndian,
        PolicyPresentationState::default(),
    )
    .unwrap();
    assert_eq!(changed.len(), 2);
    let net_state = atoms.atom(X_ATOM_NAME_NET_WM_STATE).unwrap();
    let wm_state = atoms.atom(X_ATOM_NAME_WM_STATE).unwrap();
    assert_eq!(
        properties.get(namespace, window, net_state).unwrap().bytes,
        Vec::<u8>::new()
    );
    assert_eq!(
        properties.get(namespace, window, wm_state).unwrap().bytes,
        [1_u32.to_le_bytes(), 0_u32.to_le_bytes()].concat()
    );
    assert!(
        apply_engine_presentation_state(
            &mut properties,
            &mut atoms,
            namespace,
            window,
            XByteOrder::LittleEndian,
            PolicyPresentationState::default(),
        )
        .unwrap()
        .is_empty()
    );

    let changed = apply_engine_presentation_state(
        &mut properties,
        &mut atoms,
        namespace,
        window,
        XByteOrder::LittleEndian,
        PolicyPresentationState {
            maximized: true,
            ..PolicyPresentationState::default()
        },
    )
    .unwrap();
    assert_eq!(changed, vec![net_state]);
    let maximized_vert = atoms
        .atom(X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_VERT)
        .unwrap();
    let maximized_horz = atoms
        .atom(X_ATOM_NAME_NET_WM_STATE_MAXIMIZED_HORZ)
        .unwrap();
    assert_eq!(
        properties.get(namespace, window, net_state).unwrap().bytes,
        [
            maximized_vert.to_le_bytes(),
            maximized_horz.to_le_bytes()
        ]
        .concat()
    );

    let before = properties.clone();
    assert_eq!(
        apply_engine_presentation_state(
            &mut properties,
            &mut atoms,
            namespace,
            window,
            XByteOrder::LittleEndian,
            PolicyPresentationState {
                fullscreen: true,
                minimized: true,
                maximized: false,
            },
        ),
        Err(XPresentationPropertyError::InvalidState)
    );
    assert_eq!(properties, before);

    assert_eq!(
        properties.apply_change(
            namespace,
            XPropertyChange {
                mode: XPropertyMode::Replace,
                window,
                property: net_state,
                property_type: X_ATOM_ATOM,
                format: 32,
                bytes: Vec::new(),
            },
        ),
        Err(XPropertyError::AuthorityOwned)
    );
    let read = properties
        .read_property(
            namespace,
            XPropertyRead {
                delete: true,
                window,
                property: net_state,
                property_type: X_PROPERTY_ANY_TYPE,
                long_offset: 0,
                long_length: 8,
            },
        )
        .unwrap();
    assert!(!read.deleted);
    assert!(properties.get(namespace, window, net_state).is_some());
}

#[test]
fn x11_atom_table_resolves_predefined_and_dynamic_names() {
    let mut atoms = XAtomTable::new();

    assert_eq!(atoms.atom(X_ATOM_NAME_WM_CLASS), Some(X_ATOM_WM_CLASS));
    assert_eq!(atoms.name(X_ATOM_WM_NAME), Some(X_ATOM_NAME_WM_NAME));
    assert_eq!(
        atoms.atom(X_ATOM_NAME_RESOURCE_MANAGER),
        Some(X_ATOM_RESOURCE_MANAGER)
    );

    let net_wm_name = atoms.intern(X_ATOM_NAME_NET_WM_NAME, false).unwrap();
    assert!(net_wm_name.is_some());
    assert_eq!(
        atoms.intern(X_ATOM_NAME_NET_WM_NAME, true).unwrap(),
        net_wm_name
    );
    assert!(atoms.intern("SOPHIA PRINTABLE", false).unwrap().is_some());
    assert_eq!(atoms.intern("SOPHIA_UNKNOWN", true).unwrap(), None);
    assert!(atoms.intern("", false).is_err());
}

#[test]
fn x11_core_decoder_captures_atom_requests() {
    let namespace = NamespaceId::from_raw(45);
    let intern = decode_x11_core_request(
        context(namespace, 506, XByteOrder::LittleEndian),
        &intern_atom_request(XByteOrder::LittleEndian, false, X_ATOM_NAME_NET_WM_NAME),
    )
    .unwrap();
    assert_eq!(
        intern,
        XWireRequest::InternAtom {
            only_if_exists: false,
            name: X_ATOM_NAME_NET_WM_NAME.to_owned(),
        }
    );

    let get_name = decode_x11_core_request(
        context(namespace, 507, XByteOrder::BigEndian),
        &get_atom_name_request(XByteOrder::BigEndian, X_ATOM_WM_CLASS),
    )
    .unwrap();
    assert_eq!(
        get_name,
        XWireRequest::GetAtomName {
            atom: X_ATOM_WM_CLASS
        }
    );
}

#[test]
fn x11_core_decoder_captures_get_property_requests() {
    let namespace = NamespaceId::from_raw(45);
    let get_property = decode_x11_core_request(
        context(namespace, 507, XByteOrder::LittleEndian),
        &get_property_request(
            XByteOrder::LittleEndian,
            false,
            0x220007,
            X_ATOM_WM_NAME,
            X_PROPERTY_ANY_TYPE,
            1,
            2,
        ),
    )
    .unwrap();

    assert_eq!(
        get_property,
        XWireRequest::GetProperty(XPropertyRead {
            delete: false,
            window: XResourceId::new(0x220007, 1),
            property: X_ATOM_WM_NAME,
            property_type: X_PROPERTY_ANY_TYPE,
            long_offset: 1,
            long_length: 2,
        })
    );
}

#[test]
fn x11_core_decoder_captures_create_gc_requests() {
    let namespace = NamespaceId::from_raw(45);
    let create_gc = decode_x11_core_request(
        context(namespace, 507, XByteOrder::LittleEndian),
        &create_gc_request(XByteOrder::LittleEndian, 0x220010, X_SETUP_DEFAULT_ROOT),
    )
    .unwrap();

    assert_eq!(
        create_gc,
        XWireRequest::CreateGraphicsContext {
            gc: XResourceId::new(0x220010, 1),
            drawable: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            values: XGraphicsContextValues::default(),
        }
    );

    let clip = decode_x11_core_request(
        context(namespace, 508, XByteOrder::LittleEndian),
        &set_clip_rectangles_request(XByteOrder::LittleEndian, 0x220010, &[(2, 3, 20, 10)]),
    )
    .unwrap();
    assert_eq!(
        clip,
        XWireRequest::SetClipRectangles {
            clip_x_origin: 0,
            clip_y_origin: 0,
            gc: XResourceId::new(0x220010, 1),
            rectangles: vec![Rect {
                x: 2,
                y: 3,
                width: 20,
                height: 10,
            }],
        }
    );

    let clear = decode_x11_core_request(
        context(namespace, 509, XByteOrder::LittleEndian),
        &clear_area_request(XByteOrder::LittleEndian, true, 0x220010, 3, 4, 40, 30),
    )
    .unwrap();

    assert_eq!(
        clear,
        XWireRequest::ClearArea {
            exposures: true,
            window: XResourceId::new(0x220010, 1),
            x: 3,
            y: 4,
            width: 40,
            height: 30,
        }
    );
}

#[test]
fn x11_core_decoder_preserves_gc_raster_values_in_both_byte_orders() {
    let namespace = NamespaceId::from_raw(45);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let request = create_gc_values_request(
            byte_order,
            0x220020,
            X_SETUP_DEFAULT_ROOT,
            6,
            0x00ff_00ff,
            0x0012_3456,
            0x0065_4321,
            3,
            0x220021,
        );
        let decoded =
            decode_x11_core_request(context(namespace, 508, byte_order), &request).unwrap();
        let XWireRequest::CreateGraphicsContext { values, .. } = decoded else {
            panic!("expected CreateGC");
        };
        assert_eq!(values.function, 6);
        assert_eq!(values.plane_mask, 0x00ff_00ff);
        assert_eq!(values.foreground, 0x0012_3456);
        assert_eq!(values.background, 0x0065_4321);
        assert_eq!(values.line_width, 3);
        assert_eq!(values.font, Some(XResourceId::new(0x220021, 1)));
    }
}

#[test]
fn x11_core_decoder_preserves_change_gc_mask_and_values_in_both_byte_orders() {
    let namespace = NamespaceId::from_raw(45);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let decoded = decode_x11_core_request(
            context(namespace, 508, byte_order),
            &change_gc_request(
                byte_order,
                0x220020,
                (1 << 2) | (1 << 16) | (1 << 17),
                &[0x0012_3456, 0, 7],
            ),
        )
        .unwrap();
        let XWireRequest::ChangeGraphicsContext {
            gc,
            value_mask,
            values,
        } = decoded
        else {
            panic!("expected ChangeGC");
        };
        assert_eq!(gc, XResourceId::new(0x220020, 1));
        assert_eq!(value_mask, (1 << 2) | (1 << 16) | (1 << 17));
        assert_eq!(values.foreground, 0x0012_3456);
        assert!(!values.graphics_exposures);
        assert_eq!(values.clip_x_origin, 7);
    }
}
