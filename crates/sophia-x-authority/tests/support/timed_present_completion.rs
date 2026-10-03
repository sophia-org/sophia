//! Completion routing with supplied head samples and supplied retirement
//! permission. This does not simulate KMS or prove the mirror custody adapter.
use super::*;

#[test]
fn an_empty_session_has_no_clock_demand_before_the_first_x_client_binds_runtime() {
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(64).unwrap());
    let clocks = broker.present_clock_router();
    assert!(clocks.bound_sources().unwrap().is_empty());
    assert!(clocks.query_demands().unwrap().is_empty());
    clocks.observe_source(sample(1, 10)).unwrap();
    clocks.lose_source(sample(1, 0).source).unwrap();
    assert_eq!(clocks.completed_count(), 0);
}

fn sample(head: u64, msc: u64) -> crate::XPresentClockSample {
    crate::XPresentClockSample {
        source: crate::XPresentClockSource::Hardware {
            domain: head,
            incarnation: 1,
        },
        ust: msc * 1_000,
        msc,
    }
}

fn execute(f: &Fixture, preparation: TransactionId) -> TransactionId {
    let mut generated = XGeneratedEgress::default();
    assert!(
        execute_timed_present(
            &f.state,
            &f.broker.registry,
            preparation,
            &mut generated,
            |runtime, _| timed_present_ready(runtime, preparation, 0)
        )
        .unwrap()
    );
    generated.take().next().unwrap().transaction
}

#[test]
fn execution_freezes_the_offset_and_idle_does_not_release_completion_demand() {
    let f = Fixture::new(true);
    f.broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    let clocks = f.broker.present_clock_router();
    let feedback = f.broker.protocol_router();
    f.schedule(sample(1, 10), 11);
    let transaction = execute(&f, f.preparation);
    let next = f.prepare_next(PIXMAP, false);
    // Rebind this window's next request, keeping its continuous MSC=10.
    f.state
        .runtime
        .lock()
        .unwrap()
        .schedule_prepared_present(
            next,
            crate::XPresentMscTiming::new(11, 0, 0, false).unwrap(),
            sample(2, 800),
            None,
        )
        .unwrap();
    let successor = execute(&f, next);
    assert!(feedback.route_present_idle(transaction).unwrap());
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentIdleNotify { .. }
    ));
    assert_eq!(
        clocks.bound_sources().unwrap(),
        vec![sample(1, 0).source, sample(2, 0).source]
    );
    assert!(
        feedback
            .route_clocked_present_complete(
                transaction,
                sample(1, 11),
                XPresentCompletionMode::Copy,
                None
            )
            .unwrap()
            .routed
    );
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentCompleteNotify {
            msc: 11,
            ust: 11_000,
            ..
        }
    ));
    assert_eq!(clocks.bound_sources().unwrap(), vec![sample(2, 0).source]);
    assert!(
        feedback
            .route_clocked_present_complete(
                successor,
                sample(2, 802),
                XPresentCompletionMode::Copy,
                None
            )
            .unwrap()
            .routed
    );
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentCompleteNotify {
            msc: 12,
            ust: 802_000,
            ..
        }
    ));
    assert!(clocks.bound_sources().unwrap().is_empty());
    // A duplicate Complete never sends another event; Idle remains separate.
    assert!(
        !feedback
            .route_clocked_present_complete(
                successor,
                sample(2, 802),
                XPresentCompletionMode::Copy,
                None
            )
            .unwrap()
            .routed
    );
    assert!(f._channels.protocol.try_recv().is_err());
    feedback.route_present_idle(successor).unwrap();
    assert!(
        f.broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(f.broker.registry.present_clock.lock().unwrap().is_none());
}

fn subscribe(f: &Fixture) {
    f.broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
}

fn complete(f: &Fixture, transaction: TransactionId, sample: crate::XPresentClockSample) -> bool {
    f.broker
        .protocol_router()
        .route_clocked_present_complete(transaction, sample, XPresentCompletionMode::Copy, None)
        .unwrap()
        .routed
}

fn event(f: &Fixture) -> (u32, u64, u64) {
    match f._channels.protocol.try_recv().unwrap() {
        XClientEvent::PresentCompleteNotify {
            serial, ust, msc, ..
        } => (serial, ust, msc),
        other => panic!("unexpected feedback {other:?}"),
    }
}

