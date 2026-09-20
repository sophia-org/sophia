// A real private reader that stops taking its bytes is contained to its own
// connection. Its writer blocks inside the declared allowance, its delivery
// is answered TimedOut when that allowance is spent, and its socket is ended;
// a healthy peer admitted to the same instance keeps receiving its events
// and keeps getting its requests answered the whole time.
//
// This is the obligation `native_stalled_reader` names, and it wants real
// sockets: the terminal-level control proves a blocked head stops only its
// own connection, and the watchdog control proves a blocked write on a real
// socket can be ended, but nothing before this put two real readers on one
// running service and let one of them stop.

/// One admitted real client: its socket, the custody the registry keeps for
/// it, the window it selected input on, and the sequence its wire is at.
#[cfg(unix)]
struct RealReader {
    peer: UnixStream,
    custody: Arc<PrivateEvidenceCustody>,
    surface: SurfaceId,
    window: u32,
    sequence: u16,
}

#[cfg(unix)]
impl RealReader {
    /// Connect, set up, select key, button and focus events on one window,
    /// and find the custody the registry keeps for exactly this connection,
    /// by the window it registered rather than by arrival order.
    fn open(service: &LifecycleService, suffix: u32) -> Self {
        let mut peer = connect_private_client(&service.path);
        let window = handshake_ids(&mut peer) | suffix;
        let (surface, sequence) = selecting_window(
            &mut peer,
            &service.transactions,
            window,
            (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 21),
        );
        let custody = waited_for_value(|| {
            kept_custodies(&service.registry).into_iter().find(|custody| {
                custody.attachment() == Some(PrivateAttachment::Started)
                    && registry_window_of(&service.registry, custody.cleanup_record().client)
                        == Some(window)
            })
        })
        .expect("the exact newly serving connection");
        Self {
            peer,
            custody,
            surface,
            window,
            sequence,
        }
    }

    fn client(&self) -> XServerFrontendClientId {
        self.custody.cleanup_record().client
    }

    fn ingress(&self, service: &LifecycleService, device: u64) -> PrivateIngress {
        service
            .access
            .ingress_for(
                &service.owner.lease(),
                self.client(),
                DeviceId::from_raw(device),
            )
            .expect("an ingress for an admitted client")
    }

    /// Focus this reader's window through the service's own control
    /// producer, and read the FocusIn its window selected.
    fn focus(&mut self, service: &LifecycleService, transaction: u64) {
        let lease = service.owner.lease();
        service
            .access
            .control_producer(&lease)
            .expect("the control producer")
            .submit(
                &lease,
                XAuthorityClientControlCommand {
                    client: self.client(),
                    command: XAuthorityControlCommand::FocusSurface {
                        transaction: TransactionId::from_raw(transaction),
                        surface: self.surface,
                    },
                },
            )
            .expect("the order accepts the focus control");
        assert_eq!(
            ack_for(&service.acks, transaction)
                .expect("the focus control is acknowledged")
                .acknowledgement
                .outcome,
            XAuthorityControlOutcome::Delivered
        );
        assert_eq!(
            read_event(&mut self.peer, 3),
            Some(expected_focus_in(self.sequence, self.window)),
            "the focused window is told so"
        );
    }

    /// One GetInputFocus round trip on this connection, its reply read within
    /// the bound. An event that arrives ahead of the reply is set aside; the
    /// reply is recognised by its type and must carry this request's own
    /// sequence, so a reply to something else cannot stand in for it.
    fn round_trip(&mut self, seconds: u64) {
        use std::io::Write;
        self.peer
            .write_all(&[43, 0, 1, 0])
            .expect("a GetInputFocus request is written");
        self.sequence = self.sequence.wrapping_add(1);
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            let frame = read_event(&mut self.peer, seconds)
                .unwrap_or_else(|| panic!("a reply within {seconds} s on the healthy connection"));
            if frame[0] == 1 {
                assert_eq!(
                    u16::from_le_bytes([frame[2], frame[3]]),
                    self.sequence,
                    "the reply answers the request this round trip made"
                );
                return;
            }
            assert!(
                Instant::now() < deadline,
                "events kept arriving and no reply came within {seconds} s"
            );
        }
    }
}

/// Submit one route, retrying a saturated grant until the bound: a grant
/// holds one completion cell, and the previous delivery's cell is released
/// asynchronously after its receipt is published.
#[cfg(unix)]
fn submit_within(
    ingress: &PrivateIngress,
    lease: &PrivateServiceLease<'_>,
    route: XAuthorityRoutedInput,
    seconds: u64,
) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut route = route;
    loop {
        match ingress.submit(lease, route) {
            Ok(_) => return,
            Err(crate::PrivateSendError::Saturated(returned)) => {
                assert!(
                    Instant::now() < deadline,
                    "the grant's cell was not released within {seconds} s"
                );
                std::thread::yield_now();
                route = returned;
            }
            Err(other) => panic!("a live producer's submission was refused: {other:?}"),
        }
    }
}

