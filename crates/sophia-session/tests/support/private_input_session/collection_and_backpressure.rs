/// M4.lifetime subcase `unwind`: the invocation unwinds while collection is
/// still live.
///
/// AN UNWIND IN THE PLACE A REAL ONE WOULD HAPPEN. A panic raised after
/// `serve_until_stopped` returns proves nothing: the service's own collection
/// guard has already run, so the unwind passes through none of it. What has to
/// be established is that an invocation which unwinds WHILE that guard is live
/// still leaves its keeper, its custody and its obligations in order.
///
/// So the fault is armed only once the service is genuinely serving -- a real
/// authenticated peer, custody reporting `Running`, an admitted surface it
/// committed and an accepted obligation owned -- and it fires from inside an
/// event the serving thread actually emits when it reaps a client. A second
/// probe peer is connected and dropped to cause that reap; the primary peer and
/// its obligation stay live so the guard has something to unwind through. The
/// fault disarms itself before panicking, so unwinding cleanup that emits the
/// same event cannot panic a second time.
#[test]
fn an_unwind_on_the_serving_thread_still_collects() {
    let (mut fixture, fault) = Fixture::started_with_unwind_fault();
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let primary_place = fixture.running_row().place;
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    // ARMED ONLY NOW. Before this point there is no started worker and no owned
    // obligation, and an unwind there would be testing something else.
    fault.arm();

    // A SECOND PEER THAT NEVER FINISHES ITS HANDSHAKE. A fully connected peer
    // that closes cleanly is `Ok(())` to its worker and the reaper says nothing
    // about it; only a failure, a disconnect or a shutdown is reported. This
    // socket goes before its setup prefix, so the worker reports a real client
    // disconnect and the serving thread emits the event while its collection
    // guard is still live.
    drop(fixture.probe_socket());

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    // NO `not_yet` HERE: the fault is what ends the service, so an ending is
    // the thing waited for, not a reason to give up.
    spin_for(
        &mut (),
        "the armed fault firing on the serving thread",
        |_| format!("the service is {:?}", runtime.readiness()),
        |_| {
            if fault.fired() {
                Progress::Done(())
            } else {
                Progress::Idle
            }
        },
    );

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert!(
        matches!(
            outcome.invocation,
            super::handle::PrivateInputInvocation::Unwound(_)
        ),
        "the invocation is reported as having unwound, not as having returned: {outcome:?}"
    );
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "an unwound invocation still leaves a thread that joins: {outcome:?}"
    );
    // AN UNWIND REPORTS NOTHING OF ITS OWN, so the worker list stays absent
    // rather than being invented; custody is where collection is read from,
    // and it is required of this peer's own place rather than of any place a
    // probe might have left behind.
    assert_collected(&runtime, &outcome, &submission, primary_place, false);

    // A JOINED THREAD IS NOT A LIVE EXECUTION, even after an unwind.
    if let Some(execution) = outcome.execution {
        assert_ne!(
            execution.availability,
            sophia_x_authority::PrivateExecutionAvailability::Retained,
            "an unwound run that joined must not report a retained execution: {outcome:?}"
        );
    }
    // AND THE EXACT OBLIGATION IS RETAINED UNDER THE OUTER LIFETIME.
    assert!(
        outcome.retains_obligations(),
        "an unwind with an undrained receipt still owes it: {outcome:?}"
    );
    assert!(
        fixture.lifetime.retains_unresolved(),
        "and the reserved slot is holding that runtime"
    );
    assert_eq!(
        fixture.lifetime.unresolved_receipts(),
        Some(Some(1)),
        "the slot can say exactly what is owed"
    );
    drop((submission, peer));
}

