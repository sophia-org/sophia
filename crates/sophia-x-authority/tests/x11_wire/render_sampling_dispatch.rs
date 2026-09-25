// RENDER sampling through dispatch: transforms, filters, trapezoids and
// triangles, glyphs onto windows and pixmaps, solid fills and gradients.
// Included from x11_wire.rs beside extensions_dispatch.rs (t026).

impl RenderFixture {
    /// Compose the source picture over the destination with no mask, so a
    /// test can read what the source's transform and filter produced.
    fn composite_source_over(&mut self, width: u16, height: u16) -> XDispatchResult {
        self.send(&render_composite_request(
            Self::ORDER,
            1,
            Self::SOURCE_PICTURE,
            0,
            Self::PICTURE,
            0,
            0,
            0,
            0,
            0,
            0,
            width,
            height,
        ))
    }
}

/// A picture transform moves where a composite reads its source from.
///
/// RENDER's matrix maps a destination-relative coordinate to the source
/// pixel, so it is applied forward. GTK sends these at startup, which is why
/// refusing them ended both toolkits before they drew anything.
#[test]
fn render_picture_transforms_move_where_a_composite_samples() {
    // A two-pixel source: red then green, so which pixel was sampled is
    // visible in the result.
    let build = |transform: [i32; 9]| -> Vec<[u8; 4]> {
        let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
        fixture.add_source(2, 1, false);
        fixture.fill_source(
            [0xffff, 0, 0, 0xffff],
            Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        fixture.fill_source(
            [0, 0xffff, 0, 0xffff],
            Rect {
                x: 1,
                y: 0,
                width: 1,
                height: 1,
            },
        );
        let set = fixture.send(&render_set_picture_transform_request(
            RenderFixture::ORDER,
            RenderFixture::SOURCE_PICTURE,
            transform,
        ));
        assert_eq!(RenderFixture::error_of(&set), None);
        assert_eq!(
            RenderFixture::error_of(&fixture.composite_source_over(4, 1)),
            None
        );
        (0..4).map(|x| fixture.pixel(x, 0)).collect()
    };

    const RED: [u8; 4] = [0, 0, 0xff, 0xff];
    const GREEN: [u8; 4] = [0, 0xff, 0, 0xff];
    const NOTHING: [u8; 4] = [0, 0, 0, 0];

    // Identity samples one-to-one, and the source is only two wide, so the
    // pixels beyond it read as transparent.
    assert_eq!(
        build(X_RENDER_IDENTITY_TRANSFORM),
        vec![RED, GREEN, NOTHING, NOTHING],
        "identity"
    );

    // Translation by one source pixel shifts the read left.
    let translate = [65536, 0, 65536, 0, 65536, 0, 0, 0, 65536];
    assert_eq!(
        build(translate),
        vec![GREEN, NOTHING, NOTHING, NOTHING],
        "translated"
    );

    // A constant divisor scales: with w = 2 every coordinate halves, so the
    // two source pixels each cover two destination pixels. This is the shape
    // conformance suites use for transform coverage, and it falls out of the
    // homogeneous divide rather than needing a special case.
    let half = [65536, 0, 0, 0, 65536, 0, 0, 0, 131072];
    assert_eq!(build(half), vec![RED, RED, GREEN, GREEN], "half scale");
}

/// A projective transform that sends a point to infinity has no source pixel
/// there, and the protocol's answer for no source is transparent black.
#[test]
fn render_projective_transforms_answer_nothing_where_they_diverge() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
    fixture.add_source(4, 1, true);
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
        },
    );
    // w = x - 1.5, which is zero at the centre of destination pixel 1.
    let diverging = [65536, 0, 0, 0, 65536, 0, 65536, 0, -98304];
    let set = fixture.send(&render_set_picture_transform_request(
        RenderFixture::ORDER,
        RenderFixture::SOURCE_PICTURE,
        diverging,
    ));
    assert_eq!(RenderFixture::error_of(&set), None);
    assert_eq!(
        RenderFixture::error_of(&fixture.composite_source_over(4, 1)),
        None
    );
    assert_eq!(
        fixture.pixel(1, 0),
        [0, 0, 0, 0],
        "a point with no source samples transparent rather than panicking"
    );
}

