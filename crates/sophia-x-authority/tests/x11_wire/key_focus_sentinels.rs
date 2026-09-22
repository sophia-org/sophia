
// The two focus sentinels, over a real socket.
//
// SetInputFocus takes a window, or one of two values that are not windows:
// None (0) discards keyboard events until a focus is set again, and
// PointerRoot (1) means the focus follows the pointer, resolved at each event
// rather than stored. The conformance suite checks both and cannot run
// against us yet, so they are checked here.
#[cfg(unix)]
mod key_focus_sentinels {
    use super::key_focus_subtree::{next_key_window, press, set_input_focus};
    use super::pointer_queries::{Client, Fixture};
    use super::*;

    const X_FOCUS_NONE: u32 = 0;
    const X_FOCUS_POINTER_ROOT: u32 = 1;

    /// A main window with two children side by side, and the two pointer
    /// positions that land inside each of them. Local coordinates are
    /// relative to the main window, which sits at (100, 200).
    struct Split {
        surface: SurfaceId,
        left: u32,
        right: u32,
    }

    const IN_LEFT: ((f64, f64), (f64, f64)) = ((143.0, 259.0), (43.0, 59.0));
    const IN_RIGHT: ((f64, f64), (f64, f64)) = ((280.0, 259.0), (180.0, 59.0));

    fn split(f: &mut Fixture, client: &mut Client) -> Split {
        let main = client.window(X_SETUP_DEFAULT_ROOT, (100, 200, 320, 240));
        let left = client.window(main, (0, 0, 100, 240));
        let right = client.window(main, (150, 0, 100, 240));
        let surface = f.surface(client, main);
        Split {
            surface,
            left,
            right,
        }
    }

    fn move_pointer(f: &mut Fixture, surface: SurfaceId, at: ((f64, f64), (f64, f64))) {
        f.route_at(surface, InputEventKind::PointerMotion, at.0, at.1);
    }

    #[test]
    fn a_key_follows_the_pointer_when_the_focus_is_pointer_root() {
        let order = XByteOrder::LittleEndian;
        let mut f = Fixture::new(false);
        let mut client = f.connect(order);
        let tree = split(&mut f, &mut client);
        set_input_focus(&mut client, X_FOCUS_POINTER_ROOT);

        move_pointer(&mut f, tree.surface, IN_LEFT);
        press(&mut f, tree.surface);
        assert_eq!(
            Some(tree.left),
            next_key_window(&mut client),
            "under PointerRoot the key is reported on the window the pointer is in"
        );

        // The same focus, a different pointer window. PointerRoot is resolved
        // at each event, so nothing about the focus needs to change for the
        // answer to; a stored resolution would still name the left window.
        move_pointer(&mut f, tree.surface, IN_RIGHT);
        press(&mut f, tree.surface);
        assert_eq!(
            Some(tree.right),
            next_key_window(&mut client),
            "PointerRoot is resolved per event, not stored when it is set"
        );
    }

    #[test]
    fn a_key_is_discarded_when_the_focus_is_none() {
        let order = XByteOrder::LittleEndian;
        let mut f = Fixture::new(false);
        let mut client = f.connect(order);
        let tree = split(&mut f, &mut client);
        move_pointer(&mut f, tree.surface, IN_LEFT);
        set_input_focus(&mut client, X_FOCUS_NONE);

        press(&mut f, tree.surface);
        assert_eq!(
            None,
            next_key_window(&mut client),
            "a focus of None discards keyboard events entirely: not to the \
             pointer's window, not to the root, not to a last known focus"
        );
    }

    #[test]
    fn delivery_resumes_on_the_focus_after_none_is_replaced() {
        let order = XByteOrder::LittleEndian;
        let mut f = Fixture::new(false);
        let mut client = f.connect(order);
        let tree = split(&mut f, &mut client);
        move_pointer(&mut f, tree.surface, IN_LEFT);

        set_input_focus(&mut client, X_FOCUS_NONE);
        press(&mut f, tree.surface);
        assert_eq!(None, next_key_window(&mut client), "discarded while None");

        // The suite checks the resumption as well as the discard, because a
        // server that stopped delivering permanently would pass the first
        // half on its own.
        set_input_focus(&mut client, tree.right);
        press(&mut f, tree.surface);
        assert_eq!(
            Some(tree.right),
            next_key_window(&mut client),
            "and delivery resumes once a window is focused again"
        );
    }

    /// The other half of the subtree rule, and the control that makes the
    /// PointerRoot one above mean something.
    ///
    /// The pointer is in `left` and the focus is its sibling `right`, whose
    /// subtree the pointer is not in, so the protocol reports the key on the
    /// focus. Without this, the PointerRoot control passes whether or not
    /// PointerRoot is implemented at all -- the pointer's window would be the
    /// answer either way, and the test would be measuring the coincidence
    /// rather than the rule.
    #[test]
    fn a_key_is_reported_on_the_focus_when_the_pointer_is_outside_its_subtree() {
        let order = XByteOrder::LittleEndian;
        let mut f = Fixture::new(false);
        let mut client = f.connect(order);
        let tree = split(&mut f, &mut client);
        move_pointer(&mut f, tree.surface, IN_LEFT);
        set_input_focus(&mut client, tree.right);

        press(&mut f, tree.surface);
        assert_eq!(
            Some(tree.right),
            next_key_window(&mut client),
            "the pointer's window is not inside the focus subtree, so the key \
             is reported on the focus rather than on the pointer's window"
        );
    }
}
