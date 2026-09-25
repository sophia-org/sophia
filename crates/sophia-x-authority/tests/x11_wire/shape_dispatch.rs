// SHAPE through dispatch: the fixture, kinds, operations, notifications,
// masks, and what a shape does to presentation. Included from x11_wire.rs
// beside extensions_dispatch.rs (t026).

/// A fixture driving SHAPE requests against one window.
struct ShapeFixture {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
}

impl ShapeFixture {
    const NS: NamespaceId = NamespaceId::from_raw(93);
    const ORDER: XByteOrder = XByteOrder::LittleEndian;
    const WINDOW: u32 = 0x0020_0400;
    const OTHER: u32 = 0x0020_0401;
    const MASK: u32 = 0x0020_0402;
    const CLIENT: u64 = 77;

    /// A ten-by-ten window, which every default region below is measured
    /// against.
    fn new() -> Self {
        let mut fixture = Self {
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            sequence: 0,
        };
        let create = create_window_request(Self::ORDER, Self::WINDOW, 0, 0, 10, 10);
        let result = fixture.send(&create);
        assert_eq!(Self::error_of(&result), None, "window create");
        fixture
    }

    fn send(&mut self, bytes: &[u8]) -> XDispatchResult {
        self.send_as(Self::CLIENT, bytes)
    }

    fn send_as(&mut self, client: u64, bytes: &[u8]) -> XDispatchResult {
        self.sequence = self.sequence.wrapping_add(1);
        let request = decode_x11_core_request(
            context(Self::NS, u64::from(self.sequence) + 1400, Self::ORDER),
            bytes,
        )
        .expect("request must decode");
        let mut dispatch = dispatch_context(Self::NS, self.sequence, Self::ORDER, bytes[0]);
        dispatch.client_id = client;
        dispatch_x11_wire_request(
            dispatch,
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        )
    }

    /// Set one kind to a rectangle list.
    fn set(&mut self, kind: u8, rects: &[Rect]) -> XDispatchResult {
        self.send(&shape_rectangles_request(
            Self::ORDER,
            X_SHAPE_OP_SET,
            kind,
            X_SHAPE_ORDERING_UNSORTED,
            Self::WINDOW,
            0,
            0,
            rects,
        ))
    }

    fn rects(&mut self, kind: u8) -> Vec<Rect> {
        let result = self.send(&shape_get_rectangles_request(Self::ORDER, Self::WINDOW, kind));
        match result.outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::ShapeGetRectangles { rects, ordering, .. })] => {
                assert_eq!(*ordering, X_SHAPE_ORDERING_YX_BANDED);
                rects.clone()
            }
            other => panic!("GetRectangles produced {other:?}"),
        }
    }

    /// Whether each kind reports itself shaped, and its extents.
    fn extents(&mut self) -> (bool, bool, Rect) {
        let result = self.send(&shape_query_extents_request(Self::ORDER, Self::WINDOW));
        match result.outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::ShapeQueryExtents {
                bounding_shaped,
                clip_shaped,
                bounding_extents,
                ..
            })] => (*bounding_shaped, *clip_shaped, *bounding_extents),
            other => panic!("QueryExtents produced {other:?}"),
        }
    }

    fn notify_of(result: &XDispatchResult) -> Option<(u8, bool, Rect)> {
        result.outputs.iter().find_map(|output| match output {
            XClientOutput::Event(XClientEvent::ShapeNotify {
                kind,
                shaped,
                extents,
                ..
            }) => Some((*kind, *shaped, *extents)),
            _ => None,
        })
    }

    fn error_of(result: &XDispatchResult) -> Option<XErrorCode> {
        result.outputs.iter().find_map(|output| match output {
            XClientOutput::Error(error) => Some(error.code),
            _ => None,
        })
    }
}

