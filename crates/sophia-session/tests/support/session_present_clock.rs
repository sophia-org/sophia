use super::*;
use sophia_backend_live::{LiveNativePresentClockSample, LiveNativePresentClockStatus};
use sophia_protocol::TransactionId;
use sophia_x_authority::{
    XPresentMscTiming, XPresentScheduledKind, XPresentScheduledRequest, XPresentWindowClock,
    XPresentWindowSchedule,
};
use std::cell::RefCell;
use std::num::NonZeroUsize;

struct Frontend {
    queue: RefCell<XPresentWindowSchedule>,
    losses: RefCell<Vec<XPresentClockSource>>,
}
impl Frontend {
    fn new(source: XPresentClockSource) -> Self {
        let mut clock = XPresentWindowClock::default();
        clock
            .observe(
                XPresentClockSample {
                    source,
                    ust: 10_000,
                    msc: 10,
                },
                None,
            )
            .unwrap();
        let mut queue = XPresentWindowSchedule::new(NonZeroUsize::new(4).unwrap());
        queue
            .insert(XPresentScheduledRequest {
                request: TransactionId::from_raw(1),
                kind: XPresentScheduledKind::Pixmap,
                binding: clock.binding().unwrap(),
                target: clock
                    .target(XPresentMscTiming::new(20, 0, 0, false).unwrap())
                    .unwrap(),
                replaces_contents: true,
            })
            .unwrap();
        Self {
            queue: RefCell::new(queue),
            losses: RefCell::new(Vec::new()),
        }
    }
}
impl FrontendClocks for Frontend {
    fn admissions(&self) -> ClockResult<Vec<XPresentClockAdmission>> {
        Ok(Vec::new())
    }
    fn bind_admission(&self, _: TransactionId, _: XPresentClockSample) -> ClockResult<()> {
        panic!("no unbound admissions")
    }

    fn query_demands(&self) -> ClockResult<Vec<(XPresentClockSource, u64)>> {
        let mut demands = BTreeMap::<_, u64>::new();
        for (source, fields) in self.queue.borrow().clock_demands() {
            demands
                .entry(source)
                .and_modify(|n| *n = (*n).min(fields))
                .or_insert(fields);
        }
        Ok(demands.into_iter().collect())
    }
    fn bound_sources(&self) -> ClockResult<Vec<XPresentClockSource>> {
        Ok(self.queue.borrow().clock_sources().collect())
    }
    fn lose_source(&self, source: XPresentClockSource) -> ClockResult<()> {
        self.queue.borrow_mut().lose_source(source);
        self.losses.borrow_mut().push(source);
        Ok(())
    }
    fn observe_source(&self, sample: XPresentClockSample) -> ClockResult<()> {
        self.queue.borrow_mut().observe(sample).unwrap();
        Ok(())
    }
}
fn hardware(owner: u64, incarnation: u64) -> LiveNativePresentClockSource {
    LiveNativePresentClockSource { owner, incarnation }
}
fn live(source_id: LiveNativePresentClockSource) -> BTreeMap<XPresentClockSource, ClockHead> {
    BTreeMap::from([(
        source(source_id),
        ClockHead {
            head: RenderHeadId::from_raw(1),
            interval: Duration::from_millis(10),
            observed: None,
        },
    )])
}
fn observation(
    source: LiveNativePresentClockSource,
    msc: u64,
) -> LiveNativePresentClockObservation {
    LiveNativePresentClockObservation {
        lost: None,
        current: Some(LiveNativePresentClockSample {
            source,
            ust_usec: msc * 1_000,
            msc,
        }),
        status: LiveNativePresentClockStatus::Observed,
    }
}