/// Observing a receipt releases the place its delivery took, and the ledger
/// can be cycled through that release more than once.
///
/// WHAT THIS PROVES THAT NOTHING ELSE DOES. Everything else about receipts
/// establishes that Session keeps them and counts them. This establishes the
/// claim the whole design rests on: that handing one back is what frees the
/// delivery place it was holding. A receipt taken off the channel and dropped
/// would satisfy every other control here and still leak the place forever.
///
/// IT ASKS THE LEDGER, NOT A COUNTER. `state()` reports whether the ledger is
/// still holding a ticket for that exact delivery, so the release is observed
/// where it actually happens rather than inferred from a number Session keeps.
///
/// An earlier version tried to prove this by filling the declared bound until
/// it refused. That could never work: the bound is not the configured input
/// capacity but `input*2 + 2047*(input+1)` -- 6145 for a capacity of two --
/// because the ordinary ledger reserves a place for every resource range a
/// server could hand out. Sizing it to the private client bound instead does
/// make it reachable, but it also changes a bound M3 acceptance already
/// measures, so it is not a change to make inside M4 closure.
#[test]
fn observing_a_receipt_releases_the_place_its_delivery_took() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let surface = fixture.admitted_surface();
    fixture.focus(surface);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    // TWICE, because once is consistent with a place that was never taken.
    let mut pressed = true;
    for cycle in 0..2 {
        let accepted = submission
            .submit_pointer_button(surface, BTN_LEFT, pressed)
            .unwrap_or_else(|error| panic!("cycle {cycle} submitted: {error:?}"));
        pressed = !pressed;
        await_flushed(&runtime, accepted.delivery);

        // SETTLED BUT STILL HELD. The delivery has its terminal answer and the
        // ledger has not given the place back, because nobody has observed it.
        assert_eq!(
            runtime.observer.state(accepted.delivery),
            sophia_x_authority::DeliveryState::Live,
            "cycle {cycle}: a settled delivery still holds its place until observed"
        );

        // Take the receipt off the channel WITHOUT observing it. Taking it is
        // not what frees the place, and this is where that is established.
        let receipt = take_one_receipt(&runtime);
        assert_eq!(receipt.delivery, accepted.delivery, "cycle {cycle}");
        assert_eq!(
            runtime.observer.state(accepted.delivery),
            sophia_x_authority::DeliveryState::Live,
            "cycle {cycle}: taking the receipt off the channel frees nothing"
        );

        // Hand it back. THIS is the release.
        assert_eq!(
            runtime.observer.observe(receipt),
            sophia_x_authority::PrivateDeliveryObservation::Observed,
            "cycle {cycle}"
        );
        assert_eq!(
            runtime.observer.state(accepted.delivery),
            sophia_x_authority::DeliveryState::Ended,
            "cycle {cycle}: observing the receipt released its delivery's place"
        );

        // AND IT IS NOT RELEASABLE TWICE.
        assert_eq!(
            runtime.observer.observe(receipt),
            sophia_x_authority::PrivateDeliveryObservation::UnknownDelivery,
            "cycle {cycle}: a released place cannot be released again"
        );
    }

    drop((submission, peer));
}

/// Wait for one delivery's terminal answer to be exactly `Flushed`.
fn await_flushed(
    runtime: &std::sync::Arc<crate::private_input::service::PrivateInputRuntime>,
    delivery: sophia_x_authority::XAuthorityInputDeliveryId,
) {
    let settled = spin_for(
        &mut (),
        &format!("a terminal answer for delivery {delivery:?}"),
        |_| {
            format!(
                "the ledger holds it as {:?}",
                runtime.observer.state(delivery)
            )
        },
        |_| match runtime.observer.settled(delivery) {
            Some(settled) => Progress::Done(settled),
            None => match runtime.readiness() {
                PrivateInputReadiness::Ready => Progress::Idle,
                gone => Progress::Lost(format!("the service is {gone:?}")),
            },
        },
    );
    assert_eq!(
        settled.outcome,
        sophia_x_authority::XAuthorityInputDeliveryOutcome::Flushed,
        "delivery {delivery:?} settled without being delivered: {settled:?}"
    );
}

