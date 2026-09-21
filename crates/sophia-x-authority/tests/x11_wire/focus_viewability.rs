
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

/// Unmaps a window and returns what the client was sent for that request.
fn unmap_outputs(
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
    namespace: NamespaceId,
    sequence: u16,
    window: XResourceId,
) -> Vec<Vec<u8>> {
    let mut request = vec![10u8, 0, 2, 0];
    request.extend_from_slice(&u32::try_from(window.local.raw()).unwrap().to_le_bytes());
    let decoded = decode_x11_core_request(
        context(namespace, u64::from(sequence), XByteOrder::LittleEndian),
        &request,
    )
    .unwrap();
    let mut dispatch = dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 10);
    dispatch.server_time = 4_242;
    dispatch_x11_wire_request(dispatch, decoded, runtime, atoms, properties)
        .encoded_outputs(XByteOrder::LittleEndian)
}

#[test]
fn x11_unmapping_the_focus_window_reverts_the_focus_and_says_so() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    let base = focus_candidate_window(&mut runtime, namespace, 0x0b00_0001, true);
    let child = focus_candidate_window(&mut runtime, namespace, 0x0b00_0002, true);
    runtime.set_window_parent(namespace, child, base).unwrap();

    // Focus the child with revert_to Parent, the case the protocol describes
    // at length: the focus should climb to the closest viewable ancestor and
    // forget that it was ever told to.
    let mut request = vec![42u8, X_REVERT_TO_PARENT, 3, 0];
    request.extend_from_slice(&u32::try_from(child.local.raw()).unwrap().to_le_bytes());
    request.extend_from_slice(&X_CURRENT_TIME.to_le_bytes());
    let decoded = decode_x11_core_request(
        context(namespace, 1, XByteOrder::LittleEndian),
        &request,
    )
    .unwrap();
    let mut dispatch = dispatch_context(namespace, 1, XByteOrder::LittleEndian, 42);
    dispatch.server_time = 4_242;
    dispatch_x11_wire_request(dispatch, decoded, &mut runtime, &mut atoms, &mut properties);
    assert_eq!(
        (child, X_REVERT_TO_PARENT),
        runtime.input_focus(namespace),
        "the child holds the focus before anything is unmapped"
    );

    let outputs = unmap_outputs(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        2,
        child,
    );
    assert_eq!(
        (base, X_REVERT_TO_NONE),
        runtime.input_focus(namespace),
        "the focus reverts to the parent and the revert_to becomes None"
    );

    // The child is told the focus left it and the base is told it arrived,
    // which is the pair the conformance suite waits for.
    let focus_events: Vec<(u8, u8)> = outputs
        .iter()
        .filter(|output| output[0] == 9 || output[0] == 10)
        .map(|output| (output[0], output[1]))
        .collect();
    assert_eq!(
        vec![(10, X_FOCUS_DETAIL_ANCESTOR), (9, X_FOCUS_DETAIL_INFERIOR)],
        focus_events,
        "a FocusOut on the child and a FocusIn on its parent: {outputs:?}"
    );
}

#[test]
fn x11_unmapping_the_focus_window_honours_pointer_root_and_none_reverts() {
    for (revert_to, expected) in [
        (X_REVERT_TO_POINTER_ROOT, X_FOCUS_POINTER_ROOT),
        (X_REVERT_TO_NONE, X_FOCUS_NONE),
    ] {
        let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
        let window = focus_candidate_window(&mut runtime, namespace, 0x0c00_0001, true);
        let mut request = vec![42u8, revert_to, 3, 0];
        request.extend_from_slice(&u32::try_from(window.local.raw()).unwrap().to_le_bytes());
        request.extend_from_slice(&X_CURRENT_TIME.to_le_bytes());
        let decoded = decode_x11_core_request(
            context(namespace, 1, XByteOrder::LittleEndian),
            &request,
        )
        .unwrap();
        let mut dispatch = dispatch_context(namespace, 1, XByteOrder::LittleEndian, 42);
        dispatch.server_time = 4_242;
        dispatch_x11_wire_request(dispatch, decoded, &mut runtime, &mut atoms, &mut properties);

        unmap_outputs(
            &mut runtime,
            &mut atoms,
            &mut properties,
            namespace,
            2,
            window,
        );
        let (focus, kept) = runtime.input_focus(namespace);
        assert_eq!(
            u64::from(expected),
            focus.local.raw(),
            "revert_to {revert_to} reverts to exactly that value"
        );
        assert_eq!(
            revert_to, kept,
            "and keeps its revert_to, because there is nothing left to walk"
        );
    }
}

#[test]
fn x11_a_reversion_does_not_move_the_last_focus_change_time() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    let window = focus_candidate_window(&mut runtime, namespace, 0x0d00_0001, true);
    let mut request = vec![42u8, X_REVERT_TO_NONE, 3, 0];
    request.extend_from_slice(&u32::try_from(window.local.raw()).unwrap().to_le_bytes());
    request.extend_from_slice(&700u32.to_le_bytes());
    let decoded =
        decode_x11_core_request(context(namespace, 1, XByteOrder::LittleEndian), &request).unwrap();
    let mut dispatch = dispatch_context(namespace, 1, XByteOrder::LittleEndian, 42);
    dispatch.server_time = 4_242;
    dispatch_x11_wire_request(dispatch, decoded, &mut runtime, &mut atoms, &mut properties);
    assert_eq!(700, runtime.last_focus_change(namespace));

    unmap_outputs(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        2,
        window,
    );
    assert_eq!(
        700,
        runtime.last_focus_change(namespace),
        "reverting is the server acting, not a client naming a moment, so a \
         request that was honest when it was sent still lands afterwards"
    );
}