/// Bilinear filtering blends the pixels either side of the sample point.
#[test]
fn render_bilinear_filtering_blends_between_source_pixels() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
    fixture.add_source(2, 1, false);
    // Black then white, so the blend is readable as a midpoint grey.
    fixture.fill_source(
        [0, 0, 0, 0xffff],
        Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    fixture.fill_source(
        [0xffff, 0xffff, 0xffff, 0xffff],
        Rect {
            x: 1,
            y: 0,
            width: 1,
            height: 1,
        },
    );
    let filter = fixture.send(&render_set_picture_filter_request(
        RenderFixture::ORDER,
        RenderFixture::SOURCE_PICTURE,
        "bilinear",
        &[],
    ));
    assert_eq!(RenderFixture::error_of(&filter), None);
    // Half scale, so destination pixel 1 samples exactly halfway between the
    // two source pixels: (1.5)/2 = 0.75, minus the half-pixel tap offset is
    // 0.25 into the span from pixel 0 to pixel 1.
    let half = [65536, 0, 0, 0, 65536, 0, 0, 0, 131072];
    let set = fixture.send(&render_set_picture_transform_request(
        RenderFixture::ORDER,
        RenderFixture::SOURCE_PICTURE,
        half,
    ));
    assert_eq!(RenderFixture::error_of(&set), None);
    assert_eq!(
        RenderFixture::error_of(&fixture.composite_source_over(4, 1)),
        None
    );
    let blended = fixture.pixel(1, 0);
    assert!(
        blended[0] > 0 && blended[0] < 0xff,
        "a bilinear tap between black and white is neither: {blended:?}"
    );
    // Nearest would have produced one or the other exactly.
    assert_ne!(blended[0], 0);
    assert_ne!(blended[0], 0xff);
}

/// The filter table is what a client reads before deciding what to ask for,
/// so its bytes are pinned.
///
/// Aliases come first on the wire, one slot per name, carrying the index of
/// the name each resolves to. `convolution` is deliberately absent: a client
/// that finds it missing disables its own kernel work cleanly.
#[test]
fn render_query_filters_answers_the_filters_this_server_honours() {
    let mut fixture = RenderFixture::new();
    let result = fixture.send(&render_query_filters_request(
        RenderFixture::ORDER,
        X_SETUP_DEFAULT_ROOT,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    let encoded = result.encoded_outputs(RenderFixture::ORDER);
    let reply = &encoded[0];
    assert_eq!(reply.len(), 76, "five aliases, five names, both padded");
    assert_eq!(read_u32(RenderFixture::ORDER, &reply[8..12]), 5, "aliases");
    assert_eq!(read_u32(RenderFixture::ORDER, &reply[12..16]), 5, "names");
    // Canonical entries carry no alias; the three aliases point at the names
    // they resolve to.
    let aliases: Vec<u16> = (0..5)
        .map(|i| read_u16(RenderFixture::ORDER, &reply[32 + i * 2..34 + i * 2]))
        .collect();
    assert_eq!(aliases, vec![0xffff, 0xffff, 0, 1, 1]);
    // Names follow the padded alias list.
    let names = &reply[44..76];
    let mut offset = 0;
    let mut read = Vec::new();
    while offset < names.len() {
        let len = usize::from(names[offset]);
        if len == 0 {
            break;
        }
        read.push(String::from_utf8_lossy(&names[offset + 1..offset + 1 + len]).to_string());
        offset += 1 + len;
    }
    assert_eq!(read, vec!["nearest", "bilinear", "fast", "good", "best"]);

    // A drawable that does not exist is refused rather than answered.
    let refused = fixture.send(&render_query_filters_request(
        RenderFixture::ORDER,
        0x0020_09ff,
    ));
    assert_eq!(
        RenderFixture::error_of(&refused),
        Some(XErrorCode::BadDrawable)
    );
}

/// Filter names are accepted, aliased, or refused on the terms the protocol
/// sets.
#[test]
fn render_set_picture_filter_accepts_what_it_advertises_and_refuses_the_rest() {
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    for name in ["nearest", "bilinear", "fast", "good", "best"] {
        let result = fixture.send(&render_set_picture_filter_request(
            RenderFixture::ORDER,
            RenderFixture::PICTURE,
            name,
            &[],
        ));
        assert_eq!(RenderFixture::error_of(&result), None, "filter {name}");
    }
    // Not advertised, so not a filter this server has.
    let convolution = fixture.send(&render_set_picture_filter_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        "convolution",
        &[],
    ));
    assert_eq!(
        RenderFixture::error_of(&convolution),
        Some(XErrorCode::BadValue)
    );
    // A filter this server does have, sent with parameters it does not take,
    // is a mismatch between the request and its argument rather than a bad
    // name.
    let with_params = fixture.send(&render_set_picture_filter_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        "nearest",
        &[65536],
    ));
    assert_eq!(
        RenderFixture::error_of(&with_params),
        Some(XErrorCode::BadMatch)
    );
}

