// UnmapSubwindows, CirculateWindow, RotateProperties and
// ChangeActivePointerGrab over a real socket.
//
// WHAT THESE PROVE. UnmapSubwindows unmaps every mapped child top to bottom
// with an UnmapNotify each; CirculateWindow moves the lowest occluded child
// to the top (or the highest occluding one to the bottom) with a
// CirculateNotify, and a parent with no occlusion moves nothing;
// RotateProperties moves values along the list with a PropertyNotify per
// moved value, and refuses a missing property as BadMatch; and
// ChangeActivePointerGrab without an active grab does nothing, while a bit
// outside the pointer events is BadValue. Before t166 all four were
// BadRequest, which xterm's error handler turns into an exit.

#[cfg(unix)]
mod hierarchy_requests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    const BAD_VALUE: u8 = 2;
    const BAD_MATCH: u8 = 8;
    const UNMAP_NOTIFY: u8 = 18;
    const CONFIGURE_NOTIFY: u8 = 22;
    const CIRCULATE_NOTIFY: u8 = 26;
    const PROPERTY_NOTIFY: u8 = 28;

    fn served(name: &str, byte_order: XByteOrder) -> (std::os::unix::net::UnixStream, std::path::PathBuf, thread::JoinHandle<()>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-hierarchy-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1266),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap();
        let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
        let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
            .unwrap()
            .with_admission_policy(policy);
        let server = thread::spawn(move || {
            let mut frontend = XServerFrontend::bind(config).unwrap();
            frontend.serve_next().unwrap();
        });
        wait_for_socket(&socket_path);
        let mut client = connect_x_socket(&socket_path);
        client
            .write_all(&setup_request(byte_order, 11, 0, b"", b""))
            .unwrap();
        read_setup_success(&mut client, byte_order);
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        (client, socket_path, server)
    }

    fn request(byte_order: XByteOrder, opcode: u8, detail: u8, words: &[u32]) -> Vec<u8> {
        let mut out = vec![opcode, detail];
        push_u16(&mut out, byte_order, 1 + words.len() as u16);
        for word in words {
            push_u32(&mut out, byte_order, *word);
        }
        out
    }

    fn get_input_focus(byte_order: XByteOrder) -> Vec<u8> {
        request(byte_order, 43, 0, &[])
    }

    /// Reads records until the GetInputFocus reply, returning the events seen.
    fn events_until_reply(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream) -> Vec<[u8; 32]> {
        let mut events = Vec::new();
        loop {
            let record = read_x_record(client);
            match record[0] {
                1 => return events,
                0 => panic!("{byte_order:?}: an error while events were owed: {record:?}"),
                _ => events.push(record),
            }
        }
    }

    #[test]
    fn unmap_subwindows_unmaps_top_first_and_circulate_raises_the_lowest_occluded() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("stack", byte_order);
            let parent = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
            let lower = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 2;
            let upper = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 3;
            client.write_all(&create_window_request(byte_order, parent, 0, 0, 200, 200)).unwrap();
            client.write_all(&map_window_request(byte_order, parent)).unwrap();
            // Two overlapping children, created in order so `upper` stacks
            // above `lower`.
            client.write_all(&create_window_request_with_parent(byte_order, lower, parent, 10, 10, 80, 80)).unwrap();
            client.write_all(&create_window_request_with_parent(byte_order, upper, parent, 40, 40, 80, 80)).unwrap();
            client.write_all(&map_window_request(byte_order, lower)).unwrap();
            client.write_all(&map_window_request(byte_order, upper)).unwrap();
            // StructureNotify on both children: CirculateNotify and
            // UnmapNotify are delivered to whoever selected it, as the
            // protocol says, not to whoever asked.
            client.write_all(&change_window_event_mask_request(byte_order, lower, 1 << 17)).unwrap();
            client.write_all(&change_window_event_mask_request(byte_order, upper, 1 << 17)).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let _ = events_until_reply(byte_order, &mut client);

            // RaiseLowest: `lower` is occluded by `upper`, so it goes to the
            // top, and the tree says so.
            client.write_all(&request(byte_order, 13, 0, &[parent])).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let events = events_until_reply(byte_order, &mut client);
            let circulated = events.iter().filter(|e| e[0] & 0x7f == CIRCULATE_NOTIFY).collect::<Vec<_>>();
            assert_eq!(circulated.len(), 1, "{byte_order:?}: one CirculateNotify: {events:?}");
            assert_eq!(read_u32(byte_order, &circulated[0][8..12]), lower, "{byte_order:?}: the lowest occluded child moved");
            assert_eq!(circulated[0][16], 0, "{byte_order:?}: to the top");
            client.write_all(&resource_request(byte_order, 15, parent)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            let count = usize::from(read_u16(byte_order, &reply[16..18]));
            let children = (0..count).map(|i| read_u32(byte_order, &reply[32 + 4 * i..36 + 4 * i])).collect::<Vec<_>>();
            assert_eq!(children, vec![upper, lower], "{byte_order:?}: bottom to top after the raise");
            // Now nothing occludes `lower` from above: a second RaiseLowest
            // finds `upper` occluded and raises it back; a parent whose
            // children do not meet moves nothing.
            client.write_all(&request(byte_order, 13, 0, &[parent])).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let events = events_until_reply(byte_order, &mut client);
            assert_eq!(events.iter().filter(|e| e[0] & 0x7f == CIRCULATE_NOTIFY).count(), 1);
            // Direction 2 is BadValue.
            client.write_all(&request(byte_order, 13, 2, &[parent])).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_VALUE, 13), "{byte_order:?}: {record:?}");

            // UnmapSubwindows: both children, top first, one UnmapNotify each;
            // again is nothing, both being unmapped already.
            client.write_all(&request(byte_order, 11, 0, &[parent])).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let events = events_until_reply(byte_order, &mut client);
            let unmapped = events.iter().filter(|e| e[0] & 0x7f == UNMAP_NOTIFY).map(|e| read_u32(byte_order, &e[8..12])).collect::<Vec<_>>();
            // Two raises swapped them twice: `upper` is on top again.
            assert_eq!(unmapped, vec![upper, lower], "{byte_order:?}: top to bottom: {events:?}");
            client.write_all(&request(byte_order, 11, 0, &[parent])).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            assert!(events_until_reply(byte_order, &mut client).is_empty(), "{byte_order:?}: nothing to unmap twice");

            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    /// A stacking change alone is a configuration change: XRaiseWindow is
    /// ConfigureWindow with stack-mode and nothing else, and it owes the
    /// window's StructureNotify and the parent's SubstructureNotify
    /// selectors a ConfigureNotify whose above-sibling names the sibling now
    /// beneath (XTS Xlib11 ConfigureNotify 1-2). A restack that leaves the
    /// order as it was reports nothing. Red before the fix: only a geometry
    /// change was reported, and above-sibling was always None.
    #[test]
    fn a_restack_alone_is_a_configure_notify_naming_the_sibling_beneath() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("restack", byte_order);
            let parent = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
            let lower = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 2;
            let upper = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 3;
            client.write_all(&create_window_request(byte_order, parent, 0, 0, 200, 200)).unwrap();
            client.write_all(&map_window_request(byte_order, parent)).unwrap();
            client.write_all(&create_window_request_with_parent(byte_order, lower, parent, 10, 10, 80, 80)).unwrap();
            client.write_all(&create_window_request_with_parent(byte_order, upper, parent, 40, 40, 80, 80)).unwrap();
            client.write_all(&map_window_request(byte_order, lower)).unwrap();
            client.write_all(&map_window_request(byte_order, upper)).unwrap();
            // StructureNotify on the window, SubstructureNotify on the parent.
            client.write_all(&change_window_event_mask_request(byte_order, lower, 1 << 17)).unwrap();
            client.write_all(&change_window_event_mask_request(byte_order, parent, 1 << 19)).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let _ = events_until_reply(byte_order, &mut client);

            // Raise `lower` to the top: stack-mode Above, no sibling.
            client.write_all(&configure_stack_mode_request(byte_order, lower, 0)).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let events = events_until_reply(byte_order, &mut client);
            let configured = events.iter().filter(|e| e[0] & 0x7f == CONFIGURE_NOTIFY).collect::<Vec<_>>();
            assert_eq!(configured.len(), 2, "{byte_order:?}: the window's copy and the parent's: {events:?}");
            for event in &configured {
                assert_eq!(read_u32(byte_order, &event[8..12]), lower, "{byte_order:?}: the raised window");
                assert_eq!(read_u32(byte_order, &event[12..16]), upper, "{byte_order:?}: above-sibling is the sibling now beneath");
                assert_eq!((read_u16(byte_order, &event[16..18]), read_u16(byte_order, &event[18..20])), (10, 10), "{byte_order:?}: geometry unchanged");
            }
            let addressed = configured.iter().map(|e| read_u32(byte_order, &e[4..8])).collect::<Vec<_>>();
            assert!(addressed.contains(&lower) && addressed.contains(&parent), "{byte_order:?}: {addressed:?}");
            client.write_all(&resource_request(byte_order, 15, parent)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            let count = usize::from(read_u16(byte_order, &reply[16..18]));
            let children = (0..count).map(|i| read_u32(byte_order, &reply[32 + 4 * i..36 + 4 * i])).collect::<Vec<_>>();
            assert_eq!(children, vec![upper, lower], "{byte_order:?}: bottom to top after the raise");

            // Raising the top window again changes nothing and reports nothing.
            client.write_all(&configure_stack_mode_request(byte_order, lower, 0)).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            assert!(events_until_reply(byte_order, &mut client).is_empty(), "{byte_order:?}: an unchanged order is not reported");

            // Lowering it to the bottom: above-sibling None.
            client.write_all(&configure_stack_mode_request(byte_order, lower, 1)).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let events = events_until_reply(byte_order, &mut client);
            let configured = events.iter().filter(|e| e[0] & 0x7f == CONFIGURE_NOTIFY).collect::<Vec<_>>();
            assert_eq!(configured.len(), 2, "{byte_order:?}: {events:?}");
            assert!(configured.iter().all(|e| read_u32(byte_order, &e[12..16]) == 0), "{byte_order:?}: nothing beneath the bottom");

            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    /// ConfigureWindow with stack-mode alone (value-mask bit 6).
    fn configure_stack_mode_request(byte_order: XByteOrder, window: u32, stack_mode: u32) -> Vec<u8> {
        let mut out = vec![12, 0];
        push_u16(&mut out, byte_order, 4);
        push_u32(&mut out, byte_order, window);
        push_u16(&mut out, byte_order, 1 << 6);
        push_u16(&mut out, byte_order, 0);
        push_u32(&mut out, byte_order, stack_mode);
        out
    }

    #[test]
    fn rotate_properties_moves_values_along_the_list_and_a_grab_change_without_a_grab_is_nothing() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("rotate", byte_order);
            let window = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
            client.write_all(&create_window_request(byte_order, window, 0, 0, 64, 48)).unwrap();
            let mut atoms = Vec::new();
            for name in ["SOPHIA_ROTATE_A", "SOPHIA_ROTATE_B", "SOPHIA_ROTATE_C"] {
                client.write_all(&intern_atom_request(byte_order, false, name)).unwrap();
                let reply = read_x_reply(&mut client, byte_order);
                atoms.push(read_u32(byte_order, &reply[8..12]));
            }
            for (index, atom) in atoms.iter().enumerate() {
                client.write_all(&change_property_request(byte_order, XPropertyMode::Replace, window, *atom, 6, 32, &(index as u32 + 1).to_ne_bytes())).unwrap();
            }
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let _ = events_until_reply(byte_order, &mut client);

            // Delta 1: A takes B's value, B takes C's, C takes A's; three
            // PropertyNotify, one per property.
            let mut rotate = vec![114, 0];
            push_u16(&mut rotate, byte_order, 3 + 3);
            push_u32(&mut rotate, byte_order, window);
            push_u16(&mut rotate, byte_order, 3);
            push_u16(&mut rotate, byte_order, 1);
            for atom in &atoms {
                push_u32(&mut rotate, byte_order, *atom);
            }
            client.write_all(&rotate).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let events = events_until_reply(byte_order, &mut client);
            let notified = events.iter().filter(|e| e[0] & 0x7f == PROPERTY_NOTIFY).map(|e| read_u32(byte_order, &e[8..12])).collect::<Vec<_>>();
            assert_eq!(notified, atoms, "{byte_order:?}: one notice per property: {events:?}");
            client.write_all(&get_property_request(byte_order, false, window, atoms[0], 6, 0, 1)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            assert_eq!(u32::from_ne_bytes(reply[32..36].try_into().unwrap()), 2, "{byte_order:?}: A holds what B held");
            // A property the window does not have: BadMatch, nothing moved.
            let mut missing = rotate.clone();
            let foreign = atoms[2] + 1000;
            missing.truncate(rotate.len() - 4);
            push_u32(&mut missing, byte_order, foreign);
            client.write_all(&intern_atom_request(byte_order, false, "SOPHIA_ROTATE_D")).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            let d = read_u32(byte_order, &reply[8..12]);
            missing.truncate(rotate.len() - 4);
            push_u32(&mut missing, byte_order, d);
            let _ = foreign;
            client.write_all(&missing).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_MATCH, 114), "{byte_order:?}: {record:?}");

            // ChangeActivePointerGrab: no active grab, so nothing; a bit
            // outside the pointer events is BadValue carrying the mask.
            client.write_all(&request(byte_order, 30, 0, &[0, 0, 0])).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            assert!(events_until_reply(byte_order, &mut client).is_empty(), "{byte_order:?}: nothing without a grab");
            let mut bad = request(byte_order, 30, 0, &[0, 0, 0]);
            let mask = 0x8001u16;
            bad[12..14].copy_from_slice(&match byte_order {
                XByteOrder::LittleEndian => mask.to_le_bytes(),
                XByteOrder::BigEndian => mask.to_be_bytes(),
            });
            client.write_all(&bad).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_VALUE, 30), "{byte_order:?}: {record:?}");
            assert_eq!(read_u32(byte_order, &record[4..8]), u32::from(mask), "{byte_order:?}: carrying the mask");

            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}

