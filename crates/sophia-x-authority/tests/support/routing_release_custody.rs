// A release and the custody it carries: the press it ends, the hold it answers,
// and the order between an older carried press and a newer held one.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_release_answers_its_hold_after_the_surface_is_gone() {
    let client = XServerFrontendClientId(731);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(731), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,

                watch,
            )
        .expect("the press to run");
    assert!(run.first_press);
    let _ = pressed.observe();

    // The surface goes. A release that consulted the route would now refuse,
    // which is exactly when a release matters most: the client still holds the
    // button and is owed the event that ends it.
    private
        .broker
        .registry
        .surfaces
        .lock()
        .expect("the surfaces")
        .remove(&surface);

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(732), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,

                watch,
            )
        .expect("the release to run with its target gone");
    let reached = run.reached.expect("the release names its hold");
    assert_eq!(reached.client(), client);
    assert_eq!(
        reached.window(),
        window,
        "it answers to what the press recorded"
    );
    assert!(matches!(
        run.release,
        Some(sophia_input_authority::ReleaseOutcome::DeliverTo(_))
    ));
}

#[test]
fn a_release_keeps_the_window_its_press_recorded() {
    let client = XServerFrontendClientId(741);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(741), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,

                watch,
            )
        .expect("the press to run");
    assert_eq!(run.reached.expect("a press decides").window(), window);
    let _ = pressed.observe();

    // The surface now maps somewhere else entirely.
    let moved = XResourceId::new(0x200742, 2);
    {
        let mut surfaces = private
            .broker
            .registry
            .surfaces
            .lock()
            .expect("the surfaces");
        let route = surfaces.get_mut(&surface).expect("the route");
        route.window = moved;
    }

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(742), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,

                watch,
            )
        .expect("the release to run");
    let reached = run.reached.expect("the release names its hold");
    assert_eq!(
        reached.window(),
        window,
        "the release answers the window its press reached, not where the route points now"
    );
    assert_ne!(reached.window(), moved);
}

#[test]
fn a_release_of_nothing_held_is_an_outcome_not_a_missing_target() {
    let client = XServerFrontendClientId(751);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    // Nothing was ever pressed. The target is registered and present, so a
    // refusal blaming the target would be describing a problem that is not
    // there; the ledger simply owes nobody an event.
    let released = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &button_to(surface, XAuthorityInputDeliveryId::from_raw(751), 272, false),
            &released,

                watch,
            )
        .expect("an unheld release is a successful outcome");
    assert!(
        matches!(
            run.release,
            Some(sophia_input_authority::ReleaseOutcome::NotHeld)
        ),
        "the ledger says it was not holding, got {:?}",
        run.release
    );
    assert!(run.reached.is_none(), "so nobody is owed an event");
    assert!(run.event.is_none());
}

/// The pointer state this seat/namespace currently projects.
fn projected_buttons(
    private: &crate::PrivateXServerFrontend,
    namespace: NamespaceId,
    seat: SeatId,
) -> u16 {
    private
        .broker
        .registry
        .pointer_state
        .lock()
        .expect("the pointer state")
        .get(&(namespace, seat))
        .map_or(0, |mapper| mapper.state())
}

