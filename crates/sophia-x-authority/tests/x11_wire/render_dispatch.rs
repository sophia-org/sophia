// RENDER through dispatch: the handshake, fills, clips, composites, glyphs
// and cursors. Included from x11_wire.rs beside extensions_dispatch.rs
// (t026).

/// The RENDER handshake answers the lower version, and the formats it
/// reports are the visuals' formats.
///
/// A client binds a picture format to a visual and expects the bytes it drew
/// through core requests to mean the same thing through RENDER, so agreement
/// between the two tables is the whole reply.
#[test]
fn render_handshake_answers_the_lower_version_and_the_visuals_formats() {
    let namespace = NamespaceId::from_raw(86);
    let byte_order = XByteOrder::LittleEndian;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();

    // The version answered is the lower of the two.
    for (asked, answered) in [((0, 99), (0, 10)), ((0, 2), (0, 2)), ((1, 0), (0, 10))] {
        let request = decode_x11_core_request(
            context(namespace, 701, byte_order),
            &render_query_version_request(byte_order, asked.0, asked.1),
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, 2, byte_order, X_RENDER_MAJOR_OPCODE),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        let encoded = result.encoded_outputs(byte_order);
        assert_eq!(read_u32(byte_order, &encoded[0][8..12]), answered.0);
        assert_eq!(read_u32(byte_order, &encoded[0][12..16]), answered.1);
    }

    // The four formats, and their agreement with the setup visuals. A client
    // binds a format to a visual and expects core-drawn bytes to mean the
    // same thing through RENDER, so the shifts and masks here must
    // reconstruct exactly the channel masks the visual advertises.
    let request = decode_x11_core_request(
        context(namespace, 702, byte_order),
        &render_query_pict_formats_request(byte_order),
    )
    .unwrap();
    let result = dispatch_x11_wire_request(
        dispatch_context(namespace, 3, byte_order, X_RENDER_MAJOR_OPCODE),
        request,
        &mut runtime,
        &mut atoms,
        &mut properties,
    );
    let encoded = result.encoded_outputs(byte_order);
    let reply = &encoded[0];
    assert_eq!(read_u32(byte_order, &reply[8..12]), 4, "format count");
    assert_eq!(read_u32(byte_order, &reply[12..16]), 1, "screen count");
    assert_eq!(read_u32(byte_order, &reply[16..20]), 2, "depth count");
    assert_eq!(read_u32(byte_order, &reply[20..24]), 2, "visual count");
    let channel = |offset: usize| -> u32 {
        u32::from(read_u16(byte_order, &reply[offset + 2..offset + 4]))
            << read_u16(byte_order, &reply[offset..offset + 2])
    };
    let mut formats = std::collections::BTreeMap::new();
    for index in 0..4 {
        let offset = 32 + index * 28;
        let id = read_u32(byte_order, &reply[offset..offset + 4]);
        let depth = reply[offset + 5];
        let (red, green, blue, alpha) = (
            channel(offset + 8),
            channel(offset + 12),
            channel(offset + 16),
            channel(offset + 20),
        );
        formats.insert(id, (depth, red, green, blue, alpha));
    }
    let argb_visual = x_true_color_visual(X_SETUP_ARGB_VISUAL).unwrap();
    assert_eq!(
        formats.get(&X_RENDER_FORMAT_ARGB32),
        Some(&(
            32,
            argb_visual.red_mask,
            argb_visual.green_mask,
            argb_visual.blue_mask,
            argb_visual.alpha_mask,
        ))
    );
    let default_visual = x_true_color_visual(X_SETUP_DEFAULT_VISUAL).unwrap();
    assert_eq!(
        formats.get(&X_RENDER_FORMAT_RGB24),
        Some(&(
            24,
            default_visual.red_mask,
            default_visual.green_mask,
            default_visual.blue_mask,
            0,
        ))
    );
    assert_eq!(formats.get(&X_RENDER_FORMAT_A8), Some(&(8, 0, 0, 0, 0xff)));
    assert_eq!(formats.get(&X_RENDER_FORMAT_A1), Some(&(1, 0, 0, 0, 0x1)));

    // The screen maps each visual-bearing depth to its format.
    let screen = 32 + 4 * 28;
    assert_eq!(read_u32(byte_order, &reply[screen..screen + 4]), 2);
    assert_eq!(
        read_u32(byte_order, &reply[screen + 4..screen + 8]),
        X_RENDER_FORMAT_RGB24,
        "fallback format"
    );
    let depth24 = screen + 8;
    assert_eq!(reply[depth24], 24);
    assert_eq!(read_u16(byte_order, &reply[depth24 + 2..depth24 + 4]), 1);
    assert_eq!(
        read_u32(byte_order, &reply[depth24 + 8..depth24 + 12]),
        X_SETUP_DEFAULT_VISUAL
    );
    assert_eq!(
        read_u32(byte_order, &reply[depth24 + 12..depth24 + 16]),
        X_RENDER_FORMAT_RGB24
    );
    let depth32 = depth24 + 16;
    assert_eq!(reply[depth32], 32);
    assert_eq!(
        read_u32(byte_order, &reply[depth32 + 8..depth32 + 12]),
        X_SETUP_ARGB_VISUAL
    );
    assert_eq!(
        read_u32(byte_order, &reply[depth32 + 12..depth32 + 16]),
        X_RENDER_FORMAT_ARGB32
    );
}