/// Take exactly one receipt off the channel, without observing it.
fn take_one_receipt(
    runtime: &std::sync::Arc<crate::private_input::service::PrivateInputRuntime>,
) -> sophia_x_authority::XAuthorityClientInputDelivery {
    runtime
        .deliveries
        .lock()
        .expect("the delivery channel is readable")
        .recv_timeout(WAIT)
        .expect("a receipt for a delivered event")
}

/// A poisoned command slot still stops the service rather than hanging on it.
///
/// WHAT THIS CATCHES, AND IT IS A DEADLOCK RATHER THAN A BAD REPORT. The stop
/// read the command slot with `.ok()`, so a poisoned slot answered `None`
/// while the real sender stayed stored. No StopAndDisconnect was sent, the
/// service's receiver was still connected so it never ended, and the wait on
/// the closed channel never returned. The slot is recovered for shutdown now,
/// and the poisoning is reported instead of standing in for an answer.
#[test]
fn a_poisoned_command_slot_still_stops_the_service() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);
    let primary_place = running.place;
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let poisoner = std::sync::Arc::clone(&runtime);
    let thread = std::thread::spawn(move || {
        let _held = poisoner.commands.lock().unwrap();
        panic!("poisoning the command slot on purpose");
    });
    assert!(thread.join().is_err(), "the poisoning thread panicked");

    // Ordinary access may refuse; shutdown may not.
    assert!(
        runtime.command_sender().is_none(),
        "ordinary command access refuses a poisoned slot"
    );

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert!(
        outcome.command_slot_poisoned,
        "the poisoning is reported as its own fact: {outcome:?}"
    );
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "the stop recovered the sender, ended the service and joined: {outcome:?}"
    );
    assert_collected(&runtime, &outcome, &submission, primary_place, true);
    drop(peer);
}

/// Observations the order never committed are owed at stop, not discarded.
///
/// WHAT THIS CATCHES. The stop counted what the bridge was holding and nothing
/// else, so batches the frontend had observed and handed over -- still queued
/// on the transaction channel, never taken by the bridge -- were reported as
/// nothing owed. They are taken into owned intake at shutdown instead, and
/// deliberately not committed: the order has stopped, and committing on its
/// behalf afterwards would be doing work for a service that has ended.
/// THE BARRIER IS A REAL ROUND TRIP, NOT A SPIN. An earlier version waited for
/// `retained_intake` to become non-empty, which only the stop below ever does,
/// so it burned its whole bound and established no prerequisite at all. It also
/// created a second window, but the peer uses a fixed XID, so that was the same
/// window rather than a fresh one. A draw on the window already admitted,
/// followed by a GetGeometry reply, proves the server processed the draw --
/// and nothing here drives a commit, so the observation is still queued.
#[test]
fn queued_intake_nobody_committed_is_owed_at_stop() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    // The surface this service actually admitted, so the batch retained below
    // can be identified by that exact surface rather than by a count.
    let surface = fixture.admitted_surface();
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    // Draw again on the same window and DO NOT pump a commit for it. The
    // geometry reply is the barrier: the server has processed the draw, so the
    // observation has been made and is sitting on the transaction channel with
    // nothing about to take it.
    peer.draw(window);
    peer.confirm_geometry(window);

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    let owed = outcome
        .intake_uncommitted
        .expect("the intake queue and the channel were both readable");
    assert!(
        owed > 0,
        "observations the order never committed are owed at stop: {outcome:?}"
    );
    let retained = runtime.retained_intake.lock().unwrap();
    assert_eq!(
        retained.len(),
        owed,
        "and they are held rather than counted and dropped"
    );
    // THE EXACT SURFACE, NOT ANY QUEUED LIFECYCLE BATCH. A count alone would be
    // satisfied by whatever else happened to be in flight at shutdown.
    assert!(
        retained.iter().any(|batch| {
            batch
                .transactions
                .iter()
                .any(|transaction| transaction.surface == surface)
        }),
        "the retained intake carries the draw for surface {surface:?}: {retained:?}"
    );
    drop(retained);
    assert!(
        outcome.retains_obligations(),
        "uncommitted intake retains custody: {outcome:?}"
    );
    assert!(
        fixture.lifetime.retains_unresolved(),
        "and the reserved slot is holding the runtime that owes it"
    );
    drop(peer);
}