/// Unset, explicitly empty, and concrete are three different answers.
///
/// An unset kind reports the window's live bounds, so a resize moves the
/// answer without anything writing the store -- materialising the default
/// instead freezes an extent that stops tracking geometry, which is the bug
/// this tri-state exists to avoid. An explicitly empty shape is a client
/// asking for nothing, and must not read back as unset.
#[test]
fn shape_distinguishes_unset_from_empty_from_concrete() {
    let mut fixture = ShapeFixture::new();

    // Unset: the window's own bounds, and not reported as shaped.
    let (bounding_shaped, clip_shaped, extents) = fixture.extents();
    assert!(!bounding_shaped, "an untouched window is not shaped");
    assert!(!clip_shaped);
    assert_eq!(
        extents,
        Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10
        }
    );
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_BOUNDING),
        vec![Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10
        }],
        "an unset kind answers with the window rectangle"
    );

    // Explicitly empty: shaped, with nothing in it.
    let result = fixture.set(X_SHAPE_KIND_BOUNDING, &[]);
    assert_eq!(ShapeFixture::error_of(&result), None);
    let (shaped, _, extents) = fixture.extents();
    assert!(shaped, "an empty shape is still a shape");
    assert_eq!(extents, Rect::default(), "and its extents are nothing");
    assert!(fixture.rects(X_SHAPE_KIND_BOUNDING).is_empty());

    // Concrete.
    let half = Rect {
        x: 0,
        y: 0,
        width: 10,
        height: 5,
    };
    fixture.set(X_SHAPE_KIND_BOUNDING, &[half]);
    let (shaped, _, extents) = fixture.extents();
    assert!(shaped);
    assert_eq!(extents, half);
    assert_eq!(fixture.rects(X_SHAPE_KIND_BOUNDING), vec![half]);

    // The three kinds are independent: shaping bounding left input alone.
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_INPUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10
        }]
    );
}

/// Each operation combines the way the protocol defines, including the two
/// that differ only in which side is subtracted.
///
/// Invert is the source with the destination taken out of it -- the mirror of
/// Subtract. At least one other implementation aliases it to Set, which is
/// silently wrong for any client that uses it.
#[test]
fn shape_operations_combine_as_the_protocol_defines() {
    let left = Rect {
        x: 0,
        y: 0,
        width: 6,
        height: 10,
    };
    let right = Rect {
        x: 4,
        y: 0,
        width: 6,
        height: 10,
    };

    let cases: &[(u8, &[Rect])] = &[
        (X_SHAPE_OP_SET, &[right]),
        (
            X_SHAPE_OP_UNION,
            &[Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            }],
        ),
        (
            X_SHAPE_OP_INTERSECT,
            &[Rect {
                x: 4,
                y: 0,
                width: 2,
                height: 10,
            }],
        ),
        // Subtract: the destination without the source.
        (
            X_SHAPE_OP_SUBTRACT,
            &[Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 10,
            }],
        ),
        // Invert: the source without the destination.
        (
            X_SHAPE_OP_INVERT,
            &[Rect {
                x: 6,
                y: 0,
                width: 4,
                height: 10,
            }],
        ),
    ];

    for (op, expected) in cases {
        let mut fixture = ShapeFixture::new();
        fixture.set(X_SHAPE_KIND_BOUNDING, &[left]);
        let result = fixture.send(&shape_rectangles_request(
            ShapeFixture::ORDER,
            *op,
            X_SHAPE_KIND_BOUNDING,
            X_SHAPE_ORDERING_UNSORTED,
            ShapeFixture::WINDOW,
            0,
            0,
            &[right],
        ));
        assert_eq!(ShapeFixture::error_of(&result), None, "op {op}");
        assert_eq!(fixture.rects(X_SHAPE_KIND_BOUNDING), *expected, "op {op}");
    }
}

/// An operation against a kind that was never set becomes Set.
///
/// There is nothing to combine with, and combining against the default the
/// client never asked for would answer a question it did not pose.
#[test]
fn shape_operations_on_an_unset_kind_become_set() {
    let quarter = Rect {
        x: 0,
        y: 0,
        width: 5,
        height: 5,
    };
    for op in [
        X_SHAPE_OP_UNION,
        X_SHAPE_OP_INTERSECT,
        X_SHAPE_OP_SUBTRACT,
        X_SHAPE_OP_INVERT,
    ] {
        let mut fixture = ShapeFixture::new();
        let result = fixture.send(&shape_rectangles_request(
            ShapeFixture::ORDER,
            op,
            X_SHAPE_KIND_BOUNDING,
            X_SHAPE_ORDERING_UNSORTED,
            ShapeFixture::WINDOW,
            0,
            0,
            &[quarter],
        ));
        assert_eq!(ShapeFixture::error_of(&result), None, "op {op}");
        assert_eq!(fixture.rects(X_SHAPE_KIND_BOUNDING), vec![quarter], "op {op}");
    }
}

