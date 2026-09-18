// Controls for the registered destruction decision: what a registration's
// real `Drop` decides when its connection has a private source, and what it
// leaves behind. Every control here drops the actual registration -- never a
// record acted on directly -- with the actual service keeper (the outer
// owner the private service guarantees) kept through release and collection.
//
// THE OBSERVATIONS ARE TAKEN BEFORE THE WORKER IS RELEASED, into locals, and
// compared only after the worker has been collected: a failing observation
// must report as a failure, not abandon a live thread behind a panic.
//
// STAGE-ONLY ARRANGEMENTS ARE LABELLED where they appear. Nothing here
// attaches a private worker to the production service, and nothing here
// executes a deferred duty.

/// Arm the arbitration's labelled hook for the next departure on this
/// thread (see `STAGE_AFTER_DEPARTURE_BOUNDARY` in
/// `private_departure_arbitration.rs`).
fn stage_after_boundary(hook: impl FnOnce() + 'static) {
    STAGE_AFTER_DEPARTURE_BOUNDARY.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

/// Everything destruction is expected to leave in one state or another,
/// read once and compared later.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DestructionObserved {
    /// The connection's own bound stop, when the control holds that
    /// credential; `None` where it does not, which is not a reading.
    stop: Option<bool>,
    admitted: Option<bool>,
    departure: Option<PrivateDeparture>,
    decision: Option<PrivateDestructionDecision>,
    standing: PrivateDestructionStanding,
    number: Option<PrivateNumberStanding>,
    row: bool,
    lease: bool,
    home: PrivateHomeStanding,
    gate_fenced: bool,
    life: PrivateWorkerLife,
    handle_in_slot: bool,
}

fn observe_destruction(
    custody: &PrivateEvidenceCustody,
    registry: &XServerFrontendRouteRegistry,
    client: XServerFrontendClientId,
    stop: Option<&AtomicBool>,
) -> DestructionObserved {
    let record = custody.cleanup_record();
    let (life, handle_in_slot) = match custody.worker_slot().lock() {
        Ok(slot) => (slot.life, slot.handle.is_some()),
        Err(poisoned) => {
            let slot = poisoned.into_inner();
            (slot.life, slot.handle.is_some())
        }
    };
    DestructionObserved {
        stop: stop.map(|stop| stop.load(std::sync::atomic::Ordering::Acquire)),
        admitted: custody.startup_admitted(),
        departure: custody.departure_observation(),
        decision: record.destruction_decision(),
        standing: record.destruction_standing(),
        number: registry.occupancy.state_of(client),
        row: registry
            .clients
            .lock()
            .map(|clients| clients.contains_key(&client))
            .unwrap_or(false),
        lease: record
            .lifecycle
            .lock()
            .map(|held| held.is_some())
            .unwrap_or(false),
        home: record.ordered_home.standing(),
        gate_fenced: custody
            .gate()
            .fenced
            .lock()
            .map(|fenced| *fenced)
            .unwrap_or_else(|poisoned| *poisoned.into_inner()),
        life,
        handle_in_slot,
    }
}

/// What a deferred destruction must have left untouched, whichever way it
/// deferred: the duty and the number with the custodian, the row, the lease,
/// the home and the gate exactly as they were.
fn assert_deferred_untouched(
    seen: &DestructionObserved,
    why: PrivateDestructionDeferral,
    what: &str,
) {
    assert_eq!(
        seen.decision,
        Some(PrivateDestructionDecision::Deferred(why)),
        "{what}: the record says destruction deferred, and why"
    );
    assert_eq!(seen.admitted, Some(false), "{what}: no further start is admitted");
    assert_eq!(
        seen.number,
        Some(PrivateNumberStanding::Held),
        "{what}: the number claim stays with the connection"
    );
    assert!(seen.row, "{what}: the row is not removed by number");
    assert!(seen.lease, "{what}: the lifecycle lease is not disposed of");
    assert_eq!(seen.home, PrivateHomeStanding::Live, "{what}: the home is not retained");
    assert!(!seen.gate_fenced, "{what}: the gate is not fenced");
}

