// Controls for the runner's own accounting under the service: the live
// reclamation visit (held, retired, reused; refused by the allowance;
// unwatched; interrupted), a control the supervisor will not watch, a key
// before any native pointer observation, and the retained diagnostic of
// this checkpoint's one remaining input limitation (an overlapping-button
// release never delivered). Harness in `private_producer_service.rs`.

/// A key through the service before any native pointer source has observed
/// the pointer. The window selects KeyPress/KeyRelease and the applied focus
/// names it. The pointer's position comes from the observation preparation
/// made -- over the bare root, at the centre of the screen -- so the key
/// carries those root coordinates, the same coordinates relative to a window
/// at the root's origin, and no child, because the pointer is over nothing.
/// This was a retained diagnostic (the key refused MissingQueryScope) until
/// the prepared observation lifted the limitation.
#[test]
fn a_key_through_the_service_before_any_pointer_observation_carries_the_prepared_position() {
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
    let original = delivery_cell(&launched.registry, 97010).expect("the original admitted cell");
    let on_wire = read_event(&mut client, 2);
    let cell = delivery_cell(&launched.registry, 97010);
    let answer = cell.as_ref().and_then(|cell| cell.answer());
    let registry = launched.registry.clone();
    launched.commands.send(XServerFrontendServiceCommand::StopAndDisconnect).expect("listening");
    let outcome = produced_outcome(launched, "key diagnostic");
    let order = outcome.order.expect("the tally");
    assert_eq!(focus, Some(XAuthorityControlOutcome::Delivered), "the applied focus was established");
    assert_eq!(focus_in, Some(expected_focus_in(sequence, window)), "and seen by the window");
    let mut expected = expected_key_service_event(sequence, window, 38, true, 0);
    // The screen's centre, 1280 by 720 halved, as root coordinates and as
    // coordinates relative to a window at the root's origin.
    for (offset, value) in [(20, 640_i16), (22, 360), (24, 640), (26, 360)] {
        expected[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(on_wire, Some(expected), "the key reached the client at the prepared position");
    assert!(Arc::ptr_eq(cell.as_ref().unwrap(), &original));
    assert_eq!(answer, Some(XAuthorityClientInputDelivery {
        client: client_id,
        delivery: XAuthorityInputDeliveryId::from_raw(97010),
        outcome: XAuthorityInputDeliveryOutcome::Flushed,
    }), "the delivery answers the original admission before stop");
    assert_eq!(cell.unwrap().answer(), answer, "service exit cannot rewrite the original answer");
    assert_collected_running(&observe_worker(&custody, &registry), "key delivery");
    assert_eq!(order.taken, 2, "the control and the key were taken: {order:?}");
    assert_eq!(order.refused, 0, "(masks {event_mask:#x}): {order:?}");
    assert_eq!(order.last_refusal, None, "{order:?}");
    let _ = std::fs::remove_file(&socket_path);
}

include!("../../../tests/support/private_shared_activation.rs");

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
    let reserved_before = owner.store().reserved();
    // The seam is inside the charged, watched visit, before its first
    // observation. This establishes interruption at entry, not after any
    // partial sequence of removals.
    stage_reclaim_visit_with(|| panic!("the reclamation visit is lost inside its interval"));
    let lost = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner.service_turn(&lease)));
    assert!(lost.is_err());
    assert!(runner.service.is_interrupted(), "the dropped run closed the budget");
    assert_eq!(runner.frontend().outstanding, before, "the list is exactly as it was");
    assert_eq!(owner.store().reserved(), reserved_before, "nothing released");
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
