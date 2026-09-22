
// Where a routed button lands when an override-redirect popup is open.
//
// A GTK menu is an override-redirect toplevel: a child of the root, beside
// the window that opened it rather than under it, holding a pointer grab with
// owner_events while it is up. That shape is why the authority's hit test
// cannot recover a popup Engine failed to name -- descending from the main
// window never reaches a sibling -- and it is the shape a live dropdown was
// measured in when every click landed on the main window instead.
//
// Both halves are pinned here, over a real socket and with the grab held, so
// that a later repair in the wrong layer reads as wrong: a route naming the
// popup reaches the popup, and a route naming the main window does not.
#[cfg(unix)]
mod popup_pointer_target {
    use super::pointer_queries::{Client, Fixture};
    use super::*;
    use std::io::Write;

    /// GrabPointer with owner_events set, which is how a menu holds the
    /// pointer while it is open. The fixture's own grab helper leaves
    /// owner_events clear, and that byte is the one that selects the delivery
    /// branch under test.
    fn grab_with_owner_events(client: &mut Client, window: u32) {
        let mut request = vec![26, 1];
        push_u16(&mut request, client.order, 6);
        push_u32(&mut request, client.order, window);
        push_u16(&mut request, client.order, 0x7f);
        request.extend_from_slice(&[1, 1]);
        for _ in 0..3 {
            push_u32(&mut request, client.order, 0);
        }
        client.stream.write_all(&request).unwrap();
        let reply = client.reply();
        assert_eq!(&reply[..2], &[1, 0], "the grab is held: {reply:?}");
    }

    /// The next ButtonPress or ButtonRelease this client is sent, and the
    /// window it is reported with respect to.
    fn next_button(client: &mut Client) -> (u8, u32) {
        loop {
            let record = read_x_record(&mut client.stream);
            if record[0] == 4 || record[0] == 5 {
                return (record[0], read_u32(client.order, &record[12..16]));
            }
        }
    }

    /// A mapped override-redirect window beside `main`, selecting buttons.
    fn popup_beside(client: &mut Client, geometry: (i16, i16, u16, u16)) -> u32 {
        let window = client.next;
        client.next += 1;
        let (x, y, w, h) = geometry;
        client
            .stream
            .write_all(&create_window_override_redirect_request(
                client.order,
                window,
                x,
                y,
                w,
                h,
            ))
            .unwrap();
        client
            .stream
            .write_all(&change_window_event_mask_request(client.order, window, 4 | 8))
            .unwrap();
        client.window_request(8, window);
        client.barrier();
        window
    }

    #[test]
    fn a_button_named_at_the_popup_reaches_the_popup_even_under_the_main_windows_grab() {
        for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let mut f = Fixture::new(false);
            let mut client = f.connect(order);
            let main = client.window(X_SETUP_DEFAULT_ROOT, (100, 200, 320, 240));
            client
                .stream
                .write_all(&change_window_event_mask_request(order, main, 4 | 8))
                .unwrap();
            client.barrier();
            let main_surface = f.surface(&mut client, main);

            // The menu: a root child, override-redirect, beside main rather
            // than under it, with the pointer grabbed by the same client.
            let popup = popup_beside(&mut client, (120, 220, 200, 26));
            let popup_surface = f.surface(&mut client, popup);
            grab_with_owner_events(&mut client, main);

            // Engine names the popup: the click lands on the popup.
            f.route(
                popup_surface,
                InputEventKind::PointerButton {
                    button: 272,
                    pressed: true,
                },
            );
            assert_eq!(
                (4, popup),
                next_button(&mut client),
                "a route naming the popup surface is reported on the popup window, \
                 grab or no grab"
            );
            f.route(
                popup_surface,
                InputEventKind::PointerButton {
                    button: 272,
                    pressed: false,
                },
            );
            assert_eq!((5, popup), next_button(&mut client));

            // Engine names the main window while the popup is open: the click
            // lands on the main window, because descending from main can never
            // reach a sibling. This is the live symptom, pinned as the
            // authority's correct behaviour given the wrong surface, so a fix
            // attempted here rather than where the surface is chosen reads as
            // the wrong layer.
            f.route(
                main_surface,
                InputEventKind::PointerButton {
                    button: 272,
                    pressed: true,
                },
            );
            assert_eq!(
                (4, main),
                next_button(&mut client),
                "the authority cannot recover from Engine naming the wrong surface"
            );
            f.route(
                main_surface,
                InputEventKind::PointerButton {
                    button: 272,
                    pressed: false,
                },
            );
            assert_eq!((5, main), next_button(&mut client));
        }
    }
}