/// A shape that does not move produces no event.
///
/// Window managers re-assert the same shape constantly; a notify for each
/// re-assertion is what broke panel buttons in the implementation this
/// gating was taken from. Re-asserting the same area written differently
/// must also stay silent, which is what canonical region equality buys.
#[test]
fn shape_notify_fires_only_when_the_shape_actually_moves() {
    let mut fixture = ShapeFixture::new();
    let whole = Rect {
        x: 0,
        y: 0,
        width: 8,
        height: 8,
    };

    let first = fixture.set(X_SHAPE_KIND_BOUNDING, &[whole]);
    let (kind, shaped, extents) =
        ShapeFixture::notify_of(&first).expect("the first shape is a change");
    assert_eq!(kind, X_SHAPE_KIND_BOUNDING);
    assert!(shaped);
    assert_eq!(extents, whole);

    // The same shape again: nothing.
    let repeat = fixture.set(X_SHAPE_KIND_BOUNDING, &[whole]);
    assert!(ShapeFixture::notify_of(&repeat).is_none(), "re-assertion");

    // The same area, written as four pieces: still nothing, because the
    // region is compared as a region and not as a list.
    let quarters = [
        Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        },
        Rect {
            x: 4,
            y: 0,
            width: 4,
            height: 4,
        },
        Rect {
            x: 0,
            y: 4,
            width: 4,
            height: 4,
        },
        Rect {
            x: 4,
            y: 4,
            width: 4,
            height: 4,
        },
    ];
    let respelled = fixture.set(X_SHAPE_KIND_BOUNDING, &quarters);
    assert!(
        ShapeFixture::notify_of(&respelled).is_none(),
        "the same area spelled differently is not a change"
    );

    // A real change fires.
    let moved = fixture.set(
        X_SHAPE_KIND_BOUNDING,
        &[Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 4,
        }],
    );
    assert!(ShapeFixture::notify_of(&moved).is_some());

    // Returning to unset covers the same area as the default here would not
    // -- but the kind going from set to unset is itself a change, because
    // QueryExtents reports it differently.
    let mut fixture = ShapeFixture::new();
    let full = Rect {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
    };
    let set_to_default = fixture.set(X_SHAPE_KIND_BOUNDING, &[full]);
    assert!(
        ShapeFixture::notify_of(&set_to_default).is_some(),
        "setting a kind is a change even at the default area"
    );
    let cleared = fixture.send(&shape_mask_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        0,
    ));
    let (_, shaped, _) = ShapeFixture::notify_of(&cleared).expect("unsetting is a change");
    assert!(!shaped, "and it reports the kind as no longer shaped");
    assert!(!fixture.extents().0);
}

/// A mask's set bits become the shape.
#[test]
fn shape_mask_reads_a_depth_one_pixmap() {
    let mut fixture = ShapeFixture::new();
    let create = create_pixmap_request(
        ShapeFixture::ORDER,
        1,
        ShapeFixture::MASK,
        X_SETUP_DEFAULT_ROOT,
        4,
        2,
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create)), None);

    // Two rows: the left half set on the first, nothing on the second.
    let gc = 0x0020_0410;
    let create_gc = create_gc_request(ShapeFixture::ORDER, gc, ShapeFixture::MASK);
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create_gc)), None);
    // A depth-1 ZPixmap packs one bit per pixel, least significant first,
    // with rows padded to four bytes: the first row has its two leftmost
    // pixels set, the second has none.
    let data: Vec<u8> = vec![0b0000_0011, 0, 0, 0, 0, 0, 0, 0];
    let put = put_image_request_at_depth(
        ShapeFixture::ORDER,
        1,
        ShapeFixture::MASK,
        gc,
        4,
        2,
        &data,
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&put)), None, "mask upload");

    let result = fixture.send(&shape_mask_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        ShapeFixture::MASK,
    ));
    assert_eq!(ShapeFixture::error_of(&result), None);
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_BOUNDING),
        vec![Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 1
        }],
        "only the set bits, as one banded rectangle"
    );

    // A mask that is not one bit deep cannot describe a shape.
    let deep = 0x0020_0411;
    let create_deep = create_pixmap_request(
        ShapeFixture::ORDER,
        24,
        deep,
        X_SETUP_DEFAULT_ROOT,
        4,
        4,
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create_deep)), None);
    let refused = fixture.send(&shape_mask_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        deep,
    ));
    assert_eq!(
        ShapeFixture::error_of(&refused),
        Some(XErrorCode::BadMatch),
        "a depth-24 mask"
    );
}

