#[test]
fn x11_client_error_encoder_and_parse_mapping_use_core_error_shape() {
    let error = x_error_from_wire_parse(&XWireParseError::UnknownOpcode(99), 11, 99, 7);
    assert_eq!(error.code, XErrorCode::BadRequest);

    let encoded = encode_x_client_output(XByteOrder::LittleEndian, XClientOutput::Error(error));
    assert_eq!(encoded.len(), 32);
    assert_eq!(encoded[0], 0);
    assert_eq!(encoded[1], 1);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[2..4]), 11);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[8..10]), 7);
    assert_eq!(encoded[10], 99);

    let bad_length = x_error_from_wire_parse(
        &XWireParseError::InvalidLength {
            opcode: 8,
            expected_at_least: 8,
            actual: 12,
        },
        12,
        8,
        0,
    );
    assert_eq!(bad_length.code, XErrorCode::BadLength);
}

#[test]
fn x11_dispatch_emits_configure_map_property_and_selection_failure_outputs() {
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
    assert!(matches!(
        create.outputs.as_slice(),
        [XClientOutput::Event(XClientEvent::CreateNotify { .. })]
    ));

    let map = decode_x11_core_request(
        context(namespace, 602, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 8, 0x220101),
    )
    .unwrap();
    let map = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 8),
        map,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(map.outputs.len(), 3);
    assert_eq!(
        encode_x_client_output(XByteOrder::LittleEndian, map.outputs[0].clone())[0],
        19
    );
    assert_eq!(
        encode_x_client_output(XByteOrder::LittleEndian, map.outputs[1].clone())[0],
        15
    );
    assert_eq!(
        encode_x_client_output(XByteOrder::LittleEndian, map.outputs[2].clone())[0],
        12
    );

    let unmap = decode_x11_core_request(
        context(namespace, 603, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 10, 0x220101),
    )
    .unwrap();
    let unmap = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 10),
        unmap,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    // A successful unmap is a state change the window's watchers are owed.
    assert_eq!(unmap.outputs.len(), 1, "the unmap transition is reported");
    let encoded = encode_x_client_output(XByteOrder::LittleEndian, unmap.outputs[0].clone());
    assert_eq!(encoded[0], 18, "UnmapNotify");
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[4..8]), 0x220101);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[8..12]), 0x220101);
    assert_eq!(encoded[12], 0, "not from a configure");

    // Unmapping again is not a transition, so it owes nobody anything.
    let repeat = decode_x11_core_request(
        context(namespace, 603, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 10, 0x220101),
    )
    .unwrap();
    let repeat = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 10),
        repeat,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        repeat.outputs.is_empty(),
        "an already-unmapped window reports no second unmap"
    );

    let configure = decode_x11_core_request(
        context(namespace, 604, XByteOrder::LittleEndian),
        &configure_window_request(XByteOrder::LittleEndian, 0x220101, 0x000c, &[12, 14]),
    )
    .unwrap();
    let configure = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 12),
        configure,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(configure.outputs.len(), 1);
    assert_eq!(
        configure.outputs[0],
        XClientOutput::Event(XClientEvent::ConfigureNotify {
            sequence: 4,
            synthetic: false,
            event: XResourceId::new(0x220101, 1),
            window: XResourceId::new(0x220101, 1),
            above_sibling: None,
            x: 10,
            y: 20,
            width: 12,
            height: 14,
            border_width: 0,
            override_redirect: false,
        })
    );
    assert_eq!(
        runtime
            .window_geometry(namespace, XResourceId::new(0x220101, 1))
            .unwrap(),
        Rect {
            x: 10,
            y: 20,
            width: 12,
            height: 14,
        }
    );

    let map_subwindows = decode_x11_core_request(
        context(namespace, 605, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 9, X_SETUP_DEFAULT_ROOT),
    )
    .unwrap();
    let map_subwindows = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 9),
        map_subwindows,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(map_subwindows.outputs.len(), 3);
    assert_eq!(
        encode_x_client_output(XByteOrder::LittleEndian, map_subwindows.outputs[0].clone())[0],
        19
    );
    assert_eq!(
        encode_x_client_output(XByteOrder::LittleEndian, map_subwindows.outputs[1].clone())[0],
        15
    );
    assert_eq!(
        encode_x_client_output(XByteOrder::LittleEndian, map_subwindows.outputs[2].clone())[0],
        12
    );

    let attributes = decode_x11_core_request(
        context(namespace, 606, XByteOrder::LittleEndian),
        &change_window_attributes_request(XByteOrder::LittleEndian, X_SETUP_DEFAULT_ROOT),
    )
    .unwrap();
    let attributes = dispatch_x11_wire_request(
        dispatch_context(namespace, 6, XByteOrder::LittleEndian, 2),
        attributes,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(attributes.outputs.is_empty());

    let property = decode_x11_core_request(
        context(namespace, 607, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            0x220101,
            7,
            8,
            8,
            b"hello",
        ),
    )
    .unwrap();
    let property = dispatch_x11_wire_request(
        dispatch_context(namespace, 7, XByteOrder::LittleEndian, 18),
        property,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(property.outputs.len(), 1);
    assert_eq!(
        encode_x_client_output(XByteOrder::LittleEndian, property.outputs[0].clone())[0],
        28
    );

    let selection = decode_x11_core_request(
        context(namespace, 608, XByteOrder::LittleEndian),
        // Atoms the table knows: an atom that names nothing is BadAtom, not a failed conversion.
        &convert_selection_request(XByteOrder::LittleEndian, 0x220101, X_ATOM_PRIMARY, X_ATOM_STRING, X_ATOM_WM_NAME, 33),
    )
    .unwrap();
    let selection = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 24),
        selection,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(selection.outputs.len(), 1);
    let encoded = encode_x_client_output(XByteOrder::LittleEndian, selection.outputs[0].clone());
    assert_eq!(encoded[0], 31);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[20..24]),
        X_ATOM_NONE
    );
}