#[test]
fn queries_follow_demand_and_real_observations_without_idle_polling() {
    let native_source = hardware(1, 1);
    let frontend = Frontend::new(source(native_source));
    let mut bridge = SessionPresentClocks::default();
    let start = Instant::now();
    let mut queried = 0;
    bridge
        .service_with(&frontend, live(native_source), start, |_| {
            queried += 1;
            observation(native_source, 10)
        })
        .unwrap();
    assert_eq!(queried, 1);
    assert_eq!(
        bridge.cap_wait(start, Duration::from_secs(1)),
        Duration::from_millis(10)
    );
    for ms in 1..10 {
        bridge
            .service_with(
                &frontend,
                live(native_source),
                start + Duration::from_millis(ms),
                |_| panic!("a busy owner loop must not query on every pass"),
            )
            .unwrap();
    }
    // Ten milliseconds did not synthesize MSC=11; the kernel still says 10.
    bridge
        .service_with(
            &frontend,
            live(native_source),
            start + Duration::from_millis(10),
            |_| {
                queried += 1;
                observation(native_source, 10)
            },
        )
        .unwrap();
    assert_eq!(queried, 2);
    assert!(frontend.queue.borrow().ready().next().is_none());
    bridge
        .service_with(
            &frontend,
            live(native_source),
            start + Duration::from_millis(20),
            |_| {
                queried += 1;
                observation(native_source, 19)
            },
        )
        .unwrap();
    assert_eq!(frontend.queue.borrow().ready().count(), 1);
    frontend.queue.borrow_mut().take(TransactionId::from_raw(1));
    bridge
        .service_with(
            &frontend,
            live(native_source),
            start + Duration::from_secs(1),
            |_| panic!("a settled source has no clock demand"),
        )
        .unwrap();
    assert!(bridge.next.is_empty());
    assert_eq!(
        bridge.cap_wait(start, Duration::from_secs(1)),
        Duration::from_secs(1)
    );
}

#[test]
fn empty_owner_startup_can_service_the_real_frontend_before_client_setup() {
    let broker =
        sophia_x_authority::XServerFrontendRouteBroker::new(NonZeroUsize::new(64).unwrap());
    let frontend = broker.present_clock_router();
    let mut bridge = SessionPresentClocks::default();
    let now = Instant::now();
    bridge.service(&frontend, None, None, None, now).unwrap();
    bridge
        .service(&frontend, None, None, None, now + Duration::from_secs(1))
        .unwrap();
    assert_eq!(bridge.queries(), 0);
    assert_eq!(
        bridge.cap_wait(now, Duration::from_secs(1)),
        Duration::from_secs(1)
    );
}

#[test]
fn modeset_rollback_suspend_and_replacement_settle_vanished_bindings_once() {
    // Empty inventory: invalidate before modeset, quarantine/rollback, suspend.
    // Same head/new incarnation: rollback followed by a query or native resume.
    for current in [None, Some(hardware(1, 2)), Some(hardware(2, 1))] {
        let old = hardware(1, 1);
        let frontend = Frontend::new(source(old));
        let mut bridge = SessionPresentClocks::default();
        let now = Instant::now();
        bridge
            .service_with(&frontend, live(old), now, |_| observation(old, 10))
            .unwrap();
        let available = current.map(live).unwrap_or_default();
        for _ in 0..2 {
            bridge
                .service_with(&frontend, available.clone(), now, |_| {
                    panic!("old requests must lose their source, never query/rebind a replacement")
                })
                .unwrap();
        }
        assert_eq!(*frontend.losses.borrow(), vec![source(old)]);
        let request = frontend.queue.borrow().ready().next().unwrap();
        assert_eq!(request.kind, XPresentScheduledKind::Skip);
        assert_eq!(
            frontend.queue.borrow().completion_sample(request.request),
            Some((10_000, 10))
        );
        assert!(frontend.queue.borrow_mut().take(request.request).is_some());
        assert!(frontend.queue.borrow().ready().next().is_none());
        assert!(bridge.next.is_empty());
    }
}

#[test]
fn query_failure_loses_the_bound_source_and_fake_demand_stays_in_frontend() {
    let native_source = hardware(1, 1);
    let frontend = Frontend::new(source(native_source));
    let mut bridge = SessionPresentClocks::default();
    bridge
        .service_with(&frontend, live(native_source), Instant::now(), |_| {
            LiveNativePresentClockObservation {
                lost: Some(native_source),
                current: None,
                status: LiveNativePresentClockStatus::QueryFailed,
            }
        })
        .unwrap();
    assert_eq!(*frontend.losses.borrow(), vec![source(native_source)]);
    assert!(bridge.next.is_empty());
    let frontend = Frontend::new(XPresentClockSource::Fake);
    bridge
        .service_with(&frontend, BTreeMap::new(), Instant::now(), |_| {
            panic!("a fake clock never queries hardware")
        })
        .unwrap();
    assert!(frontend.losses.borrow().is_empty());
    assert!(frontend.queue.borrow().ready().next().is_none());
    assert!(bridge.next.is_empty());
}