/// RENDER refusals are two-tier, and each names the minor it declines.
///
/// A minor defined within the advertised version answers BadImplementation:
/// the request exists here and is not offered, which is also what Xorg
/// answers for the five it never wrote. A minor beyond the advertised
/// version answers BadRequest, because a genuine server of that version had
/// no dispatch entry for it at all. The split is what lets a client's
/// version-gated fallback logic work unmodified.
#[test]
fn render_refusals_split_between_not_offered_and_not_that_version() {
    let namespace = NamespaceId::from_raw(87);
    let byte_order = XByteOrder::LittleEndian;
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let mut refusal_for = |minor: u8| -> (XErrorCode, u16) {
        let request = decode_x11_core_request(
            context(namespace, 710, byte_order),
            &render_minor_request(byte_order, minor),
        )
        .unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(namespace, 4, byte_order, X_RENDER_MAJOR_OPCODE),
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
        );
        match result.outputs.as_slice() {
            [XClientOutput::Error(error)] => {
                assert_eq!(error.major_code, X_RENDER_MAJOR_OPCODE);
                (error.code, error.minor_code)
            }
            other => panic!("minor {minor} produced {other:?}"),
        }
    };

    // Within the advertised 0.4: the never-implemented five, the declined
    // trapezoid family, and the base requests still to be implemented.
    // Within the advertised 0.10 and not implemented: the five no server
    // ever wrote, plus indexed-visual palettes, animated cursors and
    // AddTraps -- declined by name rather than by silence.
    for minor in [2, 3, 9, 14, 15, 21, 31, 32] {
        let (code, named) = refusal_for(minor);
        assert_eq!(code, XErrorCode::BadImplementation, "minor {minor}");
        assert_eq!(named, u16::from(minor));
    }
    // Beyond 0.4, or defined by no version at all.
    // Defined by no version of the protocol at all.
    for minor in [16, 37, 99] {
        let (code, named) = refusal_for(minor);
        assert_eq!(code, XErrorCode::BadRequest, "minor {minor}");
        assert_eq!(named, u16::from(minor));
    }
}

/// A fixture that drives RENDER requests against one runtime and reads the
/// resulting pixels back.
struct RenderFixture {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
}

impl RenderFixture {
    const NS: NamespaceId = NamespaceId::from_raw(88);
    const ORDER: XByteOrder = XByteOrder::LittleEndian;
    const PIXMAP: u32 = 0x0020_0100;
    const PICTURE: u32 = 0x0020_0101;

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
            context(Self::NS, u64::from(self.sequence) + 900, Self::ORDER),
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