impl RenderFixture {
    /// An opaque white source, so a trapezoid's coverage byte becomes the
    /// destination pixel and can be read directly.
    fn with_white_source(width: u16, height: u16) -> Self {
        let mut fixture = Self::with_argb_pixmap(width, height);
        fixture.add_source(width, height, true);
        fixture.fill_source(
            [0xffff, 0xffff, 0xffff, 0xffff],
            Rect {
                x: 0,
                y: 0,
                width: i32::from(width),
                height: i32::from(height),
            },
        );
        fixture
    }
}

/// A trapezoid covers what it encloses and nothing beyond it.
///
/// Ported from yserver's rasteriser tests. Compositing an opaque white
/// source through the coverage makes each destination pixel equal to the
/// coverage byte, so the assertions read the rasteriser directly.
#[test]
fn render_trapezoids_cover_their_interior_and_leave_the_rest_alone() {
    let mut fixture = RenderFixture::with_white_source(8, 8);
    // An axis-aligned box from (1,1) to (5,5).
    let result = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        &[(
            fixed(1),
            fixed(5),
            (fixed(1), fixed(1)),
            (fixed(1), fixed(5)),
            (fixed(5), fixed(1)),
            (fixed(5), fixed(5)),
        )],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    assert_eq!(
        fixture.pixel(2, 2),
        [0xff, 0xff, 0xff, 0xff],
        "the interior is fully covered"
    );
    assert_eq!(
        fixture.pixel(0, 0),
        [0, 0, 0, 0],
        "outside the trapezoid nothing was drawn"
    );
    assert_eq!(fixture.pixel(6, 6), [0, 0, 0, 0], "and beyond it");
}

/// A trapezoid that is not axis-aligned covers only its slant.
#[test]
fn render_trapezoids_narrow_with_their_slanted_sides() {
    let mut fixture = RenderFixture::with_white_source(16, 8);
    // A wedge: wide at the top, narrow at the bottom.
    let result = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        &[(
            fixed(0),
            fixed(8),
            (fixed(0), fixed(0)),
            (fixed(6), fixed(8)),
            (fixed(16), fixed(0)),
            (fixed(10), fixed(8)),
        )],
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    // The top row spans the full width; the bottom row does not.
    assert_ne!(fixture.pixel(1, 0)[3], 0, "wide at the top");
    assert_eq!(
        fixture.pixel(1, 7)[3],
        0,
        "the left slant has moved inward by the bottom"
    );
    assert_ne!(fixture.pixel(8, 7)[3], 0, "and the middle is still covered");
}

