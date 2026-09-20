/// The minor a decoded XTEST request came from, for the error that refuses it.
fn xtest_minor_opcode(request: &XWireRequest) -> Option<u8> {
    match request {
        XWireRequest::XTestGetVersion { .. } => Some(crate::X_TEST_GET_VERSION_MINOR_OPCODE),
        XWireRequest::XTestCompareCursor { .. } => Some(crate::X_TEST_COMPARE_CURSOR_MINOR_OPCODE),
        XWireRequest::XTestFakeInput { .. } => Some(crate::X_TEST_FAKE_INPUT_MINOR_OPCODE),
        XWireRequest::XTestGrabControl { .. } => Some(crate::X_TEST_GRAB_CONTROL_MINOR_OPCODE),
        XWireRequest::XTestUnimplemented { minor_opcode } => Some(*minor_opcode),
        _ => None,
    }
}

fn xtest_error(context: XDispatchContext, code: XErrorCode, minor: u8) -> XDispatchResult {
    XDispatchResult {
        response: None,
        outputs: vec![XClientOutput::Error(crate::XClientError {
            code,
            sequence: context.sequence,
            // No resource was named, and naming one the request did not carry
            // would invent evidence.
            resource_id: 0,
            minor_code: u16::from(minor),
            major_code: context.major_opcode,
        })],
        metadata_candidates: Vec::new(),
    }
}

/// XTEST, answered against the one decision about whether this client may
/// inject.
///
/// A client that may not gets `BadAccess` on every request, and that is a
/// complete answer rather than a placeholder. `BadAccess` and not
/// `BadRequest` is the point: an undecoded major answers `BadRequest`, which
/// tells a client the server has no such extension, and a client that guessed
/// the opcode would believe it. `BadAccess` says the request exists and this
/// client may not make it. Discovery agrees, because it reads the same field.
fn dispatch_xtest_request(
    context: XDispatchContext,
    request: XWireRequest,
    _runtime: &mut XAuthorityRuntime,
) -> XDispatchFamilyResult {
    let Some(minor) = xtest_minor_opcode(&request) else {
        return Unhandled(request);
    };

    if matches!(context.injection, crate::XTestAdmission::Absent) {
        return Handled(xtest_error(context, XErrorCode::BadAccess, minor));
    }

    Handled(match request {
        XWireRequest::XTestGetVersion { .. } => XDispatchResult {
            response: None,
            // A constant, whatever was asked for. The reference server never
            // reads the requested version at all, and the wire cases demand
            // 2.1 for everything, so there is no negotiation to perform and
            // no per-client version state to keep.
            outputs: vec![XClientOutput::Reply(XClientReply::XTestGetVersion {
                sequence: context.sequence,
                major_version: u8::try_from(crate::X_TEST_MAJOR_VERSION).unwrap_or(u8::MAX),
                minor_version: crate::X_TEST_MINOR_VERSION,
            })],
            metadata_candidates: Vec::new(),
        },
        // XTEST has four requests and will not grow any, so a minor it does
        // not define is a request this extension does not have -- which is
        // BadRequest, not BadAccess, for a client that may use the rest.
        XWireRequest::XTestUnimplemented { .. } => {
            xtest_error(context, XErrorCode::BadRequest, minor)
        }
        // Each joins here as it lands. Unreachable meanwhile: no connection
        // is issued an injector yet, so nothing is ever admitted.
        _ => xtest_error(context, XErrorCode::BadImplementation, minor),
    })
}
