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
                let before = store.reserved();
                let queued_before = runner
                    .frontend()
                    .admission
                    .ready
                    .lock()
                    .expect("a readable order")
                    .ready
                    .len();

                // ONE ITEM, THROUGH THE PRODUCTION STEP THE TURN ITSELF
                // TAKES. A whole turn is allowed a budget that covers
                // everything routed input can put in this order, so a turn
                // can never leave a remainder to look at; stepping once does,
                // and it is the same step, on the same prepared runner, in
                // the service's own frame. The charge is read either side of
                // it, so what is compared is a move and nothing else.
                let stepped = {
                    let PrivatePreparedRunner {
                        frontend,
                        keyboards,
                        watch,
                        ..
                    } = &mut *runner;
                    let private = frontend.as_mut().expect("the live runner");
                    private
                        .step_once(
                            keyboards,
                            &mut |_, _| Ok(()),
                            watch.as_ref().expect("its own supervisor"),
                        )
                        .map(|step| {
                            match step {
                                PrivateOrderedStep::Decided(_) => "Decided",
                                PrivateOrderedStep::Deferred { .. } => "Deferred",
                                PrivateOrderedStep::Resumed { .. } => "Resumed",
                                PrivateOrderedStep::Blocked(_) => "Blocked",
                                PrivateOrderedStep::Idle => "Idle",
                                // Named as what they are rather than folded
                                // into the ones above: a step that took an
                                // item and a step that did not are the whole
                                // point of this reading.
                                _ => "OtherStep",
                            }
                            .to_owned()
                        })
                        .map_err(|error| format!("{error:?}"))
                };
                let after_step = store.reserved();
                let (queued_after_step, owned_after_step) = {
                    let private = runner.frontend();
                    (
                        private
                            .admission
                            .ready
                            .lock()
                            .expect("a readable order")
                            .ready
                            .len(),
                        private.terminal.turn.len()
                            + usize::from(private.terminal.current.is_some()),
                    )
                };

                // Then the actual turn, which finishes what it can.
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
                        stepped,
                        after_step,
                        queued_after_step,
                        owned_after_step,
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
            let (
                before,
                queued_before,
                stepped,
                after_step,
                queued_after_step,
                owned_after_step,
                taken,
                after,
                queued_after,
                in_turn,
                current,
            ) = reported
                .recv_timeout(Duration::from_secs(5))
                .expect("the actual step and turn reported");
            let stepped = stepped.expect("the production step decided its item");

            // THE DISCRIMINATOR, and it is a state that actually existed: one
            // item is the executor's, the rest are still in the order, and the
            // charge has not moved, because moving an item is not disposing of
            // it and no item's credit answers for another's.
            assert_eq!(
                queued_after_step,
                queued_before - 1,
                "exactly one item left the order on one production step"
            );
            assert_eq!(
                owned_after_step, 1,
                "and the executor owns exactly that one: {stepped}"
            );
            assert!(
                queued_after_step >= 1,
                "with a real remainder behind it, because the declared bound exceeds one"
            );
            assert_eq!(
                after_step, before,
                "the current item and the remainder each still hold their own credit"
            );
            assert_eq!(after_step, Some(C_RESERVATION_BOUND));

            assert!(
                taken + 1 >= 1,
                "the actual turn then ran, bounded by the service budget"
            );
            assert_eq!(
                queued_after,
                queued_after_step - taken,
                "exactly what the turn took left the order"
            );
            // EACH ITEM'S CREDIT IS ITS OWN. What the turn released is exactly
            // what the turn disposed of; the remainder it did not reach is
            // still charged, and so is anything the executor still owns. No
            // item's completion releases a neighbour's credit, and no credit
            // is released for an item that merely moved.
            assert_eq!(
                after,
                Some(queued_after + in_turn + usize::from(current)),
                "the charge is exactly one credit per item still owned, current and remainder alike"
            );
            assert_eq!(before, Some(C_RESERVATION_BOUND));
            remainder = Some(json!({
                "charged_before": before,
                "queued_before": queued_before,
                "production_step": stepped,
                "charged_after_one_step": after_step,
                "queued_after_one_step": queued_after_step,
                "owned_by_executor_after_one_step": owned_after_step,
                "taken_by_the_following_turn": taken,
                "charged_after_turn": after,
                "queued_after_turn": queued_after,
                "held_in_turn": in_turn,
                "current_owned": current,
                "seam": "acceptance runner hook, inside the held frame: one production step, then the service's own turn",
                "why_a_step": "the turn's budget covers everything routed input can put in this order, so only a single step leaves a current item and a remainder at the same instant",
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

/// What one interrupted invocation left behind, read through the store the
/// service ran over and the custody its owner kept.
#[derive(Debug)]
struct Interrupted {
    owed: usize,
    outstanding: Option<usize>,
    indeterminate: Option<usize>,
    terminal_inventories: usize,
    origin_retained: bool,
    charged: Option<usize>,
    /// What the retained inventory of this service's own origin still carries.
    holds: usize,
    /// Whose window each retained hold reached, so the retained work is
    /// compared by identity and not only counted.
    hold_identities: Vec<(u64, u64)>,
    settling: usize,
    current: bool,
    turn: usize,
    delivering: usize,
    undelivered: usize,
    pending_custody: bool,
}

/// Read the durable store's retained inventory without taking anything.
fn interrupted_custody(service: &LifecycleService) -> Interrupted {
    let held = service
        .owner
        .store
        .inner
        .lock()
        .expect("a readable store after the invocation ended");
    let mine = held
        .terminal
        .iter()
        .find(|inventory| Arc::ptr_eq(&inventory.origin.clients, &service.registry.clients));
    let mut interrupted = Interrupted {
        owed: held.held.len(),
        outstanding: None,
        indeterminate: None,
        terminal_inventories: held.terminal.len(),
        origin_retained: mine.is_some(),
        charged: None,
        holds: mine.map_or(0, |inventory| inventory.holds.len()),
        hold_identities: mine.map_or_else(Vec::new, |inventory| {
            inventory
                .holds
                .iter()
                .map(|hold| (hold.reached.client.0, hold.reached.window.local.raw()))
                .collect()
        }),
        settling: mine.map_or(0, |inventory| inventory.settling.len()),
        current: mine.is_some_and(|inventory| inventory.current.is_some()),
        turn: mine.map_or(0, |inventory| inventory.turn.len()),
        delivering: mine.map_or(0, |inventory| inventory.delivering.len()),
        undelivered: mine.map_or(0, |inventory| inventory.undelivered.len()),
        pending_custody: mine.is_some_and(|inventory| inventory.pending_custody.is_some()),
    };
    drop(held);
    interrupted.outstanding = service.owner.store.outstanding();
    interrupted.indeterminate = service.owner.store.indeterminate();
    interrupted.charged = service.owner.store.reserved();
    interrupted
}

#[test]
fn c_interrupted_ownership() {
    let mut actors = Vec::new();
    let mut facts = Vec::new();
    for (index, kind) in ["return", "error", "unwind"].into_iter().enumerate() {
        let store = PrivateSettlementOwner::with_capacity(C_RESERVATION_BOUND);
        let mut service = LifecycleService::launch_over_store(
            kind,
            12010 + index as u64,
            None,
            kind == "unwind",
            1,
            store.clone(),
        );
        service.start();
        let (mut peer, custody) = service.connect();
        let window = 0x320301 + index as u32 * 0x100;
        let (surface, sequence, ingress) = focus_window(&service, &mut peer, window, 12010);
        let client = custody.cleanup_record().client;

        // THE KEY SOURCE'S POINTER OBSERVATION comes only from a real pointer
        // press and release through this same producer. Nothing here writes a
        // query scope or substitutes a history for it.
        let pointer_pair = press_and_release(
            &service,
            &ingress,
            &mut peer,
            surface,
            sequence,
            window,
            12050 + index as u64 * 10,
        );

        // ONE REAL KEYBOARD HISTORY, established through the actual source and
        // written to the recipient's socket, so what outlives the runner is a
        // history this invocation actually made.
        let key = 12100 + index as u64 * 10;
        ingress
            .submit(&service.owner.lease(), key_service_route(surface, key, 42, true))
            .expect("an actual held key press");
        let key_bytes = expected_key_service_event(sequence, window, 50, true, 0);
        assert_eq!(read_event(&mut peer, 3), Some(key_bytes));
        assert_eq!(
            receipt_for(&service.deliveries, key),
            XAuthorityInputDeliveryOutcome::Flushed
        );
        assert!(waited_for(|| store.reserved() == Some(0)));

        // The unwind is armed from the recipient's own drawn surface before
        // the runner is held: the service must reach its raster wait itself.
        let drawn = (kind == "unwind").then(|| {
            let drawn = draw_and_learn_surface(&mut peer, &service.transactions);
            assert!(waited_for(|| saw_kind(
                &service.telemetry,
                XAuthorityBackpressureTelemetryKind::Wait,
                true
            )));
            drawn
        });

        let identity = custody_identity(&custody);
        let producers = leased_producers(&service, client, C_PRODUCERS);
        let held = hold_runner(&service);
        let entered = held.entered();
        let first = 12200 + index as u64 * 100;
        let (accepted, refusal) = fill_until_refused(
            &service,
            &producers,
            surface,
            first,
            C_RESERVATION_BOUND + 4,
        );
        assert_eq!(accepted, C_RESERVATION_BOUND);
        assert_eq!(refusal, Some("Saturated"));
        // THE EXACT CELLS THIS INVOCATION MINTED, held by the case so that
        // what is compared afterwards is the completion the accepted request
        // was given and not a lookup that could answer with a successor.
        let cells: Vec<Arc<PrivateDeliveryCompletion>> = (0..accepted as u64)
            .map(|offset| {
                delivery_cell(&service.registry, first + offset)
                    .expect("the accepted request's own completion")
            })
            .collect();
        assert!(
            cells.iter().all(|cell| cell.answer().is_none()),
            "nothing accepted has been answered while the runner is held"
        );
        let charged_before_exit = store.reserved();
        assert_eq!(charged_before_exit, Some(C_RESERVATION_BOUND));

        match kind {
            "return" => service.command(XServerFrontendServiceCommand::StopAndDisconnect),
            "error" => {
                let (acknowledgement, acknowledged) = sync_channel(1);
                drop(acknowledged);
                service.command(XServerFrontendServiceCommand::UpdateOutputTopology {
                    snapshot: sophia_protocol::OutputTopologySnapshot {
                        generation: 1,
                        primary: sophia_protocol::OutputId::from_raw(1),
                        outputs: Vec::new(),
                    },
                    acknowledgement,
                });
            }
            "unwind" => {
                service
                    .raster
                    .try_route(raster_requirement_for(drawn.expect("a drawn surface")))
                    .expect("the raster requirement reached the service");
            }
            _ => unreachable!(),
        }
        held.release();
        let closed = service.closed();
        assert_eq!(closed.unwound, kind == "unwind");
        assert_eq!(closed.error.is_some(), kind == "error");

        // THE ORIGINAL KEYBOARD HISTORY OUTLIVED THE RUNNER that was using it.
        assert_eq!(
            closed.modifiers,
            Some(1),
            "{kind}: the held Shift is the same history, not a neutral rebuild"
        );
        // THE EXACT CUSTODY, unchanged by the interruption.
        assert_eq!(custody_identity(&custody), identity, "{kind}");
        let retained = interrupted_custody(&service);
        assert!(
            retained.origin_retained,
            "{kind}: the retained inventory names the registry that accepted the work"
        );
        // THE UNFINISHED WORK ITSELF IS STILL HERE. The held key was never
        // released, so its native obligation and the delivery custody that
        // carries it are retained rather than settled by the interruption.
        // THE EXACT HELD KEY, by identity. Not a count of holds: the retained
        // work is this connection's press on this window, and nothing else.
        assert_eq!(
            retained.hold_identities,
            vec![(client.0, u64::from(window))],
            "{kind}: the retained hold is this invocation's own, unresolved: {retained:?}"
        );
        assert_eq!(
            retained.terminal_inventories, 1,
            "{kind}: exactly this invocation's inventory reached the store, whole: {retained:?}"
        );
        assert!(
            !retained.pending_custody,
            "{kind}: no half-taken pending custody was left behind by the interruption: {retained:?}"
        );
        assert!(
            retained.charged.is_some(),
            "{kind}: the store says what it holds rather than answering zero for unreadable"
        );
        // Whatever the exit could not finish stays owned, and every credit the
        // store still reports is one this invocation actually took.
        // EVERY CREDIT COVERS A RETAINED ITEM, AND EVERY RETAINED ITEM ONE
        // CREDIT. Which list the release is sitting in when the interruption
        // lands is the executor's business and changes between revisions; what
        // may never change is that the count of charges equals the count of
        // things still owed. A settled item releases its own credit and no
        // other's, and nothing is released for an item still owned.
        let carried = retained.owed
            + retained.outstanding.unwrap_or_default()
            + retained.indeterminate.unwrap_or_default()
            + retained.turn
            + retained.delivering
            + retained.undelivered
            + usize::from(retained.current);
        assert_eq!(
            retained.charged,
            Some(carried),
            "{kind}: the charge is exactly what is still owed, no more and no less: {retained:?}"
        );
        let answered = cells.iter().filter(|cell| cell.answer().is_some()).count();
        assert!(
            answered < accepted,
            "{kind}: work the interrupted invocation never ran is not answered by the interruption"
        );
        facts.push(json!({
            "exit": kind,
            "accepted_before_exit": accepted,
            "charged_before_exit": charged_before_exit,
            "held_on": format!("{entered:?}"),
            "pointer_observation": pointer_pair,
            "original_key_bytes": key_bytes.to_vec(),
            "original_modifiers_after_exit": closed.modifiers,
            "custody_identity": format!("{identity:?}"),
            "retained": format!("{retained:?}"),
            "retained_inventories": retained.terminal_inventories,
            "retained_hold_identities": format!("{:?}", retained.hold_identities),
            "retained_stage_settling": retained.settling,
            "retained_stage_turn": retained.turn,
            "retained_pending_custody": retained.pending_custody,
            "original_cells_answered": answered,
            "original_cells_held": cells.len(),
            "closed_error": closed.error.clone(),
            "order": format!("{:?}", closed.order),
        }));
        drop(cells);
        actors.extend(service.finish(&[custody]));
    }

    emit_case(
        "C.interrupted_ownership",
        &[
            ("return", facts[0].clone()),
            ("error", facts[1].clone()),
            ("unwind", facts[2].clone()),
            (
                "original_completion_and_origin",
                json!({"all_exits": facts}),
            ),
            (
                "runner_loss_keyboard_custody",
                json!({"all_exits": facts}),
            ),
        ],
        &actors,
    );
}

/// The actual service's own shared admission, taken from inside its frame.
///
/// LABELLED TEST-ONLY SEAM, and a read: the hook clones the handle the running
/// service is using and returns, so nothing is substituted for it.
fn admission_of(service: &LifecycleService) -> Arc<SharedAdmission> {
    let (found, taken) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, _| {
            found
                .send(Arc::clone(&runner.frontend().admission))
                .expect("the case is waiting for this handle");
        }),
    );
    taken
        .recv_timeout(Duration::from_secs(5))
        .expect("the actual service's own admission")
}

/// Poison one lock and nothing else, through a caught unwind.
fn poison_lock<T>(lock: &Mutex<T>, what: &'static str) {
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = lock.lock().expect("a readable lock before this case poisons it");
        panic!("labelled acceptance poison of {what}");
    }));
    assert!(poisoned.is_err(), "the poisoning unwind happened here");
    assert!(lock.is_poisoned(), "{what} is now unreadable");
}

