//! REVIEW-08: actual authority/service boundaries with injected clocks.
use super::*;

struct OtherWindow {
    window: XResourceId,
    surface: SurfaceId,
    client: XServerFrontendClientId,
    preparation: TransactionId,
    _registration: XServerFrontendClientRouteRegistration,
    _channels: XServerFrontendClientRouteChannels,
}

fn add_window(fixture: &Fixture, number: u32) -> OtherWindow {
    let window = XResourceId::new(0x500000 + u64::from(number), 1);
    let surface = SurfaceId::new(298 + number, 1);
    let client = XServerFrontendClientId::from_raw(u64::from(number) + 1);
    let (registration, channels) = fixture.broker.registry.register_client(client).unwrap();
    fixture
        .broker
        .registry
        .register_surface(client, NS, surface, window)
        .unwrap();
    let preparation = fixture.state.allocate_transaction().unwrap();
    {
        let mut runtime = fixture.state.runtime.lock().unwrap();
        assert_eq!(
            runtime
                .apply(XAuthorityRequestPacket {
                    transaction: preparation,
                    namespace: NS,
                    kind: XAuthorityRequestKind::CreateWindow {
                        window,
                        surface,
                        geometry: Rect {
                            x: 0,
                            y: 0,
                            width: 4,
                            height: 4
                        },
                        constraints: SurfaceConstraints {
                            min_size: None,
                            max_size: None
                        },
                        generation: 1,
                    },
                })
                .outcome,
            crate::XAuthorityResponseOutcome::Accepted
        );
        runtime.begin_dispatch();
        runtime
            .prepare_standard_pixmap(
                client.raw(),
                preparation,
                NS,
                window,
                PIXMAP,
                (0, 0),
                None,
                None,
                Default::default(),
            )
            .unwrap();
    }
    fixture
        .broker
        .registry
        .queue_present(
            preparation,
            client,
            window,
            PIXMAP,
            preparation.raw() as u32,
            None,
            false,
        )
        .unwrap();
    OtherWindow {
        window,
        surface,
        client,
        preparation,
        _registration: registration,
        _channels: channels,
    }
}

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

#[test]
fn timed_present_one_windows_bad_sample_does_not_stop_another_window() {
    let fixture = Fixture::new(true);
    fixture.schedule(hardware(50), 60);
    let other = add_window(&fixture, 1);
    fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .schedule_prepared_present(
            other.preparation,
            crate::XPresentMscTiming::new(46, 0, 0, false).unwrap(),
            hardware(40),
            None,
        )
        .unwrap();
    let router = fixture.broker.present_clock_router();
    // Regresses window A, advances B to its composition lead. A is unchanged.
    router.observe_source(hardware(45)).unwrap();
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .present_timing_statistics()
            .clock_stale_samples,
        1
    );
    let mut generated = XGeneratedEgress::default();
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 45_000
        )
        .unwrap()
    );
    let batch = generated.take().next().unwrap().batch.unwrap();
    assert_eq!(batch.software_present_submissions[0].surface, other.surface);
    assert!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_origin(fixture.preparation)
            .is_some()
    );
    assert!(
        !service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 46_000
        )
        .unwrap()
    );
    router.observe_source(hardware(59)).unwrap();
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 59_000
        )
        .unwrap()
    );
    assert_eq!(
        generated
            .take()
            .next()
            .unwrap()
            .batch
            .unwrap()
            .software_present_submissions[0]
            .surface,
        SURFACE
    );
}

#[test]
fn timed_present_surface_observation_rejects_the_whole_window_update() {
    let fixture = Fixture::new(true);
    fixture.schedule(hardware(10), 20);
    let router = fixture.broker.present_clock_router();
    let next = crate::XPresentClockSample {
        source: crate::XPresentClockSource::Hardware {
            domain: 2,
            incarnation: 1,
        },
        ust: 12_000,
        msc: 200,
    };
    router
        .observe(
            SURFACE,
            next,
            Some(crate::XPresentClockSample {
                source: crate::XPresentClockSource::Fake,
                ust: 11_000,
                msc: 11,
            }),
        )
        .unwrap();
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .present_timing_statistics()
            .clock_wrong_sources,
        1
    );
    // A later valid observation still uses the old clock. No partial rebind.
    router.observe(SURFACE, hardware(19), None).unwrap();
    let mut generated = XGeneratedEgress::default();
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 19_000
        )
        .unwrap()
    );
}

