// CUT_BUFFER0, the second and independent path a selection can travel.
//
// t124 asks for it separately from the selection protocol because it is
// separate: xterm's mouse drag writes the text to CUT_BUFFER0 as well as
// taking PRIMARY, and xterm's own paste bindings read CUT_BUFFER0 when the
// selection conversion yields nothing. It is an ordinary property on the root
// window, so it owes nothing to selection ownership, targets, timestamps or
// the clipboard portal -- which is exactly why it is worth reading on its own
// before any of those are suspected.

#[test]
fn cut_buffer_zero_on_the_root_is_readable_by_another_client() {
    // ONE NAMESPACE, TWO CLIENTS, which is what a display is. The namespace
    // belongs to the listener rather than to the connection --
    // `run_x11_core_socket_server` takes one and serves every client on it --
    // so two X clients on one $DISPLAY always share it. A cross-namespace
    // reading of this path would be describing two displays.
    let namespace = NamespaceId::from_raw(124);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let root = crate::X_SETUP_DEFAULT_ROOT;
    const CUT_BUFFER0: u32 = 9;
    let cut = b"selected by the mouse";

    // The owner writes it, as xterm does on a drag.
    let write = decode_x11_core_request(
        context(namespace, 900, XByteOrder::LittleEndian),
        &change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            root,
            CUT_BUFFER0,
            X_ATOM_STRING,
            8,
            cut,
        ),
    )
    .expect("ChangeProperty on the root decodes");
    let written = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        write,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        !written
            .outputs
            .iter()
            .any(|output| matches!(output, XClientOutput::Error(_))),
        "writing CUT_BUFFER0 on the root was refused: {:?}",
        written.outputs
    );

    // A DIFFERENT CLIENT READS IT. Client two, not one: the whole question is
    // whether the buffer crosses connections, and a reader that is also the
    // writer would answer a different one.
    let read = decode_x11_core_request(
        context(namespace, 901, XByteOrder::LittleEndian),
        &get_property_request(
            XByteOrder::LittleEndian,
            false,
            root,
            CUT_BUFFER0,
            X_ATOM_STRING,
            0,
            1024,
        ),
    )
    .expect("GetProperty on the root decodes");
    let got = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 2),
        read,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let value = got
        .outputs
        .iter()
        .find_map(|output| match output {
            XClientOutput::Reply(XClientReply::GetProperty {
                bytes,
                property_type,
                format,
                ..
            }) => Some((bytes.clone(), *property_type, *format)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no GetProperty reply: {:?}", got.outputs));
    assert_eq!(
        value.0, cut,
        "another client read different bytes from CUT_BUFFER0"
    );
    assert_eq!(value.1, X_ATOM_STRING, "the type did not survive");
    assert_eq!(value.2, 8, "the format did not survive");
}
