#[test]
fn key_reached_surface_comes_from_selected_window_ancestry_not_the_request() {
    let mut fixture = KeyFixture::new();
    let leaf = XResourceId::new(0x200009, 1);
    let registered = SurfaceId::new(9901, 1);
    assert_ne!(registered, surface());
    fixture
        .base
        .private
        .broker
        .registry
        .register_surface(client(), namespace(), registered, window())
        .unwrap();
    {
        let mut selected = fixture.base.selections.lock().unwrap();
        selected.register(
            leaf,
            window(),
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 80,
            },
        );
        selected.observe_mapped(leaf);
        selected.update(leaf, Some(3), None);
    }
    let mut hold = None;
    fixture.press(30, 8851, &mut hold).unwrap();
    let hold = hold.as_ref().unwrap();
    assert_eq!(hold.delivered_window(), leaf);
    assert_eq!(hold.reached_surface(), Some(registered));
    assert_eq!(hold.client(), client());
    assert_eq!(hold.namespace(), namespace());
}

#[test]
fn key_reached_surface_explicitly_records_an_unregistered_target() {
    let mut fixture = KeyFixture::new();
    let mut hold = None;
    fixture.press(30, 8852, &mut hold).unwrap();
    let hold = hold.as_ref().unwrap();
    assert_eq!(hold.delivered_window(), window());
    assert_eq!(hold.reached_surface(), None);
}
