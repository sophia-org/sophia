// XTEST on the wire: what each of the four requests decodes to, and what a
// client that is not admitted to inject is told.
//
// Every decode is checked in both byte orders, because the only fields that
// could disagree are the ones a swapped client sends differently, and those
// are exactly the fields that decide a delay, a coordinate and a cursor.

fn xtest_request(byte_order: XByteOrder, minor: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![X_TEST_MAJOR_OPCODE, minor, 0, 0];
    bytes.extend_from_slice(body);
    let units = u16::try_from(bytes.len() / 4).expect("a request fits its own length field");
    let length = match byte_order {
        XByteOrder::LittleEndian => units.to_le_bytes(),
        XByteOrder::BigEndian => units.to_be_bytes(),
    };
    bytes[2..4].copy_from_slice(&length);
    bytes
}

fn u16_bytes(byte_order: XByteOrder, value: u16) -> [u8; 2] {
    match byte_order {
        XByteOrder::LittleEndian => value.to_le_bytes(),
        XByteOrder::BigEndian => value.to_be_bytes(),
    }
}

fn u32_bytes(byte_order: XByteOrder, value: u32) -> [u8; 4] {
    match byte_order {
        XByteOrder::LittleEndian => value.to_le_bytes(),
        XByteOrder::BigEndian => value.to_be_bytes(),
    }
}

/// One 32-byte FakeInput event record.
fn fake_input_body(
    byte_order: XByteOrder,
    event_type: u8,
    detail: u8,
    delay: u32,
    root: u32,
    root_x: i16,
    root_y: i16,
) -> Vec<u8> {
    let mut body = vec![0u8; 32];
    body[0] = event_type;
    body[1] = detail;
    body[4..8].copy_from_slice(&u32_bytes(byte_order, delay));
    body[8..12].copy_from_slice(&u32_bytes(byte_order, root));
    body[20..22].copy_from_slice(&u16_bytes(byte_order, root_x as u16));
    body[22..24].copy_from_slice(&u16_bytes(byte_order, root_y as u16));
    body
}

fn decode_xtest_in(byte_order: XByteOrder, bytes: &[u8]) -> XWireRequest {
    decode_x11_core_request(context(NamespaceId::from_raw(7), 1, byte_order), bytes)
        .expect("a well-formed XTEST request decodes")
}

#[test]
fn xtest_get_version_carries_what_was_asked_in_both_byte_orders() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut body = vec![0u8; 4];
        body[0] = 2;
        body[2..4].copy_from_slice(&u16_bytes(byte_order, 0x0102));
        let request = xtest_request(byte_order, X_TEST_GET_VERSION_MINOR_OPCODE, &body);

        // The requested version is carried and decides nothing: the reply is
        // a constant 2.1 whatever arrives here. It is decoded so a record can
        // say what a client asked for.
        assert_eq!(
            decode_xtest_in(byte_order, &request),
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestGetVersion {
                major_version: 2,
                minor_version: 0x0102,
            }),
            "{byte_order:?} must read the requested minor in its own order"
        );
    }
}

#[test]
fn xtest_compare_cursor_keeps_none_and_current_cursor_raw() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        for cursor in [0, X_TEST_CURRENT_CURSOR, 0x00c0_0007] {
            let mut body = Vec::new();
            body.extend_from_slice(&u32_bytes(byte_order, 0x0022_0001));
            body.extend_from_slice(&u32_bytes(byte_order, cursor));
            let request = xtest_request(byte_order, X_TEST_COMPARE_CURSOR_MINOR_OPCODE, &body);

            let XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestCompareCursor {
                window,
                cursor: decoded,
            }) = decode_xtest_in(byte_order, &request)
            else {
                panic!("CompareCursor must decode to its own variant");
            };
            assert_eq!(window.local.raw(), 0x0022_0001);
            // Raw, so zero stays None and one stays CurrentCursor. Making
            // either a resource id here would lose the distinction the
            // dispatcher has to draw before it looks anything up.
            assert_eq!(decoded, cursor, "{byte_order:?} cursor {cursor:#x}");
        }
    }
}

