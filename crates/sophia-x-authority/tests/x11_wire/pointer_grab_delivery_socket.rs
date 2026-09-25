// Pointer grabs and propagation over real sockets, on the XTEST fixture:
// the passive and implicit grabs a press activates and whom they report
// to, GrabPointer's own crossings, and the one propagation stop every
// recipient of a button shares (t158, t220, t230). The fixture and its
// client live in xtest_admission_socket.rs.

#[cfg(unix)]
mod pointer_grab_delivery_socket {
    use super::xtest_admission_socket::{XtestClient, XtestFixture};
    use super::*;
    use std::{io::Write, time::Duration};

    /// A press nobody selected on the source propagates up to the first
    /// window where the client selected it, the root included, and stops at
    /// a do-not-propagate mask or at the first selector (XTS Xlib11
    /// ButtonPress 7, ButtonRelease 4). The owner's walk stopped at its own
    /// toplevel, so a client that selected on the root heard nothing from
    /// its own windows. Red before the fix: the first press reaches nobody.
    #[test]
    fn a_press_propagates_to_the_root_and_stops_at_do_not_propagate() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let toplevel = client.next;
        let child = client.next + 2;
        let grandchild = client.next + 4;
        client.next += 6;
        client.stream
            .write_all(&create_window_request(client.order, toplevel, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, child, toplevel, 2, 2, 12, 12))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, grandchild, child, 2, 2, 8, 8))
            .unwrap();
        for window in [toplevel, child, grandchild] {
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        // ButtonPress on the root only.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, X_SETUP_DEFAULT_ROOT, 1 << 2))
            .unwrap();
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let field = |event: &[u8; 32], at: usize| u32::from_le_bytes([event[at], event[at + 1], event[at + 2], event[at + 3]]);
        let at = |event: &[u8; 32], offset: usize| i16::from_le_bytes([event[offset], event[offset + 1]]);
        let dnp = |client: &mut XtestClient, window: u32, mask: u32| {
            let mut out = vec![2, 0];
            push_u16(&mut out, client.order, 4);
            push_u32(&mut out, client.order, window);
            push_u32(&mut out, client.order, 1 << 12);
            push_u32(&mut out, client.order, mask);
            client.stream.write_all(&out).unwrap();
        };

        // In the grandchild (root 24..32, 4..12): the press climbs to the root.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (X_SETUP_DEFAULT_ROOT, toplevel), "the press on the root names the toplevel as child");
        assert_eq!((at(&press, 20), at(&press, 22), at(&press, 24), at(&press, 26)), (27, 7, 27, 7), "root coordinates on the root");
        client.fake_input(5, 1);
        client.assert_quiet("nobody selected the release");

        // The root stops selecting, the toplevel selects, and the child
        // refuses to propagate presses: the press reaches nobody.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, X_SETUP_DEFAULT_ROOT, 0))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, toplevel, 1 << 2))
            .unwrap();
        dnp(&mut client, child, 1 << 2);
        client.barrier();
        client.fake_input(4, 1);
        client.assert_quiet("the child's do-not-propagate mask stops the press");
        client.fake_input(5, 1);

        // The child selects and the toplevel refuses to propagate: the press
        // stops at the child, the first selector, and the toplevel hears
        // nothing.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, child, 1 << 2))
            .unwrap();
        dnp(&mut client, toplevel, 1 << 2);
        dnp(&mut client, child, 0);
        client.barrier();
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (child, grandchild), "the press on the child names the grandchild");
        client.fake_input(5, 1);
        client.assert_quiet("the toplevel heard nothing");
    }

    /// A press with passive grabs on the source window and its ancestors
    /// activates the ancestor-most grab, searched from the root down, and
    /// the activation crosses the pointer from the source window to the
    /// grab window with mode NotifyGrab: an EnterNotify on the ancestor and
    /// none on the windows below it (XTS Xlib11 ButtonPress 2). The release
    /// crosses back with NotifyUngrab. Red before the fix: the activation
    /// crossed nothing.
    #[test]
    fn a_press_activates_the_ancestor_most_passive_grab_with_grab_crossings() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let outer = client.next;
        let middle = client.next + 2;
        let inner = client.next + 4;
        client.next += 6;
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        client.stream
            .write_all(&create_window_request(client.order, outer, 20, 0, 60, 60))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, middle, outer, 10, 10, 30, 30))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, inner, middle, 10, 10, 10, 10))
            .unwrap();
        for window in [outer, middle, inner] {
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, (1 << 4) | (1 << 5)))
                .unwrap();
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 45, 25);
        for expected in [outer, middle, inner] {
            let entered = client.next_event(7);
            assert_eq!(window_of(&entered), expected, "the pointer's way into the innermost window");
        }
        // Passive grabs on all three: owner_events False, no event mask,
        // asynchronous, AnyModifier, as the suite sets them.
        for window in [outer, middle, inner] {
            let mut grab = vec![28, 0];
            push_u16(&mut grab, client.order, 6);
            push_u32(&mut grab, client.order, window);
            push_u16(&mut grab, client.order, 0);
            grab.extend_from_slice(&[1, 1]);
            push_u32(&mut grab, client.order, 0);
            push_u32(&mut grab, client.order, 0);
            grab.extend_from_slice(&[1, 0]);
            push_u16(&mut grab, client.order, 0x8000);
            client.stream.write_all(&grab).unwrap();
        }
        client.settle();

        client.fake_input(4, 1);
        let left = client.next_event(8);
        assert_eq!((window_of(&left), left[1], left[30]), (inner, 0, 1), "leave of the source window, Ancestor, NotifyGrab");
        let left = client.next_event(8);
        assert_eq!((window_of(&left), left[1], left[30]), (middle, 1, 1), "leave of the window between, Virtual, NotifyGrab");
        let entered = client.next_event(7);
        assert_eq!((window_of(&entered), entered[1], entered[30]), (outer, 2, 1), "enter of the grab window, Inferior, NotifyGrab");
        client.assert_quiet("no enter below the grab window, and no press: the grab's mask is empty");

        client.fake_input(5, 1);
        let left = client.next_event(8);
        assert_eq!((window_of(&left), left[1], left[30]), (outer, 2, 2), "leave of the grab window, Inferior, NotifyUngrab");
        let entered = client.next_event(7);
        assert_eq!((window_of(&entered), entered[1], entered[30]), (middle, 1, 2), "enter of the window between, Virtual, NotifyUngrab");
        let entered = client.next_event(7);
        assert_eq!((window_of(&entered), entered[1], entered[30]), (inner, 0, 2), "enter of the source window, Ancestor, NotifyUngrab");
        client.assert_quiet("after the release");
    }

    /// The implicit pointer grab belongs to the client the press was
    /// delivered to, with that client's selection on the event window as
    /// its mask: a peer that selected ButtonPress on the owner's window,
    /// which the owner did not, gets the press, the grab and the release,
    /// and the owner, which selected only the release, hears nothing while
    /// the grab lasts (t230; the reference's TryClientEvents refuses a
    /// client that is not the grab's). Red before the fix: the grab was the
    /// owner's on every press, with a mask of every event.
    #[test]
    fn the_implicit_grab_belongs_to_the_client_the_press_was_delivered_to() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let window = owner.next;
        owner.next += 2;
        for client in [&mut owner, &mut peer] {
            client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        }
        owner.stream
            .write_all(&create_window_request(owner.order, window, 20, 0, 16, 16))
            .unwrap();
        // ButtonRelease alone for the owner.
        owner.stream
            .write_all(&change_window_event_mask_request(owner.order, window, 1 << 3))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, window)).unwrap();
        owner.settle();
        // ButtonPress and ButtonRelease for the peer.
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, window, (1 << 2) | (1 << 3)))
            .unwrap();
        peer.settle();
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        owner.settle();
        owner.fake_input(4, 1);
        let pressed = peer.next_event(4);
        assert_eq!((window_of(&pressed), pressed[1]), (window, 1), "the peer's press");
        owner.assert_quiet("the owner did not select the press");
        owner.fake_input(5, 1);
        let released = peer.next_event(5);
        assert_eq!((window_of(&released), released[1]), (window, 1), "the peer's release under its grab");
        owner.assert_quiet("the owner, not the grab's client, hears no release");
    }

    /// An implicit grab reports every pointer event to the window that took
    /// the press, relative to it, until the last button is up: a press in a
    /// child that selected it, the drag past the child's edge inside the
    /// toplevel, and the release there all arrive on the child with the
    /// child's coordinates, and never on the shell window (t158; the drag
    /// that never let xterm claim PRIMARY). Red before the fix: the motion
    /// and the release were delivered by position to the shell, which had
    /// not selected them, so nothing arrived.
    #[test]
    fn an_implicit_grab_reports_to_the_window_that_took_the_press() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let shell = client.next;
        let text = client.next + 2;
        client.next += 4;
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        client.stream
            .write_all(&create_window_request(client.order, shell, 20, 0, 100, 60))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, shell)).unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, text, shell, 10, 10, 40, 30))
            .unwrap();
        // ButtonPress, ButtonRelease and Button1Motion on the text widget alone.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, text, (1 << 2) | (1 << 3) | (1 << 8)))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, text)).unwrap();
        client.settle();
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        let at = |record: &[u8]| (i16::from_le_bytes([record[24], record[25]]), i16::from_le_bytes([record[26], record[27]]));
        // Into the text widget, whose root origin is (30, 10).
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 35, 25);
        client.settle();
        client.fake_input(4, 1);
        let pressed = client.next_event(4);
        assert_eq!((window_of(&pressed), at(&pressed)), (text, (5, 15)), "the press on the text widget");
        // Past the widget's right edge, still inside the shell.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 100, 55);
        let moved = client.next_event(6);
        assert_eq!((window_of(&moved), at(&moved)), (text, (70, 45)), "the drag reported to the widget, past its edge");
        client.fake_input(5, 1);
        let released = client.next_event(5);
        assert_eq!((window_of(&released), at(&released)), (text, (70, 45)), "the release on the widget that took the press");
        client.assert_quiet("nothing on the shell");
    }

    /// The same past the toplevel altogether: the pointer released over the
    /// root, where no window of the client's is, and the release still
    /// arrives on the widget that took the press, relative to it (t158,
    /// the note's second case).
    #[test]
    fn an_implicit_grab_reports_a_release_over_the_root_to_the_window_that_took_the_press() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let shell = client.next;
        let text = client.next + 2;
        client.next += 4;
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        client.stream
            .write_all(&create_window_request(client.order, shell, 20, 0, 100, 60))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, shell)).unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, text, shell, 10, 10, 40, 30))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, text, (1 << 2) | (1 << 3)))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, text)).unwrap();
        client.settle();
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        let at = |record: &[u8]| (i16::from_le_bytes([record[24], record[25]]), i16::from_le_bytes([record[26], record[27]]));
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 35, 25);
        client.settle();
        client.fake_input(4, 1);
        let pressed = client.next_event(4);
        assert_eq!((window_of(&pressed), at(&pressed)), (text, (5, 15)), "the press on the text widget");
        // Over the root, below the shell.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 5, 90);
        client.fake_input(5, 1);
        let released = client.next_event(5);
        assert_eq!((window_of(&released), at(&released)), (text, (-25, 80)), "the release over the root, on the widget that took the press");
        client.assert_quiet("nothing else");
    }

    /// GrabPointer's activation crosses the pointer from the window it is
    /// in to the grab window with mode NotifyGrab, and UngrabPointer back
    /// with NotifyUngrab, both before the request's reply (t220; the
    /// protocol's "as if the pointer were to suddenly warp"). Red before
    /// the fix: a grab across sibling toplevels crossed nothing.
    #[test]
    fn a_pointer_grab_crosses_into_the_grab_window_and_back_when_it_ends() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let first = client.next;
        let second = client.next + 2;
        client.next += 4;
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        for (window, x) in [(first, 20), (second, 60)] {
            client.stream
                .write_all(&create_window_request(client.order, window, x, 0, 16, 16))
                .unwrap();
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, (1 << 4) | (1 << 5)))
                .unwrap();
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        let entered = client.next_event(7);
        assert_eq!(window_of(&entered), first, "the pointer in the first window");

        // GrabPointer on the second: owner_events False, no mask, async.
        let mut grab = vec![26, 0];
        push_u16(&mut grab, client.order, 6);
        push_u32(&mut grab, client.order, second);
        push_u16(&mut grab, client.order, 0);
        grab.extend_from_slice(&[1, 1]);
        push_u32(&mut grab, client.order, 0);
        push_u32(&mut grab, client.order, 0);
        push_u32(&mut grab, client.order, 0);
        client.stream.write_all(&grab).unwrap();
        let left = read_x_record(&mut client.stream);
        assert_eq!((left[0] & 0x7f, window_of(&left), left[30]), (8, first, 1), "leave of the first window, NotifyGrab: {left:?}");
        let entered = read_x_record(&mut client.stream);
        assert_eq!((entered[0] & 0x7f, window_of(&entered), entered[30]), (7, second, 1), "enter of the grab window, NotifyGrab: {entered:?}");
        let reply = read_x_record(&mut client.stream);
        assert_eq!((reply[0], reply[1]), (1, 0), "the grab's Success reply after its crossings: {reply:?}");

        let mut ungrab = vec![27, 0];
        push_u16(&mut ungrab, client.order, 2);
        push_u32(&mut ungrab, client.order, 0);
        client.stream.write_all(&ungrab).unwrap();
        let left = client.next_event(8);
        assert_eq!((window_of(&left), left[30]), (second, 2), "leave of the grab window, NotifyUngrab: {left:?}");
        let entered = client.next_event(7);
        assert_eq!((window_of(&entered), entered[30]), (first, 2), "enter of the pointer's window, NotifyUngrab: {entered:?}");
        client.assert_quiet("nothing else on the ungrab");
    }

    /// A peer that selected EnterWindow on the grab window is told of the
    /// grab's activation crossing as of any other: the crossings a grab
    /// owes go to every selecting client, grab or no grab (t220).
    #[test]
    fn a_peer_selecting_on_the_grab_window_is_told_of_the_grab_crossing() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let first = owner.next;
        let second = owner.next + 2;
        owner.next += 4;
        for client in [&mut owner, &mut peer] {
            client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        }
        for (window, x) in [(first, 20), (second, 60)] {
            owner.stream
                .write_all(&create_window_request(owner.order, window, x, 0, 16, 16))
                .unwrap();
            owner.stream.write_all(&map_window_request(owner.order, window)).unwrap();
        }
        owner.settle();
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, second, 1 << 4))
            .unwrap();
        peer.settle();
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        owner.settle();
        let mut grab = vec![26, 0];
        push_u16(&mut grab, owner.order, 6);
        push_u32(&mut grab, owner.order, second);
        push_u16(&mut grab, owner.order, 0);
        grab.extend_from_slice(&[1, 1]);
        push_u32(&mut grab, owner.order, 0);
        push_u32(&mut grab, owner.order, 0);
        push_u32(&mut grab, owner.order, 0);
        owner.stream.write_all(&grab).unwrap();
        owner.settle();
        let entered = peer.next_event(7);
        assert_eq!((window_of(&entered), entered[30]), (second, 1), "the peer's enter of the grab window, NotifyGrab: {entered:?}");
        peer.assert_quiet("nothing else for the peer");
    }

    /// One propagation walk for every recipient: a press propagates from
    /// the source window up to the first window any client selected it on,
    /// and only clients selecting there are told. A peer selecting
    /// ButtonPress on the child under the pointer stops the press there, so
    /// the owner, selecting only on the toplevel above, hears nothing
    /// (t220; the reference's DeliverDeviceEvents). Red before the fix: the
    /// owner's writer walked its own table and delivered at the toplevel.
    #[test]
    fn a_peers_nearer_selection_stops_the_owners_delivery_farther_up() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let shell = owner.next;
        let child = owner.next + 2;
        owner.next += 4;
        for client in [&mut owner, &mut peer] {
            client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        }
        owner.stream
            .write_all(&create_window_request(owner.order, shell, 20, 0, 60, 60))
            .unwrap();
        owner.stream
            .write_all(&change_window_event_mask_request(owner.order, shell, (1 << 2) | (1 << 3)))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, shell)).unwrap();
        owner.stream
            .write_all(&create_window_request_with_parent(owner.order, child, shell, 10, 10, 20, 20))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, child)).unwrap();
        owner.settle();
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, child, 1 << 2))
            .unwrap();
        peer.settle();
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        // Into the child, whose root origin is (30, 10).
        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 35, 15);
        owner.settle();
        owner.fake_input(4, 1);
        let pressed = peer.next_event(4);
        assert_eq!(window_of(&pressed), child, "the peer's press on the child that stopped it");
        owner.assert_quiet("the owner, selecting only above the stop, hears nothing");
        // The press was delivered to the peer, so the implicit grab is the
        // peer's, with the peer's press-only selection as its mask: the
        // release reaches nobody, neither the owner above the stop nor the
        // peer that did not select it (t230).
        owner.fake_input(5, 1);
        owner.assert_quiet("the owner, not the grab's client, hears no release");
        peer.assert_quiet("the peer did not select the release");
    }

    /// Both selecting on the same window are both told there, as before.
    #[test]
    fn selectors_on_the_same_window_are_all_told_there() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let shell = owner.next;
        let child = owner.next + 2;
        owner.next += 4;
        for client in [&mut owner, &mut peer] {
            client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        }
        owner.stream
            .write_all(&create_window_request(owner.order, shell, 20, 0, 60, 60))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, shell)).unwrap();
        owner.stream
            .write_all(&create_window_request_with_parent(owner.order, child, shell, 10, 10, 20, 20))
            .unwrap();
        owner.stream
            .write_all(&change_window_event_mask_request(owner.order, child, 1 << 2))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, child)).unwrap();
        owner.settle();
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, child, 1 << 2))
            .unwrap();
        peer.settle();
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 35, 15);
        owner.settle();
        owner.fake_input(4, 1);
        for (name, client) in [("owner", &mut owner), ("peer", &mut peer)] {
            let pressed = client.next_event(4);
            assert_eq!(window_of(&pressed), child, "{name}: the press on the child");
        }
    }
}