/// A worker body that waits for its permit or its stop the way a registered
/// worker does, then stays alive until the control lets it go, and reports
/// whether its stop had been published by the time it was released.
fn held_worker(
    stop: Arc<AtomicBool>,
    wake: Arc<PrivateOrderedWake>,
    hold: std::sync::mpsc::Receiver<()>,
    saw: std::sync::mpsc::SyncSender<&'static str>,
) -> impl FnOnce() + Send + 'static {
    move || {
        let mut state = wake
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while !state.started && !stop.load(std::sync::atomic::Ordering::Acquire) {
            state = wake
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        drop(state);
        // HELD LIVE HERE until the control has taken its observations.
        let _ = hold.recv();
        let outcome = if stop.load(std::sync::atomic::Ordering::Acquire) {
            "stopped"
        } else {
            "released unstopped"
        };
        let _ = saw.send(outcome);
    }
}

fn waited_until(condition: impl Fn() -> bool, bound: Duration) -> bool {
    let deadline = std::time::Instant::now() + bound;
    while !condition() {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    true
}

/// Drop a registration on its own thread, bounded.
///
/// THE DISCRIMINATOR FOR "NO JOIN IN DROP". A drop that returns inside the
/// bound while the worker is still held never waited for that worker. One
/// that does not is not left hanging: the worker is let go so the drop can
/// finish, and the control reports which it was instead of abandoning a
/// thread behind a timeout.
fn dropped_without_waiting(
    registration: XServerFrontendClientRouteRegistration,
    let_go: impl FnOnce(),
) -> bool {
    std::thread::scope(|scope| {
        let dropper = scope.spawn(move || drop(registration));
        let finished = waited_until(|| dropper.is_finished(), Duration::from_secs(5));
        if !finished {
            let_go();
        }
        dropper.join().expect("the drop returns");
        finished
    })
}

/// A registration with a private source on a fresh owner, no worker.
struct PlainPrivateRegistration {
    durable: PrivateSettlementOwner,
    keeper: crate::PrivateServiceOwner,
    private: crate::PrivateXServerFrontend,
}

fn plain_private(clients: usize) -> PlainPrivateRegistration {
    let durable = PrivateSettlementOwner::default();
    let keeper = service_owner(&durable, clients);
    let private = private_over(&keeper, clients);
    PlainPrivateRegistration {
        durable,
        keeper,
        private,
    }
}

/// The registry a worker fixture's connection is registered with, reached
/// through the runner alone so the registration can be moved out beside it.
fn worker_registry(runner: &PrivatePreparedRunner) -> &XServerFrontendRouteRegistry {
    &runner
        .frontend
        .as_ref()
        .expect("a live runner")
        .broker
        .registry
}

#[test]
fn never_started_destruction_runs_the_synchronous_cleanup_and_records_it() {
    let p = plain_private(4);
    let client = XServerFrontendClientId(9201);
    let (registration, _channels) = p
        .private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a source and a row");
    let PrivateCustodyReach::Reached(custody) = registration
        .registered_custody(&p.keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    assert_eq!(custody.startup_admitted(), Some(true));
    assert_eq!(custody.cleanup_record().destruction_decision(), None);
    drop(registration);
    let seen = observe_destruction(&custody, &p.private.broker.registry, client, None);
    assert_eq!(
        seen.decision,
        Some(PrivateDestructionDecision::Synchronous),
        "nothing was started, so the synchronous body was requested here"
    );
    assert_eq!(seen.departure, Some(PrivateDeparture::NothingStarted));
    assert_eq!(seen.admitted, Some(false), "and no start is admitted afterwards");
    assert_eq!(seen.number, None, "the number went back: every effect was established");
    assert!(!seen.row, "the row is gone");
    assert_eq!(seen.home, PrivateHomeStanding::Retained, "the home was retained");
    assert!(seen.gate_fenced, "and the endpoint was fenced");
    drop(custody);
}

#[test]
fn a_context_obtained_before_destruction_cannot_start_afterwards_and_calls_no_spawner() {
    let f = worker_fixture(XServerFrontendClientId(9202));
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    drop(f.fixture.registration);
    let called = AtomicBool::new(false);
    let outcome = context.start(|| {
        called.store(true, std::sync::atomic::Ordering::Release);
        std::thread::Builder::new().spawn(|| {})
    });
    assert_eq!(outcome, PrivateStartupOutcome::NoLongerStartable);
    assert!(
        !called.load(std::sync::atomic::Ordering::Acquire),
        "destruction closed admission first, so the spawner was never reached"
    );
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, Some(&f.stop));
    assert_eq!(seen.decision, Some(PrivateDestructionDecision::Synchronous));
    assert_eq!(seen.departure, Some(PrivateDeparture::NothingStarted));
    assert_eq!(seen.life, PrivateWorkerLife::NeverStarted);
    assert_eq!(
        seen.stop,
        Some(false),
        "no worker was ever admitted, so no stop was published"
    );
    assert_eq!(seen.number, None);
    drop(custody);
}

#[test]
fn an_already_established_nothing_started_is_used_by_destruction() {
    let f = worker_fixture(XServerFrontendClientId(9203));
    let client = f.fixture.client;
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    assert_eq!(
        context.depart(),
        PrivateDeparted::Decided(PrivateDeparture::NothingStarted),
        "an earlier ask established it"
    );
    drop(f.fixture.registration);
    let seen = observe_destruction(&custody, worker_registry(&f.fixture.runner), client, Some(&f.stop));
    assert_eq!(
        seen.decision,
        Some(PrivateDestructionDecision::Synchronous),
        "AlreadyDecided(NothingStarted) is the same established fact"
    );
    assert_eq!(seen.number, None, "and the body ran under it");
    assert!(!seen.row);
    drop(custody);
}

#[test]
fn destroying_one_registration_leaves_a_same_store_sibling_and_its_queued_work_alone() {
    let f = worker_fixture(XServerFrontendClientId(9204));
    f.permit();
    let client = f.fixture.client;
    let sibling = XServerFrontendClientId(9205);
    let registry = worker_registry(&f.fixture.runner);
    let (sibling_registration, sibling_channels) = registry
        .register_client_with_admission(sibling, Some(admitted(sibling)))
        .expect("a sibling on the same store");
    registry
        .attach_private_lifecycle(&sibling_registration, admitted(sibling))
        .expect("the sibling's lifecycle attaches");
    let sibling_sender = capture_gated_sender(
        f.fixture.runner.frontend.as_ref().expect("live"),
        sibling,
    );
    // THE SIBLING'S EXACT WORK: a capsule with its own completion, whose
    // frames and completion identity are kept to compare against what is
    // still on the queue afterwards.
    let (queued, _endpoint, _recovery, _receipts) = answerable_capsule(92050);
    let queued_delivery = queued.delivery();
    let queued_frames = order_pass_frames(&queued);
    let queued_cell = Arc::clone(&queued.finalizer().expect("carried").completion);
    gated_send(&sibling_sender, queued).expect("the sibling's open endpoint");
    let PrivateCustodyReach::Reached(sibling_custody) = sibling_registration
        .registered_custody(&f.fixture.keeper.lease())
        .expect("the sibling's own custody")
    else {
        panic!("its owner keeps it")
    };
    let custody = custody_for(&f, &f.fixture.keeper);
    let context = custody.prepare_control().expect("its own owner");
    let (let_go, hold) = sync_channel::<()>(1);
    let (saw, seen_by_worker) = sync_channel(1);
    let body = held_worker(Arc::clone(&f.stop), Arc::clone(&f.wake), hold, saw);
    assert_eq!(
        context.start(|| std::thread::Builder::new().spawn(body)),
        PrivateStartupOutcome::Started
    );
    let unheld = dropped_without_waiting(f.fixture.registration, || {
        let _ = let_go.send(());
    });
    let ours = observe_destruction(&custody, registry, client, Some(&f.stop));
    // The sibling never bound a control association, so it has no stop of
    // its own to read; nothing is claimed about one.
    let theirs = observe_destruction(&sibling_custody, registry, sibling, None);
    let_go.send(()).expect("the worker is held");
    let worker_saw = seen_by_worker.recv_timeout(Duration::from_secs(5)).ok();
    let reaped = PrivateReapingRecord::bound_to(&custody).reap().reaped;
    assert_eq!(reaped, PrivateReaped::Joined);
    assert!(unheld, "the drop returned without waiting for the held worker");
    assert_eq!(worker_saw, Some("stopped"));
    assert_deferred_untouched(&ours, PrivateDestructionDeferral::WorkerRunning, "ours");
    assert_eq!(theirs.admitted, Some(true), "the sibling still admits a start");
    assert_eq!(theirs.departure, None, "no departure was asked of it");
    assert_eq!(
        theirs.standing,
        PrivateDestructionStanding::NotRequested,
        "and no destruction was requested against it"
    );
    assert_eq!(theirs.number, Some(PrivateNumberStanding::Held));
    assert!(theirs.row && theirs.lease && !theirs.gate_fenced);
    assert_eq!(theirs.home, PrivateHomeStanding::Live);
    let still_queued = sibling_channels
        .ordered
        .receiver
        .try_recv()
        .expect("the sibling's exact queued work is still there");
    assert_eq!(still_queued.delivery(), queued_delivery);
    assert_eq!(order_pass_frames(&still_queued), queued_frames, "the same frames");
    assert!(
        Arc::ptr_eq(
            &queued_cell,
            &still_queued.finalizer().expect("carried").completion
        ),
        "the same completion, by identity"
    );
    assert!(queued_cell.answer().is_none(), "and nothing answered for it");
    assert!(
        matches!(
            sibling_channels.ordered.receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ),
        "and nothing else was put on its queue"
    );
    drop((sibling_custody, sibling_registration, custody));
}

#[test]
fn a_missing_keeper_leaves_destruction_deferred_rather_than_never_started() {
    let p = plain_private(4);
    let client = XServerFrontendClientId(9206);
    let (registration, _channels) = p
        .private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a source and a row");
    // A test-side handle on the pre-reserved record, so the decision can be
    // read after the registration and the custody are both gone.
    let record = Arc::clone(&registration.cleanup);
    // STAGE-ONLY: THE OUTER OWNER DESTROYED BEFORE ITS CONNECTION. The
    // private service guarantees the keeper around every connection, and
    // this component does not open destroying it over one; the arrangement
    // exists only to show what the branch establishes -- which is nothing.
    let PlainPrivateRegistration {
        durable,
        keeper,
        private,
    } = p;
    drop(keeper);
    drop(registration);
    assert_eq!(
        record.destruction_decision(),
        Some(PrivateDestructionDecision::Deferred(
            PrivateDestructionDeferral::SourceUnreachable
        )),
        "a custody that cannot be reached establishes no absence of a worker"
    );
    assert_eq!(
        private.broker.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held),
        "so the number stays claimed"
    );
    assert!(
        private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable registry")
            .contains_key(&client),
        "and nothing was removed by number"
    );
    assert_eq!(
        record.ordered_home.standing(),
        PrivateHomeStanding::Live,
        "the home was not retained"
    );
    drop((record, private, durable));
}