fn schedule_next(
    f: &Fixture,
    sample: crate::XPresentClockSample,
    target: u64,
) -> (TransactionId, TransactionId) {
    let preparation = f.prepare_next(PIXMAP, false);
    f.state
        .runtime
        .lock()
        .unwrap()
        .schedule_prepared_present(
            preparation,
            crate::XPresentMscTiming::new(target, 0, 0, false).unwrap(),
            sample,
            None,
        )
        .unwrap();
    (preparation, execute(f, preparation))
}

#[test]
fn saved_chosen_head_sample_survives_later_queries_and_does_not_release_idle() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let transaction = execute(&f, f.preparation);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 30)).unwrap();
    assert!(f._channels.protocol.try_recv().is_err());
    assert_eq!(
        f.broker.protocol_router().route_present_complete(
            transaction,
            800_000,
            800,
            XPresentCompletionMode::Copy
        ),
        Err(XServerFrontendRouteError::PresentClockMismatch { transaction })
    );
    assert!(complete(&f, transaction, sample(1, 11)));
    assert_eq!(event(&f), (41, 11_000, 11));
    assert!(clocks.bound_sources().unwrap().is_empty());
    assert!(
        f.broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .contains_key(&transaction)
    );
    assert!(!complete(&f, transaction, sample(1, 12)));
    assert!(f._channels.protocol.try_recv().is_err());
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
    assert!(
        f.broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn off_head_then_on_head_retirements_complete_immediately_in_delivery_order() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let first = execute(&f, f.preparation);
    let (next, second) = schedule_next(&f, sample(1, 10), 11);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 11)).unwrap();
    // Copy custody idles even without PresentOptionCopy, independently of Complete.
    f.broker
        .protocol_router()
        .route_present_idle(first)
        .unwrap();
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentIdleNotify { .. }
    ));
    assert!(complete(&f, first, sample(2, 800)));
    assert_eq!(event(&f), (41, 11_000, 11));
    assert!(complete(&f, second, sample(1, 12)));
    assert_eq!(event(&f), (next.raw() as u32, 12_000, 12));
    clocks.observe_source(sample(1, 20)).unwrap();
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(clocks.bound_sources().unwrap().is_empty());
}

#[test]
fn off_head_retirement_never_uses_a_bound_observation_from_after_the_event() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let first = execute(&f, f.preparation);
    let (next, second) = schedule_next(
        &f,
        crate::XPresentClockSample {
            source: sample(2, 0).source,
            ust: 11_000,
            msc: 800,
        },
        11,
    );
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 11)).unwrap();
    clocks.observe_source(sample(1, 12)).unwrap();
    // Pass-entry clock service can observe A=20 before the owner processes
    // the already queued off-head retirement from UST=12_000.
    clocks.observe_source(sample(1, 20)).unwrap();
    assert!(complete(
        &f,
        first,
        crate::XPresentClockSample {
            source: sample(2, 0).source,
            ust: 12_000,
            msc: 801,
        }
    ));
    // Retain A=12 across the later observation. Do not fall all the way back
    // to execution A=10, wait for A=21, or fabricate a shared-window clamp.
    assert_eq!(event(&f), (41, 12_000, 12));
    assert!(complete(
        &f,
        second,
        crate::XPresentClockSample {
            source: sample(2, 0).source,
            ust: 14_000,
            msc: 802,
        }
    ));
    assert_eq!(event(&f), (next.raw() as u32, 14_000, 12));
    assert!(f._channels.protocol.try_recv().is_err());
}

#[test]
fn completion_history_is_bounded_and_duplicate_or_rejected_samples_do_not_evict_it() {
    for newer_samples in [3, 4] {
        let f = Fixture::new(true);
        subscribe(&f);
        f.schedule(sample(1, 10), 11);
        let transaction = execute(&f, f.preparation);
        let clocks = f.broker.present_clock_router();
        // Four distinct accepted observations fit. A fifth evicts the oldest
        // and intentionally falls back to the immutable execution sample.
        for msc in 15..=15 + newer_samples {
            clocks.observe_source(sample(1, msc)).unwrap();
        }
        for _ in 0..8 {
            clocks
                .observe_source(sample(1, 15 + newer_samples))
                .unwrap();
        }
        clocks.observe_source(sample(1, 9)).unwrap();
        assert!(f._channels.protocol.try_recv().is_err());
        assert!(complete(
            &f,
            transaction,
            crate::XPresentClockSample {
                source: sample(2, 0).source,
                ust: 15_500,
                msc: 900,
            }
        ));
        let expected = if newer_samples == 3 { 15 } else { 10 };
        assert_eq!(event(&f), (41, expected * 1_000, expected));
        assert!(!complete(&f, transaction, sample(2, 901)));
        assert!(f._channels.protocol.try_recv().is_err());
        assert_eq!(clocks.completed_count(), 1);
    }
}

