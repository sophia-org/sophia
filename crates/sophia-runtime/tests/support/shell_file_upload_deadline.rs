use super::*;

#[test]
fn partial_write_does_not_renew_idle_and_expired_tail_cannot_touch_successor() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(1);
    let grant = expected.grant;
    let idle = u64::from(expected.transfer_idle_timeout_ms);
    transport.reserve_content(&mut registry, expected).unwrap();
    let socket = transport.socket_path().to_owned();
    let (advance, clock) = std::sync::mpsc::channel();
    let (ready, advanced) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 1), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        peer.ack(&negotiated);
        peer.submit_acknowledged(&begin_record(1, 2, 0, upload_begin(grant, 1)), 2);
        assert_eq!(next_status(&mut peer).status, 1);
        assert_eq!(peer.open_path(10, &[b"upload", b"0"], 1).0, 13);

        advance.send(idle - 1).unwrap();
        advanced.recv_timeout(Duration::from_secs(2)).unwrap();
        // These bytes remain export scratch: no canonical chunk reaches the owner.
        assert_eq!(peer.write_at(10, 0, &[0, 0, 0, 255].repeat(250)).0, 119);
        advance.send(idle).unwrap();
        advanced.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(next_status(&mut peer).status, 3);
        for reply in peer.write_burst(10, &[(1000, &[0; 4]), (1004, &[0; 4])]) {
            assert_eq!(errno(reply), ESTALE);
        }

        peer.submit_acknowledged(&begin_record(1, 3, 0, upload_begin(grant, 2)), 3);
        assert_eq!(next_status(&mut peer).status, 1);
        assert_eq!(peer.open_path(11, &[b"upload", b"0"], 1).0, 13);
        assert_eq!(errno(peer.write_at(10, 0, &[0; 4])), ESTALE);
        peer.clunk(10);
        // An old writer's late clunk must not cancel the new binding.
        assert_eq!(peer.write_at(11, 0, &[0, 0, 0, 255]).0, 119);
        peer.clunk(11);
        assert_eq!(next_status(&mut peer).status, 4);
    });
    negotiate(&mut transport, &mut registry, 1, granted(), &peer)
        .unwrap()
        .expect("negotiated");
    let start = Instant::now();
    let mut now = 0;
    while !peer.is_finished() {
        let changed = if let Ok(next) = clock.try_recv() {
            now = next;
            true
        } else {
            false
        };
        match transport.service_content_resources(&mut registry, now) {
            Ok(_) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => panic!("resources: {error}"),
        }
        if changed {
            ready.send(()).unwrap();
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}
