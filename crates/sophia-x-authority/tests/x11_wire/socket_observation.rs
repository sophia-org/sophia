#[cfg(unix)]
#[test]
fn x11_core_listener_reclaims_disconnected_client_window_before_next_client() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-persistent-core-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = thread::spawn(move || {
        let listener = bind_x11_core_socket_server(&server_path).unwrap();
        let state = X11CoreSocketServerState::new();
        serve_x11_core_socket_listener_once(&listener, NamespaceId::from_raw(52), &state)
            .unwrap();
        serve_x11_core_socket_listener_once(&listener, NamespaceId::from_raw(52), &state)
            .unwrap();
    });

    wait_for_socket(&socket_path);
    let mut first = connect_x_socket(&socket_path);
    first
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut first, XByteOrder::LittleEndian);
    first
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            0x220701,
            1,
            2,
            300,
            200,
        ))
        .unwrap();
    drop(first);

    let mut second = connect_x_socket(&socket_path);
    second
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut second, XByteOrder::LittleEndian);
    second
        .write_all(&resource_request(XByteOrder::LittleEndian, 8, 0x220701))
        .unwrap();
    let error = read_x_record(&mut second);
    assert_eq!(error[0], 0);
    assert_eq!(error[1], 3, "the released window must be BadWindow");
    assert_eq!(read_u32(XByteOrder::LittleEndian, &error[4..8]), 0x220701);

    drop(second);
    server.join().unwrap();
    let _ = std::fs::remove_file(&socket_path);
}

#[cfg(unix)]
#[test]
fn x11_core_socket_recreated_xid_receives_a_fresh_surface_generation() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-surface-generation-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = thread::spawn(move || {
        let mut created = Vec::new();
        let mut removed = Vec::new();
        run_x11_core_socket_server_once_observed(
            &server_path,
            NamespaceId::from_raw(53),
            |result| {
                if let Some(response) = &result.response {
                    created.extend(response.surfaces.iter().map(|surface| surface.surface));
                    removed.extend(response.removed_surfaces.iter().copied());
                }
            },
        )
        .unwrap();
        (created, removed)
    });

    wait_for_socket(&socket_path);
    let mut stream = connect_x_socket(&socket_path);
    stream
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut stream, XByteOrder::LittleEndian);
    let xid = 0x220711;
    stream
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            xid,
            1,
            2,
            300,
            200,
        ))
        .unwrap();
    // A rejected duplicate probes generation two but must not consume it.
    stream
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            xid,
            9,
            9,
            1,
            1,
        ))
        .unwrap();
    let duplicate_error = read_x_record(&mut stream);
    assert_eq!(duplicate_error[0], 0);
    assert_eq!(duplicate_error[1], 14, "duplicate XID must be BadIdChoice");
    stream
        .write_all(&resource_request(XByteOrder::LittleEndian, 4, xid))
        .unwrap();
    stream
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            xid,
            3,
            4,
            320,
            240,
        ))
        .unwrap();

    // The authority reads one request at a time, so a successful write says
    // nothing about whether the replacement create has been dispatched yet.
    // Dropping here raced that: the assertions below describe work the server
    // may not have reached. A round trip is the only barrier, and GetGeometry
    // is the cheapest one that names the window -- its reply cannot arrive
    // before the create queued ahead of it was handled. Real clients get this
    // for free because XCloseDisplay performs a final XSync.
    stream
        .write_all(&resource_request(XByteOrder::LittleEndian, 14, xid))
        .unwrap();
    // Skip any events that arrive ahead of the reply. A record's first byte is
    // 0 for an error, 1 for a reply, and anything higher for an event.
    let geometry = loop {
        let record = read_x_record(&mut stream);
        if record[0] >= 2 {
            continue;
        }
        break record;
    };
    assert_eq!(geometry[0], 1, "expected a GetGeometry reply");
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &geometry[16..18]),
        320,
        "the replacement window answers with its own geometry"
    );
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &geometry[18..20]),
        240,
        "the replacement window answers with its own geometry"
    );

    drop(stream);
    let _ = std::fs::remove_file(&socket_path);
    let (created, removed) = server.join().unwrap();
    assert_eq!(
        created,
        vec![SurfaceId::new(xid, 1), SurfaceId::new(xid, 2)]
    );
    assert_eq!(
        removed,
        vec![SurfaceId::new(xid, 1), SurfaceId::new(xid, 2)],
        "DestroyWindow retires generation 1; disconnect cleanup retires the live replacement"
    );
}

