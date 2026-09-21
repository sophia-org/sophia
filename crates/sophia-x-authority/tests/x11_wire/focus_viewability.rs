
/// Creates a real window and optionally maps it, returning its id.
fn focus_candidate_window(
    runtime: &mut XAuthorityRuntime,
    namespace: NamespaceId,
    raw: u32,
    mapped: bool,
) -> XResourceId {
    let window = XResourceId::new(u64::from(raw), 1);
    assert_eq!(
        runtime
            .apply(XAuthorityRequestPacket {
                namespace,
                transaction: TransactionId::from_raw(u64::from(raw)),
                kind: XAuthorityRequestKind::CreateWindow {
                    window,
                    surface: SurfaceId::new(raw, 1),
                    geometry: Rect {
                        x: 0,
                        y: 0,
                        width: 40,
                        height: 30,
                    },
                    constraints: sophia_protocol::SurfaceConstraints {
                        min_size: None,
                        max_size: None,
                    },
                    generation: 1,
                },
            })
            .outcome,
        XAuthorityResponseOutcome::Accepted
    );
    if mapped {
        assert_eq!(
            runtime
                .apply(XAuthorityRequestPacket {
                    namespace,
                    transaction: TransactionId::from_raw(u64::from(raw) + 1),
                    kind: XAuthorityRequestKind::MapWindow {
                        window,
                        generation: 1,
                    },
                })
                .outcome,
            XAuthorityResponseOutcome::Accepted
        );
    }
    window
}

#[test]
fn x11_set_input_focus_refuses_a_window_that_is_not_viewable() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    let unmapped = focus_candidate_window(&mut runtime, namespace, 0x0a00_0001, false);
    let mapped = focus_candidate_window(&mut runtime, namespace, 0x0a00_0002, true);

    // Created but never mapped: the window is real, so this is not BadWindow.
    // Input cannot go to something nobody can see, which the protocol calls
    // BadMatch.
    let (focus, outputs) = focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        1,
        4_242,
        u32::try_from(unmapped.local.raw()).unwrap(),
        X_CURRENT_TIME,
    );
    assert_eq!(outputs.len(), 1, "one error and nothing else: {outputs:?}");
    assert_eq!(outputs[0][0], 0, "an error, not a reply");
    assert_eq!(outputs[0][1], 8, "BadMatch rather than BadWindow");
    assert_eq!(
        u64::from(X_SETUP_DEFAULT_ROOT),
        focus,
        "a refused request leaves the focus where it was"
    );

    // The same window, mapped, is accepted.
    let (focus, outputs) = focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        2,
        4_242,
        u32::try_from(mapped.local.raw()).unwrap(),
        X_CURRENT_TIME,
    );
    assert!(
        outputs.iter().all(|output| output[0] != 0),
        "a viewable window is focusable: {outputs:?}"
    );
    assert_eq!(mapped.local.raw(), focus);
}

#[test]
fn x11_set_input_focus_accepts_none_and_pointer_root_without_looking_them_up() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    // Neither is a window, so neither is looked up or checked for viewability.
    // PointerRoot in particular is the id 1, which no window ever has, and
    // treating it as one would refuse a request the protocol requires.
    for (sequence, sentinel) in [(1u16, X_FOCUS_NONE), (2, X_FOCUS_POINTER_ROOT)] {
        let (focus, outputs) = focus_after_request(
            &mut runtime,
            &mut atoms,
            &mut properties,
            namespace,
            sequence,
            4_242,
            sentinel,
            X_CURRENT_TIME,
        );
        assert!(
            outputs.iter().all(|output| output[0] != 0),
            "focus {sentinel} was refused: {outputs:?}"
        );
        assert_eq!(u64::from(sentinel), focus);
    }
}

#[test]
fn x11_set_input_focus_refuses_an_out_of_range_revert_to_with_bad_value() {
    // The decoder refuses it before the runtime ever sees it, and both
    // agree on BadValue: a revert_to of 3 names no choice the protocol
    // defines, which is not a complaint about the window.
    let namespace = NamespaceId::from_raw(77);
    let mut request = vec![42u8, 3, 3, 0];
    request.extend_from_slice(&X_SETUP_DEFAULT_ROOT.to_le_bytes());
    request.extend_from_slice(&X_CURRENT_TIME.to_le_bytes());
    let parsed = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &request,
    );
    let error = parsed.expect_err("a revert_to of 3 does not decode");
    assert_eq!(
        XErrorCode::BadValue,
        x_error_from_wire_parse(&error, 1, 42, 0).code
    );

    // And the runtime says the same, for callers that do not arrive by wire.
    let runtime = XAuthorityRuntime::new();
    assert_eq!(
        Err(XAuthorityRuntimeError::InvalidValue),
        runtime.validate_input_focus(
            namespace,
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            3,
        )
    );
}
