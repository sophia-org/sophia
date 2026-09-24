// PutImage through wire dispatch: the transaction it emits and the pixels it
// leaves, into windows and into pixmaps copied to windows.

#[test]
fn x11_dispatch_put_image_emits_software_surface_transaction() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let create = decode_x11_core_request(
        context(namespace, 611, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, 0x220111, 10, 20, 640, 480),
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
        context(namespace, 612, XByteOrder::LittleEndian),
        &create_gc_request(XByteOrder::LittleEndian, 0x220112, 0x220111),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 55),
        gc,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let put = decode_x11_core_request(
        context(namespace, 612, XByteOrder::LittleEndian),
        &put_image_request(
            XByteOrder::LittleEndian,
            0x220111,
            0x220112,
            PutImageGeometry {
                width: 8,
                height: 4,
                dst_x: 3,
                dst_y: 5,
            },
            &[0xaa; 128],
        ),
    )
    .unwrap();
    let put = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 72),
        put,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(put.outputs.is_empty());
    let response = put.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].surface,
        SurfaceId::new(0x220111, 1)
    );
    assert!(matches!(
        response.transactions[0].target_buffer(),
        BufferSource::CpuBuffer { .. }
    ));
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 3,
            y: 5,
            width: 8,
            height: 4,
        })
    );
}

#[test]
fn x11_dispatch_pixmap_put_image_and_copy_area_emit_window_transaction() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let create = decode_x11_core_request(
        context(namespace, 621, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, 0x220121, 10, 20, 640, 480),
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
        context(namespace, 622, XByteOrder::LittleEndian),
        &create_gc_request(XByteOrder::LittleEndian, 0x220123, 0x220121),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 55),
        gc,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let pixmap = decode_x11_core_request(
        context(namespace, 622, XByteOrder::LittleEndian),
        &create_pixmap_request(XByteOrder::LittleEndian, 24, 0x220122, 0x220121, 64, 32),
    )
    .unwrap();
    let pixmap = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 53),
        pixmap,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(pixmap.outputs.is_empty());

    let invalid_depth = decode_x11_core_request(
        context(namespace, 622, XByteOrder::LittleEndian),
        &create_pixmap_request(XByteOrder::LittleEndian, 2, 0x220124, 0x220121, 64, 32),
    )
    .unwrap();
    let invalid_depth = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 53),
        invalid_depth,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(invalid_depth[0][1], XErrorCode::BadValue.wire_code());
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &invalid_depth[0][4..8]),
        2
    );

    let put = decode_x11_core_request(
        context(namespace, 623, XByteOrder::LittleEndian),
        &put_image_request(
            XByteOrder::LittleEndian,
            0x220122,
            0x220123,
            PutImageGeometry {
                width: 8,
                height: 4,
                dst_x: 0,
                dst_y: 0,
            },
            &[0xaa; 128],
        ),
    )
    .unwrap();
    let put = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 72),
        put,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(put.outputs.is_empty());
    assert!(put.response.unwrap().transactions.is_empty());

    let copy = decode_x11_core_request(
        context(namespace, 624, XByteOrder::LittleEndian),
        &copy_area_request(
            XByteOrder::LittleEndian,
            0x220122,
            0x220121,
            0x220123,
            0,
            0,
            5,
            6,
            8,
            4,
        ),
    )
    .unwrap();
    let copy = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 62),
        copy,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        copy.outputs.as_slice(),
        [XClientOutput::Event(XClientEvent::NoExpose {
            sequence: 4,
            drawable,
            minor_opcode: 0,
            major_opcode: 62,
        })] if *drawable == XResourceId::new(0x220121, 1)
    ));
    let encoded_no_expose = copy.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded_no_expose.len(), 1);
    assert_eq!(encoded_no_expose[0][0], 14);
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &encoded_no_expose[0][2..4]),
        4
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded_no_expose[0][4..8]),
        0x220121
    );
    assert_eq!(encoded_no_expose[0][10], 62);
    let response = copy.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].surface,
        SurfaceId::new(0x220121, 1)
    );
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 5,
            y: 6,
            width: 8,
            height: 4,
        })
    );
}

#[test]
fn x11_put_image_preserves_a_non_gray_xrgb_palette_without_channel_swaps() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let window = 0x220131;
    let create = decode_x11_core_request(
        context(namespace, 631, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 6, 1),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let gc = 0x220132;
    let create_gc = decode_x11_core_request(
        context(namespace, 632, XByteOrder::LittleEndian),
        &create_gc_request(XByteOrder::LittleEndian, gc, window),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 55),
        create_gc,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    // ZPixmap bytes follow the setup image order: blue, green, red, padding.
    let palette = [
        0x00, 0x00, 0x00, 0x00, // black
        0x00, 0x00, 0xff, 0x00, // red
        0x00, 0xff, 0x00, 0x00, // green
        0xff, 0x00, 0x00, 0x00, // blue
        0x80, 0xab, 0x12, 0x00, // mixed
        0x7f, 0x7f, 0x7f, 0x00, // gray
    ];
    let put = decode_x11_core_request(
        context(namespace, 632, XByteOrder::LittleEndian),
        &put_image_request(
            XByteOrder::LittleEndian,
            window,
            gc,
            PutImageGeometry {
                width: 6,
                height: 1,
                dst_x: 0,
                dst_y: 0,
            },
            &palette,
        ),
    )
    .unwrap();
    let put = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 72),
        put,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert_eq!(
        put.response.unwrap().outcome,
        XAuthorityResponseOutcome::Accepted
    );
    let XAuthorityCpuBufferUpdate::Replace(snapshot) = runtime.take_cpu_buffer_update().unwrap()
    else {
        panic!("the first palette upload must publish a replacement buffer");
    };
    assert_eq!(snapshot.format, X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888);
    assert_eq!(snapshot.stride, 24);
    assert_eq!(snapshot.bytes.as_slice(), palette);
}
