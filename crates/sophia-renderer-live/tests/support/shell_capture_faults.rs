#[test]
fn begin_refusal_distinguishes_rollback_from_an_already_submitted_job() {
    let (mut inbox, mut executor, _, budget, epoch) = fixture();
    let now = Instant::now();
    let (request, mut peer) = make_request(&budget, now);
    let id = inbox.submit(request).unwrap();
    executor.next(now).unwrap();
    assert_eq!(
        executor.begin(JobId(0), now).unwrap_err(),
        BeginFailure::UnknownJob
    );
    inbox.cancel(id);
    assert_eq!(
        executor.begin(id, now).unwrap_err(),
        BeginFailure::RolledBack(StopReason::Cancelled)
    );
    held(&mut peer);
    drop(inbox.take_completed(now).unwrap());
    released(&mut peer);
    let (request, mut peer) = make_request(&budget, now);
    let id = inbox.submit(request).unwrap();
    executor.next(now).unwrap();
    executor.begin(id, now).unwrap();
    assert_eq!(
        executor.begin(id, now).unwrap_err(),
        BeginFailure::AlreadySubmitted
    );
    assert!(inbox.take_completed(now).unwrap().completions.is_empty());
    held(&mut peer);
    executor.complete(observed(id, &epoch), 1, now).unwrap();
    drop(inbox.take_completed(now).unwrap());
    released(&mut peer);
}

#[test]
fn proofs_require_the_exact_job_epoch_and_submission_and_cannot_repeat() {
    let (mut inbox, mut executor, _, budget, epoch) = fixture();
    let now = Instant::now();
    let (request, mut peer) = make_request(&budget, now);
    let id = inbox.submit(request).unwrap();
    executor.next(now).unwrap();
    assert_eq!(
        executor.complete(observed(id, &epoch), 8, now).unwrap_err(),
        (ProofFailure::NotSubmitted, 8)
    );
    executor.begin(id, now).unwrap();
    let impostor = LiveRendererSnapshotEpoch::new(1, 2, 3);
    assert_eq!(
        executor
            .complete(observed(id, &impostor), 9, now)
            .unwrap_err(),
        (ProofFailure::Generation, 9)
    );
    assert_eq!(
        executor
            .complete(observed(JobId(0), &epoch), 10, now)
            .unwrap_err(),
        (ProofFailure::UnknownJob, 10)
    );
    assert!(inbox.take_completed(now).unwrap().completions.is_empty());
    held(&mut peer);
    executor.complete(observed(id, &epoch), 11, now).unwrap();
    assert_eq!(
        executor
            .complete(observed(id, &epoch), 12, now)
            .unwrap_err(),
        (ProofFailure::UnknownJob, 12)
    );
    let batch = inbox.take_completed(now).unwrap();
    assert_eq!(batch.completions.len(), 1);
    assert!(matches!(
        batch.completions[0].outcome,
        Outcome::Completed { output: 11, .. }
    ));
    drop(batch);
    released(&mut peer);
    let (request, _) = make_request(&budget, now);
    let newer = inbox.submit(request).unwrap();
    assert_ne!(newer, id);
    executor.next(now).unwrap();
    executor.begin(newer, now).unwrap();
    assert_eq!(
        executor
            .complete(observed(id, &epoch), 13, now)
            .unwrap_err(),
        (ProofFailure::UnknownJob, 13)
    );
    assert_eq!(budget.usage(), (1, 64));
    executor.complete(observed(newer, &epoch), 14, now).unwrap();
    drop(inbox.take_completed(now).unwrap());
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn close_settles_every_stage_and_quarantine_outlives_receipts_and_mailboxes() {
    let (mut inbox, mut executor, graveyard, budget, _) = fixture();
    let now = Instant::now();
    let mut peers = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..4 {
        let (request, peer) = make_request(&budget, now);
        peers.push(peer);
        ids.push(inbox.submit(request).unwrap());
    }
    executor.next(now).unwrap();
    executor.begin(ids[0], now).unwrap(); // May have reached the GPU.
    executor.next(now).unwrap(); // Selected, but strictly before GPU use.
    executor.close(); // The other two requests are still queued.
    assert!(ready(&inbox));
    let batch = inbox.take_completed(now).unwrap();
    assert!(!batch.fault);
    assert_eq!(batch.completions.len(), 4);
    assert_eq!(
        batch
            .completions
            .iter()
            .filter(|c| matches!(c.outcome, Outcome::FailedUncertain))
            .count(),
        1
    );
    assert_eq!(
        batch
            .completions
            .iter()
            .filter(|c| matches!(c.outcome, Outcome::RolledBack { .. }))
            .count(),
        3
    );
    drop(batch);
    held(&mut peers[0]);
    for peer in &mut peers[1..] {
        released(peer);
    }
    assert_eq!(graveyard.usage(), (1, 1));
    assert_eq!(budget.usage(), (1, 64));
    executor.close();
    assert!(inbox.take_completed(now).unwrap().completions.is_empty());
    drop((inbox, executor));
    // A replacement mailbox must share the old graveyard, not reset limits.
    let (mut replacement, driver) =
        channel::<_, u64>(LiveRendererSnapshotEpoch::new(1, 2, 4), graveyard.clone()).unwrap();
    let (request, _) = make_request(&budget, now);
    let (reason, request) = replacement.submit(request).unwrap_err();
    assert_eq!(reason, SubmitFailure::Capacity);
    drop(request);
    drop((replacement, driver));
    held(&mut peers[0]);
    assert_eq!(budget.usage(), (1, 64));
    // Supplied teardown boundary only. The real renderer must retain this
    // owner until its context/display destruction is actually proven.
    drop(graveyard);
    released(&mut peers[0]);
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn unwinding_after_begin_wakes_the_owner_and_keeps_uncertain_custody() {
    let (mut inbox, mut executor, graveyard, budget, _) = fixture();
    let now = Instant::now();
    let (request, mut peer) = make_request(&budget, now);
    let id = inbox.submit(request).unwrap();
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        executor.next(now).unwrap();
        executor.begin(id, now).unwrap();
        panic!("supplied GPU-operation unwind");
    }));
    assert!(unwind.is_err());
    assert!(ready(&inbox));
    let batch = inbox.take_completed(now).unwrap();
    assert!(!batch.fault);
    assert!(matches!(
        batch.completions[0].outcome,
        Outcome::FailedUncertain
    ));
    drop((batch, inbox));
    held(&mut peer);
    assert_eq!(budget.usage(), (1, 64));
    drop(graveyard); // Supplied teardown, not a timeout.
    released(&mut peer);
}