#[test]
fn x11_dispatch_poly_fill_rectangle_emits_core_draw_transaction() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let create = decode_x11_core_request(
        context(namespace, 601, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, 0x220101, 10, 20, 640, 480),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let gc = decode_x11_core_request(
        context(namespace, 602, XByteOrder::LittleEndian),
        &create_gc_request(XByteOrder::LittleEndian, 0x220102, 0x220101),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 55),
        gc,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let clear = decode_x11_core_request(
        context(namespace, 601, XByteOrder::LittleEndian),
        &clear_area_request(XByteOrder::LittleEndian, false, 0x220101, 4, 5, 33, 22),
    )
    .unwrap();
    let clear_transaction = TransactionId::from_raw(6_001);
    let clear = dispatch_x11_wire_request(
        dispatch_context_with_transaction(
            namespace,
            clear_transaction,
            1,
            XByteOrder::LittleEndian,
            61,
        ),
        clear,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(clear.outputs.is_empty());
    let response = clear.response.unwrap();
    assert_eq!(response.transaction, clear_transaction);
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(response.transactions[0].transaction, clear_transaction);
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 4,
            y: 5,
            width: 33,
            height: 22,
        })
    );

    let fill = decode_x11_core_request(
        context(namespace, 602, XByteOrder::LittleEndian),
        &poly_fill_rectangle_request(
            XByteOrder::LittleEndian,
            0x220101,
            0x220102,
            &[(5, 6, 40, 30)],
        ),
    )
    .unwrap();
    let fill = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 70),
        fill,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(fill.outputs.is_empty());
    let response = fill.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].surface,
        SurfaceId::new(0x220101, 1)
    );
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 5,
            y: 6,
            width: 40,
            height: 30,
        })
    );

    let segments = decode_x11_core_request(
        context(namespace, 603, XByteOrder::LittleEndian),
        &poly_segment_request(
            XByteOrder::LittleEndian,
            0x220101,
            0x220102,
            &[(2, 3, 12, 8)],
        ),
    )
    .unwrap();
    let segments = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 66),
        segments,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(segments.outputs.is_empty());
    let response = segments.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].surface,
        SurfaceId::new(0x220101, 1)
    );
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 2,
            y: 3,
            width: 11,
            height: 6,
        })
    );

    let line = decode_x11_core_request(
        context(namespace, 604, XByteOrder::LittleEndian),
        &poly_line_request(
            XByteOrder::LittleEndian,
            0x220101,
            0x220102,
            &[(1, 2), (11, 7), (5, 18)],
        ),
    )
    .unwrap();
    let line = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 65),
        line,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(line.outputs.is_empty());
    let response = line.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].surface,
        SurfaceId::new(0x220101, 1)
    );
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 1,
            y: 2,
            width: 11,
            height: 17,
        })
    );

    let fill_poly = decode_x11_core_request(
        context(namespace, 605, XByteOrder::LittleEndian),
        &fill_poly_request(
            XByteOrder::LittleEndian,
            0x220101,
            0x220102,
            &[(4, 5), (14, 10), (7, 20)],
        ),
    )
    .unwrap();
    let fill_poly = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 69),
        fill_poly,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(fill_poly.outputs.is_empty());
    let response = fill_poly.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].surface,
        SurfaceId::new(0x220101, 1)
    );
    // The polygon is filled now, so the damage covers the pixels it painted
    // rather than the hull of its vertices. The scanline fill samples row
    // centres, so a sliver narrower than a pixel at the apex paints nothing
    // and is not reported.
    let damage = response.transactions[0].damage.rects.clone();
    assert_eq!(damage.len(), 1, "one conservative rectangle, not one per span");
    let painted = damage[0];
    assert!(
        painted.x >= 4
            && painted.y >= 5
            && painted.x + painted.width <= 15
            && painted.y + painted.height <= 21,
        "the damage stays inside the vertex hull: {painted:?}"
    );
    assert!(
        painted.width > 5 && painted.height > 10,
        "and covers the body of the triangle: {painted:?}"
    );

    let fill_arcs = decode_x11_core_request(
        context(namespace, 606, XByteOrder::LittleEndian),
        &poly_fill_arc_request(
            XByteOrder::LittleEndian,
            0x220101,
            0x220102,
            &[(6, 7, 22, 12, 0, 23040)],
        ),
    )
    .unwrap();
    let fill_arcs = dispatch_x11_wire_request(
        dispatch_context(namespace, 6, XByteOrder::LittleEndian, 71),
        fill_arcs,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(fill_arcs.outputs.is_empty());
    let response = fill_arcs.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].surface,
        SurfaceId::new(0x220101, 1)
    );
    // A full revolution, so the fill covers the ellipse the arc bounds. The
    // damage is what was painted, which sits inside that box rather than
    // being it: the scanline fill rounds at the rim.
    let damage = response.transactions[0].damage.rects.clone();
    assert_eq!(damage.len(), 1);
    let painted = damage[0];
    assert!(
        painted.x >= 6
            && painted.y >= 7
            && painted.x + painted.width <= 28
            && painted.y + painted.height <= 19,
        "the damage stays inside the arc's bounding box: {painted:?}"
    );
    assert!(
        painted.width > 18 && painted.height > 9,
        "and covers the body of the ellipse: {painted:?}"
    );
}

