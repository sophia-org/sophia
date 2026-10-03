//! Actual runtime and feedback paths with injected time, no native hardware.
use super::*;

fn sample(ust: u64) -> crate::XPresentClockSample {
    crate::XPresentClockSample {
        source: crate::XPresentClockSource::Unclocked {
            domain: 7,
            incarnation: 3,
            minimum_period_usec: 16_667,
        },
        ust,
        msc: 0,
    }
}

fn bind(f: &Fixture, request: TransactionId, sample: crate::XPresentClockSample) -> bool {
    f.broker
        .present_clock_router()
        .bind_admission_with_time(request, sample, None, || sample.ust)
        .unwrap()
}

#[test]
fn unclocked_notify_wait_starts_at_binding_after_a_delayed_session_observation() {
    let f = fixture();
    let request = prepare_notify(&f, 90, 0, 0, 0);
    assert!(
        f.broker
            .present_clock_router()
            .bind_admission_with_time(request, sample(10), None, || 200_000)
            .unwrap()
    );
    let mut generated = XGeneratedEgress::default();
    assert!(!turn(&f, &mut generated, 216_666));
    assert!(turn(&f, &mut generated, 216_667));
    assert_eq!(notification(&f), (90, 216_667, 0));
}

#[test]
fn unclocked_notify_waits_without_spinning_then_settles_once_without_idle() {
    let f = fixture();
    let request = prepare_notify(&f, 91, 10_000, 0, 0);
    assert!(bind(&f, request, sample(100_000)));
    let mut generated = XGeneratedEgress::default();
    let origin = Instant::now();
    assert_eq!(
        timed_present_service_deadline(&f.state, 100_000, origin, false, true).unwrap(),
        Some(origin + Duration::from_micros(16_667))
    );
    for now in [100_000, 100_001, 110_000, 116_666] {
        assert!(!turn(&f, &mut generated, now));
        assert!(f._channels.protocol.try_recv().is_err());
    }
    assert!(turn(&f, &mut generated, 116_667));
    assert_eq!(notification(&f), (91, 116_667, 0));
    assert!(!turn(&f, &mut generated, 200_000));
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(!generated.pending());
    assert!(!f.state.present_service_demand.load(Ordering::Acquire));
    assert_eq!(
        f.broker
            .present_clock_router()
            .wire_timing_statistics()
            .unwrap()
            .unclocked_notify_settled,
        1
    );
    assert_eq!(
        timed_present_service_deadline(&f.state, 200_000, origin, false, true).unwrap(),
        None
    );
}

#[test]
fn unclocked_pixmap_completes_at_retirement_with_plateau_and_independent_idle() {
    for (target, lost) in [0, 1, 1_000_000]
        .into_iter()
        .flat_map(|target| [(target, false), (target, true)])
    {
        let f = fixture();
        let request = f.prepare_next(PIXMAP, false);
        f.state
            .runtime
            .lock()
            .unwrap()
            .request_prepared_present_clock(
                request,
                crate::XPresentMscTiming::new(target, 0, 0, false).unwrap(),
            )
            .unwrap();
        assert!(bind(&f, request, sample(100_000)));
        let mut generated = XGeneratedEgress::default();
        assert!(turn(&f, &mut generated, 100_000));
        let transaction = generated.take().next().unwrap().transaction;
        assert!(f._channels.protocol.try_recv().is_err());
        if lost {
            f.broker
                .present_clock_router()
                .lose_source(sample(0).source)
                .unwrap();
        }
        let router = f.broker.protocol_router();
        let (outcome, reported) = router
            .route_present_complete_with_evidence(
                transaction,
                (116_667, 999),
                std::iter::empty(),
                XPresentCompletionMode::Copy,
                None,
            )
            .unwrap();
        assert!(outcome.routed);
        assert_eq!(reported, Some((116_667, 0)));
        assert!(matches!(
            f._channels.protocol.try_recv().unwrap(),
            XClientEvent::PresentCompleteNotify {
                ust: 116_667,
                msc: 0,
                kind: 0,
                ..
            }
        ));
        assert!(f._channels.protocol.try_recv().is_err());
        assert!(
            !router
                .route_present_complete_with_evidence(
                    transaction,
                    (133_334, 1000),
                    std::iter::empty(),
                    XPresentCompletionMode::Copy,
                    None
                )
                .unwrap()
                .0
                .routed
        );
        assert!(router.route_present_idle(transaction).unwrap());
        assert!(matches!(
            f._channels.protocol.try_recv().unwrap(),
            XClientEvent::PresentIdleNotify { .. }
        ));
        assert!(!router.route_present_idle(transaction).unwrap());
        assert!(!turn(&f, &mut generated, 200_000));
        assert!(!f.state.present_service_demand.load(Ordering::Acquire));
    }
}

