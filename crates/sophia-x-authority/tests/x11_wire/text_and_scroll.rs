fn text_contract_dispatch(
    namespace: NamespaceId,
    sequence: u16,
    opcode: u8,
    bytes: &[u8],
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
) -> XDispatchResult {
    let request = decode_x11_core_request(
        context(
            namespace,
            9_000 + u64::from(sequence),
            XByteOrder::LittleEndian,
        ),
        bytes,
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
        request,
        runtime,
        atoms,
        properties,
    )
}

fn xrgb_pixels(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|pixel| u32::from_le_bytes(pixel.try_into().unwrap()))
        .collect()
}

#[test]
fn fixed_text_late_density_replay_produces_distinct_coverage_raster() {
    let namespace = NamespaceId::from_raw(146);
    let window = 0x220a01;
    let gc = 0x220a02;
    let surface = SurfaceId::new(window, 1);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    text_contract_dispatch(
        namespace,
        1,
        1,
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 80, 40),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    text_contract_dispatch(
        namespace,
        2,
        55,
        &create_gc_values_request(
            XByteOrder::LittleEndian,
            gc,
            window,
            0,
            u32::MAX,
            0x00ff_ffff,
            0,
            0,
            0,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let draw = text_contract_dispatch(
        namespace,
        3,
        76,
        &image_text8_request(XByteOrder::LittleEndian, window, gc, 4, 16, b"AaZz"),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let transaction = draw.response.unwrap().transactions.remove(0);
    assert_eq!(transaction.content.variants().len(), 1);
    assert_eq!(runtime.take_cpu_buffer_updates().len(), 1);

    let response = expect_satisfied_raster(
        runtime
            .apply_surface_raster_requirements(
                TransactionId::from_raw(9_100),
            &SurfaceRasterRequirements {
                surface,
                committed_content_generation: 2,
                requirement_generation: 1,
                logical_extent: Size {
                    width: 80,
                    height: 40,
                },
                classes: vec![
                    SurfaceRasterClass {
                        density_millis: 750,
                        transform: SurfaceRasterTransform::Normal,
                    },
                    SurfaceRasterClass {
                        density_millis: 1_000,
                        transform: SurfaceRasterTransform::Normal,
                    },
                ],
            },
        )
            .unwrap(),
        "fixed text journal should replay at 0.75 density",
    );
    assert_eq!(response.transaction.content.variants().len(), 2);
    let [XAuthorityCpuBufferUpdate::Replace(derived)] = response.cpu_buffer_updates.as_slice()
    else {
        panic!("late density must publish one immutable derived replacement");
    };
    assert_eq!(derived.size, Size { width: 60, height: 30 });
    let pixels = xrgb_pixels(&derived.bytes);
    assert!(pixels.iter().any(|pixel| *pixel != 0));
    assert!(pixels.iter().any(|pixel| {
        let intensity = pixel & 0xff;
        intensity != 0 && intensity != 0xff
    }));
}

#[test]
fn x11_fixed_6x13_gc_survives_font_close_and_image_text_forces_copy_solid() {
    let namespace = NamespaceId::from_raw(46);
    let window = 0x220901;
    let font = 0x220902;
    let gc = 0x220903;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    text_contract_dispatch(
        namespace,
        1,
        1,
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 12, 20),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let open = text_contract_dispatch(
        namespace,
        2,
        45,
        &open_font_request(XByteOrder::LittleEndian, font, "6x13"),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(open.outputs.is_empty());
    let bad_name = text_contract_dispatch(
        namespace,
        3,
        45,
        &open_font_request(XByteOrder::LittleEndian, 0x220904, "not-a-real-font"),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        bad_name.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadName,
            resource_id: 0x220904,
            ..
        })]
    ));

    let create_gc = text_contract_dispatch(
        namespace,
        4,
        55,
        &create_gc_values_request(
            XByteOrder::LittleEndian,
            gc,
            window,
            6,
            u32::MAX,
            0x00ff_ffff,
            0,
            0,
            font,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(create_gc.outputs.is_empty());
    let close = text_contract_dispatch(
        namespace,
        5,
        46,
        &resource_request(XByteOrder::LittleEndian, 46, font),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(close.outputs.is_empty());

    // QueryFont accepts a FONTABLE GC and reports the retained face after the
    // source font XID has been closed.
    let query_gc = text_contract_dispatch(
        namespace,
        6,
        47,
        &resource_request(XByteOrder::LittleEndian, 47, gc),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    // A graphics context answers QueryFont with the face it retains, and that
    // face is the built-in 6x13: single byte, every character present.
    let [XClientOutput::Reply(XClientReply::QueryFont { metrics, .. })] =
        query_gc.outputs.as_slice()
    else {
        panic!("a fontable graphics context answers QueryFont");
    };
    assert_eq!(metrics.font_ascent, 11);
    assert_eq!(metrics.font_descent, 2);
    assert_eq!(metrics.min_byte1, 0);
    assert_eq!(metrics.max_byte1, 0);
    assert!(metrics.all_chars_exist);
    let query_closed_font = text_contract_dispatch(
        namespace,
        6,
        47,
        &resource_request(XByteOrder::LittleEndian, 47, font),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        query_closed_font.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadFont,
            resource_id: 0x220902,
            ..
        })]
    ));

    let red = 0x00cc_2200u32.to_le_bytes();
    let red_cell: Vec<u8> = (0..(6 * 13)).flat_map(|_| red).collect();
    let put = text_contract_dispatch(
        namespace,
        7,
        72,
        &put_image_request(
            XByteOrder::LittleEndian,
            window,
            gc,
            PutImageGeometry {
                width: 6,
                height: 13,
                dst_x: 2,
                dst_y: 0,
            },
            &red_cell,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(put.outputs.is_empty());

    for sequence in [8, 9] {
        let draw = text_contract_dispatch(
            namespace,
            sequence,
            76,
            &image_text8_request(XByteOrder::LittleEndian, window, gc, 2, 11, b"a"),
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(draw.outputs.is_empty());
        assert_eq!(draw.response.unwrap().transactions.len(), 1);
    }

    let pixels = xrgb_pixels(
        &runtime
            .drawable_image_region(
                namespace,
                XResourceId::new(window.into(), 1),
                Rect {
                    x: 2,
                    y: 0,
                    width: 6,
                    height: 13,
                },
            )
            .unwrap(),
    );
    let rows = x_fixed_glyph_rows(b'a');
    for (row, bits) in rows.iter().copied().enumerate() {
        for column in 0..6usize {
            let expected = if bits & (1 << (5 - column)) != 0 {
                0x00ff_ffff
            } else {
                0
            };
            assert_eq!(pixels[row * 6 + column], expected, "row={row} col={column}");
        }
    }
}

#[test]
fn x11_poly_text8_applies_signed_deltas_and_scoped_font_shifts() {
    let namespace = NamespaceId::from_raw(46);
    let window = 0x220911;
    let font = 0x220912;
    let shifted_font = 0x220913;
    let gc = 0x220914;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    text_contract_dispatch(
        namespace,
        1,
        1,
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 64, 20),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    for (sequence, xid, name) in [
        (2, font, "fixed"),
        (3, shifted_font, X_FIXED_6X13_CANONICAL_NAME),
    ] {
        let open = text_contract_dispatch(
            namespace,
            sequence,
            45,
            &open_font_request(XByteOrder::LittleEndian, xid, name),
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(open.outputs.is_empty());
    }
    text_contract_dispatch(
        namespace,
        4,
        55,
        &create_gc_values_request(
            XByteOrder::LittleEndian,
            gc,
            window,
            3,
            u32::MAX,
            0x00ff_ffff,
            0,
            0,
            font,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let shifted = shifted_font.to_be_bytes();
    let items = [
        1, 0xfe, b'A', 255, shifted[0], shifted[1], shifted[2], shifted[3], 1, 3, b'b',
    ];
    let draw = text_contract_dispatch(
        namespace,
        5,
        74,
        &poly_text8_items_request(XByteOrder::LittleEndian, window, gc, 10, 11, &items),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(draw.outputs.is_empty());
    assert_eq!(
        draw.response.unwrap().transactions[0].damage,
        Region {
            rects: vec![
                Rect {
                    x: 8,
                    y: 0,
                    width: 6,
                    height: 13,
                },
                Rect {
                    x: 17,
                    y: 0,
                    width: 6,
                    height: 13,
                },
            ],
        }
    );

    let missing = 0x2209ffu32.to_be_bytes();
    let items = [
        1, 0, b'c', 255, missing[0], missing[1], missing[2], missing[3],
    ];
    let draw = text_contract_dispatch(
        namespace,
        6,
        74,
        &poly_text8_items_request(XByteOrder::LittleEndian, window, gc, 0, 11, &items),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        draw.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadFont,
            resource_id: 0x2209ff,
            ..
        })]
    ));
    assert_eq!(draw.response.unwrap().transactions.len(), 1);
}

#[test]
fn x11_copy_area_scrolls_overlap_and_copies_text_from_pixmap_backing() {
    let namespace = NamespaceId::from_raw(46);
    let window = 0x220921;
    let pixmap = 0x220922;
    let gc = 0x220923;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    text_contract_dispatch(
        namespace,
        1,
        1,
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 12, 26),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    text_contract_dispatch(
        namespace,
        2,
        55,
        &create_gc_request(XByteOrder::LittleEndian, gc, window),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    text_contract_dispatch(
        namespace,
        3,
        53,
        &create_pixmap_request(XByteOrder::LittleEndian, 24, pixmap, window, 12, 13),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let pixmap_text = text_contract_dispatch(
        namespace,
        4,
        76,
        &image_text8_request(XByteOrder::LittleEndian, pixmap, gc, 0, 11, b"Ag"),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(pixmap_text.outputs.is_empty());
    assert!(pixmap_text.response.unwrap().transactions.is_empty());
    let pixmap_pixels = runtime
        .drawable_image_region(
            namespace,
            XResourceId::new(pixmap.into(), 1),
            Rect {
                x: 0,
                y: 0,
                width: 12,
                height: 13,
            },
        )
        .unwrap();

    let copy_pixmap = text_contract_dispatch(
        namespace,
        5,
        62,
        &copy_area_request(
            XByteOrder::LittleEndian,
            pixmap,
            window,
            gc,
            0,
            0,
            0,
            13,
            12,
            13,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        copy_pixmap.outputs.as_slice(),
        [XClientOutput::Event(XClientEvent::NoExpose {
            sequence: 5,
            drawable,
            minor_opcode: 0,
            major_opcode: 62,
        })] if *drawable == XResourceId::new(window.into(), 1)
    ));
    assert_eq!(copy_pixmap.response.unwrap().transactions.len(), 1);
    let bottom_before = runtime
        .drawable_image_region(
            namespace,
            XResourceId::new(window.into(), 1),
            Rect {
                x: 0,
                y: 13,
                width: 12,
                height: 13,
            },
        )
        .unwrap();
    assert_eq!(bottom_before, pixmap_pixels);

    let scroll = text_contract_dispatch(
        namespace,
        6,
        62,
        &copy_area_request(
            XByteOrder::LittleEndian,
            window,
            window,
            gc,
            0,
            13,
            0,
            0,
            12,
            13,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        scroll.outputs.as_slice(),
        [XClientOutput::Event(XClientEvent::NoExpose {
            sequence: 6,
            drawable,
            minor_opcode: 0,
            major_opcode: 62,
        })] if *drawable == XResourceId::new(window.into(), 1)
    ));
    assert_eq!(
        scroll.response.unwrap().transactions[0].damage,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 12,
            height: 13,
        })
    );
    let top_after = runtime
        .drawable_image_region(
            namespace,
            XResourceId::new(window.into(), 1),
            Rect {
                x: 0,
                y: 0,
                width: 12,
                height: 13,
            },
        )
        .unwrap();
    assert_eq!(top_after, bottom_before);

    let disable_exposures = text_contract_dispatch(
        namespace,
        70,
        56,
        &change_gc_request(XByteOrder::LittleEndian, gc, 1 << 16, &[0]),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(disable_exposures.outputs.is_empty());

    let clipped = text_contract_dispatch(
        namespace,
        7,
        62,
        &copy_area_request(
            XByteOrder::LittleEndian,
            window,
            window,
            gc,
            -2,
            0,
            1,
            13,
            6,
            13,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(clipped.outputs.is_empty());
    assert_eq!(
        clipped.response.unwrap().transactions[0].damage,
        Region::single(Rect {
            x: 3,
            y: 13,
            width: 4,
            height: 13,
        })
    );

    let bad_source = text_contract_dispatch(
        namespace,
        8,
        62,
        &copy_area_request(
            XByteOrder::LittleEndian,
            0x2209fe,
            window,
            gc,
            0,
            0,
            0,
            0,
            1,
            1,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        bad_source.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadDrawable,
            resource_id: 0x2209fe,
            ..
        })]
    ));
    let bad_gc = text_contract_dispatch(
        namespace,
        9,
        62,
        &copy_area_request(
            XByteOrder::LittleEndian,
            window,
            window,
            0x2209fd,
            0,
            0,
            0,
            0,
            1,
            1,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        bad_gc.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadGraphicsContext,
            resource_id: 0x2209fd,
            ..
        })]
    ));

    let deep_pixmap = 0x220924;
    let deep_gc = 0x220925;
    text_contract_dispatch(
        namespace,
        10,
        53,
        &create_pixmap_request(XByteOrder::LittleEndian, 32, deep_pixmap, window, 2, 2),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    text_contract_dispatch(
        namespace,
        11,
        55,
        &create_gc_request(XByteOrder::LittleEndian, deep_gc, deep_pixmap),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let bad_match = text_contract_dispatch(
        namespace,
        12,
        62,
        &copy_area_request(
            XByteOrder::LittleEndian,
            window,
            window,
            deep_gc,
            0,
            0,
            0,
            0,
            1,
            1,
        ),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        bad_match.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadMatch,
            resource_id: 0x220921,
            ..
        })]
    ));
}
