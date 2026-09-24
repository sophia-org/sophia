// A window's colormap attribute (t210): ChangeWindowAttributes keeps the
// colormap it names, refuses an unknown one as a Color error and one of
// another visual as a Match error, and tells every client that selected
// ColormapChange on the window, each with the window, the colormap and
// new = True. Setting the same colormap again tells no one. Freeing the
// colormap sets the window's colormap to None and tells the same clients.

#[cfg(unix)]
mod colormap_notify {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const COLORMAP_NOTIFY: u8 = 32;
    const COLORMAP_CHANGE: u32 = 1 << 23;
    const CW_COLORMAP: u32 = 1 << 13;

    fn routed_service(name: &str, clients: usize) -> (std::path::PathBuf, thread::JoinHandle<Result<(), X11SetupSocketError>>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-cmap-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(2101),
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

    /// Everything up to a GetInputFocus reply: events, and errors as `0`.
    fn sync(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream) -> Vec<[u8; 32]> {
        let mut records = Vec::new();
        client.write_all(&request(byte_order, 43, 0, &[])).unwrap();
        loop {
            let record = read_x_record(client);
            if record[0] == 1 {
                return records;
            }
            records.push(record);
        }
    }

    /// The next record, which must be an event: a routed event reaches a
    /// peer behind that peer's own writer, so it is waited for, not synced.
    fn next_event(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream) -> [u8; 32] {
        let record = read_x_record(client);
        assert!(record[0] >= 2, "{byte_order:?}: not an event: {record:?}");
        record
    }

    fn set_colormap(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream, window: u32, colormap: u32) {
        client.write_all(&request(byte_order, 2, 0, &[window, CW_COLORMAP, colormap])).unwrap();
    }

    fn notify(byte_order: XByteOrder, record: &[u8; 32]) -> (u32, u32, u8) {
        assert_eq!(record[0], COLORMAP_NOTIFY, "{byte_order:?}: {record:?}");
        (read_u32(byte_order, &record[4..8]), read_u32(byte_order, &record[8..12]), record[12])
    }

    #[test]
    fn a_new_window_colormap_is_told_to_every_client_that_selected_it() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("told", 3);
            let (mut owner, base) = connected(&socket_path, byte_order);
            let (mut watcher, _) = connected(&socket_path, byte_order);
            let (mut bystander, _) = connected(&socket_path, byte_order);
            let window = base + 1;
            let colormap = base + 2;
            owner.write_all(&create_window_request(byte_order, window, 0, 0, 40, 40)).unwrap();
            owner.write_all(&request(byte_order, 78, 0, &[colormap, window, sophia_x_authority::X_SETUP_DEFAULT_VISUAL])).unwrap();
            owner.write_all(&change_window_event_mask_request(byte_order, window, COLORMAP_CHANGE)).unwrap();
            assert!(sync(byte_order, &mut owner).is_empty());
            watcher.write_all(&change_window_event_mask_request(byte_order, window, COLORMAP_CHANGE)).unwrap();
            assert!(sync(byte_order, &mut watcher).is_empty());
            assert!(sync(byte_order, &mut bystander).is_empty());

            set_colormap(byte_order, &mut owner, window, colormap);
            let records = sync(byte_order, &mut owner);
            assert_eq!(records.len(), 1, "{byte_order:?}: owner: {records:?}");
            assert_eq!(notify(byte_order, &records[0]), (window, colormap, 1), "{byte_order:?}: owner");
            let record = next_event(byte_order, &mut watcher);
            assert_eq!(notify(byte_order, &record), (window, colormap, 1), "{byte_order:?}: watcher");
            // By now the notice has gone out to every selector.
            assert!(sync(byte_order, &mut bystander).is_empty(), "{byte_order:?}: no selection, no event");

            // The same colormap again is no change.
            set_colormap(byte_order, &mut owner, window, colormap);
            assert!(sync(byte_order, &mut owner).is_empty(), "{byte_order:?}: an unchanged colormap");

            // An unknown colormap is a Color error and changes nothing.
            set_colormap(byte_order, &mut owner, window, base + 9);
            let records = sync(byte_order, &mut owner);
            assert_eq!(records.len(), 1, "{byte_order:?}: {records:?}");
            assert_eq!((records[0][0], records[0][1]), (0, 12), "{byte_order:?}: BadColor");

            // Freeing the colormap leaves the window with None, and says so.
            owner.write_all(&request(byte_order, 79, 0, &[colormap])).unwrap();
            let records = sync(byte_order, &mut owner);
            assert_eq!(records.len(), 1, "{byte_order:?}: owner: {records:?}");
            assert_eq!(notify(byte_order, &records[0]), (window, 0, 1), "{byte_order:?}: owner after free");
            let record = next_event(byte_order, &mut watcher);
            assert_eq!(notify(byte_order, &record), (window, 0, 1), "{byte_order:?}: watcher after free");
            drop((owner, watcher, bystander));
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
