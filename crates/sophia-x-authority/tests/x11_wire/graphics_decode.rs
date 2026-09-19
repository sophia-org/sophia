#[test]
fn x11_core_decoder_captures_poly_fill_rectangle_requests() {
    let namespace = NamespaceId::from_raw(45);
    let fill = decode_x11_core_request(
        context(namespace, 507, XByteOrder::LittleEndian),
        &poly_fill_rectangle_request(
            XByteOrder::LittleEndian,
            0x220010,
            0x220011,
            &[(5, 6, 40, 30), (10, 12, 8, 9)],
        ),
    )
    .unwrap();

    assert_eq!(
        fill,
        XWireRequest::PolyFillRectangle {
            drawable: XResourceId::new(0x220010, 1),
            gc: XResourceId::new(0x220011, 1),
            rectangles: vec![
                Rect {
                    x: 5,
                    y: 6,
                    width: 40,
                    height: 30,
                },
                Rect {
                    x: 10,
                    y: 12,
                    width: 8,
                    height: 9,
                },
            ],
        }
    );

    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let outline = decode_x11_core_request(
            context(namespace, 508, byte_order),
            &poly_rectangle_request(
                byte_order,
                0x220010,
                0x220011,
                &[(-5, 6, 40, 30), (10, -12, 8, 9)],
            ),
        )
        .unwrap();
        assert_eq!(
            outline,
            XWireRequest::PolyRectangle {
                drawable: XResourceId::new(0x220010, 1),
                gc: XResourceId::new(0x220011, 1),
                rectangles: vec![
                    Rect {
                        x: -5,
                        y: 6,
                        width: 40,
                        height: 30,
                    },
                    Rect {
                        x: 10,
                        y: -12,
                        width: 8,
                        height: 9,
                    },
                ],
            }
        );
    }

    let mut malformed = poly_rectangle_request(XByteOrder::LittleEndian, 0x220010, 0x220011, &[]);
    malformed[2..4].copy_from_slice(&4u16.to_le_bytes());
    malformed.extend_from_slice(&[0; 4]);
    assert_eq!(
        decode_x11_core_request(
            context(namespace, 509, XByteOrder::LittleEndian),
            &malformed,
        ),
        Err(XWireParseError::InvalidLength {
            opcode: 67,
            expected_at_least: 20,
            actual: 16,
        })
    );

    let segments = decode_x11_core_request(
        context(namespace, 508, XByteOrder::LittleEndian),
        &poly_segment_request(
            XByteOrder::LittleEndian,
            0x220010,
            0x220011,
            &[(5, 6, 15, 16), (20, 30, 10, 24)],
        ),
    )
    .unwrap();

    assert_eq!(
        segments,
        XWireRequest::PolySegment {
            drawable: XResourceId::new(0x220010, 1),
            gc: XResourceId::new(0x220011, 1),
            damage: vec![
                Rect {
                    x: 5,
                    y: 6,
                    width: 11,
                    height: 11,
                },
                Rect {
                    x: 10,
                    y: 24,
                    width: 11,
                    height: 7,
                },
            ],
        }
    );

    let line = decode_x11_core_request(
        context(namespace, 509, XByteOrder::LittleEndian),
        &poly_line_request(
            XByteOrder::LittleEndian,
            0x220010,
            0x220011,
            &[(3, 4), (13, 9), (8, 20)],
        ),
    )
    .unwrap();

    assert_eq!(
        line,
        XWireRequest::PolyLine {
            drawable: XResourceId::new(0x220010, 1),
            gc: XResourceId::new(0x220011, 1),
            points: vec![
                XPoint { x: 3, y: 4 },
                XPoint { x: 13, y: 9 },
                XPoint { x: 8, y: 20 },
            ],
        }
    );

    let fill_poly = decode_x11_core_request(
        context(namespace, 510, XByteOrder::LittleEndian),
        &fill_poly_request(
            XByteOrder::LittleEndian,
            0x220010,
            0x220011,
            &[(5, 6), (15, 16), (8, 20)],
        ),
    )
    .unwrap();

    assert_eq!(
        fill_poly,
        XWireRequest::FillPoly {
            drawable: XResourceId::new(0x220010, 1),
            gc: XResourceId::new(0x220011, 1),
            damage: Some(Rect {
                x: 5,
                y: 6,
                width: 11,
                height: 15,
            }),
        }
    );

    let fill_arcs = decode_x11_core_request(
        context(namespace, 511, XByteOrder::LittleEndian),
        &poly_fill_arc_request(
            XByteOrder::LittleEndian,
            0x220010,
            0x220011,
            &[(7, 8, 41, 31, 0, 23040)],
        ),
    )
    .unwrap();

    assert_eq!(
        fill_arcs,
        XWireRequest::PolyFillArc {
            drawable: XResourceId::new(0x220010, 1),
            gc: XResourceId::new(0x220011, 1),
            damage: vec![Rect {
                x: 7,
                y: 8,
                width: 41,
                height: 31,
            }],
        }
    );

    let text = decode_x11_core_request(
        context(namespace, 512, XByteOrder::LittleEndian),
        &poly_text8_request(XByteOrder::LittleEndian, 0x220010, 0x220011, 5, 16, b"Hi"),
    )
    .unwrap();

    assert_eq!(
        text,
        XWireRequest::PolyText8 {
            drawable: XResourceId::new(0x220010, 1),
            gc: XResourceId::new(0x220011, 1),
            x: 5,
            y: 16,
            items: vec![XPolyTextItem::Text {
                delta: 0,
                chars: vec![72, 105],
            }],
        }
    );

    let padded_text = decode_x11_core_request(
        context(namespace, 513, XByteOrder::LittleEndian),
        &poly_text8_request(XByteOrder::LittleEndian, 0x220010, 0x220011, 5, 16, b"="),
    )
    .unwrap();

    assert_eq!(
        padded_text,
        XWireRequest::PolyText8 {
            drawable: XResourceId::new(0x220010, 1),
            gc: XResourceId::new(0x220011, 1),
            x: 5,
            y: 16,
            items: vec![XPolyTextItem::Text {
                delta: 0,
                chars: vec![61],
            }],
        }
    );

    let compact_text = decode_x11_core_request(
        context(namespace, 514, XByteOrder::LittleEndian),
        &poly_text8_compact_item_request(
            XByteOrder::LittleEndian,
            0x220010,
            0x220011,
            5,
            16,
            b"Hi",
        ),
    )
    .unwrap_err();
    assert!(matches!(
        compact_text,
        XWireParseError::InvalidLength { .. }
    ));

    let image_text = decode_x11_core_request(
        context(namespace, 515, XByteOrder::LittleEndian),
        &image_text8_request(XByteOrder::LittleEndian, 0x220010, 0x220011, 5, 16, b"Hi"),
    )
    .unwrap();

    assert_eq!(
        image_text,
        XWireRequest::ImageText8 {
            drawable: XResourceId::new(0x220010, 1),
            gc: XResourceId::new(0x220011, 1),
            x: 5,
            y: 16,
            text: b"Hi".to_vec(),
        }
    );
}

