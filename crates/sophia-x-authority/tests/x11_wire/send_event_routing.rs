// SendEvent reaches whoever it is for (t182): with no event mask, the owner
// of the destination window; with one, every client selecting any of those
// events on the destination, climbing to the ancestors when the request
// says to propagate and nobody on the window selected. The sender keeps a
// copy only when it is among them. Before, the ClientMessage form reached
// only the sender, whoever the destination belonged to.

#[cfg(unix)]
mod send_event_routing {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const ROOT: u32 = 0x20;
    const CLIENT_MESSAGE: u8 = 33;
    const EXPOSURE: u32 = 1 << 15;
    const STRUCTURE_NOTIFY: u32 = 1 << 17;
    const SUBSTRUCTURE_NOTIFY: u32 = 1 << 19;

    fn routed_service(name: &str, clients: usize) -> (std::path::PathBuf, thread::JoinHandle<Result<(), X11SetupSocketError>>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-send-event-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1822),
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

    /// A SendEvent carrying a ClientMessage (format 32) whose first datum
    /// is `data`, addressed to `window` inside the message.
    fn send_event(byte_order: XByteOrder, propagate: bool, destination: u32, mask: u32, window: u32, data: u32) -> Vec<u8> {
        let mut out = vec![25, u8::from(propagate)];
        push_u16(&mut out, byte_order, 11);
        push_u32(&mut out, byte_order, destination);
        push_u32(&mut out, byte_order, mask);
        let start = out.len();
        out.extend_from_slice(&[0; 32]);
        out[start] = CLIENT_MESSAGE;
        out[start + 1] = 32;
        let mut window_bytes = Vec::new();
        push_u32(&mut window_bytes, byte_order, window);
        out[start + 4..start + 8].copy_from_slice(&window_bytes);
        let mut data_bytes = Vec::new();
        push_u32(&mut data_bytes, byte_order, data);
        out[start + 12..start + 16].copy_from_slice(&data_bytes);
        out
    }

    /// Routed events are queued behind a peer's writer, so a round trip does
    /// not drain them: read until the message carrying `data` arrives, and
    /// return every sent message seen on the way, which is how a message
    /// nobody was owed is shown to have not arrived (it would have come
    /// before the one that was).
    fn messages_until(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream, data: u32) -> Vec<[u8; 32]> {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        loop {
            assert!(std::time::Instant::now() < deadline, "{byte_order:?}: the message {data} never came: {seen:?}");
            let record = read_x_record(client);
            assert!(record[0] >= 2, "{byte_order:?}: not an event: {record:?}");
            if record[0] & 0x7f != CLIENT_MESSAGE {
                continue;
            }
            let datum = read_u32(byte_order, &record[12..16]);
            seen.push(record);
            if datum == data {
                return seen;
            }
        }
    }

    fn assert_message(byte_order: XByteOrder, record: &[u8; 32], window: u32, data: u32, what: &str) {
        assert_eq!(record[0], CLIENT_MESSAGE | 0x80, "{byte_order:?}: {what}: marked as sent: {record:?}");
        assert_eq!(record[1], 32, "{byte_order:?}: {what}: format");
        assert_eq!(read_u32(byte_order, &record[4..8]), window, "{byte_order:?}: {what}: the message's window");
        assert_eq!(read_u32(byte_order, &record[12..16]), data, "{byte_order:?}: {what}: the datum");
    }

    #[test]
    fn a_sent_message_reaches_the_destinations_owner_and_the_sender_keeps_no_copy() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("owner", 2);
            let (mut owner, owner_base) = connected(&socket_path, byte_order);
            let (mut sender, sender_base) = connected(&socket_path, byte_order);
            let window = owner_base + 1;
            let own = sender_base + 1;
            owner.write_all(&create_window_request(byte_order, window, 0, 0, 40, 40)).unwrap();
            sync(byte_order, &mut owner);
            sender.write_all(&create_window_request(byte_order, own, 0, 0, 40, 40)).unwrap();
            sync(byte_order, &mut sender);

