
#[test]
fn x11_setup_parser_accepts_little_endian_auth_fields() {
    let bytes = setup_request(
        XByteOrder::LittleEndian,
        11,
        0,
        b"MIT-MAGIC-COOKIE-1",
        b"0123456789abcdef",
    );

    let request = parse_x11_setup_request(&bytes).unwrap();

    assert_eq!(request.byte_order, XByteOrder::LittleEndian);
    assert_eq!(request.major_version, 11);
    assert_eq!(request.minor_version, 0);
    assert_eq!(request.authorization_protocol_name, b"MIT-MAGIC-COOKIE-1");
    assert_eq!(request.authorization_data, b"0123456789abcdef");
    assert_eq!(
        x11_setup_request_total_len(&bytes[..12]).unwrap(),
        bytes.len()
    );
}

#[test]
fn x11_setup_parser_accepts_big_endian_empty_auth() {
    let bytes = setup_request(XByteOrder::BigEndian, 11, 0, b"", b"");

    let request = parse_x11_setup_request(&bytes).unwrap();

    assert_eq!(request.byte_order, XByteOrder::BigEndian);
    assert_eq!(request.major_version, 11);
    assert!(request.authorization_protocol_name.is_empty());
    assert!(request.authorization_data.is_empty());
}

#[test]
fn x11_setup_parser_rejects_malformed_inputs() {
    assert_eq!(
        parse_x11_setup_request(&[b'l'; 4]),
        Err(XSetupParseError::Truncated {
            needed: 12,
            actual: 4
        })
    );
    assert_eq!(
        parse_x11_setup_request(&setup_request(XByteOrder::LittleEndian, 12, 0, b"", b"")),
        Err(XSetupParseError::UnsupportedMajorVersion(12))
    );
    assert_eq!(
        parse_x11_setup_request(&[b'x', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(XSetupParseError::InvalidByteOrder(b'x'))
    );

    let mut overlarge = setup_request(XByteOrder::LittleEndian, 11, 0, b"", b"");
    overlarge[6..8].copy_from_slice(&1025u16.to_le_bytes());
    assert_eq!(
        parse_x11_setup_request(&overlarge),
        Err(XSetupParseError::AuthFieldTooLarge {
            field: "authorization_protocol_name",
            len: 1025,
            max: X_SETUP_MAX_AUTH_FIELD_LEN,
        })
    );

    let mut truncated = setup_request(XByteOrder::LittleEndian, 11, 0, b"AUTH", b"DATA");
    truncated.pop();
    assert!(matches!(
        parse_x11_setup_request(&truncated),
        Err(XSetupParseError::Truncated { .. })
    ));
}

#[test]
fn x11_setup_success_reply_encodes_resource_id_facts() {
    let reply = encode_x11_setup_success(
        XByteOrder::LittleEndian,
        &XSetupSuccess {
            major_version: 11,
            minor_version: 0,
            release: 7,
            resource_id_base: 0x0020_0000,
            resource_id_mask: 0x001f_ffff,
            max_request_units: 4096,
            vendor: b"Sophia".to_vec(),
            roots: 0,
            formats: 0,
            root_size: Size {
                width: 1280,
                height: 720,
            },
        },
    )
    .unwrap();

    assert_eq!(reply[0], 1);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[2..4]), 11);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[4..6]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &reply[8..12]), 7);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reply[12..16]),
        0x0020_0000
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reply[16..20]),
        0x001f_ffff
    );
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[24..26]), 6);
    assert_eq!(&reply[40..46], b"Sophia");
}

