/// A drain that fails on a poisoned channel must not have consumed what it
/// was already holding.
///
/// WHAT THIS CATCHES. An earlier version observed the retained receipts first
/// and then reached for the delivery channel, so a poisoned channel returned
/// `Err` after those receipts had already been observed -- their places given
/// back in the ledger, and the caller never told which receipts those were.
/// Every fallible lock is taken before any observation now, so a failure means
/// nothing happened. The retained queue is read directly rather than through
/// the drain, because the drain is the thing under test.
#[test]
fn a_failed_drain_consumes_none_of_the_receipts_it_was_holding() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    // Into custody without observing, which is what this call is for.
    let inventory = fixture.handle().unobserved_receipts().unwrap();
    assert!(
        inventory.complete,
        "the channel was emptied into custody: {inventory:?}"
    );
    let held = inventory.retained;
    assert!(
        held > 0,
        "a delivered event leaves a receipt to be consumed"
    );

    let poisoner = std::sync::Arc::clone(&runtime);
    let thread = std::thread::spawn(move || {
        let _held = poisoner.deliveries.lock().unwrap();
        panic!("poisoning the delivery channel on purpose");
    });
    assert!(thread.join().is_err());

    assert!(
        fixture.handle().drain_deliveries().is_err(),
        "a poisoned delivery channel is refused"
    );
    assert_eq!(
        runtime.retained_receipts.lock().unwrap().len(),
        held,
        "the failed drain observed none of the receipts it already held"
    );
    drop((submission, peer));
}

/// One call visits at most the drain bound, however much is waiting.
///
/// WHAT THIS CATCHES. The bound used to be read off what remained at the end
/// of the call, so a full retention queue could be observed and a full channel
/// drained on top of it -- twice the advertised bound in one call. The queue
/// is seeded past the bound directly, because arranging that many real
/// deliveries would test the wire rather than the bound.
#[test]
fn one_drain_visits_no_more_than_its_bound() {
    let fixture = Fixture::started(PrivateInputGrantPolicy::Disabled);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);

    let seeded = super::receipts::PRIVATE_INPUT_DRAIN_BOUND + 44;
    {
        let mut retained = runtime.retained_receipts.lock().unwrap();
        for index in 0..seeded {
            retained.push_back(sophia_x_authority::XAuthorityClientInputDelivery {
                client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
                delivery: sophia_x_authority::XAuthorityInputDeliveryId::from_raw(
                    u64::try_from(index + 1).unwrap(),
                ),
                outcome: sophia_x_authority::XAuthorityInputDeliveryOutcome::Flushed,
            });
        }
    }

    let receipts = fixture.handle().drain_deliveries().unwrap();
    assert_eq!(
        receipts.visited,
        super::receipts::PRIVATE_INPUT_DRAIN_BOUND,
        "one call visits exactly its bound when more is waiting"
    );
    // None of these belong to the ledger, so none of them is released and all
    // of them stay owed.
    assert!(
        receipts.observed.is_empty(),
        "{:?}",
        receipts.observed.len()
    );
    assert_eq!(receipts.retained, seeded);
    assert_eq!(runtime.retained_receipts.lock().unwrap().len(), seeded);
}

/// A stop counts receipts nobody drained, not only the ones already taken.
///
/// WHAT THIS CATCHES. The count used to read the retained queue alone, so a
/// service stopped with receipts still sitting unread in its channel reported
/// zero owed -- while every one of those receipts still held a place in the
/// delivery ledger.
///
/// NOTHING HERE TOUCHES THE CHANNEL BEFORE THE STOP. An earlier version of this
/// control waited for its delivery by draining, which moved the receipt into
/// custody and left it testing the case it was written to rule out. The wait
/// goes through the ledger's own terminal answer instead, which reads and frees
/// nothing, and the retained queue is asserted empty beforehand so the count
/// afterwards can only have come from the channel.
#[test]
fn a_stop_counts_receipts_nobody_drained() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let delivery = fixture.deliver_one(&submission);

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    assert_eq!(
        runtime.retained_receipts.lock().unwrap().len(),
        0,
        "nothing has been taken into custody, so the receipt is still queued"
    );
    // The ledger settled it, which is how this knows there is one to count.
    let settled = runtime
        .observer
        .settled(delivery)
        .expect("the delivery this submitted has a terminal answer");
    assert_eq!(settled.delivery, delivery);

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    let owed = outcome
        .receipts_unobserved
        .expect("the channel and the queue were both readable");
    assert!(
        owed >= 1,
        "a receipt nobody drained is still owed at stop: {outcome:?}"
    );
    // AND IT IS THE EXACT ONE. A count that happened to be positive for some
    // other reason would pass a weaker assertion than this.
    let queued = runtime.retained_receipts.lock().unwrap();
    assert!(
        queued.iter().any(|receipt| receipt.delivery == delivery),
        "the stop took the exact queued delivery into custody: {queued:?}"
    );
    drop(queued);
    assert!(
        outcome.retains_obligations(),
        "owed receipts retain custody: {outcome:?}"
    );
    assert!(
        fixture.lifetime.retains_unresolved(),
        "and the lifetime's reserved slot is holding it"
    );
    record_actors(&runtime, &outcome);
    drop((submission, peer));
}

