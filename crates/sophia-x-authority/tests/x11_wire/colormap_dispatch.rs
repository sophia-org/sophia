// Colormaps and colour allocation through dispatch, and the depth, visual
// and colormap a CreateWindow must agree on. Included from x11_wire.rs
// beside properties_dispatch.rs (t026).

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
        XWireRequest::Core(sophia_x_authority::XCoreRequest::CreateColormap {
            alloc: 0,
            colormap: XResourceId::new(0x200001, 1),
            window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            visual: X_SETUP_DEFAULT_VISUAL,
        })
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
            XWireRequest::Core(sophia_x_authority::XCoreRequest::AllocNamedColor {
                colormap: XResourceId::new(u64::from(X_SETUP_DEFAULT_COLORMAP), 1),
                name: "Light Gray".to_owned(),
            })
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
            XWireRequest::Core(sophia_x_authority::XCoreRequest::AllocColor {
                colormap: XResourceId::new(u64::from(X_SETUP_DEFAULT_COLORMAP), 1),
                red: 0x1234,
                green: 0xabcd,
                blue: 0x80ff,
            })
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
