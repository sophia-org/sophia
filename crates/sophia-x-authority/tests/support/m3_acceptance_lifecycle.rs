// Integrated private-service lifecycle cases. Scheduling faults are explicit;
// service preparation, producer issuance, registrations, workers, collection
// and post-collection maintenance use the actual production owners.

include!("m3_acceptance_scheduler.rs");

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
    assert!(
        ingress
            .submit(
                &service.owner.lease(),
                button_to(
                    surface,
                    XAuthorityInputDeliveryId::from_raw(11002),
                    272,
                    true
                )
            )
            .is_err()
    );
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
        // DEPARTURE ALONE IS NOT A JOIN, AND THIS IS WHERE THAT IS PINNED.
        // The worker is running, so a join begun by now could only have come
        // from the departure path -- which is the claim. Below, once the
        // worker has exited, it no longer can be: the service frame reclaims
        // departed connections on any idle turn and may join an exited worker
        // at once, so a NotBegun after the exit would pin the scheduler
        // rather than the departure.
        assert_eq!(
            custody.join().phase(),
            PrivateReapingPhase::NotBegun,
            "nothing joined this worker while it was still running"
        );
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

#[test]
fn d_service_exit() {
    let mut actors = Vec::new();
    let mut facts = Vec::new();
    for kind in ["shutdown", "command_channel_loss", "error", "unwind"] {
        let mut service = LifecycleService::launch(kind, 11005, None, kind == "unwind");
        service.start();
        let (mut peer, custody) = service.connect();
        let (surface, sequence, ingress) = focus_window(&service, &mut peer, 0x310501, 11050);
        // The original key source requires pointer geometry, established by
        // an actual leased motion and its independently checked wire event.
        ingress
            .submit(
                &service.owner.lease(),
                motion_to(surface, XAuthorityInputDeliveryId::from_raw(11049)),
            )
            .unwrap();
        let mut motion = expected_button_event(true, sequence, 0x310501, 1);
        motion[0] = 6;
        motion[1] = 0;
        assert_eq!(read_event(&mut peer, 3), Some(motion));
        assert_eq!(
            service
                .deliveries
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .delivery,
            XAuthorityInputDeliveryId::from_raw(11049)
        );
        ingress
            .submit(
                &service.owner.lease(),
                key_service_route(surface, 11051, 42, true),
            )
            .unwrap();
        let expected = expected_key_service_event(sequence, 0x310501, 50, true, 0);
        assert_eq!(read_event(&mut peer, 3), Some(expected));
        let delivered = service
            .deliveries
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert_eq!(
            delivered.delivery,
            XAuthorityInputDeliveryId::from_raw(11051)
        );
        assert_eq!(delivered.outcome, XAuthorityInputDeliveryOutcome::Flushed);
        let identity = custody_identity(&custody);
        let admission = pause_after_admission_closed(&service.registry);
        let frame =
            pause_after_registration_drop(&service.registry, custody.cleanup_record().client);
        match kind {
            "shutdown" => service.command(XServerFrontendServiceCommand::StopAndDisconnect),
            "command_channel_loss" => drop(service.commands.take()),
            "error" => {
                let (acknowledgement, acknowledged) = sync_channel(1);
                drop(acknowledged);
                service.command(XServerFrontendServiceCommand::UpdateOutputTopology {
                    snapshot: sophia_protocol::OutputTopologySnapshot {
                        generation: 1,
                        primary: sophia_protocol::OutputId::from_raw(1),
                        outputs: Vec::new(),
                    },
                    acknowledgement,
                });
            }
            "unwind" => {
                let drawn = draw_and_learn_surface(&mut peer, &service.transactions);
                assert!(waited_for(|| saw_kind(
                    &service.telemetry,
                    XAuthorityBackpressureTelemetryKind::Wait,
                    true
                )));
                service
                    .raster
                    .try_route(raster_requirement_for(drawn))
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(waited_for(|| !admission_pause_pending(&service.registry)));
        assert_eq!(service.access.standing(), PrivatePortStanding::Ended);
        assert!(matches!(
            service.access.control_producer(&service.owner.lease()),
            Err(PrivateProducerRefusal::Ended)
        ));
        assert!(matches!(
            ingress.submit(
                &service.owner.lease(),
                key_service_route(surface, 11052, 42, false)
            ),
            Err(PrivateSendError::Disconnected(_))
        ));
        assert_eq!(custody.join().phase(), PrivateReapingPhase::NotBegun);
        admission.send(()).unwrap();
        assert!(waited_for(|| !pause_pending(
            &service.registry,
            custody.cleanup_record().client
        )));
        assert_eq!(
            custody.join().phase(),
            PrivateReapingPhase::NotBegun,
            "the actual connection frame is still owned, so no collected token authorizes joining"
        );
        frame.send(()).unwrap();
        let closed = service.closed();
        assert_eq!(closed.unwound, kind == "unwind");
        assert_eq!(
            closed.succeeded,
            matches!(kind, "shutdown" | "command_channel_loss")
        );
        assert_eq!(closed.error.is_some(), kind == "error");
        assert_eq!(
            closed.modifiers,
            Some(1),
            "original XKB Shift history outlives exiting runner"
        );
        assert_eq!(custody_identity(&custody), identity);
        assert_eq!(custody.join().phase(), PrivateReapingPhase::Joined);
        assert!(eof_within(&mut peer, 3));
        let held = service.owner.store.inner.lock().unwrap();
        let exact_inventory = held
            .terminal
            .iter()
            .find(|inventory| {
                inventory
                    .execution
                    .as_ref()
                    .is_some_and(|witness| witness.instance == closed.instance)
            })
            .unwrap();
        assert!(
            !exact_inventory.holds.is_empty(),
            "held key remains terminal debt, not settled by namespace removal"
        );
        let witness = Arc::clone(exact_inventory.execution.as_ref().unwrap());
        let retained_holds = exact_inventory.holds.len();
        drop(held);
        let egress = service.owner.store.unresolved_egress().unwrap();
        if kind == "unwind" {
            assert_eq!(egress, 1);
        }
        let step = service.step();
        assert_eq!(step.instance, closed.instance);
        assert_eq!(step.phase, PrivateMaintenancePhase::Output);
        assert_eq!(step.modifiers, Some(1));
        assert!(step.charged);
        facts.push(json!({"exit":kind,"original_key_bytes":expected.to_vec(),"closed":format!("{closed:?}"),"identity":format!("{identity:?}"),"held_records":retained_holds,"egress_residual":egress,"bounded_visit":step.detail,"residual_disposition":"original history retained until explicit test owner ends; held debt is not claimed settled"}));
        actors.extend(service.finish(&[custody]));
        assert_eq!(
            witness.reading().availability,
            PrivateExecutionAvailability::Abandoned,
            "ending original executor records loss while residual debt remains owed"
        );
    }
    emit_case(
        "D.service_exit",
        &[
            ("shutdown", facts[0].clone()),
            ("command_channel_loss", facts[1].clone()),
            ("error", facts[2].clone()),
            ("unwind", facts[3].clone()),
            (
                "admission_closed_before_wait",
                json!({"all_exit_paths":facts}),
            ),
            (
                "outer_custody_and_original_keyboard_history",
                json!({"all_exit_paths":facts}),
            ),
        ],
        &actors,
    );
}

#[test]
fn d_namespace_reuse() {
    let (pause, release) = Pause::pair();
    let mut old = LifecycleService::launch(
        "old-number-custody",
        11006,
        Some(AttachFault::Body { pause, panic: None }),
        false,
    );
    old.start();
    let (peer, custody) = old.connect();
    let worker = release.entered();
    let client = custody.cleanup_record().client;
    assert!(Arc::ptr_eq(&old.custody(), &custody));
    let captured = old.registry.client_senders(client).unwrap();
    let before = custody_identity(&custody);
    let frame = pause_after_registration_drop(&old.registry, client);
    peer.shutdown(std::net::Shutdown::Both).unwrap();
    assert!(waited_for(|| !pause_pending(&old.registry, client)));
    assert_eq!(
        old.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held)
    );
    assert!(
        old.registry
            .occupancy
            .claim(client, &captured.connection_state)
            .is_err(),
        "the held number itself excludes a successor"
    );
    let without_collection = custody.visit_deferred_cleanup(None);
    assert_eq!(
        without_collection.result,
        Err(PrivateDeferredCleanupRefusal::ConnectionsUncollected)
    );
    assert_eq!(
        old.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held)
    );
    let mut denied = connect_private_client(&old.path);
    denied
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    assert!(
        matches!(denied.read(&mut [0;1]),Err(error) if matches!(error.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)),
        "the pending connection has no admitted reply while the old frame owns capacity"
    );
    assert_eq!(kept_custodies(&old.registry).len(), 1);
    drop(denied);
    release.release();
    frame.send(()).unwrap();
    old.command(XServerFrontendServiceCommand::StopAndDisconnect);
    let closed = old.closed();
    assert_eq!(old.registry.occupancy.state_of(client), None);
    assert!(matches!(
        custody.deferred_cleanup_standing(),
        PrivateDeferredCleanupStanding::Done(_)
    ));

    // This is a new real service invocation with a colliding number. It does
    // not claim that a closed old invocation reopened admission in-place.
    let mut successor = LifecycleService::launch("successor-number-custody", 11006, None, false);
    successor.start();
    let (mut next_peer, next) = successor.connect();
    wait_attached(&successor.registry);
    assert_eq!(next.cleanup_record().client, client);
    assert!(!Arc::ptr_eq(
        &successor.registry.clients,
        &old.registry.clients
    ));
    let next_identity = custody_identity(&next);
    let state = successor
        .registry
        .client_senders(client)
        .unwrap()
        .connection_state;
    assert!(!Arc::ptr_eq(&state, &captured.connection_state));
    let repeat = custody.visit_deferred_cleanup(None);
    assert!(
        repeat.result.is_ok(),
        "the old completed cleanup answers only from its own record"
    );
    let maintenance = old.step();
    assert_eq!(maintenance.instance, closed.instance);
    assert_eq!(custody_identity(&next), next_identity);
    assert_eq!(
        successor.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held)
    );
    let failure = successor.registry.route_to_client(
        client,
        &captured.connection_state,
        captured.protocol,
        crate::XClientEvent::XfixesSelectionNotify {
            sequence: 0,
            subtype: 0,
            window: XResourceId::new(0x310601, 1),
            owner: crate::XResourceId::NONE,
            selection: 1,
            time: 0,
            selection_time: 0,
        },
    );
    assert!(
        matches!(failure,Err(XServerFrontendRouteError::ClientQueueDisconnected {client: failed}) if failed==client)
    );
    assert!(Arc::ptr_eq(
        &successor
            .registry
            .client_senders(client)
            .unwrap()
            .connection_state,
        &state
    ));
    assert_eq!(
        successor.registry.occupancy.state_of(client),
        Some(PrivateNumberStanding::Held)
    );
    let (surface, sequence, ingress) = focus_window(&successor, &mut next_peer, 0x310601, 11060);
    ingress
        .submit(
            &successor.owner.lease(),
            button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(11061),
                272,
                true,
            ),
        )
        .unwrap();
    assert_eq!(
        read_event(&mut next_peer, 3),
        Some(expected_button_event(true, sequence, 0x310601, 1))
    );
    successor.command(XServerFrontendServiceCommand::StopAndDisconnect);
    successor.closed();
    let fact = json!({"scope":"successive actual invocations; unfinished old frame excludes reuse within its invocation","worker":format!("{worker:?}"),"old":format!("{before:?}"),"successor":format!("{next_identity:?}"),"refused_cleanup":format!("{without_collection:?}"),"repeated_cleanup":format!("{repeat:?}"),"actual_stale_sender_failure":format!("{failure:?}"),"original_maintenance":format!("{maintenance:?}"),"successor_wire":"exact focused button press delivered after stale effects"});
    let mut actors = old.finish(&[custody]);
    actors.extend(successor.finish(&[next]));
    emit_case(
        "D.namespace_reuse",
        &[
            ("old_cleanup_cannot_touch_successor", fact.clone()),
            ("captured_failure_cannot_touch_successor", fact.clone()),
            ("unfinished_excludes_reuse", fact),
        ],
        &actors,
    );
}
