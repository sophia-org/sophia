// KillClient, SetCloseDownMode and ChangeSaveSet over real sockets, on the
// routed service where more than one client lives.
//
// WHAT THESE PROVE. KillClient by a peer's window id ends that peer (its
// socket reads EOF, its window answers BadWindow to a third client) while
// the killer's round trip completes, and an id nobody holds is BadValue. A
// client that set RetainPermanent leaves its window answering after it
// departs, until a KillClient names it; RetainTemporary is freed by
// KillClient AllTemporary. A window-manager-like client that reparents a
// peer's window under its frame and saves it departs, and the peer's window
// is back under the root and mapped, with UnmapNotify, ReparentNotify and
// MapNotify to whoever selected on it. Before t166 all three were
// BadRequest, and a desktop without KillClient cannot force a window closed.

#[cfg(unix)]
mod client_lifetime {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const BAD_VALUE: u8 = 2;
    const BAD_WINDOW: u8 = 3;
    const BAD_MATCH: u8 = 8;
    const UNMAP_NOTIFY: u8 = 18;
    const MAP_NOTIFY: u8 = 19;
    const REPARENT_NOTIFY: u8 = 21;

    fn routed_service(name: &str, clients: usize) -> (std::path::PathBuf, thread::JoinHandle<Result<(), X11SetupSocketError>>) {
        let socket_path = std::env::temp_dir().join(format!(
            "sophia-x-lifetime-{name}-{}-{}.sock",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(1366),
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

    fn reads_eof(client: &mut std::os::unix::net::UnixStream) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut buffer = [0u8; 4096];
        loop {
            assert!(std::time::Instant::now() < deadline, "the peer was never ended");
            match client.read(&mut buffer) {
                Ok(0) => return true,
                Ok(_) => continue,
                Err(error) if matches!(error.kind(), std::io::ErrorKind::ConnectionReset) => return true,
                Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                Err(error) => panic!("reading the ended peer: {error}"),
            }
        }
    }

    /// GetWindowAttributes: a reply, or the error's code.
    fn attributes(byte_order: XByteOrder, client: &mut std::os::unix::net::UnixStream, window: u32) -> Result<(), u8> {
        client.write_all(&resource_request(byte_order, 3, window)).unwrap();
        let record = read_x_record(client);
        match record[0] {
            1 => {
                let mut rest = [0u8; 12];
                fill_from_socket(client, &mut rest);
                Ok(())
            }
            0 => Err(record[1]),
            other => panic!("{byte_order:?}: unexpected record {other} for GetWindowAttributes"),
        }
    }

    #[test]
    fn kill_client_ends_the_owner_and_names_an_id_nobody_holds() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("kill", 3);
            let (mut victim, victim_base) = connected(&socket_path, byte_order);
            let (mut killer, _) = connected(&socket_path, byte_order);
            let (mut witness, _) = connected(&socket_path, byte_order);
            let window = victim_base + 1;
            victim.write_all(&create_window_request(byte_order, window, 0, 0, 32, 32)).unwrap();
            victim.write_all(&map_window_request(byte_order, window)).unwrap();
            victim.write_all(&get_input_focus(byte_order)).unwrap();
            loop {
                if read_x_record(&mut victim)[0] == 1 {
                    break;
                }
            }
            assert_eq!(attributes(byte_order, &mut witness, window), Ok(()), "{byte_order:?}: the window exists");
            // An id nobody holds is BadValue carrying it.
            killer.write_all(&request(byte_order, 113, 0, &[0x7ff0_0001])).unwrap();
            let record = read_x_record(&mut killer);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_VALUE, 113), "{byte_order:?}: {record:?}");
            assert_eq!(read_u32(byte_order, &record[4..8]), 0x7ff0_0001);
            // The victim's window names the victim: it is ended, and the
            // killer's round trip completes.
            killer.write_all(&request(byte_order, 113, 0, &[window])).unwrap();
            killer.write_all(&get_input_focus(byte_order)).unwrap();
            let record = read_x_record(&mut killer);
            assert_eq!(record[0], 1, "{byte_order:?}: the killer is served: {record:?}");
            assert!(reads_eof(&mut victim), "{byte_order:?}: the victim reads EOF");
            let gone = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                match attributes(byte_order, &mut witness, window) {
                    Err(code) => {
                        assert_eq!(code, BAD_WINDOW, "{byte_order:?}: the victim's window is gone");
                        break;
                    }
                    Ok(()) => {
                        assert!(std::time::Instant::now() < gone, "{byte_order:?}: the victim's window outlived it");
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
            }
            drop(victim);
            drop(killer);
            drop(witness);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn a_retained_range_outlives_its_client_until_killed() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("retain", 3);
            let (mut permanent, permanent_base) = connected(&socket_path, byte_order);
            let (mut temporary, temporary_base) = connected(&socket_path, byte_order);
            let (mut witness, _) = connected(&socket_path, byte_order);
            let kept = permanent_base + 1;
            let fleeting = temporary_base + 1;
            // Mode 3 is BadValue; then RetainPermanent and RetainTemporary.
            permanent.write_all(&request(byte_order, 112, 3, &[])).unwrap();
            let record = read_x_record(&mut permanent);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_VALUE, 112), "{byte_order:?}: {record:?}");
            permanent.write_all(&request(byte_order, 112, 1, &[])).unwrap();
            permanent.write_all(&create_window_request(byte_order, kept, 0, 0, 32, 32)).unwrap();
            permanent.write_all(&get_input_focus(byte_order)).unwrap();
            assert_eq!(read_x_record(&mut permanent)[0], 1);
            temporary.write_all(&request(byte_order, 112, 2, &[])).unwrap();
            temporary.write_all(&create_window_request(byte_order, fleeting, 0, 0, 32, 32)).unwrap();
            temporary.write_all(&get_input_focus(byte_order)).unwrap();
            assert_eq!(read_x_record(&mut temporary)[0], 1);
            drop(permanent);
            drop(temporary);
            // Both windows answer after their clients departed.
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(attributes(byte_order, &mut witness, kept), Ok(()), "{byte_order:?}: the permanent window is retained");
            assert_eq!(attributes(byte_order, &mut witness, fleeting), Ok(()), "{byte_order:?}: the temporary window is retained");
            // AllTemporary frees the temporary range only.
            witness.write_all(&request(byte_order, 113, 0, &[0])).unwrap();
            witness.write_all(&get_input_focus(byte_order)).unwrap();
            assert_eq!(read_x_record(&mut witness)[0], 1);
            assert_eq!(attributes(byte_order, &mut witness, fleeting), Err(BAD_WINDOW), "{byte_order:?}: the temporary range is freed");
            assert_eq!(attributes(byte_order, &mut witness, kept), Ok(()), "{byte_order:?}: the permanent one stays");
            // Naming a resource in the permanent range frees it.
            witness.write_all(&request(byte_order, 113, 0, &[kept])).unwrap();
            witness.write_all(&get_input_focus(byte_order)).unwrap();
            assert_eq!(read_x_record(&mut witness)[0], 1);
            assert_eq!(attributes(byte_order, &mut witness, kept), Err(BAD_WINDOW), "{byte_order:?}: the permanent range is freed on demand");
            drop(witness);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }

    #[test]
    fn a_departing_managers_save_set_returns_a_peers_window_to_the_root_mapped() {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let (socket_path, server) = routed_service("saveset", 2);
            let (mut manager, manager_base) = connected(&socket_path, byte_order);
            let (mut peer, peer_base) = connected(&socket_path, byte_order);
            let frame = manager_base + 1;
            let window = peer_base + 1;
            manager.write_all(&create_window_request(byte_order, frame, 0, 0, 100, 100)).unwrap();
            manager.write_all(&map_window_request(byte_order, frame)).unwrap();
            manager.write_all(&get_input_focus(byte_order)).unwrap();
            loop {
                if read_x_record(&mut manager)[0] == 1 {
                    break;
                }
            }
            // The peer's window, selecting StructureNotify, reparented into
            // the frame by the manager and mapped.
            peer.write_all(&create_window_request(byte_order, window, 0, 0, 40, 40)).unwrap();
            peer.write_all(&change_window_event_mask_request(byte_order, window, 1 << 17)).unwrap();
            peer.write_all(&get_input_focus(byte_order)).unwrap();
            loop {
                if read_x_record(&mut peer)[0] == 1 {
                    break;
                }
            }
            manager.write_all(&request(byte_order, 7, 0, &[window, frame, 0])).unwrap();
            manager.write_all(&map_window_request(byte_order, window)).unwrap();
            // Refusals first: the manager's own frame is BadMatch, an unknown
            // window BadWindow, mode 2 BadValue.
            manager.write_all(&request(byte_order, 6, 0, &[frame])).unwrap();
            let record = read_x_record(&mut manager);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_MATCH, 6), "{byte_order:?}: {record:?}");
            manager.write_all(&request(byte_order, 6, 0, &[0x7ff0_0001])).unwrap();
            let record = read_x_record(&mut manager);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_WINDOW, 6), "{byte_order:?}: {record:?}");
            manager.write_all(&request(byte_order, 6, 2, &[window])).unwrap();
            let record = read_x_record(&mut manager);
            assert_eq!((record[0], record[1], record[10]), (0, BAD_VALUE, 6), "{byte_order:?}: {record:?}");
            // Saved, then the manager departs.
            manager.write_all(&request(byte_order, 6, 0, &[window])).unwrap();
            manager.write_all(&get_input_focus(byte_order)).unwrap();
            loop {
                if read_x_record(&mut manager)[0] == 1 {
                    break;
                }
            }
            // The peer is owed the MapNotify of its window inside the frame.
            // A routed event is queued behind the peer's own writer, so a
            // round trip does not drain it: wait for the notice itself, or
            // it lands among the save-set events below.
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                assert!(std::time::Instant::now() < deadline, "{byte_order:?}: the map inside the frame was never reported");
                if read_x_record(&mut peer)[0] & 0x7f == MAP_NOTIFY {
                    break;
                }
            }
            drop(manager);
            // Unmap, reparent to the root, map: in that order, to the peer.
            let mut seen = Vec::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while seen.len() < 3 {
                assert!(std::time::Instant::now() < deadline, "{byte_order:?}: save-set events never came: {seen:?}");
                let record = read_x_record(&mut peer);
                if record[0] >= 2 {
                    seen.push(record);
                }
            }
            let kinds = seen.iter().map(|e| e[0] & 0x7f).collect::<Vec<_>>();
            assert_eq!(kinds, vec![UNMAP_NOTIFY, REPARENT_NOTIFY, MAP_NOTIFY], "{byte_order:?}: {seen:?}");
            assert_eq!(read_u32(byte_order, &seen[1][12..16]), X_SETUP_DEFAULT_ROOT, "{byte_order:?}: back under the root");
            peer.write_all(&resource_request(byte_order, 15, window)).unwrap();
            let reply = read_x_reply(&mut peer, byte_order);
            assert_eq!(read_u32(byte_order, &reply[12..16]), X_SETUP_DEFAULT_ROOT, "{byte_order:?}: QueryTree agrees");
            assert_eq!(attributes(byte_order, &mut peer, window), Ok(()), "{byte_order:?}: the window lives");
            drop(peer);
            server.join().unwrap().unwrap();
            let _ = std::fs::remove_file(&socket_path);
        }
    }
}
