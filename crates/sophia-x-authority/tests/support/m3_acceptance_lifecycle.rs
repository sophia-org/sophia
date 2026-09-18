// Integrated private-service lifecycle cases. Scheduling faults are explicit;
// service preparation, producer issuance, registrations, workers, collection
// and post-collection maintenance use the actual production owners.

#[test]
fn d_ready_before_exposure() {
    let mut service = LifecycleService::launch("readiness-and-revocation", 11000, None, false);
    assert_eq!(service.access.standing(), PrivatePortStanding::NotReady);
    assert!(matches!(
        service.access.control_producer(&service.owner.lease()),
        Err(PrivateProducerRefusal::NotReady)
    ));
    assert!(matches!(
        service.access.ingress_for(
            &service.owner.lease(),
            XServerFrontendClientId(1),
            DeviceId::from_raw(1)
        ),
        Err(PrivateProducerRefusal::NotReady)
    ));
    service.start();
    assert_eq!(service.access.standing(), PrivatePortStanding::Ready);
    let (mut peer, custody) = service.connect();
    let (surface, sequence, ingress) = focus_window(&service, &mut peer, 0x310101, 11000);
    let client = custody.cleanup_record().client;
    let admission = service
        .participant
        .bindings
        .lock()
        .unwrap()
        .bound
        .get(&client)
        .unwrap()
        .admission;
    let (pause, release) = Pause::pair();
    observe_turns(&service.registry);
    arm_runner(&service.registry, Box::new(move |_, _| pause.wait()));
    let runner = release.entered();
    let position = ingress
        .submit(
            &service.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(11001),
                272,
                true,
            ),
        )
        .unwrap();
    let revoked = service
        .participant
        .revoke_admission(client, admission)
        .unwrap();
    assert_eq!(revoked.closed, 1);
    assert_eq!(revoked.retired, 1);
    release.release();
    assert!(waited_for(|| delivery_cell(&service.registry, 11001)
        .and_then(|cell| cell.answer())
        .is_some()));
    let answer = delivery_cell(&service.registry, 11001)
        .unwrap()
        .answer()
        .unwrap();
    assert_ne!(answer.outcome, XAuthorityInputDeliveryOutcome::Flushed);
    assert!(matches!(
        ingress.submit(
            &service.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(11002),
                272,
                true
            )
        ),
        Err(_)
    ));
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    let turns = take_turns(&service.registry);
    assert!(
        turns
            .iter()
            .any(|turn| turn.refused > 0 && turn.last_refusal.is_some())
    );
    assert_eq!(
        closed.order.unwrap().dispatched,
        0,
        "revoked input never reaches native output dispatch"
    );
    let observation = json!({"client":client.0,"surface":format!("{surface:?}"),"focus_sequence":sequence,"position":format!("{position:?}"),"runner":format!("{runner:?}"),"revocation":format!("{revoked:?}"),"answer":format!("{answer:?}"),"turns":format!("{turns:?}")});
    let actors = service.finish(&[custody]);
    emit_case(
        "D.ready_before_exposure",
        &[
            (
                "not_ready_refuses",
                json!({"control":"NotReady","ingress":"NotReady"}),
            ),
            ("ready_exposes_real_producers", observation.clone()),
            ("execution_checks_admission_and_revocation", observation),
        ],
        &actors,
    );
}