#[test]
fn a_thousand_field_target_uses_a_measured_wake_and_an_earlier_request_interrupts_it() {
    for interrupt in [false, true] {
        let native_source = hardware(1, 1);
        let frontend = Frontend::new(source(native_source));
        frontend.queue.borrow_mut().take(TransactionId::from_raw(1));
        let mut clock = XPresentWindowClock::default();
        clock
            .observe(
                XPresentClockSample {
                    source: source(native_source),
                    ust: 100_000,
                    msc: 10,
                },
                None,
            )
            .unwrap();
        let request = XPresentScheduledRequest {
            request: TransactionId::from_raw(1),
            kind: XPresentScheduledKind::Pixmap,
            binding: clock.binding().unwrap(),
            target: clock
                .target(XPresentMscTiming::new(1010, 0, 0, false).unwrap())
                .unwrap(),
            replaces_contents: true,
        };
        frontend.queue.borrow_mut().insert(request).unwrap();
        let mut bridge = SessionPresentClocks::default();
        let start = Instant::now();
        for (ms, msc) in [(0, 10), (10, 11)] {
            bridge
                .service_with(
                    &frontend,
                    live(native_source),
                    start + Duration::from_millis(ms),
                    |_| {
                        let mut value = observation(native_source, msc);
                        value.current.as_mut().unwrap().ust_usec = msc * 10_000;
                        value
                    },
                )
                .unwrap();
        }
        assert_eq!(bridge.queries(), 2);
        let second = start + Duration::from_millis(10);
        assert_eq!(
            bridge.cap_wait(second, Duration::from_secs(60)),
            Duration::from_millis(9970)
        );
        if interrupt {
            clock
                .observe(
                    XPresentClockSample {
                        source: source(native_source),
                        ust: 110_000,
                        msc: 11,
                    },
                    None,
                )
                .unwrap();
            frontend
                .queue
                .borrow_mut()
                .insert(XPresentScheduledRequest {
                    request: TransactionId::from_raw(2),
                    target: clock
                        .target(XPresentMscTiming::new(13, 0, 0, false).unwrap())
                        .unwrap(),
                    ..request
                })
                .unwrap();
            bridge
                .service_with(
                    &frontend,
                    live(native_source),
                    second + Duration::from_millis(1),
                    |_| {
                        let mut value = observation(native_source, 12);
                        value.current.as_mut().unwrap().ust_usec = 120_000;
                        value
                    },
                )
                .unwrap();
            assert_eq!(bridge.queries(), 3);
            assert_eq!(
                frontend.queue.borrow().ready().next().unwrap().request,
                TransactionId::from_raw(2)
            );
        } else {
            for ms in (20..9980).step_by(10) {
                bridge
                    .service_with(
                        &frontend,
                        live(native_source),
                        start + Duration::from_millis(ms),
                        |_| panic!("far-future demand must not poll once per refresh"),
                    )
                    .unwrap();
            }
            assert!(frontend.queue.borrow().ready().next().is_none());
            bridge
                .service_with(
                    &frontend,
                    live(native_source),
                    start + Duration::from_millis(9980),
                    |_| {
                        let mut value = observation(native_source, 1008);
                        value.current.as_mut().unwrap().ust_usec = 10_080_000;
                        value
                    },
                )
                .unwrap();
            assert_eq!(bridge.queries(), 3);
            assert!(frontend.queue.borrow().ready().next().is_none());
            bridge
                .service_with(
                    &frontend,
                    live(native_source),
                    start + Duration::from_millis(9990),
                    |_| {
                        let mut value = observation(native_source, 1009);
                        value.current.as_mut().unwrap().ust_usec = 10_090_000;
                        value
                    },
                )
                .unwrap();
            assert_eq!(bridge.queries(), 4);
            assert_eq!(
                frontend.queue.borrow().ready().next().unwrap().request,
                request.request
            );
        }
    }
}

