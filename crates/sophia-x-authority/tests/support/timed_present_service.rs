//! Supplied clocks, real authority runtime, reservation registry and routed
//! service. No native clock query, KMS or wire timing admission is claimed.
use super::*;
use crate::{XAuthorityRequestKind, XAuthorityRequestPacket};
use sophia_protocol::{Rect, Region, SurfaceConstraints};

const NS: NamespaceId = NamespaceId::from_raw(298);
const WINDOW: XResourceId = XResourceId::new(0x400001, 1);
const PIXMAP: XResourceId = XResourceId::new(0x400002, 1);
const SURFACE: SurfaceId = SurfaceId::new(298, 1);
const CLIENT: XServerFrontendClientId = XServerFrontendClientId::from_raw(1);

struct Fixture {
    state: X11CoreSocketServerState,
    broker: XServerFrontendRouteBroker,
    _registration: XServerFrontendClientRouteRegistration,
    _channels: XServerFrontendClientRouteChannels,
    preparation: TransactionId,
}

impl Fixture {
    fn new(pixels: bool) -> Self {
        let state = X11CoreSocketServerState::new();
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(64).unwrap());
        let (registration, channels) = broker.registry.register_client(CLIENT).unwrap();
        broker.registry.bind_runtime(&state.runtime).unwrap();
        broker
            .registry
            .register_surface(CLIENT, NS, SURFACE, WINDOW)
            .unwrap();
        let preparation = state.allocate_transaction().unwrap();
        {
            let mut runtime = state.runtime.lock().unwrap();
            let response = runtime.apply(XAuthorityRequestPacket {
                transaction: preparation,
                namespace: NS,
                kind: XAuthorityRequestKind::CreateWindow {
                    window: WINDOW,
                    surface: SURFACE,
                    geometry: Rect {
                        x: 0,
                        y: 0,
                        width: 4,
                        height: 4,
                    },
                    constraints: SurfaceConstraints {
                        min_size: None,
                        max_size: None,
                    },
                    generation: 1,
                },
            });
            assert_eq!(response.outcome, crate::XAuthorityResponseOutcome::Accepted);
            runtime
                .create_pixmap(
                    NS,
                    PIXMAP,
                    Size {
                        width: 4,
                        height: 4,
                    },
                    24,
                    1,
                )
                .unwrap();
            if pixels {
                let response = runtime.apply_put_image(
                    preparation,
                    NS,
                    PIXMAP,
                    Region::single(Rect {
                        x: 0,
                        y: 0,
                        width: 4,
                        height: 4,
                    }),
                    Some(&[0x55; 64]),
                    None,
                );
                assert_eq!(response.outcome, crate::XAuthorityResponseOutcome::Accepted);
            }
            runtime.begin_dispatch();
            runtime
                .prepare_standard_pixmap(
                    CLIENT.raw(),
                    preparation,
                    NS,
                    WINDOW,
                    PIXMAP,
                    (0, 0),
                    None,
                    None,
                    crate::XPresentFenceResources::default(),
                )
                .unwrap();
        }
        broker
            .registry
            .queue_present(preparation, CLIENT, WINDOW, PIXMAP, 41, None, true)
            .unwrap();
        Self {
            state,
            broker,
            _registration: registration,
            _channels: channels,
            preparation,
        }
    }

    fn schedule(&self, sample: crate::XPresentClockSample, target: u64) {
        self.state
            .runtime
            .lock()
            .unwrap()
            .schedule_prepared_present(
                self.preparation,
                crate::XPresentMscTiming::new(target, 0, 0, false).unwrap(),
                sample,
                None,
            )
            .unwrap();
    }

    fn destroy(state: &X11CoreSocketServerState, registry: &XServerFrontendRouteRegistry) {
        let mut runtime = state.runtime.lock().unwrap();
        let _transaction = state.allocate_transaction().unwrap();
        runtime.destroy_window_subtree(NS, WINDOW).unwrap();
        registry.cancel_present_window(WINDOW).unwrap();
    }

    fn prepare_next(&self, pixmap: XResourceId, partial: bool) -> TransactionId {
        let id = self.state.allocate_transaction().unwrap();
        self.state
            .runtime
            .lock()
            .unwrap()
            .prepare_standard_pixmap(
                CLIENT.raw(),
                id,
                NS,
                WINDOW,
                pixmap,
                (0, 0),
                None,
                partial.then(|| {
                    Region::single(Rect {
                        x: 0,
                        y: 0,
                        width: 2,
                        height: 2,
                    })
                }),
                crate::XPresentFenceResources::default(),
            )
            .unwrap();
        self.broker
            .registry
            .queue_present(id, CLIENT, WINDOW, pixmap, id.raw() as u32, None, false)
            .unwrap();
        id
    }
}

