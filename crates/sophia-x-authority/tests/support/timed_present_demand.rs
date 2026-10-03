//! Idle service must not contend on the authority runtime; live obligations
//! must still wake and progress after an empty visit or a concurrent clear.
use super::*;

fn turn(f: &Fixture, now: u64) -> bool {
    service_timed_presents(
        &f.state,
        &f.broker.registry,
        &mut XGeneratedEgress::default(),
        || now,
    )
    .unwrap()
}

fn empty_fixture() -> Fixture {
    let f = Fixture::new(true);
    f.state
        .runtime
        .lock()
        .unwrap()
        .cancel_prepared_standard_pixmap(f.preparation);
    f.broker.registry.cancel_present(f.preparation).unwrap();
    assert!(!turn(&f, 1_000_000));
    assert!(!f.state.present_service_demand.load(Ordering::Acquire));
    f
}

fn prepare_notify(f: &Fixture, request: TransactionId) {
    f.state
        .runtime
        .lock()
        .unwrap()
        .prepare_present_msc_notify(
            CLIENT.raw(),
            request,
            NS,
            WINDOW,
            request.raw() as u32,
            crate::XPresentMscTiming::notify(0, 0, 0).unwrap(),
        )
        .unwrap();
}

#[test]
fn idle_timed_service_acquires_no_runtime_lock_and_keeps_transport_deadlines() {
    let state = X11CoreSocketServerState::new();
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(64).unwrap());
    let origin = Instant::now();
    for _ in 0..100 {
        assert!(
            !service_timed_presents(
                &state,
                &broker.registry,
                &mut XGeneratedEgress::default(),
                || 1_000_000
            )
            .unwrap()
        );
        assert_eq!(
            timed_present_service_deadline(&state, 1_000_000, origin, false, true).unwrap(),
            None
        );
        assert_eq!(
            timed_present_service_deadline(&state, 1_000_000, origin, true, false).unwrap(),
            origin.checked_add(Duration::from_millis(1))
        );
    }
    let stats = state.runtime.lock().unwrap().present_timing_statistics();
    assert_eq!(stats.service_runtime_locks, 0);
    assert_eq!(stats.deadline_runtime_locks, 0);
    // A poisoned runtime makes an accidental acquisition fail immediately,
    // rather than hanging this regression while the mutex is deliberately held.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = state.runtime.lock().unwrap();
        panic!("test-only poison");
    }));
    assert!(
        !service_timed_presents(
            &state,
            &broker.registry,
            &mut XGeneratedEgress::default(),
            || 1_000_000
        )
        .unwrap()
    );
    assert_eq!(
        timed_present_service_deadline(&state, 1_000_000, origin, false, true).unwrap(),
        None
    );
}

#[test]
fn timed_work_after_an_empty_visit_is_not_lost_and_clears_after_cancellation() {
    let f = empty_fixture();
    let request = f.state.allocate_transaction().unwrap();
    prepare_notify(&f, request);
    assert!(f.state.present_service_demand.load(Ordering::Acquire));
    // An unbound/unpublished request owes work but no immediate deadline.
    assert!(!turn(&f, 1_000_000));
    assert_eq!(
        timed_present_service_deadline(&f.state, 1_000_000, Instant::now(), false, true).unwrap(),
        None
    );
    assert!(f.state.present_service_demand.load(Ordering::Acquire));
    f.broker
        .present_clock_router()
        .bind_admission(
            request,
            crate::XPresentClockSample::background(1_000_000),
            None,
        )
        .unwrap();
    assert!(turn(&f, 1_000_000));
    assert!(!f.state.present_service_demand.load(Ordering::Acquire));
    let request = f.state.allocate_transaction().unwrap();
    prepare_notify(&f, request);
    f.state
        .runtime
        .lock()
        .unwrap()
        .cancel_prepared_msc_notify(request);
    assert!(!turn(&f, 1_000_000));
    assert!(!f.state.present_service_demand.load(Ordering::Acquire));
}