#[test]
fn vrr_idle_samples_cannot_delay_the_wake_past_the_fast_mode() {
    let native_source = hardware(1, 1);
    let frontend = Frontend::new(source(native_source));
    let mut bridge = SessionPresentClocks::default();
    let start = Instant::now();
    // The nominal mode is 100 Hz, but the idle head has taken 100ms per field.
    for (ms, msc, ust) in [(0, 10, 10_000), (100, 11, 110_000)] {
        bridge
            .service_with(
                &frontend,
                live(native_source),
                start + Duration::from_millis(ms),
                |_| {
                    let mut sample = observation(native_source, msc);
                    sample.current.as_mut().unwrap().ust_usec = ust;
                    sample
                },
            )
            .unwrap();
    }
    let resumed = start + Duration::from_millis(100);
    // Eight fields to the preparation lead, wake one early: 7*10ms, not 700ms.
    assert_eq!(
        bridge.cap_wait(resumed, Duration::from_secs(1)),
        Duration::from_millis(70)
    );
    bridge
        .service_with(
            &frontend,
            live(native_source),
            resumed + Duration::from_millis(69),
            |_| panic!("no query before the bounded wake"),
        )
        .unwrap();
    assert!(frontend.queue.borrow().ready().next().is_none());
    for (ms, msc) in [(70, 18), (80, 19)] {
        bridge
            .service_with(
                &frontend,
                live(native_source),
                resumed + Duration::from_millis(ms),
                |_| {
                    let mut sample = observation(native_source, msc);
                    sample.current.as_mut().unwrap().ust_usec = 110_000 + ms * 1_000;
                    sample
                },
            )
            .unwrap();
    }
    assert_eq!(frontend.queue.borrow().ready().count(), 1);
    assert_eq!(bridge.queries(), 4);
    assert!(bridge.next.is_empty());
    // An acquire fence or egress may still block this ready request. No more
    // queries, but source loss must still turn it into a Skip exactly once.
    bridge
        .service_with(
            &frontend,
            live(native_source),
            start + Duration::from_secs(60),
            |_| panic!("a ready but blocked request must not keep vblank IRQ queries alive"),
        )
        .unwrap();
    bridge
        .service_with(
            &frontend,
            BTreeMap::new(),
            start + Duration::from_secs(60),
            |_| panic!("a lost source is not queried"),
        )
        .unwrap();
    assert_eq!(*frontend.losses.borrow(), vec![source(native_source)]);
    assert_eq!(
        frontend.queue.borrow().ready().next().unwrap().kind,
        XPresentScheduledKind::Skip
    );
}

#[test]
fn real_flips_rearm_a_pending_wake_without_queries_or_synthetic_progress() {
    let native_source = hardware(1, 1);
    let frontend = Frontend::new(source(native_source));
    let mut bridge = SessionPresentClocks::default();
    let start = Instant::now();
    for (ms, msc, ust) in [(0, 10, 10_000), (100, 11, 110_000)] {
        bridge
            .service_with(
                &frontend,
                live(native_source),
                start + Duration::from_millis(ms),
                |_| {
                    let mut sample = observation(native_source, msc);
                    sample.current.as_mut().unwrap().ust_usec = ust;
                    sample
                },
            )
            .unwrap();
    }
    // The callback pump exposes accepted real observations for the same head.
    // They can satisfy the target before another GET_SEQUENCE is needed.
    for msc in 12..=19 {
        let mut heads = live(native_source);
        let observed = XPresentClockSample {
            source: source(native_source),
            msc,
            ust: 110_000 + (msc - 11) * 10_000,
        };
        heads.get_mut(&source(native_source)).unwrap().observed = Some(observed);
        let now = start + Duration::from_micros(observed.ust - 10_000);
        bridge
            .service_with(&frontend, heads, now, |_| {
                panic!("the flip already supplies real progress")
            })
            .unwrap();
        if msc < 19 {
            assert_eq!(bridge.next[&source(native_source)].observed, observed);
            assert_eq!(
                bridge.cap_wait(now, Duration::from_secs(1)),
                Duration::from_millis((19 - msc).saturating_sub(1).max(1) * 10)
            );
            assert!(frontend.queue.borrow().ready().next().is_none());
        }
    }
    assert_eq!(bridge.queries(), 2);
    assert!(bridge.next.is_empty());
    assert_eq!(frontend.queue.borrow().ready().count(), 1);
    // Reaching the preparation lead stops queries, but accepted flips still
    // update bindings held for fences/egress or for executed completion.
    let mut heads = live(native_source);
    heads.get_mut(&source(native_source)).unwrap().observed = Some(XPresentClockSample {
        source: source(native_source),
        msc: 20,
        ust: 200_000,
    });
    for _ in 0..2 {
        bridge
            .service_with(
                &frontend,
                heads.clone(),
                start + Duration::from_millis(190),
                |_| panic!("passive clock delivery must not query"),
            )
            .unwrap();
    }
    assert_eq!(
        frontend
            .queue
            .borrow()
            .completion_sample(TransactionId::from_raw(1)),
        Some((200_000, 20))
    );
    assert_eq!(bridge.passive_observed.len(), 1);
    assert!(bridge.next.is_empty());
    frontend.queue.borrow_mut().take(TransactionId::from_raw(1));
    bridge
        .service_with(&frontend, heads, start + Duration::from_millis(200), |_| {
            panic!("settled")
        })
        .unwrap();
    assert!(bridge.passive_observed.is_empty());
}