#[test]
fn xtest_fake_input_decodes_every_field_in_both_byte_orders() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let body = fake_input_body(byte_order, 6, X_TEST_MOTION_RELATIVE, 0xdead_beef, 0, -7, 9);
        let request = xtest_request(byte_order, X_TEST_FAKE_INPUT_MINOR_OPCODE, &body);
        assert_eq!(request.len(), 36, "the core request is 36 bytes");

        assert_eq!(
            decode_xtest_in(byte_order, &request),
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestFakeInput {
                event_type: 6,
                sent_event_type: 6,
                detail: X_TEST_MOTION_RELATIVE,
                // The full CARD32 domain, so the top bit must survive the
                // decode rather than being read as a sign.
                delay: 0xdead_beef,
                root: 0,
                root_x: -7,
                root_y: 9,
                events: 1,
            }),
            "{byte_order:?}"
        );
    }
}

#[test]
fn xtest_fake_input_masks_the_send_event_bit_but_remembers_it() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let body = fake_input_body(byte_order, 0x82, 38, 0, 0, 0, 0);
        let request = xtest_request(byte_order, X_TEST_FAKE_INPUT_MINOR_OPCODE, &body);

        let XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestFakeInput {
            event_type,
            sent_event_type,
            ..
        }) = decode_xtest_in(byte_order, &request)
        else {
            panic!("FakeInput must decode to its own variant");
        };
        // The reference server reads the type as `type & 0177`, so a set
        // send-event bit is an ordinary KeyPress rather than a refusal.
        assert_eq!(event_type, 2);
        // An error reports the byte that arrived, not the byte it was read
        // as, so the unmasked value has to survive to the dispatcher.
        assert_eq!(sent_event_type, 0x82);
    }
}

#[test]
fn xtest_fake_input_counts_event_records_without_judging_them() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut body = fake_input_body(byte_order, 2, 38, 0, 0, 0, 0);
        body.extend_from_slice(&fake_input_body(byte_order, 0, 0, 0, 0, 0, 0));
        let request = xtest_request(byte_order, X_TEST_FAKE_INPUT_MINOR_OPCODE, &body);

        let XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestFakeInput { events, .. }) = decode_xtest_in(byte_order, &request)
        else {
            panic!("FakeInput must decode to its own variant");
        };
        // Two records is a valid wire shape and an invalid request, and which
        // error it earns depends on the event type: a core type with two is a
        // length fault, an XInput type with two is a refused extension event.
        // The count is carried so the dispatcher can tell those apart.
        assert_eq!(events, 2);
    }
}

#[test]
fn xtest_refuses_a_body_that_is_not_whole_event_records() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        // A four-byte header and nothing else: no event at all.
        let empty = xtest_request(byte_order, X_TEST_FAKE_INPUT_MINOR_OPCODE, &[]);
        assert!(
            matches!(
                decode_x11_core_request(context(NamespaceId::from_raw(7), 1, byte_order), &empty),
                Err(XWireParseError::InvalidLength { .. })
            ),
            "{byte_order:?} must refuse a FakeInput carrying no event"
        );

        // A whole number of words that is not a whole number of records.
        let short = xtest_request(byte_order, X_TEST_FAKE_INPUT_MINOR_OPCODE, &[0u8; 16]);
        assert!(
            matches!(
                decode_x11_core_request(context(NamespaceId::from_raw(7), 1, byte_order), &short),
                Err(XWireParseError::InvalidLength { .. })
            ),
            "{byte_order:?} must refuse a partial event record"
        );
    }
}

#[test]
fn xtest_get_version_and_grab_control_require_their_exact_length() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        for minor in [
            X_TEST_GET_VERSION_MINOR_OPCODE,
            X_TEST_GRAB_CONTROL_MINOR_OPCODE,
        ] {
            let overlong = xtest_request(byte_order, minor, &[0u8; 8]);
            assert!(
                matches!(
                    decode_x11_core_request(
                        context(NamespaceId::from_raw(7), 1, byte_order),
                        &overlong
                    ),
                    Err(XWireParseError::InvalidLength { .. })
                ),
                "{byte_order:?} minor {minor} must require its exact length"
            );
        }
    }
}

