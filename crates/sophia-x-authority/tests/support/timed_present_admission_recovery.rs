use super::*;

fn bind_at(
    f: &Fixture,
    request: TransactionId,
    sample: crate::XPresentClockSample,
    now: u64,
) -> bool {
    f.broker
        .present_clock_router()
        .bind_admission_with_time(request, sample, None, || now)
        .unwrap()
}

fn request_pixmap_clock(f: &Fixture, id: TransactionId, target: u64) {
    f.state
        .runtime
        .lock()
        .unwrap()
        .request_prepared_present_clock(
            id,
            crate::XPresentMscTiming::new(target, 0, 0, false).unwrap(),
        )
        .unwrap();
}

#[test]
fn stale_hardware_binding_retries_fake_and_completes_without_a_frontend_error() {
    let f = fixture();
    let seed = prepare_notify(&f, 120, 10, 0, 0);
    assert!(
        f.broker
            .present_clock_router()
            .bind_admission_with_time(seed, hardware(10), None, || panic!(
                "successful admission must not read a fallback clock"
            ))
            .unwrap()
    );
    assert!(turn(&f, &mut XGeneratedEgress::default(), 10_000));
    assert_eq!(notification(&f).0, 120);
    let request = prepare_notify(&f, 121, 12, 0, 0);
    // Equal UST, regressing sequence: this cannot be replaced by the
    // newer-UST race guard. The actual binding reducer must reject it.
    let stale = crate::XPresentClockSample {
        msc: 9,
        ..hardware(10)
    };
    assert!(bind_at(&f, request, stale, 50_100_000));
    assert!(!bind_at(&f, request, stale, 50_100_000)); // once only
    assert_eq!(
        f.broker.present_clock_router().admission_counts().unwrap(),
        (1, 1, 0, 0, 0)
    );
    assert_eq!(
        f.broker.present_clock_router().bound_sources().unwrap(),
        vec![crate::XPresentClockSource::Fake]
    );
    assert!(!turn(&f, &mut XGeneratedEgress::default(), 51_999_999));
    assert!(turn(&f, &mut XGeneratedEgress::default(), 52_000_000));
    assert_eq!(notification(&f), (121, 52_000_000, 12));
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(
        f.broker
            .present_clock_router()
            .bound_sources()
            .unwrap()
            .is_empty()
    );
}

fn rejected_fake(f: &Fixture) {
    let seed = prepare_notify(f, 122, 100, 0, 0);
    assert!(bind_at(
        f,
        seed,
        crate::XPresentClockSample::background(10_000_000),
        10_000_000
    ));
    f.broker
        .present_clock_router()
        .lose_source(crate::XPresentClockSource::Fake)
        .unwrap();
}

#[test]
fn failed_fake_retry_settles_both_kinds_at_the_last_accepted_sample() {
    for pixmap in [false, true] {
        let f = fixture();
        rejected_fake(&f);
        let request = if pixmap {
            let id = f.prepare_next(PIXMAP, false);
            request_pixmap_clock(&f, id, 100);
            id
        } else {
            prepare_notify(&f, 123, 100, 0, 0)
        };
        assert!(bind_at(
            &f,
            request,
            crate::XPresentClockSample::background(50_100_000),
            50_100_000
        ));
        assert!(!bind_at(&f, request, hardware(900), 50_200_000));
        assert_eq!(
            f.broker.present_clock_router().admission_counts().unwrap(),
            (2, 1, 1, 0, 0)
        );
        assert!(
            f.broker
                .present_clock_router()
                .admissions()
                .unwrap()
                .is_empty()
        );
        let mut generated = XGeneratedEgress::default();
        assert!(turn(&f, &mut generated, 50_100_000));
        if pixmap {
            assert!(matches!(
                f._channels.protocol.try_recv().unwrap(),
                XClientEvent::PresentIdleNotify { .. }
            ));
            assert!(matches!(
                f._channels.protocol.try_recv().unwrap(),
                XClientEvent::PresentCompleteNotify {
                    kind: 0,
                    mode: 2,
                    ust: 10_000_000,
                    msc: 10,
                    ..
                }
            ));
            assert_eq!(notification(&f), (122, 10_000_000, 10));
            assert!(
                f.broker
                    .registry
                    .pending_presentations
                    .entries
                    .lock()
                    .unwrap()
                    .is_empty()
            );
        } else {
            assert_eq!(notification(&f), (122, 10_000_000, 10));
            assert_eq!(notification(&f), (123, 10_000_000, 10));
        }
        assert!(!generated.pending());
        assert!(!turn(&f, &mut generated, 60_000_000));
        assert!(f._channels.protocol.try_recv().is_err());
        assert!(
            f.state
                .runtime
                .lock()
                .unwrap()
                .take_cpu_buffer_updates()
                .is_empty()
        );
        // A healthy request still binds and completes after the local refusal.
        let healthy = prepare_notify(&f, 124, 0, 0, 0);
        assert!(bind_at(&f, healthy, hardware(20), 60_000_000));
        assert!(turn(&f, &mut generated, 60_000_000));
        assert_eq!(notification(&f).0, 124);
    }
}