#[cfg(unix)]
#[test]
fn x11_core_socket_observer_sees_poly_fill_rectangle_transaction() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-core-draw-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = thread::spawn(move || {
        let mut transactions = 0usize;
        run_x11_core_socket_server_once_observed(
            &server_path,
            NamespaceId::from_raw(48),
            |result| {
                if let Some(response) = &result.response {
                    transactions = transactions.saturating_add(response.transactions.len());
                }
            },
        )
        .unwrap();
        transactions
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
            0x220301,
            1,
            2,
            300,
            200,
        ))
        .unwrap();

    stream
        .write_all(&create_gc_request(
            XByteOrder::LittleEndian,
            0x220302,
            0x220301,
        ))
        .unwrap();
    stream
        .write_all(&poly_fill_rectangle_request(
            XByteOrder::LittleEndian,
            0x220301,
            0x220302,
            &[(5, 6, 40, 30)],
        ))
        .unwrap();

    drop(stream);
    let _ = std::fs::remove_file(&socket_path);
    assert_eq!(server.join().unwrap(), 1);
}

#[cfg(unix)]
#[test]
fn x11_core_socket_observer_sees_put_image_transaction() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-put-image-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = thread::spawn(move || {
        let mut transactions = 0usize;
        run_x11_core_socket_server_once_observed(
            &server_path,
            NamespaceId::from_raw(49),
            |result| {
                if let Some(response) = &result.response {
                    transactions = transactions.saturating_add(response.transactions.len());
                }
            },
        )
        .unwrap();
        transactions
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
            0x220401,
            1,
            2,
            300,
            200,
        ))
        .unwrap();

    stream
        .write_all(&create_gc_request(
            XByteOrder::LittleEndian,
            0x220402,
            0x220401,
        ))
        .unwrap();
    stream
        .write_all(&put_image_request(
            XByteOrder::LittleEndian,
            0x220401,
            0x220402,
            PutImageGeometry {
                width: 8,
                height: 4,
                dst_x: 3,
                dst_y: 5,
            },
            &[0xaa; 128],
        ))
        .unwrap();

    drop(stream);
    let _ = std::fs::remove_file(&socket_path);
    assert_eq!(server.join().unwrap(), 1);
}

#[cfg(unix)]
#[test]
fn x11_core_socket_returns_large_get_image_reply() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-get-image-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = thread::spawn(move || {
        run_x11_core_socket_server_once_observed(
            &server_path,
            NamespaceId::from_raw(54),
            |_| {},
        )
        .unwrap();
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
            0x220601,
            1,
            2,
            400,
            200,
        ))
        .unwrap();
    stream
        .write_all(&resource_request(
            XByteOrder::LittleEndian,
            8,
            0x220601,
        ))
        .unwrap();
    stream
        .write_all(&create_gc_request(
            XByteOrder::LittleEndian,
            0x220602,
            0x220601,
        ))
        .unwrap();
    stream
        .write_all(&put_image_request(
            XByteOrder::LittleEndian,
            0x220601,
            0x220602,
            PutImageGeometry {
                width: 1,
                height: 1,
                dst_x: 3,
                dst_y: 5,
            },
            &[0x44, 0x33, 0x22, 0x11],
        ))
        .unwrap();
    stream
        .write_all(&get_image_request(
            XByteOrder::LittleEndian,
            2,
            0x220601,
            0,
            0,
            400,
            200,
            u32::MAX,
        ))
        .unwrap();

    let reply = read_x_reply(&mut stream, XByteOrder::LittleEndian);
    assert_eq!(reply.len(), 32 + 400 * 200 * 4);
    assert_eq!(reply[0], 1);
    assert_eq!(reply[1], 24);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &reply[4..8]), 80_000);
    let marker = 32 + ((5 * 400 + 3) * 4);
    assert_eq!(&reply[marker..marker + 4], &[0x44, 0x33, 0x22, 0]);

    drop(stream);
    server.join().unwrap();
    let _ = std::fs::remove_file(&socket_path);
}

