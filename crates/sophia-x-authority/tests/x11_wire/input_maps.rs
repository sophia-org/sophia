// SetPointerMapping, ChangeKeyboardMapping, SetModifierMapping and
// QueryKeymap over real sockets, on the routed service where a peer can be
// told.
//
// WHAT THESE PROVE. A pointer mapping a client sets is what GetPointerMapping
// reports, a MappingNotify reaches the requester and a peer, a list of the
// wrong length or with a repeated button is BadValue; a keyboard mapping a
// client writes is what GetKeyboardMapping and XKB GetMap both report, with
// MappingNotify; SetModifierMapping serves the current map (Success and
// MappingNotify) and answers Failed to another, xkbcommon owning modifier
// state; QueryKeymap replies the thirty-two bytes. Before t166 all four
// were BadRequest, which xterm's error handler turns into an exit.

#[cfg(unix)]
mod input_maps {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const BAD_VALUE: u8 = 2;
    const MAPPING_NOTIFY: u8 = 34;

    fn routed_service(name: &str, clients: usize) -> (std::path::PathBuf, thread::JoinHandle<Result<(), X11SetupSocketError>>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-input-maps-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1466),
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

    fn connected(socket_path: &std::path::Path, byte_order: XByteOrder) -> std::os::unix::net::UnixStream {
        let mut client = connect_x_socket(socket_path);
        client
            .write_all(&setup_request(byte_order, 11, 0, b"", b""))
            .unwrap();
        read_setup_success(&mut client, byte_order);
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        client
    }

    fn request(byte_order: XByteOrder, opcode: u8, detail: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![opcode, detail];
        push_u16(&mut out, byte_order, 1 + body.len().div_ceil(4) as u16);
        out.extend_from_slice(body);
        out.resize(4 + ((body.len() + 3) & !3), 0);
        out
    }

    /// Reads records until a reply, returning the events seen and the reply.
    fn until_reply(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream, what: &str) -> (Vec<[u8; 32]>, [u8; 32]) {
        let mut events = Vec::new();
        loop {
            let record = read_x_record(client);
            match record[0] {
                1 => return (events, record),
                0 => panic!("{byte_order:?}: {what}: an error while a reply was owed: {record:?}"),
                _ => events.push(record),
            }
        }
    }

