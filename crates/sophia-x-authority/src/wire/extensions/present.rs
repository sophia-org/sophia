fn decode_present(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_PRESENT_QUERY_VERSION_MINOR_OPCODE => decode_extension_query_version(
            context,
            bytes,
            X_PRESENT_MAJOR_OPCODE,
            X_PRESENT_QUERY_VERSION_MINOR_OPCODE,
            |major_version, minor_version| XWireRequest::Present(crate::XPresentRequest::PresentQueryVersion {
                major_version,
                minor_version,
            }),
        ),
        X_PRESENT_PIXMAP_MINOR_OPCODE => {
            require_len(X_PRESENT_MAJOR_OPCODE, 72, bytes.len())?;
            if !(bytes.len() - 72).is_multiple_of(8) {
                return Err(XWireParseError::InvalidLength {
                    opcode: X_PRESENT_MAJOR_OPCODE,
                    expected_at_least: 72,
                    actual: bytes.len(),
                });
            }
            let raw_resource = |offset: usize| context.byte_order.u32(&bytes[offset..offset + 4]);
            let resource = |offset: usize| XResourceId::new(u64::from(raw_resource(offset)), 1);
            let optional_resource = |offset: usize| {
                let raw = raw_resource(offset);
                (raw != 0).then(|| XResourceId::new(u64::from(raw), 1))
            };
            let notifies = bytes[72..]
                .chunks_exact(8)
                .map(|notify| {
                    (
                        XResourceId::new(u64::from(context.byte_order.u32(&notify[..4])), 1),
                        context.byte_order.u32(&notify[4..]),
                    )
                })
                .collect();
            Ok(XWireRequest::Present(crate::XPresentRequest::PresentPixmap {
                transaction: context.transaction,
                window: resource(4),
                pixmap: resource(8),
                serial: raw_resource(12),
                valid_region: raw_resource(16),
                update_region: raw_resource(20),
                x_offset: context.byte_order.i16(&bytes[24..26]),
                y_offset: context.byte_order.i16(&bytes[26..28]),
                target_crtc: raw_resource(28),
                wait_fence: optional_resource(32),
                idle_fence: optional_resource(36),
                options: raw_resource(40),
                target_msc: context.byte_order.u64(&bytes[48..56]),
                divisor: context.byte_order.u64(&bytes[56..64]),
                remainder: context.byte_order.u64(&bytes[64..72]),
                notifies,
            }))
        }
        X_PRESENT_SELECT_INPUT_MINOR_OPCODE => {
            require_exact_len(X_PRESENT_MAJOR_OPCODE, 16, bytes.len())?;
            let event_id = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(event_id)?;
            Ok(XWireRequest::Present(crate::XPresentRequest::PresentSelectInput {
                event_id: XResourceId::new(u64::from(event_id), 1),
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                event_mask: context.byte_order.u32(&bytes[12..16]),
            }))
        }
        X_PRESENT_QUERY_CAPABILITIES_MINOR_OPCODE => {
            require_exact_len(X_PRESENT_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Present(crate::XPresentRequest::PresentQueryCapabilities {
                target: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_PRESENT_NOTIFY_MSC_MINOR_OPCODE => {
            require_exact_len(X_PRESENT_MAJOR_OPCODE, 40, bytes.len())?;
            Ok(XWireRequest::Present(crate::XPresentRequest::PresentNotifyMsc {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                serial: context.byte_order.u32(&bytes[8..12]),
                target_msc: context.byte_order.u64(&bytes[16..24]),
                divisor: context.byte_order.u64(&bytes[24..32]),
                remainder: context.byte_order.u64(&bytes[32..40]),
            }))
        }
        // Sophia answers the Present minors it implements and refuses the rest
        // as an implementation gap the client can see. Refusing to parse would
        // deny the client a sequence number to attribute the failure to.
        minor => Ok(XWireRequest::Present(crate::XPresentRequest::PresentUnimplemented {
            minor_opcode: minor,
        })),
    }
}

