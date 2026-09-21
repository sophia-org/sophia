#[test]
fn x11_dispatch_create_colormap_accepts_root_visual() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = decode_x11_core_request(
        context(namespace, 526, XByteOrder::LittleEndian),
        &create_colormap_request(
            XByteOrder::LittleEndian,
            0x200001,
            X_SETUP_DEFAULT_ROOT,
            X_SETUP_DEFAULT_VISUAL,
        ),
    )
    .unwrap();

    assert_eq!(
        request,
        XWireRequest::CreateColormap {
            alloc: 0,
            colormap: XResourceId::new(0x200001, 1),
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            visual: X_SETUP_DEFAULT_VISUAL,
        }
    );

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 78),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty());
    assert_eq!(
        runtime
            .colormap_visual(namespace, XResourceId::new(0x200001, 1))
            .unwrap(),
        X_SETUP_DEFAULT_VISUAL
    );
}

#[test]
fn x11_dispatch_colormap_lifecycle_enforces_visual_and_resource_errors() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let dispatch = |request,
                    sequence,
                    runtime: &mut XAuthorityRuntime,
                    atoms: &mut XAtomTable,
                    properties: &mut XPropertyTable| {
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 78),
            request,
            runtime,
            atoms,
            properties,
        )
    };

    let argb_bytes = create_colormap_request(
        XByteOrder::LittleEndian,
        0x200002,
        X_SETUP_DEFAULT_ROOT,
        X_SETUP_ARGB_VISUAL,
    );
    let argb = decode_x11_core_request(
        context(namespace, 527, XByteOrder::LittleEndian),
        &argb_bytes,
    )
    .unwrap();
    assert!(
        dispatch(
            argb.clone(),
            1,
            &mut runtime,
            &mut atoms,
            &mut properties
        )
        .outputs
        .is_empty()
    );

    let duplicate = dispatch(
        argb,
        2,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(duplicate[0][1], XErrorCode::BadIdChoice.wire_code());
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &duplicate[0][4..8]),
        0x200002
    );

    let bad_visual = decode_x11_core_request(
        context(namespace, 528, XByteOrder::LittleEndian),
        &create_colormap_request(
            XByteOrder::LittleEndian,
            0x200003,
            X_SETUP_DEFAULT_ROOT,
            0xdead_beef,
        ),
    )
    .unwrap();
    let bad_visual = dispatch(
        bad_visual,
        3,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(bad_visual[0][1], XErrorCode::BadMatch.wire_code());
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &bad_visual[0][4..8]),
        0xdead_beef
    );

    let mut alloc_all_bytes = create_colormap_request(
        XByteOrder::LittleEndian,
        0x200004,
        X_SETUP_DEFAULT_ROOT,
        X_SETUP_DEFAULT_VISUAL,
    );
    alloc_all_bytes[1] = 1;
    let alloc_all = decode_x11_core_request(
        context(namespace, 529, XByteOrder::LittleEndian),
        &alloc_all_bytes,
    )
    .unwrap();
    let alloc_all = dispatch(
        alloc_all,
        4,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(alloc_all[0][1], XErrorCode::BadMatch.wire_code());

    let mut invalid_alloc_bytes = create_colormap_request(
        XByteOrder::LittleEndian,
        0x200005,
        X_SETUP_DEFAULT_ROOT,
        X_SETUP_DEFAULT_VISUAL,
    );
    invalid_alloc_bytes[1] = 2;
    let invalid_alloc = decode_x11_core_request(
        context(namespace, 530, XByteOrder::LittleEndian),
        &invalid_alloc_bytes,
    )
    .unwrap();
    let invalid_alloc = dispatch(
        invalid_alloc,
        5,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(invalid_alloc[0][1], XErrorCode::BadValue.wire_code());
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &invalid_alloc[0][4..8]),
        2
    );

    let free = decode_x11_core_request(
        context(namespace, 531, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 79, 0x200002),
    )
    .unwrap();
    let free = dispatch_x11_wire_request(
        dispatch_context(namespace, 6, XByteOrder::LittleEndian, 79),
        free,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(free.outputs.is_empty());
    assert!(
        runtime
            .colormap_visual(namespace, XResourceId::new(0x200002, 1))
            .is_err()
    );

    let free_default = decode_x11_core_request(
        context(namespace, 532, XByteOrder::LittleEndian),
        &resource_request(
            XByteOrder::LittleEndian,
            79,
            X_SETUP_DEFAULT_COLORMAP,
        ),
    )
    .unwrap();
    let free_default = dispatch_x11_wire_request(
        dispatch_context(namespace, 7, XByteOrder::LittleEndian, 79),
        free_default,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(free_default.outputs.is_empty());
}

#[test]
fn x11_dispatch_alloc_named_color_encodes_exact_palette_values_in_both_orders() {
    let namespace = NamespaceId::from_raw(45);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        let request = decode_x11_core_request(
            context(namespace, 542, byte_order),
            &alloc_named_color_request(
                byte_order,
                X_SETUP_DEFAULT_COLORMAP,
                "Light Gray",
            ),
        )
        .unwrap();

        assert_eq!(
            request,
            XWireRequest::AllocNamedColor {
                colormap: XResourceId::new(u64::from(X_SETUP_DEFAULT_COLORMAP), 1),
                name: "Light Gray".to_owned(),
            }
        );
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, 1, byte_order, 85),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let encoded = result.encoded_outputs(byte_order);
        assert_eq!(encoded[0][0], 1);
        assert_eq!(read_u32(byte_order, &encoded[0][8..12]), 0x00d3_d3d3);
        for offset in [12, 14, 16, 18, 20, 22] {
            assert_eq!(read_u16(byte_order, &encoded[0][offset..offset + 2]), 0xd3d3);
        }

        let unknown = decode_x11_core_request(
            context(namespace, 543, byte_order),
            &alloc_named_color_request(
                byte_order,
                X_SETUP_DEFAULT_COLORMAP,
                "not-a-retained-color",
            ),
        )
        .unwrap();
        let unknown = dispatch_x11_wire_request(
            dispatch_context(namespace, 2, byte_order, 85),
            unknown,
            &mut runtime,
            &mut atoms,
            &mut properties,
        )
        .encoded_outputs(byte_order);
        assert_eq!(unknown[0][0], 0);
        assert_eq!(unknown[0][1], XErrorCode::BadName.wire_code());
    }
}

