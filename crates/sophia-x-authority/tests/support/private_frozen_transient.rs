#[test]
fn service_pointer_async_thaw_keeps_original_motion_axis_cells_and_output_order() {
    use std::io::Write;
    let (launched, socket) = launch_producing("producer-transient-thaw", 9784, 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .unwrap();
    let mut peer = connect_private_client(&socket);
    let window = handshake_ids(&mut peer) | 0x0c41;
    let (surface, sequence) = selecting_window(
        &mut peer,
        &launched.transactions,
        window,
        (1 << 2) | (1 << 3) | (1 << 6) | (1 << 21),
    );
    let client = wait_attached(&launched.registry).cleanup_record().client;
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).unwrap();
    let first = launched
        .access
        .ingress_for(&lease, client, DeviceId::from_raw(1))
        .unwrap();
    let second = launched
        .access
        .ingress_for(&lease, client, DeviceId::from_raw(2))
        .unwrap();
    assert_eq!(
        apply_focus(&launched, &control, &mut peer, client, surface, 997840).0,
        Some(XAuthorityControlOutcome::Delivered)
    );
    key_service_pointer_pair(
        &launched, &first, &mut peer, surface, sequence, window, 997841,
    );
    let mut grab = [0u8; 24];
    grab[0] = 26;
    grab[2..4].copy_from_slice(&6u16.to_le_bytes());
    grab[4..8].copy_from_slice(&window.to_le_bytes());
    grab[8..10].copy_from_slice(&((1u16 << 2) | (1 << 3) | (1 << 6)).to_le_bytes());
    grab[11] = 1; // synchronous pointer, asynchronous keyboard
    peer.write_all(&grab).unwrap();
    let reply = read_event(&mut peer, 5).unwrap();
    assert_eq!((reply[0], reply[1]), (1, 0));
    let mut motion = motion_to(surface, XAuthorityInputDeliveryId::from_raw(997843));
    motion.request.global_position = Point { x: 2.0, y: 3.0 };
    motion.request.local_position = motion.request.global_position;
    motion.request.time_msec = 41;
    let mut axis = motion.clone();
    axis.delivery = Some(XAuthorityInputDeliveryId::from_raw(997844));
    axis.request.serial = 997844;
    axis.request.time_msec = 42;
    axis.request.kind = InputEventKind::PointerAxis {
        horizontal_v120: 0,
        vertical_v120: 120,
    };
    let first_sequence = first.submit(&lease, motion).unwrap();
    let second_sequence = second.submit(&lease, axis).unwrap();
    let cells = [
        delivery_cell(&launched.registry, 997843).unwrap(),
        delivery_cell(&launched.registry, 997844).unwrap(),
    ];
    let control_sequence = control
        .submit(&lease, configure(client, surface, 997845))
        .unwrap();
    assert!(first_sequence < second_sequence && second_sequence < control_sequence);
    assert_eq!(
        ack_for(&launched.acks, 997845)
            .unwrap()
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert!(cells.iter().all(|cell| cell.answer().is_none()));
    let before = launched
        .registry
        .input_authority
        .lock()
        .unwrap()
        .pointer_query_state(NamespaceId::from_raw(9784));
    assert_eq!(before.vertical_scroll_v120, 0);
    assert_eq!(
        (
            before.position.unwrap().root_x,
            before.position.unwrap().root_y
        ),
        (0, 0)
    );
    peer.write_all(&[35, 0, 2, 0, 0, 0, 0, 0]).unwrap();
    let frames = [
        read_event(&mut peer, 5),
        read_event(&mut peer, 5),
        read_event(&mut peer, 5),
    ];
    for cell in &cells {
        assert!(waited_for(|| cell.answer().is_some()));
    }
    let after = launched
        .registry
        .input_authority
        .lock()
        .unwrap()
        .pointer_query_state(NamespaceId::from_raw(9784));
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "original transient thaw");
    assert_eq!(
        frames,
        [
            Some(expected_transient_event(6, 0, sequence + 2, window, 0, 41)),
            Some(expected_transient_event(4, 5, sequence + 2, window, 0, 42)),
            Some(expected_transient_event(
                5,
                5,
                sequence + 2,
                window,
                1 << 12,
                42
            )),
        ],
        "order {:?}; error {:?}",
        outcome.order,
        outcome.error
    );
    for (cell, id) in cells.iter().zip([997843, 997844]) {
        let answer = cell.answer().unwrap();
        assert_eq!(
            (answer.delivery, answer.outcome),
            (
                XAuthorityInputDeliveryId::from_raw(id),
                XAuthorityInputDeliveryOutcome::Flushed
            )
        );
    }
    assert_eq!(
        after.vertical_scroll_v120, 120,
        "one original axis application"
    );
    assert_eq!(after.mask, 0);
    assert_eq!(
        (
            after.position.unwrap().root_x,
            after.position.unwrap().root_y
        ),
        (2, 3)
    );
    assert_eq!(outcome.ok, Some(true));
    assert_eq!(outcome.order.unwrap().refused, 0);
    assert!(outcome.execution_inventory_matches && outcome.execution_collected);
    let _ = std::fs::remove_file(socket);
}
