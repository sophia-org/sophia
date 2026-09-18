// Included into the flat acceptance facade through the lifecycle cases.

fn await_empty_cleanup(service: &LifecycleService) {
    use sophia_input_authority::CleanupReadiness;
    let mut last = String::new();
    for _ in 0..64 {
        let (reported, reading) = sync_channel(1);
        arm_runner(
            &service.registry,
            Box::new(move |runner, _| {
                let frontend = runner.frontend();
                let terminal = &frontend.terminal;
                reported
                    .send((frontend.cleanup_readiness(),format!("lifecycle={:?}, holds={}, settling={}, turn={}, delivering={}, undelivered={}, outstanding={}, transient={}, frozen={}, native_pending={}, pending={}",terminal.lifecycle.inventory(),terminal.holds.len(),terminal.settling.len(),terminal.turn.len(),terminal.delivering.len(),terminal.undelivered.len(),frontend.outstanding.len(),terminal.transients.outstanding(),terminal.frozen.len(),terminal.native_pending.is_some(),terminal.pending_custody.is_some())))
                    .unwrap();
            }),
        );
        let (readiness, detail) = reading.recv_timeout(Duration::from_secs(3)).unwrap();
        last = detail;
        if readiness == CleanupReadiness::NoneEligible {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("actual owner did not finish its cleanup obligations: {last}");
}

fn scheduler_accounting_and_maintenance() -> (Value, Vec<String>) {
    use sophia_input_authority::CleanupReadiness;
    let mut service = LifecycleService::launch("scheduler-actual-dequeues", 11007, None, false);
    service.start();
    let (mut peer, custody) = service.connect();
    let (surface, sequence, ingress) = focus_window(&service, &mut peer, 0x310701, 11070);
    await_empty_cleanup(&service);
    observe_turns(&service.registry);
    observe_dequeues(&service.registry);
    let mut positions = Vec::new();
    for (offset, pressed) in [(0, true), (1, false)] {
        if !pressed {
            std::thread::sleep(Duration::from_millis(17));
        }
        delay_next_dequeue(&service.registry);
        let accepted = ingress
            .submit(
                &service.owner.lease(),
                button_to(
                    surface,
                    XAuthorityInputDeliveryId::from_raw(11071 + offset),
                    272,
                    pressed,
                ),
            )
            .unwrap();
        positions.push(accepted);
        assert_eq!(
            read_event(&mut peer, 3),
            Some(expected_button_event(pressed, sequence, 0x310701, 1))
        );
    }
    std::thread::sleep(Duration::from_millis(17));
    // The fault is the existing unreserved ingress, actually accepted by this
    // frontend. It must park once, without repeatedly spending starts.
    let (reported, reading) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, lease| {
            let before = runner.service.usage();
            for _ in 0..3 {
                assert!(matches!(
                    runner.execute_accounted_step().unwrap(),
                    PrivateAccountedStep::Step {
                        step: PrivateOrderedStep::Idle,
                        charge: None
                    }
                ));
            }
            assert_eq!(runner.service.usage(), before);
            let parked = runner
                .frontend()
                .submit(
                    lease,
                    button_to(
                        surface,
                        XAuthorityInputDeliveryId::from_raw(11073),
                        272,
                        true,
                    ),
                )
                .unwrap();
            let result = runner.execute_accounted_step().unwrap();
            let PrivateAccountedStep::Step {
                step: PrivateOrderedStep::Parked(at),
                charge: Some(charge),
            } = result
            else {
                panic!("actual unreserved dequeue must be charged and parked");
            };
            assert_eq!(at, parked);
            let spent = runner.service.usage();
            for _ in 0..3 {
                assert!(
                    matches!(runner.execute_accounted_step().unwrap(),PrivateAccountedStep::Step {step:PrivateOrderedStep::Blocked(at),charge:None} if at==parked)
                );
            }
            assert_eq!(runner.service.usage(), spent);
            assert_eq!(
                runner.frontend().cleanup_readiness(),
                CleanupReadiness::Eligible
            );
            reported
                .send((
                    parked,
                    format!("{before:?}"),
                    format!("{spent:?}"),
                    format!("Parked({parked:?}) charged {charge:?}"),
                ))
                .unwrap();
        }),
    );
    let (parked, idle, blocked, park_step) = reading.recv_timeout(Duration::from_secs(3)).unwrap();
    let sender = registry_sender(&service.registry, custody.cleanup_record().client);
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    let turns = take_turns(&service.registry);
    let dequeues = take_dequeues(&service.registry);
    assert_eq!(dequeues.len(), 3);
    assert_eq!(
        (dequeues[0].0, dequeues[0].1),
        (positions[0], CleanupReadiness::NoneEligible)
    );
    assert_eq!(
        (dequeues[1].0, dequeues[1].1),
        (positions[1], CleanupReadiness::Eligible)
    );
    let donated = dequeues[0].2.unwrap();
    let reserved = dequeues[1].2.unwrap();
    assert!(donated.elapsed >= Duration::from_millis(3));
    assert!(reserved.elapsed >= Duration::from_millis(3));
    assert!(
        donated.cleanup_reservation_overrun.is_zero(),
        "original budget actually donated this operation's cleanup allowance"
    );
    assert!(
        !reserved.cleanup_reservation_overrun.is_zero(),
        "original budget actually preserved the eligible cleanup reservation"
    );
    assert_eq!(dequeues[2].0, parked);
    let taken: usize = turns.iter().map(|turn| turn.taken).sum();
    assert_eq!(
        taken, 2,
        "two real reserved dequeues; the third was asserted directly at the same runner hook"
    );
    assert!(
        turns
            .iter()
            .filter(|turn| turn.taken > 0)
            .all(|turn| turn.starts >= turn.taken && !turn.charged.is_zero())
    );
    let first = service.step();
    assert_eq!(first.phase, PrivateMaintenancePhase::Output);
    assert_eq!(first.instance, closed.instance);
    assert!(first.charged);
    assert_eq!(
        first.settled,
        Some(false),
        "captured sender still owns original output queue"
    );
    let second = service.step();
    assert_eq!(second.phase, PrivateMaintenancePhase::Terminal);
    assert_eq!(second.instance, closed.instance);
    assert!(second.charged);
    drop(sender);
    let mut visits = vec![first.detail, second.detail];
    let mut settled = false;
    for _ in 0..32 {
        let step = service.step();
        assert_eq!(step.instance, closed.instance);
        assert!(!matches!(
            step.status,
            PrivateMaintenanceStatus::Unavailable
                | PrivateMaintenanceStatus::SupervisionFailed
                | PrivateMaintenanceStatus::AccountingFailed
        ));
        settled |= step.settled == Some(true);
        visits.push(step.detail);
        if settled {
            break;
        }
        if step.status == PrivateMaintenanceStatus::Yielded {
            std::thread::sleep(Duration::from_millis(17));
        }
    }
    assert!(
        settled,
        "finite original visits settle the exact drained output home"
    );
    let observed = json!({"dequeues":format!("{dequeues:?}"),"turns":format!("{turns:?}"),"idle_usage":idle,"blocked_usage":blocked,"parked_step":park_step,"fault_seam":"actual runner accepts one unreserved legacy input to exercise its parked barrier","cleanup_donation":"positive same-invocation empty inventory at actual press dequeue","cleanup_reserved":"actual held press exists at actual release dequeue","maintenance_visits":visits,"residual":"output settled; the deliberately parked unreserved request remains owed"});
    (observed, service.finish(&[custody]))
}