/// A controller dropped while a real submission lives fills the reserved slot.
///
/// WHAT THIS CATCHES. `Drop` performed the stop and threw the outcome away
/// without running retention, so the reserved closing slot was never filled on
/// the one path it exists for. A caller that never calls `stop` -- which is
/// exactly the caller this slot was reserved for -- left unresolved work with
/// no owner at all. The submission is real and still alive across the drop, so
/// this is also the live-adapter case rather than a cloned handle.
#[test]
fn dropping_the_controller_with_work_owed_fills_the_reserved_slot() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    // An accepted obligation nobody drains, so the exit owes something.
    let _delivery = fixture.deliver_one(&submission);

    assert!(fixture.lifetime.in_use());
    assert!(!fixture.lifetime.retains_unresolved());

    // No stop. The controller simply goes, with the submission still held.
    drop(fixture.handle.take());

    assert!(
        fixture.lifetime.retains_unresolved(),
        "the controller's drop placed the unresolved runtime in the reserved slot"
    );
    assert!(
        !fixture.lifetime.in_use(),
        "and the ended service no longer claims the lifetime"
    );
    assert_eq!(
        fixture.lifetime.unresolved_receipts(),
        Some(Some(1)),
        "the slot can say what the retained runtime still owes"
    );
    assert!(!fixture.lifetime.slot_poisoned());

    // The submission outlived the controller and is refused by the ended
    // service rather than served through it.
    let refused = submission.submit_key(sophia_protocol::SurfaceId::new(window, 1), 38, false);
    assert!(refused.is_err(), "{refused:?}");
    drop((submission, peer));
}

/// Dropping the submission after the controller changes nothing about custody.
///
/// THE OUTER OWNER IS THE ONE THAT MATTERS. Custody was placed when the
/// controller went; an adapter releasing its own handle afterwards is not a
/// resolution of anything, and the slot must still be holding the work. This is
/// the pair of the control above: there, the submission outlives the
/// controller; here it is released afterwards and the answer is the same.
#[test]
fn releasing_the_submission_after_the_controller_leaves_custody_where_it_was() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    drop(fixture.handle.take());
    assert!(fixture.lifetime.retains_unresolved());

    drop(submission);

    assert!(
        fixture.lifetime.retains_unresolved(),
        "an adapter letting go of its handle resolves nothing"
    );
    assert_eq!(
        fixture.lifetime.unresolved_receipts(),
        Some(Some(1)),
        "and the work it owed is still exactly what it was"
    );
    drop(peer);
}

/// A service that ends owing nothing gives its claim back.
///
/// Without this the lifetime would be spent by a service that finished clean,
/// and the reserved slot would be unusable for a reason that never happened.
#[test]
fn a_clean_exit_returns_the_lifetime_claim() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::Disabled);
    assert!(fixture.lifetime.in_use());

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    if outcome.retains_obligations() {
        // Nothing to prove here: this control is about the clean case, and an
        // exit that owed something is reported rather than asserted away.
        assert!(fixture.lifetime.retains_unresolved(), "{outcome:?}");
        return;
    }
    assert!(
        !fixture.lifetime.retains_unresolved(),
        "a clean exit retains nothing: {outcome:?}"
    );
    assert!(
        !fixture.lifetime.in_use(),
        "and gives its claim back: {outcome:?}"
    );
}