#[test]
fn steady_delivery_traffic_does_not_starve_an_older_native_proof() {
    // Choosing native work only when the queues fall empty is not fairness,
    // it is a promise that never comes due: a pointer anyone is actually
    // using keeps a delivery ready at every terminal call, and the proof of a
    // release that already happened waits behind traffic for ever.
    let client = XServerFrontendClientId(2441);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // One release, left owing a proof. Its entry is delivered by hand so the
    // wrapper does not spend the visit.
    let run_by_hand = |private: &mut crate::PrivateXServerFrontend,
                           keyboards: &mut crate::PrivateKeyboards,
                           delivery: u64,
                           button: u32,
                           pressed: bool| {
        ingress
            .submit(&keeper.lease(), button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
    };
    run_by_hand(private, keyboards, 2441, 272, true);
    private.deliver_one(None, &mut |_, _| Ok(())).expect("the press delivers");
    run_by_hand(private, keyboards, 2442, 272, false);
    private
        .deliver_one(None, &mut |_, _| Ok(()))
        .expect("the release delivers");
    assert_eq!(private.terminal.settling.len(), 1);
    assert!(
        !private.terminal.settling[0].native_recorded(),
        "its proof is still owed, which is what this control is about"
    );

    // Now keep a real delivery ready at EVERY terminal call, on a different
    // button so no new native debt is created, and count the visits the old
    // proof gets while that traffic runs.
    let mut visits = 0;
    let mut delivered = 0;
    for round in 0..8u64 {
        // A real entry is ready and waiting at every terminal call. The first
        // is a press of a second button; the rest join it, so the traffic
        // creates no new native debt of its own.
        run_by_hand(private, keyboards, 2450 + round, 273, true);
        // Steps until this round's entry is delivered. A native visit taking
        // one of them is exactly the fairness under test; the delivery still
        // gets its step and keeps its place.
        loop {
            match private
                .deliver_one(None, &mut |_, _| Ok(()))
                .expect("a terminal step")
            {
                PrivateDeliveryStep::Recorded { .. } => visits += 1,
                PrivateDeliveryStep::Advanced { .. } => {
                    delivered += 1;
                    break;
                }
                // Once its proof is in, the same release owes a delivery
                // attempt. That is more native work, and it takes its turn
                // the same bounded way.
                PrivateDeliveryStep::Dispatched { .. } => visits += 1,
                PrivateDeliveryStep::Receipt { .. } => {
                    panic!("no receipt has been published in this control")
                }
                PrivateDeliveryStep::Idle => panic!("traffic was ready, so no step is idle"),
                // This control lends no keyboards, so the visit that looks
                // for a departed source's release is never offered one.
                PrivateDeliveryStep::DepartedRelease { .. } => {
                    unreachable!("no keyboards were lent, so no such visit is made")
                }
                PrivateDeliveryStep::SharedActivation { .. } => {}
                PrivateDeliveryStep::TransientReceipt { .. } => {}
                PrivateDeliveryStep::NativeDisposal { .. } => {}
                PrivateDeliveryStep::Blocked(_) => panic!("no entry is indeterminate here"),
            }
        }
    }
    assert_eq!(delivered, 8, "every round's entry was delivered, in its turn");

    assert!(
        visits >= 1,
        "native work got a bounded turn while deliveries stayed ready; it \
         received {visits} visits across eight rounds of traffic"
    );
    assert!(
        private.terminal.settling[0].native_recorded(),
        "and the older proof was recorded rather than waiting behind traffic"
    );
}

/// Stage what an interrupted handover leaves on a hold record: the phase
/// saying the handover began, with the exact capsule still in its slot.
///
/// Reproduces an unwind between the write-ahead and the send. It forges no
/// receipt and builds no delivery -- the capsule is the one production made.
fn stage_interrupted_head(record: &mut PrivateHoldRecord, capsule: XAuthorityOrderedDelivery) {
    record.custody.pending = Some(PrivatePendingDelivery::Capsule(capsule));
    record.custody.dispatch = PrivateDispatchPhase::Indeterminate;
}

/// Stage what an interrupted handover leaves in a settling record: the capsule
/// gone from the slot, and the phase saying the handover was begun.
///
/// Lives here rather than in the production module, where a cfg(test) helper
/// is inline test code. It fabricates no delivery and forges no receipt -- it
/// reproduces exactly the state an unwind between the take and the report
/// would leave behind.
fn stage_interrupted_handover(release: &mut PrivateSettlingRelease) {
    release.custody.pending = None;
    release.custody.dispatch = PrivateDispatchPhase::Indeterminate;
}

fn settling_slot_is_empty(release: &PrivateSettlingRelease) -> bool {
    release.custody.pending.is_none()
}

#[test]
fn an_outcome_is_owned_before_an_ordinary_observer_can_prune_it() {
    // Recovery drops a routing-finished ticket the moment an ordinary
    // observer consumes it, and that ticket is the only place the outcome
    // lives. A join that read it only when it was ready to settle would find
    // the attempt still out and its answer already gone.
    let client = XServerFrontendClientId(2491);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2491u64, true), (2492u64, false)] {
        let lease = keeper.lease();
        ingress
            .submit(&lease, button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    for _ in 0..12 {
        private.deliver_one(None, &mut |_, _| Ok(())).expect("a step");
    }
    assert_eq!(
        private.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Enqueued
    );
    let delivery = private.terminal.settling[0]
        .delivery()
        .expect("the release knows its delivery");

    // The writer answers, and an ORDINARY OBSERVER consumes it -- which is
    // what prunes the ticket.
    let recovery = &private.broker.registry.input_recovery;
    recovery
        .finish(client, Some(delivery), XAuthorityInputDeliveryOutcome::Flushed)
        .expect("the answer is published");

    // One visit, so the terminal side takes custody of the outcome.
    private.deliver_one(None, &mut |_, _| Ok(())).expect("a step");

    assert_eq!(
        private.terminal.settling[0].outcome_seen(),
        Some(XAuthorityInputDeliveryOutcome::Flushed),
        "the outcome is owned here, not merely readable over there"
    );
    assert!(
        private.terminal.settling[0].attempt().is_none(),
        "and the attempt was finished against it"
    );
}

/// Run one real request from one real producer and return what it decided.
///
/// Adopted from the independent review, which found the two no-event branches
/// my own fixture could not reach: they need DISTINCT producers. One device
/// pressing and joining always leaves the same holder bit, so a release from
/// it is always a final one.
fn noevent_run(
    fixture: &mut PreparedOrderedFixture,
    ingress: &crate::PrivateIngress,
    delivery: u64,
    device: u64,
    button: u32,
    pressed: bool,
) -> PrivateOrderedRun {
    let mut route = button_to(
        fixture.surface,
        XAuthorityInputDeliveryId::from_raw(delivery),
        button,
        pressed,
    );
    route.request.device = DeviceId::from_raw(device);
    ingress.submit(&fixture.keeper.lease(), route).expect("the order to accept it");
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().expect("a live runner");
    assert!(matches!(
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().expect("a sealed watch"))
            .expect("a step"),
        PrivateOrderedStep::Decided(_)
    ));
    let Some(item) = private.terminal.turn.pop() else {
        panic!("an accepted request produces a decision")
    };
    let PrivateOrderedItem::Ran { run, custody, route, .. } = item else {
        panic!("an accepted request must run, not refuse")
    };
    assert_eq!(route.request.device, DeviceId::from_raw(device));
    assert!(
        custody.observe().expect("a readable completion").is_some(),
        "the real common completion, not a fabricated writer result"
    );
    run
}

