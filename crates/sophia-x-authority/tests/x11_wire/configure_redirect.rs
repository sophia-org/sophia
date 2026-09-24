// ConfigureWindow redirection over real sockets, on the routed service
// where more than one client lives (t198).
//
// WHAT THESE PROVE. A client selecting SubstructureRedirect on a parent has
// asked to decide what happens to its children: a ConfigureWindow on one by
// another client reaches the manager as a ConfigureRequest carrying the
// request, and nothing is applied, as a map becomes a MapRequest. An
// override-redirect window is never redirected. A client selecting
// ResizeRedirect on a window is asked about a size change with a
// ResizeRequest, and the size is not applied while the rest of the request
// is. Before t198 every ConfigureWindow was applied and told no manager.

#[cfg(unix)]
mod configure_redirect {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const CONFIGURE_NOTIFY: u8 = 22;
    const CONFIGURE_REQUEST: u8 = 23;
    const RESIZE_REQUEST: u8 = 25;

    fn routed_service(name: &str, clients: usize) -> (std::path::PathBuf, thread::JoinHandle<Result<(), X11SetupSocketError>>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-configure-redirect-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1367),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap();
        let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
        let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
            .unwrap()
            .with_admission_policy(policy)
            .with_max_concurrent_clients(std::num::NonZeroUsize::new(4).unwrap());
        let broker = XServerFrontendRouteBroker::new(std::num::NonZeroUsize::new(8).unwrap());
        let server = thread::spawn(move || -> Result<(), X11SetupSocketError> {
            let mut frontend = XServerFrontend::bind(config).unwrap();
            for _ in 0..clients {
                frontend.serve_next_concurrently_routed(&broker)?;
            }
            frontend.wait_for_clients()
        });
        wait_for_socket(&socket_path);
        (socket_path, server)
    }

    fn connected(socket_path: &std::path::Path, byte_order: XByteOrder) -> (std::os::unix::net::UnixStream, u32) {
        let mut client = connect_x_socket(socket_path);
        client
            .write_all(&setup_request(byte_order, 11, 0, b"", b""))
            .unwrap();
        let base = read_setup_resource_id_base(&mut client, byte_order);
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        (client, base)
    }

    /// GetInputFocus, and every event queued before its reply.
    fn sync(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream) -> Vec<[u8; 32]> {
        let mut request = vec![43, 0];
        push_u16(&mut request, byte_order, 1);
        client.write_all(&request).unwrap();
        let mut events = Vec::new();
        loop {
            let record = read_x_record(client);
            if record[0] == 1 {
                return events;
            }
            events.push(record);
        }
    }

    /// ConfigureWindow with the given value-mask and values, in mask order.
    fn configure_request(byte_order: XByteOrder, window: u32, value_mask: u16, values: &[u32]) -> Vec<u8> {
        let mut out = vec![12, 0];
        push_u16(&mut out, byte_order, 3 + values.len() as u16);
        push_u32(&mut out, byte_order, window);
        push_u16(&mut out, byte_order, value_mask);
        push_u16(&mut out, byte_order, 0);
        for value in values {
            push_u32(&mut out, byte_order, *value);
        }
        out
    }

    /// ChangeWindowAttributes override-redirect.
    fn override_redirect_request(byte_order: XByteOrder, window: u32, on: bool) -> Vec<u8> {
        let mut out = vec![2, 0];
        push_u16(&mut out, byte_order, 4);
        push_u32(&mut out, byte_order, window);
        push_u32(&mut out, byte_order, 1 << 9);
        push_u32(&mut out, byte_order, u32::from(on));
        out
    }

    fn geometry(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream, window: u32) -> (i16, i16, u16, u16) {
        client.write_all(&resource_request(byte_order, 14, window)).unwrap();
        let reply = read_x_reply(client, byte_order);
        (
            read_u16(byte_order, &reply[12..14]) as i16,
            read_u16(byte_order, &reply[14..16]) as i16,
            read_u16(byte_order, &reply[16..18]),
            read_u16(byte_order, &reply[18..20]),
        )
    }

    #[test]
    fn a_configure_on_a_managed_child_is_the_managers_to_decide() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("substructure", 2);
            let (mut manager, _manager_base) = connected(&socket_path, byte_order);
            let (mut owner, owner_base) = connected(&socket_path, byte_order);
            let window = owner_base + 1;
            owner.write_all(&create_window_request(byte_order, window, 10, 10, 100, 100)).unwrap();
            owner.write_all(&change_window_event_mask_request(byte_order, window, 1 << 17)).unwrap();
            owner.write_all(&map_window_request(byte_order, window)).unwrap();
            let _ = sync(byte_order, &mut owner);
            // The manager takes the root's children after the window exists.
            manager.write_all(&change_window_event_mask_request(byte_order, X_SETUP_DEFAULT_ROOT, 1 << 20)).unwrap();
            let _ = sync(byte_order, &mut manager);

            // Moved and resized by its owner: the manager hears the request,
            // the owner hears nothing, the window stays where it was.
            owner.write_all(&configure_request(byte_order, window, 0xF, &[30, 40, 200, 150])).unwrap();
            let events = sync(byte_order, &mut owner);
            assert!(
                !events.iter().any(|e| e[0] & 0x7f == CONFIGURE_NOTIFY),
                "{byte_order:?}: a redirected configure applies nothing: {events:?}"
            );
            let request = read_x_record(&mut manager);
            assert_eq!(request[0] & 0x7f, CONFIGURE_REQUEST, "{byte_order:?}: {request:?}");
            assert_eq!(read_u32(byte_order, &request[4..8]), X_SETUP_DEFAULT_ROOT, "{byte_order:?}: parent");
            assert_eq!(read_u32(byte_order, &request[8..12]), window, "{byte_order:?}: window");
            assert_eq!(read_u32(byte_order, &request[12..16]), 0, "{byte_order:?}: no sibling asked");
            assert_eq!(
                (
                    read_u16(byte_order, &request[16..18]) as i16,
                    read_u16(byte_order, &request[18..20]) as i16,
                    read_u16(byte_order, &request[20..22]),
                    read_u16(byte_order, &request[22..24]),
                ),
                (30, 40, 200, 150),
                "{byte_order:?}: the request as made"
            );
            assert_eq!(read_u16(byte_order, &request[26..28]), 0xF, "{byte_order:?}: value-mask");
            assert_eq!(geometry(byte_order, &mut owner, window), (10, 10, 100, 100), "{byte_order:?}: unchanged");

            // A partial request carries the current values for the rest.
            owner.write_all(&configure_request(byte_order, window, 0x4, &[64])).unwrap();
            let _ = sync(byte_order, &mut owner);
            let request = read_x_record(&mut manager);
            assert_eq!(request[0] & 0x7f, CONFIGURE_REQUEST, "{byte_order:?}: {request:?}");
            assert_eq!(
                (
                    read_u16(byte_order, &request[16..18]) as i16,
                    read_u16(byte_order, &request[18..20]) as i16,
                    read_u16(byte_order, &request[20..22]),
                    read_u16(byte_order, &request[22..24]),
                    read_u16(byte_order, &request[26..28]),
                ),
                (10, 10, 64, 100, 0x4),
                "{byte_order:?}: width asked, the rest current"
            );

            // Override-redirect: the window is not for a manager to place,
            // so the same request is applied and reported, and the manager
            // hears nothing.
            owner.write_all(&override_redirect_request(byte_order, window, true)).unwrap();
            owner.write_all(&configure_request(byte_order, window, 0xF, &[30, 40, 200, 150])).unwrap();
            let events = sync(byte_order, &mut owner);
            assert!(
                events.iter().any(|e| e[0] & 0x7f == CONFIGURE_NOTIFY),
                "{byte_order:?}: an override-redirect configure is applied: {events:?}"
            );
            assert_eq!(geometry(byte_order, &mut owner, window), (30, 40, 200, 150), "{byte_order:?}: applied");
            assert!(sync(byte_order, &mut manager).is_empty(), "{byte_order:?}: nothing redirected");

            drop(owner);
            drop(manager);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn a_size_change_on_a_resize_redirected_window_is_asked_for_and_not_applied() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("resize", 2);
            let (mut manager, _manager_base) = connected(&socket_path, byte_order);
            let (mut owner, owner_base) = connected(&socket_path, byte_order);
            // A child of the owner's toplevel: a toplevel's placement is the
            // policy's on this service, a child's geometry is its client's.
            let toplevel = owner_base + 1;
            let window = owner_base + 2;
            owner.write_all(&create_window_request(byte_order, toplevel, 0, 0, 300, 300)).unwrap();
            owner.write_all(&map_window_request(byte_order, toplevel)).unwrap();
            owner.write_all(&create_window_request_with_parent(byte_order, window, toplevel, 10, 10, 100, 100)).unwrap();
            owner.write_all(&change_window_event_mask_request(byte_order, window, 1 << 17)).unwrap();
            owner.write_all(&map_window_request(byte_order, window)).unwrap();
            let _ = sync(byte_order, &mut owner);
            manager.write_all(&change_window_event_mask_request(byte_order, window, 1 << 18)).unwrap();
            let _ = sync(byte_order, &mut manager);

            // A move with a resize: the move is applied and reported, the
            // size is asked of the manager and not applied.
            owner.write_all(&configure_request(byte_order, window, 0xD, &[50, 300, 250])).unwrap();
            let events = sync(byte_order, &mut owner);
            let configured = events.iter().filter(|e| e[0] & 0x7f == CONFIGURE_NOTIFY).collect::<Vec<_>>();
            assert_eq!(configured.len(), 1, "{byte_order:?}: {events:?}");
            assert_eq!(
                (
                    read_u16(byte_order, &configured[0][16..18]) as i16,
                    read_u16(byte_order, &configured[0][20..22]),
                    read_u16(byte_order, &configured[0][22..24]),
                ),
                (50, 100, 100),
                "{byte_order:?}: moved, not resized"
            );
            let request = read_x_record(&mut manager);
            assert_eq!(request[0] & 0x7f, RESIZE_REQUEST, "{byte_order:?}: {request:?}");
            assert_eq!(read_u32(byte_order, &request[4..8]), window, "{byte_order:?}: window");
            assert_eq!(
                (read_u16(byte_order, &request[8..10]), read_u16(byte_order, &request[10..12])),
                (300, 250),
                "{byte_order:?}: the size asked for"
            );
            assert_eq!(geometry(byte_order, &mut owner, window), (50, 10, 100, 100), "{byte_order:?}");

            // The same size again is no change and asks nothing.
            owner.write_all(&configure_request(byte_order, window, 0xC, &[100, 100])).unwrap();
            let _ = sync(byte_order, &mut owner);
            assert!(sync(byte_order, &mut manager).is_empty(), "{byte_order:?}: an unchanged size is not asked");

            drop(owner);
            drop(manager);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