    /// A depth-32 pixmap with an ARGB32 picture bound to it.
    fn with_argb_pixmap(width: u16, height: u16) -> Self {
        let mut fixture = Self::new();
        let create = create_pixmap_request(
            Self::ORDER,
            32,
            Self::PIXMAP,
            X_SETUP_DEFAULT_ROOT,
            width,
            height,
        );
        assert!(fixture.send(&create).outputs.is_empty(), "pixmap create");
        let picture = render_create_picture_request(
            Self::ORDER,
            Self::PICTURE,
            Self::PIXMAP,
            X_RENDER_FORMAT_ARGB32,
            &[],
        );
        assert!(fixture.send(&picture).outputs.is_empty(), "picture create");
        fixture
    }

    fn error_of(result: &XDispatchResult) -> Option<XErrorCode> {
        result.outputs.iter().find_map(|output| match output {
            XClientOutput::Error(error) => Some(error.code),
            _ => None,
        })
    }

    /// One pixel of the pixmap as `[b, g, r, a]`.
    fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        let bytes = self
            .runtime
            .drawable_image_region(
                Self::NS,
                XResourceId::new(u64::from(Self::PIXMAP), 1),
                Rect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                },
            )
            .expect("pixmap must have backing");
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }
}

/// FillRectangles blends premultiplied color the way the protocol defines,
/// and the bytes in the drawable are the ones a client can hand-compute.
///
/// The store had no alpha semantics before this: every core drawing operation
/// masks the top byte away. A picture over a depth-32 pixmap is where alpha
/// becomes real, and asserting exact bytes rather than "something changed" is
/// what makes the operator table falsifiable.
#[test]
fn render_fill_rectangles_blends_premultiplied_color_into_the_drawable() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    let whole = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };

    // Src writes the color through, ignoring what was there.
    let opaque_red = [0xffff, 0, 0, 0xffff];
    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        opaque_red,
        &[whole],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(1, 1), [0, 0, 0xff, 0xff], "Src red");

    // Over with a half-alpha premultiplied blue: the protocol's result is
    // src + dst * (1 - src_alpha). With src = (0x80,0,0,0x80) over
    // (0,0,0xff,0xff): blue 0x80 + 0 = 0x80, red 0 + 0xff*0x7f/0xff = 0x7f,
    // alpha 0x80 + 0xff*0x7f/0xff = 0xff.
    let half_blue = [0, 0, 0x8080, 0x8080];
    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::PICTURE,
        half_blue,
        &[whole],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(1, 1), [0x80, 0, 0x7f, 0xff], "Over blue");

    // Clear zeroes every channel, alpha included -- the one operator that
    // proves the alpha byte is genuinely being written rather than defaulted.
    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        0,
        RenderFixture::PICTURE,
        opaque_red,
        &[whole],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(1, 1), [0, 0, 0, 0], "Clear");
}

/// A picture's clip list bounds what its fills touch.
#[test]
fn render_picture_clip_rectangles_bound_what_a_fill_touches() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    let result = fixture.send(&render_set_picture_clip_rectangles_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        1,
        1,
        &[Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        }],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        }],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    // The clip origin shifts the rectangle to cover (1,1)..(3,3).
    assert_eq!(fixture.pixel(1, 1), [0xff, 0xff, 0xff, 0xff], "inside clip");
    assert_eq!(fixture.pixel(0, 0), [0, 0, 0, 0], "outside clip");
    assert_eq!(fixture.pixel(3, 3), [0, 0, 0, 0], "outside clip");
}