fn install_wait_fence(
    fixture: &Fixture,
    sample: crate::XPresentClockSample,
    target: u64,
) -> (XResourceId, Arc<std::os::fd::OwnedFd>) {
    let fence = XResourceId::new(0x400003, 1);
    let fd = Arc::new(sophia_xshmfence::allocate().unwrap());
    {
        let mut runtime = fixture.state.runtime.lock().unwrap();
        runtime.cancel_prepared_standard_pixmap(fixture.preparation);
        runtime.create_dri3_fence(NS, fence, 1).unwrap();
        let handle = runtime.dri3_fence_handle(NS, fence).unwrap();
        runtime
            .retain_present_fence_descriptor(handle, fd.clone())
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
                    wait: Some(fence),
                    idle: None,
                },
            )
            .unwrap();
    }
    fixture.schedule(sample, target);
    (fence, fd)
}

#[test]
fn timed_present_never_triggered_fence_backs_off_despite_other_wakes() {
    let fixture = Fixture::new(true);
    let (_, fd) = install_wait_fence(&fixture, hardware(10), 11);
    let mut generated = XGeneratedEgress::default();
    let origin = Instant::now();
    // Simulate unrelated traffic every 100 us for a second. Neither traffic
    // nor the ready MSC may restart the fence timer or allocate a ticket.
    for now in (10_000..1_010_000).step_by(100) {
        assert!(
            !service_timed_presents(
                &fixture.state,
                &fixture.broker.registry,
                &mut generated,
                || now
            )
            .unwrap()
        );
        let deadline = timed_present_service_deadline(&fixture.state, now, origin, false, true)
            .unwrap()
            .unwrap();
        assert!(deadline > origin);
        assert!(deadline <= origin + Duration::from_millis(32));
    }
    let stats = fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .present_timing_statistics();
    assert!((30..=37).contains(&stats.fence_queries), "{stats:?}");
    assert_eq!(stats.fence_queries, stats.fence_blocked);
    assert!(!generated.pending());
    assert_eq!(fixture.state.next_transaction_id.load(Ordering::Relaxed), 2);
    // A shared-memory trigger, with no X request, is observed within the cap.
    sophia_xshmfence::trigger(&fd).unwrap();
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 1_042_000
        )
        .unwrap()
    );
    assert_eq!(generated.take().next().unwrap().transaction.raw(), 2);
}

#[test]
fn timed_present_fence_waiter_is_not_scrapped_or_allowed_to_hide_a_future_deadline() {
    let fixture = Fixture::new(true);
    let (fence, _) = install_wait_fence(&fixture, crate::XPresentClockSample::background(0), 1);
    let mut generated = XGeneratedEgress::default();
    assert!(
        !service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 1_000_000
        )
        .unwrap()
    );
    let equal = fixture.prepare_next(PIXMAP, false);
    // Use the frozen original sample to give exactly the same fake target.
    // The old MSC event has been serviced, so its fence wait is not scrapped.
    // Observe at the current sample and async=0 distance selects current MSC.
    assert!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .schedule_prepared_present(
                equal,
                crate::XPresentMscTiming::new(1, 0, 0, true).unwrap(),
                crate::XPresentClockSample::background(1_000_000),
                None
            )
            .unwrap()
            .is_empty()
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
    assert_eq!(generated.take().next().unwrap().transaction.raw(), 3);
    let future = fixture.prepare_next(PIXMAP, true);
    fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .schedule_prepared_present(
            future,
            crate::XPresentMscTiming::new(2, 0, 0, false).unwrap(),
            crate::XPresentClockSample::background(1_000_000),
            None,
        )
        .unwrap();
    for now in [
        1_001_000, 1_003_000, 1_007_000, 1_015_000, 1_031_000, 1_999_500,
    ] {
        assert!(
            !service_timed_presents(
                &fixture.state,
                &fixture.broker.registry,
                &mut generated,
                || now
            )
            .unwrap()
        );
    }
    let origin = Instant::now();
    assert_eq!(
        timed_present_service_deadline(&fixture.state, 1_999_500, origin, false, true).unwrap(),
        Some(origin + Duration::from_micros(500))
    );
    // DestroyFence clears its private handle and bypasses the old backoff.
    fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .destroy_dri3_fence(NS, fence)
        .unwrap();
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 1_999_500
        )
        .unwrap()
    );
    generated.take().for_each(drop);
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 2_000_000
        )
        .unwrap()
    );
}

