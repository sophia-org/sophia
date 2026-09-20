// Controls for the service frame's idle-window reclaim (t138): the
// non-blocking reap it needs, the decision it must not run ahead of, and the
// place and custody a departed connection gives back during the run. Split
// from private_deferred_cleanup.rs when that file passed its ceiling; the
// fixtures are its.
// t138. The live idle-window reclaim and the non-blocking reap it needs.
// These run the real reaping record and the real discharge over a worker
// fixture, the same way the controls above do.

#[test]
fn a_running_worker_is_not_reaped_and_its_record_may_be_asked_again() {
    // The whole point of the non-blocking ask. `reap` would wait here, and
    // the service frame that drives this cannot: it is the thread that
    // accepts connections and serves the order, and a blocked ordered
    // delivery is allowed six seconds.
    let f = worker_fixture(XServerFrontendClientId(9520));
    let custody = custody_for(&f, &f.fixture.keeper);
    start_worker(&f, &custody, None);

    let reaping = PrivateReapingRecord::bound_to(&custody).reap_finished();
    assert_eq!(
        reaping.reaped,
        PrivateReaped::StillRunning,
        "a worker parked on its queue is not finished"
    );
    assert_eq!(reaping.exit, None, "nothing was read from the exit record");
    // NOTHING WAS CONSUMED, which is the property the whole design rests on:
    // there is no way to put a handle back, so an ask that would have had to
    // wait must leave the slot exactly as it found it.
    let slot = custody.worker_slot();
    let held = slot.lock().expect("a readable slot");
    assert!(held.handle.is_some(), "the handle is still in the slot");
    assert_eq!(
        held.life,
        PrivateWorkerLife::Running,
        "and its life was not advanced to HandedToJoiner"
    );
    drop(held);

    // So the record is askable again, and the blocking ask still works.
    let lease = f.fixture.keeper.lease();
    let registry = worker_registry(&f.fixture.runner);
    drop(f.fixture.registration);
    let workers = stop_and_collect(&lease, registry, &custody);
    assert!(
        workers.iter().all(|worker| worker.joined),
        "the ordinary collection still joins it: {workers:?}"
    );
    drop(custody);
}

