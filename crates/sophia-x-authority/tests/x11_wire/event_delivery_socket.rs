// Core event delivery over real sockets, on the XTEST fixture: what an
// injected key, button, motion or warp reaches, in what form and in what
// order (t220, t211, t229). The fixture and its client live in
// xtest_admission_socket.rs; this module borrows them.

#[cfg(unix)]
mod event_delivery_socket {
    use super::xtest_admission_socket::{XtestClient, XtestFixture};
    use super::*;
    use std::{io::Write, time::Duration};

    /// A press nobody selected on the event window or above it is no event
    /// at all (XTS Xlib11 ButtonPress 4), and a client that did not select
    /// it hears nothing when another client did (ButtonPress 6). The press
    /// used to be written to the surface's owner regardless: the implicit
    /// grab a press activates was taken for the owner's own grab, whose mask
    /// admits everything. Red on the tree before the fix: the owner reads a
    /// ButtonPress ahead of its barrier's reply.
    #[test]
    fn a_press_the_owner_did_not_select_is_not_written_to_it() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let window = owner.next;
        owner.next += 2;
        // Keys, ButtonRelease, motion of every kind and FocusChange: what a
        // client may select on a window but ButtonPress, less the crossing
        // masks, whose EnterNotify the motion below would have to read past.
        let all_but_press = 3 | (1 << 3) | (1 << 6) | (1 << 8) | (1 << 13) | (1 << 21);
        owner.stream
            .write_all(&create_window_request(owner.order, window, 20, 0, 16, 16))
            .unwrap();
        owner.stream
            .write_all(&change_window_event_mask_request(owner.order, window, all_but_press))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, window)).unwrap();
        owner.settle();
        let event_window = |event: &[u8; 32]| u32::from_le_bytes([event[12], event[13], event[14], event[15]]);

        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        let motion = owner.next_event(6);
        assert_eq!(event_window(&motion), window, "motion was selected");
        owner.fake_input(4, 1);
        owner.assert_quiet("a press nobody selected is discarded");
        owner.fake_input(5, 1);
        let release = owner.next_event(5);
        assert_eq!(event_window(&release), window, "the release was selected");

        // A peer selecting the press hears it; the owner still does not.
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, window, 1 << 2))
            .unwrap();
        peer.barrier();
        owner.fake_input(4, 1);
        let press = peer.next_event(4);
        assert_eq!(event_window(&press), window, "the peer's press");
        owner.assert_quiet("the owner did not select the press");
        owner.fake_input(5, 1);
        let release = owner.next_event(5);
        assert_eq!(event_window(&release), window, "the owner's release");
        peer.assert_quiet("the peer did not select the release");
    }

    /// A key is selected by direction: a client that selected KeyRelease and
    /// not KeyPress is written the release and not the press (XTS Xlib11
    /// KeyPress 3). The delivery rule checked one combined mask, so a press
    /// reached every client that had selected either. Red on the tree before
    /// the fix: the owner reads a KeyPress ahead of its barrier's reply.
    #[test]
    fn a_key_press_reaches_only_the_clients_that_selected_presses() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let window = fixture.focused_window(&mut owner);
        // KeyRelease, ButtonPress, ButtonRelease and the fixture's FocusChange.
        owner.stream
            .write_all(&change_window_event_mask_request(owner.order, window, (1 << 1) | (1 << 2) | (1 << 3) | (1 << 21)))
            .unwrap();
        owner.settle();
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, window, 1 << 0))
            .unwrap();
        peer.barrier();

        owner.fake_input(2, 38);
        let press = peer.next_event(2);
        assert_eq!(u32::from_le_bytes([press[12], press[13], press[14], press[15]]), window, "the peer's press");
        assert_eq!(press[1], 38);
        owner.assert_quiet("the owner selected releases, not presses");
        owner.fake_input(3, 38);
        let release = owner.next_event(3);
        assert_eq!(release[1], 38, "the owner's release");
        peer.assert_quiet("the peer selected presses, not releases");
    }

    /// A KeymapNotify follows every EnterNotify and FocusIn for a client
    /// that selected KeymapState on the window entered or focused, carrying
    /// the keys down (XTS Xlib11 KeymapNotify 1 and 2). None was written,
    /// and the suite's KeymapNotify 1 binary then crashed on its own
    /// "Missing %s event" report once no stray motion followed the
    /// EnterNotify. Red on the tree before the fix: the record after the
    /// EnterNotify is not a KeymapNotify.
    ///
    /// The pointer moves between two windows of the one client: a motion
    /// onto the root reaches no client's writer, so the return from it
    /// crosses nothing this layer can see (t211).
    #[test]
    fn a_keymap_notify_follows_an_enter_notify_and_a_focus_in() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let first = client.next;
        let second = client.next + 2;
        client.next += 4;
        // KeyPress, KeyRelease, EnterWindow, KeymapState and FocusChange on
        // both windows.
        for (window, x) in [(first, 20), (second, 40)] {
            client.stream
                .write_all(&create_window_request(client.order, window, x, 0, 16, 16))
                .unwrap();
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, 3 | (1 << 4) | (1 << 14) | (1 << 21)))
                .unwrap();
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        // A read that starves is a failure here, not a hang.
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let event_window = |event: &[u8; 32]| u32::from_le_bytes([event[12], event[13], event[14], event[15]]);
        let event_window_of_focus = |event: &[u8; 32]| u32::from_le_bytes([event[4], event[5], event[6], event[7]]);

        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        let entered = client.next_event(7);
        assert_eq!(event_window(&entered), first, "EnterNotify on the first window");
        let keymap = read_x_record(&mut client.stream);
        assert_eq!(keymap[0], 11, "a KeymapNotify follows the EnterNotify: {keymap:?}");
        assert_eq!(keymap[4] & 0x40, 0, "no key is down yet");

        // Keycode 38 held: bit 38 of the bitmap is byte 4, bit 6, and the
        // KeymapNotify carries bytes 1 to 31 of the bitmap at 1 to 31.
        client.fake_input(2, 38);
        let _ = client.next_event(2);
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 45, 5);
        let entered = client.next_event(7);
        assert_eq!(event_window(&entered), second, "EnterNotify on the second window");
        let keymap = read_x_record(&mut client.stream);
        assert_eq!(keymap[0], 11, "a KeymapNotify follows the second EnterNotify: {keymap:?}");
        assert_eq!(keymap[4] & 0x40, 0x40, "keycode 38 is down in the bitmap: {keymap:?}");

        // FocusIn on the first window, then its KeymapNotify. The focus was
        // at the root with the pointer in the second window, so a FocusOut
        // with detail Pointer on the second window comes first.
        let mut request = vec![42, 0];
        push_u16(&mut request, client.order, 3);
        push_u32(&mut request, client.order, first);
        push_u32(&mut request, client.order, 0);
        client.stream.write_all(&request).unwrap();
        let focus = loop {
            let record = read_x_record(&mut client.stream);
            match record[0] & 0x7f {
                9 => break record,
                10 => assert_eq!((event_window_of_focus(&record), record[1]), (second, 5), "the pointer window's FocusOut"),
                other => panic!("expected a focus event, got record {other}: {record:?}"),
            }
        };
        assert_eq!(event_window_of_focus(&focus), first, "FocusIn on the first window");
        let keymap = read_x_record(&mut client.stream);
        assert_eq!(keymap[0], 11, "a KeymapNotify follows the FocusIn: {keymap:?}");
        assert_eq!(keymap[4] & 0x40, 0x40, "keycode 38 is still down: {keymap:?}");
        client.fake_input(3, 38);
        let _ = client.next_event(3);
        client.barrier();
    }

    /// What an injection owes the injecting client reaches its socket before
    /// the reply to its next request. FakeInput's barrier ended at routing,
    /// with the event still in the writer's queue, so a client that injected
    /// a motion and then asked anything saw the reply first and, reading
    /// what was pending after it, nothing (t229; XTS Xlib11 KeymapNotify 1
    /// on a loaded machine). Red on the tree before the fix, on a fraction
    /// of the rounds; the fraction is what a race gives, so the test rounds.
    #[test]
    fn an_injections_events_precede_the_reply_to_the_next_request() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let window = client.next;
        client.next += 2;
        // PointerMotion and EnterWindow.
        client.stream
            .write_all(&create_window_request(client.order, window, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, window, (1 << 4) | (1 << 6)))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

        let mut replies_first = Vec::new();
        for round in 0..40 {
            let x = 25 + (round % 2) as i16;
            client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, x, 5);
            let mut request = vec![43, 0];
            push_u16(&mut request, client.order, 1);
            client.stream.write_all(&request).unwrap();
            // Everything up to the reply, then whatever the motion still
            // owes if it came after the reply.
            let mut before = Vec::new();
            loop {
                let record = read_x_record(&mut client.stream);
                if record[0] == 1 {
                    break;
                }
                before.push(record[0] & 0x7f);
            }
            if !before.contains(&6) {
                replies_first.push(round);
                let _ = client.next_event(6);
            }
        }
        assert!(
            replies_first.is_empty(),
            "the reply overtook the injected motion in rounds {replies_first:?}"
        );
    }

    /// A core event's `child` names the child of the event window on the
    /// way to the source: the source itself when it is a child, the ancestor
    /// of the source that is a child of the event window when it is deeper,
    /// and None when the source is the event window (XTS Xlib11 ButtonPress
    /// 8 to 10, KeyPress 5 to 7, MotionNotify 15 and 16). It was always
    /// None. Red before the fix: the press in the grandchild names no child.
    #[test]
    fn a_core_events_child_is_the_event_windows_child_toward_the_source() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let toplevel = client.next;
        let child = client.next + 2;
        let grandchild = client.next + 4;
        client.next += 6;
        // Keys, ButtonPress and ButtonRelease on the toplevel only.
        client.stream
            .write_all(&create_window_request(client.order, toplevel, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, toplevel, 3 | (1 << 2) | (1 << 3)))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, child, toplevel, 4, 4, 8, 8))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, grandchild, child, 2, 2, 4, 4))
            .unwrap();
        for window in [toplevel, child, grandchild] {
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let field = |event: &[u8; 32], at: usize| u32::from_le_bytes([event[at], event[at + 1], event[at + 2], event[at + 3]]);

        // In the grandchild (root 26..30, 6..10): the event window is the
        // toplevel and its child toward the source is the child.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (toplevel, child), "press in the grandchild");
        client.fake_input(5, 1);
        let _ = client.next_event(5);
        client.fake_input(2, 38);
        let key = client.next_event(2);
        assert_eq!((field(&key, 12), field(&key, 16)), (toplevel, child), "key with the pointer in the grandchild");
        client.fake_input(3, 38);
        let _ = client.next_event(3);

        // In the child (root 24..32, 4..12) outside the grandchild: the child
        // is the source and the subwindow.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (toplevel, child), "press in the child");
        client.fake_input(5, 1);
        let _ = client.next_event(5);

        // In the toplevel itself: no subwindow.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 21, 1);
        client.fake_input(4, 1);
        let press = client.next_event(4);
        assert_eq!((field(&press, 12), field(&press, 16)), (toplevel, 0), "press in the toplevel");
        client.fake_input(5, 1);
        let _ = client.next_event(5);
        client.barrier();
    }

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

    /// An injected wheel button is a button to the core protocol: its press
    /// and release are ButtonPress and ButtonRelease with its detail, and
    /// motion while it is held carries Button4Mask and answers to
    /// Button4Motion (XTS Xlib11 MotionNotify 6 and 7). XTEST dropped it
    /// as a wheel step it could not carry. Red before the fix: no press
    /// arrives.
    #[test]
    fn an_injected_wheel_button_is_held_like_any_button() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let window = client.next;
        client.next += 2;
        // ButtonPress, ButtonRelease and Button4Motion.
        client.stream
            .write_all(&create_window_request(client.order, window, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, window, (1 << 2) | (1 << 3) | (1 << 11)))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let state = |event: &[u8; 32]| u16::from_le_bytes([event[28], event[29]]);

        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        client.fake_input(4, 4);
        let press = client.next_event(4);
        assert_eq!((press[1], state(&press)), (4, 0), "button 4 pressed with nothing held");
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 26, 6);
        let motion = client.next_event(6);
        assert_eq!(state(&motion), 1 << 11, "motion with button 4 held carries Button4Mask");
        client.fake_input(5, 4);
        let release = client.next_event(5);
        assert_eq!((release[1], state(&release)), (4, 1 << 11), "button 4 released, still in the state");
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        client.assert_quiet("plain motion is not selected");
    }

    /// A pointer move generates the protocol's crossings: leaves from the
    /// window left up to the common ancestor, then enters down to the
    /// window entered, each with its detail (Ancestor, Virtual, Inferior,
    /// Nonlinear, NonlinearVirtual), its child toward the pointer and
    /// coordinates in its own window; a move out of every window of the
    /// client is a move to the root (XTS Xlib11 EnterNotify 3, 4, 7 to 9,
    /// 12, 13; LeaveNotify 4, 5, 8 to 10, 14, 15). The crossing was one
    /// EnterNotify of detail Nonlinear on the window a motion was reported
    /// on, and nothing when the pointer left. Red before the fix: the first
    /// move into the grandchild produces one record where four are owed.
    #[test]
    fn a_pointer_move_generates_the_protocols_crossings() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let toplevel = client.next;
        let child = client.next + 2;
        let grandchild = client.next + 4;
        let other = client.next + 6;
        client.next += 8;
        let root = X_SETUP_DEFAULT_ROOT;
        // EnterWindow and LeaveWindow everywhere, the root included.
        client.stream
            .write_all(&create_window_request(client.order, toplevel, 20, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, child, toplevel, 4, 4, 8, 8))
            .unwrap();
        client.stream
            .write_all(&create_window_request_with_parent(client.order, grandchild, child, 2, 2, 4, 4))
            .unwrap();
        client.stream
            .write_all(&create_window_request(client.order, other, 40, 0, 16, 16))
            .unwrap();
        for window in [toplevel, child, grandchild, other, root] {
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, (1 << 4) | (1 << 5)))
                .unwrap();
        }
        for window in [toplevel, child, grandchild, other] {
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let field = |event: &[u8; 32], at: usize| u32::from_le_bytes([event[at], event[at + 1], event[at + 2], event[at + 3]]);
        let at = |event: &[u8; 32], offset: usize| i16::from_le_bytes([event[offset], event[offset + 1]]);
        // (type, detail, window, child) of every crossing up to the barrier.
        let crossings = |client: &mut XtestClient| {
            let mut request = vec![43, 0];
            push_u16(&mut request, client.order, 1);
            client.stream.write_all(&request).unwrap();
            let mut seen = Vec::new();
            loop {
                let record = read_x_record(&mut client.stream);
                match record[0] & 0x7f {
                    1 => break seen,
                    7 | 8 => seen.push((record[0] & 0x7f, record[1], field(&record, 12), field(&record, 16), at(&record, 24), at(&record, 26))),
                    6 => {}
                    other => panic!("unexpected record {other}: {record:?}"),
                }
            }
        };
        const ANCESTOR: u8 = 0;
        const VIRTUAL: u8 = 1;
        const INFERIOR: u8 = 2;
        const NONLINEAR: u8 = 3;
        const NONLINEAR_VIRTUAL: u8 = 4;

        // From the root into the grandchild (root 26..30, 6..10): the root
        // is left toward the toplevel, the windows between are entered
        // virtually, the grandchild as Ancestor; coordinates in each window.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        assert_eq!(
            crossings(&mut client),
            vec![
                (8, INFERIOR, root, toplevel, 27, 7),
                (7, VIRTUAL, toplevel, child, 7, 7),
                (7, VIRTUAL, child, grandchild, 3, 3),
                (7, ANCESTOR, grandchild, 0, 1, 1),
            ],
            "root to grandchild"
        );
        // Up to the toplevel: the grandchild is left as Ancestor, the child
        // virtually, the toplevel entered as Inferior toward the child.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 21, 1);
        assert_eq!(
            crossings(&mut client),
            vec![
                (8, ANCESTOR, grandchild, 0, -5, -5),
                (8, VIRTUAL, child, grandchild, -3, -3),
                (7, INFERIOR, toplevel, child, 1, 1),
            ],
            "grandchild to toplevel"
        );
        // Out to the root: no window of the client is under the pointer.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 5, 5);
        assert_eq!(
            crossings(&mut client),
            vec![(8, ANCESTOR, toplevel, 0, -15, 5), (7, INFERIOR, root, toplevel, 5, 5)],
            "toplevel to root"
        );
        // Into the other toplevel, then across to the grandchild: siblings
        // under the root, so the root is the common ancestor and hears
        // nothing, and the windows between are crossed as NonlinearVirtual.
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 45, 5);
        assert_eq!(
            crossings(&mut client),
            vec![(8, INFERIOR, root, other, 45, 5), (7, ANCESTOR, other, 0, 5, 5)],
            "root to the other toplevel"
        );
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 27, 7);
        assert_eq!(
            crossings(&mut client),
            vec![
                (8, NONLINEAR, other, 0, -13, 7),
                (7, NONLINEAR_VIRTUAL, toplevel, child, 7, 7),
                (7, NONLINEAR_VIRTUAL, child, grandchild, 3, 3),
                (7, NONLINEAR, grandchild, 0, 1, 1),
            ],
            "the other toplevel to the grandchild"
        );
    }

    /// A crossing's focus flag says whether the event window is the focus
    /// window or one of its inferiors (XTS Xlib11 EnterNotify 12, LeaveNotify
    /// 14); it was always set. And a window destroyed while the pointer was
    /// in it leaves the pointer in the root as far as its client's next
    /// crossing is concerned, not in a window nobody knows: the enter of the
    /// next window is Ancestor from the root, not Nonlinear from nowhere
    /// (EnterNotify 3). Red before the fix on both counts.
    #[test]
    fn a_crossings_focus_flag_follows_the_focus_and_a_destroyed_window_leaves_the_pointer_in_the_root() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let first = client.next;
        let second = client.next + 2;
        client.next += 4;
        for (window, x) in [(first, 20), (second, 40)] {
            client.stream
                .write_all(&create_window_request(client.order, window, x, 0, 16, 16))
                .unwrap();
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, (1 << 4) | (1 << 5) | (1 << 21)))
                .unwrap();
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let set_focus = |client: &mut XtestClient, window: u32| {
            let mut request = vec![42, 0];
            push_u16(&mut request, client.order, 3);
            push_u32(&mut request, client.order, window);
            push_u32(&mut request, client.order, 0);
            client.stream.write_all(&request).unwrap();
            // Read past the focus events to the barrier.
            client.settle();
        };

        // The focus on the first window: entering it is entering the focus.
        set_focus(&mut client, first);
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        let entered = client.next_event(7);
        assert_eq!((entered[1], entered[31] & 1), (0, 1), "enter of the focus window, detail Ancestor, focus set");
        // The focus on the second window: leaving the first is leaving a
        // window outside the focus, and entering the second is entering it.
        set_focus(&mut client, second);
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 45, 5);
        let left = client.next_event(8);
        assert_eq!((left[1], left[31] & 1), (3, 0), "leave of the first window, Nonlinear, focus clear");
        let entered = client.next_event(7);
        assert_eq!((entered[1], entered[31] & 1), (3, 1), "enter of the second window, Nonlinear, focus set");

        // The second window is destroyed under the pointer, and a third
        // takes its place: the pointer comes from the root, and the map
        // that puts the third window under it is what generates the enter.
        let third = client.next;
        client.next += 2;
        let mut destroy = vec![4, 0];
        push_u16(&mut destroy, client.order, 2);
        push_u32(&mut destroy, client.order, second);
        client.stream.write_all(&destroy).unwrap();
        client.stream
            .write_all(&create_window_request(client.order, third, 40, 0, 16, 16))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, third, (1 << 4) | (1 << 5)))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, third)).unwrap();
        // The destroyed window was the focus, so its FocusOut comes first.
        let entered = loop {
            let record = read_x_record(&mut client.stream);
            match record[0] & 0x7f {
                7 => break record,
                6 | 9 | 10 => {}
                other => panic!("expected an enter, got record {other}: {record:?}"),
            }
        };
        assert_eq!(
            (u32::from_le_bytes([entered[12], entered[13], entered[14], entered[15]]), entered[1]),
            (third, 0),
            "enter of the third window from the root, detail Ancestor"
        );
    }

    /// The focus flag of a crossing says whether the event window is the
    /// focus window or one of its inferiors, for the subwindows of one
    /// toplevel as for toplevels: with the focus on a sibling subwindow
    /// the flag is clear on the enter and on the leave (XTS Xlib11
    /// EnterNotify 12, LeaveNotify 14).
    #[test]
    fn a_crossings_focus_flag_is_clear_when_the_focus_is_a_sibling_subwindow() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let guardian = client.next;
        let first = client.next + 2;
        let second = client.next + 4;
        client.next += 6;
        client.stream
            .write_all(&create_window_request(client.order, guardian, 20, 20, 100, 40))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, guardian)).unwrap();
        for (window, x) in [(first, 10), (second, 50)] {
            client.stream
                .write_all(&create_window_request_with_parent(client.order, window, guardian, x, 0, 20, 20))
                .unwrap();
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.stream
            .write_all(&change_window_event_mask_request(client.order, first, (1 << 4) | (1 << 5)))
            .unwrap();
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let set_focus = |client: &mut XtestClient, window: u32| {
            let mut request = vec![42, 0];
            push_u16(&mut request, client.order, 3);
            push_u32(&mut request, client.order, window);
            push_u32(&mut request, client.order, 0);
            client.stream.write_all(&request).unwrap();
            client.settle();
        };
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);

        set_focus(&mut client, first);
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 35, 25);
        let entered = client.next_event(7);
        assert_eq!((window_of(&entered), entered[31] & 1), (first, 1), "enter of the focus subwindow, focus set");
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 0, 0);
        let left = client.next_event(8);
        assert_eq!((window_of(&left), left[31] & 1), (first, 1), "leave of the focus subwindow, focus set");

        set_focus(&mut client, second);
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 35, 25);
        let entered = client.next_event(7);
        assert_eq!((window_of(&entered), entered[31] & 1), (first, 0), "enter of the sibling of the focus, focus clear");
        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 0, 0);
        let left = client.next_event(8);
        assert_eq!((window_of(&left), left[31] & 1), (first, 0), "leave of the sibling of the focus, focus clear");
    }

    /// The same through WarpPointer, as the suite moves the pointer: an
    /// admitted client's warp becomes a motion, and the enter it generates
    /// carries the focus flag of the focus at that moment.
    #[test]
    fn a_warps_enter_carries_the_focus_flag_of_the_focus_at_the_time() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let guardian = client.next;
        let first = client.next + 2;
        let second = client.next + 4;
        client.next += 6;
        client.stream
            .write_all(&create_window_request(client.order, guardian, 2, 2, 400, 300))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, guardian)).unwrap();
        for (window, x) in [(first, 2), (second, 60)] {
            client.stream
                .write_all(&create_window_request_with_parent(client.order, window, guardian, x, 2, 50, 50))
                .unwrap();
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.stream.write_all(&change_window_event_mask_request(client.order, first, 1 << 4)).unwrap();
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let set_focus = |client: &mut XtestClient, window: u32| {
            let mut request = vec![42, 1];
            push_u16(&mut request, client.order, 3);
            push_u32(&mut request, client.order, window);
            push_u32(&mut request, client.order, 0);
            client.stream.write_all(&request).unwrap();
            client.settle();
        };
        let warp_to = |client: &mut XtestClient, window: u32| {
            let mut request = vec![41, 0];
            push_u16(&mut request, client.order, 6);
            push_u32(&mut request, client.order, 0);
            push_u32(&mut request, client.order, window);
            request.extend_from_slice(&[0; 12]);
            // No barrier: the connection does not read its next request
            // until the warp's motion has been routed, and a barrier here
            // would read past the enter.
            client.stream.write_all(&request).unwrap();
        };
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);

        set_focus(&mut client, first);
        warp_to(&mut client, 0x20);
        warp_to(&mut client, first);
        let entered = client.next_event(7);
        assert_eq!((window_of(&entered), entered[31] & 1), (first, 1), "enter of the focus subwindow, focus set");
        warp_to(&mut client, 0x20);
        set_focus(&mut client, second);
        warp_to(&mut client, first);
        let entered = client.next_event(7);
        assert_eq!((window_of(&entered), entered[31] & 1), (first, 0), "enter of the sibling of the focus, focus clear");
    }

    /// A client that selects LeaveWindow on a window the pointer is already
    /// in is told when the pointer leaves, as the owner is, and a client
    /// that selected nothing is not (XTS Xlib11 LeaveNotify 2).
    #[test]
    fn a_peer_that_selects_leave_while_the_pointer_is_inside_is_told_of_the_leave() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let mut bystander = fixture.connect();
        let window = owner.next;
        owner.next += 2;
        for client in [&mut owner, &mut peer, &mut bystander] {
            client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        }
        owner.stream
            .write_all(&create_window_request(owner.order, window, 20, 0, 16, 16))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, window)).unwrap();
        owner.settle();
        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        owner.settle();
        for (client, mask) in [(&mut owner, 1 << 5), (&mut peer, 1 << 5), (&mut bystander, 0)] {
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, mask))
                .unwrap();
            client.settle();
        }
        let window_of = |record: &[u8]| u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 0, 0);
        let left = owner.next_event(8);
        assert_eq!((window_of(&left), left[1]), (window, 0), "the owner's leave, detail Ancestor");
        let left = peer.next_event(8);
        assert_eq!((window_of(&left), left[1]), (window, 0), "the peer's leave, detail Ancestor");
        owner.assert_quiet("the owner after the leave");
        peer.assert_quiet("the peer after the leave");
        bystander.assert_quiet("a client that selected nothing");
    }

    /// A peer that selected EnterWindow and KeymapState on the owner's window
    /// and nothing else is told of the pointer entering it, with the
    /// KeymapNotify after (XTS Xlib11 KeymapNotify 3). The fan-out carried
    /// motion to motion selectors alone, so the peer never saw the pointer
    /// arrive. Red before the fix: the peer reads nothing.
    #[test]
    fn a_peer_selecting_only_crossings_is_told_of_the_pointer_entering() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let window = owner.next;
        owner.next += 2;
        owner.stream
            .write_all(&create_window_request(owner.order, window, 20, 0, 16, 16))
            .unwrap();
        owner.stream.write_all(&map_window_request(owner.order, window)).unwrap();
        owner.barrier();
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, window, (1 << 4) | (1 << 14)))
            .unwrap();
        peer.barrier();
        peer.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

        owner.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        let entered = peer.next_event(7);
        assert_eq!(u32::from_le_bytes([entered[12], entered[13], entered[14], entered[15]]), window, "the peer's EnterNotify");
        let keymap = read_x_record(&mut peer.stream);
        assert_eq!(keymap[0], 11, "and its KeymapNotify: {keymap:?}");
        owner.assert_quiet("the owner selected nothing");
    }

    /// An event routed to a client before it sends a request is written
    /// before that request's reply. The protocol writer and the request
    /// loop were two threads with nothing between them, so a PropertyNotify
    /// queued for a watching peer could follow the reply to the peer's next
    /// request, and the peer, having synced, found nothing pending (the
    /// peer-side half of t229; XTS Xlib4 XMapWindow 6 under load). The race
    /// needs the writer descheduled, which an idle machine does not do: on
    /// the tree before the fix this read green here and red under the
    /// gate's load. The watermark's own wait is red-then-green in
    /// x11_socket/tests/protocol_watermark.rs; this stands as the guard.
    #[test]
    fn an_event_routed_before_a_request_precedes_its_reply() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut owner = fixture.connect();
        let mut peer = fixture.connect();
        let window = owner.next;
        owner.next += 2;
        owner.stream
            .write_all(&create_window_request(owner.order, window, 20, 0, 16, 16))
            .unwrap();
        owner.barrier();
        // PropertyChange on the owner's window.
        peer.stream
            .write_all(&change_window_event_mask_request(peer.order, window, 1 << 22))
            .unwrap();
        peer.barrier();
        peer.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

        // A burst of changes to WM_NAME as STRING: the owner's round trip
        // proves every PropertyNotify is queued for the peer before the peer
        // asks anything, and the peer's writer is still writing them when
        // the reply is due.
        // Under the fixture's queue capacity of sixteen, which a stalled
        // recipient would otherwise fall to.
        const BURST: usize = 12;
        let mut overtaken = Vec::new();
        for round in 0..8_u8 {
            for step in 0..BURST as u8 {
                owner.stream
                    .write_all(&change_property_request(owner.order, XPropertyMode::Replace, window, 39, 31, 8, &[round, step]))
                    .unwrap();
            }
            owner.barrier();
            let mut request = vec![43, 0];
            push_u16(&mut request, peer.order, 1);
            peer.stream.write_all(&request).unwrap();
            let mut before = 0;
            loop {
                let record = read_x_record(&mut peer.stream);
                if record[0] == 1 {
                    break;
                }
                assert_eq!(record[0] & 0x7f, 28, "only PropertyNotify was selected: {record:?}");
                before += 1;
            }
            if before < BURST {
                overtaken.push((round, before));
                for _ in before..BURST {
                    let _ = peer.next_event(28);
                }
            }
        }
        assert!(
            overtaken.is_empty(),
            "the reply overtook queued PropertyNotify events in (round, written before it): {overtaken:?}"
        );
    }

    /// VisibilityNotify follows occlusion: a window mapped clear is
    /// Unobscured, a sibling mapped over part of it makes it
    /// PartiallyObscured, one moved to cover it FullyObscured, and its unmap
    /// Unobscured again; a window mapped under a cover reports the cover
    /// from the start (XTS Xlib11 VisibilityNotify 2, 3, 7 to 9). The
    /// state was Unobscured on map and never changed. Red before the fix.
    /// Remapping a window reports VisibilityNotify and Expose on every
    /// mapped inferior that becomes viewable with it, the VisibilityNotify
    /// before the Expose on each (XTS Xlib11 VisibilityNotify 3). Only the
    /// remapped window was reported; its children were not.
    #[test]
    fn remapping_a_window_reports_visibility_and_exposure_on_its_viewable_inferiors() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let parent = client.next;
        let child = client.next + 2;
        let grandchild = client.next + 4;
        client.next += 6;
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        client.stream
            .write_all(&create_window_request(client.order, parent, 20, 0, 40, 40))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, parent)).unwrap();
        // VisibilityChange and Exposure on the two below it.
        for (window, above, x) in [(child, parent, 2), (grandchild, child, 1)] {
            client.stream
                .write_all(&create_window_request_with_parent(client.order, window, above, x, x, 10, 10))
                .unwrap();
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, (1 << 15) | (1 << 16)))
                .unwrap();
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();

        let mut unmap = vec![10, 0];
        push_u16(&mut unmap, client.order, 2);
        push_u32(&mut unmap, client.order, child);
        client.stream.write_all(&unmap).unwrap();
        client.settle();
        client.stream.write_all(&map_window_request(client.order, child)).unwrap();
        let mut order = std::collections::BTreeMap::<u32, Vec<u8>>::new();
        for _ in 0..4 {
            let record = read_x_record(&mut client.stream);
            let window = u32::from_le_bytes([record[4], record[5], record[6], record[7]]);
            order.entry(window).or_default().push(record[0] & 0x7f);
        }
        assert_eq!(
            order,
            [(child, vec![15, 12]), (grandchild, vec![15, 12])].into_iter().collect(),
            "VisibilityNotify then Expose on the remapped window and on its viewable child"
        );
        client.assert_quiet("nothing else on the remap");
    }

    #[test]
    fn visibility_follows_what_covers_a_window() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let lower = client.next;
        let upper = client.next + 2;
        let under = client.next + 4;
        client.next += 6;
        client.stream
            .write_all(&create_window_request(client.order, lower, 20, 0, 16, 16))
            .unwrap();
        // VisibilityChange only.
        client.stream
            .write_all(&change_window_event_mask_request(client.order, lower, 1 << 16))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, lower)).unwrap();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let visibility = |client: &mut XtestClient, what: &str| {
            let record = read_x_record(&mut client.stream);
            assert_eq!(record[0] & 0x7f, 15, "{what}: a VisibilityNotify: {record:?}");
            (u32::from_le_bytes([record[4], record[5], record[6], record[7]]), record[8])
        };
        assert_eq!(visibility(&mut client, "mapped clear"), (lower, 0));
        client.assert_quiet("nothing else on map");

        // A sibling over its right half.
        client.stream
            .write_all(&create_window_request(client.order, upper, 28, 0, 16, 16))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, upper)).unwrap();
        assert_eq!(visibility(&mut client, "partly covered"), (lower, 1));
        client.assert_quiet("one report per change");

        // Moved to cover it whole.
        client.stream
            .write_all(&configure_window_request(client.order, upper, 1, &[20]))
            .unwrap();
        assert_eq!(visibility(&mut client, "fully covered"), (lower, 2));
        client.assert_quiet("one report per change");

        // A window mapped under the cover reports the cover from the start:
        // a new window stacks on top, so the cover is raised above it first.
        client.stream
            .write_all(&create_window_request(client.order, under, 24, 4, 8, 8))
            .unwrap();
        client.stream
            .write_all(&change_window_event_mask_request(client.order, under, 1 << 16))
            .unwrap();
        client.stream
            .write_all(&configure_window_request(client.order, upper, 1 << 6, &[0]))
            .unwrap();
        client.stream.write_all(&map_window_request(client.order, under)).unwrap();
        assert_eq!(visibility(&mut client, "mapped under a cover"), (under, 2));
        client.assert_quiet("the covered window did not change");

        // The cover unmapped: the window it hid comes clear, and the lower
        // one is now partly covered by that window alone.
        let mut unmap = vec![10, 0];
        push_u16(&mut unmap, client.order, 2);
        push_u32(&mut unmap, client.order, upper);
        client.stream.write_all(&unmap).unwrap();
        let mut cleared = vec![visibility(&mut client, "uncovered"), visibility(&mut client, "uncovered")];
        cleared.sort_unstable();
        assert_eq!(cleared, vec![(lower, 1), (under, 0)]);
        client.assert_quiet("one report each");
    }
    /// A hierarchy change under the pointer generates the crossings a move
    /// would: unmapping the window the pointer is in leaves it for the window
    /// beneath, after the UnmapNotify, and mapping it again enters it
    /// (XTS Xlib11 EnterNotify 1, LeaveNotify 1). Nothing was generated:
    /// the pointer's window changed with no motion to carry the news. The
    /// position is routed again after such a request, as a motion of no
    /// distance that writes crossings and no MotionNotify. Red before the
    /// fix: nothing follows the UnmapNotify.
    #[test]
    fn unmapping_the_window_under_the_pointer_crosses_to_the_one_beneath() {
        let mut fixture = XtestFixture::sharing_a_namespace();
        let mut client = fixture.connect();
        let lower = client.next;
        let upper = client.next + 2;
        client.next += 4;
        // EnterWindow, LeaveWindow, PointerMotion and StructureNotify on
        // both, over the same area; the upper one is created last and so
        // stacks on top.
        for window in [lower, upper] {
            client.stream
                .write_all(&create_window_request(client.order, window, 20, 0, 16, 16))
                .unwrap();
            client.stream
                .write_all(&change_window_event_mask_request(client.order, window, (1 << 4) | (1 << 5) | (1 << 6) | (1 << 17)))
                .unwrap();
            client.stream.write_all(&map_window_request(client.order, window)).unwrap();
        }
        client.settle();
        client.stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let window_of = |event: &[u8; 32]| u32::from_le_bytes([event[12], event[13], event[14], event[15]]);
        let records_until_quiet = |client: &mut XtestClient| {
            let mut request = vec![43, 0];
            push_u16(&mut request, client.order, 1);
            client.stream.write_all(&request).unwrap();
            let mut seen = Vec::new();
            loop {
                let record = read_x_record(&mut client.stream);
                if record[0] == 1 {
                    break seen;
                }
                seen.push((record[0] & 0x7f, record[1], window_of(&record)));
            }
        };

        client.fake_input_at(6, X_TEST_MOTION_ABSOLUTE, 25, 5);
        assert_eq!(
            records_until_quiet(&mut client),
            vec![(7, 0, upper), (6, 0, upper)],
            "entered the upper window, with its motion"
        );
        // Unmap the upper window: UnmapNotify first, then the pointer leaves
        // it for the lower one, siblings under the root, and no motion.
        let mut unmap = vec![10, 0];
        push_u16(&mut unmap, client.order, 2);
        push_u32(&mut unmap, client.order, upper);
        client.stream.write_all(&unmap).unwrap();
        let seen = records_until_quiet(&mut client);
        assert_eq!(seen[0].0, 18, "UnmapNotify first: {seen:?}");
        assert_eq!(
            &seen[1..],
            &[(8, 3, upper), (7, 3, lower)],
            "then the crossing from the unmapped window to the one beneath, and no motion: {seen:?}"
        );
        // Map it again: MapNotify, then the pointer crosses back into it.
        client.stream.write_all(&map_window_request(client.order, upper)).unwrap();
        let seen = records_until_quiet(&mut client);
        assert_eq!(seen[0].0, 19, "MapNotify first: {seen:?}");
        assert_eq!(
            &seen[1..],
            &[(8, 3, lower), (7, 3, upper)],
            "then the crossing back into the mapped window: {seen:?}"
        );
    }
}