#[test]
fn x11_dispatch_alloc_color_returns_quantized_true_color_in_both_orders() {
    let namespace = NamespaceId::from_raw(45);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        let request = decode_x11_core_request(
            context(namespace, 544, byte_order),
            &alloc_color_request(
                byte_order,
                X_SETUP_DEFAULT_COLORMAP,
                0x1234,
                0xabcd,
                0x80ff,
            ),
        )
        .unwrap();

        assert_eq!(
            request,
            XWireRequest::AllocColor {
                colormap: XResourceId::new(u64::from(X_SETUP_DEFAULT_COLORMAP), 1),
                red: 0x1234,
                green: 0xabcd,
                blue: 0x80ff,
            }
        );
        let encoded = dispatch_x11_wire_request(
            dispatch_context(namespace, 3, byte_order, 84),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        )
        .encoded_outputs(byte_order);
        assert_eq!(encoded[0][0], 1);
        assert_eq!(read_u16(byte_order, &encoded[0][8..10]), 0x1212);
        assert_eq!(read_u16(byte_order, &encoded[0][10..12]), 0xabab);
        assert_eq!(read_u16(byte_order, &encoded[0][12..14]), 0x8080);
        assert_eq!(read_u32(byte_order, &encoded[0][16..20]), 0x0012_ab80);
    }
}

#[test]
fn x11_dispatch_alloc_color_validates_colormap_and_preserves_argb_alpha() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    runtime
        .create_colormap(
            namespace,
            XResourceId::new(0x200010, 1),
            X_SETUP_ARGB_VISUAL,
            1,
        )
        .unwrap();

    let argb = decode_x11_core_request(
        context(namespace, 545, XByteOrder::LittleEndian),
        &alloc_color_request(
            XByteOrder::LittleEndian,
            0x200010,
            0x1234,
            0xabcd,
            0x80ff,
        ),
    )
    .unwrap();
    let argb = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 84),
        argb,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &argb[0][16..20]),
        0xff12_ab80
    );

    let invalid = decode_x11_core_request(
        context(namespace, 546, XByteOrder::LittleEndian),
        &alloc_color_request(
            XByteOrder::LittleEndian,
            0x200011,
            0,
            0,
            0,
        ),
    )
    .unwrap();
    let invalid = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 84),
        invalid,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(invalid[0][1], XErrorCode::BadColor.wire_code());
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &invalid[0][4..8]),
        0x200011
    );
}