#[test]
fn xtest_grab_control_carries_a_non_boolean_rather_than_refusing_it() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let request = xtest_request(byte_order, X_TEST_GRAB_CONTROL_MINOR_OPCODE, &[2, 0, 0, 0]);
        // The boolean is strict and two is BadValue, but that refusal names
        // the value it refused, so it belongs where a value can be reported
        // rather than here, where a parse either succeeds or does not.
        assert_eq!(
            decode_xtest_in(byte_order, &request),
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestGrabControl { impervious: 2 })
        );
    }
}

#[test]
fn xtest_minors_the_extension_does_not_define_decode_rather_than_fail() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let request = xtest_request(byte_order, 9, &[0u8; 4]);
        // Decoded rather than refused at the parser, so the client gets a
        // normal error against a sequence number it can attribute, which a
        // parse failure would deny it.
        assert_eq!(
            decode_xtest_in(byte_order, &request),
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestUnimplemented { minor_opcode: 9 })
        );
    }
}

#[test]
fn xtest_refuses_an_unadmitted_client_with_access_not_request() {
    let namespace = NamespaceId::from_raw(7);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    for (minor, request) in [
        (
            X_TEST_GET_VERSION_MINOR_OPCODE,
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestGetVersion {
                major_version: 2,
                minor_version: 2,
            }),
        ),
        (
            X_TEST_COMPARE_CURSOR_MINOR_OPCODE,
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestCompareCursor {
                window: XResourceId::new(0x0022_0001, 1),
                cursor: 0,
            }),
        ),
        (
            X_TEST_FAKE_INPUT_MINOR_OPCODE,
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestFakeInput {
                event_type: 2,
                sent_event_type: 2,
                detail: 38,
                delay: 0,
                root: 0,
                root_x: 0,
                root_y: 0,
                events: 1,
            }),
        ),
        (
            X_TEST_GRAB_CONTROL_MINOR_OPCODE,
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestGrabControl { impervious: 1 }),
        ),
        (17, XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestUnimplemented { minor_opcode: 17 })),
    ] {
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, 90, XByteOrder::LittleEndian, X_TEST_MAJOR_OPCODE),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );

        let [XClientOutput::Error(error)] = result.outputs.as_slice() else {
            panic!("minor {minor} must answer with exactly one error");
        };
        // BadAccess, not BadRequest. An undecoded major answers BadRequest,
        // which tells a client the server has no such extension; a client
        // that guessed the opcode would believe it. BadAccess says the
        // request exists and this client may not make it, which is true.
        assert_eq!(error.code, XErrorCode::BadAccess, "minor {minor}");
        assert_eq!(error.major_code, X_TEST_MAJOR_OPCODE);
        assert_eq!(error.minor_code, u16::from(minor));
        assert_eq!(error.sequence, 90);
        // No resource was named, and naming one the request did not carry
        // would invent evidence.
        assert_eq!(error.resource_id, 0, "minor {minor}");
        assert!(result.response.is_none(), "minor {minor} owes no reply");
    }
}

#[test]
fn xtest_answers_an_admitted_client_a_constant_version() {
    let namespace = NamespaceId::from_raw(7);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let admitted = XDispatchContext {
        injection: XTestAdmission::Admitted,
        ..dispatch_context(namespace, 91, XByteOrder::LittleEndian, X_TEST_MAJOR_OPCODE)
    };

    for requested in [(1u8, 0u16), (2, 2), (255, 65535)] {
        let result = dispatch_x11_wire_request(
            admitted,
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestGetVersion {
                major_version: requested.0,
                minor_version: requested.1,
            }),
            &mut runtime,
            &mut atoms,
            &mut properties,
        );

        let [XClientOutput::Reply(XClientReply::XTestGetVersion {
            sequence,
            major_version,
            minor_version,
        })] = result.outputs.as_slice()
        else {
            panic!("an admitted client is answered, not refused");
        };
        // The same answer whatever was asked for. The reference server never
        // reads the requested version, and a client that asked for 2.2 is
        // told 2.1 rather than being refused, because there is no
        // negotiation to fail.
        assert_eq!(*major_version, 2, "requested {requested:?}");
        assert_eq!(*minor_version, 1, "requested {requested:?}");
        assert_eq!(*sequence, 91);
    }
}

