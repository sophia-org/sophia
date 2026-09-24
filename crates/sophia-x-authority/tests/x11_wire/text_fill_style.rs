// PolyText paints its glyphs through the graphics context's fill style, as
// every other drawing request does; only ImageText is fixed to FillSolid.

fn text_through_fill_style(fill_style: u32) -> (Vec<u32>, u32) {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5401);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x7b0001, 0, 0, 40, 20));
    fixture.send(ns, 53, create_pixmap_request(order, 24, 0x7b0002, 0x7b0001, 40, 20));
    fixture.send(ns, 53, create_pixmap_request(order, 24, 0x7b0003, 0x7b0001, 2, 1));
    fixture.send(ns, 45, open_font_request(order, 0x7b0004, "fixed"));
    fixture.gc(ns, 0x7b0005, 0x7b0001, 0x0000_0000, false);
    fixture.fill(ns, 0x7b0002, 0x7b0005, (0, 0, 40, 20));
    // The tile: red in its first column, blue in its second.
    fixture.gc(ns, 0x7b0006, 0x7b0001, 0x00ff_0000, false);
    fixture.fill(ns, 0x7b0003, 0x7b0006, (0, 0, 1, 1));
    fixture.gc(ns, 0x7b0007, 0x7b0001, 0x0000_00ff, false);
    fixture.fill(ns, 0x7b0003, 0x7b0007, (1, 0, 1, 1));
    let foreground = 0x0012_ab34;
    fixture.gc(ns, 0x7b0008, 0x7b0001, foreground, false);
    // Font, fill style and tile, in value-mask order: fill style (1 << 8),
    // tile (1 << 10), font (1 << 14).
    fixture.send(ns, 56, change_gc_request(order, 0x7b0008, (1 << 8) | (1 << 10) | (1 << 14), &[fill_style, 0x7b0003, 0x7b0004]));
    fixture.send(ns, 74, poly_text8_request(order, 0x7b0002, 0x7b0008, 2, 14, b"HH"));
    let pixels = (0..20)
        .flat_map(|y| (0..40).map(move |x| (x, y)))
        .map(|(x, y)| fixture.read(ns, 0x7b0002, x, y))
        .collect();
    (pixels, foreground)
}

#[test]
fn poly_text_paints_its_glyphs_through_the_tile() {
    let (pixels, foreground) = text_through_fill_style(1);
    assert!(!pixels.contains(&foreground), "a tiled glyph takes the tile's pixels, not the foreground");
    for (index, pixel) in pixels.iter().enumerate() {
        let expected = if index % 2 == 0 { 0x00ff_0000 } else { 0x0000_00ff };
        assert!(*pixel == 0 || *pixel == expected, "pixel {index} is {pixel:06x}, from the tile's column");
    }
    assert!(pixels.iter().any(|pixel| *pixel != 0), "the glyphs were drawn");
}

#[test]
fn poly_text_stays_solid_under_fill_solid() {
    let (pixels, foreground) = text_through_fill_style(0);
    assert!(pixels.iter().all(|pixel| *pixel == 0 || *pixel == foreground));
    assert!(pixels.contains(&foreground));
}
