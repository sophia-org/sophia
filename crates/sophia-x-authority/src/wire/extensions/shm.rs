fn decode_mit_shm(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_MIT_SHM_QUERY_VERSION_MINOR_OPCODE => {
            require_exact_len(
                X_MIT_SHM_MAJOR_OPCODE,
                X_MIT_SHM_QUERY_VERSION_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Shm(crate::XShmRequest::ShmQueryVersion))
        }
        X_MIT_SHM_ATTACH_MINOR_OPCODE => {
            require_exact_len(
                X_MIT_SHM_MAJOR_OPCODE,
                X_MIT_SHM_ATTACH_REQ_LEN,
                bytes.len(),
            )?;
            let segment = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(segment)?;
            Ok(XWireRequest::Shm(crate::XShmRequest::ShmAttach {
                segment: XResourceId::new(u64::from(segment), 1),
                shmid: context.byte_order.u32(&bytes[8..12]),
                read_only: bytes[12] != 0,
            }))
        }
        X_MIT_SHM_ATTACH_FD_MINOR_OPCODE => {
            require_exact_len(
                X_MIT_SHM_MAJOR_OPCODE,
                X_MIT_SHM_ATTACH_FD_REQ_LEN,
                bytes.len(),
            )?;
            let segment = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(segment)?;
            Ok(XWireRequest::Shm(crate::XShmRequest::ShmAttachFd {
                segment: XResourceId::new(u64::from(segment), 1),
                read_only: bytes[8] != 0,
            }))
        }
        X_MIT_SHM_CREATE_SEGMENT_MINOR_OPCODE => {
            require_exact_len(
                X_MIT_SHM_MAJOR_OPCODE,
                X_MIT_SHM_CREATE_SEGMENT_REQ_LEN,
                bytes.len(),
            )?;
            let segment = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(segment)?;
            Ok(XWireRequest::Shm(crate::XShmRequest::ShmCreateSegment {
                segment: XResourceId::new(u64::from(segment), 1),
                size: context.byte_order.u32(&bytes[8..12]),
                read_only: bytes[12] != 0,
            }))
        }
        X_MIT_SHM_DETACH_MINOR_OPCODE => {
            require_exact_len(
                X_MIT_SHM_MAJOR_OPCODE,
                X_MIT_SHM_DETACH_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Shm(crate::XShmRequest::ShmDetach {
                segment: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_MIT_SHM_PUT_IMAGE_MINOR_OPCODE => {
            require_exact_len(
                X_MIT_SHM_MAJOR_OPCODE,
                X_MIT_SHM_PUT_IMAGE_REQ_LEN,
                bytes.len(),
            )?;
            validate_wire_image_format(bytes[29])?;
            Ok(XWireRequest::Shm(crate::XShmRequest::ShmPutImage {
                drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                gc: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                total_width: context.byte_order.u16(&bytes[12..14]),
                total_height: context.byte_order.u16(&bytes[14..16]),
                src_x: context.byte_order.u16(&bytes[16..18]),
                src_y: context.byte_order.u16(&bytes[18..20]),
                src_width: context.byte_order.u16(&bytes[20..22]),
                src_height: context.byte_order.u16(&bytes[22..24]),
                dst_x: context.byte_order.i16(&bytes[24..26]),
                dst_y: context.byte_order.i16(&bytes[26..28]),
                depth: bytes[28],
                format: bytes[29],
                send_event: bytes[30] != 0,
                segment: XResourceId::new(u64::from(context.byte_order.u32(&bytes[32..36])), 1),
                offset: context.byte_order.u32(&bytes[36..40]),
            }))
        }
        X_MIT_SHM_GET_IMAGE_MINOR_OPCODE => {
            require_exact_len(
                X_MIT_SHM_MAJOR_OPCODE,
                X_MIT_SHM_GET_IMAGE_REQ_LEN,
                bytes.len(),
            )?;
            validate_wire_image_format(bytes[20])?;
            Ok(XWireRequest::Shm(crate::XShmRequest::ShmGetImage {
                drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                x: context.byte_order.i16(&bytes[8..10]),
                y: context.byte_order.i16(&bytes[10..12]),
                width: context.byte_order.u16(&bytes[12..14]),
                height: context.byte_order.u16(&bytes[14..16]),
                plane_mask: context.byte_order.u32(&bytes[16..20]),
                format: bytes[20],
                segment: XResourceId::new(u64::from(context.byte_order.u32(&bytes[24..28])), 1),
                offset: context.byte_order.u32(&bytes[28..32]),
            }))
        }
        X_MIT_SHM_CREATE_PIXMAP_MINOR_OPCODE => {
            require_exact_len(
                X_MIT_SHM_MAJOR_OPCODE,
                X_MIT_SHM_CREATE_PIXMAP_REQ_LEN,
                bytes.len(),
            )?;
            let pixmap = context.byte_order.u32(&bytes[4..8]);
            context.validate_new_resource_id(pixmap)?;
            Ok(XWireRequest::Shm(crate::XShmRequest::ShmCreatePixmap {
                pixmap: XResourceId::new(u64::from(pixmap), 1),
                drawable: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                width: context.byte_order.u16(&bytes[12..14]),
                height: context.byte_order.u16(&bytes[14..16]),
                depth: bytes[16],
                segment: XResourceId::new(u64::from(context.byte_order.u32(&bytes[20..24])), 1),
                offset: context.byte_order.u32(&bytes[24..28]),
            }))
        }
        _ => Err(XWireParseError::UnknownOpcode(bytes[0])),
    }
}