#[test]
fn frozen_sources_keep_their_real_counts_even_when_independent_rates_cross() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let first = execute(&f, f.preparation);
    let (next, second) = schedule_next(
        &f,
        crate::XPresentClockSample {
            source: sample(2, 0).source,
            ust: 11_000,
            msc: 800,
        },
        11,
    );
    assert!(complete(&f, first, sample(1, 20)));
    assert_eq!(event(&f), (41, 20_000, 20));
    assert!(complete(
        &f,
        second,
        crate::XPresentClockSample {
            source: sample(2, 0).source,
            ust: 21_000,
            msc: 802,
        }
    ));
    assert_eq!(event(&f), (next.raw() as u32, 21_000, 12));
    // XLibre also freezes each vblank's offset. Cross-source MSC monotonicity
    // is not promised; rewriting 12 to 20 would invent a B observation.
}

#[test]
fn newer_request_with_earlier_target_executes_and_completes_first() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 20);
    let (next, first_execution) = schedule_next(&f, sample(1, 10), 11);
    assert!(complete(&f, first_execution, sample(1, 11)));
    assert_eq!(event(&f), (next.raw() as u32, 11_000, 11));
    f.broker
        .present_clock_router()
        .observe_source(sample(1, 19))
        .unwrap();
    let second_execution = execute(&f, f.preparation);
    assert!(complete(&f, second_execution, sample(1, 20)));
    assert_eq!(event(&f), (41, 20_000, 20));
    assert!(first_execution < second_execution);
    // Serial values are returned unchanged; neither serial nor arrival order
    // of the original wire request is a completion ordering key.
}

#[test]
fn loss_and_invalid_samples_never_grant_permission_but_keep_a_retirement_fallback() {
    for invalid in [false, true] {
        let f = Fixture::new(true);
        subscribe(&f);
        f.schedule(sample(1, 10), 11);
        let transaction = execute(&f, f.preparation);
        let clocks = f.broker.present_clock_router();
        clocks.observe_source(sample(1, 15)).unwrap();
        if invalid {
            for _ in 0..3 {
                clocks.observe_source(sample(1, 9)).unwrap();
            }
        } else {
            clocks.lose_source(sample(1, 0).source).unwrap();
        }
        assert!(f._channels.protocol.try_recv().is_err());
        assert!(clocks.bound_sources().unwrap().is_empty());
        assert!(complete(&f, transaction, sample(2, 800)));
        assert_eq!(event(&f), (41, 15_000, 15));
        clocks.observe_source(sample(1, 16)).unwrap();
        clocks.lose_source(sample(1, 0).source).unwrap();
        assert!(!complete(&f, transaction, sample(2, 801)));
        assert!(f._channels.protocol.try_recv().is_err());
        assert_eq!(
            f.broker
                .registry
                .pending_presentations
                .rejected_clock_samples
                .load(Ordering::Relaxed),
            if invalid { 3 } else { 0 }
        );
        assert!(
            f.broker
                .registry
                .pending_presentations
                .entries
                .lock()
                .unwrap()
                .contains_key(&transaction)
        );
    }
}

#[test]
fn lost_owner_final_flip_uses_last_accepted_clock_without_reviving_binding() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let transaction = execute(&f, f.preparation);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 15)).unwrap();
    // Session's first None-owner pass explicitly loses this source. Physical
    // disposal can still retain an in-flight buffer and deliver retirement.
    clocks.lose_source(sample(1, 0).source).unwrap();
    assert!(clocks.bound_sources().unwrap().is_empty());
    clocks.observe_source(sample(1, 18)).unwrap();
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(complete(&f, transaction, sample(1, 19)));
    assert_eq!(event(&f), (41, 15_000, 15));
    assert!(clocks.bound_sources().unwrap().is_empty());
    clocks.observe_source(sample(1, 20)).unwrap();
    clocks.lose_source(sample(1, 0).source).unwrap();
    assert!(!complete(&f, transaction, sample(1, 21)));
    assert!(f._channels.protocol.try_recv().is_err());
    assert_eq!(clocks.completed_count(), 1);
    // Complete did not invent release of the retiring owner's buffer.
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
    assert!(
        !f.broker
            .protocol_router()
            .route_present_idle(transaction)
            .unwrap()
    );
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(
        f.broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn loss_freezes_the_execution_only_window_anchor_before_rebinding() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let old = execute(&f, f.preparation);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 15)).unwrap();
    clocks.lose_source(sample(1, 0).source).unwrap();
    clocks.observe_source(sample(1, 18)).unwrap();
    // Nothing remains queued on A. Its last accepted execution observation
    // still has to anchor this window's new source at window MSC 15.
    let (next, replacement) = schedule_next(&f, sample(2, 800), 16);
    assert!(complete(&f, old, sample(1, 19)));
    assert_eq!(event(&f), (41, 15_000, 15));
    assert!(complete(&f, replacement, sample(2, 801)));
    assert_eq!(event(&f), (next.raw() as u32, 801_000, 16));
    assert!(clocks.bound_sources().unwrap().is_empty());
}