fn replace_with_broken_idle_fence(f: &Fixture, broken: TransactionId) {
    let fence = XResourceId::new(0x400003, 1);
    {
        let mut runtime = f.state.runtime.lock().unwrap();
        runtime.cancel_prepared_standard_pixmap(broken);
        runtime.create_dri3_fence(NS, fence, 1).unwrap();
        // A valid private handle with a missing descriptor produces the real
        // typed signal error, without an unsafe fd or device fault injection.
        runtime
            .prepare_standard_pixmap(
                CLIENT.raw(),
                broken,
                NS,
                WINDOW,
                PIXMAP,
                (0, 0),
                None,
                None,
                crate::XPresentFenceResources {
                    wait: None,
                    idle: Some(fence),
                },
            )
            .unwrap();
    }
}

#[test]
fn idle_signal_failure_settles_that_scrap_and_does_not_strand_later_scraps() {
    let f = fixture();
    let broken = f.prepare_next(PIXMAP, false);
    replace_with_broken_idle_fence(&f, broken);
    request_pixmap_clock(&f, broken, 20);
    assert!(bind_at(&f, broken, hardware(10), 50_000_000));
    let later = f.prepare_next(PIXMAP, true); // partial: does not scrap broken
    request_pixmap_clock(&f, later, 20);
    assert!(bind_at(&f, later, hardware(10), 50_000_000));
    let replacement = f.prepare_next(PIXMAP, false); // scraps both
    request_pixmap_clock(&f, replacement, 20);
    assert!(bind_at(&f, replacement, hardware(10), 50_000_000));
    assert_eq!(
        f.broker.present_clock_router().admission_counts().unwrap(),
        (0, 0, 1, 1, 0)
    );
    let runtime = f.state.runtime.lock().unwrap();
    for id in [broken, later] {
        assert_eq!(
            runtime.prepared_present_fences(id),
            Some(Default::default())
        );
    }
    assert_eq!(runtime.ready_prepared_skips(), vec![(broken, 10_000, 10)]);
    drop(runtime);
    let mut generated = XGeneratedEgress::default();
    assert!(turn(&f, &mut generated, 10_000));
    for _ in 0..2 {
        assert!(matches!(
            f._channels.protocol.try_recv().unwrap(),
            XClientEvent::PresentIdleNotify { .. }
        ));
    }
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentCompleteNotify {
            kind: 0,
            mode: 2,
            ust: 10_000,
            msc: 10,
            ..
        }
    ));
    assert!(f._channels.protocol.try_recv().is_err());
    assert_eq!(
        f.broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .len(),
        2
    );
    // The healthy older scrap still waits for target 20. The new pixmap is
    // intact and reaches its hardware preparation lead at 19.
    f.broker
        .present_clock_router()
        .observe_source(hardware(19))
        .unwrap();
    assert_eq!(
        f.state.runtime.lock().unwrap().ready_prepared_presents(),
        vec![replacement]
    );
    assert!(turn(&f, &mut generated, 19_000));
    assert!(generated.pending());
    f.broker
        .present_clock_router()
        .observe_source(hardware(20))
        .unwrap();
    assert!(turn(&f, &mut generated, 20_000));
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentCompleteNotify {
            kind: 0,
            mode: 2,
            msc: 20,
            ..
        }
    ));
    assert!(f._channels.protocol.try_recv().is_err());
}

