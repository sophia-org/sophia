fn decode_xf86_vidmode(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_XF86_VIDMODE_QUERY_VERSION_MINOR_OPCODE => {
            require_exact_len(
                X_XF86_VIDMODE_MAJOR_OPCODE,
                X_XF86_VIDMODE_QUERY_VERSION_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeQueryVersion))
        }
        X_XF86_VIDMODE_GET_MODE_LINE_MINOR_OPCODE => {
            require_exact_len(
                X_XF86_VIDMODE_MAJOR_OPCODE,
                X_XF86_VIDMODE_GET_MODE_LINE_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeGetModeLine {
                screen: context.byte_order.u16(&bytes[4..6]),
            }))
        }
        X_XF86_VIDMODE_SET_CLIENT_VERSION_MINOR_OPCODE => {
            require_exact_len(
                X_XF86_VIDMODE_MAJOR_OPCODE,
                X_XF86_VIDMODE_SET_CLIENT_VERSION_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeSetClientVersion {
                major: context.byte_order.u16(&bytes[4..6]),
                minor: context.byte_order.u16(&bytes[6..8]),
            }))
        }
        // Decoded so the refusal can name the request. The alternative is a
        // parse rejection, which tells a client only that the extension
        // exists and says nothing about which of its twenty requests it may
        // use.
        minor_opcode => Ok(XWireRequest::Extension(crate::XExtensionRequest::XF86VidModeUnimplemented { minor_opcode })),
    }
}
