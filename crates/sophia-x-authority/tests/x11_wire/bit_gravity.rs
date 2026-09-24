// A window's bit-gravity when it is resized (t215): ForgetGravity, the
// default, discards the contents and repaints the window with its
// background; any other gravity moves the contents as win-gravity moves a
// child, and repaints only what is newly uncovered.

/// A 20 by 20 child with a grey background and one red pixel at (1, 1),
/// resized to 30 by 30 under `gravity`; the pixels read back afterwards.
fn resized_with_bit_gravity(gravity: Option<u32>, ns: u64) -> impl FnMut(i16, i16) -> u32 {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(ns);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x820001, 0, 0, 60, 60));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x820002, 0x820001, 5, 5, 20, 20));
    fixture.send(ns, 2, cwa_request(order, 0x820002, 1 << 1, 0x0080_8080));
    if let Some(gravity) = gravity {
        fixture.send(ns, 2, cwa_request(order, 0x820002, 1 << 4, gravity));
    }
    fixture.send(ns, 8, map_window_request(order, 0x820002));
    fixture.send(ns, 8, map_window_request(order, 0x820001));
    fixture.gc(ns, 0x820003, 0x820001, 0x00ff_0000, false);
    fixture.fill(ns, 0x820002, 0x820003, (1, 1, 1, 1));
    fixture.send(ns, 12, configure_window_request(order, 0x820002, 0x4 | 0x8, &[30, 30]));
    move |x, y| fixture.read(ns, 0x820002, x, y)
}

#[test]
fn forget_gravity_discards_a_resized_windows_contents() {
    let mut read = resized_with_bit_gravity(None, 0x5701);
    assert_eq!(read(1, 1), 0x0080_8080, "the red pixel is discarded");
    assert_eq!(read(25, 25), 0x0080_8080, "and the whole window has its background");
}

#[test]
fn north_west_gravity_keeps_the_contents_where_they_were() {
    let mut read = resized_with_bit_gravity(Some(1), 0x5702);
    assert_eq!(read(1, 1), 0x00ff_0000, "the red pixel stays");
    assert_eq!(read(25, 25), 0x0080_8080, "the new area has the background");
}

#[test]
fn south_east_gravity_moves_the_contents_with_the_corner() {
    let mut read = resized_with_bit_gravity(Some(9), 0x5703);
    assert_eq!(read(11, 11), 0x00ff_0000, "the red pixel moved by the growth");
    assert_eq!(read(1, 1), 0x0080_8080, "and the top left is new");
}
