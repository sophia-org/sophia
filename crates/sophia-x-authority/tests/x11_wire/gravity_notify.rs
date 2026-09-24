// A parent's resize moves a child by its win-gravity and reports it (t199):
// a GravityNotify after the parent's ConfigureNotify, to the child's
// StructureNotify selectors and the parent's SubstructureNotify selectors,
// each copy addressed to the window it was selected on.

#[cfg(unix)]
mod gravity_notify {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const CONFIGURE_NOTIFY: u8 = 22;
    const GRAVITY_NOTIFY: u8 = 24;
    const STRUCTURE_NOTIFY: u32 = 1 << 17;
    const SUBSTRUCTURE_NOTIFY: u32 = 1 << 19;

    fn routed_service(clients: usize) -> (std::path::PathBuf, thread::JoinHandle<Result<(), X11SetupSocketError>>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-gravity-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1991),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap();
        let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
        let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
            .unwrap()
            .with_admission_policy(policy)
            .with_client_toplevel_placement(true)
            .with_max_concurrent_clients(std::num::NonZeroUsize::new(4).unwrap());
        let broker = XServerFrontendRouteBroker::new(std::num::NonZeroUsize::new(64).unwrap());
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
        client.write_all(&setup_request(byte_order, 11, 0, b"", b"")).unwrap();
        let base = read_setup_resource_id_base(&mut client, byte_order);
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        (client, base)
    }

    fn request(byte_order: XByteOrder, opcode: u8, detail: u8, words: &[u32]) -> Vec<u8> {
        let mut out = vec![opcode, detail];
        push_u16(&mut out, byte_order, 1 + words.len() as u16);
        for word in words {
            push_u32(&mut out, byte_order, *word);
        }
        out
    }

    fn sync(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream) -> Vec<[u8; 32]> {
        let mut events = Vec::new();
        client.write_all(&request(byte_order, 43, 0, &[])).unwrap();
        loop {
            let record = read_x_record(client);
            match record[0] {
                1 => return events,
                0 => panic!("{byte_order:?}: an error while syncing: {record:?}"),
                _ => events.push(record),
            }
        }
    }

    #[test]
    fn a_resize_moves_a_child_by_its_gravity_and_tells_both_sides() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service(2);
            let (mut owner, base) = connected(&socket_path, byte_order);
            let (mut watcher, _) = connected(&socket_path, byte_order);
            let parent = base + 1;
            let child = base + 2;
            owner.write_all(&create_window_request(byte_order, parent, 0, 0, 40, 40)).unwrap();
            owner.write_all(&create_window_request_with_parent(byte_order, child, parent, 10, 10, 5, 5)).unwrap();
            // win-gravity (1 << 5) SouthEast (9).
            owner.write_all(&request(byte_order, 2, 0, &[child, 1 << 5, 9])).unwrap();
            assert!(sync(byte_order, &mut owner).is_empty());
            watcher.write_all(&change_window_event_mask_request(byte_order, child, STRUCTURE_NOTIFY)).unwrap();
            watcher
                .write_all(&change_window_event_mask_request(byte_order, parent, STRUCTURE_NOTIFY | SUBSTRUCTURE_NOTIFY))
                .unwrap();
            watcher.write_all(&change_window_event_mask_request(byte_order, 0x20, SUBSTRUCTURE_NOTIFY)).unwrap();
            assert!(sync(byte_order, &mut watcher).is_empty());

            // Grow the parent by 6 by 4: the child moves by the same. dix
            // delivers the parent's ConfigureNotify and its copy to the root
            // before either copy of the child's GravityNotify.
            owner.write_all(&configure_window_request(byte_order, parent, 0x4 | 0x8, &[46, 44])).unwrap();
            sync(byte_order, &mut owner);
            let seen: Vec<(u8, u32, u32)> = (0..4)
                .map(|_| {
                    let record = read_x_record(&mut watcher);
                    assert!(record[0] >= 2, "{byte_order:?}: {record:?}");
                    if record[0] == GRAVITY_NOTIFY {
                        assert_eq!(
                            (read_u16(byte_order, &record[12..14]) as i16, read_u16(byte_order, &record[14..16]) as i16),
                            (16, 14),
                            "{byte_order:?}: where the child is now"
                        );
                    }
                    (record[0], read_u32(byte_order, &record[4..8]), read_u32(byte_order, &record[8..12]))
                })
                .collect();
            assert_eq!(
                seen,
                vec![
                    (CONFIGURE_NOTIFY, parent, parent),
                    (CONFIGURE_NOTIFY, 0x20, parent),
                    (GRAVITY_NOTIFY, child, child),
                    (GRAVITY_NOTIFY, parent, child),
                ],
                "{byte_order:?}: each copy follows its event"
            );
            drop((owner, watcher));
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