#[test]
fn destruction_cancels_terminal_fallbacks_before_any_feedback() {
    let f = fixture();
    rejected_fake(&f);
    let notify = prepare_notify(&f, 125, 100, 0, 0);
    let pixmap = f.prepare_next(PIXMAP, false);
    request_pixmap_clock(&f, pixmap, 100);
    for request in [notify, pixmap] {
        assert!(bind_at(
            &f,
            request,
            crate::XPresentClockSample::background(50_000_000),
            50_000_000
        ));
    }
    Fixture::destroy(&f.state, &f.broker.registry);
    assert!(!turn(&f, &mut XGeneratedEgress::default(), 60_000_000));
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(
        f.broker
            .present_clock_router()
            .admissions()
            .unwrap()
            .is_empty()
    );
    assert!(
        f.broker
            .present_clock_router()
            .bound_sources()
            .unwrap()
            .is_empty()
    );
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
fn missing_scrap_sample_uses_window_then_anchor_then_fake_and_unblocks_the_client() {
    for (window, anchor, expected) in [
        (true, true, Some((12_000, 12))),
        (false, true, Some((10_000, 10))),
        (false, false, None),
    ] {
        let f = fixture();
        let broken = f.prepare_next(PIXMAP, false);
        replace_with_broken_idle_fence(&f, broken);
        request_pixmap_clock(&f, broken, 20);
        assert!(bind_at(&f, broken, hardware(10), 50_000_000));
        f.broker
            .present_clock_router()
            .observe_source(hardware(12))
            .unwrap();
        {
            let mut runtime = f.state.runtime.lock().unwrap();
            // Catch inside the lock: debug builds diagnose the broken
            // invariant after containment, without poisoning the fixture.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runtime.scrap_with_missing_sample_for_test(broken, window, anchor)
            }));
            assert_eq!(result.is_err(), cfg!(debug_assertions));
            assert_eq!(
                runtime.prepared_present_fences(broken),
                Some(Default::default())
            );
            assert_eq!(
                runtime.ready_prepared_skips(),
                expected
                    .into_iter()
                    .map(|(ust, msc)| (broken, ust, msc))
                    .collect::<Vec<_>>()
            );
            assert_eq!(runtime.prepared_present_idle_deliveries(), vec![broken]);
            assert!(
                runtime
                    .request_prepared_present_clock(
                        broken,
                        crate::XPresentMscTiming::new(100, 0, 0, false).unwrap()
                    )
                    .is_err()
            );
            assert!(runtime.present_clock_admissions().is_empty());
        }
        assert_eq!(
            f.broker.present_clock_router().admission_counts().unwrap(),
            (0, 0, u64::from(expected.is_some()), 1, 1)
        );
        let healthy = prepare_notify(&f, 140, 0, 0, 0);
        assert!(bind_at(&f, healthy, hardware(20), 50_000_000));
        let mut generated = XGeneratedEgress::default();
        let now = 50_100_000;
        assert!(turn(&f, &mut generated, now));
        let (ust, msc) = expected.unwrap_or((now, 50));
        assert!(matches!(
            f._channels.protocol.try_recv().unwrap(),
            XClientEvent::PresentIdleNotify { .. }
        ));
        assert!(matches!(f._channels.protocol.try_recv().unwrap(),
            XClientEvent::PresentCompleteNotify { kind: 0, mode: 2,
                ust: actual_ust, msc: actual_msc, .. }
                if actual_ust == ust && actual_msc == msc));
        assert_eq!(
            f.broker.present_clock_router().admission_counts().unwrap(),
            (0, 0, 1, 1, 1)
        );
        assert_eq!(notification(&f).0, 140);
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
        assert_eq!(
            f.state
                .runtime
                .lock()
                .unwrap()
                .prepared_present_fences(broken),
            None
        );
        assert!(!generated.pending());
        assert!(!turn(&f, &mut generated, 60_000_000));
        assert!(f._channels.protocol.try_recv().is_err());
        // The exact socket reservation is free for the next Pixmap too.
        let next = f.prepare_next(PIXMAP, false);
        assert!(
            f.broker
                .registry
                .pending_presentations
                .entries
                .lock()
                .unwrap()
                .contains_key(&next)
        );
    }
}

#[test]
fn destruction_cancels_a_scrap_waiting_for_its_terminal_fake_pair() {
    let f = fixture();
    let broken = f.prepare_next(PIXMAP, false);
    replace_with_broken_idle_fence(&f, broken);
    request_pixmap_clock(&f, broken, 20);
    assert!(bind_at(&f, broken, hardware(10), 50_000_000));
    {
        let mut runtime = f.state.runtime.lock().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.scrap_with_missing_sample_for_test(broken, false, false)
        }));
        assert_eq!(result.is_err(), cfg!(debug_assertions));
        assert_eq!(runtime.prepared_present_idle_deliveries(), vec![broken]);
        assert!(runtime.ready_prepared_skips().is_empty());
    }
    Fixture::destroy(&f.state, &f.broker.registry);
    assert!(!turn(&f, &mut XGeneratedEgress::default(), 60_000_000));
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
    assert_eq!(
        f.broker.present_clock_router().admission_counts().unwrap(),
        (0, 0, 0, 1, 1)
    );
}