#[test]
fn x11_setup_success_reply_advertises_exact_true_color_visuals_in_both_orders() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let reply =
            encode_x11_setup_success(byte_order, &XSetupSuccess::client_compatible()).unwrap();

        assert_eq!(reply[0], 1);
        assert_eq!(reply[28], 1);
        assert_eq!(reply[29], 7);
        for (offset, expected) in [
            (48, [1, 1, 32]),
            (56, [4, 4, 32]),
            (64, [8, 8, 32]),
            (72, [15, 16, 32]),
            (80, [16, 16, 32]),
            (88, [24, 32, 32]),
            (96, [32, 32, 32]),
        ] {
            assert_eq!(&reply[offset..offset + 3], &expected);
        }
        assert_eq!(read_u32(byte_order, &reply[104..108]), X_SETUP_DEFAULT_ROOT);
        assert_eq!(
            read_u32(byte_order, &reply[108..112]),
            X_SETUP_DEFAULT_COLORMAP
        );
        assert_eq!(read_u32(byte_order, &reply[136..140]), X_SETUP_DEFAULT_VISUAL);
        assert_eq!(reply[142], 24);
        assert_eq!(reply[143], 7);
        for (offset, depth) in [(144, 1), (152, 4), (160, 8), (168, 15), (176, 16)] {
            assert_eq!(reply[offset], depth);
            assert_eq!(read_u16(byte_order, &reply[offset + 2..offset + 4]), 0);
        }

        for (offset, visual, depth) in [
            (192, X_SETUP_DEFAULT_VISUAL, 24),
            (224, X_SETUP_ARGB_VISUAL, 32),
        ] {
            assert_eq!(read_u32(byte_order, &reply[offset..offset + 4]), visual);
            assert_eq!(reply[offset + 4], 4);
            assert_eq!(reply[offset + 5], 8);
            assert_eq!(read_u16(byte_order, &reply[offset + 6..offset + 8]), 256);
            assert_eq!(
                read_u32(byte_order, &reply[offset + 8..offset + 12]),
                X_TRUE_COLOR_RED_MASK
            );
            assert_eq!(
                read_u32(byte_order, &reply[offset + 12..offset + 16]),
                X_TRUE_COLOR_GREEN_MASK
            );
            assert_eq!(
                read_u32(byte_order, &reply[offset + 16..offset + 20]),
                X_TRUE_COLOR_BLUE_MASK
            );
            assert_eq!(reply[offset - 8], depth);
        }
    }
}

#[test]
fn glx_vendor_string_is_nul_terminated_even_at_four_bytes() {
    let encoded = encode_x_client_output(
        XByteOrder::LittleEndian,
        XClientOutput::Reply(XClientReply::GlxString {
            sequence: 9,
            value: "mesa".to_owned(),
        }),
    );
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[12..16]), 5);
    assert_eq!(&encoded[32..37], b"mesa\0");
    assert_eq!(encoded.len(), 40);
}

#[test]
fn glx_config_requests_decode_in_both_byte_orders() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        for (minor, expected) in [
            (
                X_GLX_GET_VISUAL_CONFIGS_MINOR_OPCODE,
                XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxGetVisualConfigs { screen: 0 }),
            ),
            (
                X_GLX_GET_FB_CONFIGS_MINOR_OPCODE,
                XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxGetFbConfigs { screen: 0 }),
            ),
        ] {
            let mut request = vec![X_GLX_MAJOR_OPCODE, minor, 0, 0, 0, 0, 0, 0];
            match byte_order {
                XByteOrder::LittleEndian => request[2..4].copy_from_slice(&2u16.to_le_bytes()),
                XByteOrder::BigEndian => request[2..4].copy_from_slice(&2u16.to_be_bytes()),
            }
            assert_eq!(
                decode_x11_core_request(context(NamespaceId::from_raw(1), 1, byte_order), &request)
                    .unwrap(),
                expected
            );
        }
    }
}

#[test]
fn glx_fb_config_reply_uses_tagged_attribute_pairs() {
    let configs = vec![vec![(0x8013, 3), (0x800b, X_SETUP_ARGB_VISUAL)]];
    let encoded = encode_x_client_output(
        XByteOrder::BigEndian,
        XClientOutput::Reply(XClientReply::GlxFbConfigs {
            sequence: 11,
            configs,
        }),
    );
    assert_eq!(read_u32(XByteOrder::BigEndian, &encoded[8..12]), 1);
    assert_eq!(read_u32(XByteOrder::BigEndian, &encoded[12..16]), 2);
    assert_eq!(read_u32(XByteOrder::BigEndian, &encoded[32..36]), 0x8013);
    assert_eq!(read_u32(XByteOrder::BigEndian, &encoded[36..40]), 3);
    assert_eq!(
        read_u32(XByteOrder::BigEndian, &encoded[44..48]),
        X_SETUP_ARGB_VISUAL
    );
}

