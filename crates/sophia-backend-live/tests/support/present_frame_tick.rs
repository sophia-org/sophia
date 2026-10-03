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
fn visibility_early_release_is_exact_and_leaves_other_background_debt_parked() {
    let mut resources = LivePresentationResourceSession::default();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let now = Instant::now();
    let first = SurfaceId::new(541, 1);
    let other = SurfaceId::new(542, 1);
    let (a, candidate) = ready_candidate(&mut scheduler, &mut resources, first, 541, now);
    assert!(scheduler.defer_to_frame_tick(candidate, now, Duration::from_secs(1), false));
    let (b, candidate) = ready_candidate(&mut scheduler, &mut resources, other, 542, now);
    assert!(scheduler.defer_to_frame_tick(candidate, now, Duration::from_secs(1), false));
    let visible = now + Duration::from_millis(1);
    assert_eq!(
        scheduler.release_frame_tick_or_visible(visible, &[first]),
        [a]
    );
    assert!(
        scheduler
            .release_frame_tick_or_visible(visible, &[first])
            .is_empty()
    );
    assert_eq!(scheduler.frame_tick_parked(), 1);
    assert_eq!(
        scheduler.frame_tick_deadline(),
        Some(now + Duration::from_secs(1))
    );
    assert_eq!(
        scheduler.release_frame_tick(now + Duration::from_secs(1)),
        [b]
    );
}

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

    assert!(scheduler.defer_to_frame_tick(candidate, now, REFRESH, false));
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
    assert!(scheduler.defer_to_frame_tick(first_candidate, now, REFRESH, false));

    let later = now + REFRESH / 4;
    let (second_transaction, second_candidate) =
        ready_candidate(&mut scheduler, &mut resources, second, 512, later);
    assert!(scheduler.defer_to_frame_tick(second_candidate, later, REFRESH, false));

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
fn a_many_buffer_client_cannot_bypass_pacing_by_overflowing_eight_parks() {
    let mut resources = LivePresentationResourceSession::default();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let now = Instant::now();
    let surface = SurfaceId::new(521, 1);
    let mut parked = Vec::new();
    // The live X frontend backpressures this client's 65th request until a
    // completion frees capacity. Every accepted request keeps its ownership.
    for id in 521..585 {
        let arrival = now + Duration::from_millis(id - 521);
        let (transaction, candidate) =
            ready_candidate(&mut scheduler, &mut resources, surface, id, arrival);
        assert!(scheduler.defer_to_frame_tick(candidate, arrival, Duration::from_secs(1), false));
        parked.push(transaction);
        assert!(scheduler.release_frame_tick(arrival).is_empty());
    }
    assert_eq!(scheduler.frame_tick_parked(), 64);
    assert_eq!(scheduler.frame_tick_overflows(), 0);
    assert!(
        scheduler
            .release_frame_tick(now + Duration::from_millis(999))
            .is_empty()
    );
    assert_eq!(
        scheduler.release_frame_tick(now + Duration::from_secs(1)),
        parked
    );
    assert_eq!(scheduler.frame_tick_parked(), 0);
}

#[test]
fn background_pressure_reserves_capacity_for_visible_work() {
    let mut resources = LivePresentationResourceSession::default();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let now = Instant::now();
    let mut pressure_released = 0;
    // Five groups of 64 accepted Presents. Distinct surfaces exercise the
    // runtime's one-content-owner-per-surface shape, rather than pretending
    // one window can own all of these concurrently in production.
    for id in 1000..1320 {
        let surface = SurfaceId::new(id as u32, 1);
        let (_, candidate) = ready_candidate(&mut scheduler, &mut resources, surface, id, now);
        assert!(scheduler.defer_to_frame_tick(candidate, now, Duration::from_secs(1), false));
        for old in scheduler.bound_frame_tick_parking() {
            assert!(resources.reject(old).is_some());
            assert_eq!(
                resources.release_source(BufferHandle::from_raw(old.raw())),
                sophia_renderer_live::LiveResourceReleaseStatus::Released
            );
            pressure_released += 1;
        }
        assert!(scheduler.frame_tick_parked() <= 64);
        assert!(
            scheduler
                .release_frame_tick(now + Duration::from_millis(999))
                .is_empty()
        );
        // Visible admission continues to get a registration and an acquire
        // gate. It does not borrow an entry from the background budget.
        let (visible, _) = ready_candidate(
            &mut scheduler,
            &mut resources,
            SurfaceId::new(2000, 1),
            2000 + id,
            now,
        );
        scheduler.pop_front();
        assert!(resources.reject(visible).is_some());
        assert_eq!(
            resources.release_source(BufferHandle::from_raw(visible.raw())),
            sophia_renderer_live::LiveResourceReleaseStatus::Released
        );
    }
    assert_eq!(pressure_released, 256);
    assert_eq!(scheduler.frame_tick_overflows(), 256);
    assert_eq!(
        scheduler
            .release_frame_tick(now + Duration::from_secs(1))
            .len(),
        64
    );
}

#[test]
fn a_visible_predicate_at_park_does_not_release_on_every_owner_pass() {
    for hidden_edge in [false, true] {
        let mut resources = LivePresentationResourceSession::default();
        let mut scheduler = LiveProductionPresentScheduler::default();
        let now = Instant::now();
        let surface = SurfaceId::new(586, 1);
        let (transaction, candidate) =
            ready_candidate(&mut scheduler, &mut resources, surface, 586, now);
        assert!(scheduler.defer_to_frame_tick(candidate, now, Duration::from_secs(1), true));
        for ms in 1..100 {
            assert!(
                scheduler
                    .release_frame_tick_or_visible(now + Duration::from_millis(ms), &[surface])
                    .is_empty()
            );
        }
        if hidden_edge {
            assert!(
                scheduler
                    .release_frame_tick_or_visible(now + Duration::from_millis(100), &[])
                    .is_empty()
            );
            assert_eq!(
                scheduler
                    .release_frame_tick_or_visible(now + Duration::from_millis(101), &[surface]),
                [transaction]
            );
        } else {
            assert_eq!(
                scheduler.release_frame_tick(now + Duration::from_secs(1)),
                [transaction]
            );
        }
        assert!(
            scheduler
                .release_frame_tick(now + Duration::from_secs(2))
                .is_empty()
        );
    }
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
    assert!(scheduler.defer_to_frame_tick(candidate, now, REFRESH, false));

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
