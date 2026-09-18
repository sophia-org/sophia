#[test]
fn service_keyboard_async_thaw_keeps_original_shift_request_and_exact_writer_order() {
    use std::io::Write;
    let (launched, socket) = launch_producing("producer-key-thaw", 9772, 4);
    launched.access.await_ready(Duration::from_secs(15)).unwrap();
    let mut client = connect_private_client(&socket);
    let window = handshake_ids(&mut client) | 0x0c01;
    let (surface, sequence) = selecting_window(&mut client, &launched.transactions, window, 3 | (1 << 2) | (1 << 3) | (1 << 21));
    let client_id = wait_attached(&launched.registry).cleanup_record().client;
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).unwrap();
    let ingress = launched.access.ingress_for(&lease, client_id, DeviceId::from_raw(1)).unwrap();
    assert_eq!(apply_focus(&launched, &control, &mut client, client_id, surface, 997720),
        (Some(XAuthorityControlOutcome::Delivered), Some(expected_focus_in(sequence, window))));
    key_service_pointer_pair(&launched, &ingress, &mut client, surface, sequence, window, 997721);

    // The actual socket connection produces the synchronous grab and reply.
    let mut grab = [0u8; 16];
    grab[0] = 31;
    grab[2..4].copy_from_slice(&4u16.to_le_bytes());
    grab[4..8].copy_from_slice(&window.to_le_bytes());
    grab[12] = 1; // pointer asynchronous; keyboard synchronous
    client.write_all(&grab).unwrap();
    let reply = read_event(&mut client, 5).unwrap();
    assert_eq!((reply[0], reply[1], u16::from_le_bytes([reply[2], reply[3]])), (1, 0, sequence + 1));
    let accepted = ingress.submit(&lease, key_service_route(surface, 997723, 42, true)).unwrap();
    let original = delivery_cell(&launched.registry, 997723).unwrap();
    // A later command traverses the same ready order and reaches its real
    // writer while the earlier input is retained without any native effect.
    let configured = control.submit(&lease, configure(client_id, surface, 997724)).unwrap();
    assert!(accepted < configured);
    let configuration = ack_for(&launched.acks, 997724).map(|ack| ack.acknowledgement.outcome);
    assert_eq!(configuration, Some(XAuthorityControlOutcome::Delivered));
    assert!(original.answer().is_none());
    assert_eq!(launched.registry.input_authority.lock().unwrap()
        .pointer_query_state(NamespaceId::from_raw(9772)).mask & 1, 0);

    // AsyncKeyboard is a control request on the original socket; it does not
    // resubmit, restamp, or supply an event recipient for the held request.
    client.write_all(&[35, 3, 2, 0, 0, 0, 0, 0]).unwrap();
    let pressed = read_event(&mut client, 5);
    let released_submission = ingress.submit(&lease, key_service_route(surface, 997725, 42, false)).map(|_| ()).map_err(|cause| format!("{cause:?}"));
    let released = read_event(&mut client, 5);
    let release_cell = delivery_cell(&launched.registry, 997725);
    let settled = waited_for(|| launched.controller.under_common(|authority| authority.next_debt(&mut 0).is_none()).unwrap_or(false));
    let original_answer = original.answer();
    let release_answer = release_cell.and_then(|cell| cell.answer());
    launched.commands.send(XServerFrontendServiceCommand::StopAndDisconnect).unwrap();
    let outcome = produced_outcome(launched, "original keyboard thaw");
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    assert_eq!(pressed, Some(expected_key_service_event(sequence + 2, window, 50, true, 0)),
        "order {:?}; original {original_answer:?}; release {release_answer:?}; submission {released_submission:?}; terminal {:?}; key releases {:?}", outcome.order, outcome.terminal, outcome.key_releases);
    assert_eq!(released_submission, Ok(()));
    assert_eq!(released, Some(expected_key_service_event(sequence + 2, window, 50, false, 1)));
    assert_eq!(original_answer.unwrap().outcome, XAuthorityInputDeliveryOutcome::Flushed);
    assert_eq!(release_answer.unwrap().outcome, XAuthorityInputDeliveryOutcome::Flushed);
    assert!(settled, "writer completion alone cannot close the exact native thaw obligation");
    assert!(outcome.execution_inventory_matches && outcome.execution_collected);
}
