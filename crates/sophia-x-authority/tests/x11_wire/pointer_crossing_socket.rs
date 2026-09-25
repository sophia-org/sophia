// Pointer crossings over real sockets, on the XTEST fixture: the EnterNotify
// and LeaveNotify a move, a warp or a hierarchy change owes, their focus
// flag, and the peers selecting crossings that are told of them (t211,
// t220). The fixture and its client live in xtest_admission_socket.rs.

#[cfg(unix)]
mod pointer_crossing_socket {
    use super::xtest_admission_socket::{XtestClient, XtestFixture};
    use super::*;
    use std::{io::Write, time::Duration};

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
