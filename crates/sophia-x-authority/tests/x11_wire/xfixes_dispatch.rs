// XFIXES regions through dispatch: the fixture, combining, inverting,
// translating, extents, fetches, window shapes, GC clips and bitmaps.
// Included from x11_wire.rs beside extensions_dispatch.rs (t026).

/// A fixture driving XFIXES region requests against one runtime.
struct XfixesRegionFixture {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
}

impl XfixesRegionFixture {
    const NS: NamespaceId = NamespaceId::from_raw(91);
    const ORDER: XByteOrder = XByteOrder::LittleEndian;
    const A: u32 = 0x0020_0300;
    const B: u32 = 0x0020_0301;
    const OUT: u32 = 0x0020_0302;

    fn new() -> Self {
        Self {
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            sequence: 0,
        }
    }

    fn send(&mut self, bytes: &[u8]) -> XDispatchResult {
        self.sequence = self.sequence.wrapping_add(1);
        let request = decode_x11_core_request(
            context(Self::NS, u64::from(self.sequence) + 1200, Self::ORDER),
            bytes,
        )
        .expect("request must decode");
        dispatch_x11_wire_request(
            dispatch_context(Self::NS, self.sequence, Self::ORDER, bytes[0]),
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        )
    }

    fn create(&mut self, id: u32, rects: &[Rect]) {
        let request = xfixes_create_region_request(Self::ORDER, id, rects);
        assert!(self.send(&request).outputs.is_empty(), "create {id:#x}");
    }

    /// The region's rectangles, read back the way a client reads them.
    fn fetch(&mut self, id: u32) -> Vec<Rect> {
        let result = self.send(&xfixes_fetch_region_request(Self::ORDER, id));
        match result.outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::XfixesFetchRegion { rects, .. })] => rects.clone(),
            other => panic!("fetch produced {other:?}"),
        }
    }

    fn error_of(result: &XDispatchResult) -> Option<XErrorCode> {
        result.outputs.iter().find_map(|output| match output {
            XClientOutput::Error(error) => Some(error.code),
            _ => None,
        })
    }
}

/// The region operations combine what the client asked them to, and the
/// result reads back canonically.
///
/// XFIXES has answered version 6.0 since before these existed, so a client
/// that believed the version and tried to compute with a region got a parse
/// failure. These are the minors that make a region a value rather than a
/// container.
#[test]
fn xfixes_regions_combine_and_read_back_canonically() {
    let mut fixture = XfixesRegionFixture::new();
    let left = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    let right = Rect {
        x: 2,
        y: 0,
        width: 4,
        height: 4,
    };
    fixture.create(XfixesRegionFixture::A, &[left]);
    fixture.create(XfixesRegionFixture::B, &[right]);
    fixture.create(XfixesRegionFixture::OUT, &[]);

    let union = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_UNION_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&union), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 6,
            height: 4,
        }],
        "the union is one rect, not two overlapping ones"
    );

    let intersect = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_INTERSECT_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&intersect), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 2,
            y: 0,
            width: 2,
            height: 4,
        }]
    );

    let subtract = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_SUBTRACT_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&subtract), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 4,
        }]
    );

    // Copy carries one source across and canonicalises on the way.
    let copy = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_COPY_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        0,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&copy), None);
    assert_eq!(fixture.fetch(XfixesRegionFixture::OUT), vec![left]);
}

/// A destination that names one of its own sources still means what the
/// client asked.
///
/// `UnionRegion(a, b, a)` is ordinary client code, and an implementation
/// that wrote the destination while still reading it would answer from
/// half-updated state.
#[test]
fn xfixes_region_operations_allow_the_destination_to_be_a_source() {
    let mut fixture = XfixesRegionFixture::new();
    fixture.create(
        XfixesRegionFixture::A,
        &[Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        }],
    );
    fixture.create(
        XfixesRegionFixture::B,
        &[Rect {
            x: 4,
            y: 0,
            width: 4,
            height: 4,
        }],
    );
    let result = fixture.send(&xfixes_combine_region_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_UNION_REGION_MINOR_OPCODE,
        XfixesRegionFixture::A,
        XfixesRegionFixture::B,
        XfixesRegionFixture::A,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&result), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 0,
            y: 0,
            width: 8,
            height: 4,
        }]
    );
}

