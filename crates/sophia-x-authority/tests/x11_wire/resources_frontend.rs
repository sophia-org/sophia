#[test]
fn x11_dispatch_accepts_open_and_close_font_resources() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let open = decode_x11_core_request(
        context(namespace, 631, XByteOrder::LittleEndian),
        &open_font_request(XByteOrder::LittleEndian, 0x220131, "fixed"),
    )
    .unwrap();
    let open = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 45),
        open,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(open.outputs.is_empty());

    let query = decode_x11_core_request(
        context(namespace, 632, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 47, 0x220131),
    )
    .unwrap();
    let query = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 47),
        query,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = query.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 1);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][4..8]), 7);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][10..12]), 6);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][12..14]), 6);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][14..16]), 11);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][16..18]), 2);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][26..28]), 6);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][28..30]), 6);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][30..32]), 11);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][32..34]), 2);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][52..54]), 11);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][54..56]), 2);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][56..60]), 0);

    let list = decode_x11_core_request(
        context(namespace, 634, XByteOrder::LittleEndian),
        &list_fonts_request(XByteOrder::LittleEndian, 5, "*"),
    )
    .unwrap();
    let list = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 49),
        list,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = list.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 1);
    // A wildcard lists what the path actually publishes, bounded by the
    // client's own limit. With no host directory configured that is the
    // built-in element's names, `fixed` among them. It used to be exactly one
    // name whatever was asked for.
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][8..10]), 5);
    let mut at = 32;
    let mut listed = Vec::new();
    for _ in 0..5 {
        let len = usize::from(encoded[0][at]);
        listed.push(String::from_utf8_lossy(&encoded[0][at + 1..at + 1 + len]).into_owned());
        at += 1 + len;
    }
    assert!(listed.iter().any(|name| name == "fixed"), "listed {listed:?}");
    assert!(listed.iter().any(|name| name == "6x13"), "listed {listed:?}");

    let list = decode_x11_core_request(
        context(namespace, 635, XByteOrder::LittleEndian),
        &list_fonts_with_info_request(XByteOrder::LittleEndian, 5, "*"),
    )
    .unwrap();
    let list = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 50),
        list,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = list.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 1);
    // One 60-byte description per name, then a zero-length terminator. Each
    // carries the metrics of the face that name resolves to; a single set for
    // every name was the defect that reported an ascent of eight for a face
    // whose ascent is eleven.
    assert_eq!(encoded[0][1], 5, "the first entry names five characters");
    assert_eq!(&encoded[0][60..65], b"fixed");
    assert_eq!(
        read_i16(XByteOrder::LittleEndian, &encoded[0][52..54]),
        11,
        "the built-in face's real ascent"
    );
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][54..56]), 2);
    assert_eq!(encoded[0][49], 0, "single byte: min_byte1");
    assert_eq!(encoded[0][50], 0, "single byte: max_byte1");
    // The terminator is the last description and names nothing.
    let terminator = encoded[0].len() - 60;
    assert_eq!(encoded[0][terminator + 1], 0, "the series ends with an empty name");

    let close = decode_x11_core_request(
        context(namespace, 636, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 46, 0x220131),
    )
    .unwrap();
    let close = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 46),
        close,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(close.outputs.is_empty());
}

#[test]
fn x11_dispatch_accepts_glyph_cursor_lifecycle() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    for (sequence, font) in [(1u16, 0x220141), (2u16, 0x220142)] {
        let open = decode_x11_core_request(
            context(
                namespace,
                640 + u64::from(sequence),
                XByteOrder::LittleEndian,
            ),
            &open_font_request(XByteOrder::LittleEndian, font, "cursor"),
        )
        .unwrap();
        let open = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 45),
            open,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(open.outputs.is_empty());
    }

    let cursor = decode_x11_core_request(
        context(namespace, 643, XByteOrder::LittleEndian),
        &create_glyph_cursor_request(XByteOrder::LittleEndian, 0x220143, 0x220141, 0x220142),
    )
    .unwrap();
    let cursor = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 94),
        cursor,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(cursor.outputs.is_empty());

    let recolor = decode_x11_core_request(
        context(namespace, 644, XByteOrder::LittleEndian),
        &recolor_cursor_request(XByteOrder::LittleEndian, 0x220143),
    )
    .unwrap();
    let recolor = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 96),
        recolor,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(recolor.outputs.is_empty());

    let free = decode_x11_core_request(
        context(namespace, 645, XByteOrder::LittleEndian),
        &resource_request(XByteOrder::LittleEndian, 95, 0x220143),
    )
    .unwrap();
    let free = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 95),
        free,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(free.outputs.is_empty());
}

