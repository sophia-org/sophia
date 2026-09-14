#[test]
fn review_private_deadline_unbound_queued_work_does_not_expire_by_age() {
    let client = XServerFrontendClientId(6201);
    let surface = SurfaceId::new(6201, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(6201);
    let f = ordered_ingress_fixture(client, surface);
    f.ingress.submit(button_to(surface, delivery, 272, true)).unwrap();
    let recovery = &f.private.broker.registry.input_recovery;
    let ticket = recovery.ticket(delivery).unwrap();
    assert_eq!(ticket.client, None);
    assert!(recovery.recover(ticket.admitted_at + Duration::from_secs(7), false).unwrap().is_empty(),
        "private unbound queue residence produced a deadline");
    assert!(f.deliveries.try_recv().is_err());
    assert_eq!(recovery.ticket(delivery).unwrap().client, None);
}

#[test]
fn review_private_deadline_enqueued_unwritten_work_keeps_socket_and_ticket() {
    let client = XServerFrontendClientId(6202);
    let surface = SurfaceId::new(6202, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(6202);
    let mut f = ordered_ingress_fixture(client, surface);
    let (mut socket, mut peer) = UnixStream::pair().unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    f.private.broker.registry.input_recovery.attach(client, socket.try_clone().unwrap()).unwrap();
    held_button(&mut f, surface, 6202);
    let recovery = &f.private.broker.registry.input_recovery;
    let ticket = recovery.ticket(delivery).unwrap();
    assert_eq!(ticket.client, Some(client));
    // deliver_turn enqueued the real input event; no writer was constructed.
    assert!(recovery.recover(ticket.admitted_at + Duration::from_secs(7), false).unwrap().is_empty(),
        "private input queued for an absent writer produced a transport deadline");
    assert!(f.deliveries.try_recv().is_err());
    assert!(!recovery.state.lock().unwrap().connections.get(&client).unwrap().revoked);
    assert!(recovery.ticket(delivery).is_some());
    assert!(f.channels.input.try_recv().is_ok(), "queued event was retained");
    peer.write_all(b"p").unwrap();
    let mut byte = [0]; socket.read_exact(&mut byte).unwrap(); assert_eq!(byte, [b'p']);
}

#[test]
fn review_private_deadline_forced_unbound_revoke_still_ends_work() {
    let client = XServerFrontendClientId(6203);
    let surface = SurfaceId::new(6203, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(6203);
    let f = ordered_ingress_fixture(client, surface);
    f.ingress.submit(button_to(surface, delivery, 272, true)).unwrap();
    let recovery = &f.private.broker.registry.input_recovery;
    assert_eq!(recovery.recover(Instant::now(), true).unwrap().len(), 1);
    let receipt = f.deliveries.try_recv().unwrap();
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(receipt.outcome, XAuthorityInputDeliveryOutcome::EpochRevoked);
    assert_eq!(recovery.claim_execution(Some(delivery)), ExecutionClaim::Ended);
    assert!(f.deliveries.try_recv().is_err());
}

#[test]
fn review_private_deadline_forced_bound_revoke_respects_an_applied_claim() {
    let client = XServerFrontendClientId(6204);
    let surface = SurfaceId::new(6204, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(6204);
    let mut f = ordered_ingress_fixture(client, surface);
    let (mut socket, _peer) = UnixStream::pair().unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    f.private.broker.registry.input_recovery.attach(client, socket.try_clone().unwrap()).unwrap();
    held_button(&mut f, surface, 6204);
    let recovery = &f.private.broker.registry.input_recovery;
    assert_eq!(recovery.claim_execution(Some(delivery)), ExecutionClaim::Claimed);
    assert!(recovery.recover(Instant::now(), true).unwrap().is_empty());
    assert!(recovery.state.lock().unwrap().connections.get(&client).unwrap().revoked);
    let mut byte = [0]; assert_eq!(socket.read(&mut byte).unwrap(), 0);
    recovery.resolve_claim(Some(delivery), false);
    assert!(f.deliveries.try_recv().is_err(), "earlier application forbids pre-effect cancellation");
    assert!(recovery.ticket(delivery).is_some(), "delivery settlement remains owed");
}
