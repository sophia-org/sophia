fn decode_xtest(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_TEST_GET_VERSION_MINOR_OPCODE => {
            require_exact_len(
                X_TEST_MAJOR_OPCODE,
                X_TEST_GET_VERSION_REQ_LEN,
                bytes.len(),
            )?;
            // Both fields are decoded and neither decides anything: the reply
            // is a constant. They are carried so a record can say what a
            // client asked for, and so the byte-order tests have a swapped
            // field to check. The reference server reads the minor only to
            // swap it and then discards the swapped value.
            Ok(XWireRequest::XTestGetVersion {
                major_version: bytes[4],
                minor_version: context.byte_order.u16(&bytes[6..8]),
            })
        }
        X_TEST_COMPARE_CURSOR_MINOR_OPCODE => {
            require_exact_len(
                X_TEST_MAJOR_OPCODE,
                X_TEST_COMPARE_CURSOR_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::XTestCompareCursor {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                // Raw, because zero is None and one is CurrentCursor. Turning
                // either into a resource id here would lose the distinction
                // the dispatcher has to make before it looks anything up.
                cursor: context.byte_order.u32(&bytes[8..12]),
            })
        }
        X_TEST_FAKE_INPUT_MINOR_OPCODE => {
            // The wire shape first: the body is a whole number of 32-byte
            // event records and at least one. The count is carried rather
            // than resolved here because what a count of two means depends on
            // the event type, and the type's own validity is the dispatcher's
            // question. A core type with two records is a length fault; an
            // XInput type with two is a refused extension event, and those
            // are different answers.
            let body = bytes.len().saturating_sub(X_TEST_FAKE_INPUT_HEADER_LEN);
            if body == 0 || !body.is_multiple_of(X_TEST_FAKE_INPUT_EVENT_LEN) {
                return Err(XWireParseError::InvalidLength {
                    opcode: X_TEST_MAJOR_OPCODE,
                    expected_at_least: X_TEST_FAKE_INPUT_REQ_LEN,
                    actual: bytes.len(),
                });
            }
            Ok(XWireRequest::XTestFakeInput {
                // Masked of its send-event bit, which the reference server
                // ignores, so 0x82 is an ordinary KeyPress. The unmasked byte
                // is kept beside it because an error reports what arrived,
                // not what it was read as.
                event_type: bytes[4] & X_TEST_EVENT_TYPE_MASK,
                sent_event_type: bytes[4],
                detail: bytes[5],
                delay: context.byte_order.u32(&bytes[8..12]),
                // Raw: zero is None, meaning the pointer's current screen.
                root: context.byte_order.u32(&bytes[12..16]),
                root_x: context.byte_order.i16(&bytes[24..26]),
                root_y: context.byte_order.i16(&bytes[26..28]),
                events: body / X_TEST_FAKE_INPUT_EVENT_LEN,
            })
        }
        X_TEST_GRAB_CONTROL_MINOR_OPCODE => {
            require_exact_len(
                X_TEST_MAJOR_OPCODE,
                X_TEST_GRAB_CONTROL_REQ_LEN,
                bytes.len(),
            )?;
            // Carried unvalidated. The boolean is strict -- anything but zero
            // or one is BadValue -- but that refusal names a value, so it
            // belongs where a value can be reported rather than here where a
            // parse either succeeds or does not.
            Ok(XWireRequest::XTestGrabControl {
                impervious: bytes[4],
            })
        }
        // XTEST has had four requests since it was defined and will not grow
        // any: anything else is a request this extension does not have.
        minor_opcode => Ok(XWireRequest::XTestUnimplemented { minor_opcode }),
    }
}