#[test]
fn d_start_failures() {
    let mut actors = Vec::new();
    let mut facts = Vec::new();
    for (label, fault, expected, started) in [
        (
            "spawn_refused",
            AttachFault::SpawnRefused,
            PrivateStartupOutcome::SpawnRefused,
            false,
        ),
        (
            "permit_refused",
            AttachFault::PermitRefused,
            PrivateStartupOutcome::PermitRefused,
            true,
        ),
    ] {
        let mut service = LifecycleService::launch(label, 11001, Some(fault), false);
        service.start();
        let (peer, custody) = service.connect();
        let attachment = waited_for_value(|| custody.attachment()).unwrap();
        assert_eq!(
            attachment,
            PrivateAttachment::Refused(PrivateAttachmentRefusal::Startup(expected))
        );
        let identity = custody_identity(&custody);
        let before = observe_worker(&custody, &service.registry);
        assert_eq!(before.handle_in_slot, started);
        assert_eq!(
            before.life,
            if started {
                PrivateWorkerLife::Running
            } else {
                PrivateWorkerLife::NeverStarted
            }
        );
        assert_eq!(before.join_phase, PrivateReapingPhase::NotBegun);
        let handle = custody
            .worker_slot()
            .lock()
            .unwrap()
            .handle
            .as_ref()
            .map(|handle| handle.thread().id());
        service.command(XServerFrontendServiceCommand::StopAndDisconnect);
        let closed = service.closed();
        assert!(closed.succeeded);
        assert_eq!(custody_identity(&custody), identity);
        assert_eq!(
            custody.join().phase(),
            if started {
                PrivateReapingPhase::Joined
            } else {
                PrivateReapingPhase::NotBegun
            }
        );
        if started {
            assert!(matches!(
                custody.join().result(),
                Some(PrivateJoinResult::Returned)
            ));
        }
        facts.push(json!({"fault_seam":label,"attachment":format!("{attachment:?}"),"before":format!("{before:?}"),"actual_handle":format!("{handle:?}"),"collection":format!("{:?}",closed.workers),"identity":format!("{identity:?}")}));
        drop(peer);
        actors.extend(service.finish(&[custody]));
    }

    let (pause, release) = Pause::pair();
    let mut service = LifecycleService::launch(
        "departure-during-spawn",
        11002,
        Some(AttachFault::DuringSpawn(pause)),
        false,
    );
    service.start();
    let (peer, custody) = service.connect();
    let source = release.entered();
    let identity = custody_identity(&custody);
    assert!(
        custody.worker_slot().try_lock().is_err(),
        "actual startup holds its slot"
    );
    let (stop, _) = custody.bound_pair().unwrap();
    peer.shutdown(std::net::Shutdown::Both).unwrap();
    assert!(
        waited_for(|| stop.load(Ordering::Acquire)),
        "registration departure reaches stop before waiting for the startup slot"
    );
    assert_eq!(
        custody.cleanup_record().destruction_standing(),
        PrivateDestructionStanding::Requested
    );
    release.release();
    let attachment = waited_for_value(|| custody.attachment()).unwrap();
    assert_eq!(attachment, PrivateAttachment::Started);
    let handle = custody
        .worker_slot()
        .lock()
        .unwrap()
        .handle
        .as_ref()
        .unwrap()
        .thread()
        .id();
    assert_eq!(custody.join().phase(), PrivateReapingPhase::NotBegun);
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    assert_eq!(custody_identity(&custody), identity);
    assert!(matches!(
        custody.join().result(),
        Some(PrivateJoinResult::Returned)
    ));
    let departure = json!({"fault_seam":"actual spawn closure paused while owning startup slot","source":format!("{source:?}"),"handle":format!("{handle:?}"),"identity":format!("{identity:?}"),"collection":format!("{:?}",closed.workers)});
    actors.extend(service.finish(&[custody]));
    emit_case(
        "D.start_failures",
        &[
            ("spawn_refused", facts[0].clone()),
            ("failure_after_spawn", facts[1].clone()),
            ("departure_during_startup", departure.clone()),
            (
                "exact_handle_and_attempt_phase",
                json!({"unattempted_and_attempted":facts,"departing_attempt":departure}),
            ),
        ],
        &actors,
    );
}