/// Pictures are refused, and die, on the terms the protocol sets.
#[test]
fn render_pictures_are_refused_and_reclaimed_on_protocol_terms() {
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);

    // A format whose depth is not the drawable's would read colour bytes as
    // coverage; BadMatch is the protocol's answer.
    let mismatched = render_create_picture_request(
        RenderFixture::ORDER,
        0x0020_0200,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_A8,
        &[],
    );
    let result = fixture.send(&mismatched);
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadMatch));

    // An unknown format id gets the extension's own error, not BadValue.
    let unknown = render_create_picture_request(
        RenderFixture::ORDER,
        0x0020_0201,
        RenderFixture::PIXMAP,
        0x1234,
        &[],
    );
    let result = fixture.send(&unknown);
    assert_eq!(
        RenderFixture::error_of(&result),
        Some(XErrorCode::RenderPictFormat)
    );

    // Reusing a live id is BadIdChoice, as for any resource.
    let duplicate = render_create_picture_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_ARGB32,
        &[],
    );
    let result = fixture.send(&duplicate);
    assert_eq!(
        RenderFixture::error_of(&result),
        Some(XErrorCode::BadIdChoice)
    );

    // Alpha maps are declined by name rather than silently dropped: dropping
    // one changes what the client drew without telling it.
    let alpha_map = render_create_picture_request(
        RenderFixture::ORDER,
        0x0020_0202,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_ARGB32,
        &[(1, RenderFixture::PIXMAP)],
    );
    let result = fixture.send(&alpha_map);
    assert_eq!(
        RenderFixture::error_of(&result),
        Some(XErrorCode::BadImplementation)
    );

    // The protocol defines four repeat modes; a fifth is not one of them.
    let undefined_repeat = render_create_picture_request(
        RenderFixture::ORDER,
        0x0020_0203,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_ARGB32,
        &[(0, 4)],
    );
    let result = fixture.send(&undefined_repeat);
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadValue));

    // An operator the protocol defines and this server withholds is refused
    // as unimplemented; one no version defines gets the PictOp error.
    let disjoint = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        0x13,
        RenderFixture::PICTURE,
        [0, 0, 0, 0],
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
    ));
    assert_eq!(
        RenderFixture::error_of(&disjoint),
        Some(XErrorCode::BadImplementation)
    );
    let undefined = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        0x2e,
        RenderFixture::PICTURE,
        [0, 0, 0, 0],
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
    ));
    assert_eq!(
        RenderFixture::error_of(&undefined),
        Some(XErrorCode::RenderPictOp)
    );

    // Freeing the picture releases the id; using it afterwards is refused
    // with the extension's Picture error.
    let free = fixture.send(&render_free_picture_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
    ));
    assert_eq!(RenderFixture::error_of(&free), None);
    let after_free = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        [0, 0, 0, 0],
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
    ));
    assert_eq!(
        RenderFixture::error_of(&after_free),
        Some(XErrorCode::RenderPicture)
    );
}

impl RenderFixture {
    const SOURCE_PIXMAP: u32 = 0x0020_0110;
    const SOURCE_PICTURE: u32 = 0x0020_0111;

    /// A second depth-32 pixmap and picture, to composite from.
    fn add_source(&mut self, width: u16, height: u16, repeat: bool) {
        let create = create_pixmap_request(
            Self::ORDER,
            32,
            Self::SOURCE_PIXMAP,
            X_SETUP_DEFAULT_ROOT,
            width,
            height,
        );
        assert!(self.send(&create).outputs.is_empty(), "source pixmap");
        let values: &[(u32, u32)] = if repeat { &[(0, 1)] } else { &[] };
        let picture = render_create_picture_request(
            Self::ORDER,
            Self::SOURCE_PICTURE,
            Self::SOURCE_PIXMAP,
            X_RENDER_FORMAT_ARGB32,
            values,
        );
        assert!(self.send(&picture).outputs.is_empty(), "source picture");
    }

    /// Fill the source picture with one premultiplied colour, using Src.
    fn fill_source(&mut self, color: [u16; 4], rect: Rect) {
        let request = render_fill_rectangles_request(Self::ORDER, 1, Self::SOURCE_PICTURE, color, &[rect]);
        assert!(self.send(&request).outputs.is_empty(), "source fill");
    }

    fn fill_destination(&mut self, color: [u16; 4], rect: Rect) {
        let request = render_fill_rectangles_request(Self::ORDER, 1, Self::PICTURE, color, &[rect]);
        assert!(self.send(&request).outputs.is_empty(), "destination fill");
    }
}