#[test]
fn a_finished_worker_is_reaped_by_the_non_blocking_ask() {
    let f = worker_fixture(XServerFrontendClientId(9521));
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    start_worker(&f, &custody, None);
    let lease = f.fixture.keeper.lease();
    drop(f.fixture.registration);
    let failures = stop_attached_workers(&lease, registry);
    assert!(failures.is_empty(), "{failures:?}");

    // ASKED REPEATEDLY RATHER THAN WAITED FOR, because that is what a caller
    // who may not block does. A stopped thread is not an instantly finished
    // one, and each refusal consumes nothing, so asking again is the
    // sanctioned way to get there.
    let record = PrivateReapingRecord::bound_to(&custody);
    let mut reaped = PrivateReaped::StillRunning;
    for _ in 0..2000 {
        reaped = record.reap_finished().reaped;
        if reaped != PrivateReaped::StillRunning {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(reaped, PrivateReaped::Joined, "the finished worker was joined");
    assert!(
        record.result().is_some(),
        "and its result was published, which is what says it was collected"
    );
    drop(custody);
}

#[test]
fn the_idle_window_reclaims_a_departed_connection_and_a_busy_one_does_not() {
    let f = worker_fixture(XServerFrontendClientId(9522));
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let frontend = f.fixture.runner.frontend.as_ref().expect("a live runner");
    start_worker(&f, &custody, None);
    let lease = f.fixture.keeper.lease();
    drop(f.fixture.registration);
    let failures = stop_attached_workers(&lease, registry);
    assert!(failures.is_empty(), "{failures:?}");

    // A FRAME STILL ACTIVE IS NOT A WINDOW. The count is the whole guard:
    // the token this would mint says no connection frame is active, and with
    // one active that would be a lie. Refusing on the number is what keeps
    // the ordinary token honest rather than widening it.
    assert_eq!(
        reclaim_idle_departures(frontend, &lease, 1),
        0,
        "nothing is reclaimed while a connection frame is active"
    );
    assert_eq!(
        cleanup_seen(&custody, registry, &f.fixture.durable).standing,
        PrivateDeferredCleanupStanding::NotVisited,
        "and the cleanup was not even visited"
    );

    // Then the window. Asked repeatedly for the same reason as above: the
    // discharge refuses JoinUnpublished until the reap lands, and neither
    // step waits.
    let mut progressed = 0;
    for _ in 0..2000 {
        progressed += reclaim_idle_departures(frontend, &lease, 0);
        if matches!(
            cleanup_seen(&custody, registry, &f.fixture.durable).standing,
            PrivateDeferredCleanupStanding::Done(_)
        ) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(progressed > 0, "the window reclaimed something");
    let seen = cleanup_seen(&custody, registry, &f.fixture.durable);
    assert!(
        matches!(seen.standing, PrivateDeferredCleanupStanding::Done(_)),
        "the deferred cleanup discharged during the run, not at shutdown: {seen:?}"
    );
    assert_eq!(
        seen.home,
        PrivateHomeStanding::Retained,
        "and the home its connection left was finally told the connection had gone"
    );
    drop(custody);
}

#[test]
fn the_idle_window_does_not_reap_a_departure_that_has_not_decided() {
    // The M3 start-failures control found this one run in ten. A worker
    // whose permit is refused finishes at once, and its connection may still
    // be deciding its departure -- destruction Requested, not yet Decided.
    // The decision reads the slot's life to say what it found, so a reap in
    // that gap made it record WorkerHandedOn where the truth was
    // WorkerRunning: the reclaim was changing what the departure said about
    // itself. So the reap waits for the decision, exactly as the discharge
    // does, and takes only what the discharge could take next.
    let f = worker_fixture(XServerFrontendClientId(9530));
    let custody = custody_for(&f, &f.fixture.keeper);
    let frontend = f.fixture.runner.frontend.as_ref().expect("a live runner");
    // A body that returns at once, so the thread is finished by the time the
    // window looks -- the shape of a refused permit, without the fault.
    start_worker(&f, &custody, Some("finished at once"));
    let lease = f.fixture.keeper.lease();
    let record = custody.cleanup_record();
    assert!(record.claim_destruction(), "the departure is requested");
    assert_eq!(
        record.destruction_standing(),
        PrivateDestructionStanding::Requested,
        "and not yet decided"
    );
    let _ = PrivateReapingRecord::bound_to(&custody);
    // Let the thread actually end before asking, so the only thing standing
    // between the window and the handle is the decision.
    std::thread::sleep(std::time::Duration::from_millis(20));

    let progressed = reclaim_idle_departures(frontend, &lease, 0);
    assert_eq!(progressed, 0, "nothing is reaped ahead of the decision");
    assert!(
        custody.worker_slot().lock().expect("a readable slot").handle.is_some(),
        "the handle is still in the slot for the decision to read"
    );
    assert_eq!(
        custody.join().phase(),
        PrivateReapingPhase::NotBegun,
        "and no join was begun"
    );

    // ONCE DECIDED, IT IS THE WINDOW'S TO TAKE. The same arm the discharge
    // requires is the one that admits the reap.
    assert!(record.publish_destruction(PrivateDestructionDecision::Deferred(
        PrivateDestructionDeferral::WorkerRunning
    )));
    let mut reaped = false;
    for _ in 0..2000 {
        if reclaim_idle_departures(frontend, &lease, 0) > 0 {
            reaped = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(reaped, "a decided deferral is reaped in the window");
    assert!(
        custody.join().result().is_some(),
        "and its join is published for the discharge"
    );
    drop(custody);
}

#[test]
fn the_idle_window_retires_no_custody_while_busy_and_none_whose_continuation_has_not_settled() {
    // t138's last layer, and the two ways it must refuse. The custody is the
    // last thing a departed connection holds; it is retired in the idle
    // window, last, from the same per-custody proof the invocation-end visit
    // uses. This control pins what stops it: a frame still active, and a
    // continuation whose wire never ended.
    //
    // THE POSITIVE HALF IS NOT HERE, ON PURPOSE. This fixture stops the
    // worker but never ends the wire, so the continuation never settles, the
    // place is never returned, and `storage_returned` stays false -- exactly
    // the state in which retiring the custody would be wrong. Retiring it
    // needs the real path: a client that drops its socket. That witness is
    // the M5 `departures_reclaimed` group, ten rounds on a real private
    // instance, every admission answered.
    let f = worker_fixture(XServerFrontendClientId(9540));
    let custody = custody_for(&f, &f.fixture.keeper);
    let registry = worker_registry(&f.fixture.runner);
    let frontend = f.fixture.runner.frontend.as_ref().expect("a live runner");
    start_worker(&f, &custody, None);
    let lease = f.fixture.keeper.lease();
    drop(f.fixture.registration);
    let failures = stop_attached_workers(&lease, registry);
    assert!(failures.is_empty(), "{failures:?}");
    let held = |lease: &PrivateServiceLease<'_>| {
        lease
            .custodies_of(registry)
            .iter()
            .any(|pin| Arc::ptr_eq(&pin.custody, &custody.custody))
    };
    assert!(held(&lease), "the departed connection's custody is in its place");

    // A FRAME STILL ACTIVE IS NOT A WINDOW, for the custody exactly as for
    // the place: nothing of it moves.
    for _ in 0..50 {
        assert_eq!(reclaim_idle_departures(frontend, &lease, 1), 0);
    }
    assert!(held(&lease), "nothing was retired while a frame was active");

    // THE WINDOW DISCHARGES AND STOPS THERE. The join lands and the deferred
    // cleanup reaches Done -- so it is not an earlier step that holds the
    // custody back -- and the custody stays, because the work in its place
    // is not gone: the wire never ended, so the continuation is not settled
    // and the place was never returned.
    let mut discharged = false;
    for _ in 0..2000 {
        reclaim_idle_departures(frontend, &lease, 0);
        if matches!(
            cleanup_seen(&custody, registry, &f.fixture.durable).standing,
            PrivateDeferredCleanupStanding::Done(_)
        ) {
            discharged = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(discharged, "the deferred cleanup discharged in the window");
    for _ in 0..200 {
        reclaim_idle_departures(frontend, &lease, 0);
    }
    assert!(
        held(&lease),
        "a custody whose continuation has not settled is not retired, however \
         many idle turns pass"
    );
    assert!(
        !custody
            .cleanup_record()
            .ordered_home
            .storage_returned
            .load(std::sync::atomic::Ordering::Acquire),
        "and the place it waits on was indeed never returned"
    );
    drop(custody);
}