#[test]
fn x11_dispatch_poly_rectangle_draws_outlines_and_validates_resources() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let window = 0x220141;
    let gc = 0x220142;
    let create = decode_x11_core_request(
        context(namespace, 621, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 32, 24),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let create_gc = decode_x11_core_request(
        context(namespace, 622, XByteOrder::LittleEndian),
        &create_gc_values_request(
            XByteOrder::LittleEndian,
            gc,
            window,
            6,
            u32::MAX,
            0x00ff_8040,
            0,
            0,
            0,
        ),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 55),
        create_gc,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let outline = decode_x11_core_request(
        context(namespace, 623, XByteOrder::LittleEndian),
        &poly_rectangle_request(
            XByteOrder::LittleEndian,
            window,
            gc,
            &[(5, 6, 10, 8), (20, 4, 0, 5), (2, 18, 4, 0)],
        ),
    )
    .unwrap();
    let outline = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 67),
        outline,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(outline.outputs.is_empty());
    let response = outline.response.unwrap();
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 2,
            y: 4,
            width: 19,
            height: 15,
        })
    );
    let XAuthorityCpuBufferUpdate::Replace(snapshot) = runtime.take_cpu_buffer_update().unwrap()
    else {
        panic!("the first rectangle draw must replace the CPU buffer");
    };
    let pixel = |x: usize, y: usize| {
        let offset = y * usize::try_from(snapshot.stride).unwrap() + x * 4;
        u32::from_le_bytes(snapshot.bytes[offset..offset + 4].try_into().unwrap())
    };
    // GXxor exposes duplicate corner writes; every outline pixel must be touched once.
    for (x, y) in [(5, 6), (15, 6), (15, 14), (5, 14), (10, 6), (5, 10)] {
        assert_eq!(pixel(x, y), 0x00ff_8040);
    }
    assert_eq!(pixel(10, 10), 0);
    for y in 4..=9 {
        assert_eq!(pixel(20, y), 0x00ff_8040);
    }
    for x in 2..=6 {
        assert_eq!(pixel(x, 18), 0x00ff_8040);
    }

    let wide_window = 0x220148;
    let wide_gc = 0x220145;
    let create_wide_window = decode_x11_core_request(
        context(namespace, 624, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, wide_window, 0, 0, 32, 24),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 1),
        create_wide_window,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let create_wide_gc = decode_x11_core_request(
        context(namespace, 624, XByteOrder::LittleEndian),
        &create_gc_values_request(
            XByteOrder::LittleEndian,
            wide_gc,
            wide_window,
            6,
            u32::MAX,
            0x0000_c0ff,
            0,
            3,
            0,
        ),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 55),
        create_wide_gc,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let wide = decode_x11_core_request(
        context(namespace, 625, XByteOrder::LittleEndian),
        &poly_rectangle_request(
            XByteOrder::LittleEndian,
            wide_window,
            wide_gc,
            &[(23, 13, 4, 4)],
        ),
    )
    .unwrap();
    let wide = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 67),
        wide,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(
        wide.response.unwrap().transactions[0].damage,
        Region::single(Rect {
            x: 22,
            y: 12,
            width: 7,
            height: 7,
        })
    );
    let XAuthorityCpuBufferUpdate::Replace(wide_snapshot) =
        runtime.take_cpu_buffer_update().unwrap()
    else {
        panic!("wide rectangle draw must preserve a CPU snapshot");
    };
    let wide_pixel = |x: usize, y: usize| {
        let offset = y * usize::try_from(wide_snapshot.stride).unwrap() + x * 4;
        u32::from_le_bytes(wide_snapshot.bytes[offset..offset + 4].try_into().unwrap())
    };
    for y in 12..=18 {
        for x in 22..=28 {
            let expected = if (x, y) == (25, 15) { 0 } else { 0x0000_c0ff };
            assert_eq!(wide_pixel(x, y), expected, "pixel ({x}, {y})");
        }
    }

    let empty = decode_x11_core_request(
        context(namespace, 624, XByteOrder::LittleEndian),
        &poly_rectangle_request(XByteOrder::LittleEndian, window, gc, &[]),
    )
    .unwrap();
    let empty = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 67),
        empty,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(
        empty.response.unwrap().outcome,
        XAuthorityResponseOutcome::Accepted
    );
    assert!(runtime.take_cpu_buffer_update().is_none());

    for (sequence, drawable, gc, code, resource_id) in [
        (
            5,
            window,
            0x2201ff,
            XErrorCode::BadGraphicsContext,
            0x2201ff,
        ),
        (6, 0x2201fe, gc, XErrorCode::BadDrawable, 0x2201fe),
    ] {
        let request = decode_x11_core_request(
            context(
                namespace,
                624 + u64::from(sequence),
                XByteOrder::LittleEndian,
            ),
            &poly_rectangle_request(XByteOrder::LittleEndian, drawable, gc, &[]),
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 67),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert_eq!(
            result.outputs,
            vec![XClientOutput::Error(XClientError {
                code,
                sequence,
                resource_id,
                minor_code: 0,
                major_code: 67,
            })]
        );
        assert_eq!(
            result.encoded_outputs(XByteOrder::LittleEndian)[0][1],
            code.wire_code()
        );
    }

    let other_namespace = NamespaceId::from_raw(47);
    let confined = decode_x11_core_request(
        context(other_namespace, 630, XByteOrder::LittleEndian),
        &poly_rectangle_request(XByteOrder::LittleEndian, window, gc, &[]),
    )
    .unwrap();
    let confined = dispatch_x11_wire_request(
        dispatch_context(other_namespace, 7, XByteOrder::LittleEndian, 67),
        confined,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        confined.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadAccess,
            resource_id,
            ..
        })] if *resource_id == window
    ));

    let pixmap = 0x220143;
    let depth_one_gc = 0x220144;
    for (sequence, major_opcode, request) in [
        (
            7,
            53,
            create_pixmap_request(XByteOrder::LittleEndian, 1, pixmap, window, 8, 8),
        ),
        (
            8,
            55,
            create_gc_request(XByteOrder::LittleEndian, depth_one_gc, pixmap),
        ),
    ] {
        let request = decode_x11_core_request(
            context(
                namespace,
                630 + u64::from(sequence),
                XByteOrder::LittleEndian,
            ),
            &request,
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, major_opcode),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
    }
    let mismatch = decode_x11_core_request(
        context(namespace, 639, XByteOrder::LittleEndian),
        &poly_rectangle_request(XByteOrder::LittleEndian, window, depth_one_gc, &[]),
    )
    .unwrap();
    let mismatch = dispatch_x11_wire_request(
        dispatch_context(namespace, 9, XByteOrder::LittleEndian, 67),
        mismatch,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        mismatch.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadMatch,
            resource_id,
            ..
        })] if *resource_id == window
    ));

    let source_pixmap = 0x220146;
    let retained_gc = 0x220147;
    for (sequence, major_opcode, request) in [
        (
            10,
            53,
            create_pixmap_request(XByteOrder::LittleEndian, 24, source_pixmap, window, 8, 8),
        ),
        (
            11,
            55,
            create_gc_request(XByteOrder::LittleEndian, retained_gc, source_pixmap),
        ),
        (
            12,
            54,
            resource_request(XByteOrder::LittleEndian, 54, source_pixmap),
        ),
    ] {
        let request = decode_x11_core_request(
            context(
                namespace,
                640 + u64::from(sequence),
                XByteOrder::LittleEndian,
            ),
            &request,
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, major_opcode),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(result.outputs.is_empty());
    }
    let retained = decode_x11_core_request(
        context(namespace, 653, XByteOrder::LittleEndian),
        &poly_rectangle_request(XByteOrder::LittleEndian, window, retained_gc, &[]),
    )
    .unwrap();
    let retained = dispatch_x11_wire_request(
        dispatch_context(namespace, 13, XByteOrder::LittleEndian, 67),
        retained,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(
        retained.response.unwrap().outcome,
        XAuthorityResponseOutcome::Accepted
    );
    assert!(retained.outputs.is_empty());
}
