
/// A SetInputFocus request naming `focus` with `revert_to` and `time`.
fn set_input_focus_request(focus: u32, revert_to: u8, time: u32) -> Vec<u8> {
    let mut bytes = vec![42u8, revert_to, 3, 0];
    bytes.extend_from_slice(&focus.to_le_bytes());
    bytes.extend_from_slice(&time.to_le_bytes());
    bytes
}

/// Dispatches one SetInputFocus at a chosen server time and returns the focus
/// that stands afterwards together with what the client was sent.
fn focus_after_request(
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
    namespace: NamespaceId,
    sequence: u16,
    server_time: u32,
    focus: u32,
    time: u32,
) -> (u64, Vec<Vec<u8>>) {
    let request = set_input_focus_request(focus, 1, time);
    let decoded = decode_x11_core_request(
        context(namespace, u64::from(sequence), XByteOrder::LittleEndian),
        &request,
    )
    .unwrap();
    let mut dispatch = dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 42);
    dispatch.server_time = server_time;
    let outputs = dispatch_x11_wire_request(dispatch, decoded, runtime, atoms, properties)
        .encoded_outputs(XByteOrder::LittleEndian);
    (runtime.input_focus(namespace).0.local.raw(), outputs)
}

fn focus_fixture() -> (XAuthorityRuntime, XAtomTable, XPropertyTable, NamespaceId) {
    (
        XAuthorityRuntime::new(),
        XAtomTable::new(),
        XPropertyTable::new(),
        NamespaceId::from_raw(77),
    )
}

#[test]
fn x11_focus_time_ordering_wraps_instead_of_comparing_as_a_plain_integer() {
    // Ordinary order, and equality, which is not "after".
    assert!(x_time_is_after(500, 400));
    assert!(!x_time_is_after(400, 500));
    assert!(!x_time_is_after(400, 400));
    // Across the wrap, a small new time is still later than a large old one.
    assert!(x_time_is_after(5, u32::MAX - 5));
    assert!(!x_time_is_after(u32::MAX - 5, 5));
    // A plain `>` would call the far half of the range "later"; the signed
    // reading calls it earlier, which is what keeps a client from naming a
    // time three weeks ahead and having it accepted.
    assert!(!x_time_is_after(0x9000_0000, 0));
    assert!(x_time_is_after(0x7000_0000, 0));
}

#[test]
fn x11_set_input_focus_ignores_a_time_before_the_last_change() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    let root = X_SETUP_DEFAULT_ROOT;

    let (focus, _) = focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        1,
        4_242,
        root,
        100,
    );
    assert_eq!(u64::from(root), focus, "a request at 100 sets the focus");
    assert_eq!(100, runtime.last_focus_change(namespace));

    let (focus, outputs) = focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        2,
        4_242,
        0,
        50,
    );
    assert_eq!(
        u64::from(root),
        focus,
        "a request naming a time before the last change has no effect"
    );
    assert_eq!(
        100,
        runtime.last_focus_change(namespace),
        "and does not move the last-focus-change time either"
    );
    assert!(
        outputs.is_empty(),
        "a discarded request owes the client nothing at all, not even an error: {outputs:?}"
    );
}

#[test]
fn x11_set_input_focus_ignores_a_time_the_server_has_not_reached() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    let (focus, outputs) = focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        1,
        4_242,
        X_SETUP_DEFAULT_ROOT,
        9_000,
    );
    assert_eq!(
        u64::from(X_SETUP_DEFAULT_ROOT),
        focus,
        "the focus starts at the root and a future-dated request leaves it there"
    );
    assert_eq!(
        0,
        runtime.last_focus_change(namespace),
        "no change was made, so there is no change time"
    );
    assert!(outputs.is_empty(), "{outputs:?}");
}

#[test]
fn x11_set_input_focus_accepts_a_time_equal_to_the_last_change() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        1,
        4_242,
        X_SETUP_DEFAULT_ROOT,
        700,
    );
    let (focus, _) = focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        2,
        4_242,
        0,
        700,
    );
    assert_eq!(
        0, focus,
        "the bound is earlier-than, so the same instant still applies"
    );
}

#[test]
fn x11_current_time_is_stored_as_the_server_time_rather_than_zero() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        1,
        4_242,
        X_SETUP_DEFAULT_ROOT,
        X_CURRENT_TIME,
    );
    assert_eq!(
        4_242,
        runtime.last_focus_change(namespace),
        "CurrentTime is the client declining to name a moment, so the server names it"
    );

    // If CurrentTime had been stored as zero, this earlier request would be
    // admitted, since nothing is before zero.
    let (focus, outputs) = focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        2,
        4_242,
        0,
        4_000,
    );
    assert_eq!(u64::from(X_SETUP_DEFAULT_ROOT), focus);
    assert!(outputs.is_empty(), "{outputs:?}");
}

#[test]
fn x11_set_input_focus_reports_its_errors_whatever_the_timestamp_says() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    // A window nobody created, named at a time the server has not reached.
    // The error is owed on the window whatever the clock says, because X11
    // decides the refusals before it measures the request against the time.
    let (_, outputs) = focus_after_request(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        1,
        4_242,
        0x00bd_0001,
        9_000,
    );
    let error = outputs
        .iter()
        .find(|output| output[0] == 0)
        .expect("a focus on a window nobody created is an error, not a discarded request");
    assert_eq!(
        3, error[1],
        "BadWindow, and reported rather than swallowed by the timestamp gate"
    );
}