/// A triangle covers its interior, and a fan expands to the same triangles
/// an explicit list would have drawn.
#[test]
fn render_triangles_and_fans_cover_the_same_shapes() {
    let explicit = {
        let mut fixture = RenderFixture::with_white_source(8, 8);
        let result = fixture.send(&render_triangles_request(
            RenderFixture::ORDER,
            X_RENDER_TRIANGLES_MINOR_OPCODE,
            3,
            RenderFixture::SOURCE_PICTURE,
            RenderFixture::PICTURE,
            &[
                (fixed(0), fixed(0)),
                (fixed(6), fixed(0)),
                (fixed(0), fixed(6)),
            ],
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        assert_ne!(fixture.pixel(1, 1)[3], 0, "inside the triangle");
        assert_eq!(fixture.pixel(6, 6), [0, 0, 0, 0], "outside it");
        (0..8)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .map(|(x, y)| fixture.pixel(x, y))
            .collect::<Vec<_>>()
    };

    // The same triangle as a fan of three points.
    let fanned = {
        let mut fixture = RenderFixture::with_white_source(8, 8);
        let result = fixture.send(&render_triangles_request(
            RenderFixture::ORDER,
            X_RENDER_TRI_FAN_MINOR_OPCODE,
            3,
            RenderFixture::SOURCE_PICTURE,
            RenderFixture::PICTURE,
            &[
                (fixed(0), fixed(0)),
                (fixed(6), fixed(0)),
                (fixed(0), fixed(6)),
            ],
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        (0..8)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .map(|(x, y)| fixture.pixel(x, y))
            .collect::<Vec<_>>()
    };
    assert_eq!(explicit, fanned, "a fan draws the triangles it describes");
}

/// The source is anchored at the first primitive's leading corner.
///
/// Xorg's `fbTrapezoids` subtracts the first trapezoid's leading corner from
/// the source offset before compositing, so a client that measures that
/// offset from the shape's own corner reads the source it meant. Without the subtraction the read is
/// displaced by wherever the shape happens to sit, which is how a window
/// shadow ends up with a transparent band.
#[test]
fn render_trapezoids_anchor_the_source_at_the_first_corner() {
    // A source opaque in exactly one pixel, at (2, 2).
    let build = |source_x: i16, source_y: i16| -> [u8; 4] {
        let mut fixture = RenderFixture::with_argb_pixmap(8, 8);
        fixture.add_source(8, 8, false);
        fixture.fill_source(
            [0xffff, 0xffff, 0xffff, 0xffff],
            Rect {
                x: 2,
                y: 2,
                width: 1,
                height: 1,
            },
        );
        let result = fixture.send(&render_trapezoids_request(
            RenderFixture::ORDER,
            3,
            RenderFixture::SOURCE_PICTURE,
            RenderFixture::PICTURE,
            0,
            source_x,
            source_y,
            &[(
                fixed(2),
                fixed(4),
                (fixed(2), fixed(2)),
                (fixed(2), fixed(4)),
                (fixed(4), fixed(2)),
                (fixed(4), fixed(4)),
            )],
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        fixture.pixel(2, 2)
    };

    // Offset measured from the shape's corner: the anchor cancels it, the
    // read lands at the shape's own position, and the opaque pixel is found.
    assert_ne!(
        build(2, 2)[3],
        0,
        "an offset measured from the corner reads the source the client meant"
    );
    // A different offset reads somewhere else, which is what makes the
    // subtraction observable rather than decorative.
    assert_eq!(
        build(0, 0)[3],
        0,
        "and a different offset reads a different part of the source"
    );
}

/// Coverage requests refuse the operators and formats they must.
#[test]
fn render_coverage_requests_refuse_unimplemented_operators_and_formats() {
    let mut fixture = RenderFixture::with_white_source(4, 4);
    let trap = (
        fixed(0),
        fixed(2),
        (fixed(0), fixed(0)),
        (fixed(0), fixed(2)),
        (fixed(2), fixed(0)),
        (fixed(2), fixed(2)),
    );
    // An operator the protocol defines and this server withholds.
    let withheld = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        0x13,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        &[trap],
    ));
    assert_eq!(
        RenderFixture::error_of(&withheld),
        Some(XErrorCode::BadImplementation)
    );
    // A mask format naming nothing this server has.
    let bad_format = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0x1234,
        0,
        0,
        &[trap],
    ));
    assert_eq!(
        RenderFixture::error_of(&bad_format),
        Some(XErrorCode::RenderPictFormat)
    );
    // An empty list is accepted and draws nothing.
    let empty = fixture.send(&render_trapezoids_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        &[],
    ));
    assert_eq!(RenderFixture::error_of(&empty), None);
}

/// Glyphs composited onto a window reach the screen.
///
/// Every other glyph test here draws into a pixmap, which is not what a
/// toolkit does with text it wants visible: GTK composites glyphs straight
/// onto the window. That path goes through the drawing funnel rather than
/// ending in the store, and nothing had exercised it.
#[test]
fn render_composite_glyphs_onto_a_window_reaches_the_drawing_funnel() {
    let window = 0x0020_0500;
    let picture = 0x0020_0501;
    let glyphset = 0x0020_0502;
    let mut fixture = RenderFixture::new();

    let create = create_window_request(RenderFixture::ORDER, window, 0, 0, 16, 16);
    assert_eq!(RenderFixture::error_of(&fixture.send(&create)), None);
    let bind = render_create_picture_request(
        RenderFixture::ORDER,
        picture,
        window,
        X_RENDER_FORMAT_RGB24,
        &[],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&bind)), None);

    // A one-pixel repeating source, the way a client paints text one colour.
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

    let set = render_create_glyph_set_request(RenderFixture::ORDER, glyphset, X_RENDER_FORMAT_A8);
    assert_eq!(RenderFixture::error_of(&fixture.send(&set)), None);
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        glyphset,
        &[(1, [2, 1], [0, 0, 2, 0], vec![0xff, 0xff, 0, 0])],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&add)), None);

    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        picture,
        X_RENDER_FORMAT_A8,
        glyphset,
        0,
        0,
        (1, 1),
        &[1],
    ));
    assert_eq!(
        RenderFixture::error_of(&result),
        None,
        "compositing glyphs onto a window must not be refused"
    );
}