#[test]
fn xtest_tells_an_admitted_client_an_undefined_minor_does_not_exist() {
    let namespace = NamespaceId::from_raw(7);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let admitted = XDispatchContext {
        injection: XTestAdmission::Admitted,
        ..dispatch_context(namespace, 92, XByteOrder::LittleEndian, X_TEST_MAJOR_OPCODE)
    };

    let result = dispatch_x11_wire_request(
        admitted,
        XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestUnimplemented { minor_opcode: 9 }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let [XClientOutput::Error(error)] = result.outputs.as_slice() else {
        panic!("an undefined minor is refused");
    };
    // BadRequest and not BadAccess. For a client that may use the extension,
    // a minor it does not define is a request that does not exist, which is a
    // different statement from one it may not make.
    assert_eq!(error.code, XErrorCode::BadRequest);
    assert_eq!(error.minor_code, 9);
}

fn admitted_context(sequence: u16, byte_order: XByteOrder) -> XDispatchContext {
    XDispatchContext {
        injection: XTestAdmission::Admitted,
        ..dispatch_context(NamespaceId::from_raw(7), sequence, byte_order, X_TEST_MAJOR_OPCODE)
    }
}

fn fake_input_request(event_type: u8, detail: u8, root: u32, events: usize) -> XWireRequest {
    XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestFakeInput {
        event_type: event_type & X_TEST_EVENT_TYPE_MASK,
        sent_event_type: event_type,
        detail,
        delay: 0,
        root,
        root_x: 0,
        root_y: 0,
        events,
    })
}

#[test]
fn xtest_fake_input_refuses_in_the_reference_order_with_the_refused_value() {
    // Each row is one refusal the reference server makes, with the value it
    // names. The order matters and is observable: a request with a bad type
    // AND a bad detail reports the type, and a core type with two records is
    // a length fault whatever its detail.
    let table: [(&str, XWireRequest, XErrorCode, u32); 10] = [
        ("type 0", fake_input_request(0, 38, 0, 1), XErrorCode::BadValue, 0),
        ("type 35", fake_input_request(35, 38, 0, 1), XErrorCode::BadValue, 35),
        // The send-event bit is masked for dispatch and reported as sent.
        ("type 0x87", fake_input_request(0x87, 38, 0, 1), XErrorCode::BadValue, 0x87),
        ("two records", fake_input_request(2, 38, 0, 2), XErrorCode::BadLength, 0),
        ("key below 8", fake_input_request(2, 7, 0, 1), XErrorCode::BadValue, 7),
        ("button 0", fake_input_request(4, 0, 0, 1), XErrorCode::BadValue, 0),
        ("button 11", fake_input_request(5, 11, 0, 1), XErrorCode::BadValue, 11),
        ("motion detail 2", fake_input_request(6, 2, 0, 1), XErrorCode::BadValue, 2),
        // A root that is no window at all.
        ("unknown root", fake_input_request(6, 0, 0x00c0_0999, 1), XErrorCode::BadWindow, 0x00c0_0999),
        // Type before detail: this one has both wrong and names the type.
        ("type then detail", fake_input_request(7, 0, 0, 1), XErrorCode::BadValue, 7),
    ];
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        for (label, request, code, value) in table.iter().cloned() {
            let result = dispatch_x11_wire_request(
                admitted_context(40, byte_order),
                request,
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            let [XClientOutput::Error(error)] = result.outputs.as_slice() else {
                panic!("{byte_order:?} {label}: exactly one error is owed");
            };
            assert_eq!(error.code, code, "{byte_order:?} {label}");
            assert_eq!(error.resource_id, value, "{byte_order:?} {label} names its value");
            assert_eq!(error.minor_code, u16::from(X_TEST_FAKE_INPUT_MINOR_OPCODE));
            assert_eq!(error.major_code, X_TEST_MAJOR_OPCODE);
        }
    }
}

