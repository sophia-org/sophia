// Drawing through the frontend: segments, GC copies and dashes, points and
// arcs, clip masks, CopyPlane, and the colormap family on a TrueColor
// server. Included from x11_wire.rs beside resources_frontend.rs (t026).

#[test]
fn poly_segment_paints_its_segments_rather_than_only_reporting_them() {
    // xterm draws the VT100 line-drawing characters with this request when the
    // font has no glyph for them. The handler used to record damage and paint
    // nothing, so a terminal's box characters were simply absent.
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    for (sequence, opcode, bytes) in [
        (
            1u16,
            1u8,
            create_window_request(XByteOrder::LittleEndian, 0x220181, 0, 0, 300, 200),
        ),
        (
            2,
            45,
            open_font_request(XByteOrder::LittleEndian, 0x220183, "fixed"),
        ),
        (
            3,
            55,
            // A white foreground, because the buffer starts black and a black
            // line on it would be indistinguishable from painting nothing --
            // which is the very failure this test exists to catch.
            create_gc_values_request(
                XByteOrder::LittleEndian,
                0x220182,
                0x220181,
                3,
                u32::MAX,
                0x00ff_ffff,
                0,
                1,
                0x220183,
            ),
        ),
    ] {
        let request = decode_x11_core_request(
            context(namespace, u64::from(sequence) + 985, XByteOrder::LittleEndian),
            &bytes,
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
    }

    let request = decode_x11_core_request(
        context(namespace, 990, XByteOrder::LittleEndian),
        &poly_segment_request(
            XByteOrder::LittleEndian,
            0x220181,
            0x220182,
            &[(10, 10, 40, 10), (10, 30, 10, 60)],
        ),
    )
    .unwrap();
    let drawn = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 66),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    assert!(drawn.outputs.is_empty(), "a valid draw is not an error");
    let response = drawn.response.unwrap();
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions.len(), 1);
    let XAuthorityCpuBufferUpdate::Replace(snapshot) = runtime
        .take_cpu_buffer_update()
        .expect("the segments reached the raster")
    else {
        panic!("the first update replaces the buffer");
    };
    let painted = |x: usize, y: usize| -> bool {
        let stride = usize::try_from(snapshot.stride).unwrap();
        let at = y * stride + x * 4;
        snapshot.bytes[at..at + 4].iter().any(|byte| *byte != 0)
    };
    assert!(painted(25, 10), "the horizontal segment");
    assert!(painted(10, 45), "the vertical segment");
    assert!(
        !painted(25, 45),
        "and nothing between them: the segments are disjoint, not a path"
    );
}

#[test]
fn copy_gc_moves_only_the_components_its_mask_names() {
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let window = 0x220191;

    let create = decode_x11_core_request(
        context(namespace, 1000, XByteOrder::LittleEndian),
        &create_window_request(XByteOrder::LittleEndian, window, 0, 0, 64, 64),
    )
    .unwrap();
    dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 1),
        create,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    for (sequence, gc, foreground) in [(2u16, 0x220192u32, 0x00ff_0000u32), (3, 0x220193, 0x0000_00ff)] {
        let request = decode_x11_core_request(
            context(namespace, u64::from(sequence) + 1000, XByteOrder::LittleEndian),
            &create_gc_values_request(
                XByteOrder::LittleEndian,
                gc,
                window,
                3,
                u32::MAX,
                foreground,
                0,
                1,
                0,
            ),
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 55),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
    }

    // Copy the foreground alone. The line width the destination carries must
    // survive, because the mask did not name it.
    let request = decode_x11_core_request(
        context(namespace, 1004, XByteOrder::LittleEndian),
        &copy_gc_request(XByteOrder::LittleEndian, 0x220192, 0x220193, 1 << 2),
    )
    .unwrap();
    let copied = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 57),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(copied.outputs.is_empty(), "a valid copy is not an error");

    // An unknown source is the client's error, not a silent no-op.
    let request = decode_x11_core_request(
        context(namespace, 1005, XByteOrder::LittleEndian),
        &copy_gc_request(XByteOrder::LittleEndian, 0x2201ff, 0x220193, 1 << 2),
    )
    .unwrap();
    let refused = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 57),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        refused.outputs.as_slice(),
        [XClientOutput::Error(error)] if error.code == XErrorCode::BadGraphicsContext
    ));
}