#[cfg(unix)]
#[test]
fn x11_core_socket_observer_sees_sophia_present_transaction() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-present-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = thread::spawn(move || {
        let mut transactions = 0usize;
        run_x11_core_socket_server_once_observed(
            &server_path,
            NamespaceId::from_raw(50),
            |result| {
                if let Some(response) = &result.response {
                    transactions = transactions.saturating_add(response.transactions.len());
                }
            },
        )
        .unwrap();
        transactions
    });

    wait_for_socket(&socket_path);
    let mut stream = connect_x_socket(&socket_path);
    stream
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut stream, XByteOrder::LittleEndian);

    stream
        .write_all(&query_extension_request(
            XByteOrder::LittleEndian,
            X_SOPHIA_PRESENT_EXTENSION_NAME,
        ))
        .unwrap();
    let query = read_x_record(&mut stream);
    assert_eq!(query[8], 1);
    assert_eq!(query[9], X_SOPHIA_PRESENT_MAJOR_OPCODE);

    stream
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            0x220501,
            1,
            2,
            300,
            200,
        ))
        .unwrap();

    stream
        .write_all(&sophia_present_pixmap_request(
            XByteOrder::LittleEndian,
            0x220501,
            0x990,
            (3, 5, 32, 24),
            1,
            250,
        ))
        .unwrap();

    drop(stream);
    let _ = std::fs::remove_file(&socket_path);
    assert_eq!(server.join().unwrap(), 1);
}

#[cfg(unix)]
#[test]
fn x11_core_socket_channel_sees_sophia_present_transaction_batch() {
    use std::io::Write;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-present-channel-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let (sender, receiver) =
        std::sync::mpsc::sync_channel(X_AUTHORITY_OBSERVED_TRANSACTION_CHANNEL_CAPACITY);
    let server = thread::spawn(move || {
        run_x11_core_socket_server_once_channel(&server_path, NamespaceId::from_raw(51), sender)
            .unwrap();
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
            0x220601,
            1,
            2,
            300,
            200,
        ))
        .unwrap();

    stream
        .write_all(&sophia_present_pixmap_request(
            XByteOrder::LittleEndian,
            0x220601,
            0x991,
            (3, 5, 32, 24),
            1,
            250,
        ))
        .unwrap();

    drop(stream);
    let _ = std::fs::remove_file(&socket_path);
    server.join().unwrap();
    let batch = std::iter::from_fn(|| receiver.try_recv().ok())
        .find(|batch| !batch.transactions.is_empty())
        .expect("present transaction batch");
    assert_eq!(batch.client.map(XServerFrontendClientId::raw), Some(1));
    assert_eq!(batch.transaction, TransactionId::from_raw(2));
    assert_eq!(batch.transactions.len(), 1);
    assert_eq!(
        batch.transactions[0].transaction,
        TransactionId::from_raw(2)
    );
    let surface = batch.transactions[0].surface;
    let mut routes = XAuthorityClientSurfaceRoutes::default();
    assert!(batch.surface_routes.is_empty());
    routes.observe(&batch).unwrap();
    assert!(routes.is_empty());
    routes.observe(&XAuthorityObservedTransactionBatch {
        client: None,
        admission: None,
        surface_routes: Vec::new(),
        transaction: TransactionId::from_raw(3),
        transactions: Vec::new(),
        surface_presentations: Vec::new(),
        presentation_intents: Vec::new(),
        removed_surfaces: vec![surface],
        surface_output_reservations: Vec::new(),
        cpu_buffer_updates: Vec::new(),
        raster_responses: Vec::new(),
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
        protocol_errors: Vec::new(),
        expected_protocol_errors: Vec::new(),
        metadata: Vec::new(),
        selection_owner_change: false,
        selection_conversion: false,
    })
    .unwrap();
    assert!(routes.is_empty());
}