#[test]
fn c_poison() {
    let store = PrivateSettlementOwner::with_capacity(C_RESERVATION_BOUND);
    let mut service =
        LifecycleService::launch_over_store("c-poison", 12020, None, false, 1, store.clone());
    service.start();
    let (mut peer, custody) = service.connect();
    let (surface, sequence, ingress) = focus_window(&service, &mut peer, 0x320401, 12020);
    let client = custody.cleanup_record().client;
    let delivered = press_and_release(
        &service,
        &ingress,
        &mut peer,
        surface,
        sequence,
        0x320401,
        12030,
    );
    assert!(waited_for(|| store.reserved() == Some(0)));

    let admission = admission_of(&service);
    let producers = leased_producers(&service, client, C_PRODUCERS);
    let held = hold_runner(&service);
    let entered = held.entered();
    // One credit is deliberately left free, so what the next send meets is the
    // unreadable order itself rather than the store's bound in front of it.
    let (accepted, refusal) =
        fill_until_refused(&service, &producers, surface, 12200, C_RESERVATION_BOUND - 1);
    assert_eq!(accepted, C_RESERVATION_BOUND - 1);
    assert_eq!(refusal, None, "the store still has a credit to give");
    let charged_before = store.reserved();
    assert_eq!(charged_before, Some(C_RESERVATION_BOUND - 1));

    // THE ORDER ITSELF BECOMES UNREADABLE while it holds accepted work. Not
    // drained, not closed: unreadable, which is a different fact from empty.
    poison_lock(&admission.ready, "the actual shared order");

    // A producer meeting an unreadable order is refused, and told so: nothing
    // is accepted, and the refusal is not saturation or a closed consumer.
    // A producer whose own grant is free, so what it meets is the order.
    let after_poison = producers[accepted]
        .submit(
            &service.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(12290),
                272,
                false,
            ),
        )
        .map_err(|refusal| refusal_name(&refusal))
        .expect_err("an unreadable order accepts nothing");
    assert_eq!(
        after_poison, "Unavailable",
        "an unreachable order and a full one are different answers"
    );
    assert_eq!(
        store.reserved(),
        charged_before,
        "the refused send took no credit and released none"
    );

    held.release();
    let closed = service.closed();

    // THE UNREADABLE QUEUE IS RETAINED AS WHAT IT IS. It is not reported
    // drained, and its instance is not reported settled.
    let failed_instances = store.failed_instances();
    let failure_slots = store.failure_slots_charged();
    let charged_after = store.reserved();
    let owed = store.owed();
    let terminal = store.terminal_inventories();
    assert_eq!(
        failed_instances,
        Some(1),
        "the instance whose order could not be read is kept, not counted and dropped"
    );
    assert_eq!(
        failure_slots,
        Some(1),
        "its failure slot, reserved before exposure, is still charged"
    );
    assert!(
        charged_after.is_some_and(|charged| charged >= accepted),
        "accepted work behind an unreadable order keeps every credit it took: {charged_after:?}"
    );
    assert!(
        owed.is_some() && terminal.is_some(),
        "the store still answers; unreadable elsewhere is not unreadable here"
    );

    let legible = json!({
        "order_poisoned": true,
        "producer_refusal_after_poison": after_poison,
        "charged_before_poison": charged_before,
        "charged_after_exit": charged_after,
        "failed_instances": failed_instances,
        "failure_slots_charged": failure_slots,
        "owed": owed,
        "terminal_inventories": terminal,
        "closed_error": closed.error.clone(),
        "held_on": format!("{entered:?}"),
        "actual_delivery_before_poison": delivered,
    });
    let actors = service.finish(&[custody]);

    // THE STORE ITSELF, made unreadable. Every reader now says it cannot say,
    // rather than answering nothing owed; the credits are still there.
    let readable_before = (store.reserved(), store.owed(), store.outstanding());
    poison_lock(&store.inner, "the actual settlement store");
    let readable_after = (store.reserved(), store.owed(), store.outstanding());
    let through_poison = {
        let records = store.records_even_if_poisoned();
        (records.reserved, records.held.len(), records.failed.len())
    };
    assert_eq!(
        readable_after,
        (None, None, None),
        "an unreadable store says so rather than answering zero"
    );
    assert_eq!(
        through_poison.0,
        readable_before.0.expect("readable before"),
        "the credits an unreadable store holds are exactly the ones it held"
    );
    assert!(
        through_poison.2 >= 1,
        "and the failed instance it was keeping is still kept"
    );

    emit_case(
        "C.poison",
        &[
            (
                "unreadable_remains_legible",
                json!({
                    "order": legible.clone(),
                    "store_readable_before": format!("{readable_before:?}"),
                    "store_readable_after": format!("{readable_after:?}"),
                    "store_through_poison_reserved_held_failed": format!("{through_poison:?}"),
                }),
            ),
            (
                "unreadable_keeps_capacity",
                json!({
                    "accepted_behind_unreadable_order": accepted,
                    "charged_before_poison": charged_before,
                    "charged_after_exit": charged_after,
                    "failure_slot_still_charged": failure_slots,
                    "store_credits_through_poison": through_poison.0,
                }),
            ),
            (
                "unreadable_is_not_settled",
                json!({
                    "failed_instances_retained": failed_instances,
                    "refusal_is_unavailable_not_saturated": after_poison,
                    "store_answers_none_not_zero": format!("{readable_after:?}"),
                    "retained_failed_through_poison": through_poison.2,
                }),
            ),
        ],
        &actors,
    );
}