#[test]
fn notify_refusal_keeps_the_window_offset_instead_of_reporting_raw_fake_msc() {
    let f = fixture();
    let clocks = f.broker.present_clock_router();
    let mut generated = XGeneratedEgress::default();
    let seed = prepare_notify(&f, 150, 0, 0, 0);
    assert!(bind_at(
        &f,
        seed,
        crate::XPresentClockSample {
            msc: 1_000_000,
            ..hardware(10)
        },
        10_000
    ));
    assert!(turn(&f, &mut generated, 10_000));
    assert_eq!(notification(&f), (150, 10_000, 1_000_000));

    // Switching to Fake preserves the high window MSC with a nonzero
    // offset. Progress on Fake then advances the window by two fields.
    let switch = prepare_notify(&f, 151, 1_000_010, 0, 0);
    assert!(bind_at(
        &f,
        switch,
        crate::XPresentClockSample::background(10_000_000),
        10_000_000
    ));
    clocks
        .observe_source(crate::XPresentClockSample::background(12_000_000))
        .unwrap();
    clocks
        .lose_source(crate::XPresentClockSource::Fake)
        .unwrap();
    let refused = prepare_notify(&f, 152, 1_000_100, 0, 0);
    let raw_fake = crate::XPresentClockSample::background(50_100_000);
    assert!(bind_at(&f, refused, raw_fake, raw_fake.ust));
    assert!(!bind_at(&f, refused, raw_fake, raw_fake.ust));
    assert_eq!(clocks.admission_counts().unwrap(), (2, 1, 1, 0, 0));
    assert!(turn(&f, &mut generated, raw_fake.ust));
    assert_eq!(notification(&f), (151, 12_000_000, 1_000_002));
    assert_eq!(notification(&f), (152, 12_000_000, 1_000_002));
    assert!(f._channels.protocol.try_recv().is_err()); // no Idle
    assert!(clocks.admissions().unwrap().is_empty());
    assert!(clocks.bound_sources().unwrap().is_empty());
    assert_eq!(
        f.state.runtime.lock().unwrap().prepared_msc_notify_count(),
        0
    );
    assert!(!generated.pending());
    assert!(!turn(&f, &mut generated, 60_000_000));
    assert!(f._channels.protocol.try_recv().is_err());

    // The refusal did not reset the window's continuity anchor either.
    let healthy = prepare_notify(&f, 153, 0, 0, 0);
    assert!(bind_at(&f, healthy, hardware(20), 60_000_000));
    assert!(turn(&f, &mut generated, 60_000_000));
    assert_eq!(notification(&f), (153, 20_000, 1_000_002));
}

#[test]
fn clockless_notify_refusal_uses_fake_once_without_creating_a_window_clock() {
    let f = fixture();
    let request = prepare_notify(&f, 154, 100, 0, 0);
    let fake = crate::XPresentClockSample::background(50_100_000);
    {
        let mut runtime = f.state.runtime.lock().unwrap();
        // Inject a terminal admission refusal before this window has ever
        // acquired a clock. Ordinary first binding does not fail this way.
        assert!(runtime.refuse_prepared_admission_for_test(request, fake));
        assert!(!runtime.refuse_prepared_admission_for_test(request, fake));
    }
    let mut generated = XGeneratedEgress::default();
    assert!(turn(&f, &mut generated, fake.ust));
    assert_eq!(notification(&f), (154, fake.ust, fake.msc));
    assert!(f._channels.protocol.try_recv().is_err()); // no Idle
    assert_eq!(
        f.state.runtime.lock().unwrap().prepared_msc_notify_count(),
        0
    );
    assert!(!turn(&f, &mut generated, 60_000_000));
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(!generated.pending());
    assert_eq!(
        f.broker.present_clock_router().admission_counts().unwrap(),
        (0, 0, 1, 0, 0)
    );
    let healthy = prepare_notify(&f, 155, 0, 0, 0);
    assert!(bind_at(&f, healthy, hardware(20), 60_000_000));
    assert!(turn(&f, &mut generated, 60_000_000));
    assert_eq!(notification(&f), (155, 20_000, 20));
}
