// Who places a mapped toplevel (t189). With a policy, the authority keeps
// the placement the policy chose and answers a client's own move with a
// synthetic ConfigureNotify, as a redirecting window manager does. With no
// window manager at all -- the conformance host, and what XTS assumes -- the
// client's request takes effect, as on the reference server without one.

#[cfg(unix)]
mod toplevel_placement {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const CONFIGURE_NOTIFY: u8 = 22;
    const STRUCTURE_NOTIFY: u32 = 1 << 17;

    fn served(name: &str, byte_order: XByteOrder, client_places: bool) -> (std::os::unix::net::UnixStream, u32, std::path::PathBuf, thread::JoinHandle<Result<(), X11SetupSocketError>>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-placement-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1890),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap();
        let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
        let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
            .unwrap()
            .with_admission_policy(policy)
            .with_client_toplevel_placement(client_places)
            .with_max_concurrent_clients(std::num::NonZeroUsize::new(2).unwrap());
        let broker = XServerFrontendRouteBroker::new(std::num::NonZeroUsize::new(64).unwrap());
        let server = thread::spawn(move || -> Result<(), X11SetupSocketError> {
            let mut frontend = XServerFrontend::bind(config).unwrap();
            frontend.serve_next_concurrently_routed(&broker)?;
            frontend.wait_for_clients()
        });
        wait_for_socket(&socket_path);
        let mut client = connect_x_socket(&socket_path);
        client.write_all(&setup_request(byte_order, 11, 0, b"", b"")).unwrap();
        let base = read_setup_resource_id_base(&mut client, byte_order);
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        (client, base, socket_path, server)
    }

    fn sync(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream) -> Vec<[u8; 32]> {
        let mut out = vec![43, 0];
        push_u16(&mut out, byte_order, 1);
        client.write_all(&out).unwrap();
        let mut events = Vec::new();
        loop {
            let record = read_x_record(client);
            match record[0] {
                1 => return events,
                0 => panic!("{byte_order:?}: an error while syncing: {record:?}"),
                _ => events.push(record),
            }
        }
    }

    fn move_request(byte_order: XByteOrder, window: u32, x: i16, y: i16) -> Vec<u8> {
        let mut out = vec![12, 0];
        push_u16(&mut out, byte_order, 5);
        push_u32(&mut out, byte_order, window);
        push_u16(&mut out, byte_order, 0x3);
        push_u16(&mut out, byte_order, 0);
        push_u32(&mut out, byte_order, x as i32 as u32);
        push_u32(&mut out, byte_order, y as i32 as u32);
        out
    }

    fn geometry(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream, window: u32) -> (i16, i16) {
        client.write_all(&resource_request(byte_order, 14, window)).unwrap();
        let reply = read_x_reply(client, byte_order);
        (read_u16(byte_order, &reply[12..14]) as i16, read_u16(byte_order, &reply[14..16]) as i16)
    }

    fn configure_notices(byte_order: XByteOrder, events: &[[u8; 32]]) -> Vec<(bool, i16, i16)> {
        events
            .iter()
            .filter(|record| record[0] & 0x7f == CONFIGURE_NOTIFY)
            .map(|record| (record[0] & 0x80 != 0, read_u16(byte_order, &record[16..18]) as i16, read_u16(byte_order, &record[18..20]) as i16))
            .collect()
    }

    #[test]
    fn a_host_without_a_window_manager_lets_a_client_move_its_mapped_toplevel() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, base, socket_path, server) = served("no-manager", byte_order, true);
            let window = base + 1;
            client.write_all(&create_window_request(byte_order, window, 100, 50, 60, 40)).unwrap();
            client.write_all(&change_window_event_mask_request(byte_order, window, STRUCTURE_NOTIFY)).unwrap();
            client.write_all(&map_window_request(byte_order, window)).unwrap();
            sync(byte_order, &mut client);
            client.write_all(&move_request(byte_order, window, 0, 0)).unwrap();
            let notices = configure_notices(byte_order, &sync(byte_order, &mut client));
            assert_eq!(notices, vec![(false, 0, 0)], "{byte_order:?}: a real notice at the new place");
            assert_eq!(geometry(byte_order, &mut client, window), (0, 0), "{byte_order:?}: the move took effect");
            drop(client);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn a_policy_managed_toplevel_keeps_its_placement_and_is_told_so() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (mut client, base, socket_path, server) = served("managed", byte_order, false);
            let window = base + 1;
            client.write_all(&create_window_request(byte_order, window, 100, 50, 60, 40)).unwrap();
            client.write_all(&change_window_event_mask_request(byte_order, window, STRUCTURE_NOTIFY)).unwrap();
            client.write_all(&map_window_request(byte_order, window)).unwrap();
            sync(byte_order, &mut client);
            client.write_all(&move_request(byte_order, window, 0, 0)).unwrap();
            let notices = configure_notices(byte_order, &sync(byte_order, &mut client));
            assert_eq!(notices, vec![(true, 100, 50)], "{byte_order:?}: a synthetic notice at the kept place");
            assert_eq!(geometry(byte_order, &mut client, window), (100, 50), "{byte_order:?}: the placement is kept");
            drop(client);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