#[test]
fn a_second_destruction_request_against_a_decided_record_is_inert() {
    let p = plain_private(4);
    let client = XServerFrontendClientId(9207);
    let (registration, _channels) = p
        .private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a place, a keeper, a source and a row");
    let PrivateCustodyReach::Reached(custody) = registration
        .registered_custody(&p.keeper.lease())
        .expect("its own custody")
    else {
        panic!("its owner keeps it")
    };
    drop(registration);
    let record = Arc::clone(custody.cleanup_record());
    assert_eq!(
        record.destruction_decision(),
        Some(PrivateDestructionDecision::Synchronous)
    );
    // STAGE-ONLY: a later request through the record's own entry point, as
    // the executor a later boundary attaches would make it. The claim is
    // refused, a decision published over it is refused, and the decision
    // that stands is the first one.
    assert!(!record.claim_destruction(), "a second request is refused");
    assert!(!record.publish_destruction(PrivateDestructionDecision::Deferred(
        PrivateDestructionDeferral::WorkerRunning
    )));
    assert_eq!(
        record.destruction_standing(),
        PrivateDestructionStanding::Decided(PrivateDestructionDecision::Synchronous),
        "and the first decision stands"
    );
    assert_eq!(p.private.broker.registry.occupancy.state_of(client), None);
    drop((record, custody));
}