/// Invert, translate and extents answer what the protocol defines.
#[test]
fn xfixes_invert_translate_and_extents_answer_the_protocol() {
    let mut fixture = XfixesRegionFixture::new();
    // A hole in the middle of a square: invert is the source subtracted from
    // the bounds the client supplies, because a region has no complement
    // without them.
    fixture.create(
        XfixesRegionFixture::A,
        &[Rect {
            x: 2,
            y: 2,
            width: 2,
            height: 2,
        }],
    );
    fixture.create(XfixesRegionFixture::OUT, &[]);
    let invert = fixture.send(&xfixes_invert_region_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::A,
        Rect {
            x: 0,
            y: 0,
            width: 6,
            height: 6,
        },
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&invert), None);
    let frame = fixture.fetch(XfixesRegionFixture::OUT);
    let area: i32 = frame.iter().map(|r| r.width * r.height).sum();
    assert_eq!(area, 32, "a frame, not the whole square and not nothing");

    let translate = fixture.send(&xfixes_translate_region_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::A,
        10,
        20,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&translate), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 12,
            y: 22,
            width: 2,
            height: 2,
        }]
    );

    fixture.create(
        XfixesRegionFixture::B,
        &[
            Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            },
            Rect {
                x: 8,
                y: 6,
                width: 2,
                height: 2,
            },
        ],
    );
    let extents = fixture.send(&xfixes_region_extents_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&extents), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 8,
        }]
    );
}

/// An XFIXES minor with no implementation is refused by name rather than
/// failing to parse.
///
/// This server answers XFIXES 6.0 and does not implement every minor behind
/// it. A parse rejection told a client only that the extension existed; a
/// named refusal says which request was declined, which is the discipline
/// every other extension here follows.
#[test]
fn xfixes_minors_without_an_implementation_are_refused_by_name() {
    let mut fixture = XfixesRegionFixture::new();
    // Defined by version 6.0, not implemented here. Minors 6 to 9 have left
    // this list; 20 to 22 install a region rather than build one and still
    // await the clip and shape plumbing.
    for minor in [20, 21, 22, 29, 32, 34] {
        let result = fixture.send(&xfixes_minor_request(XfixesRegionFixture::ORDER, minor));
        match result.outputs.as_slice() {
            [XClientOutput::Error(error)] => {
                assert_eq!(error.code, XErrorCode::BadImplementation, "minor {minor}");
                assert_eq!(error.minor_code, u16::from(minor));
                assert_eq!(error.major_code, X_XFIXES_MAJOR_OPCODE);
            }
            other => panic!("minor {minor} produced {other:?}"),
        }
    }
    // Beyond anything the version defines.
    for minor in [35, 200] {
        let result = fixture.send(&xfixes_minor_request(XfixesRegionFixture::ORDER, minor));
        assert_eq!(
            XfixesRegionFixture::error_of(&result),
            Some(XErrorCode::BadRequest),
            "minor {minor}"
        );
    }
}

/// The FetchRegion reply is read by a client at fixed offsets, so its bytes
/// are pinned.
///
/// The reply carries extents at bytes 8 through 16 and no rectangle count at
/// all -- a client derives the count from the reply's length. This encoder
/// previously wrote a count where the extents' x belongs, so every client
/// read the number of rectangles as a coordinate and zero for the rest of the
/// bounding box. The test that existed destructured the reply before it was
/// encoded and could not see it.
#[test]
fn xfixes_fetch_region_reply_matches_the_bytes_a_client_reads() {
    let mut fixture = XfixesRegionFixture::new();
    fixture.create(
        XfixesRegionFixture::A,
        &[
            Rect {
                x: 3,
                y: 4,
                width: 5,
                height: 6,
            },
            Rect {
                x: 20,
                y: 30,
                width: 2,
                height: 2,
            },
        ],
    );
    let request = xfixes_fetch_region_request(XfixesRegionFixture::ORDER, XfixesRegionFixture::A);
    let result = fixture.send(&request);
    let encoded = result.encoded_outputs(XfixesRegionFixture::ORDER);
    let reply = &encoded[0];

    // Two rectangles: a 32-byte header and two eight-byte rectangles, with the
    // length counting only what follows the header, in four-byte units.
    assert_eq!(reply.len(), 32 + 2 * 8);
    assert_eq!(reply[0], 1, "a reply, not an event");
    assert_eq!(read_u32(XfixesRegionFixture::ORDER, &reply[4..8]), 4);

    // Extents, at the offsets the protocol puts them.
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[8..10]), 3, "x");
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[10..12]), 4, "y");
    assert_eq!(
        read_u16(XfixesRegionFixture::ORDER, &reply[12..14]),
        19,
        "width spans both rectangles"
    );
    assert_eq!(
        read_u16(XfixesRegionFixture::ORDER, &reply[14..16]),
        28,
        "height spans both rectangles"
    );
    // The rest of the header is padding, and must not carry a count.
    assert!(
        reply[16..32].iter().all(|byte| *byte == 0),
        "bytes 16 through 32 are padding: {:?}",
        &reply[16..32]
    );

    // Then the rectangles themselves.
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[32..34]), 3);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[34..36]), 4);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[36..38]), 5);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[38..40]), 6);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[40..42]), 20);
    assert_eq!(read_u16(XfixesRegionFixture::ORDER, &reply[42..44]), 30);
}