/// Combine sources another window's shape; Offset moves one; both validate.
#[test]
fn shape_combine_and_offset_use_another_windows_shape() {
    let mut fixture = ShapeFixture::new();
    let create = create_window_request(ShapeFixture::ORDER, ShapeFixture::OTHER, 0, 0, 4, 4);
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create)), None);

    // The other window is unset, so its effective shape is its own bounds.
    let combined = fixture.send(&shape_combine_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        ShapeFixture::OTHER,
    ));
    assert_eq!(ShapeFixture::error_of(&combined), None);
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_BOUNDING),
        vec![Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4
        }]
    );

    let offset = fixture.send(&shape_offset_request(
        ShapeFixture::ORDER,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        3,
        2,
    ));
    assert_eq!(ShapeFixture::error_of(&offset), None);
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_BOUNDING),
        vec![Rect {
            x: 3,
            y: 2,
            width: 4,
            height: 4
        }]
    );

    // Offsetting a kind that was never set leaves it unset rather than
    // materialising the default somewhere the client never put it.
    let untouched = fixture.send(&shape_offset_request(
        ShapeFixture::ORDER,
        X_SHAPE_KIND_INPUT,
        ShapeFixture::WINDOW,
        5,
        5,
    ));
    assert_eq!(ShapeFixture::error_of(&untouched), None);
    assert!(ShapeFixture::notify_of(&untouched).is_none());
    assert_eq!(
        fixture.rects(X_SHAPE_KIND_INPUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 10
        }],
        "still the window's own bounds"
    );
}

/// Selection is per client and per window, and answers itself.
#[test]
fn shape_input_selection_is_per_client_and_window() {
    let mut fixture = ShapeFixture::new();
    let selected = |fixture: &mut ShapeFixture, client: u64| {
        let request = shape_input_selected_request(ShapeFixture::ORDER, ShapeFixture::WINDOW);
        match fixture.send_as(client, &request).outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::ShapeInputSelected { enabled, .. })] => *enabled,
            other => panic!("InputSelected produced {other:?}"),
        }
    };

    assert!(!selected(&mut fixture, ShapeFixture::CLIENT));
    let select = shape_select_input_request(ShapeFixture::ORDER, ShapeFixture::WINDOW, true);
    assert!(fixture.send(&select).outputs.is_empty());
    assert!(selected(&mut fixture, ShapeFixture::CLIENT));
    // Another client's interest is its own.
    assert!(!selected(&mut fixture, ShapeFixture::CLIENT + 1));

    let deselect = shape_select_input_request(ShapeFixture::ORDER, ShapeFixture::WINDOW, false);
    assert!(fixture.send(&deselect).outputs.is_empty());
    assert!(!selected(&mut fixture, ShapeFixture::CLIENT));
}