/// An unreadable transaction channel is refused, never read as no intake.
///
/// WHAT THIS CATCHES. The drain answered a poisoned receiver and a quiet one
/// with the same empty list, so a caller driving commits reported a step that
/// observed nothing and carried on, while intake it could no longer reach
/// accumulated behind the lock. `apply_committed` now refuses instead, and the
/// stop can no longer put a number on intake it was unable to read: it says
/// `None`, which is not none owed, and custody is retained on that basis
/// alone.
#[test]
fn an_unreadable_transaction_channel_is_refused_and_still_retains() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let poisoner = std::sync::Arc::clone(&runtime);
    let thread = std::thread::spawn(move || {
        let _held = poisoner.transactions.lock().unwrap();
        panic!("poisoning the transaction channel on purpose");
    });
    assert!(thread.join().is_err(), "the poisoning thread panicked");

    // REFUSED, NOT REPORTED AS AN EMPTY STEP.
    let refused = fixture
        .handle_mut()
        .apply_committed(Duration::from_millis(10));
    assert!(
        refused.is_err(),
        "an unreadable transaction channel is refused: {refused:?}"
    );

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert_eq!(
        outcome.intake_uncommitted, None,
        "intake that could not be read reports no count at all, not a count of zero: {outcome:?}"
    );
    assert!(
        outcome.retains_obligations(),
        "nothing has been shown to be finished, so custody is retained: {outcome:?}"
    );
    assert!(
        fixture.lifetime.retains_unresolved(),
        "and the reserved slot is holding that runtime"
    );
    drop(peer);
}

/// A surface that was drawn but never mapped gets no route; mapping it is what
/// admits it, and drawing again after that configures rather than re-admits.
///
/// THE DISCRIMINATOR IS ONE REQUEST. `create_map_and_draw`, which every other
/// control here uses, is exactly `create_unmapped` + `map` + `draw`. This runs
/// the same sequence with the `MapWindow` left out, so the only thing that can
/// account for a different outcome is the mapping fact itself.
///
/// WHAT THIS CATCHES. Nothing else here ever draws an unmapped window, so the
/// guard that refuses to route one was never exercised: deleting it left all
/// twenty-two controls green. An unmapped passive helper picking up an input
/// route is precisely what the private path must not do.
///
/// It also refuses to pass vacuously. Asserting "no admission" alone would hold
/// just as well if nothing had committed at all, so the first phase requires the
/// commit to have applied the surface while this service held no mapping fact
/// for it -- the exact state the guard exists to act on.
#[test]
fn an_unmapped_surface_gets_no_route_until_it_is_mapped() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );

    // DRAWN, NEVER MAPPED.
    let window = peer.create_unmapped();
    peer.draw(window);
    peer.confirm_geometry(window);

    // WAIT ON WHAT THE MUTATION CANNOT CHANGE. The vacuity guard has to be
    // anchored on `applied`, which is simply what the coordinator committed. An
    // earlier version waited for `mapped` to be empty as well, and that field is
    // written by the very code under test -- so the defect made the wait time
    // out instead of making the routing assertion below fire, and the control
    // reported a timeout rather than the fault it had actually found.
    let unmapped = wait_for(
        &mut fixture,
        "a commit for the unmapped draw",
        |fixture| format!("the harvest holds {:?}", fixture.harvest),
        |fixture| {
            fixture.step(|_, report| {
                report
                    .outcomes
                    .iter()
                    .find(|outcome| !outcome.applied.is_empty())
                    .map(|outcome| outcome.applied.clone())
            })
        },
    );
    // Give the bridge a moment to stage anything that commit decided, so the
    // assertion below is about what was routed rather than about what has not
    // been reached yet.
    for _ in 0..8 {
        fixture.pump();
    }
    assert!(
        fixture
            .harvest
            .iter()
            .all(|effect| effect.kind() != XAuthorityControlKind::AdmitSurface),
        "a surface this service holds no mapping fact for was routed anyway: {:?}",
        fixture.harvest
    );

    // NOW MAP IT. This is the only request that changes, and it is what admits.
    peer.map(window);
    peer.confirm_geometry(window);

    wait_for(
        &mut fixture,
        "an admission for the mapped window",
        |fixture| format!("the harvest holds {:?}", fixture.harvest),
        |fixture| {
            fixture.step(|fixture, _| {
                fixture
                    .harvest
                    .iter()
                    .any(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface)
                    .then_some(())
            })
        },
    );

    let admissions: Vec<_> = fixture
        .harvest
        .iter()
        .filter(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface)
        .collect();
    assert_eq!(
        admissions.len(),
        1,
        "mapping admits the surface exactly once: {admissions:?}"
    );
    let admitted = admissions[0];
    assert!(
        admitted.geometry().is_some(),
        "the admission carries the geometry the coordinator committed: {admitted:?}"
    );
    assert!(
        unmapped.contains(&admitted.surface()),
        "the surface admitted on mapping is the one drawn while unmapped: {admitted:?} of {unmapped:?}"
    );

    // AND A LATER DRAW CONFIGURES RATHER THAN ADMITTING AGAIN.
    peer.draw(window);
    peer.confirm_geometry(window);
    wait_for(
        &mut fixture,
        "a configure for the admitted surface after a further draw",
        |fixture| format!("the harvest holds {:?}", fixture.harvest),
        |fixture| {
            fixture.step(|fixture, _| {
                fixture
                    .harvest
                    .iter()
                    .any(|effect| effect.kind() == XAuthorityControlKind::ConfigureSurface)
                    .then_some(())
            })
        },
    );
    assert_eq!(
        fixture
            .harvest
            .iter()
            .filter(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface)
            .count(),
        1,
        "the surface is admitted once and configured afterwards, never re-admitted"
    );
    drop(peer);
}