#[test]
fn legacy_glx_context_uses_a_visual_and_normalizes_to_its_fbconfig() {
    let byte_order = XByteOrder::LittleEndian;
    let namespace = NamespaceId::from_raw(1);
    let context_id = XResourceId::new(0x0020_000b, 1);
    let mut request = vec![X_GLX_MAJOR_OPCODE, X_GLX_CREATE_CONTEXT_MINOR_OPCODE];
    push_u16(&mut request, byte_order, 6);
    push_u32(&mut request, byte_order, context_id.local.raw() as u32);
    push_u32(&mut request, byte_order, X_SETUP_DEFAULT_VISUAL);
    push_u32(&mut request, byte_order, 0);
    push_u32(&mut request, byte_order, 0);
    request.extend_from_slice(&[1, 0, 0, 0]);

    let decoded =
        decode_x11_core_request(context(namespace, 1, byte_order), &request).unwrap();
    assert_eq!(
        decoded,
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxCreateContext {
            context: context_id,
            config: XGlxContextConfig::Visual(X_SETUP_DEFAULT_VISUAL),
            screen: 0,
            share: None,
            direct: true,
        })
    );

    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let drawable = XResourceId::new(0x0020_000c, 1);
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(1),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window: drawable,
            surface: SurfaceId::new(90, 1),
            geometry: Rect {
                x: 0,
                y: 0,
                width: 500,
                height: 500,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 12, byte_order, X_GLX_MAJOR_OPCODE),
        decoded,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(result.outputs.is_empty());
    assert_eq!(runtime.glx_context(namespace, context_id), Ok((1, true)));

    let mut make_current = vec![X_GLX_MAJOR_OPCODE, X_GLX_MAKE_CURRENT_MINOR_OPCODE];
    push_u16(&mut make_current, byte_order, 4);
    push_u32(&mut make_current, byte_order, drawable.local.raw() as u32);
    push_u32(&mut make_current, byte_order, context_id.local.raw() as u32);
    push_u32(&mut make_current, byte_order, 0);
    let decoded =
        decode_x11_core_request(context(namespace, 2, byte_order), &make_current).unwrap();
    assert_eq!(
        decoded,
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxMakeCurrent {
            drawable: Some(drawable),
            context: Some(context_id),
            old_context_tag: 0,
        })
    );
    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 13, byte_order, X_GLX_MAJOR_OPCODE),
        decoded,
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(byte_order)
    .remove(0);
    assert_eq!(read_u32(byte_order, &encoded[8..12]), 1);
}

#[test]
fn kitty_glx_context_attribs_layout_decodes_the_28_byte_header() {
    let byte_order = XByteOrder::LittleEndian;
    let mut request = vec![
        X_GLX_MAJOR_OPCODE,
        X_GLX_CREATE_CONTEXT_ATTRIBS_ARB_MINOR_OPCODE,
    ];
    push_u16(&mut request, byte_order, 13);
    push_u32(&mut request, byte_order, 0x0020_000b);
    push_u32(&mut request, byte_order, 3);
    push_u32(&mut request, byte_order, 0);
    push_u32(&mut request, byte_order, 0);
    request.extend_from_slice(&[1, 0, 0, 0]);
    push_u32(&mut request, byte_order, 3);
    for (attribute, value) in [(0x2091, 3), (0x2092, 1), (0x9126, 1)] {
        push_u32(&mut request, byte_order, attribute);
        push_u32(&mut request, byte_order, value);
    }
    assert_eq!(request.len(), 52);
    assert_eq!(
        decode_x11_core_request(context(NamespaceId::from_raw(1), 1, byte_order), &request)
            .unwrap(),
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxCreateContext {
            context: XResourceId::new(0x0020_000b, 1),
            config: XGlxContextConfig::FbConfig(3),
            screen: 0,
            share: None,
            direct: true,
        })
    );
}

#[test]
fn kitty_fbconfig_catalog_has_argb_blue_aux_and_srgb_attributes() {
    let namespace = NamespaceId::from_raw(1);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 12, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxGetFbConfigs { screen: 0 }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian)
    .remove(0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[8..12]), 3);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[12..16]), 29);
    let pair = |config: usize, attribute: usize| 32 + (config * 29 + attribute) * 8;
    let aux = pair(0, 10);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[aux..aux + 4]),
        7
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[aux + 4..aux + 8]),
        0
    );
    let blue = pair(0, 13);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[blue..blue + 4]),
        10
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[blue + 4..blue + 8]),
        8
    );
    let depth = pair(0, 15);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[depth..depth + 4]),
        12
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[depth + 4..depth + 8]),
        24
    );
    let srgb = pair(2, 25);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[srgb..srgb + 4]),
        0x20b2
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &encoded[srgb + 4..srgb + 8]),
        1
    );
}