#[test]
fn timed_present_scrap_releases_only_pixels_and_idle_before_the_target() {
    let fixture = Fixture::new(true);
    let sample = crate::XPresentClockSample::background(10_500_000);
    let timing = crate::XPresentMscTiming::new(20, 0, 0, false).unwrap();
    fixture.schedule(sample, 20);
    fixture
        .broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    {
        let mut runtime = fixture.state.runtime.lock().unwrap();
        runtime.free_pixmap(NS, PIXMAP).unwrap();
        assert_eq!(runtime.retained_pixmap_count(), 1);
        runtime
            .create_pixmap(
                NS,
                PIXMAP,
                Size {
                    width: 4,
                    height: 4,
                },
                24,
                1,
            )
            .unwrap();
    }
    let partial = fixture.prepare_next(PIXMAP, true);
    assert!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .schedule_prepared_present(partial, timing, sample, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .retained_pixmap_count(),
        1
    );
    let full = fixture.prepare_next(PIXMAP, false);
    {
        let mut runtime = fixture.state.runtime.lock().unwrap();
        assert_eq!(
            runtime
                .schedule_prepared_present(full, timing, sample, None)
                .unwrap(),
            vec![fixture.preparation, partial]
        );
        // The freed old XID's backing is gone immediately. The new XID's
        // backing still belongs to the full update. All three reservations
        // remain charged; only the new full request can ever execute.
        assert_eq!(runtime.retained_pixmap_count(), 0);
        assert_eq!(runtime.prepared_present_count(), 3);
        assert_eq!(
            runtime
                .execute_prepared_standard_pixmap(fixture.preparation, TransactionId::from_raw(100))
                .unwrap_err(),
            crate::XPresentExecutionError::Superseded
        );
        assert_eq!(
            runtime.prepared_present_fences(fixture.preparation),
            Some(crate::XPreparedPresentFences::default())
        );
    }
    let mut generated = XGeneratedEgress::default();
    for visit in 0..2 {
        assert_eq!(
            service_timed_presents(
                &fixture.state,
                &fixture.broker.registry,
                &mut generated,
                || sample.ust
            )
            .unwrap(),
            visit == 0
        );
    }
    for expected_serial in [41, partial.raw() as u32] {
        let event = fixture
            ._channels
            .protocol
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        assert!(
            matches!(event, XClientEvent::PresentIdleNotify { serial, pixmap: PIXMAP, .. } if serial == expected_serial)
        );
    }
    assert!(fixture._channels.protocol.try_recv().is_err());
    assert!(!generated.pending());
    assert_eq!(
        fixture
            .broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .len(),
        3
    );
    // Cancellation after Idle still consumes the queued completion, without
    // releasing the old pixmap twice or touching the reused XID.
    Fixture::destroy(&fixture.state, &fixture.broker.registry);
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_count(),
        0
    );
    assert!(
        fixture
            .broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(fixture._channels.protocol.try_recv().is_err());
}