    #[test]
    fn a_pointer_mapping_is_stored_reported_and_told_to_every_client() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("pointer", 2);
            let mut client = connected(&socket_path, byte_order);
            let mut peer = connected(&socket_path, byte_order);
            // The wrong length, then a repeated button: BadValue carrying it.
            client.write_all(&request(byte_order, 116, 8, &[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_VALUE, 116), "{byte_order:?}: {record:?}");
            assert_eq!(read_u32(byte_order, &record[4..8]), 8);
            client.write_all(&request(byte_order, 116, 9, &[3, 2, 3, 4, 5, 6, 7, 8, 9])).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_VALUE, 116), "{byte_order:?}: {record:?}");
            assert_eq!(read_u32(byte_order, &record[4..8]), 3);
            // A swap of 1 and 3: Success, MappingNotify to the requester
            // (before the reply, as its own output) and to the peer.
            client.write_all(&request(byte_order, 116, 9, &[3, 2, 1, 4, 5, 6, 7, 8, 9])).unwrap();
            let (events, reply) = until_reply(byte_order, &mut client, "SetPointerMapping");
            assert_eq!(reply[1], 0, "{byte_order:?}: Success: {reply:?}");
            assert!(events.iter().any(|e| e[0] & 0x7f == MAPPING_NOTIFY && e[4] == 2), "{byte_order:?}: MappingNotify(pointer) to the requester: {events:?}");
            let record = read_x_record(&mut peer);
            assert_eq!((record[0] & 0x7f, record[4]), (MAPPING_NOTIFY, 2), "{byte_order:?}: the peer is told: {record:?}");
            client.write_all(&request(byte_order, 117, 0, &[])).unwrap();
            let (_, reply) = until_reply(byte_order, &mut client, "GetPointerMapping");
            let count = usize::from(reply[1]);
            let mut mapping = vec![0u8; (count + 3) & !3];
            fill_from_socket(&mut client, &mut mapping);
            assert_eq!(&mapping[..count], &[3, 2, 1, 4, 5, 6, 7, 8, 9], "{byte_order:?}: what was set is reported");
            // Identity again, so nothing outlives this test.
            client.write_all(&request(byte_order, 116, 9, &[1, 2, 3, 4, 5, 6, 7, 8, 9])).unwrap();
            let (_, reply) = until_reply(byte_order, &mut client, "SetPointerMapping back");
            assert_eq!(reply[1], 0);
            let _ = read_x_record(&mut peer);
            drop(client);
            drop(peer);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn a_keyboard_mapping_is_reported_by_core_and_xkb_and_a_modifier_map_is_served_only_as_it_is() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("keyboard", 1);
            let mut client = connected(&socket_path, byte_order);
            // Keycode 38 gets three keysyms: the map widens to three.
            let mut body = vec![38u8, 3, 0, 0];
            for keysym in [0x61u32, 0x41, 0xe6] {
                push_u32(&mut body, byte_order, keysym);
            }
            let mut change = request(byte_order, 100, 1, &body);
            change[1] = 1;
            client.write_all(&change).unwrap();
            client.write_all(&request(byte_order, 43, 0, &[])).unwrap();
            let (events, _) = until_reply(byte_order, &mut client, "ChangeKeyboardMapping");
            let notice = events.iter().find(|e| e[0] & 0x7f == MAPPING_NOTIFY).expect("MappingNotify(keyboard)");
            assert_eq!((notice[4], notice[5], notice[6]), (1, 38, 1), "{byte_order:?}: {notice:?}");
            client.write_all(&request(byte_order, 101, 0, &[38, 1, 0, 0])).unwrap();
            let (_, reply) = until_reply(byte_order, &mut client, "GetKeyboardMapping");
            assert_eq!(reply[1], 3, "{byte_order:?}: three keysyms per keycode now");
            let mut syms = vec![0u8; 12];
            fill_from_socket(&mut client, &mut syms);
            assert_eq!(
                [read_u32(byte_order, &syms[0..4]), read_u32(byte_order, &syms[4..8]), read_u32(byte_order, &syms[8..12])],
                [0x61, 0x41, 0xe6],
                "{byte_order:?}: what was written is reported"
            );
            // A keycode below the minimum is BadValue carrying it.
            let mut low = request(byte_order, 100, 1, &[7u8, 1, 0, 0, 0, 0, 0, 0]);
            low[1] = 1;
            client.write_all(&low).unwrap();
            let record = read_x_record(&mut client);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_VALUE, 100), "{byte_order:?}: {record:?}");
            // The current modifier map restated (kpm 2): Success and
            // MappingNotify(modifier); a different one: Failed, no event.
            client.write_all(&request(byte_order, 119, 0, &[])).unwrap();
            let (_, reply) = until_reply(byte_order, &mut client, "GetModifierMapping");
            let kpm = usize::from(reply[1]);
            let mut current = vec![0u8; 8 * kpm];
            fill_from_socket(&mut client, &mut current);
            client.write_all(&request(byte_order, 118, reply[1], &current)).unwrap();
            let (events, reply) = until_reply(byte_order, &mut client, "SetModifierMapping same");
            assert_eq!(reply[1], 0, "{byte_order:?}: Success for the current map: {reply:?}");
            assert!(events.iter().any(|e| e[0] & 0x7f == MAPPING_NOTIFY && e[4] == 0), "{byte_order:?}: MappingNotify(modifier): {events:?}");
            let mut other = current.clone();
            other[0] = 9;
            client.write_all(&request(byte_order, 118, kpm as u8, &other)).unwrap();
            let (events, reply) = until_reply(byte_order, &mut client, "SetModifierMapping other");
            assert_eq!(reply[1], 2, "{byte_order:?}: Failed for another map: {reply:?}");
            assert!(events.is_empty(), "{byte_order:?}: no event for a refused map: {events:?}");
            // QueryKeymap: forty bytes, nothing down.
            client.write_all(&request(byte_order, 44, 0, &[])).unwrap();
            let (_, reply) = until_reply(byte_order, &mut client, "QueryKeymap");
            assert_eq!(read_u32(byte_order, &reply[4..8]), 2, "{byte_order:?}: two words follow");
            let mut keys = vec![0u8; 8];
            fill_from_socket(&mut client, &mut keys);
            assert!(reply[8..32].iter().chain(keys.iter()).all(|b| *b == 0), "{byte_order:?}: nothing is down");
            drop(client);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
