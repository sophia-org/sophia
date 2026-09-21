// `_NET_ACTIVE_WINDOW` on the root, over a real socket, on both paths that
// move the input focus: the session's own focus command through the control
// writer, and a client's SetInputFocus through request dispatch. What is
// asserted is that the property is a value from the first connection -- None
// is 0, never an absent property, because a toolkit that finds it absent asks
// for the name of the missing type and is exited for it -- and that it says
// the focused window after either path has moved the focus.

#[cfg(unix)]
mod active_window_socket {
    use super::xtest_admission_socket::{XtestClient, XtestFixture};
    use super::*;
    use std::io::Write;

    fn intern(client: &mut XtestClient, name: &str) -> u32 {
        client
            .stream
            .write_all(&intern_atom_request(client.order, false, name))
            .unwrap();
        let reply = read_x_record(&mut client.stream);
        assert_eq!(reply[0], 1);
        u32::from_le_bytes([reply[8], reply[9], reply[10], reply[11]])
    }

    fn active_window(client: &mut XtestClient, property: u32) -> Option<u32> {
        client
            .stream
            .write_all(&get_property_request(client.order, false, X_SETUP_DEFAULT_ROOT, property, 0, 0, 1))
            .unwrap();
        let reply = read_x_reply(&mut client.stream, client.order);
        assert_eq!(reply[0], 1);
        let property_type = u32::from_le_bytes([reply[8], reply[9], reply[10], reply[11]]);
        if property_type == 0 {
            return None;
        }
        assert_eq!(reply[1], 32, "format");
        assert_eq!(u32::from_le_bytes([reply[16], reply[17], reply[18], reply[19]]), 1, "one window");
        Some(u32::from_le_bytes([reply[32], reply[33], reply[34], reply[35]]))
    }

    /// SetInputFocus, then the focus event the window selected for -- it
    /// arrives before the barrier's reply, and a barrier that read it as the
    /// reply would fail on an event that is exactly right.
    fn set_input_focus(client: &mut XtestClient, window: u32, event: (bool, u32)) {
        let mut request = vec![42, 0]; // SetInputFocus, revert_to None
        push_u16(&mut request, client.order, 3);
        push_u32(&mut request, client.order, window);
        push_u32(&mut request, client.order, 0); // CurrentTime
        client.stream.write_all(&request).unwrap();
        // Both transitions here are between a toplevel and None, which
        // the protocol reads as nonlinear on the window's own event.
        assert_core_focus_event(
            &mut client.stream,
            event.0,
            event.1,
            X_FOCUS_DETAIL_NONLINEAR,
        );
        client.barrier();
    }

    #[test]
    fn the_root_says_which_window_has_focus_on_both_paths_that_move_it() {
        let mut fixture = XtestFixture::new();
        let mut client = fixture.connect();
        let property = intern(&mut client, "_NET_ACTIVE_WINDOW");

        // A value from the first connection, before anything was focused.
        assert_eq!(active_window(&mut client, property), Some(0), "seeded as None, not absent");

        // The session's focus command, through the control writer.
        let window = fixture.focused_window(&mut client);
        assert_eq!(active_window(&mut client, property), Some(window));

        // The client's own SetInputFocus, through request dispatch: None
        // publishes as 0, and the window again as itself.
        set_input_focus(&mut client, 0, (false, window));
        assert_eq!(active_window(&mut client, property), Some(0));
        set_input_focus(&mut client, window, (true, window));
        assert_eq!(active_window(&mut client, property), Some(window));
    }
}
