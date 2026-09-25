fn installed_map(order: XByteOrder, client: &mut std::os::unix::net::UnixStream) -> u32 {
    use std::io::Read;
    client
        .write_all(&request(order, 83, 0, &[X_SETUP_DEFAULT_ROOT]))
        .unwrap();
    let reply = read_x_record(client);
    assert_eq!(reply[0], 1);
    assert_eq!(read_u32(order, &reply[4..8]), 1);
    let mut map = [0; 4];
    client.read_exact(&mut map).unwrap();
    read_u32(order, &map)
}

fn installed_attribute(
    order: XByteOrder,
    client: &mut std::os::unix::net::UnixStream,
    window: u32,
) -> bool {
    use std::io::Read;
    client.write_all(&request(order, 3, 0, &[window])).unwrap();
    let reply = read_x_record(client);
    assert_eq!(reply[0], 1);
    client.read_exact(&mut [0; 12]).unwrap();
    reply[25] != 0
}

fn installation_notice(order: XByteOrder, record: &[u8; 32]) -> (u32, u32, u8, u8) {
    let (window, map, new) = notify(order, record);
    (window, map, new, record[13])
}

#[test]
fn installed_maps_replace_notify_and_restore_on_free() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let (path, server) = routed_service("installed", 2);
        let (mut owner, base) = connected(&path, order);
        let (mut watcher, _) = connected(&path, order);
        let default_window = base + 1;
        let custom_window = base + 2;
        let map = base + 3;
        for window in [default_window, custom_window] {
            owner
                .write_all(&create_window_request(order, window, 0, 0, 20, 20))
                .unwrap();
        }
        owner
            .write_all(&request(
                order,
                78,
                0,
                &[map, custom_window, X_SETUP_DEFAULT_VISUAL],
            ))
            .unwrap();
        set_colormap(order, &mut owner, custom_window, map);
        assert!(sync(order, &mut owner).is_empty());
        for window in [default_window, custom_window] {
            for client in [&mut owner, &mut watcher] {
                client
                    .write_all(&change_window_event_mask_request(
                        order,
                        window,
                        COLORMAP_CHANGE,
                    ))
                    .unwrap();
                assert!(sync(order, client).is_empty());
            }
        }
        assert_eq!(installed_map(order, &mut owner), X_SETUP_DEFAULT_COLORMAP);
        assert!(!installed_attribute(order, &mut owner, custom_window));
        owner.write_all(&request(order, 81, 0, &[map])).unwrap();
        let expected = [
            (default_window, X_SETUP_DEFAULT_COLORMAP, 0, 0),
            (custom_window, map, 0, 1),
        ];
        let records = sync(order, &mut owner);
        assert_eq!(
            records
                .iter()
                .map(|r| installation_notice(order, r))
                .collect::<Vec<_>>(),
            expected
        );
        for notice in expected {
            assert_eq!(
                installation_notice(order, &next_event(order, &mut watcher)),
                notice
            );
        }
        assert_eq!(installed_map(order, &mut owner), map);
        assert!(installed_attribute(order, &mut owner, custom_window));
        assert!(!installed_attribute(
            order,
            &mut owner,
            X_SETUP_DEFAULT_ROOT
        ));
        // Reinstallation and uninstalling a map that is not installed do nothing.
        for (opcode, target) in [(81, map), (82, X_SETUP_DEFAULT_COLORMAP)] {
            owner
                .write_all(&request(order, opcode, 0, &[target]))
                .unwrap();
            assert!(sync(order, &mut owner).is_empty());
        }
        owner.write_all(&request(order, 82, 0, &[map])).unwrap();
        let expected = [
            (custom_window, map, 0, 0),
            (default_window, X_SETUP_DEFAULT_COLORMAP, 0, 1),
        ];
        let records = sync(order, &mut owner);
        assert_eq!(
            records
                .iter()
                .map(|r| installation_notice(order, r))
                .collect::<Vec<_>>(),
            expected
        );
        for notice in expected {
            assert_eq!(
                installation_notice(order, &next_event(order, &mut watcher)),
                notice
            );
        }
        assert_eq!(installed_map(order, &mut owner), X_SETUP_DEFAULT_COLORMAP);
        owner.write_all(&request(order, 81, 0, &[map])).unwrap();
        assert_eq!(sync(order, &mut owner).len(), 2);
        for _ in 0..2 {
            next_event(order, &mut watcher);
        }
        owner.write_all(&request(order, 79, 0, &[map])).unwrap();
        let expected = [
            (custom_window, map, 0, 0),
            (default_window, X_SETUP_DEFAULT_COLORMAP, 0, 1),
            (custom_window, 0, 1, 0),
        ];
        let records = sync(order, &mut owner);
        assert_eq!(
            records
                .iter()
                .map(|r| installation_notice(order, r))
                .collect::<Vec<_>>(),
            expected
        );
        for notice in expected {
            assert_eq!(
                installation_notice(order, &next_event(order, &mut watcher)),
                notice
            );
        }
        assert_eq!(installed_map(order, &mut owner), X_SETUP_DEFAULT_COLORMAP);
        assert!(!installed_attribute(order, &mut owner, custom_window));
        drop((owner, watcher));
        server.join().unwrap().unwrap();
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn disconnecting_the_installed_maps_owner_notifies_surviving_windows() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let (path, server) = routed_service("installed-close", 2);
        let (mut owner, base) = connected(&path, order);
        let (mut watcher, other_base) = connected(&path, order);
        let map = base + 1;
        let window = other_base + 1;
        owner
            .write_all(&request(
                order,
                78,
                0,
                &[map, X_SETUP_DEFAULT_ROOT, X_SETUP_DEFAULT_VISUAL],
            ))
            .unwrap();
        assert!(sync(order, &mut owner).is_empty());
        watcher
            .write_all(&create_window_request(order, window, 0, 0, 20, 20))
            .unwrap();
        set_colormap(order, &mut watcher, window, map);
        watcher
            .write_all(&change_window_event_mask_request(
                order,
                window,
                COLORMAP_CHANGE,
            ))
            .unwrap();
        assert!(sync(order, &mut watcher).is_empty());
        owner.write_all(&request(order, 81, 0, &[map])).unwrap();
        assert!(sync(order, &mut owner).is_empty());
        assert_eq!(
            installation_notice(order, &next_event(order, &mut watcher)),
            (window, map, 0, 1)
        );
        drop(owner);
        for expected in [(window, map, 0, 0), (window, 0, 1, 0)] {
            assert_eq!(
                installation_notice(order, &next_event(order, &mut watcher)),
                expected
            );
        }
        assert_eq!(installed_map(order, &mut watcher), X_SETUP_DEFAULT_COLORMAP);
        drop(watcher);
        server.join().unwrap().unwrap();
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn installed_colormap_state_and_root_notices_stay_in_the_namespace() {
    let order = XByteOrder::LittleEndian;
    let namespaces = [2213, 2214].map(|id| {
        NamespaceContext::new(
            NamespaceId::from_raw(id),
            NamespaceProfile::ClassicShared,
            NamespaceCapabilities::NONE,
        )
        .unwrap()
    });
    let policy = Arc::new(SequencedXAdmissionPolicy {
        namespaces: namespaces.to_vec(),
        next_client: std::sync::atomic::AtomicU64::new(0),
        revoked: std::sync::Mutex::new(Vec::new()),
    });
    let path = std::env::temp_dir().join(format!(
        "sophia-cmap-isolation-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new_with_namespace_context(&path, namespaces[0])
        .unwrap()
        .with_admission_policy(policy)
        .with_max_concurrent_clients(std::num::NonZeroUsize::new(2).unwrap());
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let broker = XServerFrontendRouteBroker::new(std::num::NonZeroUsize::new(64).unwrap());
        for _ in 0..2 {
            frontend.serve_next_concurrently_routed(&broker).unwrap();
        }
        frontend.wait_for_clients().unwrap();
    });
    wait_for_socket(&path);
    let (mut owner, base) = connected(&path, order);
    let (mut foreign, _) = connected(&path, order);
    for client in [&mut owner, &mut foreign] {
        client
            .write_all(&change_window_event_mask_request(
                order,
                X_SETUP_DEFAULT_ROOT,
                COLORMAP_CHANGE,
            ))
            .unwrap();
        assert!(sync(order, client).is_empty());
    }
    let map = base + 1;
    owner
        .write_all(&request(
            order,
            78,
            0,
            &[map, X_SETUP_DEFAULT_ROOT, X_SETUP_DEFAULT_VISUAL],
        ))
        .unwrap();
    owner.write_all(&request(order, 81, 0, &[map])).unwrap();
    let records = sync(order, &mut owner);
    assert_eq!(records.len(), 1);
    assert_eq!(
        installation_notice(order, &records[0]),
        (X_SETUP_DEFAULT_ROOT, X_SETUP_DEFAULT_COLORMAP, 0, 0)
    );
    assert!(sync(order, &mut foreign).is_empty());
    assert_eq!(installed_map(order, &mut foreign), X_SETUP_DEFAULT_COLORMAP);
    assert_eq!(installed_map(order, &mut owner), map);
    foreign.write_all(&request(order, 81, 0, &[map])).unwrap();
    let errors = sync(order, &mut foreign);
    assert_eq!((errors[0][0], errors[0][1]), (0, 12));
    assert_eq!(installed_map(order, &mut owner), map);
    drop(owner);
    drop(foreign);
    server.join().unwrap();
    let _ = std::fs::remove_file(path);
}