#[test]
fn timed_present_capacity_wait_releases_locks_needed_by_execution() {
    let fixture = Fixture::new(true);
    for _ in 1..crate::X_PRESENT_PER_CLIENT_CAPACITY {
        fixture.prepare_next(PIXMAP, false);
    }
    let next = fixture.state.allocate_transaction().unwrap();
    let registry = fixture.broker.registry.clone();
    let (done, completion) = sync_channel(1);
    let waiter = std::thread::spawn(move || {
        done.send(registry.queue_present(next, CLIENT, WINDOW, PIXMAP, 999, None, false))
            .unwrap();
    });
    assert!(waited_for(|| fixture
        .broker
        .registry
        .pending_presentations
        .capacity_waits
        .load(Ordering::Relaxed)
        != 0));
    assert!(completion.try_recv().is_err());
    // The capacity waiter must not hold any of these locks while sleeping.
    // Timed execution uses exactly this nested order to transfer a slot.
    {
        let _runtime = fixture
            .state
            .runtime
            .try_lock()
            .expect("capacity wait holds runtime");
        let _clients = fixture
            .broker
            .registry
            .clients
            .try_lock()
            .expect("capacity wait holds clients");
        let mut pending = fixture
            .broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap();
        pending.remove(&fixture.preparation).unwrap();
        fixture
            .broker
            .registry
            .pending_presentations
            .capacity_changed
            .notify_all();
    }
    completion
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    waiter.join().unwrap();
    assert_eq!(
        fixture
            .broker
            .registry
            .pending_presentations
            .entries
            .lock()
            .unwrap()
            .len(),
        64
    );
}

#[test]
fn timed_present_multiple_clients_execute_without_owner_round_trips() {
    const CLIENTS: usize = 8;
    const PER_CLIENT: usize = 4;
    const TOTAL: usize = CLIENTS * PER_CLIENT;
    let fixture = Fixture::new(true);
    let others = (1..CLIENTS)
        .map(|n| add_window(&fixture, n as u32))
        .collect::<Vec<_>>();
    let windows = std::iter::once((CLIENT, WINDOW, fixture.preparation))
        .chain(others.iter().map(|w| (w.client, w.window, w.preparation)))
        .collect::<Vec<_>>();
    let mut runtime = fixture.state.runtime.lock().unwrap();
    for &(client, window, preparation) in &windows {
        runtime
            .schedule_prepared_present(
                preparation,
                crate::XPresentMscTiming::new(101, 0, 0, false).unwrap(),
                hardware(100),
                None,
            )
            .unwrap();
        for field in 102..=104 {
            let id = fixture.state.allocate_transaction().unwrap();
            runtime
                .prepare_standard_pixmap(
                    client.raw(),
                    id,
                    NS,
                    window,
                    PIXMAP,
                    (0, 0),
                    None,
                    None,
                    Default::default(),
                )
                .unwrap();
            runtime
                .schedule_prepared_present(
                    id,
                    crate::XPresentMscTiming::new(field, 0, 0, false).unwrap(),
                    hardware(100),
                    None,
                )
                .unwrap();
        }
    }
    let ready = runtime.ready_prepared_presents();
    assert_eq!(ready.len(), 8); // next-field hardware targets get one-field lead
    drop(runtime);
    // Admission reservations are deliberately made outside the runtime lock.
    for id in 9..=TOTAL as u64 {
        let origin = fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_origin(TransactionId::from_raw(id))
            .unwrap();
        fixture
            .broker
            .registry
            .queue_present(
                TransactionId::from_raw(id),
                XServerFrontendClientId::from_raw(origin.0),
                origin.2,
                PIXMAP,
                id as u32,
                None,
                false,
            )
            .unwrap();
    }
    fixture
        .broker
        .present_clock_router()
        .observe_source(hardware(104))
        .unwrap();
    let path = private_service_socket("timed-multiple-clients");
    let wake = sophia_wake::WakeSlot::default();
    let mut frontend = XServerFrontend::bind(
        XServerFrontendConfig::new(&path, NS)
            .unwrap()
            .with_service_wake(wake.clone()),
    )
    .unwrap();
    frontend.state = fixture.state.clone();
    let (sender, output) = sync_channel(TOTAL);
    let egress =
        XAuthorityOrderedEgress::new(sender, Arc::new(AtomicBool::new(false)), Arc::new(|_| {}));
    // Empty preparation tickets have already crossed ordinary request order.
    for id in 1..=TOTAL as u64 {
        egress
            .submit_blocking(XAuthorityBoundedEgressEnvelope::new(
                TransactionId::from_raw(id),
                None,
            ))
            .unwrap();
    }
    let (commands, inbox) = sync_channel(1);
    let commands = sophia_wake::SignalSender::new(commands, wake.clone());
    let state = fixture.state.clone();
    let started = Instant::now();
    let worker = std::thread::spawn(move || {
        let mut broker = fixture.broker;
        let observer: Arc<X11CoreTraceObserver> = Arc::new(|_| Ok(None));
        let mut generated = XGeneratedEgress::default();
        drive_routed_service(
            &mut frontend,
            &mut broker,
            &inbox,
            &egress,
            &observer,
            &mut generated,
        )
        .unwrap();
        (generated.admission_passes, egress.report().unwrap())
    });
    // Do not consume or acknowledge ANY output until the entire offered
    // batch executes. A required owner round trip would stall this fixture.
    assert!(waited_for(|| state
        .next_transaction_id
        .load(Ordering::Relaxed)
        == (2 * TOTAL + 1) as u64));
    let elapsed = started.elapsed();
    let mut completed = BTreeMap::<XServerFrontendClientId, usize>::new();
    for index in 0..TOTAL {
        let batch = output.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(batch.transaction.raw(), (TOTAL + index + 1) as u64);
        assert_eq!(batch.software_present_submissions.len(), 1);
        *completed.entry(batch.client.unwrap()).or_default() += 1;
    }
    commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let (passes, report) = worker.join().unwrap();
    assert_eq!(completed.len(), CLIENTS);
    assert!(completed.values().all(|n| *n == PER_CLIENT));
    assert_eq!(report.batches_delivered, TOTAL as u64);
    assert!(
        (TOTAL as u64..=TOTAL as u64 + 1).contains(&passes),
        "{passes}"
    );
    println!(
        "timed_service_fixture clients={CLIENTS} offered={TOTAL} executed={TOTAL} passes={passes} elapsed_usec={}",
        elapsed.as_micros()
    );
    std::fs::remove_file(path).ok();
}

