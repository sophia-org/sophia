//! Runtime/request-clock/router/private-service components, with supplied
//! clocks. Wire admission and native source selection are not enabled here.
use super::*;

#[path = "timed_present_rootless.rs"]
mod rootless;

#[path = "timed_present_admission_recovery.rs"]
mod recovery;

fn hardware(msc: u64) -> crate::XPresentClockSample {
    crate::XPresentClockSample {
        source: crate::XPresentClockSource::Hardware {
            domain: 1,
            incarnation: 1,
        },
        ust: msc * 1_000,
        msc,
    }
}

fn fixture() -> Fixture {
    let f = Fixture::new(true);
    f.state
        .runtime
        .lock()
        .unwrap()
        .cancel_prepared_standard_pixmap(f.preparation);
    f.broker.registry.cancel_present(f.preparation).unwrap();
    f.broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    f
}

fn prepare_notify(
    f: &Fixture,
    serial: u32,
    target: u64,
    divisor: u64,
    remainder: u64,
) -> TransactionId {
    let request = f.state.allocate_transaction().unwrap();
    f.state
        .runtime
        .lock()
        .unwrap()
        .prepare_present_msc_notify(
            CLIENT.raw(),
            request,
            NS,
            WINDOW,
            serial,
            crate::XPresentMscTiming::notify(target, divisor, remainder).unwrap(),
        )
        .unwrap();
    request
}

fn bind(f: &Fixture, request: TransactionId, sample: crate::XPresentClockSample) -> bool {
    f.broker
        .present_clock_router()
        .bind_admission(request, sample, None)
        .unwrap()
}

fn turn(f: &Fixture, generated: &mut XGeneratedEgress, now: u64) -> bool {
    service_timed_presents(&f.state, &f.broker.registry, generated, now).unwrap()
}

fn notification(f: &Fixture) -> (u32, u64, u64) {
    match f._channels.protocol.try_recv().unwrap() {
        XClientEvent::PresentCompleteNotify {
            serial,
            ust,
            msc,
            kind: 1,
            mode: 0,
            ..
        } => (serial, ust, msc),
        other => panic!("expected resource-free NotifyMSC Copy, got {other:?}"),
    }
}

#[test]
fn notify_current_without_modulus_and_next_matching_field_with_modulus() {
    for (divisor, remainder, expected) in [(0, 0, 10), (5, 0, 15), (5, 2, 12)] {
        let f = fixture();
        let id = prepare_notify(&f, 81, 0, divisor, remainder);
        let clocks = f.broker.present_clock_router();
        assert_eq!(
            clocks.admissions().unwrap(),
            vec![crate::XPresentClockAdmission {
                request: id,
                target: None, // The fixture's window has not been mapped.
            }]
        );
        assert!(bind(&f, id, hardware(10)));
        assert!(clocks.admissions().unwrap().is_empty());
        let mut generated = XGeneratedEgress::default();
        if expected != 10 {
            assert!(!turn(&f, &mut generated, 10_000));
            clocks.observe_source(hardware(expected - 1)).unwrap();
            assert!(!turn(&f, &mut generated, (expected - 1) * 1_000));
            assert!(f._channels.protocol.try_recv().is_err());
            clocks.observe_source(hardware(expected)).unwrap();
        }
        assert!(turn(&f, &mut generated, expected * 1_000));
        assert_eq!(notification(&f), (81, expected * 1_000, expected));
        assert!(!generated.pending());
        assert!(f._channels.protocol.try_recv().is_err()); // no Idle
        assert_eq!(
            f.state.runtime.lock().unwrap().prepared_msc_notify_count(),
            0
        );
        assert!(clocks.bound_sources().unwrap().is_empty());
        assert!(!turn(&f, &mut generated, 20_000));
    }
}

#[test]
fn notify_never_has_a_pixmap_lead_or_gets_scrapped_by_an_equal_target_update() {
    let f = fixture();
    let notify = prepare_notify(&f, 82, 12, 0, 0);
    assert!(bind(&f, notify, hardware(10)));
    let pixmap = f.prepare_next(PIXMAP, false);
    f.state
        .runtime
        .lock()
        .unwrap()
        .request_prepared_present_clock(
            pixmap,
            crate::XPresentMscTiming::new(12, 0, 0, false).unwrap(),
        )
        .unwrap();
    assert!(bind(&f, pixmap, hardware(10)));
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(hardware(11)).unwrap();
    {
        let runtime = f.state.runtime.lock().unwrap();
        assert_eq!(runtime.ready_prepared_presents(), vec![pixmap]);
        assert!(runtime.ready_prepared_msc_notifies().is_empty());
        assert_eq!(runtime.prepared_msc_notify_count(), 1);
    }
    assert!(f._channels.protocol.try_recv().is_err());
    clocks.observe_source(hardware(12)).unwrap();
    let mut generated = XGeneratedEgress::default();
    assert!(turn(&f, &mut generated, 12_000));
    assert_eq!(notification(&f), (82, 12_000, 12));
    assert!(generated.pending()); // only the Pixmap creates an envelope
    assert!(f._channels.protocol.try_recv().is_err()); // neither a scrap nor Notify Idle
}

