
/// Maps a window and returns what the client was sent.
fn map_outputs(
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
    namespace: NamespaceId,
    sequence: u16,
    window: u32,
) -> Vec<Vec<u8>> {
    let mut request = vec![8u8, 0, 2, 0];
    request.extend_from_slice(&window.to_le_bytes());
    let decoded = decode_x11_core_request(
        context(namespace, u64::from(sequence), XByteOrder::LittleEndian),
        &request,
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 8),
        decoded,
        runtime,
        atoms,
        properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian)
}

/// CreateWindow naming a background pixel, or none at all.
fn create_with_background(
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
    namespace: NamespaceId,
    sequence: u16,
    window: u32,
    parent: u32,
    background_pixel: Option<u32>,
) {
    let mut request = vec![1u8, 24, 0, 0];
    let length = 8 + usize::from(background_pixel.is_some());
    request[2] = u8::try_from(length).unwrap();
    request.extend_from_slice(&window.to_le_bytes());
    request.extend_from_slice(&parent.to_le_bytes());
    for value in [0i16, 0, 30, 30] {
        request.extend_from_slice(&value.to_le_bytes());
    }
    request.extend_from_slice(&0u16.to_le_bytes()); // border width
    request.extend_from_slice(&1u16.to_le_bytes()); // InputOutput
    request.extend_from_slice(&0u32.to_le_bytes()); // CopyFromParent visual
    // CWBackPixel is bit 1.
    request.extend_from_slice(&background_pixel.map_or(0u32, |_| 2).to_le_bytes());
    if let Some(pixel) = background_pixel {
        request.extend_from_slice(&pixel.to_le_bytes());
    }
    let decoded = decode_x11_core_request(
        context(namespace, u64::from(sequence), XByteOrder::LittleEndian),
        &request,
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 1),
        decoded,
        runtime,
        atoms,
        properties,
    );
}

/// Reads a window back and returns its first pixel.
fn first_pixel(
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
    namespace: NamespaceId,
    window: u32,
) -> Option<u32> {
    // GetImage is five words: the header, the drawable, four coordinates
    // packed as two words, and the plane mask.
    let mut request = vec![73u8, 2, 5, 0];
    request.extend_from_slice(&window.to_le_bytes());
    for value in [0i16, 0] {
        request.extend_from_slice(&value.to_le_bytes());
    }
    for value in [4u16, 4] {
        request.extend_from_slice(&value.to_le_bytes());
    }
    request.extend_from_slice(&u32::MAX.to_le_bytes());
    let decoded = decode_x11_core_request(
        context(namespace, 900, XByteOrder::LittleEndian),
        &request,
    )
    .unwrap();
    let outputs = dispatch_x11_wire_request(
        dispatch_context(namespace, 900, XByteOrder::LittleEndian, 73),
        decoded,
        runtime,
        atoms,
        properties,
    )
    .encoded_outputs(XByteOrder::LittleEndian);
    let reply = outputs.iter().find(|output| output[0] == 1)?;
    Some(read_u32(XByteOrder::LittleEndian, &reply[32..36]))
}

#[test]
fn x11_mapping_a_window_paints_it_with_its_background_pixel() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    create_with_background(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        1,
        0x0e00_0001,
        X_SETUP_DEFAULT_ROOT,
        Some(0x00ff_8040),
    );
    map_outputs(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        2,
        0x0e00_0001,
    );
    assert_eq!(
        Some(0x00ff_8040),
        first_pixel(
            &mut runtime,
            &mut atoms,
            &mut properties,
            namespace,
            0x0e00_0001
        ),
        "a window that named a background pixel is that colour once it is viewable"
    );
}

#[test]
fn x11_a_window_with_no_background_is_not_painted_at_all() {
    // An undefined background is not black. The protocol says the existing
    // screen contents are not altered, so the window contributes nothing and
    // whatever is underneath shows through.
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    create_with_background(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        1,
        0x0e00_0002,
        X_SETUP_DEFAULT_ROOT,
        None,
    );
    let parent = 0x0e00_0003;
    create_with_background(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        2,
        parent,
        X_SETUP_DEFAULT_ROOT,
        Some(0x0011_2233),
    );
    map_outputs(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        3,
        parent,
    );
    runtime
        .set_window_parent(
            namespace,
            XResourceId::new(0x0e00_0002, 1),
            XResourceId::new(u64::from(parent), 1),
        )
        .unwrap();
    map_outputs(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        4,
        0x0e00_0002,
    );
    assert_eq!(
        Some(0x0011_2233),
        first_pixel(&mut runtime, &mut atoms, &mut properties, namespace, parent),
        "the child has no background of its own, so the parent still shows through it"
    );
}

#[test]
fn x11_mapping_an_already_mapped_window_says_nothing() {
    let (mut runtime, mut atoms, mut properties, namespace) = focus_fixture();
    create_with_background(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        1,
        0x0e00_0004,
        X_SETUP_DEFAULT_ROOT,
        Some(7),
    );
    let first = map_outputs(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        2,
        0x0e00_0004,
    );
    assert!(
        first.iter().any(|output| output[0] == 19),
        "the first map announces itself: {first:?}"
    );
    let again = map_outputs(
        &mut runtime,
        &mut atoms,
        &mut properties,
        namespace,
        3,
        0x0e00_0004,
    );
    assert!(
        again.is_empty(),
        "mapping an already-mapped window has no effect, events included: {again:?}"
    );
}