/// The version answered is the lower of the client's and this server's.
///
/// It answered its own version regardless of what was asked. A client that
/// asked for version 1 was told 6, and would then be entitled to send
/// requests no version 1 client should know about.
#[test]
fn xfixes_query_version_answers_the_lower_of_the_two() {
    let mut fixture = XfixesRegionFixture::new();
    let ask = |fixture: &mut XfixesRegionFixture, major: u32, minor: u32| {
        let mut out = vec![X_XFIXES_MAJOR_OPCODE, X_XFIXES_QUERY_VERSION_MINOR_OPCODE];
        push_u16(&mut out, XfixesRegionFixture::ORDER, 3);
        push_u32(&mut out, XfixesRegionFixture::ORDER, major);
        push_u32(&mut out, XfixesRegionFixture::ORDER, minor);
        let encoded = fixture.send(&out).encoded_outputs(XfixesRegionFixture::ORDER);
        (
            read_u32(XfixesRegionFixture::ORDER, &encoded[0][8..12]),
            read_u32(XfixesRegionFixture::ORDER, &encoded[0][12..16]),
        )
    };

    // A client below the server is answered its own version.
    assert_eq!(ask(&mut fixture, 2, 0), (2, 0));
    assert_eq!(ask(&mut fixture, 1, 0), (1, 0));
    // A client above the server is answered the server's.
    assert_eq!(ask(&mut fixture, 9, 0), (6, 0));
    // Equal majors take the lower minor.
    assert_eq!(ask(&mut fixture, 6, 0), (6, 0));
}

/// A region can be built from a window's shape, and Input is not one of the
/// shapes it may be built from.
#[test]
fn xfixes_builds_a_region_from_a_window_shape() {
    let window = 0x0020_0800;
    let mut fixture = XfixesRegionFixture::new();
    let create = create_window_request(XfixesRegionFixture::ORDER, window, 5, 7, 40, 30);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&create)), None);

    // An unshaped window reports its own bounds.
    let from_window = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE,
        XfixesRegionFixture::A,
        window,
        X_XFIXES_WINDOW_REGION_BOUNDING,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&from_window), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 30,
        }],
        "an unshaped window's region is the window rectangle"
    );

    // XFIXES builds a region from the bounding or clip shape and no other.
    // SHAPE has a third kind and this request does not accept it.
    let input_kind = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        window,
        2,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&input_kind),
        Some(XErrorCode::BadValue)
    );

    // A window that does not exist is named before the kind is judged.
    let unknown = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        0x0020_08ff,
        2,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&unknown),
        Some(XErrorCode::BadWindow),
        "the missing window is reported before the bad kind"
    );
}