#[test]
fn x11_dispatch_create_window_requires_matching_depth_visual_and_colormap() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    runtime
        .create_colormap(
            namespace,
            XResourceId::new(0x200020, 1),
            X_SETUP_ARGB_VISUAL,
            1,
        )
        .unwrap();

    let create = |window, depth, visual, colormap| {
        decode_x11_core_request(
            context(namespace, u64::from(window), XByteOrder::LittleEndian),
            &create_window_visual_request(
                XByteOrder::LittleEndian,
                window,
                depth,
                visual,
                colormap,
            ),
        )
        .unwrap()
    };
    let valid = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create(0x220020, 32, X_SETUP_ARGB_VISUAL, Some(0x200020)),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(valid.response.is_some());
    assert_eq!(
        runtime.window_visual(XResourceId::new(0x220020, 1)),
        (
            32,
            X_SETUP_ARGB_VISUAL,
            XResourceId::new(0x200020, 1)
        )
    );

    for (sequence, window, depth, visual, colormap, code) in [
        (
            2,
            0x220021,
            24,
            X_SETUP_ARGB_VISUAL,
            Some(0x200020),
            XErrorCode::BadMatch,
        ),
        (
            3,
            0x220022,
            32,
            X_SETUP_ARGB_VISUAL,
            None,
            XErrorCode::BadMatch,
        ),
        (
            4,
            0x220023,
            32,
            X_SETUP_ARGB_VISUAL,
            Some(X_SETUP_DEFAULT_COLORMAP),
            XErrorCode::BadMatch,
        ),
        (
            5,
            0x220024,
            32,
            X_SETUP_ARGB_VISUAL,
            Some(0x200099),
            XErrorCode::BadColor,
        ),
    ] {
        let rejected = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 1),
            create(window, depth, visual, colormap),
            &mut runtime,
            &mut atoms,
            &mut properties,
        )
        .encoded_outputs(XByteOrder::LittleEndian);
        assert_eq!(rejected[0][0], 0);
        assert_eq!(rejected[0][1], code.wire_code());
        assert!(
            runtime
                .validate_window_access(namespace, XResourceId::new(u64::from(window), 1))
                .is_err()
        );
    }
}

#[test]
fn x11_dispatch_reads_bounded_property_slices() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let utf8 = atoms
        .intern(X_ATOM_NAME_UTF8_STRING, false)
        .unwrap()
        .unwrap();
    let net_wm_name = atoms
        .intern(X_ATOM_NAME_NET_WM_NAME, false)
        .unwrap()
        .unwrap();
    let window = 0x220008;

    let create = decode_x11_core_request(
        context(namespace, 513, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 300, 200),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let title = b"Secret Terminal Title";
    let change = decode_x11_core_request(
        context(namespace, 514, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            window,
            net_wm_name,
            utf8,
            8,
            title,
        ),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 18),
        change,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let read = decode_x11_core_request(
        context(namespace, 515, XByteOrder::LittleEndian),
        &get_property_request(
            XByteOrder::LittleEndian,
            false,
            window,
            net_wm_name,
            X_PROPERTY_ANY_TYPE,
            1,
            2,
        ),
    )
    .unwrap();
    let read = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 20),
        read,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = read.encoded_outputs(XByteOrder::LittleEndian);

    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][1], 8);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), utf8);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[0][12..16]),
        u32::try_from(title.len() - 12).unwrap()
    );
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][16..20]), 8);
    assert_eq!(&encoded[0][32..40], &title[4..12]);
}