#[test]
fn legacy_glx_visual_catalog_has_rgba_double_buffer_and_depth() {
    let namespace = NamespaceId::from_raw(1);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let encoded = dispatch_x11_wire_request(
        dispatch_context(namespace, 12, XByteOrder::LittleEndian, X_GLX_MAJOR_OPCODE),
        XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxGetVisualConfigs { screen: 0 }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian)
    .remove(0);

    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[8..12]), 2);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[12..16]), 18);
    let field = |config: usize, attribute: usize| 32 + (config * 18 + attribute) * 4;
    assert_eq!(
        read_u32(
            XByteOrder::LittleEndian,
            &encoded[field(0, 2)..field(0, 2) + 4]
        ),
        1
    );
    assert_eq!(
        read_u32(
            XByteOrder::LittleEndian,
            &encoded[field(0, 11)..field(0, 11) + 4]
        ),
        1
    );
    assert_eq!(
        read_u32(
            XByteOrder::LittleEndian,
            &encoded[field(0, 14)..field(0, 14) + 4]
        ),
        24
    );
}

#[test]
fn sync_counter_and_kitty_teardown_requests_decode() {
    let namespace = NamespaceId::from_raw(1);
    let mut initialize = vec![
        X_SYNC_MAJOR_OPCODE,
        X_SYNC_INITIALIZE_MINOR_OPCODE,
        2,
        0,
        3,
        1,
        0,
        0,
    ];
    initialize[2..4].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(
        decode_x11_core_request(context(namespace, 1, XByteOrder::LittleEndian), &initialize)
            .unwrap(),
        XWireRequest::Sync(sophia_x_authority::XSyncRequest::SyncInitialize {
            desired_major: 3,
            desired_minor: 1
        })
    );
    let counter = 0x0020_0010u32;
    for (minor, value, expected) in [
        (
            X_SYNC_CREATE_COUNTER_MINOR_OPCODE,
            -2i64,
            XWireRequest::Sync(sophia_x_authority::XSyncRequest::SyncCreateCounter {
                counter: XResourceId::new(u64::from(counter), 1),
                initial_value: -2,
            }),
        ),
        (
            X_SYNC_SET_COUNTER_MINOR_OPCODE,
            17,
            XWireRequest::Sync(sophia_x_authority::XSyncRequest::SyncSetCounter {
                counter: XResourceId::new(u64::from(counter), 1),
                value: 17,
            }),
        ),
        (
            X_SYNC_CHANGE_COUNTER_MINOR_OPCODE,
            -3,
            XWireRequest::Sync(sophia_x_authority::XSyncRequest::SyncChangeCounter {
                counter: XResourceId::new(u64::from(counter), 1),
                delta: -3,
            }),
        ),
    ] {
        let mut request = vec![X_SYNC_MAJOR_OPCODE, minor, 4, 0];
        request.extend_from_slice(&counter.to_le_bytes());
        request.extend_from_slice(&((value >> 32) as u32).to_le_bytes());
        request.extend_from_slice(&(value as u32).to_le_bytes());
        assert_eq!(
            decode_x11_core_request(context(namespace, 2, XByteOrder::LittleEndian), &request)
                .unwrap(),
            expected
        );
    }
    for (minor, expected) in [
        (
            X_SYNC_QUERY_COUNTER_MINOR_OPCODE,
            XWireRequest::Sync(sophia_x_authority::XSyncRequest::SyncQueryCounter {
                counter: XResourceId::new(u64::from(counter), 1),
            }),
        ),
        (
            X_SYNC_DESTROY_COUNTER_MINOR_OPCODE,
            XWireRequest::Sync(sophia_x_authority::XSyncRequest::SyncDestroyCounter {
                counter: XResourceId::new(u64::from(counter), 1),
            }),
        ),
    ] {
        let mut request = vec![X_SYNC_MAJOR_OPCODE, minor, 2, 0];
        request.extend_from_slice(&counter.to_le_bytes());
        assert_eq!(
            decode_x11_core_request(context(namespace, 3, XByteOrder::LittleEndian), &request)
                .unwrap(),
            expected
        );
    }
    for (major, minor, expected) in [
        (
            X_SYNC_MAJOR_OPCODE,
            X_SYNC_DESTROY_FENCE_MINOR_OPCODE,
            XWireRequest::Sync(sophia_x_authority::XSyncRequest::SyncDestroyFence {
                fence: XResourceId::new(0x0020_000f, 1),
            }),
        ),
        (
            79,
            0,
            XWireRequest::Core(sophia_x_authority::XCoreRequest::FreeColormap {
                colormap: XResourceId::new(0x0020_0008, 1),
            }),
        ),
    ] {
        let mut request = vec![major, minor, 2, 0];
        request.extend_from_slice(&0x0020_000fu32.to_le_bytes());
        if major == 79 {
            request[4..8].copy_from_slice(&0x0020_0008u32.to_le_bytes());
        }
        assert_eq!(
            decode_x11_core_request(context(namespace, 2, XByteOrder::LittleEndian), &request)
                .unwrap(),
            expected
        );
    }
}
