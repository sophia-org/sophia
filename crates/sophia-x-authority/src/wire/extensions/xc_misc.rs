fn decode_xc_misc(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_XC_MISC_GET_VERSION_MINOR_OPCODE => {
            require_exact_len(
                X_XC_MISC_MAJOR_OPCODE,
                X_XC_MISC_GET_VERSION_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Extension(crate::XExtensionRequest::XCMiscGetVersion {
                major: context.byte_order.u16(&bytes[4..6]),
                minor: context.byte_order.u16(&bytes[6..8]),
            }))
        }
        X_XC_MISC_GET_XID_RANGE_MINOR_OPCODE => {
            require_exact_len(
                X_XC_MISC_MAJOR_OPCODE,
                X_XC_MISC_GET_XID_RANGE_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Extension(crate::XExtensionRequest::XCMiscGetXIDRange))
        }
        X_XC_MISC_GET_XID_LIST_MINOR_OPCODE => {
            require_exact_len(
                X_XC_MISC_MAJOR_OPCODE,
                X_XC_MISC_GET_XID_LIST_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Extension(crate::XExtensionRequest::XCMiscGetXIDList {
                count: context.byte_order.u32(&bytes[4..8]),
            }))
        }
        // Three requests is the whole extension, so anything else is a client
        // error rather than surface this server declined to implement.
        minor_opcode => Err(XWireParseError::UnknownOpcode(minor_opcode)),
    }
}