#[test]
fn x_property_reads_clamp_large_ceilings_and_delete_only_complete_matches() {
    let namespace = NamespaceId::from_raw(45);
    let window = XResourceId::new(0x220009, 1);
    let property = 71;
    let property_type = 72;
    let bytes = b"bounded clipboard value";
    let mut properties = XPropertyTable::new();
    properties
        .apply_change(
            namespace,
            XPropertyChange {
                mode: XPropertyMode::Replace,
                window,
                property,
                property_type,
                format: 8,
                bytes: bytes.to_vec(),
            },
        )
        .unwrap();

    let partial = properties
        .read_property(
            namespace,
            XPropertyRead {
                delete: true,
                window,
                property,
                property_type,
                long_offset: 0,
                long_length: 1,
            },
        )
        .unwrap();
    assert_eq!(partial.reply.bytes, bytes[..4]);
    assert_eq!(
        partial.reply.bytes_after,
        u32::try_from(bytes.len() - 4).unwrap()
    );
    assert!(!partial.deleted);
    assert!(properties.get(namespace, window, property).is_some());

    let type_mismatch = properties
        .read_property(
            namespace,
            XPropertyRead {
                delete: true,
                window,
                property,
                property_type: property_type + 1,
                long_offset: 0,
                long_length: u32::MAX,
            },
        )
        .unwrap();
    assert!(type_mismatch.reply.bytes.is_empty());
    assert_eq!(
        type_mismatch.reply.bytes_after,
        u32::try_from(bytes.len()).unwrap()
    );
    assert!(!type_mismatch.deleted);
    assert!(properties.get(namespace, window, property).is_some());

    assert_eq!(
        properties.read_property(
            namespace,
            XPropertyRead {
                delete: true,
                window,
                property,
                property_type,
                long_offset: u32::MAX,
                long_length: u32::MAX,
            },
        ),
        Err(XPropertyError::InvalidOffset)
    );
    assert!(properties.get(namespace, window, property).is_some());

    let complete = properties
        .read_property(
            namespace,
            XPropertyRead {
                delete: true,
                window,
                property,
                property_type,
                long_offset: 0,
                long_length: u32::MAX,
            },
        )
        .unwrap();
    assert_eq!(complete.reply.bytes, bytes);
    assert_eq!(complete.reply.bytes_after, 0);
    assert!(complete.deleted);
    assert!(properties.get(namespace, window, property).is_none());

    let missing = properties
        .read_property(
            namespace,
            XPropertyRead {
                delete: true,
                window,
                property,
                property_type,
                long_offset: 0,
                long_length: u32::MAX,
            },
        )
        .unwrap();
    assert_eq!(missing.reply.property_type, X_PROPERTY_ANY_TYPE);
    assert!(!missing.deleted);
}

#[test]
fn x11_dispatch_deletes_a_fully_read_property_after_the_reply() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let utf8 = atoms
        .intern(X_ATOM_NAME_UTF8_STRING, false)
        .unwrap()
        .unwrap();
    let clipboard = atoms.intern("CLIPBOARD", false).unwrap().unwrap();
    let window = 0x22000a;
    let bytes = b"kitty-shaped selection";

    let create = decode_x11_core_request(
        context(namespace, 516, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 300, 200),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let change = decode_x11_core_request(
        context(namespace, 517, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            window,
            clipboard,
            utf8,
            8,
            bytes,
        ),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 18),
        change,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let read = decode_x11_core_request(
        context(namespace, 518, XByteOrder::LittleEndian),
        &get_property_request(
            XByteOrder::LittleEndian,
            true,
            window,
            clipboard,
            utf8,
            0,
            u32::MAX,
        ),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 20),
        read,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);

    assert_eq!(encoded.len(), 2);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(&encoded[0][32..32 + bytes.len()], bytes);
    assert_eq!(encoded[1][0], 28);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[1][4..8]),
        window
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[1][8..12]),
        clipboard
    );
    assert_eq!(encoded[1][16], 1);
    assert!(
        properties
            .get(namespace, XResourceId::new(u64::from(window), 1), clipboard)
            .is_none()
    );
}

#[test]
fn x11_dispatch_get_selection_owner_reports_no_owner() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let request = decode_x11_core_request(
        context(namespace, 544, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 23, 7),
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 23),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);

    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 0);
}

