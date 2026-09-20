fn flush_live_native_capsule(capsule: XAuthorityOrderedDelivery) {
    let served = XAuthorityServedConnection::retained(capsule.endpoint().clone());
    let (sender, queue) = sync_channel(1);
    sender.send(capsule).unwrap();
    let (socket, mut recipient) = UnixStream::pair().unwrap();
    let mut in_flight = None;
    let mut refused = None;
    let mut flushed = false;
    for _ in 0..8 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Flushed => {
                flushed = true;
                break;
            }
            X11OrderedServeStep::Advanced => {}
            other => panic!("original capsule did not flush: {other:?}"),
        }
    }
    assert!(flushed);
    let mut bytes = [0; 32];
    recipient.read_exact(&mut bytes).unwrap();
    assert!(matches!(bytes[0], 4 | 5));
}

fn completed_live_native_fixture() -> PreparedOrderedFixture {
    let mut fixture = prepared_ordered_fixture(XServerFrontendClientId(9961));
    attempt_release(&mut fixture, 99610, 272);
    let private = fixture.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(true));
    flush_live_native_capsule(fixture.channels.ordered.try_recv().unwrap());
    assert_eq!(private.record_one_native(), Some(true));
    assert_eq!(private.attempt_one_delivery(), Some(true));
    flush_live_native_capsule(fixture.channels.ordered.try_recv().unwrap());
    assert!(matches!(
        private.settle_one_receipt(),
        Some(PrivateReceiptStep::Settled { debt_settled: true })
    ));
    fixture
}

#[test]
fn live_native_disposal_waits_for_the_original_press_completion_and_native_source() {
    let mut fixture = completed_live_native_fixture();
    let private = fixture.runner.frontend.as_mut().unwrap();
    let press = private.terminal.settling[0]
        .press_custody
        .as_mut()
        .unwrap()
        .completion
        .take()
        .unwrap();
    for _ in 0..8 {
        assert!(!private.terminal.dispose_live_native_one());
    }
    assert_eq!(private.terminal.settling.len(), 1);
    private.terminal.settling[0]
        .press_custody
        .as_mut()
        .unwrap()
        .completion = Some(press);
    let native = private.terminal.settling[0].native.take().unwrap();
    for _ in 0..8 {
        assert!(!private.terminal.dispose_live_native_one());
    }
    assert_eq!(private.terminal.settling.len(), 1);
    private.terminal.settling[0].native = Some(native);
    assert!((0..8).any(|_| private.terminal.dispose_live_native_one()));
    assert!(private.terminal.settling.is_empty());
}

#[test]
fn live_native_disposal_is_charged_before_observing_and_returns_only_the_exact_record() {
    let mut fixture = completed_live_native_fixture();
    let private = fixture.runner.frontend.as_mut().unwrap();
    private.terminal.live_disposal.due = false;
    let mut starts = 0;
    assert!(
        private
            .deliver_one(None, &mut |_, _| {
                starts += 1;
                Err(XServerFrontendRouteError::LifecycleUnavailable)
            })
            .is_err()
    );
    assert_eq!(starts, 1);
    assert_eq!(private.terminal.settling.len(), 1);
    let mut visits = 0;
    for _ in 0..12 {
        let step = private
            .deliver_one(None, &mut |_, _| {
                visits += 1;
                Ok(())
            })
            .unwrap();
        if matches!(step, PrivateDeliveryStep::NativeDisposal { disposed: true }) {
            break;
        }
    }
    assert!(visits >= 2, "one exact dependency pair per visit");
    assert!(private.terminal.settling.is_empty());
    assert!(
        private
            .authority()
            .under_common(|authority| authority.next_debt(&mut 0).is_none())
            .unwrap()
    );
}

fn repeated_live_native_service(key: bool) {
    let (launched, path) = launch_producing(
        if key {
            "live-key-reuse"
        } else {
            "live-pointer-reuse"
        },
        9962,
        4,
    );
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .unwrap();
    let mut client = connect_private_client(&path);
    let window = handshake_ids(&mut client) | 0x0e21;
    let (surface, sequence) = selecting_window(
        &mut client,
        &launched.transactions,
        window,
        3 | (1 << 2) | (1 << 3) | (1 << 6) | (1 << 21),
    );
    let client_id = wait_attached(&launched.registry).cleanup_record().client;
    let owner = launched.owner.clone();
    let lease = owner.lease();
    let control = launched.access.control_producer(&lease).unwrap();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .unwrap();
    assert_eq!(
        apply_focus(&launched, &control, &mut client, client_id, surface, 99620),
        (
            Some(XAuthorityControlOutcome::Delivered),
            Some(expected_focus_in(sequence, window))
        )
    );
    if key {
        ingress
            .submit(
                &lease,
                motion_to(surface, XAuthorityInputDeliveryId::from_raw(99621)),
            )
            .unwrap();
        let mut motion = expected_button_event(true, sequence, window, 1);
        motion[0] = 6;
        motion[1] = 0;
        assert_eq!(read_event(&mut client, 5), Some(motion));
    }
    let cycles = PRIVATE_HOLD_RECORDS + 3;
    for cycle in 0..cycles {
        for pressed in [true, false] {
            let id = 100_000 + (cycle as u64) * 2 + u64::from(!pressed);
            let route = if key {
                key_service_route(surface, id, 42, pressed)
            } else {
                button_to(
                    surface,
                    XAuthorityInputDeliveryId::from_raw(id),
                    272,
                    pressed,
                )
            };
            ingress.submit(&lease, route).unwrap();
            let expected = if key {
                expected_key_service_event(sequence, window, 50, pressed, u16::from(!pressed))
            } else {
                expected_button_event(pressed, sequence, window, 1)
            };
            assert_eq!(
                read_event(&mut client, 5),
                Some(expected),
                "cycle {cycle}, pressed {pressed}"
            );
            let cell = delivery_cell(&launched.registry, id).unwrap();
            assert!(waited_for(|| cell.answer().is_some()));
            assert_eq!(
                cell.answer().unwrap().outcome,
                XAuthorityInputDeliveryOutcome::Flushed
            );
            let _ = launched.deliveries.try_iter().count();
        }
        wait_for_key_service_settlement(&launched);
    }
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "bounded live native reuse");
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    let order = outcome.order.unwrap();
    assert_eq!(order.refused, 0, "{order:?}");
    assert_eq!(order.native_disposed + outcome.terminal.unwrap().1, cycles);
    assert!(
        order.native_disposed >= PRIVATE_HOLD_RECORDS,
        "actual live custody reuse exceeded the native storage bound: {order:?}"
    );
    assert!(outcome.retained_holds.is_empty());
    let _ = std::fs::remove_file(path);
}

#[test]
fn service_live_pointer_records_reuse_beyond_the_preallocated_native_bound() {
    repeated_live_native_service(false);
}

#[test]
fn service_live_modifier_records_reuse_beyond_the_preallocated_native_bound() {
    repeated_live_native_service(true);
}
