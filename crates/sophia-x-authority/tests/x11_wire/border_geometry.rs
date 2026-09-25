mod border_geometry {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn border_pointer_events_use_the_same_interior_as_pixels() {
        use std::io::Write;
        let mut fixture = xtest_admission_socket::XtestFixture::placing_toplevels();
        let mut client = fixture.connect();
        let top = client.next;
        let child = top + 1;
        let order = client.order;
        for (window, parent, x, y, size, border) in [(top, 0x20, 20, 10, 40, 1u16), (child, top, 4, 5, 20, 2u16)] {
            let mut request = create_window_request_with_parent(order, window, parent, x, y, size, size);
            request[20..22].copy_from_slice(&border.to_le_bytes());
            client.stream.write_all(&request).unwrap();
            client.stream.write_all(&change_window_event_mask_request(order, window, (1 << 2) | (1 << 3) | (1 << 6))).unwrap();
            client.stream.write_all(&map_window_request(order, window)).unwrap();
        }
        client.barrier();
        let at = |record: &[u8; 32], offset: usize| i16::from_le_bytes([record[offset], record[offset + 1]]);
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 28, 20);
        let motion = client.next_event(6);
        assert_eq!(u32::from_le_bytes(motion[12..16].try_into().unwrap()), child);
        assert_eq!((at(&motion, 20), at(&motion, 22), at(&motion, 24), at(&motion, 26)), (28, 20, 1, 2));
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((at(&press, 24), at(&press, 26)), (1, 2));
        client.fake_input(5, 1);
        client.next_event(5);
        client.stream.write_all(&configure_window_request(order, child, 1 << 4, &[3])).unwrap();
        client.settle();
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 30, 22);
        let motion = client.next_event(6);
        assert_eq!((at(&motion, 24), at(&motion, 26)), (2, 3), "a border change updates the connection's geometry too");
    }

    fn create(f: &mut InferiorsFixture, ns: NamespaceId, id: u32, parent: u32, rect: (i16, i16, u16, u16), border: u16) -> XDispatchResult {
        let mut request = create_window_request_with_parent(f.order, id, parent, rect.0, rect.1, rect.2, rect.3);
        request[20..22].copy_from_slice(&border.to_le_bytes());
        f.send(ns, 1, request)
    }

    #[test]
    fn border_origins_translate_through_every_ancestor_and_engine_round_trip() {
        let mut f = InferiorsFixture::new();
        let ns = NamespaceId::from_raw(0x5401);
        let top = 0x700001;
        let child = 0x700002;
        let leaf = 0x700003;
        let created = create(&mut f, ns, top, 0x20, (10, 20, 80, 80), 1);
        let surface = &created.response.unwrap().surfaces[0];
        assert_eq!((surface.geometry.x, surface.geometry.y), (11, 21));
        create(&mut f, ns, child, top, (4, 5, 30, 30), 2);
        create(&mut f, ns, leaf, child, (3, 4, 10, 10), 3);
        let id = XResourceId::new(u64::from(leaf), 1);
        assert_eq!(f.runtime.window_root_position(id), Some((23, 35)));
        let (_, _, x, y) = f.runtime.window_presentation_root_and_offset(ns, id).unwrap();
        assert_eq!((x, y), (12, 14), "offset from the top's interior, excluding its border");
        let order = f.order;
        let translated = f.send(ns, 40, translate_coordinates_request(order, leaf, 0x20, -2, 1));
        assert!(matches!(translated.outputs.as_slice(), [XClientOutput::Reply(XClientReply::TranslateCoordinates { dst_x: 21, dst_y: 36, .. })]));
        f.send(ns, 8, map_window_request(order, child));
        let translated = f.send(ns, 40, translate_coordinates_request(order, top, top, 4, 5));
        assert!(matches!(translated.outputs.as_slice(), [XClientOutput::Reply(XClientReply::TranslateCoordinates { child: Some(found), .. })] if *found == XResourceId::new(u64::from(child), 1)), "the border belongs to the mapped child, even before the top maps");
        f.gc(ns, 0x700004, top, 0x00ff_0000, false);
        let draw = f.send(ns, 70, poly_fill_rectangle_request(order, top, 0x700004, &[(0, 0, 80, 80)]));
        let geometry = draw.response.unwrap().transactions[0].target_geometry;
        assert_eq!((geometry.x, geometry.y), (11, 21), "the raster transaction agrees with the admitted surface origin");
        let configured = f.runtime.configure_window_from_engine(ns, XResourceId::new(u64::from(top), 1), sophia_protocol::Rect { x: 50, y: 60, width: 80, height: 80 }).unwrap();
        assert_eq!((configured.x, configured.y), (49, 59), "X reports the outer corner");
        assert_eq!(f.runtime.window_root_position(id), Some((62, 74)));
    }

    #[test]
    fn border_root_warp_query_finds_a_child_only_in_client_placed_mode() {
        let mut f = InferiorsFixture::new();
        let ns = NamespaceId::from_raw(0x5403);
        let top = 0x700021;
        let order = f.order;
        create(&mut f, ns, top, 0x20, (10, 10, 30, 30), 1);
        f.send(ns, 8, map_window_request(order, top));
        let mut warp = vec![41, 0, 6, 0];
        push_u32(&mut warp, order, 0);
        push_u32(&mut warp, order, 0x20);
        warp.extend([0; 8]);
        push_i16(&mut warp, order, 12);
        push_i16(&mut warp, order, 13);
        f.send(ns, 41, warp);
        let query = |f: &mut InferiorsFixture| f.send(ns, 38, resource_request(order, 38, 0x20)).encoded_outputs(order);
        assert_eq!(read_u32(order, &query(&mut f)[0][12..16]), 0, "desktop mode cannot invent Engine hit testing");
        f.runtime.set_client_toplevel_placement(true);
        assert_eq!(read_u32(order, &query(&mut f)[0][12..16]), top);
    }

    #[test]
    fn border_raster_readback_presentation_and_include_inferiors_agree() {
        let mut f = InferiorsFixture::new();
        let ns = NamespaceId::from_raw(0x5402);
        let order = f.order;
        let top = 0x700011;
        let child = 0x700012;
        let leaf = 0x700013;
        create(&mut f, ns, top, 0x20, (10, 10, 30, 30), 1);
        create(&mut f, ns, child, top, (4, 4, 8, 8), 2);
        create(&mut f, ns, leaf, child, (5, 5, 8, 8), 1);
        for window in [leaf, child, top] { f.send(ns, 8, map_window_request(order, window)); }
        f.gc(ns, 0x700014, top, 0x00ff_0000, false);
        f.gc(ns, 0x700015, top, 0x0000_ff00, false);
        f.gc(ns, 0x700016, top, 0x0000_00ff, false);
        f.gc(ns, 0x700017, top, 0x00ff_ffff, true);
        f.fill(ns, top, 0x700014, (0, 0, 30, 30));
        f.fill(ns, child, 0x700015, (0, 0, 8, 8));
        f.fill(ns, leaf, 0x700016, (0, 0, 8, 8));
        for (x, y, pixel) in [(4, 4, 0x00ff_0000), (6, 6, 0x0000_ff00), (12, 12, 0x0000_00ff), (14, 14, 0x00ff_0000)] {
            assert_eq!(f.read(ns, top, x, y), pixel, "readback at {x},{y}");
            assert_eq!(f.on_screen(top, i32::from(x), i32::from(y)), pixel, "presentation at {x},{y}");
        }
        f.fill(ns, top, 0x700017, (6, 6, 1, 1));
        assert_eq!(f.read(ns, child, 0, 0), 0x00ff_ffff);
        assert_eq!(f.read(ns, child, 2, 2), 0x0000_ff00);
        f.fill(ns, top, 0x700017, (14, 14, 1, 1));
        assert_eq!(f.read(ns, leaf, 2, 2), 0x0000_00ff, "IncludeInferiors stays inside the parent clip");
        f.send(ns, 12, configure_window_request(order, child, 1 << 4, &[4]));
        assert_eq!(f.read(ns, top, 6, 6), 0x00ff_ffff, "old child position reveals parent backing");
        assert_eq!(f.on_screen(top, 6, 6), 0x00ff_ffff, "border change republishes the old position");
        assert_eq!(f.read(ns, top, 8, 8), 0x00ff_ffff);
        assert_eq!(f.on_screen(top, 8, 8), 0x00ff_ffff, "the child moves without changing its pixels");
        assert_eq!(f.on_screen(top, 9, 9), 0x0000_ff00);
    }
}