#[test]
fn x11_dispatch_accepts_xterm_nil2_compatibility_font() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let open = decode_x11_core_request(
        context(namespace, 646, XByteOrder::LittleEndian),
        &open_font_request(XByteOrder::LittleEndian, 0x220149, "nil2"),
    )
    .unwrap();
    let open = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 45),
        open,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(open.outputs.is_empty());
}

#[test]
fn x11_core_resource_ids_are_global_and_collisions_preserve_the_original() {
    let namespace = NamespaceId::from_raw(46);
    let window = 0x22014a;
    let gc = 0x22014b;
    let font = 0x22014c;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let create_window = decode_x11_core_request(
        context(namespace, 647, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 3, 4, 64, 48),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create_window,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    for (sequence, opcode, request) in [
        (
            2,
            45,
            open_font_request(XByteOrder::LittleEndian, window, "fixed"),
        ),
        (
            3,
            53,
            create_pixmap_request(
                XByteOrder::LittleEndian,
                24,
                window,
                window,
                16,
                16,
            ),
        ),
        (
            4,
            55,
            create_gc_request(XByteOrder::LittleEndian, window, window),
        ),
    ] {
        let request = decode_x11_core_request(
            context(namespace, 647 + u64::from(sequence), XByteOrder::LittleEndian),
            &request,
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(matches!(
            result.outputs.as_slice(),
            [XClientOutput::Error(XClientError {
                code: XErrorCode::BadIdChoice,
                resource_id,
                ..
            })] if *resource_id == window
        ));
    }
    assert_eq!(
        runtime
            .window_geometry(namespace, XResourceId::new(window.into(), 1))
            .unwrap(),
        Rect {
            x: 3,
            y: 4,
            width: 64,
            height: 48,
        }
    );

    let create_gc = decode_x11_core_request(
        context(namespace, 652, XByteOrder::LittleEndian),
        &create_gc_request(XByteOrder::LittleEndian, gc, window),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 55),
        create_gc,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let collide_gc = decode_x11_core_request(
        context(namespace, 653, XByteOrder::LittleEndian),
        &open_font_request(XByteOrder::LittleEndian, gc, "fixed"),
    )
    .unwrap();
    let collide_gc = dispatch_x11_wire_request(
        dispatch_context(namespace, 6, XByteOrder::LittleEndian, 45),
        collide_gc,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        collide_gc.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadIdChoice,
            resource_id,
            ..
        })] if *resource_id == gc
    ));
    assert!(runtime.graphics_context_values(namespace, XResourceId::new(gc.into(), 1)).is_ok());

    let open_font = decode_x11_core_request(
        context(namespace, 654, XByteOrder::LittleEndian),
        &open_font_request(XByteOrder::LittleEndian, font, "fixed"),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 7, XByteOrder::LittleEndian, 45),
        open_font,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let collide_font = decode_x11_core_request(
        context(namespace, 655, XByteOrder::LittleEndian),
        &create_pixmap_request(XByteOrder::LittleEndian, 24, font, window, 8, 8),
    )
    .unwrap();
    let collide_font = dispatch_x11_wire_request(
        dispatch_context(namespace, 8, XByteOrder::LittleEndian, 53),
        collide_font,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        collide_font.outputs.as_slice(),
        [XClientOutput::Error(XClientError {
            code: XErrorCode::BadIdChoice,
            resource_id,
            ..
        })] if *resource_id == font
    ));
    assert!(runtime.validate_font_access(namespace, XResourceId::new(font.into(), 1)).is_ok());
}