#[test]
fn timed_present_scrap_uses_the_private_idle_fence_and_forgets_both_fences() {
    for destroy in [false, true] {
        let fixture = Fixture::new(true);
        let xid = XResourceId::new(0x400003, 1);
        let old_fd = Arc::new(sophia_xshmfence::allocate().unwrap());
        let new_fd = Arc::new(sophia_xshmfence::allocate().unwrap());
        {
            let mut runtime = fixture.state.runtime.lock().unwrap();
            runtime.cancel_prepared_standard_pixmap(fixture.preparation);
            runtime.create_dri3_fence(NS, xid, 1).unwrap();
            let fence = runtime.dri3_fence_handle(NS, xid).unwrap();
            runtime
                .retain_present_fence_descriptor(fence, old_fd.clone())
                .unwrap();
            runtime
                .prepare_standard_pixmap(
                    CLIENT.raw(),
                    fixture.preparation,
                    NS,
                    WINDOW,
                    PIXMAP,
                    (0, 0),
                    None,
                    None,
                    crate::XPresentFenceResources {
                        wait: Some(xid),
                        idle: Some(xid),
                    },
                )
                .unwrap();
            if destroy {
                runtime.destroy_dri3_fence(NS, xid).unwrap();
                runtime.create_dri3_fence(NS, xid, 1).unwrap();
                let new_handle = runtime.dri3_fence_handle(NS, xid).unwrap();
                assert_ne!(new_handle, fence);
                runtime
                    .retain_present_fence_descriptor(new_handle, new_fd.clone())
                    .unwrap();
            }
        }
        let sample = crate::XPresentClockSample::background(10_500_000);
        fixture.schedule(sample, 20);
        let next = fixture.prepare_next(PIXMAP, false);
        let mut runtime = fixture.state.runtime.lock().unwrap();
        assert_eq!(
            runtime
                .schedule_prepared_present(
                    next,
                    crate::XPresentMscTiming::new(20, 0, 0, false).unwrap(),
                    sample,
                    None
                )
                .unwrap(),
            vec![fixture.preparation]
        );
        assert_eq!(sophia_xshmfence::query(&old_fd).unwrap(), !destroy);
        assert!(!sophia_xshmfence::query(&new_fd).unwrap());
        assert_eq!(
            runtime.prepared_present_fences(fixture.preparation),
            Some(crate::XPreparedPresentFences::default())
        );
    }
}

#[test]
fn timed_present_scrapped_completions_still_consume_the_client_bound() {
    let fixture = Fixture::new(true);
    let sample = crate::XPresentClockSample::background(10_500_000);
    fixture.schedule(sample, 20);
    for _ in 1..crate::X_PRESENT_PER_CLIENT_CAPACITY {
        let next = fixture.prepare_next(PIXMAP, false);
        let superseded = fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .schedule_prepared_present(
                next,
                crate::XPresentMscTiming::new(20, 0, 0, false).unwrap(),
                sample,
                None,
            )
            .unwrap();
        assert_eq!(superseded.len(), 1);
    }
    let mut runtime = fixture.state.runtime.lock().unwrap();
    assert_eq!(
        runtime.prepared_present_count(),
        crate::X_PRESENT_PER_CLIENT_CAPACITY
    );
    assert_eq!(
        runtime.prepare_standard_pixmap(
            CLIENT.raw(),
            TransactionId::from_raw(100),
            NS,
            WINDOW,
            PIXMAP,
            (0, 0),
            None,
            None,
            crate::XPresentFenceResources::default()
        ),
        Err(crate::XPresentPreparationError::Capacity)
    );
    runtime.cancel_client_prepared_presents(CLIENT.raw());
    assert_eq!(runtime.prepared_present_count(), 0);
    runtime
        .prepare_standard_pixmap(
            CLIENT.raw(),
            TransactionId::from_raw(101),
            NS,
            WINDOW,
            PIXMAP,
            (0, 0),
            None,
            None,
            crate::XPresentFenceResources::default(),
        )
        .unwrap();
}