#[test]
fn fake_notify_arms_one_second_deadline_and_lost_hardware_settles_once() {
    let f = fixture();
    let id = prepare_notify(&f, 83, 11, 0, 0);
    assert!(bind(
        &f,
        id,
        crate::XPresentClockSample::background(10_100_000)
    ));
    let mut generated = XGeneratedEgress::default();
    let now = Instant::now();
    assert_eq!(
        timed_present_service_deadline(&f.state, 10_100_000, now, false, true).unwrap(),
        Some(now + Duration::from_micros(900_000))
    );
    assert!(!turn(&f, &mut generated, 10_999_999));
    assert!(turn(&f, &mut generated, 11_000_000));
    assert_eq!(notification(&f), (83, 11_000_000, 11));
    assert!(!generated.pending());
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(
        timed_present_service_deadline(&f.state, 11_000_000, now, false, true)
            .unwrap()
            .is_none()
    );

    let f = fixture();
    let id = prepare_notify(&f, 84, 100, 0, 0);
    assert!(bind(&f, id, hardware(10)));
    let clocks = f.broker.present_clock_router();
    clocks.observe_source(hardware(15)).unwrap();
    clocks.lose_source(hardware(0).source).unwrap();
    assert!(turn(&f, &mut generated, 15_000));
    assert_eq!(notification(&f), (84, 15_000, 15));
    clocks.observe_source(hardware(200)).unwrap();
    clocks.lose_source(hardware(0).source).unwrap();
    assert!(!turn(&f, &mut generated, 200_000));
    assert!(f._channels.protocol.try_recv().is_err());
    assert!(!generated.pending());
}

#[test]
fn admission_binds_once_in_request_order_without_executing_or_publishing_pixels() {
    let f = fixture();
    let pixmap = f.prepare_next(PIXMAP, false);
    let timing = crate::XPresentMscTiming::new(100, 0, 0, false).unwrap();
    f.state
        .runtime
        .lock()
        .unwrap()
        .request_prepared_present_clock(pixmap, timing)
        .unwrap();
    let notify = prepare_notify(&f, 85, 100, 0, 0);
    let clocks = f.broker.present_clock_router();
    assert_eq!(
        clocks
            .admissions()
            .unwrap()
            .iter()
            .map(|a| a.request)
            .collect::<Vec<_>>(),
        vec![pixmap, notify]
    );
    assert!(!bind(&f, notify, hardware(10))); // cannot overtake this window's earlier source choice
    assert!(bind(&f, pixmap, hardware(10)));
    assert!(bind(&f, notify, hardware(10)));
    assert!(!bind(&f, pixmap, hardware(99))); // no rebind/target change
    assert!(!bind(&f, notify, hardware(99)));
    assert!(clocks.admissions().unwrap().is_empty());
    let mut runtime = f.state.runtime.lock().unwrap();
    assert!(runtime.take_cpu_buffer_updates().is_empty());
    assert!(runtime.ready_prepared_presents().is_empty());
    assert!(runtime.ready_prepared_msc_notifies().is_empty());
    assert!(f._channels.protocol.try_recv().is_err());
}

