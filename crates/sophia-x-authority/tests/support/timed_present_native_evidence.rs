//! Native adapter evidence with supplied samples; no DRM or owner-loop proof.
use super::*;

fn native_complete(
    f: &Fixture,
    transaction: TransactionId,
    ust: u64,
    samples: &[crate::XPresentClockSample],
) -> bool {
    f.broker
        .protocol_router()
        .route_present_complete_with_evidence(
            transaction,
            (ust, 999_999),
            samples.iter().copied().map(Into::into),
            XPresentCompletionMode::Copy,
            None,
        )
        .unwrap()
        .0
        .routed
}

#[test]
fn slow_primary_sample_wins_over_fast_sibling_without_changing_idle_permission() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(2, 10), 11);
    let transaction = execute(&f, f.preparation);
    let (route, reported) = f
        .broker
        .protocol_router()
        .route_present_complete_with_evidence(
            transaction,
            (12_000, 999_999),
            [sample(1, 200), sample(2, 12)].into_iter().map(Into::into),
            XPresentCompletionMode::Copy,
            None,
        )
        .unwrap();
    assert!(route.routed);
    assert_eq!(reported, Some((12_000, 12)));
    assert_eq!(event(&f), (41, 12_000, 12));
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(!native_complete(&f, transaction, 13_000, &[sample(2, 13)]));
    assert!(
        f.broker
            .protocol_router()
            .route_present_idle(transaction)
            .unwrap()
    );
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentIdleNotify { .. }
    ));
}

#[test]
fn missing_nonprimary_event_and_out_fence_complete_without_waiting_or_fabricating_msc() {
    for samples in [vec![sample(1, 200)], vec![]] {
        let f = Fixture::new(true);
        subscribe(&f);
        f.schedule(sample(2, 10), 11);
        let transaction = execute(&f, f.preparation);
        f.broker
            .present_clock_router()
            .observe_source(sample(2, 11))
            .unwrap();
        assert!(native_complete(&f, transaction, 12_000, &samples));
        assert_eq!(event(&f), (41, 11_000, 11));
        assert_eq!(f.broker.present_clock_router().completed_count(), 1);
        assert!(
            f.broker
                .present_clock_router()
                .bound_sources()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn old_incarnation_event_never_matches_a_reused_head() {
    let f = Fixture::new(true);
    subscribe(&f);
    let mut current = sample(1, 10);
    current.source = crate::XPresentClockSource::Hardware {
        domain: 1,
        incarnation: 2,
    };
    f.schedule(current, 11);
    let transaction = execute(&f, f.preparation);
    assert!(native_complete(&f, transaction, 20_000, &[sample(1, 20)]));
    assert_eq!(event(&f), (41, 10_000, 10));
}

#[test]
fn native_complete_updates_queued_progress_and_the_next_rebind_anchor() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let old = execute(&f, f.preparation);
    let waiting = f.prepare_next(PIXMAP, false);
    f.state
        .runtime
        .lock()
        .unwrap()
        .schedule_prepared_present(
            waiting,
            crate::XPresentMscTiming::new(16, 0, 0, false).unwrap(),
            sample(1, 10),
            None,
        )
        .unwrap();
    assert!(
        !f.state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_is_ready(waiting)
    );
    assert!(native_complete(&f, old, 15_000, &[sample(1, 15)]));
    assert_eq!(event(&f), (41, 15_000, 15));
    assert!(
        f.state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_is_ready(waiting)
    );
    assert_eq!(
        f.broker
            .present_clock_router()
            .completion_observation_counts(),
        (1, 0)
    );
    let (next, rebound) = schedule_next(&f, sample(2, 800), 16);
    assert!(native_complete(&f, rebound, 801_000, &[sample(2, 801)]));
    assert_eq!(event(&f), (next.raw() as u32, 801_000, 16));
}

#[test]
fn historical_native_event_reports_its_time_but_never_rejects_or_rewinds_the_anchor() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let old = execute(&f, f.preparation);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 20)).unwrap();
    let before = f.state.runtime.lock().unwrap().present_timing_statistics();
    assert!(native_complete(&f, old, 15_000, &[sample(1, 15)]));
    assert_eq!(event(&f), (41, 15_000, 15));
    assert_eq!(clocks.completion_observation_counts(), (1, 1));
    assert_eq!(
        f.state.runtime.lock().unwrap().present_timing_statistics(),
        before
    );
    assert_eq!(
        f.broker
            .registry
            .pending_presentations
            .rejected_clock_samples
            .load(Ordering::Relaxed),
        0
    );
    let (next, rebound) = schedule_next(&f, sample(2, 800), 21);
    assert!(native_complete(&f, rebound, 801_000, &[sample(2, 801)]));
    assert_eq!(event(&f), (next.raw() as u32, 801_000, 21));
}