/// The two known no-event branches, reached from genuinely separate sources.
///
/// SurvivorRemains: A presses, B joins, A releases -- B's holder bit remains.
/// NotHeld: A presses, and B, which never joined, releases -- the ledger
/// finds no holder bit for B while this executor still has the record.
///
/// Neither outcome, completion cell nor holder bit is set by the control:
/// the source and the ledger produce the branch themselves.
fn noevent_from_distinct_sources(survivor: bool) {
    let (client, base) = if survivor {
        (XServerFrontendClientId(7511), 75110)
    } else {
        (XServerFrontendClientId(7521), 75210)
    };
    let mut fixture = prepared_ordered_fixture(client);
    let first = fixture
        .runner
        .frontend
        .as_mut()
        .expect("a live runner")
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("a producer");
    let second = fixture
        .runner
        .frontend
        .as_mut()
        .expect("a live runner")
        .ingress_for(client, DeviceId::from_raw(2))
        .expect("a second producer");

    let pressed = noevent_run(&mut fixture, &first, base, 1, 272, true);
    assert!(pressed.first_press && pressed.owes_event && pressed.event.is_some());
    let private = fixture.runner.frontend.as_ref().expect("a live runner");
    let recovery = private.broker.registry.input_recovery.clone();
    let original = private.terminal.holds[0]
        .custody
        .completion
        .as_ref()
        .expect("the press holds its own completion")
        .clone();
    let incarnation = private.terminal.holds[0].incarnation;
    // The obligation's own address, so "unchanged" means the same one rather
    // than merely something being present.
    let native_address =
        private.terminal.holds[0].native.as_ref().expect("a source obligation") as *const _
            as usize;

    if survivor {
        let joined = noevent_run(&mut fixture, &second, base + 1, 2, 272, true);
        assert!(!joined.first_press && !joined.owes_event && joined.event.is_none());
    }

    let noevent = noevent_run(
        &mut fixture,
        if survivor { &first } else { &second },
        base + 2,
        if survivor { 1 } else { 2 },
        272,
        false,
    );
    let expected = if survivor {
        sophia_input_authority::ReleaseOutcome::SurvivorRemains
    } else {
        sophia_input_authority::ReleaseOutcome::NotHeld
    };
    assert_eq!(noevent.release, Some(expected));
    assert!(!noevent.owes_event && noevent.event.is_none());

    // Nothing of the original press was disturbed, and nothing was invented.
    let private = fixture.runner.frontend.as_ref().expect("a live runner");
    assert_eq!(private.terminal.holds.len(), 1);
    assert!(private.terminal.settling.is_empty());
    assert_eq!(private.terminal.holds[0].incarnation, incarnation);
    assert_eq!(
        private.terminal.holds[0].native.as_ref().expect("still held") as *const _ as usize,
        native_address,
        "the same source obligation, not a replacement that merely exists"
    );
    assert!(Arc::ptr_eq(
        &original,
        private.terminal.holds[0]
            .custody
            .completion
            .as_ref()
            .expect("still held")
    ));
    let release = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(base + 2))
        .expect("a readable ledger")
        .expect("the no-event release's own admission");
    assert!(!Arc::ptr_eq(&original, &release));
    assert!(original.answer().is_none() && release.answer().is_none());
    assert!(fixture.channels.ordered.try_recv().is_err());
    // The button another genuine source still holds is untouched: the native
    // projection was not moved by a release that owed no event.
    let mask = private
        .broker
        .registry
        .pointer_state
        .lock()
        .expect("the pointer state")
        .get(&(NamespaceId::from_raw(client.raw()), SeatId::from_raw(1)))
        .expect("the original native mapper is still present")
        .state();
    assert_eq!(
        mask, 256,
        "another genuine source still holds the original button"
    );
    // Owner counts, which say who is keeping each cell alive rather than only
    // that the cells differ.
    assert_eq!(
        Arc::strong_count(&original),
        3,
        "the press cell is held by the ledger, the record and this inspection"
    );
    assert_eq!(
        Arc::strong_count(&release),
        2,
        "and the disposed release cell only by the ledger and this inspection"
    );

    // THE DISPOSAL ITSELF.
    assert!(
        private.terminal.pending_custody.is_none(),
        "a known no-event branch disposes of the custody it acquired"
    );

    // And a real new press then runs through the protected path. Accepting
    // some other refusal would not show the slot was free.
    let next = noevent_run(&mut fixture, &first, base + 3, 1, 273, true);
    assert!(next.first_press && next.owes_event && next.event.is_some());
    let private = fixture.runner.frontend.as_ref().expect("a live runner");
    assert_eq!(private.terminal.holds.len(), 2);
    assert!(private.terminal.pending_custody.is_none() && private.terminal.native_pending.is_none());
    assert!(original.answer().is_none() && release.answer().is_none());
}

