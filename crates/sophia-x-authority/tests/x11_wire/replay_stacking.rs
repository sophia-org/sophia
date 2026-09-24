// Density replay draws the journal over the whole toplevel, where a child
// window has no buffer of its own; each journaled command is therefore
// clipped to what its window shows (t200).

/// A child window created with a background pixel.
fn create_child_background_request(
    order: XByteOrder,
    window: u32,
    parent: u32,
    (x, y, width, height): (i16, i16, u16, u16),
    background: u32,
) -> Vec<u8> {
    let mut out = create_window_request_with_parent(order, window, parent, x, y, width, height);
    out[2..4].copy_from_slice(&match order {
        XByteOrder::LittleEndian => 9u16.to_le_bytes(),
        XByteOrder::BigEndian => 9u16.to_be_bytes(),
    });
    out[28..32].copy_from_slice(&match order {
        XByteOrder::LittleEndian => 2u32.to_le_bytes(),
        XByteOrder::BigEndian => 2u32.to_be_bytes(),
    });
    push_u32(&mut out, order, background);
    out
}

/// The 0.75 variant of a 40 by 40 toplevel, which must replay exactly.
fn replayed_variant(fixture: &mut InferiorsFixture, toplevel: u32, transaction: u64) -> Vec<u32> {
    let requirement = SurfaceRasterRequirements {
        surface: SurfaceId::new(toplevel, 1),
        committed_content_generation: 1,
        requirement_generation: 1,
        logical_extent: Size { width: 40, height: 40 },
        classes: vec![SurfaceRasterClass {
            density_millis: 750,
            transform: SurfaceRasterTransform::Normal,
        }],
    };
    let outcome = fixture
        .runtime
        .apply_surface_raster_requirements(TransactionId::from_raw(transaction), &requirement)
        .unwrap();
    let response = expect_satisfied_raster(outcome, "the journal must replay");
    let [XAuthorityCpuBufferUpdate::Replace(store)] = response.cpu_buffer_updates.as_slice() else {
        panic!("one derived replacement: {:?}", response.cpu_buffer_updates.len());
    };
    assert_eq!(store.size, Size { width: 30, height: 30 });
    xrgb_pixels(&store.bytes).into_iter().map(|pixel| pixel & 0x00ff_ffff).collect()
}

#[test]
fn a_parent_draw_replays_under_its_mapped_child() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5201);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x750001, 0, 0, 40, 40));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x750002, 0x750001, 10, 10, 10, 10));
    fixture.send(ns, 8, map_window_request(order, 0x750002));
    fixture.send(ns, 8, map_window_request(order, 0x750001));
    fixture.gc(ns, 0x750003, 0x750001, 0x00ff_0000, false);
    fixture.gc(ns, 0x750004, 0x750001, 0x0000_ff00, false);
    fixture.gc(ns, 0x750005, 0x750001, 0x0000_00ff, false);
    fixture.fill(ns, 0x750001, 0x750003, (0, 0, 40, 40));
    fixture.fill(ns, 0x750002, 0x750004, (0, 0, 10, 10));
    fixture.fill(ns, 0x750001, 0x750005, (5, 5, 30, 30));
    let pixels = replayed_variant(&mut fixture, 0x750001, 9_950);
    assert_eq!(pixels[11 * 30 + 11], 0x0000_ff00, "the child stays on top");
    assert_eq!(pixels[5 * 30 + 5], 0x0000_00ff, "the parent's fill shows beside it");
}

#[test]
fn a_child_draw_replays_inside_the_child_only() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5202);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x760001, 0, 0, 40, 40));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x760002, 0x760001, 10, 10, 10, 10));
    fixture.send(ns, 8, map_window_request(order, 0x760002));
    fixture.send(ns, 8, map_window_request(order, 0x760001));
    fixture.gc(ns, 0x760003, 0x760001, 0x00ff_0000, false);
    fixture.gc(ns, 0x760004, 0x760001, 0x0000_ff00, false);
    fixture.send(ns, 56, change_gc_request(order, 0x760004, 1 << 4, &[4]));
    fixture.fill(ns, 0x760001, 0x760003, (0, 0, 40, 40));
    fixture.send(ns, 65, poly_line_request(order, 0x760002, 0x760004, &[(0, 0), (30, 30)]));
    let pixels = replayed_variant(&mut fixture, 0x760001, 9_951);
    assert_eq!(pixels[11 * 30 + 11], 0x0000_ff00, "the line inside the child");
    assert_eq!(pixels[22 * 30 + 22], 0x00ff_0000, "and nothing of it past the child's edge");
}

