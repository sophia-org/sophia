fn decode_x_keyboard(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_KEYBOARD_USE_EXTENSION_MINOR_OPCODE => {
            require_exact_len(
                X_KEYBOARD_MAJOR_OPCODE,
                X_KEYBOARD_USE_EXTENSION_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbUseExtension {
                wanted_major: context.byte_order.u16(&bytes[4..6]),
                wanted_minor: context.byte_order.u16(&bytes[6..8]),
            }))
        }
        X_KEYBOARD_LATCH_LOCK_STATE_MINOR_OPCODE => {
            require_exact_len(
                X_KEYBOARD_MAJOR_OPCODE,
                X_KEYBOARD_LATCH_LOCK_STATE_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbLatchLockState {
                affect_mod_locks: bytes[6],
                mod_locks: bytes[7],
                lock_group: bytes[8] != 0,
                group_lock: bytes[9],
                affect_mod_latches: bytes[10],
                mod_latches: bytes[11],
                latch_group: bytes[13] != 0,
                group_latch: context.byte_order.u16(&bytes[14..16]),
            }))
        }
        X_KEYBOARD_GET_MAP_MINOR_OPCODE => {
            require_exact_len(
                X_KEYBOARD_MAJOR_OPCODE,
                X_KEYBOARD_GET_MAP_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbGetMap {
                full: context.byte_order.u16(&bytes[6..8]),
                partial: context.byte_order.u16(&bytes[8..10]),
            }))
        }
        X_KEYBOARD_GET_COMPAT_MAP_MINOR_OPCODE => {
            require_exact_len(X_KEYBOARD_MAJOR_OPCODE, 12, bytes.len())?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbGetCompatMap {
                device_spec: context.byte_order.u16(&bytes[4..6]),
            }))
        }
        X_KEYBOARD_GET_INDICATOR_MAP_MINOR_OPCODE => {
            require_exact_len(X_KEYBOARD_MAJOR_OPCODE, 12, bytes.len())?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbGetIndicatorMap {
                device_spec: context.byte_order.u16(&bytes[4..6]),
            }))
        }
        X_KEYBOARD_GET_STATE_MINOR_OPCODE => {
            require_exact_len(X_KEYBOARD_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbGetState))
        }
        X_KEYBOARD_GET_CONTROLS_MINOR_OPCODE => {
            require_exact_len(
                X_KEYBOARD_MAJOR_OPCODE,
                X_KEYBOARD_GET_CONTROLS_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbGetControls))
        }
        X_KEYBOARD_GET_NAMES_MINOR_OPCODE => {
            require_exact_len(X_KEYBOARD_MAJOR_OPCODE, 12, bytes.len())?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbGetNames {
                which: context.byte_order.u32(&bytes[8..12]),
            }))
        }
        X_KEYBOARD_GET_DEVICE_INFO_MINOR_OPCODE => {
            require_exact_len(X_KEYBOARD_MAJOR_OPCODE, 16, bytes.len())?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbGetDeviceInfo {
                device_spec: context.byte_order.u16(&bytes[4..6]),
                wanted: context.byte_order.u16(&bytes[6..8]),
            }))
        }
        X_KEYBOARD_SELECT_EVENTS_MINOR_OPCODE => {
            require_len(
                X_KEYBOARD_MAJOR_OPCODE,
                X_KEYBOARD_SELECT_EVENTS_REQ_LEN,
                bytes.len(),
            )?;
            let affect_which = context.byte_order.u16(&bytes[6..8]);
            let state_details = if affect_which & 4 != 0 {
                let offset = 16 + if affect_which & 1 != 0 { 4 } else { 0 };
                (bytes.len() >= offset + 4).then(|| {
                    (
                        context.byte_order.u16(&bytes[offset..offset + 2]),
                        context.byte_order.u16(&bytes[offset + 2..offset + 4]),
                    )
                })
            } else {
                None
            };
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbSelectEvents {
                affect_which,
                clear: context.byte_order.u16(&bytes[8..10]),
                select_all: context.byte_order.u16(&bytes[10..12]),
                state_details,
            }))
        }
        X_KEYBOARD_PER_CLIENT_FLAGS_MINOR_OPCODE => {
            require_exact_len(
                X_KEYBOARD_MAJOR_OPCODE,
                X_KEYBOARD_PER_CLIENT_FLAGS_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Xkb(crate::XkbRequest::XkbPerClientFlags {
                change: context.byte_order.u32(&bytes[8..12]),
                value: context.byte_order.u32(&bytes[12..16]),
            }))
        }
        _ => Err(XWireParseError::UnknownOpcode(bytes[0])),
    }
}

