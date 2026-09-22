
// Where a key lands when the pointer is inside the focus window's subtree.
//
// X11 reports a keyboard event with respect to the window the pointer is in
// when that window is inside the focus window's subtree, and with respect to
// the focus window otherwise, propagating upward from there and stopping at a
// window whose do-not-propagate mask forbids it. The conformance suite checks
// exactly this and cannot run against us yet, so it is checked here.
#[cfg(unix)]
mod key_focus_subtree {
    use super::pointer_queries::{Client, Fixture};
    use super::*;
    use std::io::Write;

    pub(super) fn set_input_focus(client: &mut Client, window: u32) {
        let mut request = vec![42u8, 2, 3, 0];
        push_u32(&mut request, client.order, window);
        push_u32(&mut request, client.order, 0);
        client.stream.write_all(&request).unwrap();
        client.barrier();
    }

    pub(super) fn select_keys(client: &mut Client, window: u32) {
        client
            .stream
            .write_all(&change_window_event_mask_request(client.order, window, 3))
            .unwrap();
        client.barrier();
    }

    /// The window the next KeyPress is reported with respect to, or `None`
    /// when no key was delivered at all.
    ///
    /// A GetInputFocus after the key is the barrier: X11 writes a connection's
    /// records in order, so anything the key produced is already on the wire
    /// ahead of that request's reply. **The reply is always consumed**, even
    /// when a key arrives first -- leaving it queued makes the next call read
    /// a stale reply and report a delivered key as discarded, which is a
    /// false green for exactly the assertions this file exists to make.
    pub(super) fn next_key_window(client: &mut Client) -> Option<u32> {
        let mut request = vec![43u8, 0];
        push_u16(&mut request, client.order, 1);
        client.stream.write_all(&request).unwrap();
        let mut reported = None;
        loop {
            let record = read_x_record(&mut client.stream);
            match record[0] {
                2 if reported.is_none() => {
                    reported = Some(read_u32(client.order, &record[12..16]));
                }
                1 => return reported,
                _ => continue,
            }
        }
    }

    pub(super) fn press(f: &mut Fixture, surface: SurfaceId) {
        f.route(
            surface,
            InputEventKind::Key {
                keycode: 38,
                pressed: true,
            },
        );
    }

    #[test]
    fn a_key_is_reported_on_the_pointers_window_when_it_is_under_the_focus() {
        let order = XByteOrder::LittleEndian;
        let mut f = Fixture::new(false);
        let mut client = f.connect(order);

        // main > child > grandchild, with the pointer landing inside the
        // grandchild and the focus on the child between them.
        let main = client.window(X_SETUP_DEFAULT_ROOT, (100, 200, 320, 240));
        let child = client.window(main, (10, 20, 100, 100));
        let grandchild = client.window(child, (10, 10, 60, 60));
        for window in [main, child, grandchild] {
            select_keys(&mut client, window);
        }
        let surface = f.surface(&mut client, main);
        set_input_focus(&mut client, child);

        // Put the pointer in the grandchild: local (43,59) inside main is
        // (33,39) inside child, which is (23,29) inside grandchild.
        f.route(surface, InputEventKind::PointerMotion);
        assert_eq!(
            (0, [143, 259, 23, 29], 0),
            client.query(grandchild),
            "the pointer is inside the grandchild, with nothing below it"
        );

        press(&mut f, surface);
        assert_eq!(
            Some(grandchild),
            next_key_window(&mut client),
            "the pointer's window is inside the focus subtree, so the key is \
             reported there rather than on the focus window above it"
        );
    }

    // A companion test for the do-not-propagate mask is deliberately absent.
    // The rule honours it -- see key_routing's own controls -- but this path
    // cannot yet act on it: when the walk finds nobody, writers/input.rs waits
    // out a readiness deadline and then delivers to the focus anyway, so a
    // blocked walk and an unselected one are indistinguishable by the time the
    // event is written. Making a block discard needs a third outcome there and
    // is raised as t150 rather than asserted here as though it worked.
}
