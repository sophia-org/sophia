// Controls for the deferred branch of registered destruction: a registration
// dropped over a worker that was ever started, handed on, or whose departure
// could not be established. Harness in `private_destruction.rs`.
//
// Every worker here is real and registered through the connection's own
// bound control context, held live across the drop, released only after the
// observations have been taken, and collected before anything is compared.

/// A registered worker held live on a worker fixture, started through the
/// connection's own context.
struct HeldWorker {
    let_go: std::sync::mpsc::SyncSender<()>,
    seen: std::sync::mpsc::Receiver<&'static str>,
}

fn start_held_worker(
    context: &PrivateControlContext<'_>,
    f: &PrivateWorkerFixture,
) -> HeldWorker {
    let (let_go, hold) = sync_channel::<()>(1);
    let (saw, seen) = sync_channel(1);
    let body = held_worker(Arc::clone(&f.stop), Arc::clone(&f.wake), hold, saw);
    assert_eq!(
        context.start(|| std::thread::Builder::new().spawn(body)),
        PrivateStartupOutcome::Started
    );
    HeldWorker { let_go, seen }
}

impl HeldWorker {
    /// Let the worker go and learn what it saw first, bounded.
    fn release(&self) -> Option<&'static str> {
        let _ = self.let_go.send(());
        self.seen.recv_timeout(Duration::from_secs(5)).ok()
    }
}