/// Draining a receipt hands it to the caller and frees its place together.
///
/// WHAT THIS CATCHES. The release control beside this one proves that observing
/// a receipt frees its delivery's place, but it does that through the ledger
/// directly. Nothing covered the drain itself, so a drain that observed the
/// receipt and then dropped it -- freeing the place while the caller never
/// learns which receipt freed it -- went unnoticed. The two have to happen
/// together or the caller cannot account for what it no longer holds.
#[test]
fn a_drain_returns_the_receipts_whose_places_it_freed() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let surface = fixture.admitted_surface();
    fixture.focus(surface);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    let mut pressed = true;
    let mut sent = Vec::new();
    for _ in 0..2 {
        let accepted = submission
            .submit_pointer_button(surface, BTN_LEFT, pressed)
            .expect("the connection is live and has authority");
        pressed = !pressed;
        await_flushed(&runtime, accepted.delivery);
        assert_eq!(
            runtime.observer.state(accepted.delivery),
            sophia_x_authority::DeliveryState::Live,
            "settled and still holding its place, because nobody has drained it"
        );
        sent.push(accepted.delivery);
    }

    let receipts = fixture
        .handle()
        .drain_deliveries()
        .expect("the channel and the queue are readable");

    // THE RECEIPTS COME BACK, AND THEY ARE THE ONES THAT WERE FREED.
    let mut observed: Vec<_> = receipts.observed.iter().map(|r| r.delivery).collect();
    observed.sort_by_key(|d| d.raw());
    let mut expected = sent.clone();
    expected.sort_by_key(|d| d.raw());
    assert_eq!(
        observed, expected,
        "the drain returned exactly the receipts it observed: {receipts:?}"
    );
    assert_eq!(receipts.retained, 0, "nothing was left owed: {receipts:?}");
    for delivery in &sent {
        assert_eq!(
            runtime.observer.state(*delivery),
            sophia_x_authority::DeliveryState::Ended,
            "and each returned receipt's place really was released"
        );
    }
    drop((submission, peer));
}