/// A toplevel its child covers never sees a single clear of all of it, so
/// the journal recovers once its clears together define every pixel.
#[test]
fn a_covered_toplevel_replays_again_once_its_clears_cover_it() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5203);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_background_request(order, 0x770001, 0, 0, 40, 40, 0x00ff_0000));
    fixture.send(ns, 1, create_child_background_request(order, 0x770002, 0x770001, (0, 10, 30, 30), 0x0000_ff00));
    fixture.send(ns, 8, map_window_request(order, 0x770002));
    fixture.send(ns, 8, map_window_request(order, 0x770001));
    // A child-first draw leaves the journal at the child's extent, which
    // the toplevel's first draw then poisons.
    fixture.send(ns, 61, clear_area_request(order, false, 0x770002, 0, 0, 0, 0));
    fixture.send(ns, 61, clear_area_request(order, false, 0x770001, 0, 0, 0, 0));
    fixture.send(ns, 61, clear_area_request(order, false, 0x770002, 0, 0, 0, 0));
    let pixels = replayed_variant(&mut fixture, 0x770001, 9_952);
    assert_eq!(pixels[3 * 30 + 15], 0x00ff_0000, "the toplevel's own top strip");
    assert_eq!(pixels[20 * 30 + 27], 0x00ff_0000, "and its right strip");
    assert_eq!(pixels[20 * 30 + 15], 0x0000_ff00, "the child over the rest");
}

/// A copy whose source a sibling hides would replay the sibling's pixels,
/// so the journal gives up on it instead.
#[test]
fn a_copy_out_of_a_hidden_source_is_not_replayed() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5204);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x780001, 0, 0, 40, 40));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x780002, 0x780001, 0, 0, 20, 20));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x780003, 0x780001, 10, 0, 20, 20));
    fixture.send(ns, 8, map_window_request(order, 0x780002));
    fixture.send(ns, 8, map_window_request(order, 0x780003));
    fixture.send(ns, 8, map_window_request(order, 0x780001));
    fixture.gc(ns, 0x780004, 0x780001, 0x00ff_0000, false);
    fixture.fill(ns, 0x780001, 0x780004, (0, 0, 40, 40));
    fixture.fill(ns, 0x780002, 0x780004, (0, 0, 20, 20));
    fixture.send(ns, 62, copy_area_request(order, 0x780002, 0x780002, 0x780004, 10, 0, 0, 10, 10, 10));
    let requirement = SurfaceRasterRequirements {
        surface: SurfaceId::new(0x780001, 1),
        committed_content_generation: 1,
        requirement_generation: 1,
        logical_extent: Size { width: 40, height: 40 },
        classes: vec![SurfaceRasterClass {
            density_millis: 750,
            transform: SurfaceRasterTransform::Normal,
        }],
    };
    let outcome = fixture
        .runtime
        .apply_surface_raster_requirements(TransactionId::from_raw(9_953), &requirement)
        .unwrap();
    assert_eq!(
        expect_raster_fallback(outcome, "a hidden copy source must not replay"),
        XRasterFallbackCause::UnsupportedCrossDrawableCopy
    );
}

/// A double-buffered client draws into a pixmap and copies it to its
/// window. The copy is replayed from the pixels it carried, so the window's
/// density variants stay exact instead of falling back (t050).
#[test]
fn a_copy_from_a_pixmap_replays_the_pixels_it_carried() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5205);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x790001, 0, 0, 40, 40));
    fixture.send(ns, 8, map_window_request(order, 0x790001));
    fixture.send(ns, 53, create_pixmap_request(order, 24, 0x790002, 0x790001, 30, 30));
    fixture.gc(ns, 0x790003, 0x790001, 0x00ff_0000, false);
    fixture.gc(ns, 0x790004, 0x790001, 0x0000_00ff, false);
    fixture.fill(ns, 0x790001, 0x790003, (0, 0, 40, 40));
    fixture.fill(ns, 0x790002, 0x790004, (0, 0, 30, 30));
    // The source runs past the pixmap's right edge, where nothing is copied.
    fixture.send(ns, 62, copy_area_request(order, 0x790002, 0x790001, 0x790003, 10, 0, 0, 0, 30, 20));
    let pixels = replayed_variant(&mut fixture, 0x790001, 9_954);
    assert_eq!(pixels[7 * 30 + 7], 0x0000_00ff, "the copied pixels");
    assert_eq!(pixels[7 * 30 + 22], 0x00ff_0000, "nothing past the source's edge");
    assert_eq!(pixels[22 * 30 + 7], 0x00ff_0000, "nor below the copy");
}