#[test]
fn set_dashes_reports_an_unknown_context_before_it_judges_the_pattern() {
    // The server's order. A request that is wrong in two ways must name the
    // resource first, or a client debugging a bad identifier is told its
    // pattern is at fault.
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    text_window_and_gc(
        namespace,
        0x2201a1,
        0x2201a2,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );

    let cases: [(&[u8], u32, Option<XErrorCode>); 4] = [
        (&[4, 4], 0x2201a2, None),
        (&[3], 0x2201a2, None),
        (&[4, 0], 0x2201a2, Some(XErrorCode::BadValue)),
        // Both wrong: the unknown context wins.
        (&[4, 0], 0x2201ff, Some(XErrorCode::BadGraphicsContext)),
    ];
    for (index, (dashes, gc, expected)) in cases.into_iter().enumerate() {
        let sequence = u16::try_from(index).unwrap() + 3;
        let request = decode_x11_core_request(
            context(namespace, u64::from(sequence) + 1100, XByteOrder::LittleEndian),
            &set_dashes_request(XByteOrder::LittleEndian, gc, 0, dashes),
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, 58),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        match expected {
            None => assert!(result.outputs.is_empty(), "case {index}"),
            Some(code) => assert!(
                matches!(
                    result.outputs.as_slice(),
                    [XClientOutput::Error(error)] if error.code == code
                ),
                "case {index}: {:?}",
                result.outputs
            ),
        }
    }

    // An empty pattern is refused at decode, before any of this.
    assert!(
        decode_x11_core_request(
            context(namespace, 1200, XByteOrder::LittleEndian),
            &set_dashes_request(XByteOrder::LittleEndian, 0x2201a2, 0, &[]),
        )
        .map(|request| dispatch_x11_wire_request(
            dispatch_context(namespace, 9, XByteOrder::LittleEndian, 58),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        ))
        .is_ok_and(|result| !result.outputs.is_empty()),
        "an empty dash pattern is a bad value"
    );
}