#[test]
fn x11_pixmap_and_graphics_context_requests_report_resource_specific_errors() {
    let namespace = NamespaceId::from_raw(46);
    let window = 0x22014d;
    let missing = 0x22014e;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let create_window = decode_x11_core_request(
        context(namespace, 656, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 64, 48),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create_window,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    for (sequence, opcode, request, expected_code) in [
        (
            2,
            53,
            create_pixmap_request(
                XByteOrder::LittleEndian,
                24,
                0x22014f,
                window,
                0,
                8,
            ),
            XErrorCode::BadValue,
        ),
        (
            3,
            54,
            free_pixmap_request(XByteOrder::LittleEndian, missing),
            XErrorCode::BadPixmap,
        ),
        (
            4,
            59,
            set_clip_rectangles_request(XByteOrder::LittleEndian, missing, &[]),
            XErrorCode::BadGraphicsContext,
        ),
        (
            5,
            60,
            free_graphics_context_request(XByteOrder::LittleEndian, missing),
            XErrorCode::BadGraphicsContext,
        ),
    ] {
        let request = decode_x11_core_request(
            context(namespace, 656 + u64::from(sequence), XByteOrder::LittleEndian),
            &request,
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(matches!(
            result.outputs.as_slice(),
            [XClientOutput::Error(XClientError { code, .. })] if *code == expected_code
        ));
    }

    assert_eq!(
        runtime
            .window_geometry(namespace, XResourceId::new(window.into(), 1))
            .unwrap(),
        Rect {
            x: 0,
            y: 0,
            width: 64,
            height: 48,
        }
    );
}

#[test]
fn x11_dispatch_text8_emits_exact_fixed_6x13_damage() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let window = 0x220151;

    let create = decode_x11_core_request(
        context(namespace, 646, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 300, 200),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let gc = decode_x11_core_request(
        context(namespace, 647, XByteOrder::LittleEndian),
        &create_gc_request(XByteOrder::LittleEndian, 0x220152, window),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 55),
        gc,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let text = decode_x11_core_request(
        context(namespace, 648, XByteOrder::LittleEndian),
        &poly_text8_request(XByteOrder::LittleEndian, window, 0x220152, 5, 16, b"Hi"),
    )
    .unwrap();
    let text = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 74),
        text,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let response = text.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 5,
            y: 5,
            width: 12,
            height: 13,
        })
    );

    let image_text = decode_x11_core_request(
        context(namespace, 649, XByteOrder::LittleEndian),
        &image_text8_request(XByteOrder::LittleEndian, window, 0x220152, 9, 20, b"OK"),
    )
    .unwrap();
    let image_text = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 76),
        image_text,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let response = image_text.response.unwrap();
    assert_eq!(response.transactions.len(), 1);
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 9,
            y: 9,
            width: 12,
            height: 13,
        })
    );
}

