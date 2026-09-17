#[cfg(unix)]
#[test]
fn cross_namespace_executor_installs_property_and_notifies_requestor() {
    use std::io::Write;
    use std::net::Shutdown;
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-cross-selection-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let portal_path = socket_path.with_extension("portal.sock");
    let source = NamespaceContext::new(
        NamespaceId::from_raw(860),
        NamespaceProfile::Confined,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let target = NamespaceContext::new(
        NamespaceId::from_raw(861),
        NamespaceProfile::Confined,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let policy = Arc::new(SequencedXAdmissionPolicy {
        namespaces: [source, target],
        next_client: std::sync::atomic::AtomicU64::new(0),
        revoked: std::sync::Mutex::new(Vec::new()),
    });
    let config = XServerFrontendConfig::new_with_namespace_context(&socket_path, source)
        .unwrap()
        .with_admission_policy(policy)
        .with_max_concurrent_clients(NonZeroUsize::new(2).unwrap());
    let (executor_sender, executor_receiver) = std::sync::mpsc::sync_channel(1);
    let (request_sender, request_receiver) = std::sync::mpsc::sync_channel(1);
    let (coordinate_sender, coordinate_receiver) = std::sync::mpsc::sync_channel(1);
    let server_portal_path = portal_path.clone();
    let server = thread::spawn(move || {
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
        let mut frontend = XServerFrontend::bind(config).unwrap();
        executor_sender
            .send(frontend.clipboard_executor(&broker))
            .unwrap();
        let first_request = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
            if trace.request_stage == X11ObservedRequestStage::SelectionRequest {
                let transfer = trace
                    .result
                    .response
                    .as_ref()
                    .and_then(|response| {
                        response
                            .portal_commands
                            .iter()
                            .find_map(|command| match command {
                                XAuthorityPortalCommand::PromptClipboardTransfer(transfer) => {
                                    Some(transfer.transfer)
                                }
                                _ => None,
                            })
                        })
                    .expect("cross-namespace selection must emit a clipboard transfer");
                request_sender.send(transfer).unwrap();
                if first_request.swap(false, std::sync::atomic::Ordering::AcqRel) {
                    coordinate_sender.send(transfer).unwrap();
                }
            }
            Ok(None)
        });
        frontend
            .serve_next_concurrently_routed_traced(&broker, observer.clone())
            .unwrap();
        frontend
            .serve_next_concurrently_routed_traced(&broker, observer)
            .unwrap();
        let transfer = coordinate_receiver.recv().unwrap();
        let request = PortalBrokerRequestPacket {
            request: PortalRequest {
                transfer: PortalTransfer {
                    transfer,
                    source_namespace: source.id,
                    target_namespace: target.id,
                    kind: PortalTransferKind::Clipboard,
                    mime_type: Some("UTF8_STRING".to_owned()),
                    byte_size: 0,
                    decision: PortalDecision::Pending,
                    generation: 1,
                },
                deadline_msec: 2_000,
            },
            source_may_publish: true,
            target_may_request: true,
        };
        coordinate_x11_clipboard_transfer(
            server_portal_path,
            &request,
            &frontend.clipboard_executor(&broker),
            &broker,
            std::time::Duration::from_secs(2),
        )
        .unwrap();
        frontend.wait_for_clients().unwrap();
    });
    wait_for_socket(&socket_path);
    let executor = executor_receiver.recv().unwrap();
    let portal_executor = executor.clone();
    let portal_server_path = portal_path.clone();
    let portal_server = thread::spawn(move || {
        sophia_portal::run_portal_clipboard_broker_socket_server_bounded(
            portal_server_path,
            1,
            sophia_portal::HeadlessPortalPolicy::Allow,
            10,
            1,
            move |grant, payload| {
                portal_executor
                    .execute(grant, payload)
                    .map(|_| ())
                    .map_err(|_| ())
            },
        )
    });
    wait_for_socket(&portal_path);
    let mut owner = connect_x_socket(&socket_path);
    owner
        .set_read_timeout(Some(X_RECORD_READ_TIMEOUT))
        .unwrap();
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
            100,
            60,
        ))
        .unwrap();
    owner
        .write_all(&intern_atom_request(
            XByteOrder::LittleEndian,
            false,
            "UTF8_STRING",
        ))
        .unwrap();
    let atom_reply = read_x_record(&mut owner);
    let utf8 = read_u32(XByteOrder::LittleEndian, &atom_reply[8..12]);
    owner
        .write_all(&intern_atom_request(
            XByteOrder::LittleEndian,
            false,
            "CLIPBOARD",
        ))
        .unwrap();
    let selection = read_u32(XByteOrder::LittleEndian, &read_x_record(&mut owner)[8..12]);
    owner
        .write_all(&set_selection_owner_request(
            XByteOrder::LittleEndian,
            owner_window,
            selection,
            10,
        ))
        .unwrap();

    let mut requestor = connect_x_socket(&socket_path);
    requestor
        .set_read_timeout(Some(X_RECORD_READ_TIMEOUT))
        .unwrap();
    requestor
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    let requestor_window =
        read_setup_resource_id_base(&mut requestor, XByteOrder::LittleEndian) + 1;
    requestor
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            requestor_window,
            0,
            0,
            100,
            60,
        ))
        .unwrap();
    requestor
        .write_all(&change_window_event_mask_request(
            XByteOrder::LittleEndian,
            requestor_window,
            1 << 22,
        ))
        .unwrap();
    requestor
        .write_all(&intern_atom_request(
            XByteOrder::LittleEndian,
            false,
            "GLFW_SELECTION",
        ))
        .unwrap();
    let property =
        read_u32(XByteOrder::LittleEndian, &read_x_record(&mut requestor)[8..12]);
    requestor
        .write_all(&convert_selection_request(
            XByteOrder::LittleEndian,
            requestor_window,
            selection,
            utf8,
            property,
            11,
        ))
        .unwrap();
    request_receiver.recv().unwrap();
    let source_request = read_x_record(&mut owner);
    assert_eq!(source_request[0], 30);
    let proxy = read_u32(XByteOrder::LittleEndian, &source_request[12..16]);
    let proxy_property = read_u32(XByteOrder::LittleEndian, &source_request[24..28]);
    owner
        .write_all(&change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            proxy,
            proxy_property,
            utf8,
            8,
            b"cross namespace",
        ))
        .unwrap();
    owner
        .write_all(&send_selection_notify_request(
            XByteOrder::LittleEndian,
            proxy,
            read_u32(XByteOrder::LittleEndian, &source_request[4..8]),
            read_u32(XByteOrder::LittleEndian, &source_request[16..20]),
            read_u32(XByteOrder::LittleEndian, &source_request[20..24]),
            proxy_property,
        ))
        .unwrap();
    let notify = read_x_record(&mut requestor);
    assert_eq!(notify[0], 31);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &notify[20..24]),
        property
    );
    requestor
        .write_all(&get_property_request(
            XByteOrder::LittleEndian,
            true,
            requestor_window,
            property,
            X_PROPERTY_ANY_TYPE,
            0,
            u32::MAX,
        ))
        .unwrap();
    let reply = read_x_reply(&mut requestor, XByteOrder::LittleEndian);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &reply[8..12]),
        utf8
    );
    assert_eq!(&reply[32..47], b"cross namespace");
    let deleted = read_x_record(&mut requestor);
    assert_eq!(deleted[0], 28);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &deleted[4..8]),
        requestor_window
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &deleted[8..12]),
        property
    );
    assert_eq!(deleted[16], 1);
    requestor
        .write_all(&get_property_request(
            XByteOrder::LittleEndian,
            false,
            requestor_window,
            property,
            X_PROPERTY_ANY_TYPE,
            0,
            u32::MAX,
        ))
        .unwrap();
    let missing = read_x_reply(&mut requestor, XByteOrder::LittleEndian);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &missing[8..12]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &missing[16..20]), 0);
    portal_server.join().unwrap().unwrap();

    requestor
        .write_all(&convert_selection_request(
            XByteOrder::LittleEndian,
            requestor_window,
            selection,
            utf8,
            property,
            12,
        ))
        .unwrap();
    let stale_transfer = request_receiver.recv().unwrap();
    owner
        .write_all(&set_selection_owner_request(
            XByteOrder::LittleEndian,
            owner_window,
            selection,
            12,
        ))
        .unwrap();
    owner
        .write_all(&resource_request(XByteOrder::LittleEndian, 23, selection))
        .unwrap();
    assert_eq!(read_x_record(&mut owner)[0], 1);
    assert!(
        executor
            .request_source(&PortalGrant {
                transfer: stale_transfer,
                source_namespace: source.id,
                target_namespace: target.id,
                kind: PortalTransferKind::Clipboard,
                source_generation: 1,
                broker_generation: 1,
                deadline_msec: 2_000,
                state: PortalGrantState::Active,
            })
            .is_err()
    );
    executor
        .fail(
            stale_transfer,
            ClipboardSelectionExecutionError::StaleOwnerGeneration,
        )
        .unwrap();
    let notify = read_x_record(&mut requestor);
    assert_eq!(notify[0], 31);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &notify[20..24]), 0);

    for failure in [
        ClipboardSelectionExecutionError::Denied,
        ClipboardSelectionExecutionError::Expired,
        ClipboardSelectionExecutionError::Disconnected,
        ClipboardSelectionExecutionError::ExecutorFailure,
    ] {
        requestor
            .write_all(&convert_selection_request(
                XByteOrder::LittleEndian,
                requestor_window,
                selection,
                utf8,
                property,
                12,
            ))
            .unwrap();
        let transfer = request_receiver.recv().unwrap();
        let outcome = executor.fail(transfer, failure).unwrap();
        assert!(matches!(
            outcome,
            ClipboardSelectionExecutionOutcome::Failed { error, .. } if error == failure
        ));
        let notify = read_x_record(&mut requestor);
        assert_eq!(notify[0], 31);
        assert_eq!(read_u32(XByteOrder::LittleEndian, &notify[20..24]), 0);
    }
    owner.shutdown(Shutdown::Both).unwrap();
    requestor.shutdown(Shutdown::Both).unwrap();
    server.join().unwrap();
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_assigns_distinct_connection_identities() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-frontend-client-id-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new(&socket_path, NamespaceId::from_raw(818)).unwrap();
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let mut clients = Vec::new();
        for _ in 0..2 {
            frontend
                .serve_next_traced(|trace| {
                    clients.push((trace.client.raw(), trace.resource_id_range));
                    Ok(())
                })
                .unwrap();
        }
        (clients, frontend.active_client_count())
    });

    wait_for_socket(&socket_path);
    for name in ["FIRST_CLIENT", "SECOND_CLIENT"] {
        let mut stream = connect_x_socket(&socket_path);
        stream
            .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
            .unwrap();
        read_setup_success(&mut stream, XByteOrder::LittleEndian);
        stream
            .write_all(&intern_atom_request(XByteOrder::LittleEndian, false, name))
            .unwrap();
        let reply = read_x_record(&mut stream);
        assert_eq!(reply[0], 1);
        drop(stream);
    }

    assert_eq!(
        server.join().unwrap(),
        (
            vec![
                (
                    1,
                    XWireClientResourceRange {
                        base: 0x0020_0000,
                        mask: X_SETUP_DEFAULT_RESOURCE_ID_MASK,
                    },
                ),
                (
                    2,
                    XWireClientResourceRange {
                        base: 0x0040_0000,
                        mask: X_SETUP_DEFAULT_RESOURCE_ID_MASK,
                    },
                ),
            ],
            0,
        )
    );
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_dispatches_two_live_clients_with_shared_x_state() {
    use std::{
        io::Write,
        num::NonZeroUsize,
        sync::{Arc, Condvar, Mutex},
        thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-frontend-concurrent-clients-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new(&socket_path, NamespaceId::from_raw(820))
        .unwrap()
        .with_max_concurrent_clients(NonZeroUsize::new(2).unwrap());
    let observations = Arc::new(Mutex::new(Vec::new()));
    // WHAT THE SECOND CLIENT ACTUALLY DEPENDS ON. Its requests are about the
    // first client's window, and the trace this test compares is the order
    // observations were APPENDED in. Writing first orders neither: two
    // connections are served by two workers, so a request written earlier can
    // be dispatched later, and one dispatched earlier can be appended later
    // still.
    //
    // So the fixture waits for the thing it needs -- the first client's
    // CreateWindow having been observed -- and this is the observer saying so.
    let first_window_observed = Arc::new((Mutex::new(false), Condvar::new()));
    let server_observations = observations.clone();
    let server_signal = first_window_observed.clone();
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
            let observation = (trace.client.raw(), trace.major_opcode);
            server_observations.lock().unwrap().push(observation);
            if observation == (1, 1) {
                let (seen, announced) = &*server_signal;
                *seen.lock().unwrap() = true;
                announced.notify_all();
            }
            Ok(None)
        });
        frontend
            .serve_next_concurrently_traced(observer.clone())
            .unwrap();
        frontend.serve_next_concurrently_traced(observer).unwrap();
        assert_eq!(
            frontend.serve_next_concurrently().unwrap_err().to_string(),
            "Sophia X Server Frontend concurrent-client limit (2) reached"
        );
        frontend.wait_for_clients().unwrap();
        frontend.active_client_count()
    });

    wait_for_socket(&socket_path);
    let mut first = connect_x_socket(&socket_path);
    first
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    assert_eq!(
        read_setup_resource_id_base(&mut first, XByteOrder::LittleEndian),
        X_SETUP_DEFAULT_RESOURCE_ID_BASE
    );
    let first_window = 0x0020_0001;
    first
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            first_window,
            0,
            0,
            160,
            90,
        ))
        .unwrap();

    // BOUNDED, AND NOT A SLEEP. If this never arrives the test goes on to
    // release both clients and collect the server, so the failure below is an
    // assertion rather than a hang.
    let created = {
        let (seen, announced) = &*first_window_observed;
        let mut held = seen.lock().unwrap();
        let deadline = Duration::from_secs(5);
        let started = std::time::Instant::now();
        while !*held && started.elapsed() < deadline {
            let (next, timeout) = announced
                .wait_timeout(held, deadline.saturating_sub(started.elapsed()))
                .unwrap();
            held = next;
            if timeout.timed_out() {
                break;
            }
        }
        *held
    };

    let mut second = connect_x_socket(&socket_path);
    second
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    assert_eq!(
        read_setup_resource_id_base(&mut second, XByteOrder::LittleEndian),
        0x0040_0000
    );
    second
        .write_all(&resource_request(XByteOrder::LittleEndian, 8, first_window))
        .unwrap();
    second
        .write_all(&resource_request(XByteOrder::LittleEndian, 3, first_window))
        .unwrap();
    let attributes = read_x_reply(&mut second, XByteOrder::LittleEndian);
    expect_x_reply(&attributes, XByteOrder::LittleEndian);
    assert_eq!(attributes[26], 2);

    drop(first);
    drop(second);

    assert_eq!(server.join().unwrap(), 0);
    assert!(
        created,
        "the first client's CreateWindow was never observed, so the order \
         below was never this fixture's to assert"
    );
    assert_eq!(
        observations.lock().unwrap().as_slice(),
        &[(1, 1), (2, 8), (2, 3), (1, 0)]
    );
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_emits_surface_removal_when_a_client_disconnects() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-frontend-disconnect-cleanup-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new(&socket_path, NamespaceId::from_raw(819)).unwrap();
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let mut removals = Vec::new();
        frontend
            .serve_next_traced(|trace| {
                if trace.request_stage == X11ObservedRequestStage::DisconnectCleanup {
                    removals.push((
                        trace.client.raw(),
                        trace
                            .result
                            .response
                            .as_ref()
                            .unwrap()
                            .removed_surfaces
                            .clone(),
                    ));
                }
                Ok(())
            })
            .unwrap();
        (removals, frontend.active_client_count())
    });

    wait_for_socket(&socket_path);
    let mut stream = connect_x_socket(&socket_path);
    stream
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut stream, XByteOrder::LittleEndian);
    stream
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            0x0020_0001,
            0,
            0,
            160,
            90,
        ))
        .unwrap();
    drop(stream);

    assert_eq!(
        server.join().unwrap(),
        (vec![(1, vec![SurfaceId::new(0x0020_0001, 1)])], 0)
    );
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x_server_frontend_rejects_create_window_outside_client_resource_range() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-server-frontend-resource-owner-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config = XServerFrontendConfig::new(&socket_path, NamespaceId::from_raw(817)).unwrap();
    let server = thread::spawn(move || {
        let mut frontend = XServerFrontend::bind(config).unwrap();
        frontend.serve_next().unwrap();
    });

    wait_for_socket(&socket_path);
    let mut stream = connect_x_socket(&socket_path);
    stream
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    assert_eq!(
        read_setup_resource_id_base(&mut stream, XByteOrder::LittleEndian),
        X_SETUP_DEFAULT_RESOURCE_ID_BASE
    );
    stream
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            0x0040_0001,
            1,
            2,
            300,
            200,
        ))
        .unwrap();
    let error = read_x_record(&mut stream);
    assert_eq!(error[0], 0);
    assert_eq!(error[1], 14);

    drop(stream);
    server.join().unwrap();
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn x11_setup_socket_smoke_completes_handshake() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-setup-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = thread::spawn(move || run_x11_setup_socket_server_once(&server_path).unwrap());

    wait_for_socket(&socket_path);
    let mut stream = connect_x_socket(&socket_path);
    stream
        .write_all(&setup_request(
            XByteOrder::LittleEndian,
            11,
            0,
            b"MIT-MAGIC-COOKIE-1",
            b"0123456789abcdef",
        ))
        .unwrap();

    let mut prefix = [0; X_SETUP_REPLY_PREFIX_LEN];
    fill_from_socket(&mut stream, &mut prefix);
    assert_eq!(prefix[0], 1);
    let body_len = usize::from(read_u16(XByteOrder::LittleEndian, &prefix[6..8])) * 4;
    let mut body = vec![0; body_len];
    fill_from_socket(&mut stream, &mut body);

    assert_eq!(read_u32(XByteOrder::LittleEndian, &body[4..8]), 0x0020_0000);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &body[8..12]),
        0x001f_ffff
    );
    let _ = std::fs::remove_file(&socket_path);
    server.join().unwrap();
}

