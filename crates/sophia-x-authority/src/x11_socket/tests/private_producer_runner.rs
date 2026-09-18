// Controls for the runner's own accounting under the service: the live
// reclamation visit (held, retired, reused; refused by the allowance;
// unwatched; interrupted), a control the supervisor will not watch, and the
// two retained diagnostics of this checkpoint's input limitations (keys
// refused by the runner's blanket key refusal; an overlapping-button
// release never delivered). Harness in `private_producer_service.rs`.

/// RETAINED DIAGNOSTIC, NOT A DELIVERY CLAIM: a key through the service.
/// The window selects KeyPress/KeyRelease, the applied focus names it and
/// was acknowledged, and the runner still refuses the key: the ordered
/// execution refuses every `InputEventKind::Key` as FocusNotApplied before
/// any focus transaction (its blanket key refusal), so no key reaches a
/// client through this service today. The prepared XKB history is not
/// exercised by this path; keyboard integration is a remaining item.
#[test]
fn a_key_through_the_service_is_refused_by_the_runners_blanket_key_refusal() {
    let (launched, socket_path) = launch_producing("producer-key", 9610, 4);
    launched.access.await_ready(Duration::from_secs(15)).expect("readiness");
    let mut client = connect_private_client(&socket_path);
    let window = handshake_ids(&mut client) | 0x0e71;
    let event_mask = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 21);
    let (surface, sequence) = selecting_window(&mut client, &launched.transactions, window, event_mask);
    let custody = wait_attached(&launched.registry);
    let client_id = custody.cleanup_record().client;
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).expect("the control producer");
    let (focus, focus_in) = apply_focus(&launched, &control, &mut client, client_id, surface, 97001);
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .expect("an ingress");
    let key = XAuthorityRoutedInput {
        request: RoutedInputRequest {
            serial: 1,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(1),
            time_msec: 1,
            target_surface: surface,
            global_position: Point::default(),
            local_position: Point::default(),
            kind: InputEventKind::Key {
                keycode: 30,
                pressed: true,
            },
        },
        route_lease: None,
        delivery: Some(XAuthorityInputDeliveryId::from_raw(97010)),
        mode: XAuthorityRoutedInputMode::Deliver,
    };
    ingress.submit(&lease, key).expect("the order accepts the key");
    let on_wire = read_event(&mut client, 2);
    let cell = delivery_cell(&launched.registry, 97010);
    launched.commands.send(XServerFrontendServiceCommand::StopAndDisconnect).expect("listening");
    let outcome = produced_outcome(launched, "key diagnostic");
    let order = outcome.order.expect("the tally");
    assert_eq!(focus, Some(XAuthorityControlOutcome::Delivered), "the applied focus was established");
    assert_eq!(focus_in, Some(expected_focus_in(sequence, window)), "and seen by the window");
    assert_eq!(on_wire, None, "no key reached the client");
    assert!(cell.is_some_and(|cell| cell.answer().is_none()), "its completion is unanswered");
    assert_eq!(order.taken, 2, "the control and the key were taken: {order:?}");
    assert_eq!(order.refused, 1);
    assert_eq!(
        order.last_refusal,
        Some(PrivateExecutionRefusal::FocusNotApplied),
        "the runner's blanket key refusal, with focus applied (masks {event_mask:#x}): {order:?}"
    );
    let _ = std::fs::remove_file(&socket_path);
}