/// A region built from a graphics context copies that context's clip
/// rectangles as they are stored.
///
/// The clip origins describe how the graphics context *uses* its clip, not
/// where the rectangles are, so folding them in here would apply them a second
/// time if the region were installed as a clip again. Nonzero stored origins
/// must therefore leave the extracted region unchanged.
#[test]
fn xfixes_copies_a_graphics_context_clip() {
    let gc = 0x0020_0810;
    let pixmap = 0x0020_0811;
    let window = 0x0020_0812;
    let mut fixture = XfixesRegionFixture::new();
    let create_window = create_window_request(XfixesRegionFixture::ORDER, window, 0, 0, 32, 32);
    assert_eq!(
        XfixesRegionFixture::error_of(&fixture.send(&create_window)),
        None
    );
    let create_pixmap =
        create_pixmap_request(XfixesRegionFixture::ORDER, 24, pixmap, window, 32, 32);
    assert_eq!(
        XfixesRegionFixture::error_of(&fixture.send(&create_pixmap)),
        None
    );
    let create_gc = create_gc_request(XfixesRegionFixture::ORDER, gc, pixmap);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&create_gc)), None);

    // A graphics context with no clip has nothing to copy, which is not the
    // same as the graphics context being absent.
    let no_clip = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        gc,
        0,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&no_clip),
        Some(XErrorCode::BadMatch)
    );

    let mut clip = set_clip_rectangles_request(XfixesRegionFixture::ORDER, gc, &[(1, 2, 3, 4)]);
    clip[8..10].copy_from_slice(&(-5_i16).to_le_bytes());
    clip[10..12].copy_from_slice(&7_i16.to_le_bytes());
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&clip)), None);
    let from_gc = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE,
        XfixesRegionFixture::A,
        gc,
        0,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&from_gc), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 1,
            y: 2,
            width: 3,
            height: 4,
        }],
        "the stored clip rectangles arrive unmoved"
    );

    // An absent graphics context is named as such.
    let unknown = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        0x0020_08fe,
        0,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&unknown),
        Some(XErrorCode::BadGraphicsContext)
    );
}

/// Expanding grows every rectangle, and an empty source leaves the
/// destination alone.
#[test]
fn xfixes_expands_a_region_and_leaves_an_empty_one_alone() {
    let mut fixture = XfixesRegionFixture::new();
    fixture.create(
        XfixesRegionFixture::A,
        &[Rect {
            x: 10,
            y: 10,
            width: 4,
            height: 4,
        }],
    );
    fixture.create(XfixesRegionFixture::B, &[]);
    fixture.create(
        XfixesRegionFixture::OUT,
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
    );

    let expand = fixture.send(&xfixes_expand_region_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::A,
        XfixesRegionFixture::OUT,
        1,
        2,
        3,
        4,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&expand), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 9,
            y: 7,
            width: 7,
            height: 11,
        }]
    );

    // An empty source leaves the destination as it was, rather than emptying
    // it -- the edge a reimplementation gets backwards.
    let empty = fixture.send(&xfixes_expand_region_request(
        XfixesRegionFixture::ORDER,
        XfixesRegionFixture::B,
        XfixesRegionFixture::OUT,
        5,
        5,
        5,
        5,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&empty), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: 9,
            y: 7,
            width: 7,
            height: 11,
        }],
        "expanding nothing changes nothing"
    );

    // Rectangles that grow into each other merge. Expanding each one and
    // stopping there would answer with two overlapping rectangles, which is
    // not a region -- the result has to be unioned back into canonical form.
    let pair = 0x0020_0303;
    fixture.create(
        pair,
        &[
            Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            },
            Rect {
                x: 10,
                y: 0,
                width: 4,
                height: 4,
            },
        ],
    );
    let merged = fixture.send(&xfixes_expand_region_request(
        XfixesRegionFixture::ORDER,
        pair,
        XfixesRegionFixture::OUT,
        4,
        4,
        4,
        4,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&merged), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::OUT),
        vec![Rect {
            x: -4,
            y: -4,
            width: 22,
            height: 12,
        }],
        "two grown rectangles that meet become one"
    );
}

