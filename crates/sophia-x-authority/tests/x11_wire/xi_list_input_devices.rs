// XI1 `ListInputDevices` and the shared XI1/XI2 master-device inventory.

/// Sophia advertises XInputExtension and answers the XI1 version handshake, so the
/// enumeration a client issues next has to be answered too. A signed session failed
/// on the `BadRequest` storm this produced.
#[test]
fn xi_decoder_accepts_the_legacy_device_enumeration() {
    let namespace = NamespaceId::from_raw(51);
    assert_eq!(
        decode_x11_core_request(
            context(namespace, 1, XByteOrder::LittleEndian),
            &[X_INPUT_MAJOR_OPCODE, X_INPUT_LIST_INPUT_DEVICES_MINOR_OPCODE, 1, 0],
        )
        .unwrap(),
        XWireRequest::Xi(sophia_x_authority::XInputRequest::XiListInputDevices)
    );
}

/// The request carries no arguments, so a longer one is malformed rather than an
/// enumeration with trailing data.
#[test]
fn xi_decoder_rejects_a_legacy_enumeration_with_trailing_bytes() {
    let namespace = NamespaceId::from_raw(52);
    assert!(matches!(
        decode_x11_core_request(
            context(namespace, 1, XByteOrder::LittleEndian),
            &[
                X_INPUT_MAJOR_OPCODE,
                X_INPUT_LIST_INPUT_DEVICES_MINOR_OPCODE,
                2,
                0,
                0,
                0,
                0,
                0,
            ],
        ),
        Err(XWireParseError::InvalidLength { .. })
    ));
}

#[test]
fn xi_list_input_devices_reports_both_virtual_masters() {
    let namespace = NamespaceId::from_raw(53);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let outputs = dispatch_x11_wire_request(
        dispatch_context(namespace, 7, XByteOrder::LittleEndian, X_INPUT_MAJOR_OPCODE),
        XWireRequest::Xi(sophia_x_authority::XInputRequest::XiListInputDevices),
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    let reply = &outputs[0];

    assert_eq!(reply[0], 1, "an XI reply, not an event or error");
    assert_eq!(reply[1], X_INPUT_LIST_INPUT_DEVICES_MINOR_OPCODE);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &reply[2..4]), 7);
    // The device count is a single byte here; XI2 puts a u16 at the same offset.
    assert_eq!(reply[8], 2);
    assert!(reply[9..32].iter().all(|byte| *byte == 0), "header pad");

    // The declared length must describe the body exactly, or a client reads into the
    // next reply. This is the assertion that catches an off-by-one in any of the
    // three body sections.
    let declared = read_u32(XByteOrder::LittleEndian, &reply[4..8]) as usize;
    assert_eq!(reply.len(), 32 + declared * 4);

    // DeviceInfo records are 8 bytes each and come first.
    let pointer = &reply[32..40];
    let keyboard = &reply[40..48];
    assert_eq!(pointer[4], 2, "virtual master pointer id");
    assert_eq!(pointer[5], 1, "one class");
    assert_eq!(pointer[6], X_INPUT_LEGACY_USE_POINTER);
    assert_eq!(keyboard[4], 3, "virtual master keyboard id");
    assert_eq!(keyboard[5], 1, "one class");
    assert_eq!(keyboard[6], X_INPUT_LEGACY_USE_KEYBOARD);
    assert_ne!(
        read_u32(XByteOrder::LittleEndian, &pointer[0..4]),
        read_u32(XByteOrder::LittleEndian, &keyboard[0..4]),
        "each device interns its own type atom"
    );

    // Then every class info, pointer first.
    let button = &reply[48..52];
    assert_eq!(button[0], X_INPUT_LEGACY_CLASS_BUTTON);
    assert_eq!(button[1], 4, "ButtonInfo is four bytes");
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &button[2..4]),
        u16::from(sophia_x_authority::X_POINTER_BUTTON_COUNT),
        "the same buttons GetPointerMapping reports"
    );
    let key = &reply[52..60];
    assert_eq!(key[0], X_INPUT_LEGACY_CLASS_KEY);
    assert_eq!(key[1], 8, "KeyInfo is eight bytes");
    assert_eq!(key[2], 8, "core minimum keycode");
    assert_eq!(key[3], 255, "core maximum keycode");
    assert_eq!(read_u16(XByteOrder::LittleEndian, &key[4..6]), 248);

    // Then the names, each a length byte followed by its bytes and no padding.
    let names = &reply[60..];
    let pointer_len = usize::from(names[0]);
    assert_eq!(
        &names[1..1 + pointer_len],
        b"Sophia master pointer",
        "the XI1 name matches the XI2 one"
    );
    let keyboard_len = usize::from(names[1 + pointer_len]);
    let keyboard_start = 2 + pointer_len;
    assert_eq!(
        &names[keyboard_start..keyboard_start + keyboard_len],
        b"Sophia master keyboard"
    );
}