#[test]
fn timed_present_skip_waits_for_its_original_head_and_keeps_its_frozen_msc() {
    let fixture = Fixture::new(true);
    let source = crate::XPresentClockSource::Hardware {
        domain: 1,
        incarnation: 1,
    };
    let sample = crate::XPresentClockSample {
        source,
        ust: 10_000,
        msc: 10,
    };
    fixture.schedule(sample, 20);
    fixture
        .broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    let next = fixture.prepare_next(PIXMAP, false);
    {
        let mut runtime = fixture.state.runtime.lock().unwrap();
        runtime
            .schedule_prepared_present(
                next,
                crate::XPresentMscTiming::new(20, 0, 0, false).unwrap(),
                sample,
                None,
            )
            .unwrap();
        // Isolate the old completion from the replacement's renderer work.
        runtime.cancel_prepared_standard_pixmap(next);
        fixture.broker.registry.cancel_present(next).unwrap();
    }
    let router = fixture.broker.present_clock_router();
    router
        .observe(
            SURFACE,
            crate::XPresentClockSample {
                source: crate::XPresentClockSource::Hardware {
                    domain: 2,
                    incarnation: 1,
                },
                ust: 12_000,
                msc: 900,
            },
            None,
        )
        .unwrap();
    assert_eq!(router.bound_sources().unwrap(), vec![source]);
    let mut generated = XGeneratedEgress::default();
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 12_000
        )
        .unwrap()
    );
    assert!(matches!(
        fixture._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentIdleNotify { serial: 41, .. }
    ));
    for msc in [19, 20] {
        router
            .observe_source(crate::XPresentClockSample {
                source,
                ust: msc * 1000,
                msc,
            })
            .unwrap();
        assert_eq!(
            service_timed_presents(
                &fixture.state,
                &fixture.broker.registry,
                &mut generated,
                || msc * 1000
            )
            .unwrap(),
            msc == 20
        );
        if msc == 19 {
            assert!(fixture._channels.protocol.try_recv().is_err());
        }
    }
    assert!(
        matches!(fixture._channels.protocol.try_recv().unwrap(), XClientEvent::PresentCompleteNotify {
        serial: 41, ust: 20_000, msc: 20, kind: 0, mode, ..
    } if mode == XPresentCompletionMode::Skip as u8)
    );
    assert!(fixture._channels.protocol.try_recv().is_err());
    assert!(
        fixture
            .broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(router.bound_sources().unwrap().is_empty());
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_count(),
        0
    );
    assert!(!generated.pending()); // A Skip has no execution ticket or pixels.
}

#[test]
fn timed_present_lost_head_settles_skip_and_idle_once_without_execution() {
    let fixture = Fixture::new(true);
    let source = crate::XPresentClockSource::Hardware {
        domain: 1,
        incarnation: 7,
    };
    fixture.schedule(
        crate::XPresentClockSample {
            source,
            ust: 10_000,
            msc: 10,
        },
        20,
    );
    fixture
        .broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    let router = fixture.broker.present_clock_router();
    router.lose_source(source).unwrap();
    router.lose_source(source).unwrap();
    let next_ticket = fixture.state.next_transaction_id.load(Ordering::Relaxed);
    let mut generated = XGeneratedEgress::default();
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 15_000
        )
        .unwrap()
    );
    assert!(matches!(
        fixture._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentIdleNotify { serial: 41, .. }
    ));
    assert!(
        matches!(fixture._channels.protocol.try_recv().unwrap(), XClientEvent::PresentCompleteNotify {
        serial: 41, ust: 10_000, msc: 10, kind: 0, mode, ..
    } if mode == XPresentCompletionMode::Skip as u8)
    );
    assert!(fixture._channels.protocol.try_recv().is_err());
    assert_eq!(
        fixture.state.next_transaction_id.load(Ordering::Relaxed),
        next_ticket
    );
    assert!(
        !service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 1_000_000
        )
        .unwrap()
    );
    assert!(router.demands().unwrap().is_empty());
    assert!(router.bound_sources().unwrap().is_empty());
}

#[test]
fn timed_present_cancel_before_execution_has_no_ticket_or_feedback() {
    let fixture = Fixture::new(true);
    Fixture::destroy(&fixture.state, &fixture.broker.registry);
    let next = fixture.state.next_transaction_id.load(Ordering::Relaxed);
    let mut generated = XGeneratedEgress::default();
    assert!(
        !execute_timed_present(
            &fixture.state,
            &fixture.broker.registry,
            fixture.preparation,
            &mut generated,
            |_, _| panic!("cancelled requests never test a fence")
        )
        .unwrap()
    );
    assert_eq!(
        fixture.state.next_transaction_id.load(Ordering::Relaxed),
        next
    );
    assert!(!generated.pending());
    assert!(
        !fixture
            .broker
            .route_present_complete(fixture.preparation, 1, 1, XPresentCompletionMode::Copy)
            .unwrap()
    );
    assert!(
        !fixture
            .broker
            .route_present_idle(fixture.preparation)
            .unwrap()
    );
}