#[test]
fn timed_present_repeated_clock_rejections_settle_only_the_failed_binding_once() {
    let fixture = Fixture::new(true);
    fixture.schedule(hardware(10), 100);
    fixture
        .broker
        .registry
        .select_present_input(CLIENT, XResourceId::new(0x40000a, 1), WINDOW, 6)
        .unwrap();
    let router = fixture.broker.present_clock_router();
    // Valid same-source progress resets the streak; unrelated fake service
    // passes do not. Three consecutive bad observations lose the binding.
    for sample in [hardware(9), hardware(10), hardware(9), hardware(9)] {
        router.observe_source(sample).unwrap();
        let mut generated = XGeneratedEgress::default();
        assert!(
            !service_timed_presents(
                &fixture.state,
                &fixture.broker.registry,
                &mut generated,
                || 50_000
            )
            .unwrap()
        );
    }
    router.observe_source(hardware(9)).unwrap();
    let mut generated = XGeneratedEgress::default();
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 50_000
        )
        .unwrap()
    );
    assert!(matches!(
        fixture._channels.protocol.try_recv().unwrap(),
        XClientEvent::PresentIdleNotify { serial: 41, .. }
    ));
    assert!(
        matches!(fixture._channels.protocol.try_recv().unwrap(), XClientEvent::PresentCompleteNotify { serial: 41, msc: 10, mode, .. } if mode == XPresentCompletionMode::Skip as u8)
    );
    for _ in 0..10 {
        router.observe_source(hardware(9)).unwrap();
    }
    assert!(
        !service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 50_000
        )
        .unwrap()
    );
    assert!(fixture._channels.protocol.try_recv().is_err());
    let stats = fixture
        .state
        .runtime
        .lock()
        .unwrap()
        .present_timing_statistics();
    assert_eq!(stats.clock_stale_samples, 4);
    assert_eq!(stats.clock_sources_lost, 1);
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .prepared_present_count(),
        0
    );
    assert_eq!(fixture.state.next_transaction_id.load(Ordering::Relaxed), 2);
}

