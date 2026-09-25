// Drive the production writers over real Unix sockets. A failed peer must not
// turn into a shared-authority error or a successful delivery receipt.
fn peer_failure_writer(
    socket: UnixStream,
    core_mask: u32,
) -> (
    X11InputEventWriter,
    Sender<XAuthorityClientInputEvent>,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let window = XResourceId::new(0x200001, 1);
    let mut selections = XCoreEventSelectionState::default();
    selections.register(
        window,
        XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        },
    );
    selections.update(window, Some(core_mask), None);
    selections.register(
        XResourceId::new(0x200002, 1),
        XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        Rect {
            x: 200,
            y: 0,
            width: 100,
            height: 100,
        },
    );
    let (sender, receiver) = channel();
    let (deliveries, receipts) = channel();
    let writer = spawn_x11_input_event_writer(
        X11InputWriterState {
            input_watermark: None,
            stream: X11ClientOutput::shared(socket, 0),
            output_control_pending: Arc::new(AtomicUsize::new(0)),
            output_wire: Arc::new(X11WirePermission::open()),
            byte_order: XByteOrder::LittleEndian,
            sequence: Arc::new(AtomicU16::new(1)),
            focused_surface_window: Arc::new(AtomicU64::new(window.local.raw())),
            core_event_selections: Arc::new(Mutex::new(selections)),
            xkb_state_details: Arc::new(AtomicU16::new(1)),
            xkb_modifiers: Arc::new(AtomicU16::new(0)),
            surface_windows: Arc::new(Mutex::new(BTreeMap::from([(SurfaceId::new(1, 1), window)]))),
            input_authority: None,
            standalone_query_authority: None,
            namespace: NamespaceId::from_raw(1),
            client: XServerFrontendClientId::from_raw(1),
        },
        X11InputEventReceiver::Routed {
            receiver,
            deliveries: Some(deliveries),
            recovery: None,
        },
    )
    .unwrap();
    (writer, sender, receipts)
}

fn peer_failure_event(kind: &str) -> XAuthorityClientInputEvent {
    let window = XResourceId::new(0x200001, 1);
    let mut route = XAuthorityClientInputEvent {
        client: XServerFrontendClientId::from_raw(1),
        event: XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Motion,
            surface: SurfaceId::new(1, 1),
            root_x: 10,
            root_y: 10,
            event_x: 10,
            event_y: 10,
            state: 0,
            time_msec: 1,
        }),
        target_window: Some(window),
        xi_event_type: Some(6),
        xi_event_window: Some(window),
        xi_emulated_button_type: None,
        xi_emulated_button_window: None,
        xi_pointer_crossing_mask: 0,
        grab_crossing: None,
        grab_target: None,
        delivery: Some(XAuthorityInputDeliveryId::from_raw(91)),
    };
    match kind {
        "xi" => {}
        "core_enter" => route.xi_pointer_crossing_mask = 1 << 7,
        "xi_enter" => route.xi_pointer_crossing_mask = 1 << 7,
        "core_leave" | "xi_leave" => {
            let next = XResourceId::new(0x200002, 1);
            route.target_window = Some(next);
            route.xi_event_window = Some(next);
            route.xi_pointer_crossing_mask = 1 << 8;
        }
        "wheel" => {
            route.xi_event_type = None;
            route.xi_event_window = None;
            route.xi_emulated_button_type = Some(4);
            route.xi_emulated_button_window = Some(window);
        }
        _ => panic!("unknown fixture path"),
    }
    route
}