/// A picture outlives the pixmap it was made from, and still takes glyphs.
///
/// A toolkit frees the pixmap as soon as it has a picture for it and keeps
/// drawing through the picture. The backing is retained for exactly that
/// reason, and text is what it is usually retained for.
#[test]
fn render_composite_glyphs_into_a_retained_pixmap_backing() {
    let glyphset = 0x0020_0512;
    let mut fixture = RenderFixture::with_argb_pixmap(8, 8);
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
    let set = render_create_glyph_set_request(RenderFixture::ORDER, glyphset, X_RENDER_FORMAT_A8);
    assert_eq!(RenderFixture::error_of(&fixture.send(&set)), None);
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        glyphset,
        &[(1, [2, 1], [0, 0, 2, 0], vec![0xff, 0xff, 0, 0])],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&add)), None);

    // The pixmap goes away; the picture stays.
    let free = fixture.send(&free_pixmap_request(
        RenderFixture::ORDER,
        RenderFixture::PIXMAP,
    ));
    assert_eq!(RenderFixture::error_of(&free), None);

    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        glyphset,
        0,
        0,
        (1, 1),
        &[1],
    ));
    assert_eq!(
        RenderFixture::error_of(&result),
        None,
        "a picture whose pixmap was freed still draws"
    );
}

/// A glyph that falls outside the destination does not refuse the run.
///
/// Text is positioned by a pen that walks along a line, and a client draws
/// runs that extend past the edge of what it is drawing into -- a label
/// wider than its window, or text scrolled partly out of view. The glyphs
/// that miss simply do not appear; refusing the request would take the
/// visible ones with them.
#[test]
fn render_composite_glyphs_outside_the_destination_still_draw_the_rest() {
    let glyphset = 0x0020_0520;
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
    let set = render_create_glyph_set_request(RenderFixture::ORDER, glyphset, X_RENDER_FORMAT_A8);
    assert_eq!(RenderFixture::error_of(&fixture.send(&set)), None);
    // A two-pixel-wide glyph advancing two pixels each time.
    let add = render_add_glyphs_request(
        RenderFixture::ORDER,
        glyphset,
        &[(1, [2, 1], [0, 0, 2, 0], vec![0xff, 0xff, 0, 0])],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&add)), None);

    // Six of them from x=0 runs off the right edge of a four-wide pixmap.
    let result = fixture.send(&render_composite_glyphs8_request(
        RenderFixture::ORDER,
        3,
        RenderFixture::SOURCE_PICTURE,
        RenderFixture::PICTURE,
        X_RENDER_FORMAT_A8,
        glyphset,
        0,
        0,
        (0, 0),
        &[1, 1, 1, 1, 1, 1],
    ));
    assert_eq!(
        RenderFixture::error_of(&result),
        None,
        "a run that overruns the destination must not be refused"
    );
    assert_ne!(
        fixture.pixel(0, 0)[3],
        0,
        "and the glyphs that did land are drawn"
    );
}

/// A composite clipped by the destination edge still reads the source it
/// would have read unclipped.
///
/// Clipping moves where the destination walk begins, so the source read has
/// to move with it. Getting this wrong draws the wrong part of the source
/// along every edge, which is the kind of fault that looks like a rendering
/// glitch rather than a bug.
#[test]
fn render_composite_clipped_by_the_edge_reads_the_same_source_pixels() {
    // A four-wide source whose third pixel is the distinctive one.
    let build = |destination_x: i16| -> [u8; 4] {
        let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
        fixture.add_source(4, 1, false);
        fixture.fill_source(
            [0, 0xffff, 0, 0xffff],
            Rect {
                x: 2,
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
            destination_x,
            0,
            4,
            1,
        ));
        assert_eq!(RenderFixture::error_of(&result), None);
        // Destination pixel 2 corresponds to source pixel 2 only when the
        // composite starts at the origin; started at -2 it shows source 4,
        // which does not exist. What matters is that starting at 0 works and
        // the clipped case does not shift the reads.
        fixture.pixel(2, 0)
    };

    // Unclipped: destination 2 reads source 2, the green pixel.
    assert_eq!(build(0), [0, 0xff, 0, 0xff], "unclipped");

    // Starting left of the drawable clips the first two columns away. The
    // remaining destination pixels must still read the source pixels they
    // were always going to read: destination 2 now corresponds to source 4,
    // which is past the end of a non-repeating source, so it is transparent
    // rather than the green pixel wrongly shifted into view.
    assert_eq!(build(-2), [0, 0, 0, 0], "clipped, and not shifted");
}