#[test]
fn d_worker_exit() {
    let mut actors = Vec::new();
    let mut facts = Vec::new();
    for panics in [false, true] {
        let (pause, release) = Pause::pair();
        let payload = Arc::new(PanicIdentity(0xd0e1));
        let mut service = LifecycleService::launch(
            "actual-worker-exit",
            11003,
            Some(AttachFault::Body {
                pause,
                panic: panics.then(|| Arc::clone(&payload)),
            }),
            false,
        );
        service.start();
        let (peer, custody) = service.connect();
        let worker = release.entered();
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
        assert_eq!(custody.exit_sink().reading(), PrivateExitReading::NotLeft);
        assert_eq!(custody.join().phase(), PrivateReapingPhase::NotBegun);
        if !panics {
            peer.shutdown(std::net::Shutdown::Both).unwrap();
            let (stop, _) = custody.bound_pair().unwrap();
            assert!(waited_for(|| stop.load(Ordering::Acquire)));
        }
        release.release();
        assert!(waited_for(|| custody.exit_sink().left()));
        let departure = custody.exit_sink().reading();
        if panics {
            assert_eq!(departure, PrivateExitReading::Unclassified);
        } else {
            assert!(matches!(departure, PrivateExitReading::Classified(_)));
        }
        assert_eq!(
            custody.join().phase(),
            PrivateReapingPhase::NotBegun,
            "departure alone is not a join"
        );
        service.command(XServerFrontendServiceCommand::StopAndDisconnect);
        let closed = service.closed();
        assert_eq!(custody.join().phase(), PrivateReapingPhase::Joined);
        match custody.join().result().unwrap() {
            PrivateJoinResult::Returned => assert!(!panics),
            PrivateJoinResult::Panicked(held) => {
                assert!(panics);
                let held = held.lock().unwrap();
                let exact = held
                    .downcast_ref::<Arc<PanicIdentity>>()
                    .expect("original typed payload");
                assert!(Arc::ptr_eq(exact, &payload));
                assert_eq!(exact.0, 0xd0e1);
            }
        }
        facts.push(json!({"fault_seam":"actual worker body after leaving guard is armed","panics":panics,"worker":format!("{worker:?}"),"departure_before_join":format!("{departure:?}"),"collection":format!("{:?}",closed.workers),"payload_pointer":format!("{:p}",Arc::as_ptr(&payload))}));
        actors.extend(service.finish(&[custody]));
    }
    emit_case(
        "D.worker_exit",
        &[
            ("ordinary_return", facts[0].clone()),
            ("worker_unwind", facts[1].clone()),
            ("published_join_and_exact_panic", json!(facts)),
        ],
        &actors,
    );
}

#[test]
fn d_registration_destruction() {
    let (pause, release) = Pause::pair();
    let mut service = LifecycleService::launch(
        "registration-over-live-worker",
        11004,
        Some(AttachFault::Body { pause, panic: None }),
        false,
    );
    service.start();
    let (peer, custody) = service.connect();
    let worker = release.entered();
    let identity = custody_identity(&custody);
    peer.shutdown(std::net::Shutdown::Both).unwrap();
    assert!(waited_for(|| matches!(
        custody.cleanup_record().destruction_standing(),
        PrivateDestructionStanding::Decided(_)
    )));
    let before = observe_worker(&custody, &service.registry);
    assert_eq!(
        before.standing,
        PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(
            PrivateDestructionDeferral::WorkerRunning
        ))
    );
    assert!(!before.left);
    assert!(before.handle_in_slot);
    assert_eq!(before.number, Some(PrivateNumberStanding::Held));
    assert_eq!(before.join_phase, PrivateReapingPhase::NotBegun);
    let (stop, _) = custody.bound_pair().unwrap();
    assert!(stop.load(Ordering::Acquire));
    assert_eq!(live_custody_identity(&service.registry), Some(identity));
    release.release();
    service.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = service.closed();
    assert_eq!(custody_identity(&custody), identity);
    assert_eq!(custody.join().phase(), PrivateReapingPhase::Joined);
    assert!(matches!(
        custody.join().result(),
        Some(PrivateJoinResult::Returned)
    ));
    let maintenance = service.step();
    assert_eq!(maintenance.instance, closed.instance);
    let observed = json!({"worker":format!("{worker:?}"),"identity":format!("{identity:?}"),"before":format!("{before:?}"),"collected":format!("{:?}",closed.workers),"original_maintenance":format!("{maintenance:?}")});
    let actors = service.finish(&[custody]);
    emit_case(
        "D.registration_destruction",
        &[
            ("controlled_live_worker", observed.clone()),
            ("stop_remains_reachable", json!({"exact_bound_stop":true})),
            ("custody_through_collection", observed),
        ],
        &actors,
    );
}