#[cfg(unix)]
#[test]
fn routed_service_confines_input_and_control_to_two_workers_and_drains() {
    use std::io::{Read, Write};
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-routed-worker-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let (transaction_sender, transaction_receiver) =
        std::sync::mpsc::sync_channel(X_AUTHORITY_OBSERVED_TRANSACTION_CHANNEL_CAPACITY);
    let (acknowledgement_sender, acknowledgement_receiver) = std::sync::mpsc::sync_channel(4);
    let broker = XServerFrontendRouteBroker::with_control_ack_sender(
        NonZeroUsize::new(4).unwrap(),
        acknowledgement_sender,
    );
    let input_sender = broker.routed_input_sender();
    let control_sender = broker.control_sender();
    let (service_command_sender, service_command_receiver) = std::sync::mpsc::sync_channel(1);
    let first_namespace = NamespaceContext::new(
        NamespaceId::from_raw(852),
        NamespaceProfile::Confined,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let second_namespace = NamespaceContext::new(
        NamespaceId::from_raw(853),
        NamespaceProfile::Confined,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let policy = Arc::new(SequencedXAdmissionPolicy {
        namespaces: [first_namespace, second_namespace],
        next_client: std::sync::atomic::AtomicU64::new(0),
        revoked: std::sync::Mutex::new(Vec::new()),
    });
    let config = XServerFrontendConfig::new_with_namespace_context(&server_path, first_namespace)
        .unwrap()
        .with_admission_policy(policy.clone())
        .with_max_concurrent_clients(NonZeroUsize::new(2).unwrap());
    let server = thread::spawn(move || {
        run_x_server_frontend_routed_until_stopped(
            config,
            transaction_sender,
            broker,
            service_command_receiver,
        )
        .unwrap();
    });

    wait_for_socket(&socket_path);
    let mut first = connect_x_socket(&socket_path);
    first
        .set_read_timeout(Some(X_RECORD_READ_TIMEOUT))
        .unwrap();
    first
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut first, XByteOrder::LittleEndian);
    first
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            0x0020_0701,
            1,
            2,
            300,
            200,
        ))
        .unwrap();
    first
        .write_all(&change_window_event_mask_request(
            XByteOrder::LittleEndian,
            0x0020_0701,
            0b11 | (1 << 15) | (1 << 17),
        ))
        .unwrap();
    first
        .write_all(&present_select_input_request(
            XByteOrder::LittleEndian,
            0x0020_0711,
            0x0020_0701,
            1,
        ))
        .unwrap();
    first
        .write_all(&sophia_present_pixmap_request(
            XByteOrder::LittleEndian,
            0x0020_0701,
            0x992,
            (0, 0, 16, 16),
            1,
            1,
        ))
        .unwrap();

    // The window and its event mask exist only once this connection's writes are
    // processed. Without the barrier `second` can arrive first and be told
    // BadWindow, which is a correct answer to a different question than the one
    // this test is asking.
    sync_x_connection(&mut first, XByteOrder::LittleEndian, 0x0020_0701);
    let mut second = connect_x_socket(&socket_path);
    second
        .set_read_timeout(Some(X_RECORD_READ_TIMEOUT))
        .unwrap();
    second
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut second, XByteOrder::LittleEndian);
    second
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            0x0040_0702,
            3,
            4,
            300,
            200,
        ))
        .unwrap();
    second
        .write_all(&change_window_event_mask_request(
            XByteOrder::LittleEndian,
            0x0020_0701,
            0b11,
        ))
        .unwrap();
    let mut error = [0; 32];
    fill_from_socket(&mut second, &mut error);
    assert_eq!(error[0], 0);
    assert_eq!(error[1], XErrorCode::BadAccess.wire_code());
    second
        .write_all(&change_window_event_mask_request(
            XByteOrder::LittleEndian,
            0x0040_0702,
            0b11 | (1 << 15) | (1 << 17),
        ))
        .unwrap();
    second
        .write_all(&sophia_present_pixmap_request(
            XByteOrder::LittleEndian,
            0x0040_0702,
            0x993,
            (0, 0, 16, 16),
            1,
            1,
        ))
        .unwrap();
    second
        .write_all(&present_select_input_request(
            XByteOrder::LittleEndian,
            0x0040_0712,
            0x0040_0702,
            1,
        ))
        .unwrap();

    let mut routes = Vec::new();
    let mut observed_protocol_error = false;
    while routes.len() < 2 || !observed_protocol_error {
        let batch = transaction_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        if batch.transactions.is_empty() && !batch.protocol_errors.is_empty() {
            assert_eq!(batch.protocol_errors.len(), 1);
            assert_eq!(
                batch.protocol_errors[0].code,
                XErrorCode::BadAccess.wire_code()
            );
            observed_protocol_error = true;
            continue;
        }
        if batch.transactions.is_empty() {
            continue;
        }
        if routes.len() < 2 {
            routes.push((
                batch
                    .client
                    .expect("routed worker must identify its client"),
                batch.transactions[0].surface,
            ));
        }
    }
    assert!(observed_protocol_error);
    routes.sort_by_key(|(client, _)| client.raw());
    assert_ne!(routes[0].0, routes[1].0);
    for (index, (_, surface)) in routes.iter().copied().enumerate() {
        input_sender
            .send(XAuthorityRoutedInput {
                request: RoutedInputRequest {
                    serial: 20 + index as u64,
                    seat: SeatId::from_raw(1),
                    device: DeviceId::from_raw(1),
                    time_msec: 10 + index as u64,
                    target_surface: surface,
                    global_position: Point::default(),
                    local_position: Point::default(),
                    kind: InputEventKind::Key {
                        keycode: 30 + index as u32,
                        pressed: true,
                    },
                },
                route_lease: None,
                delivery: None,
                mode: XAuthorityRoutedInputMode::Deliver,
                origin: XAuthorityRoutedInputOrigin::Physical,
            })
            .unwrap();
    }
    let first_key = read_x_record(&mut first);
    assert_eq!(first_key[0], 2);
    assert_eq!(first_key[1], 38);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &first_key[12..16]),
        0x0020_0701
    );
    input_sender
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 30,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 50,
                target_surface: routes[0].1,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::Key {
                    keycode: 30,
                    pressed: true,
                },
            },
            route_lease: None,
            delivery: None,
            mode: XAuthorityRoutedInputMode::Repeat,
            origin: XAuthorityRoutedInputOrigin::Physical,
        })
        .unwrap();
    let first_repeat = read_x_record(&mut first);
    assert_eq!(first_repeat[0], 2);
    assert_eq!(first_repeat[1], 38);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &first_repeat[12..16]),
        0x0020_0701
    );
    let second_key = read_x_record(&mut second);
    assert_eq!(second_key[0], 2);
    assert_eq!(second_key[1], 39);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &second_key[12..16]),
        0x0040_0702
    );
    for (index, (client, surface)) in routes.iter().copied().enumerate() {
        control_sender
            .send(XAuthorityClientControlCommand {
                client,
                command: XAuthorityControlCommand::ConfigureSurface {
                    transaction: TransactionId::from_raw(88 + index as u64),
                    surface,
                    geometry: Rect {
                        x: 41 + index as i32,
                        y: 51 + index as i32,
                        width: 301 + index as i32,
                        height: 201 + index as i32,
                    },
                },
            })
            .unwrap();
    }
    let mut acknowledgements = Vec::new();
    for _ in 0..2 {
        acknowledgements.push(
            acknowledgement_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
        );
    }
    for (index, (client, surface)) in routes.iter().copied().enumerate() {
        assert!(acknowledgements.contains(&XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::ConfigureSurface,
                transaction: TransactionId::from_raw(88 + index as u64),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        }));
    }
    let first_present = read_x_reply(&mut first, XByteOrder::LittleEndian);
    assert_eq!(first_present[0], 35);
    assert_eq!(first_present[1], X_PRESENT_MAJOR_OPCODE);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &first_present[8..10]), 0);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &first_present[12..16]),
        0x0020_0711
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &first_present[16..20]),
        0x0020_0701
    );
    assert_eq!(read_i16(XByteOrder::LittleEndian, &first_present[20..22]), 41);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &first_present[22..24]), 51);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &first_present[24..26]), 301);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &first_present[26..28]), 201);
    assert_eq!(read_x_record(&mut first)[0], 22);
    let second_present = read_x_reply(&mut second, XByteOrder::LittleEndian);
    assert_eq!(second_present[0], 35);
    assert_eq!(second_present[1], X_PRESENT_MAJOR_OPCODE);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &second_present[12..16]),
        0x0040_0712
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &second_present[16..20]),
        0x0040_0702
    );
    assert_eq!(read_i16(XByteOrder::LittleEndian, &second_present[20..22]), 42);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &second_present[22..24]), 52);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &second_present[24..26]), 302);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &second_present[26..28]), 202);
    assert_eq!(read_x_record(&mut second)[0], 22);

    let (first_client, first_surface) = routes[0];
    let moved = Rect {
        x: 71,
        y: 81,
        width: 301,
        height: 201,
    };
    control_sender
        .send(XAuthorityClientControlCommand {
            client: first_client,
            command: XAuthorityControlCommand::ConfigureSurface {
                transaction: TransactionId::from_raw(100),
                surface: first_surface,
                geometry: moved,
            },
        })
        .unwrap();
    assert_eq!(
        acknowledgement_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered
    );
    let move_present = read_x_reply(&mut first, XByteOrder::LittleEndian);
    assert_eq!(move_present[0], 35, "Present must precede core configure");
    assert_eq!(read_i16(XByteOrder::LittleEndian, &move_present[20..22]), 71);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &move_present[22..24]), 81);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &move_present[24..26]), 301);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &move_present[26..28]), 201);
    let move_core = read_x_record(&mut first);
    assert_eq!(move_core[0], 22);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &move_core[16..18]), 71);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &move_core[18..20]), 81);

    control_sender
        .send(XAuthorityClientControlCommand {
            client: first_client,
            command: XAuthorityControlCommand::ConfigureSurface {
                transaction: TransactionId::from_raw(101),
                surface: first_surface,
                geometry: moved,
            },
        })
        .unwrap();
    assert_eq!(
        acknowledgement_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered
    );
    first
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut unexpected = [0_u8; 1];
    let error = first.read(&mut unexpected).unwrap_err();
    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));

    drop(first);
    drop(second);
    service_command_sender
        .send(XServerFrontendServiceCommand::StopAccepting)
        .unwrap();
    drop(service_command_sender);
    drop(input_sender);
    drop(control_sender);
    server.join().unwrap();
    let revoked = policy.revoked.lock().unwrap();
    assert_eq!(revoked.len(), 2);
    assert_ne!(revoked[0].namespace.id, revoked[1].namespace.id);
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn configured_present_child_receives_xlibre_ordered_geometry_notification() {
    use std::io::{Read, Write};
    use std::num::NonZeroUsize;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    // The full fixture matrix can briefly starve this worker. Keep I/O bounded
    // without mistaking scheduler delay for a protocol-ordering failure.
    const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(10);
    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-present-child-configure-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let (transaction_sender, _transaction_receiver) =
        std::sync::mpsc::sync_channel(X_AUTHORITY_OBSERVED_TRANSACTION_CHANNEL_CAPACITY);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let (service_sender, service_receiver) = std::sync::mpsc::sync_channel(1);
    let config = XServerFrontendConfig::new(&server_path, NamespaceId::from_raw(855)).unwrap();
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
    let mut stream = connect_x_socket(&socket_path);
    stream.set_read_timeout(Some(SOCKET_IO_TIMEOUT)).unwrap();
    stream
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut stream, XByteOrder::LittleEndian);
    let parent = 0x0020_0901;
    let child = 0x0020_0902;
    let event_id = 0x0020_0910;
    stream
        .write_all(&create_window_request(
            XByteOrder::LittleEndian,
            parent,
            0,
            0,
            1280,
            1040,
        ))
        .unwrap();
    stream
        .write_all(&create_window_request_with_parent(
            XByteOrder::LittleEndian,
            child,
            parent,
            0,
            0,
            1280,
            1040,
        ))
        .unwrap();
    stream
        .write_all(&change_window_event_mask_request(
            XByteOrder::LittleEndian,
            child,
            1 << 17,
        ))
        .unwrap();
    stream
        .write_all(&present_select_input_request(
            XByteOrder::LittleEndian,
            event_id,
            child,
            1,
        ))
        .unwrap();
    // The child exists only once this connection's writes have been processed.
    // Without the barrier the peer below can reach the authority first and be
    // told, correctly, that the window does not exist.
    sync_x_connection(&mut stream, XByteOrder::LittleEndian, child);
    let mut peer = connect_x_socket(&socket_path);
    peer.set_read_timeout(Some(SOCKET_IO_TIMEOUT)).unwrap();
    peer.write_all(&setup_request(
        XByteOrder::LittleEndian,
        11,
        0,
        b"",
        b"",
    ))
    .unwrap();
    read_setup_success(&mut peer, XByteOrder::LittleEndian);
    let peer_event_id = 0x0040_0910;
    peer.write_all(&present_select_input_request(
        XByteOrder::LittleEndian,
        peer_event_id,
        child,
        1,
    ))
    .unwrap();
    peer.write_all(&resource_request(
        XByteOrder::LittleEndian,
        14,
        child,
    ))
    .unwrap();
    expect_x_reply(&read_x_reply(&mut peer, XByteOrder::LittleEndian), XByteOrder::LittleEndian);
    stream
        .write_all(&configure_window_request(
            XByteOrder::LittleEndian,
            child,
            0x000f,
            &[2, 16, 1276, 1422],
        ))
        .unwrap();

    let present = read_x_reply(&mut stream, XByteOrder::LittleEndian);
    assert_eq!(present.len(), 40);
    assert_eq!(present[0], 35);
    assert_eq!(present[1], X_PRESENT_MAJOR_OPCODE);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &present[8..10]), 0);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &present[12..16]), event_id);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &present[16..20]), child);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &present[20..22]), 2);
    assert_eq!(read_i16(XByteOrder::LittleEndian, &present[22..24]), 16);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &present[24..26]), 1276);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &present[26..28]), 1422);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &present[32..34]), 1276);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &present[34..36]), 1422);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &present[36..40]), 0);
    let peer_present = read_x_reply(&mut peer, XByteOrder::LittleEndian);
    assert_eq!(peer_present[0], 35);
    assert_eq!(peer_present[1], X_PRESENT_MAJOR_OPCODE);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &peer_present[12..16]),
        peer_event_id
    );
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &peer_present[16..20]),
        child
    );
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &peer_present[24..26]),
        1276
    );
    assert_eq!(
        read_u16(XByteOrder::LittleEndian, &peer_present[26..28]),
        1422
    );
    let core = read_x_record(&mut stream);
    assert_eq!(core[0], 22);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &core[8..12]), child);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &core[20..22]), 1276);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &core[22..24]), 1422);

    stream
        .write_all(&present_select_input_request(
            XByteOrder::LittleEndian,
            event_id,
            child,
            1 << 1,
        ))
        .unwrap();
    stream
        .write_all(&configure_window_request(
            XByteOrder::LittleEndian,
            child,
            0x000c,
            &[1000, 700],
        ))
        .unwrap();
    assert_eq!(read_x_record(&mut stream)[0], 22);

    stream
        .write_all(&present_select_input_request(
            XByteOrder::LittleEndian,
            event_id,
            child,
            1,
        ))
        .unwrap();
    stream
        .write_all(&configure_window_request(
            XByteOrder::LittleEndian,
            child,
            0x000c,
            &[1000, 700],
        ))
        .unwrap();
    // The final read proves silence, so it needs only a short negative window.
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut unexpected = [0_u8; 1];
    let error = stream.read(&mut unexpected).unwrap_err();
    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));

    drop(stream);
    drop(peer);
    service_sender
        .send(XServerFrontendServiceCommand::StopAccepting)
        .unwrap();
    drop(service_sender);
    server.join().unwrap();
    std::fs::remove_file(&socket_path).unwrap();
}

