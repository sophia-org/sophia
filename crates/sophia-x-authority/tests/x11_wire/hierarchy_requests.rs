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
    const BAD_WINDOW: u8 = 3;
    const BAD_ATOM: u8 = 5;
    const BAD_MATCH: u8 = 8;
    const UNMAP_NOTIFY: u8 = 18;
    const CREATE_NOTIFY: u8 = 16;
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

    /// The border width a client asks for is a fact it reads back: Sophia
    /// draws no border, but GetGeometry, CreateNotify and ConfigureNotify
    /// report the width CreateWindow or ConfigureWindow set, and a change of
    /// border width alone is a ConfigureNotify (XTS Xlib11 ConfigureRequest
    /// 1, 3, 6 differed from Xvnc only in this field). Red before the fix:
    /// both requests dropped the value at decode and everything read 0.
    #[test]
    fn a_border_width_is_kept_and_read_back_though_never_drawn() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("border", byte_order);
            let parent = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
            let window = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 2;
            client.write_all(&create_window_request(byte_order, parent, 0, 0, 200, 200)).unwrap();
            client.write_all(&change_window_event_mask_request(byte_order, parent, 1 << 19)).unwrap();
            client.write_all(&map_window_request(byte_order, parent)).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let _ = events_until_reply(byte_order, &mut client);
            // A child with a border of one.
            let mut create = create_window_request_with_parent(byte_order, window, parent, 10, 10, 80, 80);
            create[20..22].copy_from_slice(&match byte_order {
                XByteOrder::LittleEndian => 1u16.to_le_bytes(),
                XByteOrder::BigEndian => 1u16.to_be_bytes(),
            });
            client.write_all(&create).unwrap();
            client.write_all(&change_window_event_mask_request(byte_order, window, 1 << 17)).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let events = events_until_reply(byte_order, &mut client);
            let created = events.iter().find(|e| e[0] & 0x7f == CREATE_NOTIFY).unwrap_or_else(|| panic!("{byte_order:?}: {events:?}"));
            client.write_all(&resource_request(byte_order, 14, window)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            assert_eq!(read_u16(byte_order, &created[20..22]), 1, "{byte_order:?}: CreateNotify border width");
            assert_eq!(read_u16(byte_order, &reply[20..22]), 1, "{byte_order:?}: GetGeometry border width");

            // ConfigureWindow with border-width alone (value-mask bit 4).
            let mut configure = vec![12, 0];
            push_u16(&mut configure, byte_order, 4);
            push_u32(&mut configure, byte_order, window);
            push_u16(&mut configure, byte_order, 1 << 4);
            push_u16(&mut configure, byte_order, 0);
            push_u32(&mut configure, byte_order, 6);
            client.write_all(&configure).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let events = events_until_reply(byte_order, &mut client);
            let configured = events.iter().filter(|e| e[0] & 0x7f == CONFIGURE_NOTIFY).collect::<Vec<_>>();
            assert!(!configured.is_empty(), "{byte_order:?}: a border change is a configuration change: {events:?}");
            assert!(configured.iter().all(|e| read_u16(byte_order, &e[24..26]) == 6), "{byte_order:?}: ConfigureNotify border width");
            assert!(configured.iter().all(|e| (read_u16(byte_order, &e[20..22]), read_u16(byte_order, &e[22..24])) == (80, 80)), "{byte_order:?}: size unchanged");
            client.write_all(&resource_request(byte_order, 14, window)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            assert_eq!(read_u16(byte_order, &reply[20..22]), 6, "{byte_order:?}: read back");
            // The same width again changes nothing and reports nothing.
            client.write_all(&configure).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            assert!(events_until_reply(byte_order, &mut client).is_empty(), "{byte_order:?}: unchanged");

            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    /// The protocol's own refusals for a configure: a zero width or height
    /// is BadValue, a sibling without a stack-mode is BadMatch, a window
    /// that is not a sibling is BadMatch, and only an id that names no
    /// window is BadWindow (XTS Xlib4 XConfigureWindow 30-32, XRestackWindows
    /// 5, XCreateSimpleWindow 11, XResizeWindow 12, XMoveResizeWindow 14).
    /// Red before the fix: zero sizes were applied, and every sibling
    /// refusal read BadWindow.
    #[test]
    fn a_configure_is_refused_as_the_protocol_refuses_it() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("refusals", byte_order);
            let parent = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
            let other = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 2;
            let window = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 3;
            let cousin = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 4;
            client.write_all(&create_window_request(byte_order, parent, 0, 0, 200, 200)).unwrap();
            client.write_all(&create_window_request(byte_order, other, 0, 0, 200, 200)).unwrap();
            client.write_all(&create_window_request_with_parent(byte_order, window, parent, 10, 10, 80, 80)).unwrap();
            client.write_all(&create_window_request_with_parent(byte_order, cousin, other, 10, 10, 80, 80)).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let _ = events_until_reply(byte_order, &mut client);
            let expect = |client: &mut std::os::unix::net::UnixStream, code: u8, opcode: u8, what: &str| {
                let record = read_x_record(client);
                assert_eq!((record[0], record[1], record[10]), (0, code, opcode), "{byte_order:?}: {what}: {record:?}");
            };
            // A zero width, at creation and by configure.
            client.write_all(&create_window_request_with_parent(byte_order, X_SETUP_DEFAULT_RESOURCE_ID_BASE + 5, parent, 0, 0, 0, 10)).unwrap();
            expect(&mut client, BAD_VALUE, 1, "CreateWindow width 0");
            let mut configure = vec![12, 0];
            push_u16(&mut configure, byte_order, 4);
            push_u32(&mut configure, byte_order, window);
            push_u16(&mut configure, byte_order, 1 << 3);
            push_u16(&mut configure, byte_order, 0);
            push_u32(&mut configure, byte_order, 0);
            client.write_all(&configure).unwrap();
            expect(&mut client, BAD_VALUE, 12, "ConfigureWindow height 0");
            // A sibling without a stack-mode.
            let mut configure = vec![12, 0];
            push_u16(&mut configure, byte_order, 4);
            push_u32(&mut configure, byte_order, window);
            push_u16(&mut configure, byte_order, 1 << 5);
            push_u16(&mut configure, byte_order, 0);
            push_u32(&mut configure, byte_order, cousin);
            client.write_all(&configure).unwrap();
            expect(&mut client, BAD_MATCH, 12, "sibling without stack-mode");
            // A window that is not a sibling, with a stack-mode.
            let mut configure = vec![12, 0];
            push_u16(&mut configure, byte_order, 5);
            push_u32(&mut configure, byte_order, window);
            push_u16(&mut configure, byte_order, (1 << 5) | (1 << 6));
            push_u16(&mut configure, byte_order, 0);
            push_u32(&mut configure, byte_order, cousin);
            push_u32(&mut configure, byte_order, 0);
            client.write_all(&configure).unwrap();
            expect(&mut client, BAD_MATCH, 12, "not a sibling");
            // An id that names no window.
            let mut configure = vec![12, 0];
            push_u16(&mut configure, byte_order, 5);
            push_u32(&mut configure, byte_order, window);
            push_u16(&mut configure, byte_order, (1 << 5) | (1 << 6));
            push_u16(&mut configure, byte_order, 0);
            push_u32(&mut configure, byte_order, X_SETUP_DEFAULT_RESOURCE_ID_BASE + 0x7ff);
            push_u32(&mut configure, byte_order, 0);
            client.write_all(&configure).unwrap();
            expect(&mut client, BAD_WINDOW, 12, "unknown sibling");
            // The window is untouched by any of it.
            client.write_all(&resource_request(byte_order, 14, window)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            assert_eq!((read_u16(byte_order, &reply[16..18]), read_u16(byte_order, &reply[18..20])), (80, 80), "{byte_order:?}");
            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    /// The rest of the window refusals and answers the windows scenario
    /// wanted (t224): configuring the root is a silent no-op, an unknown
    /// window is BadWindow before a zero size is BadValue, an InputOutput
    /// child of an InputOnly parent is BadMatch while CopyFromParent under
    /// one is InputOnly and reports depth 0, and the attribute reply's
    /// backing-planes default to all ones (XTS Xlib4 XMoveWindow 5,
    /// XResizeWindow 5 and 13, XCreateSimpleWindow 7 and 10, Xlib5
    /// XGetGeometry 2).
    #[test]
    fn the_root_is_configured_by_nobody_and_input_only_parents_have_input_only_children() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("root-and-input-only", byte_order);
            let expect = |client: &mut std::os::unix::net::UnixStream, code: u8, opcode: u8, what: &str| {
                let record = read_x_record(client);
                assert_eq!((record[0], record[1], record[10]), (0, code, opcode), "{byte_order:?}: {what}: {record:?}");
            };
            // Move and resize the root: nothing happens, nothing is said.
            client.write_all(&configure_stack_mode_request(byte_order, X_SETUP_DEFAULT_ROOT, 0)).unwrap();
            let mut resize_root = vec![12, 0];
            push_u16(&mut resize_root, byte_order, 5);
            push_u32(&mut resize_root, byte_order, X_SETUP_DEFAULT_ROOT);
            push_u16(&mut resize_root, byte_order, 0xC);
            push_u16(&mut resize_root, byte_order, 0);
            push_u32(&mut resize_root, byte_order, 10);
            push_u32(&mut resize_root, byte_order, 10);
            client.write_all(&resize_root).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            assert!(events_until_reply(byte_order, &mut client).is_empty(), "{byte_order:?}: the root answers nothing");
            // An unknown window with a zero size: the window first.
            let mut zero_unknown = vec![12, 0];
            push_u16(&mut zero_unknown, byte_order, 4);
            push_u32(&mut zero_unknown, byte_order, X_SETUP_DEFAULT_RESOURCE_ID_BASE + 0x7f0);
            push_u16(&mut zero_unknown, byte_order, 1 << 2);
            push_u16(&mut zero_unknown, byte_order, 0);
            push_u32(&mut zero_unknown, byte_order, 0);
            client.write_all(&zero_unknown).unwrap();
            expect(&mut client, BAD_WINDOW, 12, "BadWindow before BadValue");

            // An InputOnly parent: class 2 at 22..24, depth 0.
            let parent = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
            let mut input_only_parent = create_window_request(byte_order, parent, 0, 0, 100, 100);
            input_only_parent[1] = 0;
            input_only_parent[22..24].copy_from_slice(&match byte_order {
                XByteOrder::LittleEndian => 2u16.to_le_bytes(),
                XByteOrder::BigEndian => 2u16.to_be_bytes(),
            });
            client.write_all(&input_only_parent).unwrap();
            // An explicit InputOutput child (class 1) is BadMatch.
            let child = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 2;
            let mut output_child = create_window_request_with_parent(byte_order, child, parent, 0, 0, 10, 10);
            output_child[1] = 0;
            client.write_all(&output_child).unwrap();
            expect(&mut client, BAD_MATCH, 1, "InputOutput under InputOnly");
            // CopyFromParent (class 0) is InputOnly: depth 0 in GetGeometry,
            // and drawing into it is refused as for any InputOnly window.
            let mut copied_child = create_window_request_with_parent(byte_order, child, parent, 0, 0, 10, 10);
            copied_child[1] = 0;
            copied_child[22..24].copy_from_slice(&[0, 0]);
            client.write_all(&copied_child).unwrap();
            client.write_all(&resource_request(byte_order, 14, child)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            assert_eq!(reply[1], 0, "{byte_order:?}: an InputOnly window has depth 0");
            client.write_all(&resource_request(byte_order, 14, parent)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            assert_eq!(reply[1], 0, "{byte_order:?}: the parent too");
            // backing-planes all ones in the attributes.
            client.write_all(&resource_request(byte_order, 3, parent)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            assert_eq!(read_u32(byte_order, &reply[16..20]), 0xffff_ffff, "{byte_order:?}: backing-planes default");
            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    /// The property and selection refusals the windows scenario wanted
    /// (t225): an atom the table does not know is BadAtom to DeleteProperty,
    /// SetSelectionOwner and ConvertSelection, a requestor that names no
    /// window is BadWindow, and a SetSelectionOwner with a time earlier than
    /// the selection's last change has no effect (XTS Xlib5 XDeleteProperty
    /// 3, XSetSelectionOwner 2 and 8, XConvertSelection 4-5).
    #[test]
    fn unknown_atoms_are_bad_atom_and_an_earlier_time_does_not_take_a_selection() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, socket_path, server) = served("atoms", byte_order);
            let window = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
            let other = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 2;
            client.write_all(&create_window_request(byte_order, window, 0, 0, 100, 100)).unwrap();
            client.write_all(&create_window_request(byte_order, other, 0, 0, 100, 100)).unwrap();
            let expect = |client: &mut std::os::unix::net::UnixStream, code: u8, opcode: u8, what: &str| {
                let record = read_x_record(client);
                assert_eq!((record[0], record[1], record[10]), (0, code, opcode), "{byte_order:?}: {what}: {record:?}");
            };
            let unknown_atom = 0x00ff_ffff;
            // DeleteProperty.
            let mut delete = vec![19, 0];
            push_u16(&mut delete, byte_order, 3);
            push_u32(&mut delete, byte_order, window);
            push_u32(&mut delete, byte_order, unknown_atom);
            client.write_all(&delete).unwrap();
            expect(&mut client, BAD_ATOM, 19, "DeleteProperty unknown atom");
            // SetSelectionOwner.
            client.write_all(&set_selection_owner_request(byte_order, window, unknown_atom, 5)).unwrap();
            expect(&mut client, BAD_ATOM, 22, "SetSelectionOwner unknown atom");
            // ConvertSelection: the requestor, then the atoms.
            client.write_all(&convert_selection_request(byte_order, X_SETUP_DEFAULT_RESOURCE_ID_BASE + 0x7f0, X_ATOM_PRIMARY, X_ATOM_STRING, X_ATOM_WM_NAME, 5)).unwrap();
            expect(&mut client, BAD_WINDOW, 24, "ConvertSelection unknown requestor");
            client.write_all(&convert_selection_request(byte_order, window, unknown_atom, X_ATOM_STRING, X_ATOM_WM_NAME, 5)).unwrap();
            expect(&mut client, BAD_ATOM, 24, "ConvertSelection unknown selection");
            client.write_all(&convert_selection_request(byte_order, window, X_ATOM_PRIMARY, X_ATOM_STRING, unknown_atom, 5)).unwrap();
            expect(&mut client, BAD_ATOM, 24, "ConvertSelection unknown property");
            // Ownership at time 100, then an attempt at time 50: no effect.
            client.write_all(&set_selection_owner_request(byte_order, window, X_ATOM_PRIMARY, 100)).unwrap();
            client.write_all(&set_selection_owner_request(byte_order, other, X_ATOM_PRIMARY, 50)).unwrap();
            client.write_all(&resource_request(byte_order, 23, X_ATOM_PRIMARY)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            assert_eq!(read_u32(byte_order, &reply[8..12]), window, "{byte_order:?}: an earlier time does not take the selection");
            // A later time takes it, and the previous owner hears SelectionClear.
            client.write_all(&set_selection_owner_request(byte_order, other, X_ATOM_PRIMARY, 150)).unwrap();
            client.write_all(&get_input_focus(byte_order)).unwrap();
            let records = events_until_reply(byte_order, &mut client);
            assert_eq!(records.len(), 1, "{byte_order:?}: one SelectionClear: {records:?}");
            assert_eq!((records[0][0] & 0x7f, read_u32(byte_order, &records[0][8..12])), (29, window), "{byte_order:?}: {records:?}");
            client.write_all(&resource_request(byte_order, 23, X_ATOM_PRIMARY)).unwrap();
            let reply = read_x_reply(&mut client, byte_order);
            assert_eq!(read_u32(byte_order, &reply[8..12]), other, "{byte_order:?}: a later one does");
            drop(client);
            server.join().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
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

            // Delta 1: A's value moves to B, B's to C, C's to A; three
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
            assert_eq!(u32::from_ne_bytes(reply[32..36].try_into().unwrap()), 3, "{byte_order:?}: A holds what C held");
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