#[cfg(unix)]
#[test]
fn x11_core_socket_smoke_round_trips_atom_property_and_window_events() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-core-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = thread::spawn(move || {
        run_x11_core_socket_server_once(&server_path, NamespaceId::from_raw(47)).unwrap();
    });

    wait_for_socket(&socket_path);
    let mut stream = connect_x_socket(&socket_path);
    stream
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut stream, XByteOrder::LittleEndian);

    stream
        .write_all(&intern_atom_request(
            XByteOrder::LittleEndian,
            false,
            X_ATOM_NAME_NET_WM_NAME,
        ))
        .unwrap();
    let intern = read_x_record(&mut stream);
    assert_eq!(intern[0], 1);
    let net_wm_name = read_u32(XByteOrder::LittleEndian, &intern[8..12]);
    assert_ne!(net_wm_name, 0);

    stream
        .write_all(&intern_atom_request(
            XByteOrder::LittleEndian,
            false,
            X_ATOM_NAME_UTF8_STRING,
        ))
        .unwrap();
    let intern = read_x_record(&mut stream);
    assert_eq!(intern[0], 1);
    let utf8 = read_u32(XByteOrder::LittleEndian, &intern[8..12]);
    assert_ne!(utf8, 0);

    stream
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            0x220201,
            1,
            2,
            300,
            200,
        ))
        .unwrap();
    stream
        .write_all(&change_window_event_mask_request(
            XByteOrder::LittleEndian,
            0x220201,
            (1 << 15) | (1 << 16) | (1 << 17),
        ))
        .unwrap();

    stream
        .write_all(&resource_request(XByteOrder::LittleEndian, 8, 0x220201))
        .unwrap();
    let map = read_x_record(&mut stream);
    assert_eq!(map[0], 19);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &map[8..12]), 0x220201);
    let visibility = read_x_record(&mut stream);
    assert_eq!(visibility[0], 15);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &visibility[4..8]),
        0x220201
    );
    let expose = read_x_record(&mut stream);
    assert_eq!(expose[0], 12);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &expose[4..8]), 0x220201);

    stream
        .write_all(&change_property_request(
            XByteOrder::LittleEndian,
            XPropertyMode::Replace,
            0x220201,
            net_wm_name,
            utf8,
            8,
            b"Sophia Socket",
        ))
        .unwrap();
    let property = read_x_record(&mut stream);
    assert_eq!(property[0], 28);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &property[8..12]),
        net_wm_name
    );

    stream
        .write_all(&get_property_request(
            XByteOrder::LittleEndian,
            false,
            0x220201,
            net_wm_name,
            X_PROPERTY_ANY_TYPE,
            0,
            64,
        ))
        .unwrap();
    let property = read_x_reply(&mut stream, XByteOrder::LittleEndian);
    expect_x_reply(&property, XByteOrder::LittleEndian);
    assert_eq!(property[1], 8);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &property[8..12]), utf8);
    assert_eq!(&property[32..45], b"Sophia Socket");

    drop(stream);
    let _ = std::fs::remove_file(&socket_path);
    server.join().unwrap();
}