/// The receipt for one delivery, within the bound; receipts for other
/// deliveries that arrive meanwhile are kept for the caller to judge.
#[cfg(unix)]
fn receipt_of_delivery(
    deliveries: &Receiver<XAuthorityClientInputDelivery>,
    delivery: u64,
    bound: Duration,
    others: &mut Vec<XAuthorityClientInputDelivery>,
) -> Option<XAuthorityClientInputDelivery> {
    let deadline = Instant::now() + bound;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match deliveries.recv_timeout(remaining) {
            Ok(receipt) if receipt.delivery.raw() == delivery => return Some(receipt),
            Ok(receipt) => others.push(receipt),
            Err(_) => return None,
        }
    }
}

/// A request answered, one scroll and one press and release delivered to
/// the healthy reader, each read back as its exact core frames within the
/// bound and each settled Flushed. Every bound here is a claim about the
/// healthy peer while another reader's writer is blocked, and the three
/// paths are separate claims: the reply comes from the connection's own
/// reader and dispatch, the events from the service turn and this
/// connection's own ordered writer.
#[cfg(unix)]
fn healthy_peer_continues(
    service: &LifecycleService,
    healthy: &mut RealReader,
    keys: &PrivateIngress,
    first_id: u64,
    others: &mut Vec<XAuthorityClientInputDelivery>,
) {
    let lease = service.owner.lease();
    let began = Instant::now();
    healthy.round_trip(3);
    assert!(
        began.elapsed() < Duration::from_secs(3),
        "the healthy peer's request is answered promptly"
    );
    // THE POINTER BEFORE THE KEYS. A scroll routed to this surface is read
    // back as the two core frames it makes, and puts the pointer here: a key
    // routed to the focused surface while the pointer was over the other
    // client's surface was refused by the route rather than delivered, which
    // is a question about the routing model and not what this control is
    // about.
    let scroll = first_id + 2;
    submit_within(
        keys,
        &lease,
        axis_to(healthy.surface, XAuthorityInputDeliveryId::from_raw(scroll), 1_000 + first_id),
        3,
    );
    for kind in [4u8, 5] {
        let frame = read_event(&mut healthy.peer, 3)
            .expect("the healthy peer's scroll frame arrives within 3 s");
        assert_eq!(
            (frame[0], u32::from_le_bytes([frame[12], frame[13], frame[14], frame[15]])),
            (kind, healthy.window),
            "a scroll frame for this window"
        );
    }
    let receipt = receipt_of_delivery(&service.deliveries, scroll, Duration::from_secs(3), others)
        .expect("the healthy peer's scroll is settled within 3 s");
    assert_eq!(
        (receipt.client, receipt.outcome),
        (healthy.client(), XAuthorityInputDeliveryOutcome::Flushed)
    );
    for (offset, pressed) in [(0, true), (1, false)] {
        let id = first_id + offset;
        // Evdev 30 is KEY_A, X keycode 38: no modifier, so the state field is
        // zero on both edges and nothing about keyboard state is claimed.
        submit_within(
            keys,
            &lease,
            key_service_route(healthy.surface, id, 30, pressed),
            3,
        );
        // The key event reports the pointer where the scroll put it: (2, 3)
        // in the root and, the window being at the origin, in the window.
        let mut expected = expected_key_service_event(healthy.sequence, healthy.window, 38, pressed, 0);
        expected[20..28].copy_from_slice(&[2, 0, 3, 0, 2, 0, 3, 0]);
        assert_eq!(
            read_event(&mut healthy.peer, 3),
            Some(expected),
            "the healthy peer's key event arrives within 3 s"
        );
        let receipt = receipt_of_delivery(&service.deliveries, id, Duration::from_secs(3), others)
            .expect("the healthy peer's delivery is settled within 3 s");
        assert_eq!(receipt.client, healthy.client());
        assert_eq!(
            receipt.outcome,
            XAuthorityInputDeliveryOutcome::Flushed,
            "and settled as flushed, not as anything the stalled reader earned"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_stalled_private_reader_is_contained_while_a_healthy_peer_continues() {
    let mut service =
        LifecycleService::launch_with_capacity("stalled-reader", 11603, None, false, 4);
    service.start();
    let mut stalled = RealReader::open(&service, 0x0e41);
    let mut healthy = RealReader::open(&service, 0x0e51);
    assert_ne!(stalled.client(), healthy.client());
    // KEYS FOLLOW FOCUS, BUTTONS FOLLOW THEIR ROUTE. The healthy window is
    // focused so its keys resolve to it; the stalled reader is named by the
    // surface each button is routed to, and the first pair is read back from
    // its socket so the recipient is established rather than assumed.
    healthy.focus(&service, 116030);
    let buttons = stalled.ingress(&service, 2);
    let keys = healthy.ingress(&service, 1);
    let lease = service.owner.lease();
    let mut others = Vec::new();
    for (offset, pressed) in [(0u64, true), (1, false)] {
        let id = 11_603_000 + offset;
        submit_within(&buttons, &lease, button_to(stalled.surface, XAuthorityInputDeliveryId::from_raw(id), 272, pressed), 3);
        assert_eq!(
            read_event(&mut stalled.peer, 3),
            Some(expected_button_event(pressed, stalled.sequence, stalled.window, 1)),
            "before it stalls, the reader receives what is routed to it"
        );
        let receipt = receipt_of_delivery(&service.deliveries, id, Duration::from_secs(3), &mut others)
            .expect("settled");
        assert_eq!((receipt.client, receipt.outcome), (stalled.client(), XAuthorityInputDeliveryOutcome::Flushed));
    }
    // FROM HERE THE STALLED READER NEVER READS AGAIN. Its writer keeps
    // flushing into the kernel's buffer until the buffer is full, and then
    // one delivery's send blocks for the whole declared allowance. Axis
    // events carry the fill: they hold nothing, so repeating them claims
    // nothing about press semantics, and their timestamps rise.
    let started = Instant::now();
    let mut round = 2u64;
    let mut healthy_rounds_inside_the_stall = 0u32;
    let timed_out = loop {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the stalled reader's writer was not stopped within a minute: {round} deliveries flushed"
        );
        let id = 11_603_000 + round;
        submit_within(&buttons, &lease, axis_to(stalled.surface, XAuthorityInputDeliveryId::from_raw(id), 30 + round), 3);
        round += 1;
        // A BRIEF SILENCE MEANS THE SEND IS BLOCKED, or the writer is merely
        // descheduled. Either way the healthy peer is exercised now and the
        // receipt awaited afterwards: a Flushed that arrives late costs the
        // control nothing, and the TimedOut it is about can only arrive after
        // the allowance, so the peer's rounds before it fall inside the stall.
        let receipt = match receipt_of_delivery(&service.deliveries, id, Duration::from_millis(300), &mut others) {
            Some(receipt) => receipt,
            None => {
                healthy_peer_continues(&service, &mut healthy, &keys, 11_604_000 + u64::from(healthy_rounds_inside_the_stall) * 4, &mut others);
                healthy_rounds_inside_the_stall += 1;
                receipt_of_delivery(&service.deliveries, id, Duration::from_secs(12), &mut others)
                    .expect("the blocked delivery is answered within the declared allowance")
            }
        };
        assert_eq!(receipt.client, stalled.client());
        match receipt.outcome {
            XAuthorityInputDeliveryOutcome::Flushed => {
                healthy_rounds_inside_the_stall = 0;
                continue;
            }
            XAuthorityInputDeliveryOutcome::TimedOut => break receipt,
            other => panic!("a reader that stopped taking its bytes ends in TimedOut, not {other:?}"),
        }
    };
    assert!(
        healthy_rounds_inside_the_stall >= 1,
        "the healthy peer was exercised while the stalled reader's send was blocked"
    );
    assert!(
        started.elapsed() >= Duration::from_secs(6),
        "TimedOut is the declared six-second allowance expiring, not something quicker: {:?}",
        started.elapsed()
    );
    assert!(
        others.iter().all(|receipt| receipt.client == stalled.client()
            && receipt.outcome == XAuthorityInputDeliveryOutcome::Flushed),
        "no other receipt was anything but the stalled reader's earlier flushes: {others:?}"
    );
    // CONTAINED: the stalled reader's own socket is ended by its writer. What
    // the kernel still buffered for it can be drained, and then it is over.
    {
        use std::io::Read;
        stalled.peer.set_read_timeout(Some(Duration::from_secs(5))).expect("a bounded drain");
        let mut drained = Vec::new();
        stalled.peer.read_to_end(&mut drained).expect("the stalled reader's socket reads to its end");
        assert!(!drained.is_empty(), "what was flushed before the stall was really on the wire");
    }
    let _ = timed_out;
    // AND THE HEALTHY PEER IS STILL SERVED AFTER IT.
    healthy_peer_continues(&service, &mut healthy, &keys, 11_605_000, &mut others);
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    assert!(closed.succeeded, "the instance stops cleanly with one reader ended: {:?}", closed.error);
}