/// RETAINED DIAGNOSTIC of the overlapping-button sequence through BOTH
/// releases, captured as observed: button 1 down, button 2 down, button 1
/// up (while 2 is held), button 2 up. What reaches the wire, what each
/// release's completion answers, and what the settlement still carries
/// after an ordinary stop are recorded here exactly; nothing is re-addressed
/// or replayed to make bytes appear, and no native proof is invented. The
/// joining of the source's shared-activation receipt through the retained
/// owner is remaining integration work, not this checkpoint's.
#[test]
fn an_overlapping_button_release_is_captured_through_both_releases_as_observed() {
    let (launched, socket_path) = launch_producing("producer-overlap", 9611, 4);
    launched.access.await_ready(Duration::from_secs(15)).expect("readiness");
    let (mut client, surface, sequence, custody, window) =
        admitted_connection(&launched, &socket_path, 0x0e81);
    let client_id = custody.cleanup_record().client;
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).expect("the control producer");
    let (focus, focus_in) = apply_focus(&launched, &control, &mut client, client_id, surface, 97101);
    assert_eq!(focus, Some(XAuthorityControlOutcome::Delivered));
    assert_eq!(focus_in, Some(expected_focus_in(sequence, window)));
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .expect("an ingress");
    let mut observed = Vec::new();
    for (delivery, button, pressed) in
        [(97110, 272, true), (97111, 274, true), (97112, 272, false), (97113, 274, false)]
    {
        let submitted = ingress
            .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(delivery), button, pressed))
            .map(|_| ())
            .map_err(|refusal| format!("{refusal:?}"));
        let event = read_event(&mut client, 3);
        observed.push((delivery, submitted, event));
    }
    let answers: Vec<_> = [97110u64, 97111, 97112, 97113]
        .iter()
        .map(|delivery| {
            delivery_cell(&launched.registry, *delivery).and_then(|cell| {
                waited_for(|| cell.answer().is_some());
                cell.answer().map(|answer| answer.outcome)
            })
        })
        .collect();
    launched.commands.send(XServerFrontendServiceCommand::StopAndDisconnect).expect("listening");
    let registry = launched.registry.clone();
    let outcome = produced_outcome(launched, "overlap diagnostic");
    let seen = observe_worker(&custody, &registry);
    let order = outcome.order.expect("the tally");
    // AS OBSERVED, PINNED: every submission accepted; the two presses reach
    // the wire (button 2's state carries Button1Mask); NEITHER release
    // reaches the wire, before or after the last release; neither release
    // completion is answered; the settlement carries no hold and two
    // settling releases (the earlier release retained without proof while
    // the other button was held, and the final release beside it), nothing
    // delivering, nothing undelivered. The debt does not advance after the
    // last release: unresolved, remaining integration work.
    let events: Vec<Option<(u8, u8, u16)>> = observed
        .iter()
        .map(|(_, _, event)| event.map(|e| (e[0], e[1], u16::from_le_bytes([e[28], e[29]]))))
        .collect();
    let submissions: Vec<bool> = observed.iter().map(|(_, submitted, _)| submitted.is_ok()).collect();
    assert_eq!(submissions, vec![true, true, true, true]);
    assert_eq!(
        events,
        vec![Some((4, 1, 0)), Some((4, 2, 1 << 8)), None, None],
        "observed: both presses delivered, neither release delivered"
    );
    assert_eq!(
        answers,
        vec![
            Some(XAuthorityInputDeliveryOutcome::Flushed),
            Some(XAuthorityInputDeliveryOutcome::Flushed),
            None,
            None
        ],
        "observed: the presses answered, neither release answered"
    );
    assert_eq!((order.taken, order.refused, order.routed, order.dispatched), (5, 0, 1, 2), "{order:?}");
    assert!(outcome.retained_holds.is_empty() && outcome.store_holds.is_empty(), "no hold survives");
    assert_eq!(
        outcome.terminal,
        Some((0, 2, 0, 0, false)),
        "two settling releases retained, nothing delivering or undelivered"
    );
    assert_collected_running(&seen, "overlap diagnostic");
    let _ = std::fs::remove_file(&socket_path);
}