/// Bad arguments are refused with the code the protocol expects.
#[test]
fn shape_refuses_invalid_windows_kinds_and_operations() {
    let mut fixture = ShapeFixture::new();
    let unknown = 0x0020_04ff;

    let bad_window = fixture.send(&shape_rectangles_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        X_SHAPE_ORDERING_UNSORTED,
        unknown,
        0,
        0,
        &[],
    ));
    assert_eq!(
        ShapeFixture::error_of(&bad_window),
        Some(XErrorCode::BadWindow)
    );

    for (kind, op, ordering) in [(9, X_SHAPE_OP_SET, 0), (X_SHAPE_KIND_BOUNDING, 9, 0), (X_SHAPE_KIND_BOUNDING, X_SHAPE_OP_SET, 9)] {
        let result = fixture.send(&shape_rectangles_request(
            ShapeFixture::ORDER,
            op,
            kind,
            ordering,
            ShapeFixture::WINDOW,
            0,
            0,
            &[],
        ));
        assert_eq!(
            ShapeFixture::error_of(&result),
            Some(XErrorCode::BadValue),
            "kind {kind} op {op} ordering {ordering}"
        );
    }

    // No version of SHAPE has a minor above eight.
    for minor in [9, 200] {
        let result = fixture.send(&shape_minor_request(ShapeFixture::ORDER, minor));
        assert_eq!(
            ShapeFixture::error_of(&result),
            Some(XErrorCode::BadRequest),
            "minor {minor}"
        );
    }
}

/// SHAPE answers its requests and is advertised, with its event base.
///
/// The advertisement waited for input shapes to genuinely make clicks fall
/// through, because that is what the client asking for this extension does
/// with it.
#[test]
fn shape_is_advertised_once_its_shapes_take_effect() {
    let mut fixture = ShapeFixture::new();
    let version = fixture.send(&shape_minor_request(
        ShapeFixture::ORDER,
        X_SHAPE_QUERY_VERSION_MINOR_OPCODE,
    ));
    assert!(
        matches!(
            version.outputs.as_slice(),
            [XClientOutput::Reply(XClientReply::ShapeQueryVersion {
                major_version: 1,
                minor_version: 1,
                ..
            })]
        ),
        "{:?}",
        version.outputs
    );

    let query = fixture.send(&query_extension_request(
        ShapeFixture::ORDER,
        X_SHAPE_EXTENSION_NAME,
    ));
    let encoded = query.encoded_outputs(ShapeFixture::ORDER);
    assert_eq!(encoded[0][8], 1, "present");
    assert_eq!(encoded[0][9], X_SHAPE_MAJOR_OPCODE);
    assert_eq!(encoded[0][10], X_SHAPE_FIRST_EVENT, "event base");
}

/// A bounding shape clips what the window presents, through the alpha the
/// renderer already blends.
///
/// The presentation buffer is published as ARGB with the shaped-out pixels
/// cleared to fully transparent, which is what makes the hole show the
/// desktop rather than a black patch. Asserting the exact bytes is what
/// distinguishes a shape that clips from a shape that is merely stored.
#[test]
fn a_bounding_shape_clears_what_the_window_presents_outside_it() {
    let mut fixture = ShapeFixture::new();
    let gc = 0x0020_0420;
    let create_gc = create_gc_request(ShapeFixture::ORDER, gc, ShapeFixture::WINDOW);
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create_gc)), None);

    // Fill the window so there is something to clip.
    let fill = poly_fill_rectangle_request(
        ShapeFixture::ORDER,
        ShapeFixture::WINDOW,
        gc,
        &[(0, 0, 10, 10)],
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&fill)), None);

    // Shape it to its left half.
    let result = fixture.set(
        X_SHAPE_KIND_BOUNDING,
        &[Rect {
            x: 0,
            y: 0,
            width: 5,
            height: 10,
        }],
    );
    assert_eq!(ShapeFixture::error_of(&result), None);
    assert!(
        result.response.is_some(),
        "a shape change republishes the window rather than waiting for a draw"
    );

    // The masked pixels live in the presentation buffer the update carries,
    // which is what actually ships, rather than in the window's own backing.
    let updates = fixture.runtime.take_cpu_buffer_updates();
    let bytes = shaped_presentation_bytes(&updates).expect("a shaped presentation shipped");
    // Row zero: five opaque pixels, then five cleared ones.
    for x in 0..5 {
        assert_eq!(bytes[x * 4 + 3], 0xff, "pixel {x} is inside the shape");
    }
    for x in 5..10 {
        assert_eq!(
            &bytes[x * 4..x * 4 + 4],
            &[0, 0, 0, 0],
            "pixel {x} is outside the shape, and transparent rather than black"
        );
    }
}