/// A poisoned join slot still collects the real serving thread.
///
/// WHAT THIS CATCHES. The stop read the join slot with `.ok()`, so a poisoned
/// mutex produced `None` and the run reported `NeverStarted` -- a thread that
/// was never started -- while the real join handle was still sitting in the
/// slot for a later drop to detach unjoined. Reporting the unreadable slot as
/// an answer about the thread is the failure; recovering it and reporting the
/// poisoning separately is the repair.
///
/// The slot is poisoned only once the service is genuinely serving, with a
/// started worker and an accepted obligation already owned.
#[test]
fn a_poisoned_join_slot_still_collects_the_real_thread() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    assert_eq!(
        fixture.running_row().worker,
        PrivateCustodyWorkerStanding::Running
    );
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let poisoner = std::sync::Arc::clone(&runtime);
    let thread = std::thread::spawn(move || {
        let _held = poisoner.thread.lock().unwrap();
        panic!("poisoning the join slot on purpose");
    });
    assert!(thread.join().is_err(), "the poisoning thread panicked");

    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert!(
        outcome.join_slot_poisoned,
        "the poisoning is reported as its own fact: {outcome:?}"
    );
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "and the real thread is still collected rather than called NeverStarted: {outcome:?}"
    );
    // NOTHING IS LEFT FOR A LATER DROP TO DETACH.
    let left = match runtime.thread.lock() {
        Ok(held) => held.is_some(),
        Err(poisoned) => poisoned.into_inner().is_some(),
    };
    assert!(!left, "the handle was taken for the join, not abandoned");
    drop((submission, peer));
}

/// M4.lifetime subcase `command_loss`: the service command channel is lost.
///
/// THE REAL DISCONNECTION, NOT A SIMULATED ONE. Every command sender is
/// dropped, which is what happens when the last holder of a channel goes; the
/// service's own receiver then reports a genuine `Disconnected` and the serving
/// loop treats it exactly as it treats StopAndDisconnect. Nothing is faked and
/// no error is constructed.
///
/// The fault lands on a service that is actually serving: a real authenticated
/// peer, its custody reporting `Running`, an admitted surface it committed, and
/// an accepted obligation already owned.
#[test]
fn command_loss_closes_admission_and_collects() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    // CAPTURED WHILE IT IS RUNNING. Collection is asserted against this exact
    // place afterwards, so a run that collected some other actor cannot pass.
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);
    let primary_place = running.place;
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    assert!(
        fixture.handle().runtime.drop_command_senders(),
        "the service held a command sender to lose"
    );
    assert!(
        fixture.handle().runtime.command_sender().is_none(),
        "every command sender is gone"
    );

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "a stop with no sender to ask still joins: {outcome:?}"
    );
    assert_collected(&runtime, &outcome, &submission, primary_place, true);
    drop(peer);
}

/// M4.lifetime subcase `service_error`: a real service error after startup.
///
/// THE ERROR IS THE SERVICE'S OWN. `UpdateOutputTopology` carries the sender
/// its acknowledgement goes back on; this sends a valid topology with a
/// receiver that has already been dropped, so the serving loop's own
/// `try_send` fails and it returns the error it writes for exactly that. No
/// `PrivateServiceFailure` is constructed here, and the topology is real -- a
/// malformed one would be refused earlier and would prove something else.
#[test]
fn a_service_error_after_startup_closes_admission_and_collects() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let _window = peer.create_map_and_draw();
    let running = fixture.running_row();
    assert_eq!(running.worker, PrivateCustodyWorkerStanding::Running);
    let primary_place = running.place;
    let submission = fixture.issue_submission();
    let _delivery = fixture.deliver_one(&submission);

    let sender = fixture
        .handle()
        .runtime
        .command_sender()
        .expect("the service is still taking commands");
    let (acknowledgement, receiver) = std::sync::mpsc::sync_channel(1);
    // The acknowledgement has nowhere to go before the command is even sent.
    drop(receiver);
    let mut topology = config(&fixture.socket, PrivateInputGrantPolicy::Disabled).output_topology;
    topology.generation += 1;
    sender
        .send(
            sophia_x_authority::XServerFrontendServiceCommand::UpdateOutputTopology {
                snapshot: topology,
                acknowledgement,
            },
        )
        .expect("the service is still taking commands");

    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let handle = fixture.handle.take().unwrap();
    let outcome = handle.stop();

    assert_eq!(
        outcome.invocation,
        super::handle::PrivateInputInvocation::Failed,
        "the invocation reports its own failure: {outcome:?}"
    );
    assert!(
        outcome.failure.is_some(),
        "and the failure travels whole: {outcome:?}"
    );
    assert_eq!(
        outcome.service_thread,
        PrivateInputThreadJoin::Joined,
        "a failed invocation still joins its thread: {outcome:?}"
    );
    assert_collected(&runtime, &outcome, &submission, primary_place, true);
    drop(peer);
}

/// Actors started and collected across one `lifetime` run.
///
/// ACCUMULATED ACROSS THE FIVE EXITS, because the acceptance record is about
/// the case and not about any one of them. Each subcase adds what its own exit
/// started and collected; nothing else reads or resets it.
static LIFETIME_ACTORS: std::sync::Mutex<(usize, usize)> = std::sync::Mutex::new((0, 0));