#[test]
fn a_survivor_release_from_a_distinct_source_disposes_only_its_own_custody() {
    noevent_from_distinct_sources(true);
}

#[test]
fn a_not_held_release_from_a_nonparticipating_source_disposes_only_its_own_custody() {
    noevent_from_distinct_sources(false);
}

#[test]
fn a_completed_operation_leaves_no_custody_behind_for_the_next_one() {
    // The slot is emptied by an explicit transfer or disposition. A press and
    // a release that complete move their custody into the record for their
    // debt, so nothing is left to refuse the operation after them.
    //
    // WHAT THIS DOES NOT REACH: the no-event results, NotHeld and
    // SurvivorRemains, with a record present. A join adopts the hold rather
    // than adding a holder, so a release here always reports DeliverTo and
    // this fixture cannot produce the other two. Their disposal is written and
    // is NOT proved by this control.
    let client = XServerFrontendClientId(2571);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, .. } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let button = |private: &mut crate::PrivateXServerFrontend,
                  keyboards: &mut crate::PrivateKeyboards,
                  slot: u64,
                  delivery: u64,
                  pressed: bool| {
        let custody = role.reserve(stamp, slot).expect("a reservation").accepted();
        let run = private.run_ordered_input(
            keyboards,
            &{
                let route = button_to(
                    surface,
                    XAuthorityInputDeliveryId::from_raw(delivery),
                    272,
                    pressed,
                );
                admit_for_direct_run(private, &route);
                route
            },
            &custody,
            watch,
        );
        let _ = custody.observe();
        run
    };

    button(private, keyboards, 1, 2571, true).expect("the press to run");
    assert!(
        private.terminal.pending_custody.is_none(),
        "a completed press moved its custody into its record"
    );
    button(private, keyboards, 2, 2572, false).expect("the release to run");
    assert!(
        private.terminal.pending_custody.is_none(),
        "and a completed release moved its own"
    );

    // So the next operation is not refused for something left behind.
    let next = button(private, keyboards, 3, 2573, true);
    assert!(
        !matches!(
            next,
            Err(crate::PrivateExecutionRefusal::CustodyRetained)
        ),
        "nothing was left held, so nothing is refused for holding it"
    );
}

