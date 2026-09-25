fn decode_big_requests(bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_BIG_REQUESTS_ENABLE_MINOR_OPCODE => {
            require_exact_len(
                X_BIG_REQUESTS_MAJOR_OPCODE,
                X_BIG_REQUESTS_ENABLE_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Extension(crate::XExtensionRequest::BigRequestsEnable))
        }
        _ => Err(XWireParseError::UnknownOpcode(bytes[0])),
    }
}

