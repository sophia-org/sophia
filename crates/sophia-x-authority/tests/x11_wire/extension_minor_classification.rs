/// Refusing an extension minor is two-tier, and the tiers mean different
/// things to a client. A minor defined within the advertised version but not
/// implemented answers BadImplementation: the server of that version has an
/// entry for it and declined. A minor beyond the advertised version answers
/// BadRequest, because a genuine server of that version had no entry at all.
///
/// The doctrine is stated in `wire/constants.rs` beside the RENDER minors and
/// was already honoured by RENDER and XFIXES. This checks every extension that
/// classifies, so one of them cannot quietly drift back to a single answer.
#[test]
fn every_extension_separates_unimplemented_minors_from_unknown_ones() {
    let namespace = NamespaceId::from_raw(46);

    // (major opcode, a minor inside the advertised version that is not
    // implemented, a minor beyond the advertised version)
    // (major opcode, a defined-but-unimplemented minor, a minor past the
    // advertised version, how to build that extension's refused request)
    type MinorCase = (u8, u8, u8, fn(u8) -> XWireRequest);

    let cases: Vec<MinorCase> = vec![
        // Present 1.2 defines 0..=4 and implements all of them, so the
        // in-version tier is exercised by its own boundary rather than a gap.
        (X_PRESENT_MAJOR_OPCODE, 4, 255, |minor| {
            XWireRequest::Present(sophia_x_authority::XPresentRequest::PresentUnimplemented { minor_opcode: minor })
        }),
        // DRI3 minor 5 is FDFromFence: defined, deliberately not implemented.
        (X_DRI3_MAJOR_OPCODE, 5, 200, |minor| {
            XWireRequest::Dri3(sophia_x_authority::XDri3Request::Dri3Unimplemented { minor_opcode: minor })
        }),
        // SHAPE minor 6 is SelectInput.
        (X_SHAPE_MAJOR_OPCODE, 6, 200, |minor| {
            XWireRequest::Shape(sophia_x_authority::XShapeRequest::ShapeUnimplemented { minor_opcode: minor })
        }),
        // XF86VidMode minor 10 is SwitchToMode.
        (X_XF86_VIDMODE_MAJOR_OPCODE, 10, 200, |minor| {
            XWireRequest::Extension(sophia_x_authority::XExtensionRequest::XF86VidModeUnimplemented { minor_opcode: minor })
        }),
        // GLX minor 30 sits inside the range its last minor, 35, closes.
        (X_GLX_MAJOR_OPCODE, 30, 200, |minor| {
            XWireRequest::Glx(sophia_x_authority::XGlxRequest::GlxUnimplemented { minor_opcode: minor })
        }),
        // XFIXES and RENDER already classified; they are here so a regression
        // in the shared doctrine is caught wherever it happens.
        (X_XFIXES_MAJOR_OPCODE, 30, 200, |minor| {
            XWireRequest::Xfixes(sophia_x_authority::XFixesRequest::XfixesUnimplemented { minor_opcode: minor })
        }),
        (X_RENDER_MAJOR_OPCODE, 10, 200, |minor| {
            XWireRequest::Render(sophia_x_authority::XRenderRequest::RenderUnimplemented { minor_opcode: minor })
        }),
    ];

    for (major, defined_minor, unknown_minor, build) in cases {
        for (minor, expected) in [
            (defined_minor, XErrorCode::BadImplementation),
            (unknown_minor, XErrorCode::BadRequest),
        ] {
            let mut runtime = XAuthorityRuntime::new();
            let mut atoms = XAtomTable::new();
            let mut properties = XPropertyTable::new();
            let result = dispatch_x11_wire_request(
                dispatch_context(namespace, 7, XByteOrder::LittleEndian, major),
                build(minor),
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            let [XClientOutput::Error(error)] = result.outputs.as_slice() else {
                panic!("major {major} minor {minor}: expected one error, got {:?}", result.outputs);
            };
            assert_eq!(
                error.code, expected,
                "major {major} minor {minor} must answer {expected:?}"
            );
            assert_eq!(error.major_code, major, "the error names its extension");
            assert_eq!(
                error.minor_code,
                u16::from(minor),
                "the error names the refused minor"
            );
            assert_eq!(error.sequence, 7, "the client can attribute the failure");
        }
    }
}

/// Every advertised extension must answer an unknown minor the same way.
///
/// A client that probes an extension it does not fully know sends a minor the
/// server has never heard of. The answer is BadRequest: the request does not
/// exist. Answering BadLength instead tells the client its own well-formed
/// request was the wrong size, which sends it looking for a bug it does not
/// have.
///
/// This drives the real decode-and-dispatch path for every name the server
/// enumerates, so an extension cannot be added with a different answer.
#[test]
fn every_advertised_extension_answers_an_unknown_minor_with_bad_request() {
    let namespace = NamespaceId::from_raw(52);
    let mut checked = 0;

    // Enumerated over the wire rather than from a Rust list, so an extension
    // added later is covered without anyone remembering to add it here.
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let listed = decode_x11_core_request(
        context(namespace, 899, XByteOrder::LittleEndian),
        &[99, 0, 1, 0],
    )
    .unwrap();
    let listed = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 99),
        listed,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let listed = listed.encoded_outputs(XByteOrder::LittleEndian);
    let count = usize::from(listed[0][1]);
    let mut names = Vec::new();
    let mut at = 32;
    for _ in 0..count {
        let len = usize::from(listed[0][at]);
        at += 1;
        names.push(String::from_utf8(listed[0][at..at + len].to_vec()).expect("utf8"));
        at += len;
    }

    for name in &names {
        let query = decode_x11_core_request(
            context(namespace, 900, XByteOrder::LittleEndian),
            &query_extension_request(XByteOrder::LittleEndian, name),
        )
        .unwrap();
        let answered = dispatch_x11_wire_request(
            dispatch_context(namespace, 1, XByteOrder::LittleEndian, 98),
            query,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let reply = answered.encoded_outputs(XByteOrder::LittleEndian);
        assert_eq!(reply[0][8], 1, "{name} is enumerated, so it must be present");
        let major = reply[0][9];

        // A bare request carrying nothing but a minor this server has never
        // defined, exactly as a probing client sends it.
        let bare = [major, 255, 1, 0];
        let code = match decode_x11_core_request(
            context(namespace, 901, XByteOrder::LittleEndian),
            &bare,
        ) {
            Ok(request) => {
                let refused = dispatch_x11_wire_request(
                    dispatch_context(namespace, 2, XByteOrder::LittleEndian, major),
                    request,
                    &mut runtime,
                    &mut atoms,
                    &mut properties,
                );
                match refused.outputs.as_slice() {
                    [XClientOutput::Error(error)] => error.code,
                    other => panic!("{name}: expected one error, got {other:?}"),
                }
            }
            Err(error) => x_error_from_wire_parse(&error, 2, major, 255).code,
        };
        assert_eq!(
            code,
            XErrorCode::BadRequest,
            "{name} (major {major}) answered an unknown minor with {code:?}"
        );
        checked += 1;
    }

    assert!(checked >= 16, "every advertised extension is covered: {checked}");
}
