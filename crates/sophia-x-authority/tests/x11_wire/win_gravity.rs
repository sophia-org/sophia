// A child's win-gravity when its parent is resized (t199), as dix's
// GravityTranslate places it: the table, then the runtime moving children
// topmost first, leaving NorthWest where it was and naming UnmapGravity.

#[test]
fn win_gravity_translate_follows_the_dix_table() {
    use sophia_x_authority::win_gravity_translate as place;
    let (at, grown, moved) = ((10, 20), (7, 9), (3, -4));
    let want = [
        (1, (10, 20)),
        (2, (13, 20)),
        (3, (17, 20)),
        (4, (10, 24)),
        (5, (13, 24)),
        (6, (17, 24)),
        (7, (10, 29)),
        (8, (13, 29)),
        (9, (17, 29)),
        (10, (7, 24)),
    ];
    for (gravity, expected) in want {
        assert_eq!(place(gravity, at, grown, moved), expected, "gravity {gravity}");
    }
}

#[test]
fn a_resized_parent_moves_its_children_by_their_gravity() {
    use sophia_x_authority::XGravityOutcome;
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5501);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x7c0001, 0, 0, 40, 40));
    for (child, x) in [(0x7c0002, 0), (0x7c0003, 10), (0x7c0004, 20)] {
        fixture.send(ns, 1, create_window_request_with_parent(order, child, 0x7c0001, x, 5, 5, 5));
        fixture.send(ns, 8, map_window_request(order, child));
    }
    let id = |raw: u64| XResourceId::new(raw, 1);
    fixture.runtime.set_window_gravity(id(0x7c0003), 9);
    fixture.runtime.set_window_gravity(id(0x7c0004), 0);
    let before = Rect { x: 0, y: 0, width: 40, height: 40 };
    let after = Rect { x: 0, y: 0, width: 50, height: 46 };
    let outcomes = fixture.runtime.apply_win_gravity(ns, id(0x7c0001), before, after, 9);
    assert_eq!(
        outcomes,
        vec![(id(0x7c0004), XGravityOutcome::Unmap), (id(0x7c0003), XGravityOutcome::Moved { x: 20, y: 11 })],
        "topmost first; NorthWest does not move"
    );
    let geometry = fixture.runtime.window_geometry(ns, id(0x7c0003)).unwrap();
    assert_eq!((geometry.x, geometry.y), (20, 11));
    assert!(fixture.runtime.apply_win_gravity(ns, id(0x7c0001), after, after, 10).is_empty(), "no resize, no gravity");
}