#[cfg(unix)]
#[test]
fn routed_service_applies_topology_update_and_notifies_randr_subscriber() {
    use std::io::Write;
    use std::num::NonZeroUsize;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x11-randr-update-test-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let server_path = socket_path.clone();
    let (transaction_sender, _transaction_receiver) =
        std::sync::mpsc::sync_channel(X_AUTHORITY_OBSERVED_TRANSACTION_CHANNEL_CAPACITY);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let (service_sender, service_receiver) = std::sync::mpsc::sync_channel(2);
    let config = XServerFrontendConfig::new(&server_path, NamespaceId::from_raw(854)).unwrap();
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
    let mut stream = connect_x_socket(&socket_path);
    stream
        .write_all(&setup_request(XByteOrder::LittleEndian, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut stream, XByteOrder::LittleEndian);
    stream
        .write_all(&randr_select_input_request(
            XByteOrder::LittleEndian,
            X_SETUP_DEFAULT_ROOT,
            0x47,
        ))
        .unwrap();
    stream
        .write_all(&randr_window_request(
            XByteOrder::LittleEndian,
            X_RANDR_GET_SCREEN_RESOURCES_CURRENT_MINOR_OPCODE,
            X_SETUP_DEFAULT_ROOT,
        ))
        .unwrap();
    expect_x_reply(&read_x_reply(&mut stream, XByteOrder::LittleEndian), XByteOrder::LittleEndian);

    let snapshot = OutputTopologySnapshot {
        generation: 2,
        primary: OutputId::from_raw(9),
        outputs: vec![OutputTopologyEntry {
            output: OutputId::from_raw(9),
            logical: Rect {
                x: 0,
                y: 0,
                width: 1600,
                height: 900,
            },
            pixel_size: Size {
                width: 1600,
                height: 900,
            },
            scale: 1,
            refresh_millihz: 60_000,
            timing: None,
        }],
    };
    let (ack_sender, ack_receiver) = std::sync::mpsc::sync_channel(1);
    service_sender
        .send(XServerFrontendServiceCommand::UpdateOutputTopology {
            snapshot,
            acknowledgement: ack_sender,
        })
        .unwrap();
    assert_eq!(
        ack_receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
        XAuthorityOutputUpdateOutcome::Applied {
            generation: 2,
            notifications: 4,
        }
    );
    let event = read_x_record(&mut stream);
    assert_eq!(event[0], X_RANDR_FIRST_EVENT, "event={event:?}");
    assert_eq!(read_u32(XByteOrder::LittleEndian, &event[8..12]), 2);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &event[24..26]), 1600);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &event[26..28]), 900);
    let crtc = read_x_record(&mut stream);
    assert_eq!(crtc[0], X_RANDR_FIRST_EVENT + 1);
    assert_eq!(crtc[1], 0);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &crtc[12..16]),
        0x1000_0009
    );
    assert_eq!(read_u16(XByteOrder::LittleEndian, &crtc[28..30]), 1600);
    assert_eq!(read_u16(XByteOrder::LittleEndian, &crtc[30..32]), 900);
    let output = read_x_record(&mut stream);
    assert_eq!(output[0], X_RANDR_FIRST_EVENT + 1);
    assert_eq!(output[1], 1);
    assert_eq!(
        read_u32(XByteOrder::LittleEndian, &output[16..20]),
        0x2000_0009
    );
    let resources = read_x_record(&mut stream);
    assert_eq!(resources[0], X_RANDR_FIRST_EVENT + 1);
    assert_eq!(resources[1], 5);
    assert_eq!(read_u32(XByteOrder::LittleEndian, &resources[4..8]), 2);

    drop(stream);
    service_sender
        .send(XServerFrontendServiceCommand::StopAccepting)
        .unwrap();
    drop(service_sender);
    server.join().unwrap();
    std::fs::remove_file(&socket_path).unwrap();
}
