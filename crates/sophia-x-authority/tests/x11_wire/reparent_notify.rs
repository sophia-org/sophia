// ReparentWindow reports what it did (t184): a ReparentNotify to whoever
// selected StructureNotify on the window and SubstructureNotify on the old
// or the new parent, each copy addressed to the window it was selected on;
// a mapped window is unmapped, reparented and mapped again, in that order;
// and the routing's parent map follows, so a later DestroyNotify reaches the
// new parent's watchers and not the old one's.

#[cfg(unix)]
mod reparent_notify {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const ROOT: u32 = 0x20;
    const DESTROY_NOTIFY: u8 = 17;
    const UNMAP_NOTIFY: u8 = 18;
    const MAP_NOTIFY: u8 = 19;
    const REPARENT_NOTIFY: u8 = 21;
    const STRUCTURE_NOTIFY: u32 = 1 << 17;
    const SUBSTRUCTURE_NOTIFY: u32 = 1 << 19;

    fn routed_service(name: &str, clients: usize) -> (std::path::PathBuf, thread::JoinHandle<Result<(), X11SetupSocketError>>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-reparent-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1844),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap();
        let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
        let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
            .unwrap()
            .with_admission_policy(policy)
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

    /// Records until `count` events have arrived, replies and errors refused.
    fn events(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream, count: usize) -> Vec<[u8; 32]> {
        let mut seen = Vec::new();
        while seen.len() < count {
            let record = read_x_record(client);
            assert!(record[0] >= 2, "{byte_order:?}: not an event: {record:?}");
            seen.push(record);
        }
        seen
    }

    fn reparent_request(byte_order: XByteOrder, window: u32, parent: u32, x: i16, y: i16) -> Vec<u8> {
        let mut out = vec![7, 0];
        push_u16(&mut out, byte_order, 4);
        push_u32(&mut out, byte_order, window);
        push_u32(&mut out, byte_order, parent);
        push_i16(&mut out, byte_order, x);
        push_i16(&mut out, byte_order, y);
        out
    }

    fn u32_at(byte_order: XByteOrder, record: &[u8; 32], at: usize) -> u32 {
        read_u32(byte_order, &record[at..at + 4])
    }

    #[test]
    fn a_reparent_is_reported_to_the_window_and_both_parents_and_the_parent_map_follows() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("reported", 2);
            let (mut owner, base) = connected(&socket_path, byte_order);
            let (mut watcher, _) = connected(&socket_path, byte_order);
            let frame = base + 1;
            let child = base + 2;
            owner.write_all(&create_window_request(byte_order, frame, 0, 0, 100, 100)).unwrap();
            owner.write_all(&create_window_request(byte_order, child, 0, 0, 40, 40)).unwrap();
            sync(byte_order, &mut owner);
            // The watcher asks on all three: the window, the old parent
            // (the root) and the new parent.
            watcher.write_all(&change_window_event_mask_request(byte_order, child, STRUCTURE_NOTIFY)).unwrap();
            watcher.write_all(&change_window_event_mask_request(byte_order, ROOT, SUBSTRUCTURE_NOTIFY)).unwrap();
            watcher.write_all(&change_window_event_mask_request(byte_order, frame, SUBSTRUCTURE_NOTIFY)).unwrap();
            sync(byte_order, &mut watcher);

            owner.write_all(&reparent_request(byte_order, child, frame, 5, 6)).unwrap();
            assert!(sync(byte_order, &mut owner).is_empty(), "{byte_order:?}: the requester did not select");
            let mut copies = events(byte_order, &mut watcher, 3)
                .into_iter()
                .map(|record| {
                    assert_eq!(record[0] & 0x7f, REPARENT_NOTIFY, "{byte_order:?}: {record:?}");
                    assert_eq!(u32_at(byte_order, &record, 8), child, "{byte_order:?}: the window");
                    assert_eq!(u32_at(byte_order, &record, 12), frame, "{byte_order:?}: the new parent");
                    assert_eq!(read_u16(byte_order, &record[16..18]) as i16, 5, "{byte_order:?}: x");
                    assert_eq!(read_u16(byte_order, &record[18..20]) as i16, 6, "{byte_order:?}: y");
                    u32_at(byte_order, &record, 4)
                })
                .collect::<Vec<_>>();
            copies.sort_unstable();
            let mut expected = vec![child, ROOT, frame];
            expected.sort_unstable();
            assert_eq!(copies, expected, "{byte_order:?}: one copy per window it was selected on");
            // QueryTree agrees.
            owner.write_all(&resource_request(byte_order, 15, child)).unwrap();
            let reply = read_x_reply(&mut owner, byte_order);
            assert_eq!(read_u32(byte_order, &reply[12..16]), frame, "{byte_order:?}: QueryTree parent");
            // The parent map followed: the destroy's parent copy reaches the
            // frame's watcher, and the root's watcher hears nothing of it.
            owner.write_all(&resource_request(byte_order, 4, child)).unwrap();
            sync(byte_order, &mut owner);
            let mut destroys = events(byte_order, &mut watcher, 2)
                .into_iter()
                .map(|record| {
                    assert_eq!(record[0] & 0x7f, DESTROY_NOTIFY, "{byte_order:?}: {record:?}");
                    u32_at(byte_order, &record, 4)
                })
                .collect::<Vec<_>>();
            destroys.sort_unstable();
            let mut expected = vec![child, frame];
            expected.sort_unstable();
            assert_eq!(destroys, expected, "{byte_order:?}: the new parent's copy, not the old one's");
            assert!(sync(byte_order, &mut watcher).is_empty(), "{byte_order:?}: nothing else was owed");
            drop(watcher);
            drop(owner);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn a_mapped_window_is_unmapped_reparented_and_mapped_again_in_that_order() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("mapped", 2);
            let (mut owner, base) = connected(&socket_path, byte_order);
            let (mut watcher, _) = connected(&socket_path, byte_order);
            let frame = base + 1;
            let child = base + 2;
            owner.write_all(&create_window_request(byte_order, frame, 0, 0, 100, 100)).unwrap();
            owner.write_all(&map_window_request(byte_order, frame)).unwrap();
            owner.write_all(&create_window_request(byte_order, child, 0, 0, 40, 40)).unwrap();
            owner.write_all(&map_window_request(byte_order, child)).unwrap();
            sync(byte_order, &mut owner);
            watcher.write_all(&change_window_event_mask_request(byte_order, child, STRUCTURE_NOTIFY)).unwrap();
            sync(byte_order, &mut watcher);

            owner.write_all(&reparent_request(byte_order, child, frame, 0, 0)).unwrap();
            sync(byte_order, &mut owner);
            let kinds = events(byte_order, &mut watcher, 3)
                .into_iter()
                .map(|record| {
                    assert_eq!(u32_at(byte_order, &record, 4), child, "{byte_order:?}: addressed to the window");
                    record[0] & 0x7f
                })
                .collect::<Vec<_>>();
            assert_eq!(kinds, vec![UNMAP_NOTIFY, REPARENT_NOTIFY, MAP_NOTIFY], "{byte_order:?}");
            assert!(sync(byte_order, &mut watcher).is_empty(), "{byte_order:?}: nothing else was owed");
            drop(watcher);
            drop(owner);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