#[test]
fn a_retained_custody_refuses_the_next_operation_rather_than_being_replaced() {
    // A refusal that leaves the source holding context leaves this custody
    // attached to that same continuation. Assigning over it would drop the
    // only handle able to answer whatever that continuation still owes, with
    // nothing recorded about what became of it -- and the replacement would
    // then be the one everything else believed in.
    let client = XServerFrontendClientId(2561);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    let run = |private: &mut crate::PrivateXServerFrontend,
                   keyboards: &mut crate::PrivateKeyboards,
                   delivery: u64,
                   button: u32,
                   pressed: bool| {
        ingress
            .submit(&keeper.lease(), button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("the entry delivers");
    };

    run(private, keyboards, 2561, 272, true);
    // STAGED INVENTORY MISMATCH: keep the actual original native obligation
    // alive outside inventory while common still owns its press. The next
    // source call discovers the disagreement after entering common and keeps
    // its new context. A returned ReleaseBarrier is a pre-effect refusal and
    // correctly leaves no such context to retain.
    let original = private.terminal.holds.pop().expect("the actual original hold");
    let original_cell = original.custody.completion.as_ref().unwrap().clone();
    run(private, keyboards, 2563, 272, true);
    assert!(matches!(
        private.terminal.native_pending.pointer().map(|hold| hold.status()),
        Some(private_native::Status::Retained(private_native::Residual::IncarnationMismatch))
    ));
    let retained = private
        .terminal
        .pending_custody
        .as_ref()
        .and_then(|custody| custody.completion.clone())
        .expect("the refused press left its custody held");

    // ANOTHER ADMITTED PRESS IS REFUSED, not allowed to overwrite it. Driven
    // directly so this asserts the guard rather than a full queue.
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let next = role.reserve(stamp, 9).expect("a reservation").accepted();
    let refused = private.run_ordered_input(
        keyboards,
        &{
            let route = button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(2564),
                273,
                true,
            );
            admit_for_direct_run(private, &route);
            route
        },
        &next,
        watch,
    );
    assert!(
        matches!(
            refused,
            Err(crate::PrivateExecutionRefusal::CustodyRetained)
        ),
        "the next operation is refused under its own cause rather than \
         replacing what is held"
    );
    let still = private
        .terminal
        .pending_custody
        .as_ref()
        .and_then(|custody| custody.completion.clone())
        .expect("and the held custody is still here");
    assert!(
        Arc::ptr_eq(&still, &retained),
        "the very handle the refused press left, not a replacement"
    );

    // And an instance holding it does not report itself empty.
    assert!(!private.terminal.is_empty());
    // Restore the withheld row without surrendering either source obligation.
    private.terminal.holds.push(original);
    assert!(Arc::ptr_eq(
        private.terminal.holds[0].custody.completion.as_ref().unwrap(),
        &original_cell,
    ));
    assert_eq!(retained.answer(), None);
}

