fn decode_query_extension(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    require_len(X_QUERY_EXTENSION, X_QUERY_EXTENSION_REQ_LEN, bytes.len())?;
    let name_len = usize::from(context.byte_order.u16(&bytes[4..6]));
    let expected_len = X_QUERY_EXTENSION_REQ_LEN + padded_len(name_len);
    if bytes.len() != expected_len {
        return Err(XWireParseError::InvalidLength {
            opcode: X_QUERY_EXTENSION,
            expected_at_least: expected_len,
            actual: bytes.len(),
        });
    }
    let name = core::str::from_utf8(
        &bytes[X_QUERY_EXTENSION_REQ_LEN..X_QUERY_EXTENSION_REQ_LEN + name_len],
    )
    .map_err(|_| XWireParseError::InvalidLength {
        opcode: X_QUERY_EXTENSION,
        expected_at_least: expected_len,
        actual: bytes.len(),
    })?;
    Ok(XWireRequest::Core(crate::XCoreRequest::QueryExtension {
        name: name.to_owned(),
    }))
}

fn decode_list_extensions(bytes: &[u8]) -> Result<XWireRequest, XWireParseError> {
    require_exact_len(X_LIST_EXTENSIONS, X_LIST_EXTENSIONS_REQ_LEN, bytes.len())?;
    Ok(XWireRequest::Core(crate::XCoreRequest::ListExtensions))
}

