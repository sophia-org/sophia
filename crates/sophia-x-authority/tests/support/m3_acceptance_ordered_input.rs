fn b_pointer_grab(connection: &mut BConnection, synchronous: bool) {
    use std::io::Write;
    let mut request = [0u8; 24];
    request[0] = 26;
    request[2..4].copy_from_slice(&6u16.to_le_bytes());
    request[4..8].copy_from_slice(&connection.window.to_le_bytes());
    request[8..10].copy_from_slice(&((1u16 << 2) | (1 << 3) | (1 << 6)).to_le_bytes());
    request[10] = u8::from(!synchronous);
    request[11] = 1;
    connection.peer.write_all(&request).unwrap();
    let reply = read_event(&mut connection.peer, 3).unwrap();
    connection.sequence += 1;
    assert_eq!((reply[0], reply[1]), (1, 0));
    assert_eq!(
        u16::from_le_bytes([reply[2], reply[3]]),
        connection.sequence
    );
}

fn b_protocol_barrier(connection: &mut BConnection) -> [u8; 32] {
    use std::io::Write;
    connection.peer.write_all(&[43, 0, 1, 0]).unwrap();
    connection.sequence += 1;
    let reply = read_event(&mut connection.peer, 3).unwrap();
    assert_eq!(reply[0], 1);
    assert_eq!(
        u16::from_le_bytes([reply[2], reply[3]]),
        connection.sequence
    );
    reply
}

fn b_no_input_tail(connection: &mut BConnection, focus: u32) -> [u8; 32] {
    let reply = b_protocol_barrier(connection);
    let mut expected = [0u8; 32];
    expected[0] = 1;
    expected[1] = 1; // The actual FocusSurface source uses revert-to parent.
    expected[2..4].copy_from_slice(&connection.sequence.to_le_bytes());
    expected[8..12].copy_from_slice(&focus.to_le_bytes());
    assert_eq!(
        reply, expected,
        "no extra event after the final original receipt"
    );
    reply
}