#[test]
fn c_exact_origin() {
    // TWO ACTUAL INVOCATIONS, each over its own owner and store, so the
    // origins really are different rather than two names for one.
    let store_a = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
    let store_b = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
    let mut first =
        LifecycleService::launch_over_store("c-origin-a", 12040, None, false, 1, store_a.clone());
    let mut second =
        LifecycleService::launch_over_store("c-origin-b", 12041, None, false, 1, store_b.clone());
    first.start();
    second.start();
    let (mut peer_a, custody_a) = first.connect();
    let (mut peer_b, custody_b) = second.connect();
    let (surface_a, sequence_a, ingress_a) = focus_window(&first, &mut peer_a, 0x320501, 12040);
    let (surface_b, _sequence_b, ingress_b) = focus_window(&second, &mut peer_b, 0x320601, 12041);
    let client_a = custody_a.cleanup_record().client;
    let client_b = custody_b.cleanup_record().client;

    // A COLLIDING LOCAL IDENTITY. Both invocations numbered their connection
    // the same; what tells them apart is the origin, never the number.
    assert_eq!(client_a, client_b, "the two invocations collide on the number");
    assert!(
        !Arc::ptr_eq(&first.registry.clients, &second.registry.clients),
        "and are still different origins"
    );

    // A FOREIGN KEEPER IS REFUSED AT EVERY ACCEPTANCE, on the association and
    // not on the work: nothing is accepted, reserved or consumed.
    let foreign_input = ingress_b
        .submit(
            &first.owner.lease(),
            button_to(
                surface_b,
                XAuthorityInputDeliveryId::from_raw(12290),
                272,
                false,
            ),
        )
        .map_err(|refusal| refusal_name(&refusal))
        .expect_err("a producer will not accept work for a keeper that is not its own");
    assert_eq!(foreign_input, "ForeignServiceOwner");
    let foreign_control = second
        .access
        .control_producer(&first.owner.lease())
        .map(|_| ())
        .expect_err("nor will the port issue a control producer to a foreign keeper");
    assert!(
        matches!(foreign_control, PrivateProducerRefusal::ForeignServiceOwner),
        "{foreign_control:?}"
    );
    // The same request, offered to its own keeper, is accepted: what was
    // refused above is the association and nothing about the work.
    ingress_b
        .submit(
            &second.owner.lease(),
            button_to(
                surface_b,
                XAuthorityInputDeliveryId::from_raw(12291),
                272,
                false,
            ),
        )
        .expect("its own keeper accepts the very same request");
    let charged_b_unmoved = store_a.reserved();
    assert_eq!(
        charged_b_unmoved,
        Some(0),
        "the other origin's store was neither charged nor credited by any of this"
    );

    // A COLLIDING NUMBER CANNOT REACH THE OTHER ORIGIN'S CUSTODY either.
    let foreign_turn_refusal = {
        let (found, taken) = sync_channel(1);
        let foreign = Arc::clone(&second.owner);
        arm_runner(
            &first.registry,
            Box::new(move |runner, _| {
                let refusal = runner
                    .service_turn(&foreign.lease())
                    .map(|_| String::from("accepted"))
                    .unwrap_or_else(|error| format!("{error:?}"));
                found.send(refusal).expect("the case is waiting");
            }),
        );
        taken
            .recv_timeout(Duration::from_secs(5))
            .expect("the actual runner answered a foreign lease")
    };
    assert!(
        foreign_turn_refusal.contains("ForeignServiceOwner"),
        "the turn itself refuses a foreign keeper: {foreign_turn_refusal}"
    );

    // NATIVE SIBLING-GRAB CONTINUATION, through the actual source and writer.
    // The first release keeps the shared activation while the other button
    // holds it, and is answered only once the final retirement is visited.
    let mut submitted = Vec::new();
    let mut events = Vec::new();
    for (delivery, button) in [(12300, 272), (12301, 274)] {
        submitted.push(
            ingress_a
                .submit(
                    &first.owner.lease(),
                    button_to(
                        surface_a,
                        XAuthorityInputDeliveryId::from_raw(delivery),
                        button,
                        true,
                    ),
                )
                .is_ok(),
        );
        events.push(read_event(&mut peer_a, 3));
    }
    submitted.push(
        ingress_a
            .submit(
                &first.owner.lease(),
                button_to(
                    surface_a,
                    XAuthorityInputDeliveryId::from_raw(12302),
                    272,
                    false,
                ),
            )
            .is_ok(),
    );
    let early_wire = read_event(&mut peer_a, 1);
    let early_answer = delivery_cell(&first.registry, 12302).and_then(|cell| cell.answer());
    assert_eq!(
        early_wire, None,
        "a release still required by another button sends nothing"
    );
    assert_eq!(
        early_answer, None,
        "and its own recipient half is unanswered too"
    );
    submitted.push(
        ingress_a
            .submit(
                &first.owner.lease(),
                button_to(
                    surface_a,
                    XAuthorityInputDeliveryId::from_raw(12303),
                    274,
                    false,
                ),
            )
            .is_ok(),
    );
    events.push(read_event(&mut peer_a, 3));
    events.push(read_event(&mut peer_a, 3));
    assert!(submitted.iter().all(|accepted| *accepted), "{submitted:?}");
    assert_eq!(
        events,
        vec![
            Some(expected_chord_event(true, sequence_a, 0x320501, 1, 0)),
            Some(expected_chord_event(true, sequence_a, 0x320501, 2, 1 << 8)),
            Some(expected_chord_event(
                false,
                sequence_a,
                0x320501,
                1,
                (1 << 8) | (1 << 9)
            )),
            Some(expected_chord_event(false, sequence_a, 0x320501, 2, 1 << 9)),
        ],
        "the continuation delivered both releases in the order the source decided"
    );
    let answers: Vec<_> = [12300, 12301, 12302, 12303]
        .into_iter()
        .map(|delivery| {
            delivery_cell(&first.registry, delivery).and_then(|cell| {
                waited_for(|| cell.answer().is_some());
                cell.answer().map(|answer| answer.outcome)
            })
        })
        .collect();
    assert_eq!(
        answers,
        vec![Some(XAuthorityInputDeliveryOutcome::Flushed); 4],
        "every half of the shared activation was answered by an actual receipt"
    );

    second.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed_b = second.closed();
    first.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed_a = first.closed();
    let joined = closed_a
        .order
        .expect("the first invocation returned its tally")
        .activations_joined;
    assert!(
        joined >= 1,
        "the sibling grab's retirement was visited by the actual runner, not inferred"
    );

    // REPEATED RECOVERY KEEPS WHAT IT HOLDS. Restoring and driving again
    // answers nothing further, releases no credit and renames no outcome.
    let restored_first = store_a.restore_interrupted();
    let credits_first = store_a.reserved();
    let executions_first = store_a.retained_executions();
    let restored_second = store_a.restore_interrupted();
    let credits_second = store_a.reserved();
    let executions_second = store_a.retained_executions();
    assert_eq!(
        restored_second, 0,
        "an interrupted sweep is returned once, not repeatedly"
    );
    assert_eq!(credits_first, credits_second);
    assert_eq!(executions_first, executions_second);
    let drive_first = store_a.drive();
    let credits_after_drive = store_a.reserved();
    let drive_second = store_a.drive();
    assert!(
        drive_first.readable && drive_second.readable,
        "a drive that could not look says so"
    );
    assert_eq!(
        drive_second.answered, 0,
        "the second drive answered nothing the first had already answered"
    );
    assert_eq!(
        store_a.reserved(),
        credits_after_drive,
        "and released no further credit"
    );
    assert_eq!(
        store_a.retained_executions(),
        executions_second,
        "the retained outcome identity is unchanged by repeating the recovery"
    );

    let first_origin = format!("{:p}", Arc::as_ptr(&first.registry.clients));
    let second_origin = format!("{:p}", Arc::as_ptr(&second.registry.clients));
    let mut actors = first.finish(&[custody_a]);
    actors.extend(second.finish(&[custody_b]));
    emit_case(
        "C.exact_origin",
        &[
            (
                "foreign_origin",
                json!({
                    "input_refusal": foreign_input,
                    "control_refusal": format!("{foreign_control:?}"),
                    "turn_refusal": foreign_turn_refusal,
                    "same_request_accepted_by_own_keeper": true,
                    "other_store_untouched": charged_b_unmoved,
                }),
            ),
            (
                "colliding_origin",
                json!({
                    "client_number": client_a.0,
                    "same_number_on_both": client_a == client_b,
                    "distinct_client_tables": true,
                    "first_origin": first_origin,
                    "second_origin": second_origin,
                    "closed_second": format!("{:?}", closed_b.order),
                }),
            ),
            (
                "repeated_recovery_identity_and_credit",
                json!({
                    "restore_first": restored_first,
                    "restore_second": restored_second,
                    "credits_first": credits_first,
                    "credits_second": credits_second,
                    "drive_first": format!("{drive_first:?}"),
                    "drive_second": format!("{drive_second:?}"),
                    "executions": format!("{executions_second:?}"),
                }),
            ),
            (
                "native_sibling_grab_continuation",
                json!({
                    "early_release_wire": early_wire.map(|bytes| bytes.to_vec()),
                    "early_release_answer": early_answer.map(|answer| format!("{answer:?}")),
                    "chord_bytes": events
                        .iter()
                        .map(|event| event.map(|bytes| bytes.to_vec()))
                        .collect::<Vec<_>>(),
                    "receipts": format!("{answers:?}"),
                    "activations_joined": joined,
                    "order": format!("{:?}", closed_a.order),
                }),
            ),
        ],
        &actors,
    );
}



