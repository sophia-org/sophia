// Integrated capacity, interrupted custody, indeterminate transmission,
// legibility, control cleanup and exact-origin cases for the private routed
// service.
//
// Every admission, producer, runner turn, writer, receipt and settlement
// store below is the production owner the actual service built. What a case
// supplies is the declared store bound it must meet and the acceptance
// facade's labelled runner pause, which holds the real service inside one
// bounded turn so a submission can be made against a runner known not to be
// draining. No component fixture, staged registry or standalone budget stands
// in for the service on any row.

/// The acceptance fixture's declared per-connection input capacity.
const C_INPUT_CAPACITY: usize = 4;

/// The shared order's whole depth, as `PrivateXServerFrontend` sizes it:
/// a full ingress round twice over, plus the cleanup reserve.
const C_ORDER_CAPACITY: usize = C_INPUT_CAPACITY * 2 + PRIVATE_CLEANUP_RESERVE;

/// What ordinary routed input may take of that order.
///
/// The cleanup reserve is kept back from every class that may not use it, so
/// input saturates below the whole depth rather than at it. This is the bound
/// a full-order refusal actually meets.
const C_ORDER_INPUT_DEPTH: usize = C_ORDER_CAPACITY - PRIVATE_CLEANUP_RESERVE;

/// A declared accepted-item bound below the order's input depth, so the
/// store's own reservation is the thing that refuses.
const C_RESERVATION_BOUND: usize = 6;

/// A bound far above it, so the order refuses first and the store never
/// reaches its own.
const C_DEEP_BOUND: usize = 256;

/// Distinct leased producers, so a shared bound is met by contention between
/// real grants rather than by one grant's own completion cell.
const C_PRODUCERS: u64 = 12;

/// Hold the actual service inside one bounded turn.
///
/// LABELLED TEST-ONLY SEAM. The hook is the acceptance facade's own, runs on
/// the service thread inside `serve_order`, and does nothing but wait: the
/// runner, its admission and its store are untouched production state while
/// it is held. Releasing it lets the same loop carry on.
fn hold_runner(service: &LifecycleService) -> Release {
    let (pause, release) = Pause::pair();
    arm_runner(&service.registry, Box::new(move |_, _| pause.wait()));
    release
}

/// The refusal's own name, without the request it hands back.
fn refusal_name(refusal: &PrivateSendError) -> &'static str {
    match refusal {
        PrivateSendError::Denied(_) => "Denied",
        PrivateSendError::Saturated(_) => "Saturated",
        PrivateSendError::Disconnected(_) => "Disconnected",
        PrivateSendError::DeliveryAlreadyTracked(_) => "DeliveryAlreadyTracked",
        PrivateSendError::Unavailable(_) => "Unavailable",
        PrivateSendError::ForeignServiceOwner(_) => "ForeignServiceOwner",
        PrivateSendError::Exhausted(_) => "Exhausted",
    }
}

/// One leased ingress per device, each carrying its own issued grant.
///
/// Taken before any pause: the port is answered by the service's own loop, so
/// asking while the loop is held would wait on the thing being held.
fn leased_producers(
    service: &LifecycleService,
    client: XServerFrontendClientId,
    count: u64,
) -> Vec<PrivateIngress> {
    let lease = service.owner.lease();
    (1..=count)
        .map(|device| {
            service
                .access
                .ingress_for(&lease, client, DeviceId::from_raw(device))
                .expect("an actual leased producer per device")
        })
        .collect()
}

