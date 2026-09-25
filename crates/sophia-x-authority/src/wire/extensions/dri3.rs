fn decode_dri3(context: XWireClientContext, bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_DRI3_QUERY_VERSION_MINOR_OPCODE => decode_extension_query_version(
            context,
            bytes,
            X_DRI3_MAJOR_OPCODE,
            X_DRI3_QUERY_VERSION_MINOR_OPCODE,
            |major_version, minor_version| XWireRequest::Dri3(crate::XDri3Request::Dri3QueryVersion {
                major_version,
                minor_version,
            }),
        ),
        X_DRI3_OPEN_MINOR_OPCODE => {
            require_exact_len(X_DRI3_MAJOR_OPCODE, 12, bytes.len())?;
            Ok(XWireRequest::Dri3(crate::XDri3Request::Dri3Open {
                drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                provider: context.byte_order.u32(&bytes[8..12]),
            }))
        }
        X_DRI3_PIXMAP_FROM_BUFFER_MINOR_OPCODE => {
            require_exact_len(X_DRI3_MAJOR_OPCODE, 24, bytes.len())?;
            let pixmap = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(pixmap)?;
            Ok(XWireRequest::Dri3(crate::XDri3Request::Dri3PixmapFromBuffer {
                pixmap: XResourceId::new(u64::from(pixmap), 1),
                drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                size_bytes: context.byte_order.u32(&bytes[12..16]),
                width: context.byte_order.u16(&bytes[16..18]),
                height: context.byte_order.u16(&bytes[18..20]),
                stride: context.byte_order.u16(&bytes[20..22]),
                depth: bytes[22],
                bits_per_pixel: bytes[23],
            }))
        }
        X_DRI3_FENCE_FROM_FD_MINOR_OPCODE => {
            require_exact_len(X_DRI3_MAJOR_OPCODE, 16, bytes.len())?;
            let fence = context.byte_order.u32(&bytes[8..12]);
            context.validate_new_resource_id(fence)?;
            Ok(XWireRequest::Dri3(crate::XDri3Request::Dri3FenceFromFd {
                drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                fence: XResourceId::new(u64::from(fence), 1),
                initially_triggered: bytes[12] != 0,
            }))
        }
        X_DRI3_SET_DRM_DEVICE_IN_USE_MINOR_OPCODE => {
            require_exact_len(X_DRI3_MAJOR_OPCODE, 16, bytes.len())?;
            Ok(XWireRequest::Dri3(crate::XDri3Request::Dri3SetDrmDeviceInUse {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                major: context.byte_order.u32(&bytes[8..12]),
                minor: context.byte_order.u32(&bytes[12..16]),
            }))
        }
        X_DRI3_GET_SUPPORTED_MODIFIERS_MINOR_OPCODE => {
            require_exact_len(X_DRI3_MAJOR_OPCODE, 12, bytes.len())?;
            Ok(XWireRequest::Dri3(crate::XDri3Request::Dri3GetSupportedModifiers {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                depth: bytes[8],
                bits_per_pixel: bytes[9],
            }))
        }
        X_DRI3_PIXMAP_FROM_BUFFERS_MINOR_OPCODE => {
            require_exact_len(X_DRI3_MAJOR_OPCODE, 64, bytes.len())?;
            let pixmap = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(pixmap)?;
            let num_buffers = bytes[12];
            if num_buffers == 0 || usize::from(num_buffers) > sophia_protocol::DMA_BUF_MAX_PLANES {
                return Err(XWireParseError::InvalidValue(u32::from(num_buffers)));
            }
            Ok(XWireRequest::Dri3(crate::XDri3Request::Dri3PixmapFromBuffers {
                pixmap: XResourceId::new(u64::from(pixmap), 1),
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                num_buffers,
                width: context.byte_order.u16(&bytes[16..18]),
                height: context.byte_order.u16(&bytes[18..20]),
                strides: [
                    context.byte_order.u32(&bytes[20..24]),
                    context.byte_order.u32(&bytes[28..32]),
                    context.byte_order.u32(&bytes[36..40]),
                    context.byte_order.u32(&bytes[44..48]),
                ],
                offsets: [
                    context.byte_order.u32(&bytes[24..28]),
                    context.byte_order.u32(&bytes[32..36]),
                    context.byte_order.u32(&bytes[40..44]),
                    context.byte_order.u32(&bytes[48..52]),
                ],
                depth: bytes[52],
                bits_per_pixel: bytes[53],
                modifier: context.byte_order.u64(&bytes[56..64]),
            }))
        }
        X_DRI3_BUFFER_FROM_PIXMAP_MINOR_OPCODE => {
            require_exact_len(X_DRI3_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Dri3(crate::XDri3Request::Dri3BufferFromPixmap {
                pixmap: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_DRI3_BUFFERS_FROM_PIXMAP_MINOR_OPCODE => {
            require_exact_len(X_DRI3_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Dri3(crate::XDri3Request::Dri3BuffersFromPixmap {
                pixmap: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        // Sophia answers the DRI3 minors it implements and refuses the rest as
        // an implementation gap the client can see. Refusing to parse would
        // deny the client a sequence number to attribute the failure to.
        minor => Ok(XWireRequest::Dri3(crate::XDri3Request::Dri3Unimplemented {
            minor_opcode: minor,
        })),
    }
}

