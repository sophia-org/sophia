// A request the executor refuses before the authority runs it is answered
// as a refusal and frees its grant. Before this, such a request had no
// completion at all: the grant's one cell stayed held, and the producer's
// next request was saturated for good. One refused injection wedged the
// injector, and no control had followed a refusal with a second request.

#[cfg(unix)]
#[test]
fn an_execution_refusal_answers_the_delivery_and_frees_the_grant_for_the_next_request() {
    let (launched, socket_path) = launch_producing("refused-frees-grant", 9641, 4);
    launched.access.await_ready(Duration::from_secs(15)).unwrap();
    let mut client = connect_private_client(&socket_path);
    let window = handshake_ids(&mut client) | 0x0a41;
    let (surface, _sequence) = selecting_window(&mut client, &launched.transactions, window, 3);
    let client_id = wait_attached(&launched.registry).cleanup_record().client;
    let lease = launched.owner.lease();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .unwrap();

    // REFUSED BEFORE THE AUTHORITY: a repeat-mode key is declined by the
    // executor's own checks, which never enter common.
    let mut repeat = key_service_route(surface, 96411, 42, true);
    repeat.mode = XAuthorityRoutedInputMode::Repeat;
    ingress.submit(&lease, repeat).unwrap();
    assert_eq!(read_event(&mut client, 1), None, "nothing is delivered for a refused request");

    // THE DELIVERY IS ANSWERED, as a refusal and not left owed.
    let receipt = launched
        .deliveries
        .recv_timeout(Duration::from_secs(5))
        .expect("the refused delivery is answered");
    assert_eq!(
        (receipt.delivery, receipt.outcome),
        (
            XAuthorityInputDeliveryId::from_raw(96411),
            XAuthorityInputDeliveryOutcome::RouteRejected
        )
    );

    // AND THE GRANT IS FREE: the same producer's next request is accepted
    // rather than saturated, promptly.
    let began = Instant::now();
    let mut route = key_service_route(surface, 96412, 42, true);
    let accepted = loop {
        match ingress.submit(&lease, route) {
            Ok(_) => break began.elapsed(),
            Err(crate::PrivateSendError::Saturated(returned)) => {
                assert!(
                    began.elapsed() < Duration::from_secs(3),
                    "the refused request kept the grant's cell"
                );
                std::thread::yield_now();
                route = returned;
            }
            Err(other) => panic!("the next request was refused: {other:?}"),
        }
    };
    assert!(accepted < Duration::from_secs(1), "freed by the refusal itself, not by a later visit: {accepted:?}");

    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "refused frees grant");
    let order = outcome.order.unwrap();
    assert_eq!(order.refused, 1);
    assert_eq!(order.last_refusal, Some(PrivateExecutionRefusal::RepeatUnsupported));
    let _ = std::fs::remove_file(socket_path);
}