#[test]
fn x11_poly_text8_font_shift_is_msb_first_for_both_client_orders() {
    let namespace = NamespaceId::from_raw(45);
    let items = [255, 0x01, 0x23, 0x45, 0x67, 1, 0xff, b'x'];
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let request = decode_x11_core_request(
            context(namespace, 515, byte_order),
            &poly_text8_items_request(byte_order, 0x220010, 0x220011, 5, 16, &items),
        )
        .unwrap();
        assert_eq!(
            request,
            XWireRequest::PolyText8 {
                drawable: XResourceId::new(0x220010, 1),
                gc: XResourceId::new(0x220011, 1),
                x: 5,
                y: 16,
                items: vec![
                    XPolyTextItem::Font {
                        font: XResourceId::new(0x0123_4567, 1),
                    },
                    XPolyTextItem::Text {
                        delta: -1,
                        chars: vec![120],
                    },
                ],
            }
        );
    }
}

#[test]
fn x11_core_decoder_captures_put_image_requests() {
    let namespace = NamespaceId::from_raw(45);
    let put = decode_x11_core_request(
        context(namespace, 508, XByteOrder::LittleEndian),
        &put_image_request(
            XByteOrder::LittleEndian,
            0x220020,
            0x220021,
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

    assert_eq!(
        put,
        XWireRequest::PutImage {
            format: 2,
            drawable: XResourceId::new(0x220020, 1),
            gc: XResourceId::new(0x220021, 1),
            width: 8,
            height: 4,
            dst_x: 3,
            dst_y: 5,
            left_pad: 0,
            depth: 24,
            data: vec![0xaa; 128],
        }
    );
}

#[test]
fn x11_core_decoder_rejects_out_of_range_client_resource_creators() {
    let namespace = NamespaceId::from_raw(45);
    let context = XWireClientContext {
        byte_order: XByteOrder::LittleEndian,
        namespace,
        transaction: TransactionId::from_raw(509),
        resource_id_range: Some(XWireClientResourceRange {
            base: X_SETUP_DEFAULT_RESOURCE_ID_BASE,
            mask: X_SETUP_DEFAULT_RESOURCE_ID_MASK,
        }),
    };
    let outside_range = 0x0040_0001;
    let requests = [
        create_window_request(XByteOrder::LittleEndian, outside_range, 1, 2, 300, 200),
        create_gc_request(
            XByteOrder::LittleEndian,
            outside_range,
            X_SETUP_DEFAULT_ROOT,
        ),
        create_pixmap_request(
            XByteOrder::LittleEndian,
            24,
            outside_range,
            X_SETUP_DEFAULT_ROOT,
            32,
            16,
        ),
        open_font_request(XByteOrder::LittleEndian, outside_range, "fixed"),
        create_colormap_request(
            XByteOrder::LittleEndian,
            outside_range,
            X_SETUP_DEFAULT_ROOT,
            X_SETUP_DEFAULT_VISUAL,
        ),
        create_glyph_cursor_request(
            XByteOrder::LittleEndian,
            outside_range,
            0x0020_0040,
            0x0020_0041,
        ),
        mit_shm_attach_request(XByteOrder::LittleEndian, outside_range, 77, false),
    ];

    for request in requests {
        assert_eq!(
            decode_x11_core_request(context, &request),
            Err(XWireParseError::ResourceIdOutsideClientRange {
                resource_id: outside_range,
            })
        );
    }
}

#[test]
fn x11_classic_shared_x_allows_peer_operations_on_existing_resources() {
    let namespace = NamespaceId::from_raw(45);
    let creator = XWireClientContext {
        byte_order: XByteOrder::LittleEndian,
        namespace,
        transaction: TransactionId::from_raw(510),
        resource_id_range: Some(XWireClientResourceRange {
            base: X_SETUP_DEFAULT_RESOURCE_ID_BASE,
            mask: X_SETUP_DEFAULT_RESOURCE_ID_MASK,
        }),
    };
    let peer = XWireClientContext {
        byte_order: XByteOrder::LittleEndian,
        namespace,
        transaction: TransactionId::from_raw(511),
        resource_id_range: Some(XWireClientResourceRange {
            base: 0x0040_0000,
            mask: X_SETUP_DEFAULT_RESOURCE_ID_MASK,
        }),
    };
    let window = 0x0020_0001;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let create = decode_x11_core_request(
        creator,
        &create_window_request(XByteOrder::LittleEndian, window, 10, 20, 640, 480),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    // The peer cannot create in the creator's range, but classic shared-X
    // deliberately permits it to operate on an existing same-namespace XID.
    let map = decode_x11_core_request(peer, &resource_request(XByteOrder::LittleEndian, 8, window))
        .unwrap();
    let mapped = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 8),
        map,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(mapped.outputs.iter().any(|output| {
        matches!(
            output,
            XClientOutput::Event(XClientEvent::MapNotify { window: notified, .. })
                if *notified == XResourceId::new(u64::from(window), 1)
        )
    }));
}

#[test]
fn x11_core_decoder_captures_pixmap_and_copy_area_requests() {
    let namespace = NamespaceId::from_raw(45);
    let create = decode_x11_core_request(
        context(namespace, 571, XByteOrder::LittleEndian),
        &create_pixmap_request(XByteOrder::LittleEndian, 24, 0x220030, 0x220031, 32, 16),
    )
    .unwrap();
    assert_eq!(
        create,
        XWireRequest::CreatePixmap {
            depth: 24,
            pixmap: XResourceId::new(0x220030, 1),
            drawable: XResourceId::new(0x220031, 1),
            width: 32,
            height: 16,
        }
    );

    let copy = decode_x11_core_request(
        context(namespace, 572, XByteOrder::LittleEndian),
        &copy_area_request(
            XByteOrder::LittleEndian,
            0x220030,
            0x220031,
            0x220032,
            1,
            2,
            3,
            4,
            20,
            10,
        ),
    )
    .unwrap();
    assert_eq!(
        copy,
        XWireRequest::CopyArea {
            source: XResourceId::new(0x220030, 1),
            destination: XResourceId::new(0x220031, 1),
            gc: XResourceId::new(0x220032, 1),
            src_x: 1,
            src_y: 2,
            dst_x: 3,
            dst_y: 4,
            width: 20,
            height: 10,
        }
    );
}

#[test]
fn x11_core_decoder_captures_font_requests() {
    let namespace = NamespaceId::from_raw(45);
    let open = decode_x11_core_request(
        context(namespace, 573, XByteOrder::LittleEndian),
        &open_font_request(XByteOrder::LittleEndian, 0x220040, "fixed"),
    )
    .unwrap();
    assert_eq!(
        open,
        XWireRequest::OpenFont {
            font: XResourceId::new(0x220040, 1),
            name: "fixed".to_owned(),
        }
    );

    let close = decode_x11_core_request(
        context(namespace, 574, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 47, 0x220040),
    )
    .unwrap();
    assert_eq!(
        close,
        XWireRequest::QueryFont {
            font: XResourceId::new(0x220040, 1),
        }
    );

    let close = decode_x11_core_request(
        context(namespace, 575, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 46, 0x220040),
    )
    .unwrap();
    assert_eq!(
        close,
        XWireRequest::CloseFont {
            font: XResourceId::new(0x220040, 1),
        }
    );

    let list = decode_x11_core_request(
        context(namespace, 576, XByteOrder::LittleEndian),
        &list_fonts_request(XByteOrder::LittleEndian, 5, "*"),
    )
    .unwrap();
    assert_eq!(
        list,
        XWireRequest::ListFonts {
            max_names: 5,
            pattern: "*".to_owned(),
        }
    );

    let list = decode_x11_core_request(
        context(namespace, 577, XByteOrder::LittleEndian),
        &list_fonts_with_info_request(XByteOrder::LittleEndian, 5, "*"),
    )
    .unwrap();
    assert_eq!(
        list,
        XWireRequest::ListFontsWithInfo {
            max_names: 5,
            pattern: "*".to_owned(),
        }
    );

    let cursor = decode_x11_core_request(
        context(namespace, 578, XByteOrder::LittleEndian),
        &create_glyph_cursor_request(XByteOrder::LittleEndian, 0x220050, 0x220040, 0x220041),
    )
    .unwrap();
    assert_eq!(
        cursor,
        XWireRequest::CreateGlyphCursor {
            cursor: XResourceId::new(0x220050, 1),
            source_font: XResourceId::new(0x220040, 1),
            mask_font: Some(XResourceId::new(0x220041, 1)),
        }
    );

    let free_cursor = decode_x11_core_request(
        context(namespace, 579, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 95, 0x220050),
    )
    .unwrap();
    assert_eq!(
        free_cursor,
        XWireRequest::FreeCursor {
            cursor: XResourceId::new(0x220050, 1),
        }
    );

    let recolor_cursor = decode_x11_core_request(
        context(namespace, 580, XByteOrder::LittleEndian),
        &recolor_cursor_request(XByteOrder::LittleEndian, 0x220050),
    )
    .unwrap();
    assert_eq!(
        recolor_cursor,
        XWireRequest::RecolorCursor {
            cursor: XResourceId::new(0x220050, 1),
        }
    );
}

#[test]
fn x11_core_decoder_captures_query_extension_requests() {
    let namespace = NamespaceId::from_raw(45);
    let query = decode_x11_core_request(
        context(namespace, 507, XByteOrder::LittleEndian),
        &query_extension_request(XByteOrder::LittleEndian, "BIG-REQUESTS"),
    )
    .unwrap();

    assert_eq!(
        query,
        XWireRequest::QueryExtension {
            name: "BIG-REQUESTS".to_owned(),
        }
    );
}

#[test]
fn x11_core_decoder_captures_sophia_present_pixmap_requests() {
    let namespace = NamespaceId::from_raw(45);
    let present = decode_x11_core_request(
        context(namespace, 509, XByteOrder::LittleEndian),
        &sophia_present_pixmap_request(
            XByteOrder::LittleEndian,
            0x220030,
            0x900,
            (4, 5, 64, 48),
            3,
            250,
        ),
    )
    .unwrap();

    assert_eq!(
        present,
        XWireRequest::Authority(XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(509),
            namespace,
            kind: XAuthorityRequestKind::PresentPixmap {
                window: XResourceId::new(0x220030, 1),
                pixmap: 0x900,
                damage: Region::single(Rect {
                    x: 4,
                    y: 5,
                    width: 64,
                    height: 48,
                }),
                previous_committed_generation: 3,
                timeout_msec: 250,
            },
        })
    );
}

#[test]
fn x11_core_decoder_captures_mit_shm_requests() {
    let namespace = NamespaceId::from_raw(45);

    let query = decode_x11_core_request(
        context(namespace, 530, XByteOrder::LittleEndian),
        &mit_shm_query_version_request(XByteOrder::LittleEndian),
    )
    .unwrap();
    assert_eq!(query, XWireRequest::ShmQueryVersion);

    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let create = decode_x11_core_request(
            context(namespace, 530, byte_order),
            &mit_shm_create_pixmap_request(byte_order, 0x440010, 0x220701, 0x440001, 256),
        )
        .unwrap();
        assert_eq!(
            create,
            XWireRequest::ShmCreatePixmap {
                pixmap: XResourceId::new(0x440010, 1),
                drawable: XResourceId::new(0x220701, 1),
                width: 64,
                height: 48,
                depth: 24,
                segment: XResourceId::new(0x440001, 1),
                offset: 256,
            }
        );
    }

    let attach = decode_x11_core_request(
        context(namespace, 531, XByteOrder::LittleEndian),
        &mit_shm_attach_request(XByteOrder::LittleEndian, 0x440001, 77, true),
    )
    .unwrap();
    assert_eq!(
        attach,
        XWireRequest::ShmAttach {
            segment: XResourceId::new(0x440001, 1),
            shmid: 77,
            read_only: true,
        }
    );

    let get = decode_x11_core_request(
        context(namespace, 531, XByteOrder::LittleEndian),
        &mit_shm_get_image_request(XByteOrder::LittleEndian, 0x220701, 0x440001, 128),
    )
    .unwrap();
    assert_eq!(
        get,
        XWireRequest::ShmGetImage {
            drawable: XResourceId::new(0x220701, 1),
            x: 3,
            y: 5,
            width: 32,
            height: 24,
            plane_mask: u32::MAX,
            format: 2,
            segment: XResourceId::new(0x440001, 1),
            offset: 128,
        }
    );

    let put = decode_x11_core_request(
        context(namespace, 532, XByteOrder::LittleEndian),
        &mit_shm_put_image_request(XByteOrder::LittleEndian, 0x220701, 0x220702, 0x440001, 128),
    )
    .unwrap();
    assert_eq!(
        put,
        XWireRequest::ShmPutImage {
            drawable: XResourceId::new(0x220701, 1),
            gc: XResourceId::new(0x220702, 1),
            total_width: 64,
            total_height: 48,
            src_x: 0,
            src_y: 0,
            src_width: 32,
            src_height: 24,
            dst_x: 3,
            dst_y: 5,
            depth: 24,
            format: 2,
            send_event: false,
            segment: XResourceId::new(0x440001, 1),
            offset: 128,
        }
    );
}