/// A solid fill is a source of one colour with no drawable behind it.
#[test]
fn render_solid_fills_paint_the_colour_they_were_given() {
    let solid = 0x0020_0600;
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    // Opaque red, premultiplied on the wire as the protocol defines.
    let create = render_create_solid_fill_request(
        RenderFixture::ORDER,
        solid,
        [0xffff, 0, 0, 0xffff],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&create)), None);
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        solid,
        0,
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
    assert_eq!(fixture.pixel(0, 0), [0, 0, 0xff, 0xff]);
    assert_eq!(fixture.pixel(1, 1), [0, 0, 0xff, 0xff], "everywhere");
}

/// A linear gradient runs between its stops.
///
/// Cairo paints widget backgrounds with these and sends them without asking
/// what version the server offers, which is how they arrived here.
#[test]
fn render_linear_gradients_run_between_their_stops() {
    let gradient = 0x0020_0601;
    let mut fixture = RenderFixture::with_argb_pixmap(4, 1);
    // Black at the left edge, white at the right.
    let create = render_create_linear_gradient_request(
        RenderFixture::ORDER,
        gradient,
        (0, 0),
        (4 * 65536, 0),
        &[
            (0, [0, 0, 0, 0xffff]),
            (65536, [0xffff, 0xffff, 0xffff, 0xffff]),
        ],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&create)), None);
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        gradient,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        4,
        1,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);

    let ramp: Vec<u8> = (0..4).map(|x| fixture.pixel(x, 0)[2]).collect();
    assert!(
        ramp.windows(2).all(|pair| pair[0] < pair[1]),
        "the gradient rises from one stop to the other: {ramp:?}"
    );
    assert!(ramp[0] < 0x40, "dark at the first stop: {ramp:?}");
    assert!(ramp[3] > 0xb0, "light at the last: {ramp:?}");
    // Every pixel is opaque: both stops are, and interpolating in straight
    // alpha is what keeps the middle from dimming.
    for x in 0..4 {
        assert_eq!(fixture.pixel(x, 0)[3], 0xff, "opaque at {x}");
    }
}

/// A gradient with no stops has no colour to show, and is refused.
#[test]
fn render_gradients_without_stops_are_refused() {
    let mut fixture = RenderFixture::with_argb_pixmap(2, 2);
    let create = render_create_linear_gradient_request(
        RenderFixture::ORDER,
        0x0020_0602,
        (0, 0),
        (65536, 0),
        &[],
    );
    assert_eq!(
        RenderFixture::error_of(&fixture.send(&create)),
        Some(XErrorCode::BadValue)
    );
}

/// Pad is what a client painting a gradient reaches for, and it holds the
/// end colours rather than repeating or clipping them.
#[test]
fn render_pad_repeat_holds_a_gradients_end_colours() {
    let gradient = 0x0020_0603;
    let mut fixture = RenderFixture::with_argb_pixmap(6, 1);
    // The ramp is defined over the first two pixels only.
    let create = render_create_linear_gradient_request(
        RenderFixture::ORDER,
        gradient,
        (0, 0),
        (2 * 65536, 0),
        &[
            (0, [0, 0, 0, 0xffff]),
            (65536, [0xffff, 0xffff, 0xffff, 0xffff]),
        ],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&create)), None);
    let pad = render_change_picture_request(RenderFixture::ORDER, gradient, &[(0, 2)]);
    assert_eq!(RenderFixture::error_of(&fixture.send(&pad)), None);
    let result = fixture.send(&render_composite_request(
        RenderFixture::ORDER,
        1,
        gradient,
        0,
        RenderFixture::PICTURE,
        0,
        0,
        0,
        0,
        0,
        0,
        6,
        1,
    ));
    assert_eq!(RenderFixture::error_of(&result), None);
    // Past the end of the ramp the last stop's colour is held.
    assert_eq!(fixture.pixel(5, 0), [0xff, 0xff, 0xff, 0xff], "padded");
}