#[test]
fn timed_present_execution_and_feedback_rekey_are_atomic_against_destroy() {
    let fixture = Fixture::new(true);
    let state = fixture.state.clone();
    let registry = fixture.broker.registry.clone();
    let preparation = fixture.preparation;
    let (entered, entering) = sync_channel(1);
    let (release, released) = sync_channel(1);
    let worker = std::thread::spawn(move || {
        let mut generated = XGeneratedEgress::default();
        assert!(
            execute_timed_present(
                &state,
                &registry,
                preparation,
                &mut generated,
                |_, fence| {
                    assert!(fence.is_none());
                    entered.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(2)).unwrap();
                    Ok(true)
                }
            )
            .unwrap()
        );
        generated
    });
    entering.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(fixture.state.runtime.try_lock().is_err());
    let state = fixture.state.clone();
    let registry = fixture.broker.registry.clone();
    let (started, starting) = sync_channel(1);
    let destroy = std::thread::spawn(move || {
        started.send(()).unwrap();
        Fixture::destroy(&state, &registry);
    });
    starting.recv_timeout(Duration::from_secs(2)).unwrap();
    release.send(()).unwrap();
    let mut generated = worker.join().unwrap();
    destroy.join().unwrap();
    let envelope = generated.take().next().unwrap();
    assert!(envelope.transaction.raw() > fixture.preparation.raw());
    let batch = envelope.batch.unwrap();
    assert_eq!(batch.software_present_submissions.len(), 1);
    assert_eq!(batch.client, Some(CLIENT));
    assert_eq!(batch.cpu_buffer_updates.len(), 1);
    assert_eq!(batch.transactions[0].surface, SURFACE);
    // Execution won the race; removal comes afterwards. The batch remains
    // ordinary ordered content, but no late feedback reaches a gone window.
    assert!(
        !fixture
            .broker
            .route_present_complete(batch.transaction, 1, 1, XPresentCompletionMode::Copy)
            .unwrap()
    );
    assert!(
        !fixture
            .broker
            .route_present_idle(batch.transaction)
            .unwrap()
    );
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_count(),
        0
    );
}

#[test]
fn timed_present_rejection_publishes_an_empty_fresh_ticket() {
    let fixture = Fixture::new(false);
    let source = crate::XPresentClockSource::Hardware {
        domain: 1,
        incarnation: 1,
    };
    fixture.schedule(
        crate::XPresentClockSample {
            source,
            ust: 10_000,
            msc: 10,
        },
        11,
    );
    fixture
        .broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    let mut generated = XGeneratedEgress::default();
    assert!(
        execute_timed_present(
            &fixture.state,
            &fixture.broker.registry,
            fixture.preparation,
            &mut generated,
            |_, _| Ok(true)
        )
        .unwrap()
    );
    let envelope = generated.take().next().unwrap();
    assert_eq!(envelope.transaction.raw(), 2);
    assert!(envelope.batch.is_none());
    assert_eq!(
        fixture
            .broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .len(),
        1
    );
    // No rendering happened, but the accepted request still owes both
    // phases. Idle is immediate; Skip does not borrow the one-field lead.
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 10_000
        )
        .unwrap()
    );
    assert!(matches!(
        fixture._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentIdleNotify { serial: 41, .. }
    ));
    assert!(fixture._channels.protocol.try_recv().is_err());
    fixture
        .broker
        .present_clock_router()
        .observe_source(crate::XPresentClockSample {
            source,
            ust: 11_000,
            msc: 11,
        })
        .unwrap();
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 11_000
        )
        .unwrap()
    );
    assert!(
        matches!(fixture._channels.protocol.try_recv().unwrap(), XClientEvent::PresentCompleteNotify { serial: 41, msc: 11, mode, .. } if mode == XPresentCompletionMode::Skip as u8)
    );
    assert!(fixture._channels.protocol.try_recv().is_err());
    assert!(
        fixture
            .broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_count(),
        0
    );
}

#[test]
fn timed_present_keeps_its_original_clock_after_a_source_change() {
    let fixture = Fixture::new(true);
    fixture.schedule(
        crate::XPresentClockSample {
            source: crate::XPresentClockSource::Hardware {
                domain: 1,
                incarnation: 1,
            },
            ust: 100,
            msc: 10,
        },
        11,
    );
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .ready_prepared_presents(),
        vec![fixture.preparation]
    );
    fixture
        .broker
        .present_clock_router()
        .observe(SURFACE, crate::XPresentClockSample::background(200), None)
        .unwrap();
    let mut generated = XGeneratedEgress::default();
    assert!(
        execute_timed_present(
            &fixture.state,
            &fixture.broker.registry,
            fixture.preparation,
            &mut generated,
            |runtime, _| timed_present_ready(runtime, fixture.preparation, 200)
        )
        .unwrap()
    );
    assert!(generated.pending());
    assert_eq!(fixture.state.next_transaction_id.load(Ordering::Relaxed), 3);
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_count(),
        0
    );
}