#[test]
fn x11_dispatch_opens_the_fixed_face_under_either_charset_registry() {
    // An XLFD's trailing fields are its charset registry and encoding, not a
    // different typeface. xterm asks for the iso10646-1 spelling whenever it is
    // in UTF-8 mode, which on a UTF-8 locale is by default; refusing it refused
    // the terminal, which then never presented a frame for an input proof to
    // type into.
    let accepted = [
        "fixed",
        "6x13",
        "nil2",
        "cursor",
        sophia_x_authority::X_FIXED_6X13_CANONICAL_NAME,
        sophia_x_authority::X_FIXED_6X13_UNICODE_NAME,
    ];

    for (index, name) in accepted.iter().enumerate() {
        let namespace = NamespaceId::from_raw(46);
        let mut runtime = XAuthorityRuntime::new();
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        let font = 0x22_0200 + u32::try_from(index).expect("font index");

        let open = decode_x11_core_request(
            context(namespace, 700, XByteOrder::LittleEndian),
            &open_font_request(XByteOrder::LittleEndian, font, name),
        )
        .unwrap();
        let open = dispatch_x11_wire_request(
            dispatch_context(namespace, 1, XByteOrder::LittleEndian, 45),
            open,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(
            open.outputs.is_empty(),
            "OpenFont refused an accepted face name: {name}"
        );

        // The font is only usable if it also answers QueryFont: a name that
        // opened but could not be queried would fail the client one request
        // later, which is how this defect presented.
        let query = decode_x11_core_request(
            context(namespace, 701, XByteOrder::LittleEndian),
            &resource_request(XByteOrder::LittleEndian, 47, font),
        )
        .unwrap();
        let query = dispatch_x11_wire_request(
            dispatch_context(namespace, 2, XByteOrder::LittleEndian, 47),
            query,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(
            matches!(
                query.outputs.first(),
                Some(sophia_x_authority::XClientOutput::Reply(
                    sophia_x_authority::XClientReply::QueryFont { .. }
                ))
            ),
            "QueryFont did not answer for an accepted face name: {name}"
        );
    }
}

#[test]
fn x11_dispatch_still_refuses_a_face_it_cannot_rasterize() {
    // Accepting a second registry for the one face Sophia rasterizes must not
    // become accepting any name at all.
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let open = decode_x11_core_request(
        context(namespace, 710, XByteOrder::LittleEndian),
        &open_font_request(XByteOrder::LittleEndian, 0x22_0300, "10x20"),
    )
    .unwrap();
    let open = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 45),
        open,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(
        matches!(
            open.outputs.first(),
            Some(sophia_x_authority::XClientOutput::Error(error))
                if error.code == sophia_x_authority::XErrorCode::BadName
        ),
        "a face Sophia cannot rasterize must still be refused by name"
    );
}

/// Build a window and a graphics context, then return the dispatcher's view.
fn text_window_and_gc(
    namespace: NamespaceId,
    window: u32,
    gc: u32,
    runtime: &mut XAuthorityRuntime,
    atoms: &mut XAtomTable,
    properties: &mut XPropertyTable,
) {
    for (sequence, opcode, bytes) in [
        (
            1u16,
            1u8,
            create_window_request(XByteOrder::LittleEndian, window, 0, 0, 300, 200),
        ),
        (
            2,
            55,
            create_gc_request(XByteOrder::LittleEndian, gc, window),
        ),
    ] {
        let request = decode_x11_core_request(
            context(namespace, u64::from(sequence) + 900, XByteOrder::LittleEndian),
            &bytes,
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
            request,
            runtime,
            atoms,
            properties,
        );
    }
}

#[test]
fn x11_sixteen_bit_text_draws_what_eight_bit_text_draws_for_the_same_characters() {
    // A character whose high byte is zero names the same glyph through either
    // request, so the two must produce identical pixels. This is the property
    // that says the 16-bit path reaches the raster correctly rather than
    // merely decoding: a swapped CHAR2B or a mis-scaled advance shows here.
    let namespace = NamespaceId::from_raw(46);
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let mut eight = XAuthorityRuntime::new();
    text_window_and_gc(
        namespace,
        0x220161,
        0x220162,
        &mut eight,
        &mut atoms,
        &mut properties,
    );
    let request = decode_x11_core_request(
        context(namespace, 960, XByteOrder::LittleEndian),
        &image_text8_request(XByteOrder::LittleEndian, 0x220161, 0x220162, 9, 20, b"AaZz"),
    )
    .unwrap();
    let eight_draw = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 76),
        request,
        &mut eight,
        &mut atoms,
        &mut properties,
    );

    let mut sixteen = XAuthorityRuntime::new();
    text_window_and_gc(
        namespace,
        0x220161,
        0x220162,
        &mut sixteen,
        &mut atoms,
        &mut properties,
    );
    let codes: Vec<u16> = b"AaZz".iter().map(|byte| u16::from(*byte)).collect();
    let request = decode_x11_core_request(
        context(namespace, 961, XByteOrder::LittleEndian),
        &image_text16_request(XByteOrder::LittleEndian, 0x220161, 0x220162, 9, 20, &codes),
    )
    .unwrap();
    let sixteen_draw = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 77),
        request,
        &mut sixteen,
        &mut atoms,
        &mut properties,
    );

    assert!(sixteen_draw.outputs.is_empty(), "a 16-bit draw is not an error");
    let eight_response = eight_draw.response.unwrap();
    let sixteen_response = sixteen_draw.response.unwrap();
    assert_eq!(
        sixteen_response.transactions[0].damage,
        eight_response.transactions[0].damage,
        "the same characters dirty the same rectangle"
    );
    let XAuthorityCpuBufferUpdate::Replace(eight_pixels) =
        eight.take_cpu_buffer_update().expect("the 8-bit draw reached the raster")
    else {
        panic!("the first update replaces the buffer");
    };
    let XAuthorityCpuBufferUpdate::Replace(sixteen_pixels) =
        sixteen.take_cpu_buffer_update().expect("the 16-bit draw reached the raster")
    else {
        panic!("the first update replaces the buffer");
    };
    assert!(
        eight_pixels.bytes.iter().any(|byte| *byte != 0),
        "the comparison is only worth making if something was painted"
    );
    assert_eq!(
        sixteen_pixels.bytes, eight_pixels.bytes,
        "and painted the same pixels"
    );
}