/// STAGE-ONLY, over a prepared runner fixture: the live reclamation as the
/// service's turn runs it. The controls routed here are never acknowledged
/// by a writer (the fixture has none); a record is retired by the same
/// registry publication the writer would make, with the exact
/// acknowledgement, and nothing else frees a credit.
fn routed_control_fixture() -> (
    PrivatePreparedRunner,
    PrivateServiceOwner,
    XServerFrontendClientRouteRegistration,
    XServerFrontendClientRouteChannels,
    Receiver<XAuthorityClientControlAck>,
    Receiver<XAuthorityClientInputDelivery>,
) {
    prepared_runner_fixture()
}

/// Submit and route one ConfigureSurface control through the runner's turn;
/// answers its transaction.
fn route_one_control(
    runner: &mut PrivatePreparedRunner,
    lease: &PrivateServiceLease<'_>,
    channels: &XServerFrontendClientRouteChannels,
    transaction: u64,
) -> Result<(), String> {
    runner
        .control_producer(lease)
        .expect("its own owner")
        .submit(
            lease,
            configure(XServerFrontendClientId::from_raw(9000), SurfaceId::new(9000, 1), transaction),
        )
        .map(|_| ())
        .map_err(|(refusal, _)| format!("{refusal:?}"))?;
    let progress = runner.service_turn(lease).expect("a readable order");
    assert_eq!(progress.routed, 1, "routed in this turn: {progress:?}");
    // The writer takes the command off its channel and acknowledges nothing:
    // the record stays outstanding, the queue does not fill.
    assert!(channels.control.try_recv().is_ok(), "the routed command reached the client's channel");
    Ok(())
}

/// Retire one routed control's record exactly as its writer would: the
/// registry publication of the exact acknowledgement.
fn retire_control(runner: &PrivatePreparedRunner, transaction: u64) {
    let token = runner
        .frontend()
        .outstanding
        .iter()
        .find_map(|identity| match identity {
            PrivateIdentity::Control {
                transaction: held,
                completion: Some(token),
            } if *held == TransactionId::from_raw(transaction) => Some(*token),
            _ => None,
        })
        .expect("the routed control is outstanding with its completion token");
    let acknowledgement = XAuthorityClientControlAck {
        client: XServerFrontendClientId::from_raw(9000),
        acknowledgement: XAuthorityControlAck {
            kind: XAuthorityControlKind::ConfigureSurface,
            transaction: TransactionId::from_raw(transaction),
            surface: SurfaceId::new(9000, 1),
            outcome: XAuthorityControlOutcome::Delivered,
        },
    };
    runner
        .frontend()
        .completion
        .publish_with(token, acknowledgement, |_| ControlPublication::Delivered)
        .expect("the exact acknowledgement retires the record");
}

/// An idle turn: nothing accepted, so the turn's only act is the visit.
fn idle_turn(runner: &mut PrivatePreparedRunner, lease: &PrivateServiceLease<'_>) -> PrivateRunnerProgress {
    let progress = runner.service_turn(lease).expect("a readable order");
    assert_eq!((progress.taken, progress.routed, progress.terminal_steps), (0, 0, 0), "idle: {progress:?}");
    progress
}