/// A stop that reports the service thread collected must have waited for it.
///
/// WHAT THIS CATCHES. `stop` can drop the join handle and report `Joined`
/// anyway; the thread is then detached and the claim is simply untrue. Every
/// twenty-two controls passed with that in place, including the one asserting
/// the execution is not `Retained` after the join -- which is a real assertion
/// that merely lost a race: the report is sent one statement before the closure
/// ends, so the keeper's abandonment usually lands before the read either way.
///
/// This makes the difference observable instead of likely. The thread lingers
/// past its last message and marks its exit only at the very end, so a stop that
/// joined provably waits through that and a stop that did not provably returns
/// before the mark. The linger is not a contrivance: the absence of a wait
/// cannot be seen unless something is still there to be waited for.
#[test]
fn a_stop_that_reports_the_thread_collected_really_joined_it() {
    let directory = std::env::temp_dir().join(format!(
        "m4-session-exit-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    let socket = directory.join("private.sock");
    let lifetime = PrivateInputLifetimeOwner::reserved();
    let marker = std::sync::Arc::new(
        crate::private_input::faults::PrivateInputExitMarker::lingering(Duration::from_millis(150)),
    );
    let handle = lifetime
        .start_with_faults(
            config(
                &socket,
                PrivateInputGrantPolicy::EnabledWithVerifiedEvidence,
            ),
            crate::private_input::faults::PrivateInputFaults {
                exit: Some(std::sync::Arc::clone(&marker)),
                ..Default::default()
            },
        )
        .expect("the service starts");
    assert_eq!(
        handle.await_ready(WAIT).unwrap(),
        PrivateInputReadiness::Ready
    );

    let mut peer = Peer::connect(&socket, Order::Little, Some(COOKIE)).unwrap();
    let _window = peer.create_map_and_draw();

    assert!(
        !marker.exited(),
        "the thread is still serving before the stop"
    );
    let outcome = handle.stop();
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "the stop reports the thread collected: {outcome:?}"
    );
    assert!(
        marker.exited(),
        "and it really waited for it: a stop that reported Joined returned while \
         the thread was still running"
    );

    drop(peer);
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_dir(&directory);
}

/// A control the order refuses for now stays owed, keeps its transaction, and
/// is delivered later without anything overtaking it.
///
/// WHAT THIS CATCHES. The bridge stops at the first refusal that says "later"
/// and leaves the entry at the head. Nothing exercised that, so removing the
/// stop -- dropping the entry instead of keeping it -- left every control
/// green. Work the coordinator committed and the order declined for capacity
/// would simply have vanished.
///
/// THE REFUSAL IS THE ORDER'S OWN. The ready queue admits controls up to
/// `input_capacity * 2`, and it is refilled one entry per serve turn with a
/// socket write in between, while the bridge delivers a whole staged batch in a
/// tight loop under one held lock. A burst of draws therefore outruns it and
/// earns a real `Saturated`. An earlier attempt drew one at a time and
/// concluded the bound was unreachable; it was aiming at a different, larger
/// gate and never got near this one.
#[test]
fn a_control_refused_for_now_keeps_its_place_and_its_transaction() {
    let mut fixture =
        Fixture::started_with_bounds(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence, 4, 2);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let _surface = fixture.admitted_surface();

    // Just past what the queue will take at once. The ceiling here is
    // `input_capacity * 2` = 4, and the burst is kept close to it on purpose:
    // this peer never reads the events it is sent, so every extra draw is
    // pressure on a socket that cannot drain, and a bigger burst buys nothing
    // but flakiness under load.
    for _ in 0..10 {
        peer.draw(window);
    }
    peer.confirm_geometry(window);

    let refused = wait_for(
        &mut fixture,
        "a control refused for capacity (the ceiling is input_capacity * 2)",
        |fixture| format!("the order still owes {:?}", fixture.handle().outstanding()),
        |fixture| {
            let report = fixture
                .handle_mut()
                .apply_committed(Duration::from_millis(50))
                .expect("the bridge is readable");
            if ended(&report) {
                return Progress::Lost(fixture.ended_report());
            }
            // The classification is restated here rather than borrowed from
            // the bridge, so a change to what counts as "later" cannot quietly
            // change what this control is testing.
            let refused = report.refused.iter().find_map(|refusal| match refusal {
                crate::private_input::PrivateInputControlError::Refused(
                    sophia_x_authority::AdmissionRefusal::Saturated
                    | sophia_x_authority::AdmissionRefusal::Unavailable
                    | sophia_x_authority::AdmissionRefusal::AuthorityUnreadable,
                    command,
                ) => Some(*command),
                _ => None,
            });
            match refused {
                Some(command) => Progress::Done(command),
                None if advanced(&report) => Progress::Worked,
                None => Progress::Idle,
            }
        },
    );
    let refused_transaction = control_transaction(&refused);

    // STILL OWED, NOT DISCARDED. This is what the mutation destroys.
    let owed = fixture
        .handle()
        .outstanding()
        .expect("the bridge is readable");
    assert!(
        owed >= 1,
        "a control the order deferred is kept rather than dropped"
    );

    // NOTHING BEHIND IT OVERTOOK IT, AND IT KEPT ITS OWN TRANSACTION. Minting a
    // fresh one would leave the first outstanding and unanswerable, and two
    // acknowledgements could then arrive for one committed decision.
    wait_for(
        &mut fixture,
        &format!("the deferred control going out under transaction {refused_transaction:?}"),
        |fixture| format!("the order still owes {:?}", fixture.handle().outstanding()),
        |fixture| {
            let report = fixture
                .handle_mut()
                .apply_committed(Duration::from_millis(50))
                .expect("the bridge is readable");
            if ended(&report) {
                return Progress::Lost(fixture.ended_report());
            }
            let mut delivered = false;
            for submitted in report
                .effects
                .iter()
                .filter_map(|effect| effect.submitted())
            {
                if submitted.transaction == refused_transaction {
                    delivered = true;
                } else {
                    assert!(
                        submitted.transaction < refused_transaction || delivered,
                        "a control minted after the refused one was delivered ahead of it: \
                         {submitted:?} before {refused_transaction:?}"
                    );
                }
            }
            match delivered {
                true => Progress::Done(()),
                false if advanced(&report) => Progress::Worked,
                false => Progress::Idle,
            }
        },
    );
    drop(peer);
}

/// How long a wait tolerates nothing happening at all.
///
/// TODAY'S WHOLE BOUND, DELIBERATELY. Nothing measured says how long a gap
/// between two units of work may get on a starved host; what was measured is
/// that a *total* of twenty seconds is too small. Making the gap bound the old
/// total makes this a strict relaxation: a gap this long implies a total this
/// long, so nothing that passes today can fail here.
const STALL: Duration = Duration::from_secs(20);

/// How long a wait may keep seeing work and still never arrive.
///
/// A BACKSTOP, NOT A BUDGET. A wait that reaches this has been fed work the
/// whole time and still has no answer, which is a different failure from a
/// starved one and says so. It is also what keeps these controls able to kill
/// a mutation that leaves the bridge busy but never delivers.
const CEILING: Duration = Duration::from_secs(120);

/// The bound for a wait with no progress signal, where only the number can
/// move. Three times the old one: a passing spin costs nothing, so the only
/// price of a generous number is how long a genuinely broken run takes.
const UNSIGNALLED: Duration = Duration::from_secs(60);

/// What one attempt at a wait observed.
enum Progress<T> {
    /// The thing waited for is here.
    Done(T),
    /// Not here, but the system moved, so the stall bound starts again.
    Worked,
    /// Not here, and nothing happened.
    Idle,
    /// Cannot arrive any more, for the reason given. The wait fails at once
    /// rather than spending a bound on a service that has already ended.
    Lost(String),
}
