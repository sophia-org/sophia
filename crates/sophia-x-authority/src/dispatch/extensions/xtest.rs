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

fn xtest_error(
    context: XDispatchContext,
    code: XErrorCode,
    minor: u8,
    value: u32,
) -> XDispatchResult {
    XDispatchResult {
        response: None,
        outputs: vec![XClientOutput::Error(crate::XClientError {
            code,
            sequence: context.sequence,
            // The refused value where there is one, so a client can see what
            // it sent. Zero where the request named nothing, because naming
            // something it did not carry would invent evidence.
            resource_id: value,
            minor_code: u16::from(minor),
            major_code: context.major_opcode,
        })],
        metadata_candidates: Vec::new(),
    }
}

/// Nothing to say. FakeInput and GrabControl owe no reply, so acceptance is
/// the absence of an error.
fn xtest_accepted() -> XDispatchResult {
    XDispatchResult {
        response: None,
        outputs: Vec::new(),
        metadata_candidates: Vec::new(),
    }
}

/// The lowest keycode a keyboard can carry. The reference refuses a detail
/// outside the keyboard's own range, whose floor is this.
const X_TEST_MIN_KEYCODE: u8 = 8;
/// The reference pointer carries ten buttons.
const X_TEST_MAX_BUTTON: u8 = 10;

/// Validate a FakeInput for an admitted client.
///
/// The order is the reference server's, and it is observable: type before
/// record count, and both before detail and root. The delay is not here at
/// all -- it is taken by the connection before this runs, because the
/// reference sleeps before it validates detail and root, so a malformed
/// request carrying a delay waits and only then answers its error.
///
/// Only refusals are decided here. What an accepted request does happens
/// outside the runtime guard this runs under, because submitting to the
/// authority and then waiting for its completion are both things this guard
/// must not be held across.
fn validate_fake_input(
    context: XDispatchContext,
    request: &XWireRequest,
    runtime: &XAuthorityRuntime,
) -> XDispatchResult {
    let XWireRequest::XTestFakeInput {
        event_type,
        sent_event_type,
        detail,
        root,
        events,
        ..
    } = *request
    else {
        unreachable!("validated only for FakeInput");
    };
    let minor = crate::X_TEST_FAKE_INPUT_MINOR_OPCODE;
    let bad_value = |value: u32| xtest_error(context, XErrorCode::BadValue, minor, value);

    // The type first, reporting the byte that arrived rather than the one it
    // was read as: the send-event bit was masked for dispatch and kept for
    // exactly this.
    if !(crate::client_output::X_KEY_PRESS..=crate::client_output::X_MOTION_NOTIFY).contains(&event_type) {
        return bad_value(u32::from(sent_event_type));
    }
    // A core type carries exactly one record. More is only meaningful to the
    // XInput path, which is refused above by its type, so what remains is a
    // request of the wrong length for what it asks.
    if events != 1 {
        return xtest_error(context, XErrorCode::BadLength, minor, 0);
    }
    match event_type {
        crate::client_output::X_KEY_PRESS | crate::client_output::X_KEY_RELEASE => {
            if detail < X_TEST_MIN_KEYCODE {
                return bad_value(u32::from(detail));
            }
        }
        crate::client_output::X_BUTTON_PRESS | crate::client_output::X_BUTTON_RELEASE => {
            // Zero is refused explicitly, and the count is the reference
            // pointer's. A button is a one-based index, not a mask.
            if detail == 0 || detail > X_TEST_MAX_BUTTON {
                return bad_value(u32::from(detail));
            }
        }
        _ => {
            // Strictly absolute or relative. Two is not a second spelling of
            // relative, it is a value the request does not define.
            if detail != crate::X_TEST_MOTION_ABSOLUTE && detail != crate::X_TEST_MOTION_RELATIVE {
                return bad_value(u32::from(detail));
            }
            // The root is consulted for motion only. None means the pointer's
            // current screen. Anything else has to be a window, and has to be
            // the root: a window that exists and is not the root is a value
            // the request does not accept, which is a different fault from a
            // resource that does not exist.
            if root != 0 {
                let window = crate::XResourceId::new(u64::from(root), 1);
                if runtime
                    .validate_window_access(context.namespace, window)
                    .is_err()
                {
                    return xtest_error(context, XErrorCode::BadWindow, minor, root);
                }
                if root != crate::X_SETUP_DEFAULT_ROOT {
                    return bad_value(root);
                }
            }
        }
    }
    xtest_accepted()
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
    runtime: &mut XAuthorityRuntime,
) -> XDispatchFamilyResult {
    let Some(minor) = xtest_minor_opcode(&request) else {
        return Unhandled(request);
    };

    if matches!(context.injection, crate::XTestAdmission::Absent) {
        return Handled(xtest_error(context, XErrorCode::BadAccess, minor, 0));
    }

    Handled(match &request {
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
        XWireRequest::XTestFakeInput { .. } => validate_fake_input(context, &request, runtime),
        XWireRequest::XTestGrabControl { impervious } => {
            // A strict boolean. The core protocol lets many BOOL fields pass
            // any nonzero; this one does not, and two is BadValue naming two.
            if *impervious > 1 {
                xtest_error(context, XErrorCode::BadValue, minor, u32::from(*impervious))
            } else {
                xtest_accepted()
            }
        }
        // XTEST has four requests and will not grow any, so a minor it does
        // not define is a request this extension does not have -- which is
        // BadRequest, not BadAccess, for a client that may use the rest.
        XWireRequest::XTestUnimplemented { .. } => {
            xtest_error(context, XErrorCode::BadRequest, minor, 0)
        }
        // CompareCursor joins here with the window cursor attribute.
        _ => xtest_error(context, XErrorCode::BadImplementation, minor, 0),
    })
}