#[test]
fn timed_present_full_egress_never_turns_ready_work_into_a_busy_wait() {
    let fixture = Fixture::new(true);
    fixture.schedule(hardware(10), 11);
    for field in [12, 13] {
        let request = fixture.prepare_next(PIXMAP, true);
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .schedule_prepared_present(
                request,
                crate::XPresentMscTiming::new(field, 0, 0, false).unwrap(),
                hardware(10),
                None,
            )
            .unwrap();
    }
    fixture
        .broker
        .present_clock_router()
        .observe_source(hardware(13))
        .unwrap();
    let path = private_service_socket("timed-full-receiver");
    let wake = sophia_wake::WakeSlot::default();
    let mut frontend = XServerFrontend::bind(
        XServerFrontendConfig::new(&path, NS)
            .unwrap()
            .with_service_wake(wake.clone()),
    )
    .unwrap();
    frontend.state = fixture.state.clone();
    let (sender, _output) = sync_channel(1);
    let egress =
        XAuthorityOrderedEgress::new(sender, Arc::new(AtomicBool::new(false)), Arc::new(|_| {}));
    for id in 1..=3 {
        egress
            .submit_blocking(XAuthorityBoundedEgressEnvelope::new(
                TransactionId::from_raw(id),
                None,
            ))
            .unwrap();
    }
    let (commands, inbox) = sync_channel(1);
    let commands = sophia_wake::SignalSender::new(commands, wake.clone());
    let state = fixture.state.clone();
    let worker = std::thread::spawn(move || {
        let mut broker = fixture.broker;
        let observer: Arc<X11CoreTraceObserver> = Arc::new(|_| Ok(None));
        let mut generated = XGeneratedEgress::default();
        drive_routed_service(
            &mut frontend,
            &mut broker,
            &inbox,
            &egress,
            &observer,
            &mut generated,
        )
        .unwrap();
        generated.admission_passes
    });
    assert!(waited_for(|| state
        .next_transaction_id
        .load(Ordering::Relaxed)
        == 6));
    // One delivered envelope fills the channel, one is retained by the
    // service, and the third ready request must not cause a zero-time wait.
    let origin = Instant::now();
    assert_eq!(
        timed_present_service_deadline(&state, 13_000, origin, true, false).unwrap(),
        Some(origin + Duration::from_millis(1))
    );
    std::thread::sleep(Duration::from_millis(50));
    commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let passes = worker.join().unwrap();
    assert!(
        passes < 256,
        "ready request spun for {passes} passes while egress was full"
    );
    println!("timed_full_receiver held_msec=50 passes={passes}");
    std::fs::remove_file(path).ok();
}

#[test]
fn timed_present_msc_serviced_under_backpressure_is_not_scrappable() {
    let fixture = Fixture::new(true);
    fixture.schedule(hardware(10), 11);
    let mut generated = XGeneratedEgress::default();
    generated.insert(
        XGeneratedEgressKind::Present,
        XAuthorityBoundedEgressEnvelope::new(TransactionId::from_raw(99), None),
    );
    assert!(
        !service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 10_000
        )
        .unwrap()
    );
    let replacement = fixture.prepare_next(PIXMAP, false);
    assert!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .schedule_prepared_present(
                replacement,
                crate::XPresentMscTiming::new(11, 0, 0, false).unwrap(),
                hardware(10),
                None
            )
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .state
            .runtime
            .lock()
            .unwrap()
            .ready_prepared_presents(),
        vec![fixture.preparation, replacement]
    );
}

#[test]
fn timed_present_losing_an_already_ready_fence_waiter_still_wakes_service() {
    let fixture = Fixture::new(true);
    let _fence = install_wait_fence(&fixture, hardware(10), 11);
    let mut generated = XGeneratedEgress::default();
    assert!(
        !service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 10_000
        )
        .unwrap()
    );
    let wake = sophia_wake::Wake::new().unwrap();
    fixture.broker.registry.service_wake.set(wake.notifier());
    let router = fixture.broker.present_clock_router();
    for _ in 0..3 {
        router.observe_source(hardware(9)).unwrap();
    }
    let mut fds = [rustix::event::PollFd::new(
        &wake,
        rustix::event::PollFlags::IN,
    )];
    sophia_wake::wait(&mut fds, Some(Instant::now())).unwrap();
    assert!(fds[0].revents().contains(rustix::event::PollFlags::IN));
    assert!(
        service_timed_presents(
            &fixture.state,
            &fixture.broker.registry,
            &mut generated,
            || 10_000
        )
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