/// Composite blends a source picture over a destination, and the resulting
/// bytes are the ones the protocol's formula produces.
#[test]
fn render_composite_blends_a_source_picture_over_a_destination() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    let whole = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    fixture.add_source(4, 4, false);
    // Half-alpha premultiplied blue over opaque red, the same arithmetic the
    // fill test verifies, now carried through a sampled source plane.
    fixture.fill_source([0, 0, 0x8080, 0x8080], whole);
    fixture.fill_destination([0xffff, 0, 0, 0xffff], whole);

    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        4,
        4,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(2, 2), [0x80, 0, 0x7f, 0xff]);
}

/// A one-pixel repeating picture covers a whole destination.
///
/// This is how every toolkit paints a solid colour before CreateSolidFill
/// existed, and CreateSolidFill entered at 0.10 -- above what is advertised
/// -- so for a client talking to this server it is the only way.
#[test]
fn render_composite_repeats_a_one_pixel_source_across_the_destination() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, true);
    fixture.fill_source(
        [0, 0xffff, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );

    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        4,
        4,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    for (x, y) in [(0, 0), (3, 3), (1, 2)] {
        assert_eq!(fixture.pixel(x, y), [0, 0xff, 0, 0xff], "at {x},{y}");
    }

    // A non-repeating source of the same size reads transparent black
    // outside its one pixel, which is the protocol's other answer.
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, false);
    fixture.fill_source(
        [0, 0xffff, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        4,
        4,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(0, 0), [0, 0xff, 0, 0xff]);
    assert_eq!(fixture.pixel(3, 3), [0, 0, 0, 0], "outside a bounded source");
}

/// Compositing a picture onto itself reads the pixels it started with.
///
/// A client scrolling a window sends exactly this. Sampling into an owned
/// plane before writing is what makes the answer independent of the
/// direction the loop runs; reading the destination live would smear the
/// overlapping region.
#[test]
fn render_composite_onto_itself_reads_the_pixels_it_started_with() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
    // A distinct value in the leftmost column only.
    fixture.fill_destination(
        [0xffff, 0, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    // Shift right by one: each destination pixel takes its left neighbour.
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        1,
        0,
        3,
        1,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(1, 0), [0, 0, 0xff, 0xff], "shifted red");
    // If the destination had been read live, the red would have smeared
    // across every column instead of moving one place.
    assert_eq!(fixture.pixel(2, 0), [0, 0, 0, 0], "must not smear");
    assert_eq!(fixture.pixel(3, 0), [0, 0, 0, 0], "must not smear");
}