#[test]
fn destroying_a_registration_over_a_live_worker_stops_it_and_defers_its_duty() {
    let f = worker_fixture(XServerFrontendClientId(9301));
    f.permit();
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let worker = start_held_worker(&context, &f);
    assert!(!f.stop.load(std::sync::atomic::Ordering::Acquire));
    let unheld = dropped_without_waiting(f.fixture.registration, || {
        let _ = worker.let_go.send(());
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    let worker_saw = worker.release();
    let reaping = PrivateReapingRecord::bound_to(&custody).reap();
    let after = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    assert_eq!(reaping.reaped, PrivateReaped::Joined, "collected by the custodian");
    assert!(unheld, "the drop returned without waiting for the held worker");
    assert_eq!(worker_saw, Some("stopped"), "the stop reached the worker through its pair");
    assert!(seen.stop, "the stop was published before the frame returned");
    assert_eq!(seen.departure, Some(PrivateDeparture::WorkerRunning));
    assert_eq!(seen.life, PrivateWorkerLife::Running);
    assert!(seen.handle_in_slot, "no join happened in Drop: the handle was still there");
    assert_deferred_untouched(&seen, PrivateDestructionDeferral::WorkerRunning, "live");
    assert_eq!(
        after.decision,
        Some(PrivateDestructionDecision::Deferred(
            PrivateDestructionDeferral::WorkerRunning
        )),
        "a join afterwards does not turn the record into something else"
    );
    assert_eq!(after.number, Some(PrivateNumberStanding::Held));
    assert!(after.row, "and the row is still the custodian's to remove");
    drop(custody);
}

#[test]
fn destruction_while_an_admitted_spawn_holds_its_destination_stops_before_it_waits() {
    let f = worker_fixture(XServerFrontendClientId(9302));
    f.permit();
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let (let_go, hold) = sync_channel::<()>(1);
    let (saw, seen_by_worker) = sync_channel(1);
    let body = held_worker(Arc::clone(&f.stop), Arc::clone(&f.wake), hold, saw);
    let (inside, entered) = sync_channel::<()>(1);
    let (go, wait_here) = sync_channel::<()>(1);
    let registration = f.fixture.registration;
    let mut stop_before_slot = false;
    let mut slot_still_held = false;
    let mut drop_still_blocked = false;
    let mut drop_unheld = false;
    let mut started = None;
    std::thread::scope(|scope| {
        let starter = scope.spawn(move || {
            context.start(move || {
                inside.send(()).expect("the control is listening");
                wait_here.recv().expect("the control lets go");
                std::thread::Builder::new().spawn(body)
            })
        });
        entered.recv().expect("the spawn is in flight, holding the slot");
        let dropper = scope.spawn(move || drop(registration));
        stop_before_slot = waited_until(
            || f.stop.load(std::sync::atomic::Ordering::Acquire),
            Duration::from_secs(5),
        );
        slot_still_held = custody.worker_slot().try_lock().is_err();
        std::thread::sleep(Duration::from_millis(100));
        drop_still_blocked = !dropper.is_finished();
        go.send(()).expect("the spawner is waiting");
        started = Some(starter.join().expect("the starter returns"));
        drop_unheld = waited_until(|| dropper.is_finished(), Duration::from_secs(5));
        if !drop_unheld {
            let _ = let_go.send(());
        }
        dropper.join().expect("the drop returns");
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    let _ = let_go.send(());
    let worker_saw = seen_by_worker.recv_timeout(Duration::from_secs(5)).ok();
    let reaped = PrivateReapingRecord::bound_to(&custody).reap().reaped;
    assert_eq!(reaped, PrivateReaped::Joined);
    assert_eq!(started, Some(PrivateStartupOutcome::Started), "the admitted start went ahead");
    assert!(stop_before_slot, "the stop was published while the spawn held the slot");
    assert!(slot_still_held, "and the slot really was held when it was");
    assert!(drop_still_blocked, "the drop waited for the slot, after the stop, not before");
    assert!(drop_unheld, "and for nothing else: it returned with the worker still held");
    assert_eq!(worker_saw, Some("stopped"));
    assert_deferred_untouched(&seen, PrivateDestructionDeferral::WorkerRunning, "held slot");
    drop(custody);
}

#[test]
fn a_handle_already_handed_to_a_joiner_leaves_destruction_deferred_as_handed_on() {
    let f = worker_fixture(XServerFrontendClientId(9303));
    f.permit();
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let worker = start_held_worker(&context, &f);
    let handoff = hand_worker_to_joiner(custody.worker_slot());
    let handle = handoff.handle.expect("the joiner took the handle");
    assert_eq!(handoff.found, PrivateWorkerLife::Running);
    let unheld = dropped_without_waiting(f.fixture.registration, || {
        let _ = worker.let_go.send(());
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    let worker_saw = worker.release();
    handle.join().expect("the joiner collects it");
    assert!(unheld, "the drop returned without waiting for the held worker");
    assert_eq!(worker_saw, Some("stopped"), "the stop still reached it");
    assert!(seen.stop);
    assert_eq!(seen.departure, Some(PrivateDeparture::WorkerHandedOn));
    assert_eq!(seen.life, PrivateWorkerLife::HandedToJoiner);
    assert!(!seen.handle_in_slot);
    assert_deferred_untouched(&seen, PrivateDestructionDeferral::WorkerHandedOn, "handed on");
    drop(custody);
}

#[test]
fn an_unreadable_departure_boundary_defers_and_still_stops_through_the_bound_pair() {
    let f = worker_fixture(XServerFrontendClientId(9304));
    f.permit();
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let worker = start_held_worker(&context, &f);
    // STAGE-ONLY: a holder unwinds inside the departure boundary, so nothing
    // about admission or the published pair can be read afterwards.
    let poisoner = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _inside = custody.source.departure.lock().expect("an open boundary");
                panic!("a holder unwound inside the departure boundary");
            })
            .join()
    });
    assert!(poisoner.is_err(), "the holder panicked");
    assert_eq!(custody.startup_admitted(), None, "and the boundary is unreadable");
    let unheld = dropped_without_waiting(f.fixture.registration, || {
        let _ = worker.let_go.send(());
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    let worker_saw = worker.release();
    let reaped = PrivateReapingRecord::bound_to(&custody).reap().reaped;
    assert_eq!(reaped, PrivateReaped::Joined);
    assert!(unheld, "the drop returned without waiting for the held worker");
    assert_eq!(worker_saw, Some("stopped"), "the bound pair still reached it");
    assert!(seen.stop);
    assert_eq!(seen.admitted, None);
    assert_eq!(seen.departure, None, "nothing was decided");
    assert_eq!(
        seen.decision,
        Some(PrivateDestructionDecision::Deferred(
            PrivateDestructionDeferral::BoundaryUnreadable
        ))
    );
    assert_eq!(seen.number, Some(PrivateNumberStanding::Held));
    assert!(seen.row && seen.lease && !seen.gate_fenced);
    assert_eq!(seen.home, PrivateHomeStanding::Live);
    drop(custody);
}

#[test]
fn an_interrupted_departure_leaves_destruction_deferred_as_deciding() {
    let f = worker_fixture(XServerFrontendClientId(9305));
    f.permit();
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let worker = start_held_worker(&context, &f);
    // STAGE-ONLY SCHEDULING HOOK: an earlier ask closed admission and was
    // interrupted inside its decision. That is what the boundary records
    // when a departure unwinds between its stop and its slot; it is written
    // here directly because no hook-free schedule reaches it.
    {
        let mut state = custody.source.departure.lock().expect("a readable boundary");
        state.admitted = false;
        state.deciding = true;
    }
    let unheld = dropped_without_waiting(f.fixture.registration, || {
        let _ = worker.let_go.send(());
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    // The interrupted ask is the one that owed the stop; this one sent none.
    cancel_connection_worker(&f.stop, &f.wake);
    let worker_saw = worker.release();
    let reaped = PrivateReapingRecord::bound_to(&custody).reap().reaped;
    assert_eq!(reaped, PrivateReaped::Joined);
    assert!(unheld, "the drop returned without waiting for the held worker");
    assert_eq!(worker_saw, Some("stopped"));
    assert!(!seen.stop, "a second departure inside another's decision sends no second stop");
    assert_eq!(seen.departure, None, "and records no decision of its own");
    assert_deferred_untouched(&seen, PrivateDestructionDeferral::Deciding, "deciding");
    drop(custody);
}

#[test]
fn a_recorded_running_departure_does_not_become_never_started_when_destruction_asks() {
    let f = worker_fixture(XServerFrontendClientId(9306));
    f.permit();
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let worker = start_held_worker(&context, &f);
    assert_eq!(
        context.depart(),
        PrivateDeparted::Decided(PrivateDeparture::WorkerRunning),
        "an earlier ask found the worker"
    );
    let unheld = dropped_without_waiting(f.fixture.registration, || {
        let _ = worker.let_go.send(());
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    let worker_saw = worker.release();
    let reaped = PrivateReapingRecord::bound_to(&custody).reap().reaped;
    assert_eq!(reaped, PrivateReaped::Joined);
    assert!(unheld, "the drop returned without waiting for the held worker");
    assert_eq!(worker_saw, Some("stopped"));
    assert_eq!(seen.departure, Some(PrivateDeparture::WorkerRunning));
    assert_deferred_untouched(&seen, PrivateDestructionDeferral::WorkerRunning, "recorded");
    drop(custody);
}

#[test]
fn the_started_branch_completes_while_the_home_and_gate_are_held() {
    let f = worker_fixture(XServerFrontendClientId(9307));
    f.permit();
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let worker = start_held_worker(&context, &f);
    let registration = f.fixture.registration;
    let home = Arc::clone(&custody.cleanup_record().ordered_home);
    let gate = Arc::clone(custody.gate());
    let mut finished_while_held = false;
    std::thread::scope(|scope| {
        let home_held = home.state.lock().expect("a readable home");
        let gate_held = gate.fenced.lock().expect("an open gate");
        let dropper = scope.spawn(move || drop(registration));
        finished_while_held = waited_until(|| dropper.is_finished(), Duration::from_secs(5));
        // Let go of both before joining, so a branch that did enter them
        // can finish and be reported rather than deadlocked.
        drop(gate_held);
        drop(home_held);
        if !finished_while_held {
            let _ = worker.let_go.send(());
        }
        dropper.join().expect("the drop returns");
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    let worker_saw = worker.release();
    let reaped = PrivateReapingRecord::bound_to(&custody).reap().reaped;
    assert_eq!(reaped, PrivateReaped::Joined);
    assert!(
        finished_while_held,
        "the started branch never entered the home or the gate"
    );
    assert_eq!(worker_saw, Some("stopped"));
    assert_deferred_untouched(&seen, PrivateDestructionDeferral::WorkerRunning, "held home");
    drop(custody);
}

#[test]
fn the_never_started_branch_waits_on_a_held_gate_after_its_stop_is_decided() {
    // The discriminator for the control above: the same held gate really
    // does hold up the branch that fences it, so completing under it is a
    // fact about which branch ran.
    let f = worker_fixture(XServerFrontendClientId(9308));
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let registration = f.fixture.registration;
    let gate = Arc::clone(custody.gate());
    let mut blocked_while_held = false;
    let mut decided_while_blocked = None;
    std::thread::scope(|scope| {
        let gate_held = gate.fenced.lock().expect("an open gate");
        let dropper = scope.spawn(move || drop(registration));
        let decided = waited_until(
            || custody.departure_observation().is_some(),
            Duration::from_secs(5),
        );
        std::thread::sleep(Duration::from_millis(100));
        blocked_while_held = decided && !dropper.is_finished();
        decided_while_blocked = custody.departure_observation();
        drop(gate_held);
        dropper.join().expect("the drop returns once the gate is let go");
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    assert!(blocked_while_held, "the never-started body fences the gate and waited for it");
    assert_eq!(decided_while_blocked, Some(PrivateDeparture::NothingStarted));
    assert_eq!(seen.decision, Some(PrivateDestructionDecision::Synchronous));
    assert!(seen.gate_fenced, "and it fenced once it could");
    assert_eq!(seen.number, None);
    drop(custody);
}

#[test]
fn a_slot_already_marked_departing_outside_the_boundary_leaves_destruction_deferred() {
    let f = worker_fixture(XServerFrontendClientId(9309));
    f.permit();
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let worker = start_held_worker(&context, &f);
    // STAGE-ONLY: a departure decided on the slot directly, bypassing the
    // registered boundary. No production path does this; it is the one way
    // to reach the slot's own "already departing" answer.
    assert_eq!(
        decide_departure(custody.worker_slot()),
        PrivateDeparture::WorkerRunning
    );
    let unheld = dropped_without_waiting(f.fixture.registration, || {
        let _ = worker.let_go.send(());
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    let worker_saw = worker.release();
    let reaped = PrivateReapingRecord::bound_to(&custody).reap().reaped;
    assert_eq!(reaped, PrivateReaped::Joined);
    assert!(unheld);
    assert_eq!(worker_saw, Some("stopped"), "the registered ask still sent its stop");
    assert_eq!(seen.departure, Some(PrivateDeparture::AlreadyDeparting));
    assert_deferred_untouched(
        &seen,
        PrivateDestructionDeferral::AlreadyDeparting,
        "already departing",
    );
    drop(custody);
}

#[test]
fn an_unreadable_slot_leaves_destruction_deferred_with_its_stop_sent() {
    let f = worker_fixture(XServerFrontendClientId(9310));
    f.permit();
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let worker = start_held_worker(&context, &f);
    // STAGE-ONLY: a holder unwinds inside the worker slot, so what it holds
    // cannot be read by the departure that follows.
    let poisoner = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _inside = custody.worker_slot().lock().expect("a readable slot");
                panic!("a holder unwound inside the worker slot");
            })
            .join()
    });
    assert!(poisoner.is_err(), "the holder panicked");
    let unheld = dropped_without_waiting(f.fixture.registration, || {
        let _ = worker.let_go.send(());
    });
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, &f.stop);
    let worker_saw = worker.release();
    let reaping = PrivateReapingRecord::bound_to(&custody).reap();
    assert_eq!(reaping.reaped, PrivateReaped::Joined);
    assert!(reaping.slot_poisoned, "collected through the poisoned slot");
    assert!(unheld);
    assert_eq!(worker_saw, Some("stopped"), "the stop went out before the slot was read");
    assert_eq!(seen.departure, Some(PrivateDeparture::Unreadable));
    assert_deferred_untouched(&seen, PrivateDestructionDeferral::SlotUnreadable, "slot");
    drop(custody);
}
