/// A watcher that stops draining its queue is ended, not quietly skipped.
///
/// Dropping its events instead would leave an admitted client believing it is
/// still subscribed while the server stopped telling it things. A client that
/// cannot keep up has failed as an endpoint; everyone else carries on.
#[cfg(unix)]
#[test]
fn a_watcher_that_stops_draining_is_disconnected_and_the_rest_continue() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const PRIMARY: u32 = 1;

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-xfixes-stalled-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let namespace = NamespaceContext::new(
        NamespaceId::from_raw(841),
        NamespaceProfile::ClassicShared,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let policy = Arc::new(TestXAdmissionPolicy::new(namespace, false));
    let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, namespace)
        .unwrap()
        .with_admission_policy(policy.clone())
        .with_max_concurrent_clients(std::num::NonZeroUsize::new(4).unwrap());

    // Routed: selection events reach a peer through the route registry, which
    // the unrouted server does not have at all.
    let broker = XServerFrontendRouteBroker::new(std::num::NonZeroUsize::new(8).unwrap());
    let server = thread::spawn(move || -> Result<(), X11SetupSocketError> {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        for _ in 0..2 {
            frontend.serve_next_concurrently_routed(&broker)?;
        }
        frontend.wait_for_clients()
    });

    wait_for_socket(&socket_path);

    let mut owner = connect_x_socket(&socket_path);
    owner
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    let owner_window = read_setup_resource_id_base(&mut owner, XByteOrder::LittleEndian) + 1;
    owner
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            owner_window,
            0,
            0,
            32,
            24,
        ))
        .unwrap();
    sync_x_connection(&mut owner, XByteOrder::LittleEndian, owner_window);

    let mut stalled = connect_x_socket(&socket_path);
    stalled
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    let watched = read_setup_resource_id_base(&mut stalled, XByteOrder::LittleEndian) + 1;
    stalled
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            watched,
            0,
            0,
            32,
            24,
        ))
        .unwrap();
    sync_x_connection(&mut stalled, XByteOrder::LittleEndian, watched);
    stalled
        .write_all(&xfixes_select_selection_input_request(
            XByteOrder::LittleEndian,
            watched,
            PRIMARY,
            0b111,
        ))
        .unwrap();
    sync_x_connection(&mut stalled, XByteOrder::LittleEndian, watched);

    // From here the watcher never reads again. Bounded: enough owner changes
    // that their notifications pass what the kernel will hold for a client
    // that is not reading -- half a mebibyte against a send buffer of a few
    // hundred kibibytes -- so the rest is owed by the server, and a stop if
    // the writes start failing. What ends the watcher is then the silence
    // allowance (t165): output owed, and neither drained nor any request
    // read, for six seconds. A watcher merely a buffer's worth behind is not
    // yet a failed endpoint, and the reference server keeps it too.
    //
    // Synchronised every batch, so the assertions below cannot run against
    // requests the server has not reached yet. Without the barrier the reader
    // races the owner and can observe a queue that never had time to fill.
    for round in 0..16384u32 {
        if owner
            .write_all(&set_selection_owner_request(
                XByteOrder::LittleEndian,
                owner_window,
                PRIMARY,
                round + 1,
            ))
            .is_err()
        {
            break;
        }
        if round % 256 == 255 {
            sync_x_connection(&mut owner, XByteOrder::LittleEndian, owner_window);
        }
    }
    sync_x_connection(&mut owner, XByteOrder::LittleEndian, owner_window);

    // Silent past the allowance: it reads nothing and asks nothing while
    // output is owed to it. A watcher that resumed reading now would be
    // keeping up, and would be kept.
    std::thread::sleep(X_AUTHORITY_CLIENT_OUTPUT_SILENCE_LIMIT + Duration::from_secs(1));

    // Its connection is ended rather than left believing it is subscribed.
    // An absolute deadline, not a per-read timeout: a per-read timeout starts
    // again on every byte, so a server that dribbled data forever would keep
    // the test alive instead of failing it.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    stalled
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut drained = 0usize;
    let mut buffer = [0u8; 8192];
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "the stalled watcher was never disconnected: {drained} bytes read              and the connection is still open"
        );
        match stalled.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                drained += read;
                assert!(
                    drained < 8 << 20,
                    "the stalled watcher was never disconnected"
                );
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                break
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(error) => panic!("stalled watcher read failed: {error}"),
        }
    }

    // The owner is unaffected: it still completes a round trip.
    sync_x_connection(&mut owner, XByteOrder::LittleEndian, owner_window);

    drop(stalled);
    drop(owner);
    let outcome = server.join().expect("the frontend thread must not panic");
    assert!(
        outcome.is_ok(),
        "a stalled watcher ended the whole service: {:?}",
        outcome.err()
    );
    let _ = std::fs::remove_file(&socket_path);
}