/// A mask attenuates the source, and a component-alpha mask attenuates each
/// channel separately.
///
/// The second is the subpixel-antialiasing path Xft uses when configured for
/// LCD filtering. Treating it as a plain mask renders text with colour
/// fringes that read as a display fault rather than a server one, which is
/// why it is implemented rather than ignored.
#[test]
fn render_composite_masks_attenuate_the_source_per_channel_when_asked() {
    let mask_pixmap = 0x0020_0120;
    let mask_picture = 0x0020_0121;
    let whole = Rect {
        x: 0,
        y: 0,
        width: 2,
        height: 2,
    };

    for component_alpha in [false, true] {
        let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
        fixture.add_source(2, 2, false);
        fixture.fill_source([0xffff, 0xffff, 0xffff, 0xffff], whole);

        let create = create_pixmap_request(
            RenderFixture::ORDER,
            32,
            mask_pixmap,
            X_SETUP_DEFAULT_ROOT,
            2,
            2,
        );
        assert!(fixture.send(&create).outputs.is_empty());
        let values: &[(u32, u32)] = if component_alpha { &[(12, 1)] } else { &[] };
        let picture = render_create_picture_request(
            RenderFixture::ORDER,
            mask_picture,
            mask_pixmap,
            X_RENDER_FORMAT_ARGB32,
            values,
        );
        assert!(fixture.send(&picture).outputs.is_empty());
        // A mask that is fully opaque in blue only: alpha 0xff, blue 0xff,
        // green and red zero.
        let fill = render_fill_rectangles_request(
            RenderFixture::ORDER,
            1,
            mask_picture,
            [0, 0, 0xffff, 0xffff],
            &[whole],
        );
        assert!(fixture.send(&fill).outputs.is_empty());

        let result = fixture.send(&render_composite_request(
            RenderFixture::ORDER,
            3,
            RenderFixture::SOURCE_PICTURE,
            mask_picture,
            RenderFixture::PICTURE,
            0,
            0,
            0,
            0,
            0,
            0,
            2,
            2,
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        if component_alpha {
            // Only the blue channel is covered, so only blue survives.
            assert_eq!(fixture.pixel(0, 0), [0xff, 0, 0, 0xff]);
        } else {
            // The mask's alpha is opaque, so the white source passes whole.
            assert_eq!(fixture.pixel(0, 0), [0xff, 0xff, 0xff, 0xff]);
        }
    }
}

/// Compositing onto an RGB24 destination discards the result's alpha.
///
/// The format has no alpha component, so the protocol defines the result
/// that way; the store's slot keeps a zero alpha byte and the window buffer
/// tag stays XR24, which is what the compositor was promised.
#[test]
fn render_composite_onto_an_opaque_format_discards_result_alpha() {
    let mut fixture = RenderFixture::new();
    let create = create_pixmap_request(
        RenderFixture::ORDER,
        24,
        RenderFixture::PIXMAP,
        X_SETUP_DEFAULT_ROOT,
        2,
        2,
    );
    assert!(fixture.send(&create).outputs.is_empty());
    let picture = render_create_picture_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_RGB24,
        &[],
    );
    assert!(fixture.send(&picture).outputs.is_empty());

    let result = fixture.send(&render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        [0, 0, 0xffff, 0xffff],
        &[Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        }],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(0, 0), [0xff, 0, 0, 0], "alpha byte stays zero");
}

impl RenderFixture {
    const GLYPHSET: u32 = 0x0020_0130;

    fn add_glyphset(&mut self, format: u32) {
        let request = render_create_glyph_set_request(Self::ORDER, Self::GLYPHSET, format);
        assert!(self.send(&request).outputs.is_empty(), "glyph set create");
    }
}

/// Antialiased glyph coverage attenuates the source colour, which is what
/// makes text drawn through RENDER look like text rather than a bitmap.
///
/// The A8 coverage byte is the whole point of the extension for a toolkit:
/// a client uploads partial coverage at a glyph's edges and expects the
/// server to blend it, and asserting the blended byte is what proves the
/// coverage was honoured rather than thresholded.
#[test]
fn render_composite_glyphs_blends_coverage_into_the_destination() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, true);
    // An opaque red source, repeating, which is how a client paints text in
    // one colour.
    fixture.fill_source(
        [0xffff, 0, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    fixture.add_glyphset(X_RENDER_FORMAT_A8);

    // A 2x1 glyph: one pixel fully covered, one half covered. The A8 stride
    // pads to four bytes.
    let request = render_add_glyphs_request(
        RenderFixture::ORDER,
        RenderFixture::GLYPHSET,
        &[(7, [2, 1], [0, 0, 2, 0], vec![0xff, 0x80, 0, 0])],
    );
    assert!(fixture.send(&request).outputs.is_empty(), "add glyphs");

    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        RenderFixture::GLYPHSET,
        0,
        0,
        (1, 1),
        &[7],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);

    // Full coverage passes the source through unchanged.
    assert_eq!(fixture.pixel(1, 1), [0, 0, 0xff, 0xff], "covered pixel");
    // Half coverage scales the premultiplied source: 0xff * 0x80 / 255 = 0x80
    // in both the red channel and alpha, over a transparent destination.
    assert_eq!(fixture.pixel(2, 1), [0, 0, 0x80, 0x80], "half-covered pixel");
    // Outside the glyph nothing was drawn.
    assert_eq!(fixture.pixel(3, 1), [0, 0, 0, 0], "beyond the glyph");
    assert_eq!(fixture.pixel(1, 0), [0, 0, 0, 0], "above the glyph");
}