#[test]
fn unclocked_notify_destroy_before_deadline_cancels_without_an_event() {
    let f = fixture();
    let request = prepare_notify(&f, 92, 0, 0, 0);
    assert!(bind(&f, request, sample(100_000)));
    Fixture::destroy(&f.state, &f.broker.registry);
    let mut generated = XGeneratedEgress::default();
    assert!(!turn(&f, &mut generated, 200_000));
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(
        f.broker
            .present_clock_router()
            .bound_sources()
            .unwrap()
            .is_empty()
    );
}

fn fenced_fixture() -> (Fixture, Arc<std::os::fd::OwnedFd>) {
    let f = Fixture::new(true);
    let fence = XResourceId::new(0x400003, 1);
    let fd = Arc::new(sophia_xshmfence::allocate().unwrap());
    {
        let mut runtime = f.state.runtime.lock().unwrap();
        runtime.cancel_prepared_standard_pixmap(f.preparation);
        runtime.create_dri3_fence(NS, fence, 1).unwrap();
        let handle = runtime.dri3_fence_handle(NS, fence).unwrap();
        runtime
            .retain_present_fence_descriptor(handle, fd.clone())
            .unwrap();
        runtime
            .prepare_standard_pixmap(
                CLIENT.raw(),
                f.preparation,
                NS,
                WINDOW,
                PIXMAP,
                (0, 0),
                None,
                None,
                crate::XPresentFenceResources {
                    wait: Some(fence),
                    idle: None,
                },
            )
            .unwrap();
    }
    (f, fd)
}

#[test]
fn unclocked_pixmap_still_waits_for_its_acquire_fence() {
    let (f, fd) = fenced_fixture();
    f.schedule(sample(100_000), 1_000_000);
    let mut generated = XGeneratedEgress::default();
    for now in [100_000, 132_000, 164_000] {
        assert!(!turn(&f, &mut generated, now));
        assert!(!generated.pending());
        assert!(f._channels.protocol.try_recv().is_err());
    }
    sophia_xshmfence::trigger(&fd).unwrap();
    assert!(turn(&f, &mut generated, 200_000));
    assert_eq!(generated.take().count(), 1);
    assert!(!turn(&f, &mut generated, 232_000));
}

#[test]
fn unclocked_source_loss_settles_notify_once_without_reviving_the_source() {
    let f = fixture();
    let request = prepare_notify(&f, 93, 1_000_000, 0, 0);
    assert!(bind(&f, request, sample(100_000)));
    f.broker
        .present_clock_router()
        .lose_source(sample(0).source)
        .unwrap();
    let mut generated = XGeneratedEgress::default();
    assert!(turn(&f, &mut generated, 100_001));
    assert_eq!(notification(&f), (93, 100_000, 0));
    assert!(!turn(&f, &mut generated, 200_000));
    assert!(f._channels.protocol.try_recv().is_err());
}

#[test]
fn service_clock_is_read_once_under_runtime_after_a_concurrent_bind() {
    // Model a bind at 200_000 between a pre-lock service time of 150_000
    // and the acquired runtime. Moving clock() before lock deterministically
    // returns the stale value, then three collisions lose the binding.
    for initial in [
        sample(200_000),
        crate::XPresentClockSample::background(200_000),
    ] {
        let f = fixture();
        let request = prepare_notify(&f, 94, 1_000_000, 0, 0);
        assert!(bind(&f, request, initial));
        let reads = std::cell::Cell::new(0);
        let mut generated = XGeneratedEgress::default();
        let mut progressed = false;
        for _ in 0..3 {
            let clock = || {
                reads.set(reads.get() + 1);
                if f.state.runtime.try_lock().is_ok() {
                    150_000
                } else {
                    200_000
                }
            };
            progressed |=
                service_timed_presents(&f.state, &f.broker.registry, &mut generated, clock)
                    .unwrap();
        }
        assert_eq!(reads.get(), 3);
        let statistics = f.state.runtime.lock().unwrap().present_timing_statistics();
        assert_eq!(statistics.clock_stale_samples, 0);
        assert_eq!(statistics.clock_sources_lost, 0);
        assert!(!progressed);
        let next = prepare_notify(&f, 95, 1_000_000, 0, 0);
        assert!(bind(
            &f,
            next,
            crate::XPresentClockSample {
                ust: 200_001,
                ..initial
            }
        ));
        assert_eq!(
            f.broker.present_clock_router().bound_sources().unwrap(),
            [initial.source]
        );
        assert_eq!(
            f.state
                .runtime
                .lock()
                .unwrap()
                .present_timing_statistics()
                .admission_fake_retries,
            0
        );
        assert!(f._channels.protocol.try_recv().is_err());
    }
}