fn scheduler_guard_supervision() -> (Value, Vec<String>) {
    let (body, release_body) = Pause::pair();
    let mut service = LifecycleService::launch(
        "scheduler-original-watch",
        11008,
        Some(AttachFault::Body {
            pause: body,
            panic: None,
        }),
        false,
    );
    service.start();
    let (mut peer, custody) = service.connect();
    let worker = release_body.entered();
    let (surface, _, ingress) = focus_window(&service, &mut peer, 0x310801, 11080);
    observe_turns(&service.registry);
    let (pause, release) = Pause::pair();
    arm_runner(&service.registry, Box::new(move |_, _| pause.wait()));
    release.entered();
    let sequence = ingress
        .submit(
            &service.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(11081),
                272,
                true,
            ),
        )
        .unwrap();
    let common = service.controller.common.lock().unwrap();
    release.release();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    assert!(
        eof_within(&mut peer, 1),
        "independent watchdog closes the actual socket while execution waits on common"
    );
    assert!(
        !custody.exit_sink().left(),
        "the controlled original worker is still running"
    );
    assert_eq!(
        custody.join().phase(),
        PrivateReapingPhase::NotBegun,
        "supervision did not try to join a stuck worker"
    );
    assert_eq!(
        custody
            .worker_slot()
            .lock()
            .unwrap()
            .handle
            .as_ref()
            .unwrap()
            .thread()
            .id(),
        worker
    );
    assert!(matches!(
        ingress.submit(
            &service.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(11082),
                272,
                true
            )
        ),
        Err(PrivateSendError::Disconnected(_))
    ));
    drop(common);
    release_body.release();
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    let turns = take_turns(&service.registry);
    let watched = turns
        .iter()
        .find(|turn| turn.unwatched == Some(sequence))
        .expect("actual guard acquisition was supervised");
    assert!(watched.charged >= Duration::from_millis(250));
    assert!(!watched.overrun.is_zero());
    assert_eq!(closed.modifiers, Some(0));
    let maintenance = service.step();
    assert_eq!(
        maintenance.status,
        PrivateMaintenanceStatus::SupervisionFailed,
        "original failed supervisor is not replaced for maintenance"
    );
    assert!(
        maintenance.charged,
        "the admitted maintenance attempt accounts its failed watchdog entry"
    );
    assert_eq!(maintenance.settled, None);
    let observed = json!({"fault_seam":"actual worker body paused; original common guard held across execution start","worker":format!("{worker:?}"),"sequence":format!("{sequence:?}"),"watched_turn":format!("{watched:?}"),"collection":format!("{:?}",closed.workers),"maintenance":maintenance.detail});
    (observed, service.finish(&[custody]))
}

#[test]
fn d_scheduler() {
    let (accounting, mut actors) = scheduler_accounting_and_maintenance();
    let (watch, watched_actors) = scheduler_guard_supervision();
    actors.extend(watched_actors);
    emit_case(
        "D.scheduler",
        &[
            ("every_dequeue_charged", accounting.clone()),
            ("idle_and_blocked_no_start", accounting.clone()),
            ("cleanup_reserve_and_donation", accounting.clone()),
            ("guard_acquisition_watched", watch.clone()),
            ("supervisor_does_not_join_stuck_worker", watch),
            ("finite_retained_maintenance", accounting),
        ],
        &actors,
    );
}