#[test]
fn lost_then_late_native_event_does_not_advance_the_rebind_anchor() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let old = execute(&f, f.preparation);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 15)).unwrap();
    clocks.lose_source(sample(1, 0).source).unwrap();
    assert!(native_complete(&f, old, 19_000, &[sample(1, 19)]));
    assert_eq!(event(&f), (41, 15_000, 15));
    assert_eq!(clocks.completion_observation_counts(), (0, 0));
    let (next, rebound) = schedule_next(&f, sample(2, 800), 16);
    assert!(native_complete(&f, rebound, 801_000, &[sample(2, 801)]));
    assert_eq!(event(&f), (next.raw() as u32, 801_000, 16));
}

#[test]
fn rebound_executed_only_native_completion_does_not_acquire_runtime() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let old = execute(&f, f.preparation);
    let _new = schedule_next(&f, sample(2, 800), 11);
    let router = f.broker.protocol_router();
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::scope(|threads| {
        let held = f.state.runtime.lock().unwrap();
        threads.spawn(move || {
            sent.send(
                router
                    .route_present_complete_with_evidence(
                        old,
                        (11_000, 123),
                        [sample(1, 11).into()].into_iter(),
                        XPresentCompletionMode::Copy,
                        None,
                    )
                    .unwrap()
                    .0
                    .routed,
            )
            .unwrap();
        });
        let result = received.recv_timeout(std::time::Duration::from_secs(1));
        drop(held);
        assert!(result.unwrap());
    });
    assert_eq!(event(&f), (41, 11_000, 11));
    assert_eq!(
        f.broker
            .present_clock_router()
            .completion_observation_counts(),
        (0, 0)
    );
}

#[test]
fn decoded_historical_evidence_never_enters_history_or_advances_a_lagging_frontend() {
    for observed in [10, 20] {
        let f = Fixture::new(true);
        subscribe(&f);
        f.schedule(sample(1, 10), 11);
        let old = execute(&f, f.preparation);
        let clocks = f.broker.present_clock_router();
        clocks.observe_source(sample(1, observed)).unwrap();
        let waiting = f.prepare_next(PIXMAP, false);
        f.state
            .runtime
            .lock()
            .unwrap()
            .schedule_prepared_present(
                waiting,
                crate::XPresentMscTiming::new(observed + 6, 0, 0, false).unwrap(),
                sample(1, observed),
                None,
            )
            .unwrap();
        let history = f.broker.registry.completion_clock_snapshot(old);
        let stats = f.state.runtime.lock().unwrap().present_timing_statistics();
        let (route, reported) = f
            .broker
            .protocol_router()
            .route_present_complete_with_evidence(
                old,
                (15_000, 999_999),
                [crate::XPresentRetirementClock {
                    sample: sample(1, 15),
                    historical: true,
                }]
                .into_iter(),
                XPresentCompletionMode::Copy,
                None,
            )
            .unwrap();
        assert!(route.routed);
        assert_eq!(reported, Some((15_000, 15)));
        assert_eq!(event(&f), (41, 15_000, 15));
        assert_eq!(f.broker.registry.completion_clock_snapshot(old), history);
        assert_eq!(
            f.state.runtime.lock().unwrap().present_timing_statistics(),
            stats
        );
        assert!(
            !f.state
                .runtime
                .lock()
                .unwrap()
                .prepared_present_is_ready(waiting)
        );
        assert_eq!(clocks.completion_observation_counts(), (0, 1));
        // Rebinding starts from the actual accepted frontend anchor, not 15.
        let (next, rebound) = schedule_next(&f, sample(2, 800), observed + 1);
        assert!(native_complete(&f, rebound, 801_000, &[sample(2, 801)]));
        assert_eq!(event(&f), (next.raw() as u32, 801_000, observed + 1));
    }
}