#[test]
fn destruction_and_client_cleanup_cancel_bound_and_unbound_notify_without_events() {
    for destroy in [true, false] {
        let f = fixture();
        let bound = prepare_notify(&f, 86, 100, 0, 0);
        assert!(bind(&f, bound, hardware(10)));
        let unbound = prepare_notify(&f, 87, 100, 0, 0);
        if destroy {
            Fixture::destroy(&f.state, &f.broker.registry);
        } else {
            release_x11_connection_obligations(&f.state, CLIENT).unwrap();
            // Resource retention cannot preserve a departed connection's
            // pending replies or pixmap executions.
            f.state
                .runtime
                .lock()
                .unwrap()
                .retain_client_resource_range(
                    NS,
                    crate::XWireClientResourceRange {
                        base: 0x400000,
                        mask: 0xffff,
                    },
                    &[],
                )
                .unwrap();
            assert!(
                f.state
                    .runtime
                    .lock()
                    .unwrap()
                    .validate_window_access(NS, WINDOW)
                    .is_ok()
            );
        }
        let clocks = f.broker.present_clock_router();
        assert!(clocks.admissions().unwrap().is_empty());
        assert!(clocks.bound_sources().unwrap().is_empty());
        assert!(!bind(&f, unbound, hardware(200)));
        clocks.observe_source(hardware(200)).unwrap();
        clocks.lose_source(hardware(0).source).unwrap();
        assert!(!turn(&f, &mut XGeneratedEgress::default(), 200_000));
        assert!(f._channels.protocol.try_recv().is_err());
        assert_eq!(
            f.state.runtime.lock().unwrap().prepared_msc_notify_count(),
            0
        );
    }
}

#[test]
fn notify_capacity_counts_bound_and_unbound_and_releases_only_the_cancelled_client() {
    let f = fixture();
    let timing = crate::XPresentMscTiming::notify(100, 0, 0).unwrap();
    let mut runtime = f.state.runtime.lock().unwrap();
    for client in 1..=4 {
        for slot in 1..=crate::X_PRESENT_PER_CLIENT_CAPACITY {
            let id = TransactionId::from_raw(client * 1000 + slot as u64);
            runtime
                .prepare_present_msc_notify(client, id, NS, WINDOW, slot as u32, timing)
                .unwrap();
            if client == 1 && slot <= 32 {
                assert!(
                    runtime
                        .bind_present_clock_admission(id, hardware(10), None)
                        .unwrap()
                        .is_some()
                );
            }
        }
        assert_eq!(
            runtime.prepare_present_msc_notify(
                client,
                TransactionId::from_raw(client * 1000 + 99),
                NS,
                WINDOW,
                99,
                timing
            ),
            Err(crate::XPresentPreparationError::Capacity)
        );
    }
    assert_eq!(
        runtime.prepared_msc_notify_count(),
        crate::X_PREPARED_PRESENT_CAPACITY
    );
    assert_eq!(
        runtime.prepare_present_msc_notify(
            5,
            TransactionId::from_raw(5001),
            NS,
            WINDOW,
            99,
            timing
        ),
        Err(crate::XPresentPreparationError::Capacity)
    );
    runtime.cancel_client_prepared_presents(2);
    assert_eq!(
        runtime.prepared_msc_notify_count(),
        3 * crate::X_PRESENT_PER_CLIENT_CAPACITY
    );
    runtime
        .prepare_present_msc_notify(5, TransactionId::from_raw(5001), NS, WINDOW, 99, timing)
        .unwrap();
    runtime.cancel_client_prepared_presents(1);
    assert_eq!(
        runtime.prepared_msc_notify_count(),
        2 * crate::X_PRESENT_PER_CLIENT_CAPACITY + 1
    );
    for client in 3..=5 {
        runtime.cancel_client_prepared_presents(client);
    }
    assert_eq!(runtime.prepared_msc_notify_count(), 0);
}

#[test]
fn later_notify_rebinds_only_itself_and_an_earlier_target_completes_first() {
    let f = fixture();
    let first = prepare_notify(&f, 91, 20, 0, 0);
    assert!(bind(&f, first, hardware(10)));
    let later = prepare_notify(&f, 92, 11, 0, 0);
    let mut second = hardware(800);
    second.source = crate::XPresentClockSource::Hardware {
        domain: 2,
        incarnation: 1,
    };
    assert!(bind(&f, later, second));
    let clocks = f.broker.present_clock_router();
    assert_eq!(
        clocks.bound_sources().unwrap(),
        vec![hardware(0).source, second.source]
    );
    second.msc += 1;
    second.ust += 1_000;
    clocks.observe_source(second).unwrap();
    let mut generated = XGeneratedEgress::default();
    assert!(turn(&f, &mut generated, second.ust));
    assert_eq!(notification(&f), (92, second.ust, 11));
    assert_eq!(clocks.bound_sources().unwrap(), vec![hardware(0).source]);
    clocks.observe_source(hardware(20)).unwrap();
    assert!(turn(&f, &mut generated, second.ust));
    assert_eq!(notification(&f), (91, 20_000, 20));
    assert!(clocks.bound_sources().unwrap().is_empty());
    assert!(!generated.pending());
    assert!(f._channels.protocol.try_recv().is_err());
}
