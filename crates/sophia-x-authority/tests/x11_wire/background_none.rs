mod background_none {
    use super::*;

    #[test]
    fn mapping_an_ancestor_seeds_none_descendants_in_parent_order() {
        let mut f = InferiorsFixture::new();
        let ns = NamespaceId::from_raw(0x5802);
        let order = f.order;
        let top = 0x700010;
        let child = top + 1;
        let leaf = top + 2;
        for (id, parent, x, width) in [
            (top, 0x20, 0, 20),
            (child, top, -2, 10),
            (leaf, child, 4, 4),
        ] {
            f.send(
                ns,
                1,
                create_window_request_with_parent(order, id, parent, x, 0, width, 10),
            );
        }
        f.runtime
            .set_window_background(
                ns,
                XResourceId::new(u64::from(top), 1),
                sophia_x_authority::XWindowBackground::Pixel(0x654321),
            )
            .unwrap();
        f.send(ns, 8, map_window_request(order, leaf));
        f.send(ns, 8, map_window_request(order, child));
        f.send(ns, 8, map_window_request(order, top));
        assert_eq!(f.read(ns, child, 2, 0), 0x654321);
        assert_eq!(f.read(ns, leaf, 0, 0), 0x654321);
    }

    #[test]
    fn newly_mapped_none_child_keeps_parent_and_sibling_pixels_after_partial_drawing() {
        for map_all in [false, true] {
            let mut f = InferiorsFixture::new();
            let ns = NamespaceId::from_raw(0x5801);
            let order = f.order;
            let top = 0x700001;
            let sibling = top + 1;
            let child = top + 2;
            let gc = top + 3;
            f.send(
                ns,
                1,
                create_window_request_with_parent(order, top, 0x20, 0, 0, 20, 20),
            );
            f.send(ns, 8, map_window_request(order, top));
            f.gc(ns, gc, top, 0x123456, false);
            f.fill(ns, top, gc, (0, 0, 20, 20));
            f.send(
                ns,
                1,
                create_window_request_with_parent(order, sibling, top, 5, 5, 5, 5),
            );
            f.runtime
                .set_window_background(
                    ns,
                    XResourceId::new(u64::from(sibling), 1),
                    sophia_x_authority::XWindowBackground::Pixel(0),
                )
                .unwrap();
            f.send(ns, 8, map_window_request(order, sibling));
            f.gc(ns, gc + 1, sibling, 0xabcdef, false);
            f.fill(ns, sibling, gc + 1, (0, 0, 5, 5));
            // Default background is None; the border contributes to the copied origin.
            let mut request = create_window_request_with_parent(order, child, top, 1, 1, 12, 12);
            request[20..22].copy_from_slice(&1u16.to_le_bytes());
            f.send(ns, 1, request);
            // XTS builds unmapped children with a temporary background and
            // clears them before switching to None. Those off-screen pixels
            // must not overwrite the screen when the window becomes viewable.
            let child_id = XResourceId::new(u64::from(child), 1);
            f.runtime
                .set_window_background(
                    ns,
                    child_id,
                    sophia_x_authority::XWindowBackground::Pixel(0),
                )
                .unwrap();
            f.send(ns, 61, clear_area_request(order, false, child, 0, 0, 0, 0));
            f.runtime
                .set_window_background(
                    ns,
                    child_id,
                    sophia_x_authority::XWindowBackground::Undefined,
                )
                .unwrap();
            let mut map = map_window_request(order, if map_all { top } else { child });
            if map_all {
                map[0] = 9;
            }
            f.send(ns, if map_all { 9 } else { 8 }, map);
            assert_eq!(
                f.read(ns, child, 0, 0),
                0x123456,
                "new child reads the parent"
            );
            assert_eq!(
                f.read(ns, child, 3, 3),
                0xabcdef,
                "new child preserves underlying sibling"
            );
            assert_eq!(f.read(ns, top, 5, 5), 0xabcdef);
            assert_eq!(f.on_screen(top, 5, 5), 0xabcdef);
            f.gc(ns, gc + 2, child, 0xff00, false);
            f.fill(ns, child, gc + 2, (0, 0, 1, 1));
            assert_eq!(f.read(ns, child, 0, 0), 0xff00);
            assert_eq!(
                f.read(ns, child, 1, 0),
                0x123456,
                "drawing does not zero untouched pixels"
            );
            assert_eq!(f.on_screen(top, 3, 2), 0x123456);
            // Retained pixels are not a live transparent view of the sibling.
            f.fill(ns, sibling, gc + 2, (0, 0, 5, 5));
            assert_eq!(f.read(ns, child, 3, 3), 0xabcdef);
        }
    }
}