#[test]
fn peer_write_failure_input_paths_settle_once_without_killing_the_frontend() {
    let path = std::env::temp_dir().join(format!("sophia-peer-write-{}.sock", std::process::id()));
    let mut frontend =
        XServerFrontend::bind(XServerFrontendConfig::new(&path, NamespaceId::from_raw(1)).unwrap())
            .unwrap();
    // A second production writer stays alive across every failed worker reap.
    let (good_socket, mut good_peer) = UnixStream::pair().unwrap();
    good_peer
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let (good_writer, good_sender, good_receipts) = peer_failure_writer(good_socket, 0);
    for (index, kind) in [
        "xi",
        "core_enter",
        "xi_enter",
        "core_leave",
        "xi_leave",
        "wheel",
    ]
    .into_iter()
    .enumerate()
    {
        let (socket, peer) = UnixStream::pair().unwrap();
        let shutdown = socket.try_clone().unwrap();
        let mask = match kind {
            "core_enter" => 1 << 4,
            "core_leave" => 1 << 5,
            _ => 0,
        };
        let (writer, sender, receipts) = peer_failure_writer(socket, mask);
        if kind.ends_with("leave") {
            // Establish the previous pointer destination while the peer lives.
            sender.send(peer_failure_event("xi")).unwrap();
            assert_eq!(
                receipts
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .outcome,
                XAuthorityInputDeliveryOutcome::Flushed
            );
        }
        drop(peer); // First selected record after shutdown; no close/write race.
        sender.send(peer_failure_event(kind)).unwrap();
        drop(sender);
        let receipt = receipts.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(receipt.delivery, XAuthorityInputDeliveryId::from_raw(91));
        assert_eq!(
            receipt.outcome,
            XAuthorityInputDeliveryOutcome::ClientDisconnected,
            "{kind}"
        );
        let result = writer.thread.join().unwrap();
        assert!(receipts.try_recv().is_err(), "duplicate receipt for {kind}");
        // Pass the real writer outcome through the production supervisor boundary.
        let worker_id = index as u64 + 1;
        frontend.workers.insert(
            worker_id,
            X11CoreClientWorker {
                thread: std::thread::spawn(|| {}),
                shutdown,
            },
        );
        frontend
            .reap_client_worker(X11CoreClientWorkerCompletion { worker_id, result })
            .unwrap();
        good_sender.send(peer_failure_event("xi")).unwrap();
        assert_eq!(
            good_receipts
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .outcome,
            XAuthorityInputDeliveryOutcome::Flushed
        );
        let mut header = [0; 32];
        good_peer.read_exact(&mut header).unwrap();
        assert_eq!(header[0], 35);
        let mut rest = vec![0; u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize * 4];
        good_peer.read_exact(&mut rest).unwrap();
        assert!(frontend.workers.is_empty());
    }
    drop(good_sender);
    good_writer.thread.join().unwrap().unwrap();
    drop(frontend);
    let _ = std::fs::remove_file(path);
}

#[test]
fn peer_write_failure_control_is_not_reported_as_delivered() {
    let (socket, peer) = UnixStream::pair().unwrap();
    drop(peer);
    let error = write_x11_control_records(
        &X11ClientOutput::shared(socket, 0),
        &X11WirePermission::open(),
        XByteOrder::LittleEndian,
        &AtomicU16::new(1),
        vec![vec![0; 32]],
    )
    .unwrap_err();
    assert!(error.client_disconnect);
}

#[test]
fn peer_write_failure_protocol_writer_exits_cleanly() {
    let (socket, peer) = UnixStream::pair().unwrap();
    drop(peer);
    let (sender, receiver) = channel();
    let writer = spawn_x11_protocol_event_writer(
        X11ClientOutput::shared(socket, 0),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(X11WirePermission::open()),
        XByteOrder::LittleEndian,
        Arc::new(AtomicU16::new(1)),
        XServerFrontendClientId::from_raw(1),
        receiver,
    )
    .unwrap();
    sender
        .send(XClientEvent::Expose {
            sequence: 1,
            window: XResourceId::new(0x200001, 1),
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            count: 0,
        })
        .unwrap();
    drop(sender);
    writer.thread.join().unwrap().unwrap();
}

#[test]
fn peer_write_failure_does_not_downgrade_other_io_or_poisoned_locks() {
    for kind in [
        ErrorKind::BrokenPipe,
        ErrorKind::ConnectionReset,
        ErrorKind::UnexpectedEof,
    ] {
        assert!(x11_peer_write_error("test", std::io::Error::from(kind)).client_disconnect);
    }
    let error = x11_peer_write_error("test", std::io::Error::from(ErrorKind::PermissionDenied));
    assert!(!error.client_disconnect && !error.client_failure && !error.service_shutdown);
    let (socket, _peer) = UnixStream::pair().unwrap();
    let socket = X11ClientOutput::shared(socket, 0);
    let poison = socket.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poison.lock().unwrap();
        panic!("test poison");
    })
    .join();
    let error = write_x11_control_records(
        &socket,
        &X11WirePermission::open(),
        XByteOrder::LittleEndian,
        &AtomicU16::new(1),
        vec![vec![0; 32]],
    )
    .unwrap_err();
    assert!(!error.client_disconnect && !error.client_failure && !error.service_shutdown);
}