#[test]
fn queued_unclocked_fence_loss_releases_pixels_and_reports_skip_idle_once() {
    let (f, fd) = fenced_fixture();
    f.broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    f.schedule(sample(100_000), 1_000_000);
    {
        let mut runtime = f.state.runtime.lock().unwrap();
        runtime.free_pixmap(NS, PIXMAP).unwrap();
        assert_eq!(runtime.retained_pixmap_count(), 1);
    }
    let mut generated = XGeneratedEgress::default();
    assert!(!turn(&f, &mut generated, 100_000));
    f.broker
        .present_clock_router()
        .lose_source(sample(0).source)
        .unwrap();
    assert!(turn(&f, &mut generated, 100_001));
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentIdleNotify { .. }
    ));
    assert!(
        matches!(f._channels.protocol.try_recv().unwrap(), XClientEvent::PresentCompleteNotify { mode, msc: 0, .. } if mode == XPresentCompletionMode::Skip as u8)
    );
    assert!(f._channels.protocol.try_recv().is_err());
    {
        let runtime = f.state.runtime.lock().unwrap();
        assert_eq!(runtime.prepared_present_count(), 0);
        assert_eq!(runtime.retained_pixmap_count(), 0);
    }
    assert!(
        f.broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
    sophia_xshmfence::trigger(&fd).unwrap();
    assert!(!turn(&f, &mut generated, 200_000));
    assert!(!generated.pending());
    assert!(f._channels.protocol.try_recv().is_err());
}

#[test]
fn unclocked_full_updates_do_not_scrap_a_fence_blocked_request() {
    let (f, fd) = fenced_fixture();
    f.broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    f.schedule(sample(100_000), 1_000_000);
    let next = f.prepare_next(PIXMAP, false);
    {
        let mut runtime = f.state.runtime.lock().unwrap();
        let superseded = runtime
            .schedule_prepared_present(
                next,
                crate::XPresentMscTiming::new(1_000_000, 0, 0, false).unwrap(),
                sample(100_000),
                None,
            )
            .unwrap();
        assert!(superseded.is_empty());
        assert_eq!(runtime.prepared_present_count(), 2);
    }
    let mut generated = XGeneratedEgress::default();
    assert!(turn(&f, &mut generated, 100_000)); // Later unfenced request can execute.
    let second = generated.take().next().unwrap().transaction;
    assert_eq!(f.state.runtime.lock().unwrap().prepared_present_count(), 1);
    assert!(f._channels.protocol.try_recv().is_err()); // No premature scrap or Idle.
    sophia_xshmfence::trigger(&fd).unwrap();
    assert!(turn(&f, &mut generated, 132_000));
    let first = generated.take().next().unwrap().transaction;
    for (transaction, serial, ust) in [(second, next.raw() as u32, 133_000), (first, 41, 150_000)] {
        let router = f.broker.protocol_router();
        let result = router
            .route_present_complete_with_evidence(
                transaction,
                (ust, 999),
                std::iter::empty(),
                XPresentCompletionMode::Copy,
                None,
            )
            .unwrap();
        assert_eq!(result.1, Some((ust, 0)));
        assert!(
            matches!(f._channels.protocol.try_recv().unwrap(), XClientEvent::PresentCompleteNotify { serial: actual, mode: 0, .. } if actual == serial)
        );
        assert!(router.route_present_idle(transaction).unwrap());
        assert!(
            matches!(f._channels.protocol.try_recv().unwrap(), XClientEvent::PresentIdleNotify { serial: actual, .. } if actual == serial)
        );
        assert!(!router.route_present_idle(transaction).unwrap());
    }
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
