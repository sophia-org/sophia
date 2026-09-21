
/// Atoms a client interned are undefined once the last connection closes.
///
/// The protocol scopes an atom to the server's lifetime, and for a server
/// that is one connection's worth of lifetime, that is the moment. Our
/// authority outlives its connections, so this is the one place the rule has
/// to be honoured explicitly rather than falling out of the process ending.
#[cfg(unix)]
#[test]
fn a_client_interned_atom_is_undefined_once_the_last_connection_closes() {
    use std::io::Write;
    use std::num::NonZeroUsize;
    use std::thread;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-atom-lifetime-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let (transaction_sender, _transactions) =
        std::sync::mpsc::sync_channel(X_AUTHORITY_OBSERVED_TRANSACTION_CHANNEL_CAPACITY);
    let (acknowledgement_sender, _acks) = std::sync::mpsc::sync_channel(2);
    let broker = XServerFrontendRouteBroker::with_control_ack_sender(
        NonZeroUsize::new(4).unwrap(),
        acknowledgement_sender,
    );
    let (service_sender, service_receiver) = std::sync::mpsc::sync_channel(1);
    let config = XServerFrontendConfig::new(&server_path, NamespaceId::from_raw(864)).unwrap();
    let server = thread::spawn(move || {
        run_x_server_frontend_routed_until_stopped(
            config,
            transaction_sender,
            broker,
            service_receiver,
        )
        .unwrap();
    });
    wait_for_socket(&socket_path);

    let intern = |stream: &mut std::os::unix::net::UnixStream, only_if_exists: bool| -> u32 {
        stream
            .write_all(&intern_atom_request(
                XByteOrder::LittleEndian,
                only_if_exists,
                "SOPHIA_ATOM_LIFETIME",
            ))
            .unwrap();
        let reply = read_x_record(stream);
        assert_eq!(reply[0], 1, "a reply, not an error: {reply:?}");
        read_u32(XByteOrder::LittleEndian, &reply[8..12])
    };

    let mut first = connect_x_socket(&socket_path);
    first
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut first, XByteOrder::LittleEndian);
    let created = intern(&mut first, false);
    assert_ne!(0, created, "interning a new name returns an atom");
    assert_eq!(
        created,
        intern(&mut first, true),
        "and the same connection can find it again"
    );
    drop(first);

    // The teardown is the server's own work on its own thread, so this waits
    // for it rather than assuming it has happened by the time a new
    // connection is accepted.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut looked_up = None;
    while Instant::now() < deadline {
        let mut next = connect_x_socket(&socket_path);
        next.write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
            .unwrap();
        read_setup_success(&mut next, XByteOrder::LittleEndian);
        let found = intern(&mut next, true);
        drop(next);
        looked_up = Some(found);
        if found == 0 {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(
        Some(0),
        looked_up,
        "the name a departed client interned is undefined to the next one"
    );

    service_sender
        .send(XServerFrontendServiceCommand::StopAccepting)
        .unwrap();
    drop(service_sender);
    server.join().unwrap();
    let _ = std::fs::remove_file(&socket_path);
}
