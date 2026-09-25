fn decode_x_input(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_INPUT_LIST_INPUT_DEVICES_MINOR_OPCODE => {
            require_exact_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_LIST_INPUT_DEVICES_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiListInputDevices))
        }
        X_INPUT_DEVICE_BELL_MINOR_OPCODE => {
            require_exact_len(X_INPUT_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiDeviceBell))
        }
        X_INPUT_QUERY_POINTER_MINOR_OPCODE => {
            require_exact_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_QUERY_POINTER_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiQueryPointer {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                device_id: context.byte_order.u16(&bytes[8..10]),
            }))
        }
        X_INPUT_CHANGE_CURSOR_MINOR_OPCODE => {
            require_exact_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_CHANGE_CURSOR_REQ_LEN,
                bytes.len(),
            )?;
            let cursor = context.byte_order.u32(&bytes[8..12]);
            Ok(XWireRequest::Xi(crate::XInputRequest::XiChangeCursor {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                cursor: (cursor != 0).then(|| XResourceId::new(u64::from(cursor), 1)),
            }))
        }
        X_INPUT_GET_CLIENT_POINTER_MINOR_OPCODE => {
            require_exact_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_GET_CLIENT_POINTER_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiGetClientPointer))
        }
        X_INPUT_UNGRAB_DEVICE_MINOR_OPCODE => {
            require_exact_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_UNGRAB_DEVICE_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiUngrabDevice {
                device_id: context.byte_order.u16(&bytes[8..10]),
                time: context.byte_order.u32(&bytes[4..8]),
            }))
        }
        X_INPUT_GRAB_DEVICE_MINOR_OPCODE => {
            require_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_GRAB_DEVICE_REQ_LEN,
                bytes.len(),
            )?;
            let words = usize::from(context.byte_order.u16(&bytes[22..24]));
            if words > 8 {
                return Err(XWireParseError::InvalidValue(words as u32));
            }
            let expected = X_INPUT_GRAB_DEVICE_REQ_LEN.saturating_add(words.saturating_mul(4));
            require_exact_len(X_INPUT_MAJOR_OPCODE, expected, bytes.len())?;
            let cursor = context.byte_order.u32(&bytes[12..16]);
            Ok(XWireRequest::Xi(crate::XInputRequest::XiGrabDevice {
                window: XResourceId::new(
                    u64::from(context.byte_order.u32(&bytes[4..8])),
                    1,
                ),
                time: context.byte_order.u32(&bytes[8..12]),
                cursor: (cursor != 0).then(|| XResourceId::new(u64::from(cursor), 1)),
                device_id: context.byte_order.u16(&bytes[16..18]),
                pointer_mode: bytes[18],
                keyboard_mode: bytes[19],
                owner_events: bytes[20] != 0,
                event_mask: bytes[24..]
                    .chunks_exact(4)
                    .map(|word| context.byte_order.u32(word))
                    .collect(),
            }))
        }
        X_INPUT_GET_EXTENSION_VERSION_MINOR_OPCODE => {
            require_len(X_INPUT_MAJOR_OPCODE, 8, bytes.len())?;
            let name_len = usize::from(context.byte_order.u16(&bytes[4..6]));
            let expected = 8usize.saturating_add(padded_len(name_len));
            require_exact_len(X_INPUT_MAJOR_OPCODE, expected, bytes.len())?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiGetExtensionVersion))
        }
        X_INPUT_QUERY_VERSION_MINOR_OPCODE => {
            require_exact_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_QUERY_VERSION_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiQueryVersion {
                major_version: context.byte_order.u16(&bytes[4..6]),
                minor_version: context.byte_order.u16(&bytes[6..8]),
            }))
        }
        X_INPUT_QUERY_DEVICE_MINOR_OPCODE => {
            require_exact_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_QUERY_DEVICE_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiQueryDevice {
                device_id: context.byte_order.u16(&bytes[4..6]),
            }))
        }
        X_INPUT_GET_FOCUS_MINOR_OPCODE => {
            require_exact_len(X_INPUT_MAJOR_OPCODE, X_INPUT_GET_FOCUS_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiGetFocus {
                device_id: context.byte_order.u16(&bytes[4..6]),
            }))
        }
        X_INPUT_GET_PROPERTY_MINOR_OPCODE => {
            require_exact_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_GET_PROPERTY_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xi(crate::XInputRequest::XiGetProperty))
        }
        X_INPUT_SELECT_EVENTS_MINOR_OPCODE => {
            require_len(
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_SELECT_EVENTS_REQ_LEN,
                bytes.len(),
            )?;
            let window = XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1);
            let count = usize::from(context.byte_order.u16(&bytes[8..10]));
            if count > 16 {
                return Err(XWireParseError::InvalidValue(count as u32));
            }
            let mut offset = X_INPUT_SELECT_EVENTS_REQ_LEN;
            let mut masks = Vec::with_capacity(count);
            for _ in 0..count {
                if offset.checked_add(4).is_none_or(|end| end > bytes.len()) {
                    return Err(XWireParseError::InvalidLength {
                        opcode: X_INPUT_MAJOR_OPCODE,
                        expected_at_least: offset.saturating_add(4),
                        actual: bytes.len(),
                    });
                }
                let device_id = context.byte_order.u16(&bytes[offset..offset + 2]);
                let words = usize::from(context.byte_order.u16(&bytes[offset + 2..offset + 4]));
                if words > 8 {
                    return Err(XWireParseError::InvalidValue(words as u32));
                }
                offset += 4;
                let end = offset.saturating_add(words.saturating_mul(4));
                if end > bytes.len() {
                    return Err(XWireParseError::InvalidLength {
                        opcode: X_INPUT_MAJOR_OPCODE,
                        expected_at_least: end,
                        actual: bytes.len(),
                    });
                }
                let mask = bytes[offset..end]
                    .chunks_exact(4)
                    .map(|word| context.byte_order.u32(word))
                    .collect();
                masks.push((device_id, mask));
                offset = end;
            }
            if offset != bytes.len() {
                return Err(XWireParseError::TrailingBytes(bytes.len() - offset));
            }
            Ok(XWireRequest::Xi(crate::XInputRequest::XiSelectEvents { window, masks }))
        }
        _ => Err(XWireParseError::UnknownOpcode(bytes[0])),
    }
}