/// The nine named control kinds, each as the command a producer actually
/// submits. Ordered so the two that can end the recipient's connection come
/// last, and the ones that change nothing about it come first.
fn every_control_kind(
    client: XServerFrontendClientId,
    surface: SurfaceId,
    first_transaction: u64,
) -> Vec<(XAuthorityControlKind, XAuthorityClientControlCommand)> {
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 8,
        height: 8,
    };
    let state = sophia_protocol::PolicyPresentationState {
        fullscreen: false,
        maximized: true,
        minimized: false,
    };
    let commands = [
        XAuthorityControlCommand::PublishMetadataRule {
            transaction: TransactionId::from_raw(first_transaction),
            surface,
            rule: sophia_protocol::MetadataDisclosureRule {
                surface,
                disclosure: sophia_protocol::MetadataDisclosure::ClassOnly,
                trust_level: sophia_protocol::TrustLevel::Trusted,
                icon: None,
                generation: 1,
            },
        },
        XAuthorityControlCommand::AdmitSurface {
            transaction: TransactionId::from_raw(first_transaction + 1),
            surface,
            geometry,
        },
        XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(first_transaction + 2),
            surface,
            geometry: Rect {
                width: 16,
                height: 16,
                ..geometry
            },
        },
        XAuthorityControlCommand::SetPresentationState {
            transaction: TransactionId::from_raw(first_transaction + 3),
            surface,
            state,
        },
        XAuthorityControlCommand::RestorePresentationState {
            transaction: TransactionId::from_raw(first_transaction + 4),
            surface,
            state: sophia_protocol::PolicyPresentationState {
                maximized: false,
                ..state
            },
        },
        XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(first_transaction + 5),
            surface,
        },
        XAuthorityControlCommand::ClearFocus {
            transaction: TransactionId::from_raw(first_transaction + 6),
            surface,
        },
        XAuthorityControlCommand::WithdrawSurface {
            transaction: TransactionId::from_raw(first_transaction + 7),
            surface,
        },
        XAuthorityControlCommand::CloseSurface {
            transaction: TransactionId::from_raw(first_transaction + 8),
            surface,
        },
    ];
    commands
        .into_iter()
        .map(|command| (command.kind(), XAuthorityClientControlCommand { client, command }))
        .collect()
}