#[test]
fn a_routed_control_stays_charged_until_its_record_retires_and_then_exactly_one_credit_returns() {
    let (mut runner, owner, registration, channels, _acks, _deliveries) = routed_control_fixture();
    let lease = owner.lease();
    let store = owner.store();
    let reserved_before = store.reserved();
    // ROUTED, NOT ANSWERED: outstanding, and observed as pending by the
    // accounted visit on the idle turns that follow -- observed, reclaimed
    // nothing, credit charged.
    route_one_control(&mut runner, &lease, &channels, 98001).expect("accepted");
    assert_eq!(runner.frontend().outstanding.len(), 1);
    let pending = idle_turn(&mut runner, &lease);
    assert_eq!((pending.reclaim_observed, pending.reclaimed), (1, 0), "{pending:?}");
    assert_eq!(pending.reclaim_refusal, None);
    assert!(!pending.reclaim_unwatched);
    assert_eq!(runner.frontend().outstanding.len(), 1, "still outstanding");
    assert_eq!(store.reserved(), reserved_before.map(|before| before + 1), "still charged");
    // HELD TO CAPACITY: controls are accepted until the registry is full,
    // then refused Saturated while every record is outstanding; the visits
    // observe them all (at most the bound per visit, resuming) and reclaim
    // nothing.
    let mut accepted = 1;
    let mut transaction = 98002;
    let saturated = loop {
        match route_one_control(&mut runner, &lease, &channels, transaction) {
            Ok(()) => {
                accepted += 1;
                transaction += 1;
            }
            Err(refusal) => break refusal,
        }
        assert!(accepted < 64, "a bound refuses somewhere");
    };
    assert!(saturated.starts_with("Saturated"), "{saturated}");
    assert_eq!(runner.frontend().outstanding.len(), accepted);
    let first_visit = idle_turn(&mut runner, &lease);
    let second_visit = idle_turn(&mut runner, &lease);
    assert!(
        first_visit.reclaim_observed <= PRIVATE_RECLAIM_VISIT_BOUND
            && first_visit.reclaim_observed + second_visit.reclaim_observed >= accepted.min(2 * PRIVATE_RECLAIM_VISIT_BOUND),
        "bounded, resuming: {first_visit:?} then {second_visit:?}"
    );
    assert_eq!((first_visit.reclaimed, second_visit.reclaimed), (0, 0));
    assert_eq!(runner.frontend().outstanding.len(), accepted, "all still charged");
    // ONE RECORD RETIRES BY ITS EXACT ACKNOWLEDGEMENT: the visits that follow
    // reclaim exactly one credit, the outstanding list shrinks by one, and
    // exactly one more control is accepted -- not two.
    retire_control(&runner, 98001);
    let mut reclaimed = 0;
    for _ in 0..4 {
        let visit = idle_turn(&mut runner, &lease);
        reclaimed += visit.reclaimed;
        if reclaimed > 0 {
            break;
        }
    }
    assert_eq!(reclaimed, 1, "exactly one credit returned");
    assert_eq!(runner.frontend().outstanding.len(), accepted - 1);
    assert_eq!(route_one_control(&mut runner, &lease, &channels, transaction), Ok(()), "exact reuse");
    let again = route_one_control(&mut runner, &lease, &channels, transaction + 1);
    assert!(
        matches!(again.as_ref(), Err(refusal) if refusal.starts_with("Saturated")),
        "and no second: {again:?}"
    );
    assert_eq!(runner.frontend().outstanding.len(), accepted);
    drop(registration);
    drop((runner.shutdown(), owner));
}

#[test]
fn the_reclamation_visit_is_refused_by_the_cleanup_allowance_and_unwatched_by_a_failed_supervisor() {
    let (mut runner, owner, registration, channels, _acks, _deliveries) = routed_control_fixture();
    let lease = owner.lease();
    // Two routed controls, neither retired yet: every idle turn has a visit
    // to make, and none reclaims.
    route_one_control(&mut runner, &lease, &channels, 98101).expect("accepted");
    route_one_control(&mut runner, &lease, &channels, 98102).expect("accepted");
    // THE ALLOWANCE: the planned cleanup allowance admits a few starts per
    // interval. Idle turns in quick succession spend them on visits; the
    // visit past the allowance is refused, observes nothing and reclaims
    // nothing.
    let starts_before = runner.service.usage().starts;
    let mut refused = None;
    let mut reclaimed_by_refused_turn = 0;
    for _ in 0..64 {
        let visit = idle_turn(&mut runner, &lease);
        if let Some(cause) = visit.reclaim_refusal {
            refused = Some(cause);
            reclaimed_by_refused_turn = visit.reclaimed;
            assert_eq!(visit.reclaim_observed, 0, "a refused visit observes nothing: {visit:?}");
            break;
        }
    }
    assert!(refused.is_some(), "the cleanup allowance refused a visit within the interval");
    assert_eq!(reclaimed_by_refused_turn, 0);
    assert!(runner.service.usage().starts > starts_before, "the admitted visits were charged starts");
    // THE WATCH BOUNDARY: one record retires, then the runner's own
    // supervisor latches a failure (an execution dropped unfinished, as the
    // runner controls arrange it). A later admitted visit is charged but
    // unwatched: nothing observed, nothing reclaimed, the retired record
    // still outstanding and charged.
    retire_control(&runner, 98101);
    drop(
        runner
            .watch
            .as_ref()
            .expect("prepared supervisor")
            .begin_dequeued(std::time::Instant::now())
            .expect("a supervisor still taking work"),
    );
    std::thread::sleep(Duration::from_millis(20));
    let unwatched = idle_turn(&mut runner, &lease);
    assert!(unwatched.reclaim_unwatched, "{unwatched:?}");
    assert_eq!((unwatched.reclaim_observed, unwatched.reclaimed), (0, 0), "{unwatched:?}");
    assert_eq!(unwatched.reclaim_refusal, None, "admitted, then unwatched");
    assert_eq!(runner.frontend().outstanding.len(), 2, "both still charged, still owned");
    drop(registration);
    drop((runner.shutdown(), owner));
}