/// A referenced glyph set shares storage with the set it names.
///
/// The protocol says the second name refers to the same glyphs rather than
/// copying them, so a glyph added through one name is visible through the
/// other, and the contents survive until the last name is freed. A client
/// that frees the original and keeps drawing through the reference is doing
/// something the protocol allows.
#[test]
fn render_referenced_glyph_sets_share_storage_and_outlive_the_first_name() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, true);
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    fixture.add_glyphset(X_RENDER_FORMAT_A8);

    let reference = 0x0020_0131;
    let request =
        render_reference_glyph_set_request(RenderFixture::ORDER, reference, RenderFixture::GLYPHSET);
    assert!(fixture.send(&request).outputs.is_empty(), "reference");

    // Added through the original name.
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        RenderFixture::GLYPHSET,
        &[(3, [1, 1], [0, 0, 1, 0], vec![0xff, 0, 0, 0])],
    );
    assert!(fixture.send(&add).outputs.is_empty());

    // Freeing the original must not take the glyphs with it.
    let free = render_free_glyph_set_request(RenderFixture::ORDER, RenderFixture::GLYPHSET);
    assert!(fixture.send(&free).outputs.is_empty(), "free original");

    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        reference,
        0,
        0,
        (0, 0),
        &[3],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(fixture.pixel(0, 0), [0xff, 0xff, 0xff, 0xff]);

    // The original name is genuinely gone.
    let stale = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        RenderFixture::GLYPHSET,
        0,
        0,
        (0, 0),
        &[3],
    ));
    assert_eq!(
        RenderFixture::error_of(&stale),
        Some(XErrorCode::RenderGlyphSet)
    );
}

/// A run naming a glyph the set does not hold draws nothing at all.
///
/// Resolving every glyph before drawing any is what makes the refusal clean:
/// a client that gets an error and redraws would otherwise find the prefix
/// of its run already on screen and draw it twice.
#[test]
fn render_composite_glyphs_refuses_an_unknown_glyph_without_drawing_the_prefix() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    fixture.add_source(1, 1, true);
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    fixture.add_glyphset(X_RENDER_FORMAT_A8);
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        RenderFixture::GLYPHSET,
        &[(1, [1, 1], [0, 0, 1, 0], vec![0xff, 0, 0, 0])],
    );
    assert!(fixture.send(&add).outputs.is_empty());

    // Glyph 1 exists, glyph 2 does not.
    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        RenderFixture::GLYPHSET,
        0,
        0,
        (0, 0),
        &[1, 2],
    ));
    assert_eq!(
        RenderFixture::error_of(&result),
        Some(XErrorCode::RenderGlyph)
    );
    assert_eq!(fixture.pixel(0, 0), [0, 0, 0, 0], "prefix must not draw");
}

/// AddGlyphs whose image bytes do not cover its glyph table is refused, and
/// leaves the set untouched.
#[test]
fn render_add_glyphs_refuses_data_shorter_than_its_glyph_table() {
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    fixture.add_glyphset(X_RENDER_FORMAT_A8);
    // A 4x4 A8 glyph needs sixteen bytes; four are supplied.
    let request = render_add_glyphs_request(
        RenderFixture::ORDER,
        RenderFixture::GLYPHSET,
        &[(9, [4, 4], [0, 0, 4, 0], vec![0xff, 0xff, 0xff, 0xff])],
    );
    assert_eq!(
        RenderFixture::error_of(&fixture.send(&request)),
        Some(XErrorCode::BadLength)
    );
}

/// RENDER is advertised, with its own error base, now that the requests
/// behind the advertised version answer.
#[test]
fn render_is_advertised_once_its_requests_answer() {
    let mut fixture = RenderFixture::new();
    let result = fixture.send(&query_extension_request(
        RenderFixture::ORDER,
        X_RENDER_EXTENSION_NAME,
    ));
    let encoded = result.encoded_outputs(RenderFixture::ORDER);
    assert_eq!(encoded[0][8], 1, "present");
    assert_eq!(encoded[0][9], X_RENDER_MAJOR_OPCODE);
    assert_eq!(encoded[0][11], X_RENDER_FIRST_ERROR, "first error");
}