/// One armed interruption of an ordered handover, keyed by the exact origin
/// and delivery it belongs to.
///
/// NEVER BY A CLIENT NUMBER ALONE: two live origins can hold the same number,
/// and a seam that matched on it would fire inside somebody else's handover.
type HandoverSeam = Box<dyn FnOnce() + Send>;
static HANDOVER_SEAMS: Mutex<Vec<(usize, Option<XAuthorityInputDeliveryId>, HandoverSeam)>> =
    Mutex::new(Vec::new());

/// Arm the next handover of this delivery on this origin, once.
fn arm_handover(
    registry: &XServerFrontendRouteRegistry,
    delivery: XAuthorityInputDeliveryId,
    seam: HandoverSeam,
) {
    HANDOVER_SEAMS.lock().unwrap().push((
        Arc::as_ptr(&registry.clients) as usize,
        Some(delivery),
        seam,
    ));
}

/// Production's entry into that seam, immediately after the handover returns
/// and before anything is written down about it. Empty unless a case armed
/// this exact origin and this exact delivery.
pub(crate) fn after_ordered_handover(
    registry: &XServerFrontendRouteRegistry,
    delivery: Option<XAuthorityInputDeliveryId>,
) {
    let origin = Arc::as_ptr(&registry.clients) as usize;
    let seam = {
        let mut seams = HANDOVER_SEAMS.lock().unwrap();
        seams
            .iter()
            .position(|(candidate, wanted, _)| *candidate == origin && *wanted == delivery)
            .map(|at| seams.remove(at).2)
    };
    if let Some(seam) = seam {
        seam();
    }
}

/// Finish one invocation, naming it.
///
/// `LifecycleService::finish` asserts on join evidence a stopped and
/// collected invocation already has; its precondition is actual service exit.
/// Three invocations sharing that helper produced one unlabelled assertion,
/// and it was read as belonging to the wrong one. This says which.
fn finish_labelled(
    what: &str,
    service: LifecycleService,
    custodies: &[Arc<PrivateEvidenceCustody>],
) -> Vec<String> {
    for custody in custodies {
        if custody.ever_started() {
            assert_eq!(
                custody.join().phase(),
                PrivateReapingPhase::Joined,
                "{what}: this invocation must have exited and been collected before it is finished"
            );
        }
    }
    service.finish(custodies)
}

/// What one retained release still says about its own transmission.
fn retained_dispatch(service: &LifecycleService) -> Vec<(String, bool, bool)> {
    let held = service
        .owner
        .store
        .inner
        .lock()
        .expect("a readable store");
    let seen = held
        .terminal
        .iter()
        .filter(|inventory| Arc::ptr_eq(&inventory.origin.clients, &service.registry.clients))
        .flat_map(|inventory| {
            inventory.settling.iter().map(|release| {
                (
                    format!("{:?}", release.custody.dispatch),
                    release.custody.pending.is_some(),
                    release.completion().is_some_and(|cell| cell.answer().is_some()),
                )
            })
        })
        .collect();
    drop(held);
    seen
}

/// One step of the actual writer, as it happened, for the delivery it was
/// serving.
///
/// WHOLE FRAMES, NOT BYTES. `advanced` names a frame this delivery owed that
/// went out entire. The writer's own byte offset within a frame is not
/// reported here, so nothing built from these may claim a partial-byte
/// prefix; what they establish is how much of one capsule was committed.
#[derive(Clone, Debug)]
struct ObservedFrame {
    delivery: Option<XAuthorityInputDeliveryId>,
    frames: usize,
    index: usize,
    advanced: Option<usize>,
    failure: Option<String>,
}

static OBSERVED_FRAMES: Mutex<Vec<ObservedFrame>> = Mutex::new(Vec::new());
/// The one invocation whose writer is being watched.
///
/// EXACT ORIGIN IDENTITY, not a socket and not a client number. A descriptor
/// is reused as connections come and go, and the acceptance binary runs cases
/// beside each other; recording every service while armed would let one
/// case's steps be read as another's.
static WATCHED_ORIGIN: Mutex<Option<XServerFrontendRouteRegistry>> = Mutex::new(None);

/// Start recording one invocation's writer progress. Bounded, and cleared
/// here so a case never reads another case's steps.
fn observe_frames(registry: &XServerFrontendRouteRegistry) {
    OBSERVED_FRAMES.lock().unwrap().clear();
    *WATCHED_ORIGIN.lock().unwrap() = Some(registry.clone());
}

fn take_observed_frames() -> Vec<ObservedFrame> {
    *WATCHED_ORIGIN.lock().unwrap() = None;
    std::mem::take(&mut OBSERVED_FRAMES.lock().unwrap())
}

/// Production's entry into that recording. Reads nothing back and changes
/// nothing; with no origin watched, or with a step belonging to another
/// origin, it returns immediately.
pub(crate) fn observed_ordered_frame(
    emission: Option<&PrivateOrderedEmission>,
    index: usize,
    advanced: Option<usize>,
    failure: Option<String>,
) {
    let watched = WATCHED_ORIGIN.lock().unwrap().clone();
    let (Some(watched), Some(emission)) = (watched, emission) else {
        return;
    };
    // The emission's own answer about whose registry it belongs to, compared
    // by Arc identity. Two live invocations can number a client alike.
    if !emission.answers_for(&watched) {
        return;
    }
    let mut seen = OBSERVED_FRAMES.lock().unwrap();
    if seen.len() < 4096 {
        seen.push(ObservedFrame {
            delivery: emission.delivery(),
            frames: emission.frame_count(),
            index,
            advanced,
            failure,
        });
    }
}

/// What the writer's own steps say about one delivery of the watched
/// invocation: how many of its frames went out whole, how many it owed, and
/// the failure that ended it if any. Whole frames only; no byte offset.
fn frames_of(
    observed: &[ObservedFrame],
    delivery: XAuthorityInputDeliveryId,
) -> (usize, usize, Option<String>) {
    let mine: Vec<_> = observed
        .iter()
        .filter(|step| step.delivery == Some(delivery))
        .collect();
    let advanced = mine.iter().filter(|step| step.advanced.is_some()).count();
    let owed = mine.iter().map(|step| step.frames).max().unwrap_or(0);
    let failure = mine
        .iter()
        .find_map(|step| step.failure.clone());
    (advanced, owed, failure)
}

