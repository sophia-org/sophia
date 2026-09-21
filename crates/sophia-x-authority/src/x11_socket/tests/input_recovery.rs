#![cfg(all(test, unix))]
use super::*;
use sophia_protocol::{DeviceId, Point, RoutedInputRequest};

fn route(id: u64, surface: SurfaceId) -> XAuthorityRoutedInput {
    XAuthorityRoutedInput {
        request: RoutedInputRequest {
            serial: id,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(1),
            time_msec: 0,
            target_surface: surface,
            global_position: Point::default(),
            local_position: Point::default(),
            kind: InputEventKind::Key {
                keycode: 30,
                pressed: false,
            },
        },
        route_lease: None,
        delivery: Some(XAuthorityInputDeliveryId::from_raw(id)),
        mode: XAuthorityRoutedInputMode::Deliver,
        origin: XAuthorityRoutedInputOrigin::Physical,
    }
}

fn ledger(capacity: usize) -> (InputRecovery, Receiver<XAuthorityClientInputDelivery>) {
    let (sender, receiver) = channel();
    (
        InputRecovery::new(capacity, Some(sender), Arc::default()),
        receiver,
    )
}

#[test]
fn input_recovery_keeps_absolute_deadline_and_five_second_startup_grace() {
    let (recovery, receipts) = ledger(3);
    let now = Instant::now();
    let client = XServerFrontendClientId(1);
    let old = route(1, SurfaceId::new(11, 3));
    recovery.register(client, None).unwrap();
    assert!(recovery.admit(&old, 7, now));
    recovery.bind(old.delivery, client).unwrap();
    assert!(
        recovery
            .recover(now + Duration::from_millis(5999), false)
            .unwrap()
            .is_empty()
    );
    assert!(recovery.admit(
        &route(2, old.request.target_surface),
        7,
        now + Duration::from_secs(5)
    ));
    let expired = recovery
        .recover(now + Duration::from_secs(6), false)
        .unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].surface, SurfaceId::new(11, 3));
    let receipt = receipts.try_recv().unwrap();
    assert_eq!(receipt.outcome, XAuthorityInputDeliveryOutcome::TimedOut);
    assert!(recovery.observe(receipt));
    assert!(!recovery.observe(receipt));
    recovery
        .finish(
            client,
            old.delivery,
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .unwrap();
    assert!(receipts.try_recv().is_err());
}

#[test]
fn input_recovery_cancel_before_routing_cannot_resurrect_or_release_capacity_early() {
    let (recovery, receipts) = ledger(1);
    let now = Instant::now();
    let old = route(1, SurfaceId::new(11, 3));
    assert!(recovery.admit(&old, 1, now));
    recovery.recover(now, true).unwrap();
    let receipt = receipts.try_recv().unwrap();
    assert_eq!(
        receipt.outcome,
        XAuthorityInputDeliveryOutcome::EpochRevoked
    );
    assert!(recovery.observe(receipt));
    assert!(!recovery.admit(&route(2, old.request.target_surface), 2, now));
    assert!(!recovery.begin_routing(old.delivery));
    assert!(recovery.admit(&route(2, old.request.target_surface), 2, now));
}