/// XI1's core pair must agree with XI2's master query. The attached pointer source
/// has an XI2-only identifier and does not belong in this comparison.
#[test]
fn the_legacy_and_xi2_master_enumerations_describe_one_device_set() {
    let namespace = NamespaceId::from_raw(54);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let legacy = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, X_INPUT_MAJOR_OPCODE),
        XWireRequest::Xi(sophia_x_authority::XInputRequest::XiListInputDevices),
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    let modern = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, X_INPUT_MAJOR_OPCODE),
        XWireRequest::Xi(sophia_x_authority::XInputRequest::XiQueryDevice { device_id: 1 }),
        &mut runtime,
        &mut atoms,
        &mut properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);

    let legacy = parse_legacy_devices(&legacy[0]);
    let modern = parse_xi2_devices(&modern[0]);

    assert_eq!(legacy.len(), 2);
    assert_eq!(legacy.len(), modern.len(), "same device count");
    for (legacy, modern) in legacy.iter().zip(modern.iter()) {
        assert_eq!(u16::from(legacy.0), modern.0, "same device id, same order");
        assert_eq!(legacy.1, modern.1, "same name");
    }

    // The pointer's buttons: XI1 states the count, XI2 states it in its Button class.
    assert_eq!(legacy[0].2, modern[0].2, "same button count");
    // The keyboard's keys: XI1 states the bounds, XI2 lists every key between them.
    assert_eq!(legacy[1].3, modern[1].3, "same key count");
}

/// Walks an XI1 reply into `(device_id, name, button_count, key_count)`.
///
/// The three sections are laid out end to end, so the class walk is what locates
/// the names: they begin exactly where the last class info ends.
fn parse_legacy_devices(reply: &[u8]) -> Vec<(u8, String, u16, u16)> {
    let order = XByteOrder::LittleEndian;
    let count = usize::from(reply[8]);
    let mut devices: Vec<(u8, String, u16, u16)> = (0..count)
        .map(|index| (reply[32 + index * 8 + 4], String::new(), 0, 0))
        .collect();

    let mut offset = 32 + count * 8;
    for (index, device) in devices.iter_mut().enumerate() {
        for _ in 0..reply[32 + index * 8 + 5] {
            let length = usize::from(reply[offset + 1]);
            match reply[offset] {
                X_INPUT_LEGACY_CLASS_BUTTON => {
                    device.2 = read_u16(order, &reply[offset + 2..offset + 4]);
                }
                X_INPUT_LEGACY_CLASS_KEY => {
                    device.3 = read_u16(order, &reply[offset + 4..offset + 6]);
                }
                other => panic!("unexpected XI1 class {other}"),
            }
            offset += length;
        }
    }
    for device in &mut devices {
        let length = usize::from(reply[offset]);
        device.1 = String::from_utf8(reply[offset + 1..offset + 1 + length].to_vec()).unwrap();
        offset += 1 + length;
    }
    devices
}

/// Walks an XI2 reply into the same tuple, so the two can be compared directly.
fn parse_xi2_devices(reply: &[u8]) -> Vec<(u16, String, u16, u16)> {
    let order = XByteOrder::LittleEndian;
    let count = usize::from(read_u16(order, &reply[8..10]));
    let mut devices = Vec::new();
    let mut offset = 32;
    for _ in 0..count {
        let device_id = read_u16(order, &reply[offset..offset + 2]);
        let class_count = read_u16(order, &reply[offset + 6..offset + 8]);
        let name_len = usize::from(read_u16(order, &reply[offset + 8..offset + 10]));
        let name = String::from_utf8(reply[offset + 12..offset + 12 + name_len].to_vec()).unwrap();
        // The encoder pads the assembled body, and every device starts 4-aligned.
        offset += 12 + name_len.next_multiple_of(4);
        let (mut buttons, mut keys) = (0, 0);
        for _ in 0..class_count {
            let class_id = read_u16(order, &reply[offset..offset + 2]);
            let words = usize::from(read_u16(order, &reply[offset + 2..offset + 4]));
            match class_id {
                0 => keys = read_u16(order, &reply[offset + 6..offset + 8]),
                1 => buttons = read_u16(order, &reply[offset + 6..offset + 8]),
                _ => {}
            }
            offset += words * 4;
        }
        devices.push((device_id, name, buttons, keys));
    }
    devices
}