/// Every XFIXES minor either fails to decode or is answered. None escapes to
/// the fallthrough.
///
/// A decoder with no dispatcher behind it is worse than no decoder at all: the
/// request decodes, misses every family matcher, and reaches
/// `unreachable!("extension request escaped its family dispatcher")`, which
/// takes the server down on input a client is free to send. That is not a
/// refusal, it is a crash, and it is reachable from any unprivileged client.
/// This sweep walks the whole minor range at each minor's real request length
/// so the decode-then-dispatch path actually runs for every one of them.
#[test]
fn every_xfixes_minor_is_answered_rather_than_escaping_dispatch() {
    // Minors whose decoders demand an exact length, sent at that length so
    // decoding succeeds and dispatch is genuinely exercised.
    let sized: &[(u8, usize)] = &[
        (X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE, 12),
        (X_XFIXES_CREATE_REGION_FROM_WINDOW_MINOR_OPCODE, 16),
        (X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE, 12),
        (X_XFIXES_CREATE_REGION_FROM_PICTURE_MINOR_OPCODE, 12),
        (X_XFIXES_EXPAND_REGION_MINOR_OPCODE, 20),
    ];

    for minor in 0..=X_XFIXES_LAST_MINOR_OPCODE {
        let length = sized
            .iter()
            .find(|(opcode, _)| *opcode == minor)
            .map_or(4, |(_, length)| *length);
        let mut bytes = vec![X_XFIXES_MAJOR_OPCODE, minor];
        push_u16(&mut bytes, XfixesRegionFixture::ORDER, (length / 4) as u16);
        bytes.resize(length, 0);

        let mut fixture = XfixesRegionFixture::new();
        let decoded = decode_x11_core_request(
            context(XfixesRegionFixture::NS, 9000 + u64::from(minor), XfixesRegionFixture::ORDER),
            &bytes,
        );
        let Ok(request) = decoded else {
            // Refusing to decode is a fine answer; it becomes a wire error.
            continue;
        };
        // The assertion is that this returns at all. If the minor decoded
        // without a dispatcher, this call panics.
        let result = dispatch_x11_wire_request(
            dispatch_context(XfixesRegionFixture::NS, minor.into(), XfixesRegionFixture::ORDER, X_XFIXES_MAJOR_OPCODE),
            request,
            &mut fixture.runtime,
            &mut fixture.atoms,
            &mut fixture.properties,
        );
        assert!(
            !result.outputs.is_empty() || minor == X_XFIXES_DESTROY_REGION_MINOR_OPCODE,
            "minor {minor} produced no answer at all"
        );
    }
}

/// A region built from a depth-one bitmap is the bitmap's set bits.
///
/// This is minor 6, and it is the one a reimplementation is most likely to
/// accept and quietly answer with an empty region -- the client then draws
/// nothing and has no error to explain it.
#[test]
fn xfixes_builds_a_region_from_a_bitmaps_set_bits() {
    let mask = 0x0020_0820;
    let gc = 0x0020_0821;
    let mut fixture = XfixesRegionFixture::new();
    let create = create_pixmap_request(XfixesRegionFixture::ORDER, 1, mask, X_SETUP_DEFAULT_ROOT, 4, 2);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&create)), None);
    let create_gc = create_gc_request(XfixesRegionFixture::ORDER, gc, mask);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&create_gc)), None);
    // The two leftmost pixels of the first row, nothing on the second.
    let data: Vec<u8> = vec![0b0000_0011, 0, 0, 0, 0, 0, 0, 0];
    let put = put_image_request_at_depth(XfixesRegionFixture::ORDER, 1, mask, gc, 4, 2, &data);
    assert_eq!(XfixesRegionFixture::error_of(&fixture.send(&put)), None);

    let from_bitmap = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE,
        XfixesRegionFixture::A,
        mask,
        0,
    ));
    assert_eq!(XfixesRegionFixture::error_of(&from_bitmap), None);
    assert_eq!(
        fixture.fetch(XfixesRegionFixture::A),
        vec![Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        }],
        "the set bits, and not an empty region"
    );

    // A pixmap that is not one bit deep does not describe a region.
    let deep = 0x0020_0822;
    let create_deep =
        create_pixmap_request(XfixesRegionFixture::ORDER, 24, deep, X_SETUP_DEFAULT_ROOT, 4, 2);
    assert_eq!(
        XfixesRegionFixture::error_of(&fixture.send(&create_deep)),
        None
    );
    let too_deep = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        deep,
        0,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&too_deep),
        Some(XErrorCode::BadMatch)
    );

    // And a pixmap that does not exist is a bad pixmap, not a bad match.
    let absent = fixture.send(&xfixes_create_region_from_request(
        XfixesRegionFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_BITMAP_MINOR_OPCODE,
        XfixesRegionFixture::OUT,
        0x0020_08fd,
        0,
    ));
    assert_eq!(
        XfixesRegionFixture::error_of(&absent),
        Some(XErrorCode::BadPixmap)
    );
}
