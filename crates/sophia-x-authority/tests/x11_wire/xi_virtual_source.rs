mod xi_virtual_source {
    use super::*;

    fn dispatch(request: XWireRequest, atoms: &mut XAtomTable, order: XByteOrder) -> Vec<u8> {
        dispatch_x11_wire_request(
            dispatch_context(NamespaceId::from_raw(58), 7, order, X_INPUT_MAJOR_OPCODE),
            request,
            &mut XAuthorityRuntime::new(),
            atoms,
            &mut XPropertyTable::new(),
        )
        .encoded_outputs(order)
        .remove(0)
    }

    #[test]
    fn xi_virtual_source_query_filters_and_classes_are_consistent() {
        for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let mut atoms = XAtomTable::new();
            for (query, expected) in [
                (0, vec![2, 3, 128]),
                (1, vec![2, 3]),
                (2, vec![2]),
                (3, vec![3]),
                (128, vec![128]),
            ] {
                let reply = dispatch(
                    XWireRequest::Xi(sophia_x_authority::XInputRequest::XiQueryDevice { device_id: query }),
                    &mut atoms,
                    order,
                );
                assert_eq!(reply[0], 1);
                assert_eq!(usize::from(read_u16(order, &reply[8..10])), expected.len());
                assert_eq!(reply.len(), 32 + read_u32(order, &reply[4..8]) as usize * 4);
                let mut offset = 32;
                for id in expected {
                    assert_eq!(read_u16(order, &reply[offset..offset + 2]), id);
                    let (kind, attachment, source) = match id {
                        2 => (1, 3, 128),
                        3 => (2, 2, 3),
                        128 => (3, 2, 128),
                        _ => unreachable!(),
                    };
                    assert_eq!(read_u16(order, &reply[offset + 2..offset + 4]), kind);
                    assert_eq!(read_u16(order, &reply[offset + 4..offset + 6]), attachment);
                    assert_eq!(reply[offset + 10], 1, "device enabled");
                    let classes = read_u16(order, &reply[offset + 6..offset + 8]);
                    assert_eq!(classes, if id == 3 { 1 } else { 7 });
                    let name_len = usize::from(read_u16(order, &reply[offset + 8..offset + 10]));
                    offset += 12 + name_len.next_multiple_of(4);
                    let mut valuators = 0;
                    let mut scrolls = 0;
                    for _ in 0..classes {
                        assert_eq!(read_u16(order, &reply[offset + 4..offset + 6]), source);
                        match read_u16(order, &reply[offset..offset + 2]) {
                            2 => {
                                let number = read_u16(order, &reply[offset + 6..offset + 8]);
                                let label = read_u32(order, &reply[offset + 8..offset + 12]);
                                assert_eq!(
                                    atoms.name(label),
                                    Some(
                                        ["Rel X", "Rel Y", "Rel Horiz Scroll", "Rel Vert Scroll",]
                                            [usize::from(number)]
                                    )
                                );
                                assert_eq!(reply[offset + 40], 0, "relative valuator");
                                valuators += 1;
                            }
                            3 => {
                                assert_eq!(read_u32(order, &reply[offset + 16..offset + 20]), 120);
                                assert_eq!(read_u32(order, &reply[offset + 20..offset + 24]), 0);
                                scrolls += 1;
                            }
                            _ => {}
                        }
                        offset += usize::from(read_u16(order, &reply[offset + 2..offset + 4])) * 4;
                    }
                    assert_eq!((valuators, scrolls), if id == 3 { (0, 0) } else { (4, 2) });
                }
                assert_eq!(offset, reply.len());
            }
        }
    }

    #[test]
    fn xi_virtual_source_is_not_in_the_legacy_inventory() {
        let reply = dispatch(
            XWireRequest::Xi(sophia_x_authority::XInputRequest::XiListInputDevices),
            &mut XAtomTable::new(),
            XByteOrder::LittleEndian,
        );
        assert_eq!(reply[8], 2);
        assert_eq!([reply[36], reply[44]], [2, 3]);
        assert_eq!(
            [reply[38], reply[46]],
            [X_INPUT_LEGACY_USE_POINTER, X_INPUT_LEGACY_USE_KEYBOARD]
        );
    }

    #[test]
    fn xi_virtual_source_query_pointer_and_grab_have_explicit_errors() {
        for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            for (request, code, minor) in [
                (
                    XWireRequest::Xi(sophia_x_authority::XInputRequest::XiQueryPointer {
                        window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                        device_id: X_INPUT_POINTER_SOURCE_ID,
                    }),
                    X_INPUT_FIRST_ERROR,
                    X_INPUT_QUERY_POINTER_MINOR_OPCODE,
                ),
                (
                    XWireRequest::Xi(sophia_x_authority::XInputRequest::XiGrabDevice {
                        window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                        time: 0,
                        cursor: None,
                        device_id: X_INPUT_POINTER_SOURCE_ID,
                        pointer_mode: 1,
                        keyboard_mode: 1,
                        owner_events: false,
                        event_mask: vec![0],
                    }),
                    XErrorCode::BadAccess.wire_code(),
                    X_INPUT_GRAB_DEVICE_MINOR_OPCODE,
                ),
            ] {
                let reply = dispatch(request, &mut XAtomTable::new(), order);
                assert_eq!(reply.len(), 32);
                assert_eq!(&reply[..2], &[0, code]);
                assert_eq!(read_u16(order, &reply[2..4]), 7);
                assert_eq!(
                    read_u32(order, &reply[4..8]),
                    u32::from(X_INPUT_POINTER_SOURCE_ID)
                );
                assert_eq!(read_u16(order, &reply[8..10]), u16::from(minor));
                assert_eq!(reply[10], X_INPUT_MAJOR_OPCODE);
            }
        }
    }
}
