// Controls for judging received work against a boundary read AFTER receipt.
//
// THE DEFECT. `apply_committed` used to read the admitted connections once,
// before its bounded drain waited for new batches. A client admitted while
// that drain waited sent its create/map/draw, those batches were received in
// the same call, and they were judged against a reading that predated the
// client's admission: rejected as stale and consumed, so the surface was never
// admitted. Admission happens at setup, before any request is served, so a
// reading taken after receipt always covers the client whose batch arrived.

/// Wait, bounded by `WAIT`, until `ready` holds.
fn post_receipt_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + WAIT;
    while !ready() {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::yield_now();
    }
}

fn stale_outcomes(report: &crate::private_input::PrivateInputCommitted) -> usize {
    report
        .outcomes
        .iter()
        .filter(|seen| seen.outcome == sophia_protocol::TransactionOutcome::RejectedStaleSurface)
        .count()
}

/// RED BEFORE THE REPAIR, GREEN AFTER, WITH NO TIMING LUCK.
///
/// THE BARRIER. The controller reads its pre-drain boundary before it takes
/// the bridge, and holds the bridge through the bounded drain. So once the
/// bridge is seen held, that reading has already been taken, and a client
/// connected now is admitted after it. Holding the receiver mutex until the
/// draw's ordered reply arrives ensures this drain takes surface transactions,
/// not just an earlier empty setup envelope. Nothing
/// else in a running service holds the bridge (the only other reader is the
/// stop-time count), so a held bridge is the controller's.
#[test]
fn a_client_admitted_while_the_drain_waits_is_judged_after_receipt() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    // Keep receive from returning an empty setup envelope before the draw
    // arrives. The channel has 64 slots; this client's finite setup/draw fits.
    let receiver = runtime.transactions.lock().expect("receive barrier");
    let mut handle = fixture.handle.take().expect("a started handle");
    let controller = std::thread::spawn(move || {
        let report = handle.apply_committed(WAIT);
        (handle, report)
    });
    post_receipt_until("the controller to hold the bridge in its drain", || {
        runtime.bridge.try_lock().is_err()
    });

    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    peer.confirm_geometry(window);
    drop(receiver);

    let (handle, report) = controller.join().expect("the controller returned");
    fixture.handle = Some(handle);
    let mut report = report.expect("the bridge is readable");
    assert!(
        report.batches_observed >= 2,
        "the waiting call received the new client's work: {report:?}"
    );
    assert!(
        !report.outcomes.is_empty(),
        "this call must judge the new client's surface transactions"
    );
    assert_eq!(
        stale_outcomes(&report),
        0,
        "work received during the drain was judged against a reading taken after \
         receipt, not rejected as stale: {:?}",
        report.outcomes
    );
    let applied_now = report
        .outcomes
        .iter()
        .filter(|seen| seen.outcome == sophia_protocol::TransactionOutcome::Committed)
        .flat_map(|seen| seen.applied.iter().copied())
        .collect::<Vec<_>>();
    assert!(!applied_now.is_empty(), "the waited call must apply the new surface");
    fixture.harvest.append(&mut report.effects);
    let surface = fixture.admitted_surface();
    assert!(
        applied_now.contains(&surface),
        "admission must name a surface applied in the waited call"
    );
    drop(peer);
}

/// A failed post-receipt reading drops nothing, and the next call stages that
/// work exactly once and in the order it was received.
///
/// CUSTODY BEFORE THE FALLIBLE READING. The batches are already in intake when
/// the reading fails, so the failure leaves them there; the injected failure is
/// thread-local and clears itself, so the retry is not failed by it.
#[test]
fn a_failed_post_receipt_reading_keeps_received_work_for_exactly_one_retry() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let runtime = std::sync::Arc::clone(&fixture.handle().runtime);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    // A reply orders the create/map/draw before it.
    peer.confirm_geometry(window);

    super::committed::FAIL_NEXT_POST_RECEIPT_READING.with(|fail| fail.set(true));
    // The drain waits for at least one batch, so this call receives work.
    assert!(
        fixture.handle_mut().apply_committed(WAIT).is_err(),
        "the injected post-receipt reading failure is reported"
    );
    assert!(
        !super::committed::FAIL_NEXT_POST_RECEIPT_READING.with(std::cell::Cell::get),
        "the injection was consumed by exactly one reading"
    );
    let held = runtime
        .bridge
        .lock()
        .expect("the bridge is readable")
        .intake_transactions();
    assert!(!held.is_empty(), "the received work stayed in intake");

    let mut retry = fixture
        .handle_mut()
        .apply_committed(std::time::Duration::from_millis(5))
        .expect("the retry is not failed by the spent injection");
    let committed: Vec<_> = retry.outcomes.iter().map(|seen| seen.transaction).collect();
    let mut last = None;
    for transaction in &held {
        let count = committed.iter().filter(|seen| *seen == transaction).count();
        assert_eq!(
            count, 1,
            "held transaction {transaction:?} is committed exactly once: {committed:?}"
        );
        let at = committed
            .iter()
            .position(|seen| seen == transaction)
            .expect("counted above");
        assert!(
            last.is_none_or(|before| before < at),
            "held work is staged in receipt order: held {held:?}, committed {committed:?}"
        );
        last = Some(at);
    }
    assert_eq!(
        stale_outcomes(&retry),
        0,
        "nothing retried was refused as stale: {:?}",
        retry.outcomes
    );
    fixture.harvest.append(&mut retry.effects);
    let _surface = fixture.admitted_surface();
    drop(peer);
}

/// The post-receipt reading does not resurrect a client that has ended.
///
/// REVOCATION STILL WINS. A client that disconnected before its received work
/// was staged is absent or closed in the reading taken after receipt, so that
/// work is refused as stale and nothing is admitted for it.
#[test]
fn work_from_a_client_that_ended_before_staging_is_refused_not_admitted() {
    let mut fixture = Fixture::started(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let mut peer = fixture.connect();
    let window = peer.create_map_and_draw();
    peer.confirm_geometry(window);
    drop(peer);
    post_receipt_until("the boundary to record the client as ended", || {
        fixture
            .handle()
            .admitted()
            .expect("the boundary is readable")
            .iter()
            .all(|row| row.closed || !row.lifecycle_open)
    });

    let report = fixture
        .handle_mut()
        .apply_committed(WAIT)
        .expect("the bridge is readable");
    assert!(report.batches_observed > 0, "the ended client's work was received: {report:?}");
    assert!(
        stale_outcomes(&report) > 0,
        "the ended client's work is refused as stale: {:?}",
        report.outcomes
    );
    assert!(
        !report
            .effects
            .iter()
            .any(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface),
        "nothing is admitted for an ended client: {:?}",
        report.effects
    );
}
