// A window's background pixmap: kept as its contents were when it was set,
// so the pixmap may be freed at once, as the protocol allows; tiled from the
// window's origin; and under ParentRelative, tiled from the origin of the
// parent it is taken from.

fn cwa_request(order: XByteOrder, window: u32, mask: u32, value: u32) -> Vec<u8> {
    let mut out = vec![2, 0];
    push_u16(&mut out, order, 4);
    push_u32(&mut out, order, window);
    push_u32(&mut out, order, mask);
    push_u32(&mut out, order, value);
    out
}

/// A 3 by 1 tile: red, green, blue.
fn three_tile(fixture: &mut InferiorsFixture, ns: NamespaceId, pixmap: u32, drawable: u32) {
    let order = fixture.order;
    fixture.send(ns, 53, create_pixmap_request(order, 24, pixmap, drawable, 3, 1));
    for (index, pixel) in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff].into_iter().enumerate() {
        let gc = pixmap + 0x10 + index as u32;
        fixture.gc(ns, gc, drawable, pixel, false);
        fixture.fill(ns, pixmap, gc, (index as i16, 0, 1, 1));
    }
}

#[test]
fn a_background_pixmap_may_be_freed_as_soon_as_it_is_set() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5601);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x7d0001, 0, 0, 20, 20));
    three_tile(&mut fixture, ns, 0x7d0002, 0x7d0001);
    fixture.send(ns, 2, cwa_request(order, 0x7d0001, 1 << 0, 0x7d0002));
    fixture.send(ns, 54, free_pixmap_request(order, 0x7d0002));
    fixture.send(ns, 8, map_window_request(order, 0x7d0001));
    let row: Vec<u32> = (0..6).map(|x| fixture.read(ns, 0x7d0001, x, 0)).collect();
    assert_eq!(row, vec![0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0x00ff_0000, 0x0000_ff00, 0x0000_00ff]);
}

#[test]
fn a_parent_relative_background_is_tiled_from_the_parents_origin() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5602);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x7e0001, 0, 0, 20, 20));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x7e0003, 0x7e0001, 1, 0, 10, 5));
    three_tile(&mut fixture, ns, 0x7e0002, 0x7e0001);
    fixture.send(ns, 2, cwa_request(order, 0x7e0001, 1 << 0, 0x7e0002));
    // ParentRelative is background-pixmap 1.
    fixture.send(ns, 2, cwa_request(order, 0x7e0003, 1 << 0, 1));
    fixture.send(ns, 8, map_window_request(order, 0x7e0003));
    fixture.send(ns, 8, map_window_request(order, 0x7e0001));
    // The child starts one pixel into the parent, so its first pixel is the
    // tile's second.
    let row: Vec<u32> = (0..3).map(|x| fixture.read(ns, 0x7e0003, x, 0)).collect();
    assert_eq!(row, vec![0x0000_ff00, 0x0000_00ff, 0x00ff_0000]);
}

/// MapSubwindows owes a newly viewable subwindow its background, and its
/// viewable inferiors theirs, as MapWindow does.
#[test]
fn map_subwindows_paints_the_backgrounds_it_makes_viewable() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5603);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x7f0001, 0, 0, 20, 20));
    fixture.send(ns, 8, map_window_request(order, 0x7f0001));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x7f0002, 0x7f0001, 2, 2, 10, 10));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x7f0003, 0x7f0002, 1, 1, 4, 4));
    fixture.send(ns, 2, cwa_request(order, 0x7f0002, 1 << 1, 0x0012_3456));
    fixture.send(ns, 2, cwa_request(order, 0x7f0003, 1 << 1, 0x0065_4321));
    fixture.send(ns, 8, map_window_request(order, 0x7f0003));
    fixture.send(ns, 9, resource_request(order, 9, 0x7f0001));
    assert_eq!(fixture.read(ns, 0x7f0002, 8, 8), 0x0012_3456, "the subwindow");
    assert_eq!(fixture.read(ns, 0x7f0003, 0, 0), 0x0065_4321, "and its inferior");
}

fn clear_request(order: XByteOrder, window: u32) -> Vec<u8> {
    clear_area_request(order, false, window, 0, 0, 0, 0)
}

/// ClearArea restores the background a window has: its tile from its own
/// origin, a ParentRelative tile from the parent's, and nothing at all
/// where the background is None.
#[test]
fn clear_area_restores_the_background_the_window_has() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5604);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x800001, 0, 0, 20, 20));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x800003, 0x800001, 1, 0, 10, 5));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x800004, 0x800001, 0, 10, 10, 5));
    three_tile(&mut fixture, ns, 0x800002, 0x800001);
    fixture.send(ns, 2, cwa_request(order, 0x800001, 1 << 0, 0x800002));
    fixture.send(ns, 2, cwa_request(order, 0x800003, 1 << 0, 1));
    for window in [0x800003, 0x800004] {
        fixture.send(ns, 8, map_window_request(order, window));
    }
    fixture.send(ns, 8, map_window_request(order, 0x800001));
    fixture.gc(ns, 0x800010, 0x800001, 0x0077_7777, false);
    for window in [0x800001, 0x800003, 0x800004] {
        fixture.fill(ns, window, 0x800010, (0, 0, 20, 20));
        fixture.send(ns, 61, clear_request(order, window));
    }
    let row = |fixture: &mut InferiorsFixture, window| (0..3).map(|x| fixture.read(ns, window, x, 0)).collect::<Vec<u32>>();
    assert_eq!(row(&mut fixture, 0x800001), vec![0x00ff_0000, 0x0000_ff00, 0x0000_00ff], "the tile");
    assert_eq!(row(&mut fixture, 0x800003), vec![0x0000_ff00, 0x0000_00ff, 0x00ff_0000], "the parent's tile, from its origin");
    assert_eq!(row(&mut fixture, 0x800004), vec![0x0077_7777; 3], "background None changes nothing");
}