#[test]
fn x11_core_decoder_captures_firefox_compatibility_requests_in_both_orders() {
    let namespace = NamespaceId::from_raw(45);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut get_image = vec![73, 2];
        push_u16(&mut get_image, byte_order, 5);
        push_u32(&mut get_image, byte_order, 0x220009);
        push_i16(&mut get_image, byte_order, 3);
        push_i16(&mut get_image, byte_order, 4);
        push_u16(&mut get_image, byte_order, 1290);
        push_u16(&mut get_image, byte_order, 1050);
        push_u32(&mut get_image, byte_order, u32::MAX);
        assert_eq!(
            decode_x11_core_request(context(namespace, 540, byte_order), &get_image).unwrap(),
            XWireRequest::GetImage {
                format: 2,
                drawable: XResourceId::new(0x220009, 1),
                x: 3,
                y: 4,
                width: 1290,
                height: 1050,
                plane_mask: u32::MAX,
            }
        );

        let mut reparent = vec![7, 0];
        push_u16(&mut reparent, byte_order, 4);
        push_u32(&mut reparent, byte_order, 0x22000c);
        push_u32(&mut reparent, byte_order, 0x22000d);
        push_i16(&mut reparent, byte_order, 5);
        push_i16(&mut reparent, byte_order, 6);
        assert_eq!(
            decode_x11_core_request(context(namespace, 541, byte_order), &reparent).unwrap(),
            XWireRequest::ReparentWindow {
                window: XResourceId::new(0x22000c, 1),
                parent: XResourceId::new(0x22000d, 1),
                x: 5,
                y: 6,
            }
        );

        let mut controls = vec![X_KEYBOARD_MAJOR_OPCODE, 6];
        push_u16(&mut controls, byte_order, 2);
        push_u16(&mut controls, byte_order, 3);
        push_u16(&mut controls, byte_order, 0);
        assert_eq!(
            decode_x11_core_request(context(namespace, 542, byte_order), &controls).unwrap(),
            XWireRequest::XkbGetControls
        );
    }
}