#[test]
fn concurrent_producer_and_service_clear_never_erase_new_demand() {
    let f = empty_fixture();
    for tick in 1..=32 {
        let old = f.state.allocate_transaction().unwrap();
        prepare_notify(&f, old);
        f.state
            .runtime
            .lock()
            .unwrap()
            .cancel_prepared_msc_notify(old);
        let new = f.state.allocate_transaction().unwrap();
        let start = std::sync::Barrier::new(2);
        let state = &f.state;
        let registry = &f.broker.registry;
        std::thread::scope(|scope| {
            scope.spawn(|| {
                start.wait();
                service_timed_presents(state, registry, &mut XGeneratedEgress::default(), || {
                    tick * 1_000_000
                })
                .unwrap();
            });
            start.wait();
            prepare_notify(&f, new);
        });
        assert!(f.state.present_service_demand.load(Ordering::Acquire));
        f.broker
            .present_clock_router()
            .bind_admission(
                new,
                crate::XPresentClockSample::background(tick * 1_000_000),
                None,
            )
            .unwrap();
        assert!(turn(&f, tick * 1_000_000));
        assert!(!f.state.present_service_demand.load(Ordering::Acquire));
    }
}

#[test]
fn pending_wire_work_has_no_spin_deadline_and_keeps_its_demand() {
    let f = empty_fixture();
    let request = f.state.allocate_transaction().unwrap();
    {
        let mut runtime = f.state.runtime.lock().unwrap();
        runtime
            .prepare_present_msc_notify_with_publication(
                CLIENT.raw(),
                request,
                NS,
                WINDOW,
                41,
                crate::XPresentMscTiming::notify(0, 0, 0).unwrap(),
                crate::runtime::XPresentPublication::PendingWire,
            )
            .unwrap();
    }
    assert!(!turn(&f, 1_000_000));
    assert_eq!(
        timed_present_service_deadline(&f.state, 1_000_000, Instant::now(), false, true).unwrap(),
        None
    );
    assert!(f.state.present_service_demand.load(Ordering::Acquire));
    f.state
        .runtime
        .lock()
        .unwrap()
        .cancel_prepared_msc_notify(request);
    assert!(!turn(&f, 1_000_000));
    assert!(!f.state.present_service_demand.load(Ordering::Acquire));
}

#[test]
fn executed_fake_feedback_keeps_service_alive_after_prepared_queue_empties() {
    let f = Fixture::new(true);
    f.broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    f.schedule(crate::XPresentClockSample::background(1_000_000), 2);
    let mut generated = XGeneratedEgress::default();
    assert!(
        service_timed_presents(&f.state, &f.broker.registry, &mut generated, || 2_000_000).unwrap()
    );
    let execution = generated.take().next().unwrap().transaction;
    assert!(
        !f.state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_service_pending()
    );
    assert!(!turn(&f, 9_000_000));
    assert!(f.state.present_service_demand.load(Ordering::Acquire));
    let observed = f
        .broker
        .registry
        .pending_presentations
        .entries
        .lock()
        .unwrap()
        .get(&execution)
        .unwrap()
        .clock
        .as_ref()
        .unwrap()
        .latest();
    assert_eq!(observed, crate::XPresentClockSample::background(9_000_000));
    assert!(
        f.broker
            .protocol_router()
            .route_clocked_present_complete(
                execution,
                crate::XPresentClockSample {
                    source: crate::XPresentClockSource::Hardware {
                        domain: 1,
                        incarnation: 1
                    },
                    ust: 9_100_000,
                    msc: 800
                },
                XPresentCompletionMode::Copy,
                None
            )
            .unwrap()
            .routed
    );
    assert!(matches!(
        f._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentCompleteNotify {
            ust: 9_000_000,
            msc: 9,
            ..
        }
    ));
    // Idle may still be owed, but Complete has ended fake-clock demand.
    assert!(!turn(&f, 10_000_000));
    assert!(!f.state.present_service_demand.load(Ordering::Acquire));
}