/// Run one real request through the fixture's own producer and observe its
/// completion, so the grant is free for the next one.
fn fixture_run(
    fixture: &mut PreparedOrderedFixture,
    delivery: u64,
    button: u32,
    pressed: bool,
) -> PrivateOrderedRun {
    let route = button_to(
        fixture.surface,
        XAuthorityInputDeliveryId::from_raw(delivery),
        button,
        pressed,
    );
    fixture.ingress.submit(&fixture.keeper.lease(), route).expect("the order to accept it");
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().expect("a live runner");
    assert!(matches!(
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().expect("a sealed watch"))
            .expect("a step"),
        PrivateOrderedStep::Decided(_)
    ));
    let Some(PrivateOrderedItem::Ran { run, custody, .. }) = private.terminal.turn.pop() else {
        panic!("an accepted request runs")
    };
    assert!(custody.observe().expect("a readable completion").is_some());
    run
}

#[test]
fn a_blocked_head_stops_its_own_connection_while_another_recipient_progresses() {
    // Adopted from the independent review, which supplied the assertion I had
    // said was missing: that a blocked head stops ITS connection rather than
    // every connection. A second recipient is made the way one really
    // appears -- the first client gives up its grab and the second takes one
    // -- so its events are resolved by the source, not inserted by the test.
    let client = XServerFrontendClientId(7561);
    let mut fixture = prepared_ordered_fixture(client);
    let namespace = fixture.namespace;
    fixture_run(&mut fixture, 75610, 272, true);
    fixture_run(&mut fixture, 75611, 273, true);

    let other = XServerFrontendClientId(7562);
    let other_window = XResourceId::new(0x307562, 1);
    let (other_registration, other_channels) = {
        let private = fixture.runner.frontend.as_mut().expect("a live runner");
        let registry = &private.broker.registry;
        let context = namespaced(other, namespace);
        let (registration, channels) = registry
            .register_client_with_admission(other, Some(context))
            .expect("a fresh client registers");
        registry
            .attach_private_lifecycle(&registration, context)
            .expect("the boundary admits");
        let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry
            .attach_connection_state(
                &registration,
                namespace,
                selected.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .expect("its connection state attaches");
        {
            let mut state = selected.lock().expect("the selections");
            state.register(
                other_window,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 100,
                },
            );
            state.observe_mapped(other_window);
            state.update(other_window, Some((1 << 2) | (1 << 3)), None);
        }
        // Real owner operations, so the source resolves later presses to the
        // new owner itself. Nothing foreign is inserted into this executor.
        let mut grabs = registry.input_authority.lock().expect("the grab state");
        grabs.ungrab_pointer(namespace, client.raw());
        grabs
            .grab_pointer(
                namespace,
                crate::XActiveInputGrab {
                    owner: other.raw(),
                    window: other_window,
                    owner_events: false,
                    pointer_mode: 1,
                    keyboard_mode: 1,
                    event_mask: u16::MAX,
                    xi_event_mask: [0; 8],
                    xi_event_mask_words: 0,
                    route_lease: None,
                },
            )
            .expect("the grab to take");
        (registration, channels)
    };
    fixture_run(&mut fixture, 75612, 274, true);

    let private = fixture.runner.frontend.as_mut().expect("a live runner");
    assert_eq!(private.terminal.holds.len(), 3);
    // Both earlier presses really reached A, and the later one really reached
    // B: the source resolved them, and this control asserts that rather than
    // assuming it.
    assert_eq!(private.terminal.holds[0].reached.client(), client);
    assert_eq!(private.terminal.holds[1].reached.client(), client);
    assert_eq!(private.terminal.holds[2].reached.client(), other);
    let recovery = private.broker.registry.input_recovery.clone();
    let cells = [75610u64, 75611, 75612].map(|id| {
        recovery
            .completion_for(XAuthorityInputDeliveryId::from_raw(id))
            .expect("a readable ledger")
            .expect("its own admission")
    });

    // Stage the state between the phase write and the capsule take. No
    // interrupted send is claimed; the capsule is the one production built.
    let record = &mut private.terminal.holds[0];
    let emission = record
        .native
        .as_mut()
        .expect("a source obligation")
        .take_press_emission()
        .expect("the press built its event");
    PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, client);
    record.custody.dispatch = PrivateDispatchPhase::Indeterminate;

    let mut seen_blocked = Vec::new();
    let mut seen_other = Vec::new();
    for _ in 0..8 {
        // Each visit must succeed. Discarding the result would let a failing
        // step pass for an empty queue.
        private
            .deliver_one(None, &mut |_, _| Ok(()))
            .expect("a terminal step");
        while let Ok(capsule) = fixture.channels.ordered.try_recv() {
            seen_blocked.push(capsule.delivery());
        }
        while let Ok(capsule) = other_channels.ordered.try_recv() {
            assert_eq!(capsule.client(), other);
            assert!(Arc::ptr_eq(
                &cells[2],
                &capsule.finalizer().expect("a carried finalizer").completion
            ));
            seen_other.push(capsule.delivery());
        }
    }

    assert!(
        seen_blocked.is_empty(),
        "neither the unresolved head nor anything behind it reaches its own \
         recipient"
    );
    assert_eq!(
        seen_other,
        [XAuthorityInputDeliveryId::from_raw(75612)],
        "a distinct recipient still progresses: one blocked connection is not \
         a barrier for every connection"
    );
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Indeterminate
    );
    // THE EXACT CAPSULE, by its admission's own completion and not by the
    // number it carries. A delivery id can be pruned and handed out again, so
    // matching one establishes nothing about custody.
    assert!(
        matches!(
            private.terminal.holds[0].custody.pending.as_ref(),
            Some(PrivatePendingDelivery::Capsule(capsule))
                if capsule.delivery() == XAuthorityInputDeliveryId::from_raw(75610)
                    && Arc::ptr_eq(
                        &cells[0],
                        &capsule.finalizer().expect("a carried finalizer").completion
                    )
        ),
        "and the exact unknown-handover capsule stays owned here"
    );
    assert!(cells.iter().all(|cell| cell.answer().is_none()));
    drop(other_registration);
}