/// Controls that measure rather than accept.
///
/// Nothing here is an acceptance case and nothing here may be bound to a
/// row: each one exists because its row is not established yet, and keeps
/// the measurement that says why. They live under their own name so the
/// component runner can tell them from the cases beside them, and the
/// helpers they share with those cases stay where the cases can reach them.
pub(super) mod diagnostics {
    use super::*;

    /// Per-kind diagnostics for `C.control_cleanup`, on the real service.
    ///
    /// NOT BOUND as that case. The row asks for actual cleanup of all nine kinds,
    /// and on this source only `ConfigureSurface` reports the steps that let an
    /// abandoned record be discharged; the other eight report nothing and are
    /// retained as unproved, which is the honest outcome of the rule but not
    /// evidence that each kind's cleanup was performed. The production repair for
    /// that is owned elsewhere; what is kept here is the measurement, so the
    /// difference between a discharge and a retention is recorded per kind rather
    /// than argued about.
    #[test]
    fn c_control_cleanup_diagnostics() {
        // EXECUTED, on its own invocation. `CloseSurface` can end the recipient's
        // connection, so the kinds that are meant to run and the kinds that are
        // meant never to run cannot share one.
        let executed_store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
        let mut executed_service = LifecycleService::launch_over_store(
            "c-control-executed",
            12060,
            None,
            false,
            1,
            executed_store.clone(),
        );
        executed_service.start();
        let (mut executed_peer, executed_custody) = executed_service.connect();
        let (executed_surface, _seq, _ingress) =
            focus_window(&executed_service, &mut executed_peer, 0x320801, 12060);
        let executed_client = executed_custody.cleanup_record().client;
        let executed_lease = executed_service.owner.lease();
        let executed_control = executed_service
            .access
            .control_producer(&executed_lease)
            .expect("the service's own control producer");
        let mut executed = Vec::new();
        for (kind, command) in every_control_kind(executed_client, executed_surface, 12400) {
            let transaction = command.command.transaction().raw();
            let accepted = executed_control
                .submit(&executed_lease, command)
                .map(|_| ())
                .map_err(|(refusal, _)| format!("{refusal:?}"));
            let outcome = accepted.is_ok().then(|| {
                ack_for(&executed_service.acks, transaction)
                    .map(|ack| format!("{:?}", ack.acknowledgement.outcome))
            });
            executed.push(json!({
                "kind": format!("{kind:?}"),
                "transaction": transaction,
                "accepted": format!("{accepted:?}"),
                "outcome": outcome,
            }));
        }
        executed_service.command(XServerFrontendServiceCommand::StopAndDisconnect);
        let executed_closed = executed_service.closed();
        let mut actors = executed_service.finish(&[executed_custody]);

        // ACCEPTED AND NEVER EXECUTED, on a second invocation whose runner is
        // held, so each kind is taken into the order and no executor claims it.
        let store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
        let mut service = LifecycleService::launch_over_store(
            "c-control-unexecuted",
            12061,
            None,
            false,
            1,
            store.clone(),
        );
        service.start();
        let (mut peer, custody) = service.connect();
        let (surface, _sequence, _ingress) = focus_window(&service, &mut peer, 0x320901, 12061);
        let client = custody.cleanup_record().client;
        let lease = service.owner.lease();
        let control = service
            .access
            .control_producer(&lease)
            .expect("the second service's own control producer");
        let held = hold_runner(&service);
        let entered = held.entered();
        let mut unexecuted = Vec::new();
        for (kind, command) in every_control_kind(client, surface, 12500) {
            let transaction = command.command.transaction().raw();
            let accepted = control
                .submit(&lease, command)
                .map(|_| ())
                .map_err(|(refusal, _)| format!("{refusal:?}"));
            unexecuted.push(json!({
                "kind": format!("{kind:?}"),
                "transaction": transaction,
                "accepted": format!("{accepted:?}"),
            }));
        }
        let charged_with_nine_accepted = store.reserved();
        let completion = service.registry.control_completion();
        let reconciled_while_live = completion
            .as_ref()
            .map(|registry| format!("{:?}", registry.reconcile_client(client)));
        let cleanups_owed_while_live = completion
            .as_ref()
            .map(|registry| format!("{:?}", registry.cleanups_owed().map(|owed| owed.len())));

        service.command(XServerFrontendServiceCommand::StopAndDisconnect);
        held.release();
        let closed = service.closed();

        // The production reconciliation, reached where production reaches it.
        let drive = store.drive();
        let reconciled_after_exit = completion
            .as_ref()
            .map(|registry| format!("{:?}", registry.reconcile_unstarted()));
        let cleanups_owed_after_exit = completion
            .as_ref()
            .map(|registry| format!("{:?}", registry.cleanups_owed().map(|owed| owed.len())));
        let retained = interrupted_custody(&service);
        actors.extend(service.finish(&[custody]));

        assert!(drive.readable, "the store could be looked at");
        assert_eq!(executed.len(), 9, "every named kind was actually executed");
        assert_eq!(unexecuted.len(), 9, "and every one accepted without one");
        let accepted_unexecuted = unexecuted
            .iter()
            .filter(|row| row["accepted"].as_str() == Some("Ok(())"))
            .count();
        assert!(
            accepted_unexecuted >= 8,
            "the held order took the named kinds; what it refused is recorded: {unexecuted:?}"
        );
        println!(
            "sophia_m3_control_cleanup_diagnostics {}",
            json!({
                "schema": 1,
                "case": "C.control_cleanup",
                "bound": false,
                "why_unbound": "actual cleanup is established for ConfigureSurface only; the other eight kinds report no steps and are retained as unproved. Recorded, not weakened.",
                "executed_through_real_writer": executed,
                "executed_closed_error": executed_closed.error,
                "accepted_and_never_executed": unexecuted,
                "accepted_unexecuted_count": accepted_unexecuted,
                "charged_with_nine_accepted": charged_with_nine_accepted,
                "reconcile_client_while_live": reconciled_while_live,
                "cleanups_owed_while_live": cleanups_owed_while_live,
                "reconcile_unstarted_after_exit": reconciled_after_exit,
                "cleanups_owed_after_exit": cleanups_owed_after_exit,
                "drive": format!("{drive:?}"),
                "retained": format!("{retained:?}"),
                "closed_error": closed.error,
                "held_on": format!("{entered:?}"),
                "collected_actors": actors,
            })
        );
    }