#[test]
fn x11_query_text_extents_measures_the_face_the_fontable_holds() {
    // Xlib lays text out from these numbers without asking again, so they must
    // agree with what the server paints. Four characters of the built-in face
    // are twenty four pixels wide, eleven above the baseline and two below.
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    text_window_and_gc(
        namespace,
        0x220171,
        0x220172,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let codes: Vec<u16> = b"AaZz".iter().map(|byte| u16::from(*byte)).collect();
    let request = decode_x11_core_request(
        context(namespace, 970, XByteOrder::LittleEndian),
        &query_text_extents_request(XByteOrder::LittleEndian, 0x220172, &codes),
    )
    .unwrap();
    let extents = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 48),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = extents.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 1, "a reply, not an error");
    assert_eq!(encoded[0][1], 0, "left to right");
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][8..10]), 11);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][10..12]), 2);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][12..14]), 11);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &encoded[0][14..16]), 2);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][16..20]), 24);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][20..24]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &encoded[0][24..28]), 24);

    // An unknown fontable is BadFont rather than a reply of zeros.
    let request = decode_x11_core_request(
        context(namespace, 971, XByteOrder::LittleEndian),
        &query_text_extents_request(XByteOrder::LittleEndian, 0x220199, &codes),
    )
    .unwrap();
    let refused = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 48),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        refused.outputs.as_slice(),
        [XClientOutput::Error(error)] if error.code == XErrorCode::BadFont
    ));
}

#[test]
fn a_client_cannot_change_the_font_path_but_may_read_it() {
    // The refusal is the safeguard that makes exposing a host path
    // defensible: the path is session configuration, and nothing a client
    // sends can add a directory to search. It is answered as BadAccess rather
    // than left undecoded, because a client that meets BadRequest may exit --
    // xterm installs an error handler that does exactly that.
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    let request = decode_x11_core_request(
        context(namespace, 980, XByteOrder::LittleEndian),
        &set_font_path_request(XByteOrder::LittleEndian, "/tmp/fonts"),
    )
    .unwrap();
    let refused = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 51),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        refused.outputs.as_slice(),
        [XClientOutput::Error(error)] if error.code == XErrorCode::BadAccess
    ));

    let request = decode_x11_core_request(
        context(namespace, 981, XByteOrder::LittleEndian),
        &get_font_path_request(XByteOrder::LittleEndian),
    )
    .unwrap();
    let path = dispatch_x11_wire_request(
        dispatch_context(namespace, 2, XByteOrder::LittleEndian, 52),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = path.encoded_outputs(XByteOrder::LittleEndian);
    assert_eq!(encoded.len(), 1);
    assert_eq!(encoded[0][0], 1, "a reply, not an error");
    // With no directories configured the path is the built-in element alone,
    // named the way the X server names its own.
    assert_eq!(read_u16(XByteOrder::LittleEndian, &encoded[0][8..10]), 1);
    let len = usize::from(encoded[0][32]);
    assert_eq!(&encoded[0][33..33 + len], b"built-ins");
}