/// The bytes and format of the last shaped presentation an update list
/// carries, so a test can assert on what actually ships.
fn shaped_presentation_bytes(updates: &[XAuthorityCpuBufferUpdate]) -> Option<Vec<u8>> {
    updates.iter().rev().find_map(|update| match update {
        XAuthorityCpuBufferUpdate::Replace(snapshot)
            if snapshot.format == X_AUTHORITY_CPU_BUFFER_FORMAT_ARGB8888 =>
        {
            Some(snapshot.bytes.to_vec())
        }
        XAuthorityCpuBufferUpdate::PatchBatch(batch)
            if batch.format == X_AUTHORITY_CPU_BUFFER_FORMAT_ARGB8888 =>
        {
            batch.patches.first().map(|patch| patch.bytes.clone())
        }
        _ => None,
    })
}

/// Shaping and unshaping moves the published buffer between the alpha and
/// opaque formats.
///
/// The transport shape is the store's business -- it replaces the whole
/// buffer when the format moves, which `crossing_the_format_boundary_ships_a_whole_buffer`
/// pins directly. What matters here is that the format a client's shape
/// request produces reaches the update that ships.
#[test]
fn crossing_between_shaped_and_unshaped_moves_the_published_format() {
    let mut fixture = ShapeFixture::new();
    let gc = 0x0020_0421;
    let create_gc = create_gc_request(ShapeFixture::ORDER, gc, ShapeFixture::WINDOW);
    assert_eq!(ShapeFixture::error_of(&fixture.send(&create_gc)), None);
    let fill = poly_fill_rectangle_request(
        ShapeFixture::ORDER,
        ShapeFixture::WINDOW,
        gc,
        &[(0, 0, 10, 10)],
    );
    assert_eq!(ShapeFixture::error_of(&fixture.send(&fill)), None);

    let shaped = fixture.set(
        X_SHAPE_KIND_BOUNDING,
        &[Rect {
            x: 0,
            y: 0,
            width: 5,
            height: 10,
        }],
    );
    assert!(shaped.response.is_some());
    let updates = fixture.runtime.take_cpu_buffer_updates();
    let shaped_update = updates
        .iter()
        .find(|update| update.format() == X_AUTHORITY_CPU_BUFFER_FORMAT_ARGB8888)
        .expect("a shaped window publishes in the alpha format");
    // The whole buffer has to cross the transition, not a patch of it. Every
    // pixel already in the buffer was written under the opaque format, and
    // one left uncovered would be read as though it carried alpha.
    assert_eq!(
        shaped_update.size(),
        Size {
            width: 10,
            height: 10
        },
        "the transition covers the whole buffer"
    );

    // And back: unshaping returns the buffer to the opaque format.
    let unshaped = fixture.send(&shape_mask_request(
        ShapeFixture::ORDER,
        X_SHAPE_OP_SET,
        X_SHAPE_KIND_BOUNDING,
        ShapeFixture::WINDOW,
        0,
        0,
        0,
    ));
    assert!(unshaped.response.is_some());
    let updates = fixture.runtime.take_cpu_buffer_updates();
    assert!(
        updates
            .iter()
            .any(|update| update.format() == X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888),
        "unshaping publishes in the opaque format again: {updates:?}"
    );
}

/// A window nobody has drawn into is not republished.
///
/// There is no presentation buffer to reshape, and inventing one would ship
/// a frame for a window that has never had content.
#[test]
fn shaping_a_window_that_has_never_drawn_publishes_nothing() {
    let mut fixture = ShapeFixture::new();
    let result = fixture.set(
        X_SHAPE_KIND_BOUNDING,
        &[Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        }],
    );
    assert_eq!(ShapeFixture::error_of(&result), None);
    assert!(
        ShapeFixture::notify_of(&result).is_some(),
        "the shape still changed, and subscribers are still told"
    );
    assert!(
        result.response.is_none(),
        "but there is no frame to republish"
    );
}