#[path = "timed_present_review.rs"]
mod review;

#[path = "timed_present_completion.rs"]
mod completion;

#[test]
fn timed_present_fake_deadline_and_hardware_wake_are_obligations_only() {
    let fixture = Fixture::new(true);
    let origin = Instant::now();
    assert_eq!(
        timed_present_service_deadline(&fixture.state, 250_000, origin, false, true).unwrap(),
        None
    );
    fixture.schedule(crate::XPresentClockSample::background(250_000), 1);
    assert_eq!(
        timed_present_service_deadline(&fixture.state, 250_000, origin, false, true).unwrap(),
        Some(origin + Duration::from_micros(750_000))
    );
    let mut generated = XGeneratedEgress::default();
    assert!(
        !service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 999_999
        )
        .unwrap()
    );
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 1_000_000
        )
        .unwrap()
    );
    assert_eq!(
        timed_present_service_deadline(&fixture.state, 1_000_000, origin, false, true).unwrap(),
        None
    );

    let fixture = Fixture::new(true);
    let source = crate::XPresentClockSource::Hardware {
        domain: 1,
        incarnation: 1,
    };
    fixture.schedule(
        crate::XPresentClockSample {
            source,
            ust: 100,
            msc: 40,
        },
        43,
    );
    let wake = sophia_wake::Wake::new().unwrap();
    fixture.broker.registry.service_wake.set(wake.notifier());
    let router = fixture.broker.present_clock_router();
    assert_eq!(router.demands().unwrap(), vec![SURFACE]);
    assert_eq!(
        timed_present_service_deadline(&fixture.state, 1_000_000, origin, false, true).unwrap(),
        None
    );
    router
        .observe(
            SURFACE,
            crate::XPresentClockSample {
                source,
                ust: 110,
                msc: 42,
            },
            None,
        )
        .unwrap();
    let mut fds = [rustix::event::PollFd::new(
        &wake,
        rustix::event::PollFlags::IN,
    )];
    sophia_wake::wait(&mut fds, Some(Instant::now())).unwrap();
    assert!(fds[0].revents().contains(rustix::event::PollFlags::IN));
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .ready_prepared_presents(),
        vec![fixture.preparation]
    );
}

#[test]
fn timed_present_idle_routed_service_executes_on_a_deadline_or_clock_wake() {
    for hardware in [false, true] {
        let fixture = Fixture::new(true);
        let now_usec = present_monotonic_usec();
        let source = crate::XPresentClockSource::Hardware {
            domain: 7,
            incarnation: 2,
        };
        let sample = if hardware {
            crate::XPresentClockSample {
                source,
                ust: now_usec,
                msc: 40,
            }
        } else {
            crate::XPresentClockSample::background(now_usec)
        };
        fixture.schedule(sample, if hardware { 43 } else { sample.msc + 1 });
        let router = fixture.broker.present_clock_router();
        let path = private_service_socket(if hardware {
            "timed-clock"
        } else {
            "timed-fake"
        });
        let wake = sophia_wake::WakeSlot::default();
        let mut frontend = XServerFrontend::bind(
            XServerFrontendConfig::new(&path, NS)
                .unwrap()
                .with_service_wake(wake.clone()),
        )
        .unwrap();
        frontend.state = fixture.state.clone();
        let (sender, output) = sync_channel(2);
        let egress = XAuthorityOrderedEgress::new(
            sender,
            Arc::new(AtomicBool::new(false)),
            Arc::new(|_| {}),
        );
        // The initial preparation was a request with an empty observation.
        egress
            .submit_blocking(XAuthorityBoundedEgressEnvelope::new(
                fixture.preparation,
                None,
            ))
            .unwrap();
        let (commands, inbox) = sync_channel(1);
        let commands = sophia_wake::SignalSender::new(commands, wake.clone());
        let mut broker = fixture.broker;
        let worker = std::thread::spawn(move || {
            let observer: Arc<X11CoreTraceObserver> = Arc::new(|_| Ok(None));
            drive_routed_service(
                &mut frontend,
                &mut broker,
                &inbox,
                &egress,
                &observer,
                &mut XGeneratedEgress::default(),
            )
        });
        assert!(waited_for(|| wake.attached()));
        if hardware {
            assert!(output.recv_timeout(Duration::from_millis(20)).is_err());
            // No socket traffic. A backend observation, through the real
            // router, must wake the service from its untimed wait.
            router
                .observe(
                    SURFACE,
                    crate::XPresentClockSample {
                        source,
                        ust: present_monotonic_usec(),
                        msc: 42,
                    },
                    None,
                )
                .unwrap();
        }
        let result = output.recv_timeout(Duration::from_millis(1500));
        commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect)
            .unwrap();
        worker.join().unwrap().unwrap();
        let batch =
            result.expect("idle service executes within one fake tick or hardware notification");
        assert_eq!(batch.transaction.raw(), 2);
        assert_eq!(batch.software_present_submissions.len(), 1);
        assert!(output.try_recv().is_err());
        std::fs::remove_file(path).ok();
    }
}