#[test]
fn x11_dispatch_get_selection_owner_reflects_same_namespace_updates() {
    let namespace = NamespaceId::from_raw(45);
    let other_namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let owner = 0x220010;
    let selection = 7;

    let create = decode_x11_core_request(
        context(namespace, 545, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, owner, 0, 0, 300, 200),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let set_owner = decode_x11_core_request(
        context(namespace, 546, XByteOrder::LittleEndian),
        &set_selection_owner_request(
            XByteOrder::LittleEndian,
            owner,
            selection,
            10,
        ),
    )
    .unwrap();
    let set_owner = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 22),
        set_owner,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(set_owner.outputs.is_empty());

    let mut get_owner = |namespace, transaction, sequence, runtime: &mut XAuthorityRuntime| {
        let request = decode_x11_core_request(
            context(namespace, transaction, XByteOrder::LittleEndian),
            &resource_request(XByteOrder::LittleEndian, 23, selection),
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 23),
            request,
            runtime,
            &mut atoms,
            &mut properties,
        )
        .encoded_outputs(XByteOrder::LittleEndian)
    };

    let visible = get_owner(namespace, 547, 3, &mut runtime);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &visible[0][8..12]),
        owner
    );
    let confined = get_owner(other_namespace, 548, 4, &mut runtime);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &confined[0][8..12]),
        0
    );
}

#[test]
fn x11_dispatch_accepts_root_button_grab_lifecycle() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let grab = decode_x11_core_request(
        context(namespace, 545, XByteOrder::LittleEndian),
        &grab_button_request(
            XByteOrder::LittleEndian,
            X_SETUP_DEFAULT_ROOT,
            0x001c,
            1,
            0x0040,
        ),
    )
    .unwrap();
    let grab = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 28),
        grab,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(grab.outputs.is_empty());

    let ungrab = decode_x11_core_request(
        context(namespace, 546, XByteOrder::LittleEndian),
        &ungrab_button_request(XByteOrder::LittleEndian, X_SETUP_DEFAULT_ROOT, 1, 0x0040),
    )
    .unwrap();
    let ungrab = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 29),
        ungrab,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(ungrab.outputs.is_empty());
}

#[test]
fn x11_dispatch_allows_empty_root_property_reads() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let read = decode_x11_core_request(
        context(namespace, 525, XByteOrder::LittleEndian),
        &get_property_request(
            XByteOrder::LittleEndian,
            false,
            X_SETUP_DEFAULT_ROOT,
            X_ATOM_RESOURCE_MANAGER,
            X_PROPERTY_ANY_TYPE,
            0,
            64,
        ),
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 20),
        read,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(encoded[0][1], 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][8..12]), 0);
}

#[test]
fn x11_dispatch_get_property_fails_closed_for_bad_window_atom_and_offset() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let utf8 = atoms
        .intern(X_ATOM_NAME_UTF8_STRING, false)
        .unwrap()
        .unwrap();

    let bad_window = decode_x11_core_request(
        context(namespace, 516, XByteOrder::LittleEndian),
        &get_property_request(
            XByteOrder::LittleEndian,
            false,
            0x220009,
            X_ATOM_WM_NAME,
            X_PROPERTY_ANY_TYPE,
            0,
            1,
        ),
    )
    .unwrap();
    let bad_window = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 20),
        bad_window,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(
        bad_window.encoded_outputs(XByteOrder::LittleEndian)[0][1],
        3
    );

    let create = decode_x11_core_request(
        context(namespace, 517, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, 0x220009, 0, 0, 300, 200),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let bad_atom = decode_x11_core_request(
        context(namespace, 518, XByteOrder::LittleEndian),
        &get_property_request(
            XByteOrder::LittleEndian,
            false,
            0x220009,
            0x00ff_ffff,
            X_PROPERTY_ANY_TYPE,
            0,
            1,
        ),
    )
    .unwrap();
    let bad_atom = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 20),
        bad_atom,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(
        bad_atom.encoded_outputs(XByteOrder::LittleEndian)[0][1],
        XErrorCode::BadAtom.wire_code()
    );

    let change = decode_x11_core_request(
        context(namespace, 519, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            0x220009,
            X_ATOM_WM_NAME,
            utf8,
            8,
            b"short",
        ),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 18),
        change,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let bad_offset = decode_x11_core_request(
        context(namespace, 520, XByteOrder::LittleEndian),
        &get_property_request(
            XByteOrder::LittleEndian,
            false,
            0x220009,
            X_ATOM_WM_NAME,
            X_PROPERTY_ANY_TYPE,
            2,
            1,
        ),
    )
    .unwrap();
    let bad_offset = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 20),
        bad_offset,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(
        bad_offset.encoded_outputs(XByteOrder::LittleEndian)[0][1],
        XErrorCode::BadValue.wire_code()
    );
}

