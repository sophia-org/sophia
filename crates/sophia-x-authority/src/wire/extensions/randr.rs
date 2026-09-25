fn decode_randr(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_RANDR_QUERY_VERSION_MINOR_OPCODE => {
            require_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_QUERY_VERSION_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrQueryVersion {
                major_version: context.byte_order.u32(&bytes[4..8]),
                minor_version: context.byte_order.u32(&bytes[8..12]),
            }))
        }
        X_RANDR_SELECT_INPUT_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_SELECT_INPUT_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrSelectInput {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                enable: context.byte_order.u16(&bytes[8..10]),
            }))
        }
        X_RANDR_GET_SCREEN_SIZE_RANGE_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_SCREEN_SIZE_RANGE_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetScreenSizeRange {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_RANDR_GET_SCREEN_RESOURCES_MINOR_OPCODE
        | X_RANDR_GET_SCREEN_RESOURCES_CURRENT_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_SCREEN_RESOURCES_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetScreenResources {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                current: bytes[1] == X_RANDR_GET_SCREEN_RESOURCES_CURRENT_MINOR_OPCODE,
            }))
        }
        X_RANDR_GET_OUTPUT_INFO_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_OUTPUT_INFO_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetOutputInfo {
                output: context.byte_order.u32(&bytes[4..8]),
                config_timestamp: context.byte_order.u32(&bytes[8..12]),
            }))
        }
        X_RANDR_GET_OUTPUT_PROPERTY_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_OUTPUT_PROPERTY_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetOutputProperty {
                output: context.byte_order.u32(&bytes[4..8]),
                property: context.byte_order.u32(&bytes[8..12]),
                property_type: context.byte_order.u32(&bytes[12..16]),
                long_offset: context.byte_order.u32(&bytes[16..20]),
                long_length: context.byte_order.u32(&bytes[20..24]),
                delete: bytes[24] != 0,
                pending: bytes[25] != 0,
            }))
        }
        X_RANDR_GET_CRTC_INFO_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_CRTC_INFO_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcInfo {
                crtc: context.byte_order.u32(&bytes[4..8]),
                config_timestamp: context.byte_order.u32(&bytes[8..12]),
            }))
        }
        X_RANDR_GET_CRTC_GAMMA_SIZE_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_CRTC_GAMMA_SIZE_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcGammaSize {
                crtc: context.byte_order.u32(&bytes[4..8]),
            }))
        }
        X_RANDR_GET_CRTC_GAMMA_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_CRTC_GAMMA_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcGamma {
                crtc: context.byte_order.u32(&bytes[4..8]),
            }))
        }
        X_RANDR_GET_CRTC_TRANSFORM_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_CRTC_TRANSFORM_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetCrtcTransform {
                crtc: context.byte_order.u32(&bytes[4..8]),
            }))
        }
        X_RANDR_GET_PANNING_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_PANNING_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetPanning {
                crtc: context.byte_order.u32(&bytes[4..8]),
            }))
        }
        X_RANDR_GET_OUTPUT_PRIMARY_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_OUTPUT_PRIMARY_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetOutputPrimary {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_RANDR_GET_PROVIDERS_MINOR_OPCODE => {
            require_exact_len(X_RANDR_MAJOR_OPCODE, 8, bytes.len())?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetProviders {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_RANDR_GET_MONITORS_MINOR_OPCODE => {
            require_exact_len(
                X_RANDR_MAJOR_OPCODE,
                X_RANDR_GET_MONITORS_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Randr(crate::XRandrRequest::RandrGetMonitors {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                get_active: bytes[8] != 0,
            }))
        }
        _ => Err(XWireParseError::UnknownOpcode(bytes[0])),
    }
}