#[test]
fn xtest_fake_input_accepts_every_core_type_and_owes_no_reply() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        for (event_type, detail) in [(2u8, 38u8), (3, 38), (4, 1), (5, 10), (6, 0), (6, 1)] {
            let result = dispatch_x11_wire_request(
                admitted_context(41, byte_order),
                fake_input_request(event_type, detail, 0, 1),
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            // Acceptance is the absence of an error. FakeInput has no reply,
            // and the reference sends none, so a reply here would be a bug
            // the client could see in its sequence accounting.
            assert!(
                result.outputs.is_empty() && result.response.is_none(),
                "{byte_order:?} type {event_type} detail {detail}: {:?}",
                result.outputs
            );
        }
    }
}

#[test]
fn xtest_fake_input_motion_root_must_be_the_root() {
    // A window that exists and is not the root is BadValue, which is a
    // different fault from a resource that does not exist.
    let byte_order = XByteOrder::LittleEndian;
    let namespace = NamespaceId::from_raw(7);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let child = 0x0022_0001u32;
    let created = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, byte_order, 1),
        decode_x11_core_request(
            context(namespace, 1, byte_order),
            &create_window_request(byte_order, child, 10, 20, 64, 48),
        )
        .expect("a window to create"),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        !created.outputs.iter().any(|o| matches!(o, XClientOutput::Error(_))),
        "the fixture window must exist: {:?}",
        created.outputs
    );

    let refused = dispatch_x11_wire_request(
        admitted_context(42, byte_order),
        fake_input_request(6, 0, child, 1),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let [XClientOutput::Error(error)] = refused.outputs.as_slice() else {
        panic!("a non-root window is refused");
    };
    assert_eq!(error.code, XErrorCode::BadValue);
    assert_eq!(error.resource_id, child);

    let accepted = dispatch_x11_wire_request(
        admitted_context(43, byte_order),
        fake_input_request(6, 0, X_SETUP_DEFAULT_ROOT, 1),
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(accepted.outputs.is_empty(), "the root itself is accepted");
}

#[test]
fn xtest_grab_control_takes_a_strict_boolean() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        for impervious in [0u8, 1] {
            let result = dispatch_x11_wire_request(
                admitted_context(44, byte_order),
                XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestGrabControl { impervious }),
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            assert!(result.outputs.is_empty(), "{byte_order:?} {impervious} is accepted");
        }
        let result = dispatch_x11_wire_request(
            admitted_context(45, byte_order),
            XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestGrabControl { impervious: 2 }),
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let [XClientOutput::Error(error)] = result.outputs.as_slice() else {
            panic!("two is not a boolean");
        };
        // The core protocol lets many BOOL fields pass any nonzero. This one
        // does not, and it names the value it refused.
        assert_eq!(error.code, XErrorCode::BadValue);
        assert_eq!(error.resource_id, 2);
    }
}

/// Put a window carrying its own cursor into a fresh runtime, and answer
/// with the ids of the window and the cursor.
fn window_with_a_cursor(
    byte_order: XByteOrder,
    namespace: NamespaceId,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
) -> (u32, u32) {
    let window = 0x0026_0001u32;
    let pixmap = 0x0026_0002u32;
    let cursor = 0x0026_0003u32;
    let mut send = |sequence: u16, opcode: u8, bytes: Vec<u8>| {
        let request = decode_x11_core_request(context(namespace, u64::from(sequence), byte_order), &bytes)
            .expect("a well-formed fixture request");
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, byte_order, opcode),
            request,
            runtime,
            atoms,
            properties,
        );
        assert!(
            !result
                .outputs
                .iter()
                .any(|output| matches!(output, XClientOutput::Error(_))),
            "{byte_order:?} fixture request {opcode} refused: {:?}",
            result.outputs
        );
    };
    send(1, 1, create_window_request(byte_order, window, 10, 20, 64, 48));
    send(2, 53, create_pixmap_request(byte_order, 1, pixmap, window, 1, 1));
    send(3, 93, create_cursor_request(byte_order, cursor, pixmap));
    send(4, 2, change_window_cursor_request(byte_order, window, cursor));
    (window, cursor)
}