#[test]
fn an_unresolved_head_blocks_its_connection_without_being_offered_again() {
    // An unresolved handover must stay in the ordering comparison, because it
    // is what blocks the events behind it. Being the head is not permission to
    // act on it: its slot still holding bytes is exactly what an interruption
    // after the write-ahead leaves, and offering those bytes again is a replay
    // of an event the recipient may already have.
    let client = XServerFrontendClientId(7561);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
        channels,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, button) in [(75610u64, 272u32), (75611, 273)] {
        let lease = keeper.lease();
        ingress
            .submit(&lease, button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                true,
            ))
            .expect("the order to accept it");
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch)
            .expect("a decided step");
        let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.pop() else {
            panic!("an accepted request runs")
        };
        assert!(custody.observe().expect("a readable completion").is_some());
    }
    assert_eq!(private.terminal.holds.len(), 2);

    // Build the first press's capsule, then stage exactly what an interruption
    // after the write-ahead leaves: the phase saying the handover began, and
    // the capsule still in the slot.
    private
        .deliver_one(None, &mut |_, _| Ok(()))
        .expect("a step that prepares and hands over the first press");
    let queued: Vec<_> = std::iter::from_fn(|| channels.ordered.try_recv().ok()).collect();
    assert_eq!(
        queued.len(),
        1,
        "the first press went, which is what gives us a capsule to stage"
    );
    stage_interrupted_head(&mut private.terminal.holds[0], queued.into_iter().next().unwrap());

    // NOTHING MORE IS HANDED OVER FOR THIS RECIPIENT. The staged head blocks
    // the press behind it, and is not offered again itself.
    for _ in 0..8 {
        let _ = private.deliver_one(None, &mut |_, _| Ok(()));
    }
    assert!(
        channels.ordered.try_recv().is_err(),
        "an unresolved head is not re-sent, and nothing behind it passes"
    );
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Indeterminate,
        "and its phase is unchanged: a slot holding bytes is not permission"
    );
    assert!(
        private.terminal.holds[0].custody.pending.is_some(),
        "the exact capsule is still held, not consumed by an offer"
    );

    // AND IT IS NOT REPORTED AS A DISPATCH EITHER. The head is refused before
    // anything is taken from it, so the visit says it had no press to hand
    // over -- not that it tried one and the queue refused. The difference is
    // what the stall allowance counts, and counting an ineligible head as a
    // refused attempt spends the press path's allowance on a head that can
    // never use it.
    assert_eq!(
        private.dispatch_one_press(),
        None,
        "an unresolved head is not an attempted dispatch"
    );
}

