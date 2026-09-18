mod ordered_codec_tests {
    use super::*;
    #[cfg(unix)]
    fn legacy_device_event(
        byte_order: XByteOrder,
        sequence: u16,
        event_type: u16,
        event: XAuthorityInputEvent,
        event_window: XResourceId,
        child_window: XResourceId,
        event_x: i16,
        event_y: i16,
        flags: u32,
    ) -> Vec<u8> {
        let (device, time, detail, root_x, root_y, state) = match event {
            XAuthorityInputEvent::Key(key) => {
                (3, key.time_msec, u32::from(key.keycode), 0, 0, key.state)
            }
            XAuthorityInputEvent::Pointer(pointer) => (
                2,
                pointer.time_msec,
                match pointer.kind {
                    XAuthorityPointerEventKind::Button { button, .. } => u32::from(button),
                    XAuthorityPointerEventKind::Axis { button, .. }
                        if matches!(event_type, 4 | 5) =>
                    {
                        u32::from(button)
                    }
                    XAuthorityPointerEventKind::Axis { .. } => 0,
                    XAuthorityPointerEventKind::Motion => 0,
                },
                pointer.root_x,
                pointer.root_y,
                pointer.state,
            ),
        };
        let mut out = vec![0; 80];
        out[0] = 35;
        out[1] = crate::X_INPUT_MAJOR_OPCODE;
        write_xi_u16(byte_order, &mut out[2..4], sequence);
        write_xi_u16(byte_order, &mut out[8..10], event_type);
        write_xi_u16(byte_order, &mut out[10..12], device);
        write_xi_u32(byte_order, &mut out[12..16], time);
        write_xi_u32(byte_order, &mut out[16..20], detail);
        write_xi_u32(byte_order, &mut out[20..24], X_SETUP_DEFAULT_ROOT);
        write_xi_u32(
            byte_order,
            &mut out[24..28],
            u32::try_from(event_window.local.raw()).unwrap_or(0),
        );
        write_xi_u32(
            byte_order,
            &mut out[28..32],
            u32::try_from(child_window.local.raw()).unwrap_or(0),
        );
        write_xi_u32(
            byte_order,
            &mut out[32..36],
            (i32::from(root_x) << 16) as u32,
        );
        write_xi_u32(
            byte_order,
            &mut out[36..40],
            (i32::from(root_y) << 16) as u32,
        );
        write_xi_u32(
            byte_order,
            &mut out[40..44],
            (i32::from(event_x) << 16) as u32,
        );
        write_xi_u32(
            byte_order,
            &mut out[44..48],
            (i32::from(event_y) << 16) as u32,
        );
        write_xi_u16(
            byte_order,
            &mut out[52..54],
            if device == 2 {
                crate::X_INPUT_POINTER_SOURCE_ID
            } else {
                device
            },
        );
        write_xi_u32(byte_order, &mut out[56..60], flags);
        write_xi_u32(byte_order, &mut out[72..76], u32::from(state & 0xff));
        let buttons = (1_u8..=5).fold(0_u32, |buttons, button| {
            let core_mask = 1_u16 << (u32::from(button) + 7);
            if state & core_mask != 0 {
                buttons | (1_u32 << button)
            } else {
                buttons
            }
        });
        if buttons != 0 {
            write_xi_u16(byte_order, &mut out[48..50], 1);
            match byte_order {
                XByteOrder::LittleEndian => out.extend_from_slice(&buttons.to_le_bytes()),
                XByteOrder::BigEndian => out.extend_from_slice(&buttons.to_be_bytes()),
            }
        }
        if let XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
            kind:
                XAuthorityPointerEventKind::Axis {
                    horizontal_position_v120,
                    vertical_position_v120,
                    ..
                },
            ..
        }) = event
            && event_type == 6
            && (horizontal_position_v120.is_some() || vertical_position_v120.is_some())
        {
            write_xi_u16(byte_order, &mut out[50..52], 1);
            let mut mask = 0u8;
            if horizontal_position_v120.is_some() {
                mask |= 1u8 << u32::from(crate::X_POINTER_HORIZONTAL_SCROLL_VALUATOR);
            }
            if vertical_position_v120.is_some() {
                mask |= 1u8 << u32::from(crate::X_POINTER_VERTICAL_SCROLL_VALUATOR);
            }
            out.extend_from_slice(&[mask, 0, 0, 0]);
            for position in [horizontal_position_v120, vertical_position_v120]
                .into_iter()
                .flatten()
            {
                crate::client_output::push_xi_fp3232(
                    byte_order,
                    &mut out,
                    i64::from(position) << 32,
                );
            }
        }
        let length = u32::try_from((out.len() - 32) / 4).unwrap_or(u32::MAX);
        write_xi_u32(byte_order, &mut out[4..8], length);
        out
    }

    #[cfg(unix)]
    fn legacy_crossing_event(
        byte_order: XByteOrder,
        sequence: u16,
        event_type: u16,
        event: XAuthorityInputEvent,
        event_window: XResourceId,
    ) -> Vec<u8> {
        let (device, time, root_x, root_y, event_x, event_y, state) = match event {
            XAuthorityInputEvent::Key(key) => (3, key.time_msec, 0, 0, 0, 0, key.state),
            XAuthorityInputEvent::Pointer(pointer) => (
                2,
                pointer.time_msec,
                pointer.root_x,
                pointer.root_y,
                pointer.event_x,
                pointer.event_y,
                pointer.state,
            ),
        };
        let mut out = vec![0; 72];
        out[0] = 35;
        out[1] = crate::X_INPUT_MAJOR_OPCODE;
        write_xi_u16(byte_order, &mut out[2..4], sequence);
        write_xi_u32(byte_order, &mut out[4..8], 10);
        write_xi_u16(byte_order, &mut out[8..10], event_type);
        write_xi_u16(byte_order, &mut out[10..12], device);
        write_xi_u32(byte_order, &mut out[12..16], time);
        write_xi_u16(
            byte_order,
            &mut out[16..18],
            if device == 2 {
                crate::X_INPUT_POINTER_SOURCE_ID
            } else {
                device
            },
        );
        out[18] = 0;
        out[19] = 3;
        write_xi_u32(byte_order, &mut out[20..24], X_SETUP_DEFAULT_ROOT);
        write_xi_u32(
            byte_order,
            &mut out[24..28],
            u32::try_from(event_window.local.raw()).unwrap_or(0),
        );
        write_xi_u32(
            byte_order,
            &mut out[32..36],
            (i32::from(root_x) << 16) as u32,
        );
        write_xi_u32(
            byte_order,
            &mut out[36..40],
            (i32::from(root_y) << 16) as u32,
        );
        write_xi_u32(
            byte_order,
            &mut out[40..44],
            (i32::from(event_x) << 16) as u32,
        );
        write_xi_u32(
            byte_order,
            &mut out[44..48],
            (i32::from(event_y) << 16) as u32,
        );
        out[48] = 1;
        out[49] = 1;
        write_xi_u32(byte_order, &mut out[64..68], u32::from(state & 0xff));
        out
    }

    #[test]
    fn fixed_encoder_preserves_legacy_bytes_for_all_current_device_forms_and_orders() {
        let pointer = XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Motion,
            surface: SurfaceId::new(1, 1),
            root_x: -100,
            root_y: 210,
            event_x: 3,
            event_y: -9,
            state: 0x1f07,
            time_msec: 771,
        };
        let mut cases = vec![
            XAuthorityInputEvent::Key(XAuthorityKeyEvent {
                keycode: 38,
                pressed: true,
                state: 5,
                modifiers_after: 1,
                time_msec: 123,
            }),
            XAuthorityInputEvent::Pointer(pointer),
        ];
        for pressed in [false, true] {
            for button in [1, 5, 8, 9] {
                cases.push(XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                    kind: XAuthorityPointerEventKind::Button { button, pressed },
                    ..pointer
                }));
            }
            for (horizontal, vertical) in [
                (None, None),
                (Some(-120), None),
                (None, Some(i32::MAX)),
                (Some(i32::MIN), Some(15)),
            ] {
                cases.push(XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                    kind: XAuthorityPointerEventKind::Axis {
                        button: 4,
                        pressed,
                        horizontal_position_v120: horizontal,
                        vertical_position_v120: vertical,
                    },
                    ..pointer
                }));
            }
        }
        let mut maximum = 0;
        for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            for event in cases.iter().copied() {
                for event_type in [2, 3, 4, 5, 6] {
                    for flags in [0, XI_POINTER_EMULATED] {
                        let window = XResourceId::new(0x200003, 1);
                        let child = XResourceId::new(0x200004, 1);
                        let old = legacy_device_event(
                            order, 0x4321, event_type, event, window, child, -12, 83, flags,
                        );
                        let fixed = encode_xi_device_frame(
                            order, 0x4321, event_type, event, window, child, -12, 83, flags,
                        );
                        assert_eq!(old, fixed.as_bytes());
                        assert_eq!(
                            encode_xi_device_event(
                                order, 0x4321, event_type, event, window, child, -12, 83, flags
                            ),
                            old
                        );
                        maximum = maximum.max(fixed.as_bytes().len());
                    }
                }
                for event_type in [7, 8, 9, 10] {
                    let window = XResourceId::new(0x200003, 1);
                    let old = legacy_crossing_event(order, 0x4321, event_type, event, window);
                    assert_eq!(
                        encode_xi_crossing_frame(order, 0x4321, event_type, event, window)
                            .as_bytes(),
                        old
                    );
                    assert_eq!(
                        encode_xi_crossing_event(order, 0x4321, event_type, event, window),
                        old
                    );
                }
            }
        }
        assert_eq!(maximum, 104);
    }
}