#[test]
fn xtest_compare_cursor_answers_the_windows_own_cursor_and_none() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let namespace = NamespaceId::from_raw(7);
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        let (window, cursor) = window_with_a_cursor(
            byte_order,
            namespace,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let mut compare = |sequence: u16, window: u32, cursor: u32| {
            dispatch_x11_wire_request(
                admitted_context(sequence, byte_order),
                XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestCompareCursor {
                    window: XResourceId::new(u64::from(window), 1),
                    cursor,
                }),
                &mut runtime,
                &mut atoms,
                &mut properties,
            )
        };

        let same = compare(50, window, cursor);
        let [XClientOutput::Reply(XClientReply::XTestCompareCursor { same, .. })] =
            same.outputs.as_slice()
        else {
            panic!("{byte_order:?} CompareCursor owes a reply: {:?}", same.outputs);
        };
        assert!(*same, "{byte_order:?} the window shows the cursor it was given");

        // Zero is None, and None is a real answer: this window has a cursor,
        // so it does not show none.
        let none = compare(51, window, 0);
        let [XClientOutput::Reply(XClientReply::XTestCompareCursor { same, .. })] =
            none.outputs.as_slice()
        else {
            panic!("{byte_order:?} CompareCursor owes a reply");
        };
        assert!(!*same, "{byte_order:?} a window with a cursor shows something");

        // One is CurrentCursor. Nothing has observed the pointer in this
        // runtime, so what it shows is nothing, which the window does not
        // match. The question is still answered rather than refused.
        let current = compare(52, window, 1);
        let [XClientOutput::Reply(XClientReply::XTestCompareCursor { same, .. })] =
            current.outputs.as_slice()
        else {
            panic!("{byte_order:?} CurrentCursor is answered, not refused");
        };
        assert!(!*same, "{byte_order:?} an unobserved pointer shows no cursor");
    }
}

#[test]
fn xtest_compare_cursor_refuses_the_window_before_the_cursor() {
    // Both are wrong. The window is reported, because that is the order the
    // request reads and therefore the order a client can rely on.
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let namespace = NamespaceId::from_raw(7);
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        let (window, _) = window_with_a_cursor(
            byte_order,
            namespace,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let absent_window = 0x0026_0111u32;
        let absent_cursor = 0x0026_0222u32;
        for (named_window, named_cursor, code, value) in [
            (absent_window, absent_cursor, XErrorCode::BadWindow, absent_window),
            (absent_window, 0, XErrorCode::BadWindow, absent_window),
            (absent_window, 1, XErrorCode::BadWindow, absent_window),
            (window, absent_cursor, XErrorCode::BadCursor, absent_cursor),
        ] {
            let result = dispatch_x11_wire_request(
                admitted_context(53, byte_order),
                XWireRequest::XTest(sophia_x_authority::XTestRequest::XTestCompareCursor {
                    window: XResourceId::new(u64::from(named_window), 1),
                    cursor: named_cursor,
                }),
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            let [XClientOutput::Error(error)] = result.outputs.as_slice() else {
                panic!("{byte_order:?} {named_window:#x}/{named_cursor:#x} must be refused");
            };
            assert_eq!(error.code, code, "{byte_order:?} {named_window:#x}/{named_cursor:#x}");
            assert_eq!(error.resource_id, value);
            assert_eq!(error.minor_code, u16::from(X_TEST_COMPARE_CURSOR_MINOR_OPCODE));
            assert_eq!(error.major_code, X_TEST_MAJOR_OPCODE);
        }
    }
}

#[test]
fn the_cursor_attribute_is_decoded_in_both_orders() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let namespace = NamespaceId::from_raw(7);
        let bytes = change_window_cursor_request(byte_order, 0x0026_0001, 0x0026_0003);
        let request = decode_x11_core_request(context(namespace, 1, byte_order), &bytes)
            .expect("a well-formed attribute request");
        let XWireRequest::Core(sophia_x_authority::XCoreRequest::ChangeWindowAttributes { cursor, window, .. }) = request else {
            panic!("{byte_order:?} decoded the wrong request");
        };
        // Raw, because zero is None in this attribute rather than a resource.
        assert_eq!(cursor, Some(0x0026_0003));
        assert_eq!(window.local.raw(), 0x0026_0001);
    }
}