#[test]
fn a_carried_older_press_is_handed_over_before_a_newer_held_press() {
    // A press whose hold has ended travels into the settling record. Choosing
    // held work first handed the later press over before the earlier one that
    // had merely moved, so the recipient would have seen a second button go
    // down before the first one it was already owed.
    let client = XServerFrontendClientId(7551);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        keeper,
        runner,
        ingress,
        channels,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // Decided before any terminal service: press, its release, then a second
    // press of a different button.
    for (delivery, button, pressed) in [
        (75510u64, 272u32, true),
        (75511, 272, false),
        (75512, 273, true),
    ] {
        let lease = keeper.lease();
        ingress
            .submit(&lease, button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                pressed,
            ))
            .expect("the order to accept it");
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch)
            .expect("a decided step");
        // The decision's own outcome is observed here, which is what frees the
        // grant for the next request. The handover is a separate fact and has
        // not happened yet.
        let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.pop() else {
            panic!("an accepted request runs")
        };
        assert!(custody.observe().expect("a readable completion").is_some());
    }

    // Now drive the handovers and watch the order they reach the queue in.
    let mut order = Vec::new();
    for _ in 0..12 {
        let _ = private.deliver_one(None, &mut |_, _| Ok(()));
        while let Ok(capsule) = channels.ordered.try_recv() {
            order.push(capsule.delivery());
        }
    }

    // THE WHOLE STREAM, in the order the recipient must see it. Checking only
    // that the first press led would have missed a later press overtaking the
    // release between them -- which is exactly what happened.
    assert_eq!(
        order,
        vec![
            XAuthorityInputDeliveryId::from_raw(75510),
            XAuthorityInputDeliveryId::from_raw(75511),
            XAuthorityInputDeliveryId::from_raw(75512),
        ],
        "press, its release, then the later press: one order for one \
         recipient, across press and release custody alike"
    );
}