    /// Diagnostics for `C.indeterminate_send`, on the real service.
    ///
    /// NOT BOUND, because one of the four required subcases is still not
    /// established. `partial_send_not_replayed` needs a prefix of the same
    /// delivery: one capsule that owed more than one frame, of which some but
    /// not all went out. Whole frames belonging to earlier completed
    /// deliveries say nothing about the one that then stalled.
    ///
    /// `unknown_send_not_replayed` IS exercised here, by an actual
    /// interruption between the handover and the record of its result. An
    /// earlier version of this control claimed that interrupting there leaves
    /// the invocation's connection worker unjoined. That was wrong: it was a
    /// fixture-ordering error in this file, a third service finished without
    /// being stopped first, and the claim is withdrawn.
    #[test]
    fn c_indeterminate_send_diagnostics() {
        let mut actors = Vec::new();

        // ENQUEUED WORK IS OBSERVED, NOT RESENT, and a refused publication stays
        // owned. Both on one invocation, because both are about what an executor
        // does with work it has already handed on or already decided.
        let store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
        let mut service = LifecycleService::launch_over_store(
            "c-indeterminate",
            12050,
            None,
            false,
            1,
            store.clone(),
        );
        service.start();
        let (mut peer, custody) = service.connect();
        let (surface, sequence, ingress) = focus_window(&service, &mut peer, 0x320701, 12050);
        let client = custody.cleanup_record().client;
        let producers = leased_producers(&service, client, C_PRODUCERS);

        observe_turns(&service.registry);
        let delivered = press_and_release(
            &service,
            &ingress,
            &mut peer,
            surface,
            sequence,
            0x320701,
            12060,
        );
        assert!(waited_for(|| store.reserved() == Some(0)));
        let extra_wire = read_event(&mut peer, 1);
        let extra_receipt = service.deliveries.recv_timeout(Duration::from_millis(200));
        assert_eq!(extra_wire, None, "an enqueued capsule is not sent again");
        assert!(
            extra_receipt.is_err(),
            "and its receipt was published once: {extra_receipt:?}"
        );
        let turns = take_turns(&service.registry);
        let dispatched: usize = turns.iter().map(|turn| turn.dispatched).sum();
        assert_eq!(
            dispatched, 2,
            "exactly the press and the release reached a recipient queue"
        );
        let enqueued = json!({
            "delivered": delivered,
            "dispatched_over_all_turns": dispatched,
            "further_wire_copies": 0,
            "further_receipts": 0,
            "turns_taken": turns.len(),
        });

        let held = hold_runner(&service);
        let entered = held.entered();
        let refused_delivery = XAuthorityInputDeliveryId::from_raw(12070);
        producers[1]
            .submit(&service.owner.lease(), motion_to(surface, refused_delivery))
            .expect("the order accepts a request the source will refuse");
        let cell = delivery_cell(&service.registry, 12070).expect("its own completion");
        let taken = service
            .registry
            .input_recovery
            .state
            .lock()
            .expect("a readable ledger")
            .tickets
            .remove(&refused_delivery)
            .expect("the admission this request was given");
        held.release();
        let (report, reported) = sync_channel(1);
        arm_runner(
            &service.registry,
            Box::new(move |runner, lease| {
                for _ in 0..4 {
                    let _ = runner.service_turn(lease);
                }
                let private = runner.frontend();
                report
                    .send((
                        private.terminal.undelivered.len(),
                        private.terminal.turn.len(),
                        private.terminal.current.is_some(),
                    ))
                    .expect("the case is waiting for this reading");
            }),
        );
        let (undelivered, in_turn, current) = reported
            .recv_timeout(Duration::from_secs(5))
            .expect("the actual runner reported what it still owns");
        assert_eq!(cell.answer(), None, "nothing was published for it");
        assert!(
            undelivered + in_turn + usize::from(current) >= 1,
            "the item is retained by the executor that could not publish it"
        );
        assert_eq!(store.reserved(), Some(1), "and keeps the credit it took");
        service
            .registry
            .input_recovery
            .state
            .lock()
            .expect("a readable ledger")
            .tickets
            .insert(refused_delivery, taken);
        assert_eq!(
            cell.answer(),
            None,
            "restoring the admission publishes nothing by itself"
        );
        let refused = json!({
            "delivery": 12070,
            "published_while_admission_gone": Option::<String>::None,
            "retained_undelivered": undelivered,
            "retained_in_turn": in_turn,
            "retained_current": current,
            "credit_still_held": store.reserved(),
            "published_by_restoring_admission": Option::<String>::None,
            "held_on": format!("{entered:?}"),
        });
        service.command(XServerFrontendServiceCommand::StopAndDisconnect);
        let closed_first = service.closed();
        actors.extend(finish_labelled(
            "enqueued-and-refused-publication invocation",
            service,
            &[custody],
        ));

        // A HANDOVER BEGUN AND NEVER REPORTED. The capsule left, and the record
        // of what the handover returned never happened, so nothing can say
        // whether the recipient has it. It is not offered again.
        let unknown_store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
        let mut unknown = LifecycleService::launch_over_store(
            "c-unknown-send",
            12051,
            None,
            false,
            1,
            unknown_store.clone(),
        );
        unknown.start();
        let (mut unknown_peer, unknown_custody) = unknown.connect();
        let (unknown_surface, unknown_sequence, unknown_ingress) =
            focus_window(&unknown, &mut unknown_peer, 0x320a01, 12051);
        let press = XAuthorityInputDeliveryId::from_raw(12080);
        let release = XAuthorityInputDeliveryId::from_raw(12081);
        unknown_ingress
            .submit(
                &unknown.owner.lease(),
                button_to(unknown_surface, press, 272, true),
            )
            .expect("an actual press");
        assert_eq!(
            read_event(&mut unknown_peer, 3),
            Some(expected_button_event(true, unknown_sequence, 0x320a01, 1))
        );
        assert_eq!(
            receipt_for(&unknown.deliveries, 12080),
            XAuthorityInputDeliveryOutcome::Flushed
        );
        let release_cell_before = delivery_cell(&unknown.registry, 12081);
        arm_handover(
            &unknown.registry,
            release,
            Box::new(|| panic!("labelled acceptance interruption between handover and its record")),
        );
        unknown_ingress
            .submit(
                &unknown.owner.lease(),
                button_to(unknown_surface, release, 272, false),
            )
            .expect("an actual release, whose handover this case interrupts");
        let unknown_closed = unknown.closed();
        assert!(
            unknown_closed.unwound,
            "the interruption ended the invocation it happened in"
        );
        // The capsule had already been handed over, so the recipient does hold
        // the bytes. What no longer exists is anything that knows it.
        let release_wire = read_event(&mut unknown_peer, 3);
        let release_cell = delivery_cell(&unknown.registry, 12081)
            .or(release_cell_before)
            .and_then(|cell| cell.answer());
        let phases = retained_dispatch(&unknown);
        // THE RECORD SAYS WHAT HAPPENED TO IT, which is that nobody knows. The
        // handover was begun and its result never written down, and that is the
        // one state this subcase is about.
        assert!(
            phases.iter().any(|(phase, _, _)| phase == "Indeterminate"),
            "the retained release says its handover was begun and never reported: {phases:?}"
        );
        // AND IT KEEPS NOTHING TO SEND AGAIN. The capsule left; no replayable
        // copy stayed behind, so nothing could re-offer it even if something
        // decided to.
        assert!(
            phases
                .iter()
                .all(|(phase, replayable, _)| phase != "Indeterminate" || !*replayable),
            "and keeps no replayable copy of what it handed over: {phases:?}"
        );
        // The debt is the retained release itself, not an accepted-item credit:
        // that credit is returned when the item is disposed of and its event
        // moves into separately reserved storage, which had already happened.
        // What the interruption must not do is settle the release or discard it.
        assert!(
            !phases.is_empty(),
            "the release is retained by the store that outlived the invocation"
        );
        // AN ACTUAL RETAINED MAINTENANCE VISIT BETWEEN THE TWO READINGS.
        // Reading the same store twice establishes nothing about retrying;
        // this asks the original keeper for a real visit, and what it answers
        // is recorded as it comes, refusal included. What must not happen is
        // that a visit rebuilds or re-offers a handover nobody can describe.
        let visit = unknown.step();
        let unknown_drive = unknown_store.drive();
        let unknown_after = retained_dispatch(&unknown);
        assert_eq!(
            unknown_after, phases,
            "an actual maintenance visit did not re-offer or rebuild it: {visit:?}"
        );
        let replayed = read_event(&mut unknown_peer, 1);
        assert_eq!(
            replayed, None,
            "the release reached the recipient once and was not sent again"
        );
        let unknown_fact = json!({
            "press_delivery": 12080,
            "release_delivery": 12081,
            "seam": "labelled test-only, keyed by this origin and this delivery, one shot, immediately after the handover returned and before its result was recorded",
            "seam_mode": "one shot, an actual interruption: the result of the handover is never recorded, which is the state the subcase is about",
            "unwound": unknown_closed.unwound,
            "release_bytes_on_wire": release_wire.map(|bytes| bytes.to_vec()),
            "release_answer": release_cell.map(|answer| format!("{answer:?}")),
            "retained_phases": format!("{phases:?}"),
            "retained_phases_after_actual_maintenance_visit": format!("{unknown_after:?}"),
            "maintenance_visit": format!("{visit:?}"),
            "durable_drive": format!("{unknown_drive:?}"),
            "second_copy_on_wire": replayed.map(|bytes| bytes.to_vec()),
            "writer_receipt_is_the_writers_own": "the recipient half may be answered by the writer that flushed; the executor still cannot join it, and does not resend",
            "charged": unknown_store.reserved(),
        });
        actors.extend(finish_labelled(
            "unknown-handover invocation",
            unknown,
            &[unknown_custody],
        ));

        // A RECIPIENT THAT STOPS TAKING ITS BYTES. The peer's receive buffer is
        // bounded to a real, small size and then never read, so the writer's own
        // send blocks against an actual socket rather than a simulated one. What
        // the declared limit then produces is an outcome that establishes
        // nothing, and a capsule that is never rebuilt from it.
        let blocked_store = PrivateSettlementOwner::with_capacity(C_DEEP_BOUND);
        let mut blocked = LifecycleService::launch_over_store(
            "c-blocked-send",
            12052,
            None,
            false,
            1,
            blocked_store.clone(),
        );
        blocked.start();
        let (mut blocked_peer, blocked_custody) = blocked.connect();
        let (blocked_surface, blocked_sequence, blocked_ingress) =
            focus_window(&blocked, &mut blocked_peer, 0x320b01, 12052);
        // Fixture setup on the case's own end of the real connection: a bound on
        // what this recipient can hold. Nothing about the service is simulated.
        let bounded_buffer =
            rustix::net::sockopt::set_socket_recv_buffer_size(&blocked_peer, 2048).is_ok();
        let buffer_size = rustix::net::sockopt::socket_recv_buffer_size(&blocked_peer).ok();
        // From here the recipient never reads again, and the writer's own steps
        // are recorded so a prefix of the delivery that stalls is established by
        // what the writer did, not by counting frames of the ones before it.
        observe_frames(&blocked.registry);
        let mut pairs = 0u64;
        let mut outcomes: Vec<String> = Vec::new();
        let mut stalled = None;
        // Bounded well inside the harness's own per-case allowance: this case
        // must report what it found, never run into a deadline.
        let blocking_deadline = std::time::Instant::now() + Duration::from_secs(25);
        'blocking: for round in 0..2_000u64 {
            if std::time::Instant::now() >= blocking_deadline {
                stalled = Some(("the recipient never stopped the writer within the bound", 0));
                break 'blocking;
            }
            for pressed in [true, false] {
                let id = 12600 + round * 2 + u64::from(!pressed);
                if blocked_ingress
                    .submit(
                        &blocked.owner.lease(),
                        button_to(
                            blocked_surface,
                            XAuthorityInputDeliveryId::from_raw(id),
                            272,
                            pressed,
                        ),
                    )
                    .is_err()
                {
                    stalled = Some(("submission refused while the recipient is full", id));
                    break 'blocking;
                }
                match blocked.deliveries.recv_timeout(Duration::from_secs(12)) {
                    Ok(receipt) => {
                        let name = format!("{:?}", receipt.outcome);
                        if receipt.outcome != XAuthorityInputDeliveryOutcome::Flushed {
                            outcomes.push(name.clone());
                            stalled = Some(("an outcome that establishes nothing", id));
                            break 'blocking;
                        }
                        outcomes.push(name);
                    }
                    Err(_) => {
                        stalled = Some(("no receipt within the bound", id));
                        break 'blocking;
                    }
                }
            }
            pairs += 1;
        }
        let flushed_before_stall = outcomes
            .iter()
            .filter(|outcome| outcome.as_str() == "Flushed")
            .count();
        let stalling_outcome = outcomes
            .iter()
            .rev()
            .find(|outcome| outcome.as_str() != "Flushed")
            .cloned();
        let observed = take_observed_frames();
        // PRE-STOP FACTS ARE ALREADY IN HAND above: what the recipient took, what
        // ended the run, and the writer's own steps. Only now is this invocation
        // asked to stop, and only after it has actually exited is its retained
        // inventory a thing that exists to be read. Finishing it before that
        // would be asking a live service for evidence its own exit produces.
        blocked.command(XServerFrontendServiceCommand::StopAndDisconnect);
        let blocked_closed = blocked.closed();
        let blocked_phases = retained_dispatch(&blocked);
        let stalled_delivery = stalled.map(|(_, id)| id).filter(|id| *id != 0);
        let same_capsule = stalled_delivery.map(|id| {
            let (advanced, owed, failure) =
                frames_of(&observed, XAuthorityInputDeliveryId::from_raw(id));
            json!({
                "delivery": id,
                "frames_this_delivery_owed": owed,
                "frames_of_it_that_went_out_whole": advanced,
                "writer_failure_on_it": failure,
                "is_a_prefix_of_the_same_delivery": owed > 1 && advanced >= 1 && advanced < owed,
            })
        });
        let multi_frame_deliveries: Vec<_> = observed
            .iter()
            .filter(|step| step.frames > 1)
            .map(|step| json!({"delivery": step.delivery.map(|id| id.raw()), "frames_owed": step.frames, "frame_index": step.index, "whole_frame_that_went": step.advanced}))
            .take(12)
            .collect();
        let writer_failures: Vec<_> = observed
            .iter()
            .filter(|step| step.failure.is_some())
            .map(|step| json!({"delivery": step.delivery.map(|id| id.raw()), "frames_owed": step.frames, "frame_index": step.index, "failure": step.failure.clone()}))
            .take(8)
            .collect();
        let partial = json!({
            "writer_steps_recorded": observed.len(),
            "same_capsule_prefix": same_capsule,
            "multi_frame_deliveries_seen": multi_frame_deliveries,
            "writer_failures": writer_failures,
            "recipient_buffer_bounded": bounded_buffer,
            "recipient_buffer_bytes": buffer_size,
            "pairs_delivered_before_stall": pairs,
            "flushed_before_stall": flushed_before_stall,
            "stalled": stalled.map(|(why, id)| json!({"why": why, "delivery": id})),
            "stalling_outcome": stalling_outcome,
            "retained_phases_after_exit": format!("{blocked_phases:?}"),
            "closed_error": blocked_closed.error.clone(),
            "prefix_on_wire": if flushed_before_stall > 0 {
                "this recipient took earlier whole frames, so the wire holds committed bytes before the stall"
            } else {
                "no frame was established as taken before the stall, so no prefix is claimed"
            },
            "limitation": "the seam reports whole frames of the exact watched invocation, never a byte offset. A same-capsule whole-frame prefix is established when one delivery owed more than one frame and fewer than all of them went out; a partial-byte prefix is not claimed at all.",
        });
        assert!(
            bounded_buffer,
            "the case could bound its own end of the real connection"
        );
        assert!(
            stalling_outcome
                .as_deref()
                .is_none_or(|outcome| outcome != "Flushed"),
            "whatever ended the run was not a flush: {partial}"
        );
        let blocked_after = retained_dispatch(&blocked);
        assert_eq!(
            blocked_after, blocked_phases,
            "nothing rebuilt or re-offered the stalled capsule"
        );
        let _ = blocked_sequence;
        actors.extend(finish_labelled(
            "blocked-recipient invocation",
            blocked,
            &[blocked_custody],
        ));

        println!(
            "sophia_m3_indeterminate_send_diagnostics {}",
            json!({
                "schema": 1,
                "case": "C.indeterminate_send",
                "bound": false,
                "why_unbound": "partial_send_not_replayed is not established: it needs one capsule that owed more than one frame, of which some but not all went out, and no delivery driven here owed more than one. unknown_send_not_replayed is exercised by an actual post-handover interruption. The earlier claim that such an interruption leaves a connection worker unjoined was a fixture-ordering error in this control and is withdrawn.",
                "partial_send_blocked_recipient": partial,
                "unknown_send_interval": unknown_fact,
                "enqueued_observation_only": enqueued,
                "refused_publication_retained": refused,
                "first_invocation_order": format!("{:?}", closed_first.order),
                "collected_actors": actors,
            })
        );
    }

}