#[test]
fn a_late_previous_sample_cannot_advance_a_lost_rebind_anchor() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let old = execute(&f, f.preparation);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 15)).unwrap();
    clocks.lose_source(sample(1, 0).source).unwrap();
    let next = f.prepare_next(PIXMAP, false);
    f.state
        .runtime
        .lock()
        .unwrap()
        .schedule_prepared_present(
            next,
            crate::XPresentMscTiming::new(16, 0, 0, false).unwrap(),
            sample(2, 800),
            Some(sample(1, 19)),
        )
        .unwrap();
    let replacement = execute(&f, next);
    assert!(complete(&f, old, sample(1, 19)));
    assert_eq!(event(&f), (41, 15_000, 15));
    assert!(complete(&f, replacement, sample(2, 801)));
    assert_eq!(event(&f), (next.raw() as u32, 801_000, 16));
}

#[test]
fn losing_an_old_binding_does_not_rewrite_an_already_rebound_window() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let old = execute(&f, f.preparation);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 15)).unwrap();
    let (_, rebound) = schedule_next(&f, sample(2, 800), 16);
    clocks.observe_source(sample(2, 802)).unwrap();
    clocks.observe_source(sample(1, 19)).unwrap();
    clocks.lose_source(sample(1, 0).source).unwrap();
    let (next, current) = schedule_next(&f, sample(2, 802), 18);
    assert!(complete(&f, old, sample(1, 20)));
    assert_eq!(event(&f), (41, 19_000, 19));
    // A had already been replaced before loss. Independent source rates may
    // regress across bindings; loss must not rewrite B's frozen offset.
    assert!(complete(&f, current, sample(2, 803)));
    assert_eq!(event(&f), (next.raw() as u32, 803_000, 18));
    assert!(complete(&f, rebound, sample(2, 801)));
}