#[test]
fn x11_property_records_emit_metadata_candidates_without_raw_payloads() {
    let namespace = NamespaceId::from_raw(45);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let utf8 = atoms
        .intern(X_ATOM_NAME_UTF8_STRING, false)
        .unwrap()
        .unwrap();
    let net_wm_name = atoms
        .intern(X_ATOM_NAME_NET_WM_NAME, false)
        .unwrap()
        .unwrap();
    let window = 0x220006;
    let create = decode_x11_core_request(
        context(namespace, 511, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 320, 200),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let decoded = decode_x11_core_request(
        context(namespace, 512, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            window,
            net_wm_name,
            utf8,
            8,
            b"Secret Terminal Title",
        ),
    )
    .unwrap();

    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 18),
        decoded,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.metadata_candidates.len(), 1);
    let candidate = &result.metadata_candidates[0];
    assert_eq!(candidate.namespace, namespace);
    assert_eq!(candidate.window, XResourceId::new(u64::from(window), 1));
    assert_eq!(candidate.property_name, X_ATOM_NAME_NET_WM_NAME);
    assert_eq!(
        candidate.property_type_name.as_deref(),
        Some(X_ATOM_NAME_UTF8_STRING)
    );
    assert_eq!(candidate.byte_len, b"Secret Terminal Title".len());
}

#[test]
fn x11_core_decoder_rejects_bad_lengths_and_unknown_opcodes() {
    assert_eq!(
        decode_x11_core_request(
            context(NamespaceId::from_raw(45), 506, XByteOrder::LittleEndian),
            &[1, 0, 1]
        ),
        Err(XWireParseError::Truncated {
            needed: 4,
            actual: 3,
        })
    );

    // 110 is ListHosts, which this server does not implement. 127 used to
    // stand in here, but it is NoOperation and is now recognised.
    let mut unknown = vec![110, 0];
    push_u16(&mut unknown, XByteOrder::LittleEndian, 1);
    assert_eq!(
        decode_x11_core_request(
            context(NamespaceId::from_raw(45), 507, XByteOrder::LittleEndian),
            &unknown
        ),
        Err(XWireParseError::UnknownOpcode(110))
    );

    // NoOperation carries whatever padding the client chose to align what
    // follows, so it is the one core request with no fixed length.
    for padding_units in [0, 1, 7] {
        let mut nop = vec![127, 0];
        push_u16(&mut nop, XByteOrder::LittleEndian, 1 + padding_units);
        nop.resize(4 + usize::from(padding_units) * 4, 0xcd);
        assert_eq!(
            decode_x11_core_request(
                context(NamespaceId::from_raw(45), 508, XByteOrder::LittleEndian),
                &nop
            ),
            Ok(XWireRequest::NoOperation),
            "NoOperation padded with {padding_units} units"
        );
    }

    let mut unsupported_shm_minor = vec![X_MIT_SHM_MAJOR_OPCODE, 99];
    push_u16(&mut unsupported_shm_minor, XByteOrder::LittleEndian, 1);
    assert_eq!(
        decode_x11_core_request(
            context(NamespaceId::from_raw(45), 507, XByteOrder::LittleEndian),
            &unsupported_shm_minor
        ),
        Err(XWireParseError::UnknownOpcode(X_MIT_SHM_MAJOR_OPCODE))
    );

    let mut oversized_map = vec![8, 0];
    push_u16(&mut oversized_map, XByteOrder::LittleEndian, 3);
    push_u32(&mut oversized_map, XByteOrder::LittleEndian, 0x220005);
    push_u32(&mut oversized_map, XByteOrder::LittleEndian, 0);
    assert_eq!(
        decode_x11_core_request(
            context(NamespaceId::from_raw(45), 508, XByteOrder::LittleEndian),
            &oversized_map
        ),
        Err(XWireParseError::InvalidLength {
            opcode: 8,
            expected_at_least: 8,
            actual: 12,
        })
    );
}

