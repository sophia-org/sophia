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

    // 120 is unassigned in the core protocol. 127 and then 110 used to
    // stand in here; NoOperation and ListHosts are both recognised now.
    let mut unknown = vec![120, 0];
    push_u16(&mut unknown, XByteOrder::LittleEndian, 1);
    assert_eq!(
        decode_x11_core_request(
            context(NamespaceId::from_raw(45), 507, XByteOrder::LittleEndian),
            &unknown
        ),
        Err(XWireParseError::UnknownOpcode(120))
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
            Ok(XWireRequest::Core(sophia_x_authority::XCoreRequest::NoOperation)),
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
            root_x: 0,
            root_y: 0,
            event_x: 0,
            event_y: 0,
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