#[test]
fn input_recovery_wrong_client_duplicate_and_late_receipts_cannot_settle() {
    let (recovery, receipts) = ledger(2);
    let client = XServerFrontendClientId(1);
    let request = route(1, SurfaceId::new(11, 3));
    recovery.register(client, None).unwrap();
    assert!(recovery.admit(&request, 1, Instant::now()));
    recovery.bind(request.delivery, client).unwrap();
    recovery
        .finish(
            XServerFrontendClientId(2),
            request.delivery,
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .unwrap();
    assert!(receipts.try_recv().is_err());
    recovery
        .finish(
            client,
            request.delivery,
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .unwrap();
    let receipt = receipts.try_recv().unwrap();
    assert!(!recovery.observe(XAuthorityClientInputDelivery {
        client: XServerFrontendClientId(2),
        ..receipt
    }));
    assert!(recovery.observe(receipt));
    assert!(!recovery.observe(receipt));
    assert!(
        recovery
            .recover(Instant::now() + Duration::from_secs(8), false)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn input_recovery_disconnect_interrupts_a_blocked_writer_without_its_mutex() {
    let (recovery, receipts) = ledger(3);
    let now = Instant::now();
    let client = XServerFrontendClientId(1);
    let healthy = XServerFrontendClientId(2);
    recovery.register(client, None).unwrap();
    recovery.register(healthy, None).unwrap();
    let (mut socket, _nonreading_peer) = UnixStream::pair().unwrap();
    let (mut good_socket, mut good_peer) = UnixStream::pair().unwrap();
    recovery
        .attach(client, socket.try_clone().unwrap())
        .unwrap();
    recovery
        .attach(healthy, good_socket.try_clone().unwrap())
        .unwrap();
    // Fill the actual kernel send queue, then block a writer holding the same
    // serialization mutex used by the frontend. No display or client secret.
    socket.set_nonblocking(true).unwrap();
    loop {
        match socket.write(&[0; 8192]) {
            Ok(_) => (),
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => panic!("filling test socket: {error}"),
        }
    }
    socket.set_nonblocking(false).unwrap();
    let socket = Arc::new(Mutex::new(socket));
    let writer_socket = socket.clone();
    let (locked, ready) = channel();
    let (done, finished) = channel();
    let writer = std::thread::spawn(move || {
        let mut stream = writer_socket.lock().unwrap();
        locked.send(()).unwrap();
        done.send(stream.write_all(&[0; 8192]).is_err()).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(socket.try_lock().is_err());
    let request = route(1, SurfaceId::new(11, 3));
    recovery.admit(&request, 1, now);
    recovery.bind(request.delivery, client).unwrap();
    recovery
        .recover(now + Duration::from_secs(6), false)
        .unwrap();
    assert!(finished.recv_timeout(Duration::from_secs(1)).unwrap());
    writer.join().unwrap();
    good_socket.write_all(b"healthy").unwrap();
    let mut bytes = [0; 7];
    good_peer.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"healthy");
    assert_eq!(
        receipts.try_recv().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::TimedOut
    );
    assert!(receipts.try_recv().is_err());
}

#[test]
fn input_recovery_routing_binds_the_grab_receiver_and_revokes_only_its_grab() {
    let namespace = NamespaceId::from_raw(1);
    let owner = XServerFrontendClientId(1);
    let grabber = XServerFrontendClientId(2);
    let surface = SurfaceId::new(11, 3);
    let window = XResourceId::new(0x200001, 1);
    let (ack, _acks) = sync_channel(4);
    let (delivery, receipts) = channel();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        ack,
        delivery,
    );
    let (_owner, owner_channels) = broker.registry.register_client(owner).unwrap();
    let (_grabber, grab_channels) = broker.registry.register_client(grabber).unwrap();
    broker
        .registry
        .register_surface(owner, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_keyboard(
            namespace,
            crate::XActiveInputGrab {
                owner: grabber.raw(),
                window,
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: 0,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();
    let sender = broker.routed_input_sender();
    let request = route(1, surface);
    sender.send(request.clone()).unwrap();
    broker.route_pending().unwrap();
    assert!(owner_channels.input.try_recv().is_err());
    assert_eq!(grab_channels.input.try_recv().unwrap().client, grabber);
    let ticket = sender.delivery_ticket(request.delivery.unwrap()).unwrap();
    assert_eq!(ticket.client, Some(grabber));
    sender
        .recover_input_deliveries(ticket.admitted_at + Duration::from_secs(6), false)
        .unwrap();
    assert_eq!(receipts.try_recv().unwrap().client, grabber);
    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .keyboard_grab(namespace)
            .is_none()
    );
    assert!(broker.registry.input_recovery.active(None, owner));
    assert!(!broker.registry.input_recovery.active(None, grabber));
    sender.send(route(2, surface)).unwrap();
    broker.route_pending().unwrap();
    let next = owner_channels.input.try_recv().unwrap();
    assert_eq!(next.client, owner);
    assert_eq!(next.delivery, Some(XAuthorityInputDeliveryId::from_raw(2)));
    assert!(grab_channels.input.try_recv().is_err());
}

#[test]
fn input_recovery_writer_exit_settles_current_and_queued_deliveries() {
    let (recovery, receipts) = ledger(3);
    let client = XServerFrontendClientId(1);
    recovery.register(client, None).unwrap();
    for id in 1..=2 {
        let request = route(id, SurfaceId::new(11, 3));
        recovery.admit(&request, 1, Instant::now());
        recovery.bind(request.delivery, client).unwrap();
    }
    let (_sender, receiver) = sync_channel(2);
    let receiver = X11InputEventReceiver::Routed {
        receiver,
        deliveries: None,
        recovery: Some(recovery.clone()),
    };
    {
        let _worker = X11InputWriterRecoveryGuard {
            receiver: &receiver,
            client,
        };
        let _current = X11InputDeliveryGuard {
            receiver: &receiver,
            client,
            delivery: Some(XAuthorityInputDeliveryId::from_raw(1)),
            settled: std::cell::Cell::new(false),
        };
        // Unwinding any writer error settles the current event, then revokes
        // and settles its queued successor. Neither is reported as flushed.
    }
    let results: Vec<_> = receipts.try_iter().collect();
    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0].outcome,
        XAuthorityInputDeliveryOutcome::WriteFailed
    );
    assert_eq!(
        results[1].outcome,
        XAuthorityInputDeliveryOutcome::ClientDisconnected
    );
}

#[test]
fn input_recovery_expiration_and_completion_have_one_winner() {
    for completion_first in [true, false] {
        let (recovery, receipts) = ledger(2);
        let client = XServerFrontendClientId(1);
        let request = route(1, SurfaceId::new(11, 3));
        let now = Instant::now();
        recovery.register(client, None).unwrap();
        recovery.admit(&request, 1, now);
        recovery.bind(request.delivery, client).unwrap();
        if completion_first {
            recovery
                .finish(
                    client,
                    request.delivery,
                    XAuthorityInputDeliveryOutcome::Flushed,
                )
                .unwrap();
        }
        recovery
            .recover(now + Duration::from_secs(6), false)
            .unwrap();
        if !completion_first {
            recovery
                .finish(
                    client,
                    request.delivery,
                    XAuthorityInputDeliveryOutcome::Flushed,
                )
                .unwrap();
        }
        let results: Vec<_> = receipts.try_iter().collect();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].outcome,
            if completion_first {
                XAuthorityInputDeliveryOutcome::Flushed
            } else {
                XAuthorityInputDeliveryOutcome::TimedOut
            }
        );
    }
}

#[test]
fn input_recovery_frozen_cancellation_and_disconnect_release_their_queue_credit() {
    for disconnect in [false, true] {
        let namespace = NamespaceId::from_raw(1);
        let client = XServerFrontendClientId(1);
        let surface = SurfaceId::new(11, 3);
        let window = XResourceId::new(0x200001, 1);
        let (acks, _ack_receiver) = sync_channel(4);
        let (delivery, receipts) = channel();
        let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
            NonZeroUsize::new(4).unwrap(),
            acks,
            delivery,
        );
        let (registration, _channels) = broker.registry.register_client(client).unwrap();
        broker
            .registry
            .register_surface(client, namespace, surface, window)
            .unwrap();
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .grab_keyboard(
                namespace,
                crate::XActiveInputGrab {
                    owner: client.raw(),
                    window,
                    owner_events: false,
                    pointer_mode: 1,
                    keyboard_mode: 0,
                    event_mask: 0,
                    xi_event_mask: [0; 8],
                    xi_event_mask_words: 0,
                    route_lease: None,
                },
            )
            .unwrap();
        let sender = broker.routed_input_sender();
        let request = route(1, surface);
        sender.send(request.clone()).unwrap();
        broker.route_pending().unwrap();
        assert_eq!(broker.registry.frozen_input.lock().unwrap().len(), 1);
        assert_eq!(
            sender
                .delivery_ticket(request.delivery.unwrap())
                .unwrap()
                .client,
            None
        );
        let mut registration = Some(registration);
        if disconnect {
            drop(registration.take());
        } else {
            sender
                .recover_input_deliveries(Instant::now(), true)
                .unwrap();
        }
        let receipt = receipts.try_recv().unwrap();
        assert!(sender.observe_delivery(receipt));
        // A still-frozen grab must not keep the cancelled route or its ticket.
        broker.route_pending().unwrap();
        assert!(broker.registry.frozen_input.lock().unwrap().is_empty());
        assert!(sender.delivery_ticket(request.delivery.unwrap()).is_none());
        assert!(receipts.try_recv().is_err());
    }
}
