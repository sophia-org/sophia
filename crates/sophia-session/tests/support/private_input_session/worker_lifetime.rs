/// A diagnostic control, NOT the acceptance row.
///
/// `lifetime` is reserved for the full five-subcase body -- stop, command
/// loss, service error, serving-thread unwind and retained work -- and this
/// name exists so that this control cannot be mistaken for it or bound in its
/// place. A real service, a real connected peer, custody read while it runs and
/// again after it is collected, and an execution reported from after the join
/// rather than from before it.
///
/// EVERY ASSERTION HERE IS ABOUT SOMETHING THAT ACTUALLY HAPPENED. The peer is
/// a real X client over the real socket, so the worker whose custody this reads
/// is a worker that was actually started for it. Custody after collection is
/// read from the outcome's own retention rather than from a service that is
/// still running, which is the only point at which "collected" means anything.
#[test]
fn running_worker_is_collected() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();

    // A STARTED WORKER, WAITED FOR RATHER THAN ASSUMED. Registered attachment
    // is production; a place reporting NeverStarted is the state before the
    // thing under test has happened, not evidence about it.
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);
    assert_eq!(
        running.handle_present,
        Some(true),
        "a running worker's handle is in its slot: {running:?}"
    );
    assert_eq!(
        running.departing,
        Some(false),
        "nothing has told this connection to depart: {running:?}"
    );
    assert_eq!(
        running.join,
        PrivateCustodyJoinStanding::Unpublished,
        "no join can have been published while the worker runs: {running:?}"
    );
    assert!(
        running.publication_right_unclaimed,
        "no attempt has claimed the right to publish a join: {running:?}"
    );

    let whole = fixture.custody();
    assert!(!whole.inventory_poisoned, "{whole:?}");
    assert!(whole.taken >= 1, "{whole:?}");
    assert_eq!(
        whole.places, 4,
        "the inventory is sized to the configured client bound: {whole:?}"
    );

    // THE LIFETIME IS RUNNING A SERVICE AND HOLDS NOTHING YET. A service that
    // has not ended has placed nothing, which is not a statement about what it
    // will owe.
    assert!(fixture.lifetime.in_use(), "a service is running under it");
    assert!(!fixture.lifetime.retains_unresolved());
    assert!(!fixture.lifetime.slot_poisoned());

    // COLLECTED, then custody read afterwards through a runtime kept for the
    // purpose rather than through a service that is still running.
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "the service thread is joined by the stop: {outcome:?}"
    );

    // A JOINED THREAD IS NOT A LIVE EXECUTION. The reading reported comes from
    // the durable witness after the join, so it cannot claim an execution
    // belonging to a thread that has already gone.
    if let Some(execution) = outcome.execution {
        assert_ne!(
            execution.availability,
            sophia_x_authority::PrivateExecutionAvailability::Retained,
            "a joined thread must not report a retained execution: {outcome:?}"
        );
    }

    // EXACT RETAINED WORK, COUNTING EVERY SOURCE OF IT. An earlier version
    // compared retention against the bridge count and the settlement
    // readability alone, so a run that ended with uncommitted intake or an
    // undrained receipt -- both ordinary, both retention -- read as a
    // contradiction. Which of them is non-zero depends on timing, which is why
    // it only failed once the controls ran concurrently.
    let owed = |count: Option<usize>| count.is_none_or(|owed| owed > 0);
    let something_owed = !outcome.settlement.readable
        || outcome.settlement.reserved_credits.is_some_and(|n| n > 0)
        || outcome.settlement.owed.is_some_and(|n| n > 0)
        || outcome.settlement.indeterminate.is_some_and(|n| n > 0)
        || owed(outcome.bridge_undelivered)
        || owed(outcome.receipts_unobserved)
        || owed(outcome.intake_uncommitted);
    assert_eq!(
        outcome.retains_obligations(),
        something_owed,
        "custody is retained exactly when something is still owed: {outcome:?}"
    );

    // AFTER-JOIN CUSTODY. The worker was collected, so its place says so: the
    // handle has gone to whoever joined it and the result is published.
    let after = runtime
        .owner
        .custody_snapshot(&runtime.owner.lease())
        .expect("the owner still keeps its inventory after the stop");
    assert!(!after.inventory_poisoned, "{after:?}");
    let collected = after
        .rows
        .iter()
        .find(|row| row.join != PrivateCustodyJoinStanding::Unpublished);
    assert!(
        collected.is_some(),
        "collection publishes a join result into the custody place: {after:?}"
    );
    let collected = collected.unwrap();
    assert_ne!(
        collected.worker,
        PrivateCustodyWorkerStanding::Unreadable,
        "collection leaves a readable place: {collected:?}"
    );
    assert_ne!(
        collected.worker,
        PrivateCustodyWorkerStanding::NeverStarted,
        "a place that published a join started something: {collected:?}"
    );
    assert!(
        !collected.publication_right_unclaimed,
        "the attempt that published took the right: {collected:?}"
    );

    // AND THE LIFETIME AGREES WITH THE OUTCOME. Custody is in the slot exactly
    // when the stop said something was still owed, and the slot is not left
    // claimed by a service that finished clean.
    assert_eq!(
        fixture.lifetime.retains_unresolved(),
        outcome.retains_obligations(),
        "the slot holds custody exactly when the stop reported work owed: {outcome:?}"
    );
    assert!(
        !fixture.lifetime.in_use(),
        "the ended service no longer claims the lifetime"
    );
    assert!(!fixture.lifetime.slot_poisoned());

    record_actors(&runtime, &outcome);
    drop(peer);
    drop(outcome);
    drop(runtime);
}

/// A handle that goes out of scope while an adapter still holds the runtime
/// still stops and joins.
///
/// WHAT THIS CATCHES. Drop used to stop only when it held the last `Arc`, so a
/// live submission -- the one thing this design deliberately hands to adapters
/// -- turned a drop into no stop at all, and the runtime then dropped its
/// `JoinHandle` without joining. The submission is taken while the service is
/// genuinely running and is still alive when the controller goes.
#[test]
fn dropping_the_controller_stops_even_while_a_submission_is_held() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);

    // A REAL SUBMISSION, HELD ACROSS THE DROP. This is the custody the design
    // hands to adapters, and holding it is what used to make the controller's
    // drop skip its stop and leave the JoinHandle dropped unjoined.
    let submission = fixture.issue_submission();
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    drop(fixture.handle.take());

    assert!(
        runtime.stop_once().is_none(),
        "the controller's drop performed the one stop even though a submission was live"
    );
    assert!(
        runtime
            .thread
            .lock()
            .map(|held| held.is_none())
            .unwrap_or(false),
        "the service thread was joined rather than dropped unjoined"
    );
    // Still held, which is the whole point of the arrangement.
    drop(submission);
    drop(peer);
    drop(runtime);
}

/// An explicit stop followed by the controller's drop joins exactly once.
#[test]
fn stopping_twice_joins_once() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::Disabled);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();
    assert_eq!(outcome.service_thread, PrivateInputThreadJoin::Joined);
    assert!(
        runtime.stop_once().is_none(),
        "the explicit stop is the only stop"
    );
}
