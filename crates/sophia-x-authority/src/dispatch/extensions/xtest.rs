/// The minor a decoded XTEST request came from, for the error that refuses it.
fn xtest_minor_opcode(request: &XWireRequest) -> Option<u8> {
    match request {
        XWireRequest::XTestGetVersion { .. } => Some(crate::X_TEST_GET_VERSION_MINOR_OPCODE),
        XWireRequest::XTestCompareCursor { .. } => {
            Some(crate::X_TEST_COMPARE_CURSOR_MINOR_OPCODE)
        }
        XWireRequest::XTestFakeInput { .. } => Some(crate::X_TEST_FAKE_INPUT_MINOR_OPCODE),
        XWireRequest::XTestGrabControl { .. } => Some(crate::X_TEST_GRAB_CONTROL_MINOR_OPCODE),
        XWireRequest::XTestUnimplemented { minor_opcode } => Some(*minor_opcode),
        _ => None,
    }
}

/// XTEST, answered for a client that is not admitted to inject.
///
/// Every request refuses with `BadAccess`. That is the whole of the extension
/// today and it is a complete answer rather than a placeholder: no connection
/// can yet be admitted to inject, because the injection seam that would admit
/// one is not built, so every client reaching here is exactly the unauthorized
/// client the contract describes.
///
/// `BadAccess` and not `BadRequest` is the point. An undecoded major answers
/// `BadRequest`, which tells a client the server has no such extension, and a
/// client that guessed the opcode would believe it. `BadAccess` says the
/// request exists and this client may not make it, which is true and is what
/// the conformance case for a denied connection checks. Discovery agrees:
/// `QueryExtension` and `ListExtensions` both omit XTEST while no client is
/// admitted, so nothing here contradicts what a client was told.
///
/// When the injection seam lands, the admitted case joins this dispatcher and
/// the refusal stays for everyone else, still decided in one place.
fn dispatch_xtest_request(
    context: XDispatchContext,
    request: XWireRequest,
    _runtime: &mut XAuthorityRuntime,
) -> XDispatchFamilyResult {
    let Some(minor) = xtest_minor_opcode(&request) else {
        return Unhandled(request);
    };

    Handled(XDispatchResult {
        response: None,
        outputs: vec![XClientOutput::Error(crate::XClientError {
            code: XErrorCode::BadAccess,
            sequence: context.sequence,
            // No resource was named, and naming one the request did not carry
            // would invent evidence.
            resource_id: 0,
            minor_code: u16::from(minor),
            major_code: context.major_opcode,
        })],
        metadata_candidates: Vec::new(),
    })
}