#[test]
fn losing_the_consumer_still_requires_completion_or_quarantine() {
    for complete in [false, true] {
        let (mut inbox, mut executor, graveyard, budget, epoch) = fixture();
        let now = Instant::now();
        let (request, mut peer) = make_request(&budget, now);
        let id = inbox.submit(request).unwrap();
        executor.next(now).unwrap();
        executor.begin(id, now).unwrap();
        drop(inbox);
        held(&mut peer);
        if complete {
            executor.complete(observed(id, &epoch), 1, now).unwrap();
            released(&mut peer);
            assert_eq!(budget.usage(), (0, 0));
        } else {
            executor.uncertain(id).unwrap();
            held(&mut peer);
            drop(executor);
            assert_eq!(budget.usage(), (1, 64));
            drop(graveyard);
            released(&mut peer);
        }
    }
}

#[test]
fn shutdown_does_not_report_missing_receipts_while_settlement_is_in_progress() {
    let (mut inbox, mut executor, _, budget, epoch) = fixture();
    let now = Instant::now();
    let (first, _) = make_request(&budget, now);
    let (second, _) = make_request(&budget, now);
    let first = inbox.submit(first).unwrap();
    let second = inbox.submit(second).unwrap();
    executor.next(now).unwrap();
    // Supply the prefix of close, stopping after its first terminal. Closing
    // admission is not the same event as publishing every settlement.
    executor.alive.store(false, Ordering::Release);
    epoch.invalidate();
    executor.uncertain(first).unwrap(); // Selected, never submitted: rollback.
    let batch = inbox.take_completed(now).unwrap();
    assert!(!batch.fault);
    assert_eq!(batch.completions.len(), 1);
    assert_eq!(batch.completions[0].id, first);
    drop(batch);
    assert_eq!(budget.usage(), (1, 64));
    executor.close();
    let batch = inbox.take_completed(now).unwrap();
    assert!(!batch.fault);
    assert_eq!(batch.completions[0].id, second);
    drop(batch);
    assert_eq!(budget.usage(), (0, 0));
}

#[test]
fn racing_enqueue_with_shutdown_never_loses_an_accepted_job() {
    fn assert_send<T: Send>() {}
    assert_send::<Inbox>();
    for _ in 0..32 {
        let (mut inbox, mut executor, _, budget, _) = fixture();
        let now = Instant::now();
        let (request, mut peer) = make_request(&budget, now);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let producer_barrier = barrier.clone();
        let thread = std::thread::spawn(move || {
            producer_barrier.wait();
            let result = inbox.submit(request);
            (inbox, result)
        });
        barrier.wait();
        executor.close();
        let (mut inbox, result) = thread.join().unwrap();
        match result {
            Ok(id) => {
                let batch = inbox.take_completed(now).unwrap();
                assert!(!batch.fault);
                assert_eq!(batch.completions.len(), 1);
                assert_eq!(batch.completions[0].id, id);
                assert!(matches!(
                    batch.completions[0].outcome,
                    Outcome::RolledBack { .. }
                ));
                drop(batch);
            }
            Err((reason, request)) => {
                assert_eq!(reason, SubmitFailure::Closed);
                drop(request);
            }
        }
        released(&mut peer);
        assert_eq!(budget.usage(), (0, 0));
    }
}