/// A RENDER cursor stores the picture's premultiplied pixels, and FreeCursor
/// releases them.
///
/// RENDER pictures are premultiplied already, which is the engine's
/// `CursorAsset` contract exactly -- unlike core `CreateCursor`, whose source
/// and mask bitmaps carry no alpha at all. Storing the image makes the
/// resource real; putting it on screen is a separate step, and the cursor the
/// compositor draws stays the configured one until that lands.
#[test]
fn render_cursors_store_the_pictures_premultiplied_pixels() {
    let cursor = 0x0020_0140;
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    // Half-alpha red, premultiplied, filling the picture.
    fixture.fill_destination(
        [0x8080, 0, 0, 0x8080],
        Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        },
    );
    let result = fixture.send(&render_create_cursor_request(
        RenderFixture::ORDER,
        cursor,
        RenderFixture::PICTURE,
        1,
        1,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    let image = fixture
        .runtime
        .render_cursor_image(XResourceId::new(u64::from(cursor), 1))
        .expect("the cursor image must be stored");
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!((image.hotspot_x, image.hotspot_y), (1, 1));
    // Stored exactly as the picture holds them: premultiplied [b, g, r, a].
    assert_eq!(&image.premultiplied_bgra[0..4], &[0, 0, 0x80, 0x80]);
    // Every channel is at or below its alpha, which is the invariant the
    // engine validates -- so a stored image is always one it could accept.
    for pixel in image.premultiplied_bgra.chunks_exact(4) {
        assert!(
            pixel[0..3].iter().all(|channel| *channel <= pixel[3]),
            "not premultiplied: {pixel:?}"
        );
    }

    let free = fixture.send(&free_cursor_request(RenderFixture::ORDER, cursor));
    assert_eq!(RenderFixture::error_of(&free), None);
    assert!(
        fixture
            .runtime
            .render_cursor_image(XResourceId::new(u64::from(cursor), 1))
            .is_none(),
        "FreeCursor must release the image"
    );
}

/// A cursor is refused on the terms that keep the stored image usable.
#[test]
fn render_cursors_are_refused_when_the_picture_cannot_describe_one() {
    // A picture with no alpha cannot describe a cursor's shape.
    let mut fixture = RenderFixture::new();
    let create = create_pixmap_request(
        RenderFixture::ORDER,
        24,
        RenderFixture::PIXMAP,
        X_SETUP_DEFAULT_ROOT,
        2,
        2,
    );
    assert!(fixture.send(&create).outputs.is_empty());
    let picture = render_create_picture_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        RenderFixture::PIXMAP,
        X_RENDER_FORMAT_RGB24,
        &[],
    );
    assert!(fixture.send(&picture).outputs.is_empty());
    let result = fixture.send(&render_create_cursor_request(
        RenderFixture::ORDER,
        0x0020_0141,
        RenderFixture::PICTURE,
        0,
        0,
    ));
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadMatch));

    // A hotspot outside the image would point somewhere the cursor is not.
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    let result = fixture.send(&render_create_cursor_request(
        RenderFixture::ORDER,
        0x0020_0142,
        RenderFixture::PICTURE,
        2,
        0,
    ));
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadValue));

    // Larger than the engine accepts is refused rather than scaled: a cursor
    // silently resized is one whose hotspot no longer points where the client
    // put it.
    let mut fixture = RenderFixture::with_argb_pixmap(200, 200);
    let result = fixture.send(&render_create_cursor_request(
        RenderFixture::ORDER,
        0x0020_0143,
        RenderFixture::PICTURE,
        0,
        0,
    ));
    assert_eq!(RenderFixture::error_of(&result), Some(XErrorCode::BadAlloc));
}