/// The receipt the actual writer published for one delivery.
fn receipt_for(
    deliveries: &Receiver<XAuthorityClientInputDelivery>,
    delivery: u64,
) -> XAuthorityInputDeliveryOutcome {
    let wanted = XAuthorityInputDeliveryId::from_raw(delivery);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match deliveries.recv_timeout(Duration::from_millis(50)) {
            Ok(receipt) if receipt.delivery == wanted => return receipt.outcome,
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    panic!("the actual writer published a receipt for delivery {delivery}");
}

/// One real press and release through a leased producer.
///
/// ACTUAL DELIVERY EVIDENCE for a capacity cycle: the bytes are read off the
/// recipient's own socket and compared against the X protocol encoding of the
/// request that was submitted, and each half's receipt is the one the writer
/// published. Filling work alone would show the bounds without showing that
/// the service carrying them still delivers.
fn press_and_release(
    service: &LifecycleService,
    ingress: &PrivateIngress,
    peer: &mut UnixStream,
    surface: SurfaceId,
    sequence: u16,
    window: u32,
    first_delivery: u64,
) -> Value {
    let lease = service.owner.lease();
    let press = XAuthorityInputDeliveryId::from_raw(first_delivery);
    let release = XAuthorityInputDeliveryId::from_raw(first_delivery + 1);
    ingress
        .submit(&lease, button_to(surface, press, 272, true))
        .expect("an actual press through the leased producer");
    let pressed_bytes = expected_button_event(true, sequence, window, 1);
    assert_eq!(
        read_event(peer, 3),
        Some(pressed_bytes),
        "the press reached the recipient as its exact decided bytes"
    );
    let press_receipt = receipt_for(&service.deliveries, first_delivery);
    assert_eq!(press_receipt, XAuthorityInputDeliveryOutcome::Flushed);
    ingress
        .submit(&lease, button_to(surface, release, 272, false))
        .expect("an actual release through the same grant, once the press settled");
    let released_bytes = expected_button_event(false, sequence, window, 1);
    assert_eq!(
        read_event(peer, 3),
        Some(released_bytes),
        "the release reached the recipient as its exact decided bytes"
    );
    let release_receipt = receipt_for(&service.deliveries, first_delivery + 1);
    assert_eq!(release_receipt, XAuthorityInputDeliveryOutcome::Flushed);
    json!({
        "press_delivery": first_delivery,
        "press_bytes": pressed_bytes.to_vec(),
        "press_receipt": format!("{press_receipt:?}"),
        "release_delivery": first_delivery + 1,
        "release_bytes": released_bytes.to_vec(),
        "release_receipt": format!("{release_receipt:?}"),
    })
}

/// Submit no-held-button releases across the leased producers in turn until
/// the service refuses one.
///
/// EACH IS REAL ACCEPTED WORK: the source takes it, the order carries it, the
/// executor decides it and disposes of it. None of them reaches the recipient,
/// because a release of a button this seat never held has nothing to deliver,
/// which is why each capacity cycle also drives a real press and release for
/// its delivery evidence. What the filling establishes is the bound and the
/// credit, and both are the store's own.
fn fill_until_refused(
    service: &LifecycleService,
    producers: &[PrivateIngress],
    surface: SurfaceId,
    first_delivery: u64,
    ceiling: usize,
) -> (usize, Option<&'static str>) {
    let lease = service.owner.lease();
    let mut accepted = 0;
    for offset in 0..ceiling {
        let ingress = &producers[offset % producers.len()];
        let delivery = XAuthorityInputDeliveryId::from_raw(first_delivery + offset as u64);
        match ingress.submit(&lease, button_to(surface, delivery, 272, false)) {
            Ok(_) => accepted += 1,
            Err(refusal) => return (accepted, Some(refusal_name(&refusal))),
        }
    }
    (accepted, None)
}

#[test]
fn c_capacity() {
    let mut actors = Vec::new();

    // THE SHARED ORDER REFUSES BEFORE THE STORE DOES, keeping its cleanup
    // reserve back from ordinary input, and the credit the refused send had
    // already taken is given back rather than held against work nobody
    // accepted.
    let deep = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
    let mut service =
        LifecycleService::launch_over_store("c-capacity-order", 12000, None, false, 1, deep.clone());
    service.start();
    let (mut peer, custody) = service.connect();
    let (surface, sequence, _focus_ingress) = focus_window(&service, &mut peer, 0x320101, 12000);
    let client = custody.cleanup_record().client;
    assert!(
        waited_for(|| deep.reserved() == Some(0)),
        "the focus control released its own accepted-item credit"
    );
    let producers = leased_producers(&service, client, C_PRODUCERS);
    let held = hold_runner(&service);
    let paused_on = held.entered();
    let (accepted, refusal) =
        fill_until_refused(&service, &producers, surface, 12100, C_ORDER_INPUT_DEPTH + 8);
    let charged_at_refusal = deep.reserved();
    let refusal = refusal.expect("the actual order refused before the store's bound was reached");
    assert_eq!(
        accepted, C_ORDER_INPUT_DEPTH,
        "routed input took the order's whole depth less its cleanup reserve; refusal={refusal}"
    );
    assert_eq!(
        refusal, "Saturated",
        "a full order is retryable saturation, not denial or exhaustion"
    );
    assert_eq!(
        charged_at_refusal,
        Some(C_ORDER_INPUT_DEPTH),
        "the refused send's reservation was rolled back, so only what was accepted is charged"
    );
    assert_eq!(
        (deep.owed(), deep.indeterminate()),
        (Some(0), Some(0)),
        "a refusal before acceptance leaves nothing owed and nothing unproved"
    );
    held.release();
    assert!(
        waited_for(|| deep.reserved() == Some(0)),
        "every accepted item released its own credit once it was actually disposed"
    );
    let order_delivery = press_and_release(
        &service,
        &producers[0],
        &mut peer,
        surface,
        sequence,
        0x320101,
        12190,
    );
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let order_closed = service.closed();
    let full_order = json!({
        "declared_store_bound": C_DEEP_BOUND,
        "order_capacity": C_ORDER_CAPACITY,
        "cleanup_reserve_withheld": PRIVATE_CLEANUP_RESERVE,
        "input_depth": C_ORDER_INPUT_DEPTH,
        "leased_producers": producers.len(),
        "accepted_before_refusal": accepted,
        "refusal": refusal,
        "charged_at_refusal": charged_at_refusal,
        "seam": "acceptance runner pause held on the actual service turn",
        "paused_on": format!("{paused_on:?}"),
        "delivery_after_drain": order_delivery,
        "order": format!("{:?}", order_closed.order),
    });
    actors.extend(service.finish(&[custody]));

    // THE STORE'S OWN DECLARED BOUND REFUSES, repeatedly, and every cycle
    // gives back exactly what it took.
    let bounded = PrivateSettlementOwner::with_capacity(C_RESERVATION_BOUND);
    let mut service = LifecycleService::launch_over_store(
        "c-capacity-bound",
        12001,
        None,
        false,
        1,
        bounded.clone(),
    );
    service.start();
    let (mut peer, custody) = service.connect();
    let (surface, sequence, _focus_ingress) = focus_window(&service, &mut peer, 0x320201, 12001);
    let client = custody.cleanup_record().client;
    assert!(waited_for(|| bounded.reserved() == Some(0)));
    let producers = leased_producers(&service, client, C_PRODUCERS);

    // ONE GRANT HOLDS ONE LIVE REQUEST. Its own completion cell is the
    // reservation, and a second request on the same producer is refused
    // before acceptance while the first still holds it.
    let held = hold_runner(&service);
    let one_grant_paused = held.entered();
    let lease = service.owner.lease();
    producers[0]
        .submit(
            &lease,
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(12190),
                272,
                false,
            ),
        )
        .expect("the first request on a free grant");
    let same_grant = producers[0]
        .submit(
            &lease,
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(12191),
                272,
                false,
            ),
        )
        .map_err(|refusal| refusal_name(&refusal))
        .expect_err("a grant whose one completion cell is held takes no second request");
    let sibling_grant = producers[1]
        .submit(
            &lease,
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(12192),
                272,
                false,
            ),
        )
        .is_ok();
    let charged_per_grant = bounded.reserved();
    assert_eq!(
        same_grant, "Saturated",
        "a busy grant is saturation: it is worth retrying once its request is observed"
    );
    assert!(
        sibling_grant,
        "the bound is that grant's own cell, not the service's"
    );
    assert_eq!(
        charged_per_grant,
        Some(2),
        "only the two accepted requests are charged; the refused one took nothing"
    );
    held.release();
    assert!(waited_for(|| bounded.reserved() == Some(0)));

    let mut cycles = Vec::new();
    let mut remainder = None;
    for cycle in 0..3u64 {
        // ONE HOOK PER CYCLE, so the loop cannot take a turn between the hook
        // that holds it and the hook that measures. The measuring cycle takes
        // its own actual turn from inside the held frame; the others simply
        // hold and let the loop carry on.
        let measuring = cycle == 1;
        let (report, reported) = sync_channel(1);
        let store = bounded.clone();
        let (pause, release) = Pause::pair();
        arm_runner(
            &service.registry,
            Box::new(move |runner, lease| {
                pause.wait();
                if !measuring {
                    return;
                }
                // ONE ACTUAL PRODUCTION TURN on the service's own prepared
                // runner. The item it takes becomes the executor's; the rest
                // stay in the order. Neither gives up a credit for being
                // moved, so the total does not change.
                let before = store.reserved();
                let queued_before = runner
                    .frontend()
                    .admission
                    .ready
                    .lock()
                    .expect("a readable order")
                    .ready
                    .len();
                let progress = runner.service_turn(lease).expect("an actual service turn");
                let after = store.reserved();
                let private = runner.frontend();
                let queued_after = private
                    .admission
                    .ready
                    .lock()
                    .expect("a readable order")
                    .ready
                    .len();
                report
                    .send((
                        before,
                        queued_before,
                        progress.taken,
                        after,
                        queued_after,
                        private.terminal.turn.len(),
                        private.terminal.current.is_some(),
                    ))
                    .expect("the case is waiting for this reading");
            }),
        );
        let entered = release.entered();
        let (accepted, refusal) = fill_until_refused(
            &service,
            &producers,
            surface,
            12200 + cycle * 100,
            C_RESERVATION_BOUND + 4,
        );
        let charged = bounded.reserved();
        let refusal = refusal.expect("the declared store bound refused");
        assert_eq!(
            accepted, C_RESERVATION_BOUND,
            "cycle {cycle} met the exact declared bound"
        );
        assert_eq!(
            charged,
            Some(C_RESERVATION_BOUND),
            "cycle {cycle}: the bound is the charge"
        );
        assert_eq!(refusal, "Saturated", "cycle {cycle}");
        release.release();

        if measuring {
            let (before, queued_before, taken, after, queued_after, in_turn, current) = reported
                .recv_timeout(Duration::from_secs(5))
                .expect("the actual turn reported");
            assert!(
                taken >= 1,
                "the actual turn took work; it is bounded by the service budget, not by one"
            );
            assert_eq!(
                queued_after,
                queued_before - taken,
                "exactly what the turn took left the order"
            );
            assert!(
                queued_after >= 1,
                "a real remainder stayed behind this turn"
            );
            // EACH ITEM'S CREDIT IS ITS OWN. What the turn released is exactly
            // what the turn disposed of; the remainder it did not reach is
            // still charged, and so is anything the executor still owns. No
            // item's completion releases a neighbour's credit, and no credit
            // is released for an item that merely moved.
            assert_eq!(
                after,
                Some(C_RESERVATION_BOUND - (taken - in_turn - usize::from(current))),
                "only the items this turn actually disposed of gave up a credit"
            );
            assert_eq!(
                after,
                Some(queued_after + in_turn + usize::from(current)),
                "the charge is exactly one credit per item still owned, current and remainder alike"
            );
            assert_eq!(before, Some(C_RESERVATION_BOUND));
            remainder = Some(json!({
                "charged_before_turn": before,
                "charged_after_turn": after,
                "taken": taken,
                "queued_before": queued_before,
                "queued_after": queued_after,
                "held_in_turn": in_turn,
                "current_owned": current,
                "seam": "acceptance runner hook took one actual turn inside the held frame",
            }));
        }

        assert!(
            waited_for(|| bounded.reserved() == Some(0)),
            "cycle {cycle} returned every credit it took"
        );
        let delivered = press_and_release(
            &service,
            &producers[0],
            &mut peer,
            surface,
            sequence,
            0x320201,
            12800 + cycle * 10,
        );
        assert!(
            waited_for(|| bounded.reserved() == Some(0)),
            "cycle {cycle} settled its delivered pair too"
        );
        cycles.push(json!({
            "cycle": cycle,
            "accepted": accepted,
            "charged_at_bound": charged,
            "refusal": refusal,
            "entered": format!("{entered:?}"),
            "released_to": bounded.reserved(),
            "actual_delivery": delivered,
        }));
    }
    let remainder = remainder.expect("the second cycle reported its actual turn");
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let bound_closed = service.closed();
    assert_eq!(
        bounded.reserved(),
        Some(0),
        "the invocation ended owing no accepted-item credit"
    );
    actors.extend(service.finish(&[custody]));

    emit_case(
        "C.capacity",
        &[
            ("full_queue_refuses_before_acceptance", full_order),
            (
                "exact_reservation_bound",
                json!({
                    "declared_store_bound": C_RESERVATION_BOUND,
                    "order_input_depth_above_it": C_ORDER_INPUT_DEPTH,
                    "store_bound_cycle": cycles[0].clone(),
                    "one_live_request_per_grant": {
                        "paused_on": format!("{one_grant_paused:?}"),
                        "same_grant_refusal": same_grant,
                        "sibling_grant_accepted": sibling_grant,
                        "charged": charged_per_grant,
                    },
                    "closed": format!("{:?}", bound_closed.order),
                }),
            ),
            ("current_and_remainder_credits", remainder),
            ("repeated_bounded_reuse", json!(cycles)),
        ],
        &actors,
    );
}