#[test]
fn x11_client_event_encoders_emit_32_byte_records() {
    let map = encode_x_client_output(
        XByteOrder::LittleEndian,
        XClientOutput::Event(XClientEvent::MapNotify {
            sequence: 9,
            event: XResourceId::new(0x220001, 1),
            window: XResourceId::new(0x220001, 1),
            override_redirect: false,
        }),
    );
    assert_eq!(map.len(), 32);
    assert_eq!(map[0], 19);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &map[2..4]), 9);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &map[4..8]), 0x220001);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &map[8..12]), 0x220001);

    let configure = encode_x_client_output(
        XByteOrder::BigEndian,
        XClientOutput::Event(XClientEvent::ConfigureNotify {
            sequence: 10,
            synthetic: false,
            event: XResourceId::new(0x220002, 1),
            window: XResourceId::new(0x220002, 1),
            above_sibling: None,
            x: 12,
            y: 13,
            width: 640,
            height: 480,
            border_width: 0,
            override_redirect: false,
        }),
    );
    assert_eq!(configure[0], 22);
    assert_eq!(read_u16(XByteOrder::BigEndian, &configure[2..4]), 10);
    assert_eq!(read_u32(XByteOrder::BigEndian, &configure[8..12]), 0x220002);
    assert_eq!(read_u16(XByteOrder::BigEndian, &configure[20..22]), 640);
    assert_eq!(read_u16(XByteOrder::BigEndian, &configure[22..24]), 480);

    let key = encode_x_client_output(
        XByteOrder::LittleEndian,
        XClientOutput::Event(XClientEvent::Key {
            sequence: 11,
            pressed: true,
            keycode: 38,
            time: 123,
            root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            event: XResourceId::new(0x220003, 1),
            state: 1,
        }),
    );
    assert_eq!(key.len(), 32);
    assert_eq!(key[0], 2);
    assert_eq!(key[1], 38);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &key[2..4]), 11);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &key[4..8]), 123);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &key[12..16]), 0x220003);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &key[28..30]), 1);
    assert_eq!(key[30], 1);

    let focus = encode_x_client_output(
        XByteOrder::BigEndian,
        XClientOutput::Event(XClientEvent::Focus {
            sequence: 12,
            focused: true,
            detail: 3,
            event: XResourceId::new(0x220003, 1),
            mode: 0,
        }),
    );
    assert_eq!(focus.len(), 32);
    assert_eq!(focus[0], 9);
    assert_eq!(focus[1], 3);
    assert_eq!(read_u16(XByteOrder::BigEndian, &focus[2..4]), 12);
    assert_eq!(read_u32(XByteOrder::BigEndian, &focus[4..8]), 0x220003);
    assert_eq!(focus[8], 0);

    let motion = encode_x_client_output(
        XByteOrder::LittleEndian,
        XClientOutput::Event(XClientEvent::PointerMotion {
            sequence: 12,
            time: 124,
            root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            event: XResourceId::new(0x220003, 1),
            root_x: 50,
            root_y: 60,
            event_x: 10,
            event_y: 20,
            state: 1 << 8,
        }),
    );
    assert_eq!(motion[0], 6);
    assert_eq!(motion[1], 0);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &motion[2..4]), 12);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &motion[24..26]), 10);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &motion[28..30]), 1 << 8);

    let button = encode_x_client_output(
        XByteOrder::LittleEndian,
        XClientOutput::Event(XClientEvent::PointerButton {
            sequence: 13,
            pressed: true,
            button: 1,
            time: 125,
            root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            event: XResourceId::new(0x220003, 1),
            root_x: 50,
            root_y: 60,
            event_x: 10,
            event_y: 20,
            state: 0,
        }),
    );
    assert_eq!(button[0], 4);
    assert_eq!(button[1], 1);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &button[2..4]), 13);
}

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