            // To a peer's window: the peer reads it, the sender does not (its
            // own copy is local to the request, so its round trip is exact).
            sender.write_all(&send_event(byte_order, false, window, 0, window, 7)).unwrap();
            assert!(sync(byte_order, &mut sender).is_empty(), "{byte_order:?}: the sender keeps no copy");
            let owed = messages_until(byte_order, &mut owner, 7);
            assert_eq!(owed.len(), 1, "{byte_order:?}: {owed:?}");
            assert_message(byte_order, &owed[0], window, 7, "to a peer");
            // To its own window: the sender is the owner and reads it, and the
            // peer is not told; a second message to the peer proves the
            // first was never owed, since it would have arrived before it.
            sender.write_all(&send_event(byte_order, false, own, 0, own, 8)).unwrap();
            let owed = sync(byte_order, &mut sender);
            assert_eq!(owed.len(), 1, "{byte_order:?}: {owed:?}");
            assert_message(byte_order, &owed[0], own, 8, "to itself");
            sender.write_all(&send_event(byte_order, false, window, 0, window, 9)).unwrap();
            let owed = messages_until(byte_order, &mut owner, 9);
            assert_eq!(owed.len(), 1, "{byte_order:?}: not the peer's: {owed:?}");
            // To an unknown window: BadWindow, as before.
            sender.write_all(&send_event(byte_order, false, 0x7ff0_0001, 0, window, 10)).unwrap();
            let record = read_x_record(&mut sender);
            assert_eq!((record[0], record[1], record[10]), (0, 3, 25), "{byte_order:?}: {record:?}");
            drop(sender);
            drop(owner);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn a_sent_message_with_a_mask_reaches_whoever_selected_and_propagates_when_asked() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("mask", 2);
            let (mut watcher, watcher_base) = connected(&socket_path, byte_order);
            let (mut sender, _) = connected(&socket_path, byte_order);
            let window = watcher_base + 1;
            watcher.write_all(&create_window_request(byte_order, window, 0, 0, 40, 40)).unwrap();
            watcher.write_all(&change_window_event_mask_request(byte_order, window, STRUCTURE_NOTIFY)).unwrap();
            watcher.write_all(&change_window_event_mask_request(byte_order, ROOT, SUBSTRUCTURE_NOTIFY)).unwrap();
            sync(byte_order, &mut watcher);

            // Not selected: nobody, no error. Then selected on the window:
            // delivered, and first of the two to arrive, so the first never
            // did.
            sender.write_all(&send_event(byte_order, false, window, EXPOSURE, window, 2)).unwrap();
            sender.write_all(&send_event(byte_order, false, window, STRUCTURE_NOTIFY, window, 1)).unwrap();
            assert!(sync(byte_order, &mut sender).is_empty(), "{byte_order:?}: the sender selected nothing");
            let owed = messages_until(byte_order, &mut watcher, 1);
            assert_eq!(owed.len(), 1, "{byte_order:?}: {owed:?}");
            assert_message(byte_order, &owed[0], window, 1, "selected on the window");
            // Nobody on the window and no propagate: nobody. With propagate:
            // the root's selector reads it, and only it.
            sender.write_all(&send_event(byte_order, false, window, SUBSTRUCTURE_NOTIFY, window, 4)).unwrap();
            sender.write_all(&send_event(byte_order, true, window, SUBSTRUCTURE_NOTIFY, window, 3)).unwrap();
            assert!(sync(byte_order, &mut sender).is_empty(), "{byte_order:?}");
            let owed = messages_until(byte_order, &mut watcher, 3);
            assert_eq!(owed.len(), 1, "{byte_order:?}: {owed:?}");
            assert_message(byte_order, &owed[0], window, 3, "propagated to the root");
            drop(sender);
            drop(watcher);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