#[test]
fn irrelevant_and_rebound_observations_skip_runtime_but_keep_feedback_history() {
    let f = Fixture::new(true);
    subscribe(&f);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(2, 700)).unwrap();
    assert_eq!(clocks.observation_counts(), (1, 0));
    f.schedule(sample(1, 10), 11);
    let old = execute(&f, f.preparation);
    clocks.observe_source(sample(1, 15)).unwrap();
    assert_eq!(clocks.observation_counts(), (2, 1));
    let (_, rebound) = schedule_next(&f, sample(2, 800), 16);
    std::thread::scope(|scope| {
        let runtime = f.state.runtime.lock().unwrap();
        let (done, received) = std::sync::mpsc::channel();
        let clocks = &clocks;
        scope.spawn(move || {
            clocks.observe_source(sample(1, 19)).unwrap();
            done.send(()).unwrap();
        });
        let without_runtime = received.recv_timeout(Duration::from_secs(1));
        // Always unlock before asserting, so a regression terminates too.
        drop(runtime);
        assert!(
            without_runtime.is_ok(),
            "old-source observation waited on runtime"
        );
    });
    assert_eq!(clocks.observation_counts(), (3, 1));
    assert!(complete(&f, old, sample(3, 900)));
    assert_eq!(event(&f), (41, 19_000, 19));
    clocks.observe_source(sample(2, 801)).unwrap();
    assert_eq!(clocks.observation_counts(), (4, 2));
    assert!(complete(&f, rebound, sample(2, 801)));
    clocks.observe_source(sample(2, 802)).unwrap();
    assert_eq!(clocks.observation_counts(), (5, 2));
    Fixture::destroy(&f.state, &f.broker.registry);
    assert!(
        f.broker
            .registry
            .present_clock_interests
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn clock_interest_is_published_before_admission_releases_runtime() {
    let f = Fixture::new(true);
    subscribe(&f);
    let clocks = f.broker.present_clock_router();
    // An observation before admission has no dependency. The next one must
    // see published interest even if the admitting thread still owns runtime.
    clocks.observe_source(sample(1, 19)).unwrap();
    assert_eq!(clocks.observation_counts(), (1, 0));
    std::thread::scope(|scope| {
        let mut runtime = f.state.runtime.lock().unwrap();
        runtime
            .schedule_prepared_present(
                f.preparation,
                crate::XPresentMscTiming::new(20, 0, 0, false).unwrap(),
                sample(1, 10),
                None,
            )
            .unwrap();
        let (started, observing) = std::sync::mpsc::channel();
        let (done, received) = std::sync::mpsc::channel();
        let clocks = &clocks;
        scope.spawn(move || {
            started.send(()).unwrap();
            clocks.observe_source(sample(1, 19)).unwrap();
            done.send(()).unwrap();
        });
        observing.recv_timeout(Duration::from_secs(1)).unwrap();
        let premature = received.recv_timeout(Duration::from_millis(20));
        drop(runtime);
        assert!(
            matches!(premature, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
            "live queued dependency used the feedback-only path"
        );
        received.recv_timeout(Duration::from_secs(1)).unwrap();
    });
    assert_eq!(clocks.observation_counts(), (2, 1));
    assert!(
        f.state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_is_ready(f.preparation)
    );
    let transaction = execute(&f, f.preparation);
    assert!(complete(&f, transaction, sample(1, 20)));
    assert_eq!(event(&f), (41, 20_000, 20));
}

#[test]
fn fake_bound_retirement_completes_without_waiting_for_frontend_service() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(crate::XPresentClockSample::background(1_000_000), 2);
    f.broker
        .present_clock_router()
        .observe_source(crate::XPresentClockSample::background(2_000_000))
        .unwrap();
    let transaction = execute(&f, f.preparation);
    assert!(complete(
        &f,
        transaction,
        crate::XPresentClockSample {
            source: sample(2, 0).source,
            ust: 2_100_000,
            msc: 800,
        }
    ));
    assert_eq!(event(&f), (41, 2_000_000, 2));
    service_timed_presents(
        &f.state,
        &f.broker.registry,
        &mut XGeneratedEgress::default(),
        || 3_000_000,
    )
    .unwrap();
    assert!(f._channels.protocol.try_recv().is_err());
}

#[test]
fn racing_retirements_and_clock_observers_deliver_one_completion() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let transaction = execute(&f, f.preparation);
    let feedback = f.broker.protocol_router();
    let clocks = f.broker.present_clock_router();
    let start = std::sync::Barrier::new(3);
    let count = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            start.wait();
            feedback
                .route_clocked_present_complete(
                    transaction,
                    sample(1, 11),
                    XPresentCompletionMode::Copy,
                    None,
                )
                .unwrap()
                .routed
        });
        let b = scope.spawn(|| {
            start.wait();
            feedback
                .route_clocked_present_complete(
                    transaction,
                    sample(1, 11),
                    XPresentCompletionMode::Copy,
                    None,
                )
                .unwrap()
                .routed
        });
        start.wait();
        clocks.observe_source(sample(1, 20)).unwrap();
        usize::from(a.join().unwrap()) + usize::from(b.join().unwrap())
    });
    assert_eq!(count, 1);
    assert_eq!(event(&f), (41, 11_000, 11));
    assert!(f._channels.protocol.try_recv().is_err());
}

#[test]
fn destroy_cancels_executed_feedback_and_late_clocks_cannot_revive_it() {
    let f = Fixture::new(true);
    subscribe(&f);
    f.schedule(sample(1, 10), 11);
    let transaction = execute(&f, f.preparation);
    Fixture::destroy(&f.state, &f.broker.registry);
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(sample(1, 20)).unwrap();
    clocks.lose_source(sample(1, 0).source).unwrap();
    assert!(!complete(&f, transaction, sample(2, 800)));
    assert!(
        !f.broker
            .protocol_router()
            .route_present_idle(transaction)
            .unwrap()
    );
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(
        f.broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
}

#[path = "timed_present_native_evidence.rs"]
mod native_evidence;