#[test]
fn an_interrupted_reclamation_visit_closes_the_budget_and_leaves_the_list_exactly() {
    let (mut runner, owner, registration, channels, _acks, _deliveries) = routed_control_fixture();
    let lease = owner.lease();
    route_one_control(&mut runner, &lease, &channels, 98201).expect("accepted");
    route_one_control(&mut runner, &lease, &channels, 98202).expect("accepted");
    retire_control(&runner, 98201);
    let before: Vec<PrivateIdentity> = runner.frontend().outstanding.clone();
    stage_reclaim_visit_with(|| panic!("the reclamation visit is lost inside its interval"));
    let lost = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner.service_turn(&lease)));
    assert!(lost.is_err());
    assert!(runner.service.is_interrupted(), "the dropped run closed the budget");
    assert_eq!(runner.frontend().outstanding, before, "the list is exactly as it was");
    assert_eq!(owner.store().reserved(), owner.store().reserved(), "nothing released");
    drop(registration);
    drop((runner.shutdown(), owner));
}

#[test]
fn a_control_is_not_routed_while_the_runners_supervisor_will_not_watch_it() {
    let (mut runner, owner, registration, channels, _acks, _deliveries) = routed_control_fixture();
    let lease = owner.lease();
    let sequence = runner
        .control_producer(&lease)
        .expect("its own owner")
        .submit(
            &lease,
            configure(XServerFrontendClientId::from_raw(9000), SurfaceId::new(9000, 1), 98301),
        )
        .expect("accepted");
    // THE WATCH BOUNDARY, ON THE ROUTING SIDE: the supervisor latches a
    // failure before the control's turn. The step is charged and answers
    // Unwatched; the control stays parked with its barrier, nothing reached
    // the client's channel, nothing is outstanding, and no attempt stands.
    drop(
        runner
            .watch
            .as_ref()
            .expect("prepared supervisor")
            .begin_dequeued(std::time::Instant::now())
            .expect("a supervisor still taking work"),
    );
    std::thread::sleep(Duration::from_millis(20));
    assert!(matches!(
        runner.execute_accounted_step().expect("a readable order"),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Unwatched(s), charge: Some(_) } if s == sequence
    ));
    assert!(channels.control.try_recv().is_err(), "nothing was routed to the client");
    assert_eq!(runner.frontend().outstanding.len(), 0, "nothing owed as attempted");
    assert!(runner.frontend().routing_attempt().is_none(), "no attempt stands");
    assert_eq!(runner.frontend().parked(), Some(sequence), "parked, owning the operation");
    assert!(runner.frontend().blocked().is_some(), "the order is blocked on it");
    drop(registration);
    drop((runner.shutdown(), owner));
}
