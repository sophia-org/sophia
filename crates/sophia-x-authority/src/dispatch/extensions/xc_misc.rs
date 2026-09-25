fn dispatch_xc_misc_request(
    context: XDispatchContext,
    request: XWireRequest,
    _runtime: &mut XAuthorityRuntime,
) -> XDispatchFamilyResult {
    if !matches!(
        &request,
        XWireRequest::Extension(crate::XExtensionRequest::XCMiscGetVersion { .. })
            | XWireRequest::Extension(crate::XExtensionRequest::XCMiscGetXIDRange)
            | XWireRequest::Extension(crate::XExtensionRequest::XCMiscGetXIDList { .. })
    ) {
        return Unhandled(request);
    }
    Handled(match request {
        XWireRequest::Extension(crate::XExtensionRequest::XCMiscGetVersion { .. }) => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Reply(XClientReply::XCMiscGetVersion {
                sequence: context.sequence,
                major_version: crate::X_XC_MISC_MAJOR_VERSION,
                minor_version: crate::X_XC_MISC_MINOR_VERSION,
            })],
            metadata_candidates: Vec::new(),
        },
        // Identifier ranges are owned by the socket layer, which is where the
        // client table and its range counter live. Answering "none available"
        // here means the reply is already correct if that layer cannot grant
        // one, and the socket layer replaces it when it can.
        //
        // A count of zero is a real protocol answer that clients handle. The
        // alternative -- inventing a range -- would hand out identifiers that
        // collide with another client's resources.
        XWireRequest::Extension(crate::XExtensionRequest::XCMiscGetXIDRange) => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Reply(XClientReply::XCMiscGetXIDRange {
                sequence: context.sequence,
                start_id: 0,
                count: 0,
            })],
            metadata_candidates: Vec::new(),
        },
        XWireRequest::Extension(crate::XExtensionRequest::XCMiscGetXIDList { .. }) => XDispatchResult {
            response: None,
            outputs: vec![XClientOutput::Reply(XClientReply::XCMiscGetXIDList {
                sequence: context.sequence,
                ids: Vec::new(),
            })],
            metadata_candidates: Vec::new(),
        },
        other => return Unhandled(other),
    })
}
