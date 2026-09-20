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
            XWireRequest::XTestGetVersion {
                major_version: 2,
                minor_version: 0x0102,
            },
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

            let XWireRequest::XTestCompareCursor {
                window,
                cursor: decoded,
            } = decode_xtest_in(byte_order, &request)
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
            XWireRequest::XTestFakeInput {
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
            },
            "{byte_order:?}"
        );
    }
}

#[test]
fn xtest_fake_input_masks_the_send_event_bit_but_remembers_it() {
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let body = fake_input_body(byte_order, 0x82, 38, 0, 0, 0, 0);
        let request = xtest_request(byte_order, X_TEST_FAKE_INPUT_MINOR_OPCODE, &body);

        let XWireRequest::XTestFakeInput {
            event_type,
            sent_event_type,
            ..
        } = decode_xtest_in(byte_order, &request)
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

        let XWireRequest::XTestFakeInput { events, .. } = decode_xtest_in(byte_order, &request)
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
            XWireRequest::XTestGrabControl { impervious: 2 }
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
            XWireRequest::XTestUnimplemented { minor_opcode: 9 }
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
            XWireRequest::XTestGetVersion {
                major_version: 2,
                minor_version: 2,
            },
        ),
        (
            X_TEST_COMPARE_CURSOR_MINOR_OPCODE,
            XWireRequest::XTestCompareCursor {
                window: XResourceId::new(0x0022_0001, 1),
                cursor: 0,
            },
        ),
        (
            X_TEST_FAKE_INPUT_MINOR_OPCODE,
            XWireRequest::XTestFakeInput {
                event_type: 2,
                sent_event_type: 2,
                detail: 38,
                delay: 0,
                root: 0,
                root_x: 0,
                root_y: 0,
                events: 1,
            },
        ),
        (
            X_TEST_GRAB_CONTROL_MINOR_OPCODE,
            XWireRequest::XTestGrabControl { impervious: 1 },
        ),
        (17, XWireRequest::XTestUnimplemented { minor_opcode: 17 }),
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