#[test]
fn timed_present_exact_private_fences_survive_execution_then_release_normally() {
    let fixture = Fixture::new(true);
    let wait_xid = XResourceId::new(0x400003, 1);
    let idle_xid = XResourceId::new(0x400004, 1);
    let (wait, idle) = {
        let mut runtime = fixture.state.runtime.lock().unwrap();
        runtime.cancel_prepared_standard_pixmap(fixture.preparation);
        runtime.create_dri3_fence(NS, wait_xid, 1).unwrap();
        runtime.create_dri3_fence(NS, idle_xid, 1).unwrap();
        let wait = runtime.dri3_fence_handle(NS, wait_xid).unwrap();
        let idle = runtime.dri3_fence_handle(NS, idle_xid).unwrap();
        let fd = Arc::new(sophia_xshmfence::allocate().unwrap());
        runtime
            .retain_present_fence_descriptor(wait, fd.clone())
            .unwrap();
        runtime
            .prepare_standard_pixmap(
                CLIENT.raw(),
                fixture.preparation,
                NS,
                WINDOW,
                PIXMAP,
                (0, 0),
                None,
                None,
                crate::XPresentFenceResources {
                    wait: Some(wait_xid),
                    idle: Some(idle_xid),
                },
            )
            .unwrap();
        assert!(!runtime.present_fence_ready(Some(wait)).unwrap());
        sophia_xshmfence::trigger(&fd).unwrap();
        (wait, idle)
    };
    let mut generated = XGeneratedEgress::default();
    assert!(
        execute_timed_present(
            &fixture.state,
            &fixture.broker.registry,
            fixture.preparation,
            &mut generated,
            |runtime, fence| runtime
                .present_fence_ready(fence)
                .map_err(X11SetupSocketError::new)
        )
        .unwrap()
    );
    let mut envelope = generated.take().next().unwrap();
    let batch = envelope.batch.take().unwrap();
    assert_eq!(
        batch.software_present_submissions[0].acquire_fence,
        Some(wait)
    );
    assert_eq!(batch.software_present_submissions[0].idle_fence, Some(idle));
    {
        let mut runtime = fixture.state.runtime.lock().unwrap();
        assert_eq!(runtime.destroy_dri3_fence(NS, wait_xid).unwrap(), wait);
        assert_eq!(runtime.destroy_dri3_fence(NS, idle_xid).unwrap(), idle);
        runtime.create_dri3_fence(NS, wait_xid, 1).unwrap();
        assert_ne!(runtime.dri3_fence_handle(NS, wait_xid).unwrap(), wait);
    }
    // Destroy after execution returns the backend's exact release identities.
    // The already-built submission does not re-resolve reused public XIDs.
    assert_eq!(
        batch.software_present_submissions[0].acquire_fence,
        Some(wait)
    );
    assert_eq!(batch.software_present_submissions[0].idle_fence, Some(idle));
    drop(fixture._registration);
    assert!(
        !fixture
            .broker
            .route_present_complete(batch.transaction, 1, 1, XPresentCompletionMode::Copy)
            .unwrap()
    );
    assert!(
        !fixture
            .broker
            .route_present_idle(batch.transaction)
            .unwrap()
    );
}

#[path = "timed_present_admission.rs"]
mod admission;

#[path = "timed_present_demand.rs"]
mod demand;