#[test]
fn points_and_arcs_paint_rather_than_only_reporting_damage() {
    let namespace = NamespaceId::from_raw(46);
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    for (opcode, request_bytes) in [
        (
            64u8,
            poly_point_request(
                XByteOrder::LittleEndian,
                0x2201b1,
                0x2201b2,
                0,
                &[(10, 10), (20, 20), (30, 30)],
            ),
        ),
        (
            68,
            poly_arc_request(
                XByteOrder::LittleEndian,
                0x2201b1,
                0x2201b2,
                &[(5, 5, 40, 40, 0, 360 * 64)],
            ),
        ),
    ] {
        let mut runtime = XAuthorityRuntime::new();
        for (sequence, opcode, bytes) in [
            (
                1u16,
                1u8,
                create_window_request(XByteOrder::LittleEndian, 0x2201b1, 0, 0, 64, 64),
            ),
            (
                2,
                55,
                create_gc_values_request(
                    XByteOrder::LittleEndian,
                    0x2201b2,
                    0x2201b1,
                    3,
                    u32::MAX,
                    0x00ff_ffff,
                    0,
                    1,
                    0,
                ),
            ),
        ] {
            let request = decode_x11_core_request(
                context(namespace, u64::from(sequence) + 1300, XByteOrder::LittleEndian),
                &bytes,
            )
            .unwrap();
            dispatch_x11_wire_request(
                dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
                request,
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
        }

        let request =
            decode_x11_core_request(context(namespace, 1310, XByteOrder::LittleEndian), &request_bytes)
                .unwrap();
        let drawn = dispatch_x11_wire_request(
            dispatch_context(namespace, 3, XByteOrder::LittleEndian, opcode),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        assert!(drawn.outputs.is_empty(), "opcode {opcode} is not an error");
        let XAuthorityCpuBufferUpdate::Replace(snapshot) = runtime
            .take_cpu_buffer_update()
            .unwrap_or_else(|| panic!("opcode {opcode} reached the raster"))
        else {
            panic!("the first update replaces the buffer");
        };
        assert!(
            snapshot.bytes.iter().any(|byte| *byte != 0),
            "opcode {opcode} painted nothing, which is what it used to do"
        );
    }
}

#[test]
fn a_clip_mask_is_accepted_and_actually_clips() {
    // A non-zero clip mask used to answer BadImplementation, which told a
    // client its perfectly valid request was beyond this server. It is a
    // depth-one pixmap now, and it clips.
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let window = 0x2201c1;
    let mask = 0x2201c2;
    let gc = 0x2201c3;

    for (sequence, opcode, bytes) in [
        (
            1u16,
            1u8,
            create_window_request(XByteOrder::LittleEndian, window, 0, 0, 32, 32),
        ),
        (
            2,
            53,
            create_pixmap_request(XByteOrder::LittleEndian, 1, mask, window, 8, 8),
        ),
    ] {
        let request = decode_x11_core_request(
            context(namespace, u64::from(sequence) + 1400, XByteOrder::LittleEndian),
            &bytes,
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
    }

    // A graphics context naming the mask is accepted.
    let mut values = create_gc_values_request(
        XByteOrder::LittleEndian,
        gc,
        window,
        3,
        u32::MAX,
        0x00ff_ffff,
        0,
        1,
        0,
    );
    let request =
        decode_x11_core_request(context(namespace, 1410, XByteOrder::LittleEndian), &values)
            .unwrap();
    let created = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, XByteOrder::LittleEndian, 55),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(created.outputs.is_empty());
    values.clear();

    // A mask that is not a pixmap at all is the client's error, and names the
    // pixmap rather than blaming the server.
    let request = decode_x11_core_request(
        context(namespace, 1411, XByteOrder::LittleEndian),
        &change_gc_clip_mask_request(XByteOrder::LittleEndian, gc, 0x2201ff),
    )
    .unwrap();
    let refused = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 56),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(
        matches!(
            refused.outputs.as_slice(),
            [XClientOutput::Error(error)] if error.code == XErrorCode::BadPixmap
        ),
        "{:?}",
        refused.outputs
    );

    // And a real depth-one mask is taken.
    let request = decode_x11_core_request(
        context(namespace, 1412, XByteOrder::LittleEndian),
        &change_gc_clip_mask_request(XByteOrder::LittleEndian, gc, mask),
    )
    .unwrap();
    let accepted = dispatch_x11_wire_request(
        dispatch_context(namespace, 5, XByteOrder::LittleEndian, 56),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(accepted.outputs.is_empty(), "{:?}", accepted.outputs);
}

#[test]
fn copy_plane_expands_one_bit_into_foreground_and_background() {
    // The path every Xaw button icon and menu checkmark takes: a depth-one
    // bitmap becomes coloured pixels. Where the plane's bit is set the
    // destination takes the foreground, and where it is clear, the background.
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let window = 0x2201d1;
    let source = 0x2201d2;
    let gc = 0x2201d3;

    for (sequence, opcode, bytes) in [
        (
            1u16,
            1u8,
            create_window_request(XByteOrder::LittleEndian, window, 0, 0, 32, 32),
        ),
        (
            2,
            53,
            create_pixmap_request(XByteOrder::LittleEndian, 24, source, window, 8, 8),
        ),
        (
            3,
            55,
            create_gc_values_request(
                XByteOrder::LittleEndian,
                gc,
                window,
                3,
                u32::MAX,
                0x00ff_ffff,
                0x0000_00ff,
                1,
                0,
            ),
        ),
    ] {
        let request = decode_x11_core_request(
            context(namespace, u64::from(sequence) + 1500, XByteOrder::LittleEndian),
            &bytes,
        )
        .unwrap();
        dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
    }

    let request = decode_x11_core_request(
        context(namespace, 1510, XByteOrder::LittleEndian),
        &copy_plane_request(
            XByteOrder::LittleEndian,
            source,
            window,
            gc,
            (0, 0),
            (2, 2),
            (8, 8),
            1,
        ),
    )
    .unwrap();
    let copied = dispatch_x11_wire_request(
        dispatch_context(namespace, 4, XByteOrder::LittleEndian, 63),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    // A plane copy has CopyArea's exposure semantics: the whole source was
    // there, so the context's graphics-exposures earn one NoExpose.
    assert!(
        matches!(
            copied.outputs.as_slice(),
            [XClientOutput::Event(XClientEvent::NoExpose {
                sequence: 4,
                drawable,
                minor_opcode: 0,
                major_opcode: 63,
            })] if *drawable == XResourceId::new(window.into(), 1)
        ),
        "{:?}",
        copied.outputs
    );
    let response = copied.response.expect("a copy produces a transaction");
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);

    // A bit plane of zero, or of several bits, is a bad value rather than a
    // copy of nothing.
    for plane in [0u32, 0b11] {
        assert!(
            decode_x11_core_request(
                context(namespace, 1520, XByteOrder::LittleEndian),
                &copy_plane_request(
                    XByteOrder::LittleEndian,
                    source,
                    window,
                    gc,
                    (0, 0),
                    (0, 0),
                    (8, 8),
                    plane,
                ),
            )
            .is_err(),
            "bit plane {plane:#b} names other than one plane"
        );
    }
}

#[test]
fn a_true_color_server_answers_the_colormap_family_rather_than_refusing_it() {
    // None of these can be served on a TrueColor visual, but all of them must
    // be answered: they arrive on ordinary teardown paths, and a client that
    // meets BadRequest may treat it as fatal.
    let namespace = NamespaceId::from_raw(46);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let window = 0x2201e1;
    let colormap = 0x2201e2;

    let _ = window;
    let request = decode_x11_core_request(
        context(namespace, 1601, XByteOrder::LittleEndian),
        &create_colormap_request(
            XByteOrder::LittleEndian,
            colormap,
            X_SETUP_DEFAULT_ROOT,
            X_SETUP_DEFAULT_VISUAL,
        ),
    )
    .unwrap();
    let created = dispatch_x11_wire_request(
        dispatch_context(namespace, 1, XByteOrder::LittleEndian, 78),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(created.outputs.is_empty(), "the colormap is created");

    // Each request framed as the protocol frames it (t169). Read-write
    // allocation has no cells to give; storing into a read-only colormap is
    // denied; installing notifies windows; freeing unallocated cells is denied.
    // The copy starts empty because this client has no allocations yet.
    let order = XByteOrder::LittleEndian;
    let body = |words: &[u32]| {
        let mut out = Vec::new();
        push_u32(&mut out, order, colormap);
        for word in words {
            push_u32(&mut out, order, *word);
        }
        out
    };
    let mut copy = Vec::new();
    push_u32(&mut copy, order, 0x2201e3);
    push_u32(&mut copy, order, colormap);
    let mut named = body(&[0]);
    push_u16(&mut named, order, 3);
    push_u16(&mut named, order, 0);
    named.extend_from_slice(b"red\0");
    let cases: [(u8, Vec<u8>, Option<XErrorCode>); 8] = [
        (80, copy, None),
        (86, body(&[1]), Some(XErrorCode::BadAlloc)),
        (87, body(&[1, 0]), Some(XErrorCode::BadAlloc)),
        (89, body(&[0, 0, 0]), Some(XErrorCode::BadAccess)),
        (90, named, Some(XErrorCode::BadAccess)),
        (81, body(&[]), None),
        (82, body(&[]), None),
        (88, body(&[0, 1]), Some(XErrorCode::BadAccess)),
    ];
    for (index, (opcode, request_body, expected)) in cases.into_iter().enumerate() {
        let sequence = u16::try_from(index).unwrap() + 3;
        let mut bytes = vec![opcode, 0];
        push_u16(&mut bytes, XByteOrder::LittleEndian, 1 + u16::try_from(request_body.len() / 4).unwrap());
        bytes.extend_from_slice(&request_body);
        let request = decode_x11_core_request(
            context(namespace, u64::from(sequence) + 1600, XByteOrder::LittleEndian),
            &bytes,
        )
        .unwrap_or_else(|error| panic!("opcode {opcode} decodes: {error:?}"));
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, sequence, XByteOrder::LittleEndian, opcode),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        match expected {
            None if matches!(opcode, 81 | 82) => {
                assert!(!result.outputs.is_empty());
                assert!(result.outputs.iter().all(|output| matches!(output,
                    XClientOutput::Event(XClientEvent::ColormapNotify { new: false, .. }))));
            }
            None => assert!(result.outputs.is_empty(), "opcode {opcode}"),
            Some(code) => assert!(
                matches!(
                    result.outputs.as_slice(),
                    [XClientOutput::Error(error)] if error.code == code
                ),
                "opcode {opcode}: {:?}",
                result.outputs
            ),
        }
    }

    // An unknown colormap is named as such, ahead of the family's own answer.
    let mut bytes = vec![86u8, 0];
    push_u16(&mut bytes, XByteOrder::LittleEndian, 3);
    push_u32(&mut bytes, XByteOrder::LittleEndian, 0x2201ff);
    push_u32(&mut bytes, XByteOrder::LittleEndian, 1);
    let request =
        decode_x11_core_request(context(namespace, 1700, XByteOrder::LittleEndian), &bytes).unwrap();
    let refused = dispatch_x11_wire_request(
        dispatch_context(namespace, 20, XByteOrder::LittleEndian, 86),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    assert!(matches!(
        refused.outputs.as_slice(),
        [XClientOutput::Error(error)] if error.code == XErrorCode::BadColor
    ));
}