#[test]
fn b_ordered_input() {
    use std::io::Write;
    let mut service =
        LifecycleService::launch_with_capacity("b-ordered-input", 11203, None, false, 4);
    service.start();
    let mut first = BConnection::open(&service, 0x0d41);
    let mut second = BConnection::open(&service, 0x0d51);
    first.focus(&service, 112040);
    let keys = first.ingress(&service, 1);
    let motion_source = first.ingress(&service, 2);
    let axis_source = first.ingress(&service, 3);
    first.pointer_pair(&service, &keys, 112041);
    keys.submit(
        &service.owner.lease(),
        key_service_route(first.surface, 112043, 42, true),
    )
    .unwrap();
    assert_eq!(
        read_event(&mut first.peer, 3),
        Some(expected_key_service_event(
            first.sequence,
            first.window,
            50,
            true,
            0
        ))
    );
    b_flushed(&service, 112043, first.client());
    let original_history = b_history(&service);
    assert_eq!(original_history.1, 1);
    b_pointer_grab(&mut first, true);
    let mut motion = motion_to(first.surface, XAuthorityInputDeliveryId::from_raw(112044));
    motion.request.global_position = Point { x: 2.0, y: 3.0 };
    motion.request.local_position = motion.request.global_position;
    motion.request.time_msec = 41;
    let mut axis = motion.clone();
    axis.delivery = Some(XAuthorityInputDeliveryId::from_raw(112045));
    axis.request.serial = 112045;
    axis.request.time_msec = 42;
    axis.request.kind = InputEventKind::PointerAxis {
        horizontal_v120: 0,
        vertical_v120: 120,
    };
    let motion_sequence = motion_source
        .submit(&service.owner.lease(), motion)
        .unwrap();
    let axis_sequence = axis_source.submit(&service.owner.lease(), axis).unwrap();
    let release_sequence = keys
        .submit(
            &service.owner.lease(),
            state_only_key_route(first.surface, 112046, 42),
        )
        .unwrap();
    assert!(motion_sequence < axis_sequence && axis_sequence < release_sequence);
    let cells = [
        delivery_cell(&service.registry, 112044).unwrap(),
        delivery_cell(&service.registry, 112045).unwrap(),
    ];
    // The later control passes the frozen input barrier through the actual
    // service order while all three original input requests stay untouched.
    service
        .access
        .control_producer(&service.owner.lease())
        .unwrap()
        .submit(
            &service.owner.lease(),
            configure(first.client(), first.surface, 112047),
        )
        .unwrap();
    assert_eq!(
        ack_for(&service.acks, 112047)
            .unwrap()
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert!(cells.iter().all(|cell| cell.answer().is_none()));
    assert!(delivery_cell(&service.registry, 112046).is_none());
    let (wait_thaw, allow_thaw) = Pause::pair();
    let (wait_delivery, allow_delivery) = Pause::pair();
    let (send, seen) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, _| {
            assert_eq!(runner.keyboards.modifiers(runner.seat), Some(1));
            assert_eq!(runner.frontend().terminal.frozen.len(), 3);
            let before: Vec<_> = runner
                .frontend()
                .terminal
                .frozen
                .iter()
                .map(|row| (row.sequence, row.custody.token()))
                .collect();
            for row in &runner.frontend().terminal.frozen {
                assert!(row.custody.observe().unwrap().is_none());
            }
            wait_thaw.wait();
            // Exercise only the real accounted source visits here. Its original
            // terminal owns each decided emission until normal delivery resumes.
            for _ in 0..200 {
                if runner.frontend().terminal.frozen.is_empty() {
                    break;
                }
                runner.execute_accounted_step().unwrap();
                // Frozen polling may have consumed the current allowance.
                // Wait for the original budget's own interval to advance;
                // no replacement budget, history, request or clock is made.
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(runner.frontend().terminal.frozen.is_empty());
            let after: Vec<_> = runner
                .frontend()
                .terminal
                .turn
                .iter()
                .filter_map(|item| match item {
                    PrivateOrderedItem::Ran {
                        sequence, custody, ..
                    } => Some((*sequence, custody.token())),
                    _ => None,
                })
                .collect();
            for identity in &before {
                assert!(after.contains(identity));
            }
            assert_eq!(runner.keyboards.modifiers(runner.seat), Some(0));
            assert_eq!(
                &runner.keyboards.seats[&runner.seat] as *const crate::XkbKeyboardState as usize,
                original_history.0
            );
            let record = runner
                .frontend()
                .terminal
                .settling
                .iter()
                .find(|record| {
                    record.binding == PrivateReleaseBinding::RecipientTerminationRequired
                })
                .unwrap();
            assert!(record.custody.completion.is_none());
            assert_eq!(
                record
                    .native
                    .as_ref()
                    .unwrap()
                    .key()
                    .unwrap()
                    .release_disposition(),
                private_native::KeyReleaseDisposition::RecipientTerminationRequired
            );
            assert!(!record.owes_delivery_attempt());
            send.send(json!({"before":format!("{before:?}"),"after":format!("{after:?}"),"original_history":original_history.0,"modifiers_before":1,"modifiers_after":0,"release_binding":"RecipientTerminationRequired","release_cell":false})).unwrap();
            wait_delivery.wait();
        }),
    );
    allow_thaw.entered();
    first.peer.write_all(&[35, 0, 2, 0, 0, 0, 0, 0]).unwrap();
    first.sequence += 1;
    b_protocol_barrier(&mut first);
    allow_thaw.release();
    let decided = seen.recv_timeout(Duration::from_secs(5)).unwrap();
    allow_delivery.entered();
    assert!(cells.iter().all(|cell| cell.answer().is_none()));
    // Change the real grab after decision and before delivery. Encoding may
    // use the transport sequence, but must keep the source recipient, masks,
    // timestamps and event order already stored in the original emissions.
    first.peer.write_all(&[27, 0, 2, 0, 0, 0, 0, 0]).unwrap();
    first.sequence += 1;
    b_protocol_barrier(&mut first);
    b_pointer_grab(&mut second, false);
    allow_delivery.release();
    let frames = [
        read_event(&mut first.peer, 3).unwrap(),
        read_event(&mut first.peer, 3).unwrap(),
        read_event(&mut first.peer, 3).unwrap(),
    ];
    assert_eq!(
        frames,
        [
            expected_transient_event(6, 0, first.sequence, first.window, 1, 41),
            expected_transient_event(4, 5, first.sequence, first.window, 1, 42),
            expected_transient_event(5, 5, first.sequence, first.window, 1 | (1 << 12), 42)
        ]
    );
    let receipts = [
        b_flushed(&service, 112044, first.client()),
        b_flushed(&service, 112045, first.client()),
    ];
    assert_eq!(read_event(&mut second.peer, 1), None);
    let query = service
        .registry
        .input_authority
        .lock()
        .unwrap()
        .pointer_query_state(NamespaceId::from_raw(11203));
    assert_eq!(query.mask, 0);
    assert_eq!(query.vertical_scroll_v120, 120);
    assert_eq!(
        (
            query.position.unwrap().root_x,
            query.position.unwrap().root_y
        ),
        (2, 3)
    );
    assert!(waited_for(|| service
        .controller
        .under_common(|authority| authority
            .next_debt(&mut 0)
            .is_some_and(|debt| debt.1.native_reconciled && !debt.1.recipient_settled))
        .unwrap_or(false)));
    assert!(delivery_cell(&service.registry, 112046).is_none());
    assert_eq!(
        read_event(&mut first.peer, 1),
        None,
        "StateOnly has no wire frame"
    );
    let actors = b_finish(
        service,
        &[Arc::clone(&first.custody), Arc::clone(&second.custody)],
    );
    emit_case(
        "B.ordered_input",
        &[
            ("motion", json!({"frame":frames[0],"receipt":receipts[0]})),
            (
                "axis",
                json!({"frames":&frames[1..],"receipt":receipts[1],"scroll":120}),
            ),
            ("state_only", decided.clone()),
            (
                "thaw",
                json!({"control":112047,"decided":decided,"cells":cells.iter().map(|cell|Arc::as_ptr(cell) as usize).collect::<Vec<_>>() }),
            ),
            (
                "immutable_recipient_and_order",
                json!({"original_client":first.client().raw(),"replacement_grab":second.client().raw(),"frames":frames,"source_state":1,"current_state":0,"transport_sequence":first.sequence}),
            ),
        ],
        &actors,
    );
}
