use super::*;

/// Queue one Present and take it to the head of the runnable queue, the state
/// `drive_gpu_presentation` reaches before it judges visibility.
fn ready_candidate(
    scheduler: &mut LiveProductionPresentScheduler,
    resources: &mut LivePresentationResourceSession,
    surface: SurfaceId,
    id: u64,
    now: Instant,
) -> (TransactionId, sophia_protocol::SurfaceTransactionKey) {
    let handle = BufferHandle::from_raw(id);
    let transaction = TransactionId::from_raw(id);
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let batch = scheduler_batch(transaction, surface, handle);
    scheduler
        .enqueue_group(&batch.groups[0], &[], resources, now)
        .unwrap();
    assert_eq!(
        scheduler.poll_gate(resources, now).unwrap(),
        LiveProductionPresentGate::Ready(transaction)
    );
    let candidate = scheduler.front().unwrap().candidate.key();
    (transaction, candidate)
}

const REFRESH: Duration = Duration::from_micros(16_666);

#[test]
fn a_present_no_head_can_carry_waits_for_the_heads_next_refresh() {
    // The offscreen client: its Present is settled as skipped either way, and
    // what the park buys is that the settlement -- and so the Idle that frees
    // its buffer -- arrives on the head's clock rather than instantly.
    let mut resources = LivePresentationResourceSession::default();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let now = Instant::now();
    let surface = SurfaceId::new(501, 1);
    let (transaction, candidate) =
        ready_candidate(&mut scheduler, &mut resources, surface, 501, now);

    assert!(scheduler.defer_to_frame_tick(candidate, now, REFRESH));
    assert_eq!(scheduler.paced_skips(), 1);
    assert_eq!(scheduler.frame_tick_parked(), 1);

    // Nothing may run it again, and nothing may mistake it for a Present a
    // layout epoch is still deciding about.
    assert!(!scheduler.has_eligible());
    assert!(!scheduler.has_runnable_queued());
    assert!(
        !scheduler.has_layout_deferred(),
        "a paced candidate is waiting on the clock, not on a layout decision"
    );
    assert_eq!(
        scheduler.poll_gate(&mut resources, now).unwrap(),
        LiveProductionPresentGate::Idle
    );

    assert!(
        scheduler.release_frame_tick(now + REFRESH / 2).is_empty(),
        "released before its tick, the pacing would buy nothing"
    );
    assert_eq!(scheduler.frame_tick_deadline(), Some(now + REFRESH));
    assert_eq!(scheduler.release_frame_tick(now + REFRESH), [transaction]);
    assert_eq!(scheduler.frame_tick_parked(), 0);
    assert_eq!(scheduler.frame_tick_deadline(), None);
    assert!(scheduler.release_frame_tick(now + REFRESH).is_empty());
}

#[test]
fn candidates_parked_inside_one_interval_leave_on_one_tick() {
    // A burst settles together on the tick, the way presents queued against
    // one vblank complete together, rather than each starting an interval of
    // its own and stretching the flood across as many ticks as there are
    // frames -- which would pace the client far below the head.
    let mut resources = LivePresentationResourceSession::default();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let now = Instant::now();
    let first = SurfaceId::new(511, 1);
    let second = SurfaceId::new(512, 1);
    let (first_transaction, first_candidate) =
        ready_candidate(&mut scheduler, &mut resources, first, 511, now);
    assert!(scheduler.defer_to_frame_tick(first_candidate, now, REFRESH));

    let later = now + REFRESH / 4;
    let (second_transaction, second_candidate) =
        ready_candidate(&mut scheduler, &mut resources, second, 512, later);
    assert!(scheduler.defer_to_frame_tick(second_candidate, later, REFRESH));

    assert_eq!(
        scheduler.frame_tick_deadline(),
        Some(now + REFRESH),
        "the second park joins the tick already running, it does not start one"
    );
    assert_eq!(
        scheduler.release_frame_tick(now + REFRESH),
        [first_transaction, second_transaction],
        "settled in queue order, oldest first"
    );
    assert_eq!(scheduler.max_frame_tick_parked(), 2);
}

#[test]
fn a_client_that_never_waits_for_its_buffers_is_bounded() {
    // Withholding the Idle is what paces a conforming client: it blocks on its
    // own back buffers, of which Mesa keeps four. One that does not wait would
    // otherwise grow this queue for as long as it stayed invisible.
    let mut resources = LivePresentationResourceSession::default();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let now = Instant::now();
    let surface = SurfaceId::new(521, 1);
    let mut parked = Vec::new();
    for id in 521..530 {
        let (transaction, candidate) =
            ready_candidate(&mut scheduler, &mut resources, surface, id, now);
        assert!(scheduler.defer_to_frame_tick(candidate, now, REFRESH));
        parked.push(transaction);
    }
    assert_eq!(scheduler.frame_tick_parked(), 9);

    let overflowed = scheduler.bound_frame_tick_parking(surface);

    assert_eq!(
        overflowed,
        [parked[0]],
        "the oldest is the one settled early, never the newest"
    );
    assert_eq!(scheduler.frame_tick_overflows(), 1);
    assert_eq!(scheduler.frame_tick_parked(), 8);
    // Still paced: the overflow settles one candidate early, it does not open
    // the gate for the rest.
    assert!(scheduler.release_frame_tick(now).is_empty());
    assert_eq!(scheduler.release_frame_tick(now + REFRESH).len(), 8);
}

#[test]
fn a_topology_skip_takes_the_paced_candidates_too() {
    // A topology wait blocks until no present is outstanding, and it cannot
    // wait out a tick it does not know about. Leaving a paced candidate behind
    // would deadlock that wait against a client that has been told nothing.
    let mut resources = LivePresentationResourceSession::default();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let now = Instant::now();
    let surface = SurfaceId::new(531, 1);
    let (transaction, candidate) =
        ready_candidate(&mut scheduler, &mut resources, surface, 531, now);
    assert!(scheduler.defer_to_frame_tick(candidate, now, REFRESH));

    assert_eq!(scheduler.drain_runnable_transactions(), [transaction]);
    assert_eq!(scheduler.frame_tick_parked(), 0);
    assert_eq!(
        scheduler.frame_tick_deadline(),
        None,
        "a drained tick must not keep waking the owner"
    );
}

#[test]
fn a_candidate_that_spent_its_first_visibility_budget_is_not_parked_again() {
    // The budget exists to bound exactly this kind of wait. Adding a tick on
    // top of an expired two-second park would extend the wait the expiry just
    // ended.
    let mut resources = LivePresentationResourceSession::default();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let now = Instant::now();
    let surface = SurfaceId::new(541, 1);
    let (_, candidate) = ready_candidate(&mut scheduler, &mut resources, surface, 541, now);
    assert!(scheduler.defer_first_visibility(
        candidate,
        LiveProductionFirstVisibilityReason::OutsideHeadFrames,
        now,
    ));
    assert_eq!(
        scheduler
            .expire_first_visibility(now + Duration::from_secs(3))
            .len(),
        1
    );

    assert!(
        scheduler.front_first_visibility_exhausted(),
        "the present path reads this to know not to park it a second time"
    );
}
