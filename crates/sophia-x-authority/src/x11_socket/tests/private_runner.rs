fn prepared_runner_fixture() -> (
    PrivatePreparedRunner,
    PrivateSettlementOwner,
    XServerFrontendClientRouteRegistration,
    XServerFrontendClientRouteChannels,
    Receiver<XAuthorityClientControlAck>,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let client = XServerFrontendClientId::from_raw(9000);
    let durable = PrivateSettlementOwner::default();
    let (ack_sender, acks) = sync_channel(8);
    let (delivery_sender, deliveries) = channel();
    let (authority, issuer, submit) = private_authority();
    let private = PrivateXServerFrontend::new(
        PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &durable,
    )
    .unwrap_or_else(|(cause, _)| panic!("construction refused: {cause:?}"));
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .unwrap();
    private
        .admission_participant()
        .admit(client, admitted(client))
        .unwrap();
    // The source resolves the recipient itself now, out of the selection state
    // attached to this connection. A surface registered with no window that
    // ever selected button events leaves that resolution nothing to reach, so
    // this preparation is the press's precondition rather than scenery.
    let selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    let focused = Arc::new(AtomicU64::new(0));
    private
        .broker
        .registry
        .attach_connection_state(
            &registration,
            NamespaceId::from_raw(client.raw()),
            selections.clone(),
            focused.clone(),
        )
        .unwrap();
    {
        let mut selected = selections.lock().unwrap();
        selected.register(
            XResourceId::new(9000, 1),
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
        );
        selected.observe_mapped(XResourceId::new(9000, 1));
        selected.update(XResourceId::new(9000, 1), Some((1 << 2) | (1 << 3)), None);
    }
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            SurfaceId::new(9000, 1),
            XResourceId::new(9000, 1),
        )
        .unwrap();
    let runner = private
        .prepare_runner(NamespaceId::from_raw(client.raw()))
        .unwrap_or_else(|(cause, _)| panic!("runner refused: {cause:?}"));

    // The publication starts unavailable, and resolution refuses against an
    // unpublished one. A real initial clear through the publication the runner
    // installed, borrowed rather than installed again -- not a flag set to
    // stand in for one.
    {
        let publication = runner
            .frontend
            .as_ref()
            .expect("a live runner")
            .broker
            .registry
            .private_applied
            .get()
            .expect("prepare_runner installed it")
            .publication
            .clone();
        let mut runtime = XAuthorityRuntime::new();
        runtime.prepare_input_focus_namespace(NamespaceId::from_raw(client.raw()));
        publication
            .lock()
            .expect("the publication")
            .begin_focus_change()
            .expect("a focus change")
            .apply(&mut runtime, &focused, None)
            .expect("the clear applies");
    }
    (runner, durable, registration, channels, acks, deliveries)
}

#[test]
fn a_prepared_runner_owns_state_before_exposing_its_real_producer() {
    let (mut runner, _durable, _registration, channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let client = XServerFrontendClientId::from_raw(9000);
    let seat = SeatId::from_raw(1);
    assert_eq!(runner.seat(), seat);
    assert!(runner.frontend.as_ref().unwrap().native_owner.is_some());
    assert_eq!(runner.keyboards.modifiers(seat), Some(0));
    assert!(matches!(
        runner.frontend.as_ref().unwrap().keyboards(),
        Err(PrivateKeyboardsRefusal::AlreadyIssued)
    ));
    assert!(
        runner
            .frontend
            .as_ref()
            .unwrap()
            .broker
            .registry
            .pointer_state
            .lock()
            .unwrap()
            .contains_key(&(runner.namespace(), seat))
    );
    let ingress = runner.ingress_for(client, DeviceId::from_raw(1)).unwrap();
    ingress
        .submit(button_to(
            SurfaceId::new(9000, 1),
            XAuthorityInputDeliveryId::from_raw(9000),
            272,
            true,
        ))
        .unwrap();
    let first = runner.service_turn().unwrap();
    assert_eq!(first.taken, 1);
    assert_eq!(
        first.starts, 2,
        "execution and terminal service are separate starts"
    );
    assert_eq!(first.terminal_steps, 1);
    assert_eq!(runner.service.usage().cleanup_starts, 1);
    assert_eq!(first.enqueued, 1);
    assert_eq!(first.observed, 1);
    assert_eq!(first.settled, 0);
    assert_eq!(channels.input.try_iter().count(), 1);
    let second = runner.service_turn().unwrap();
    assert_eq!(second.taken, 0);
    assert_eq!(second.enqueued, 0);
    assert_eq!(second.starts, 0);
    assert_eq!(second.terminal_steps, 0);
}

#[test]
fn native_preparation_is_retained_and_a_second_runner_cannot_replace_it() {
    let (mut runner, _durable, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let client = XServerFrontendClientId::from_raw(9000);
    let namespace = runner.namespace();
    // The connection state this instance already carries. Attaching a second
    // one here would be refused as a different state for the same connection,
    // and the witness below has to come from the one the registry is actually
    // resolving against rather than from a fresh stand-in.
    let private = runner.frontend.as_ref().unwrap();
    let owner = private.native_owner.as_ref().unwrap().clone();
    // The prepared origin accepts a witness from the actual registry under
    // common and admission. A foreign instance with colliding names does not.
    let (foreign, _foreign_durable, _foreign_registration, _foreign_channels,
        _foreign_acks, _foreign_deliveries) = prepared_runner_fixture();
    private.participant.under_boundary(|_, _, bindings| {
        let clients = private.broker.registry.clients.lock().unwrap();
        let witness = private.broker.registry
            .applied_client(&clients, client, &bindings.bound[&client]).unwrap();
        drop(owner.lock_for_connection(&witness).unwrap());
        assert!(matches!(
            foreign.frontend.as_ref().unwrap().native_owner.as_ref().unwrap()
                .lock_for_connection(&witness),
            Err(private_native::Refusal::ForeignOrigin)
        ));
    }).unwrap();
    // No producer has escaped. Reaching this path must still refuse rather
    // than replace the allocation cloned by the original execution owner.
    let private = runner.frontend.take().unwrap();
    let (cause, returned) = match private.prepare_runner(namespace) {
        Ok(_) => panic!("second preparation replaced an existing native origin"),
        Err(refused) => refused,
    };
    assert_eq!(cause, PrivateRunnerRefusal::AlreadyPrepared);
    assert!(returned.native_owner.is_some());
    runner.frontend = Some(returned);
}

#[test]
fn losing_a_prepared_runner_closes_its_producers_and_carries_its_hold() {
    let (mut runner, durable, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let ingress = runner
        .ingress_for(
            XServerFrontendClientId::from_raw(9000),
            DeviceId::from_raw(1),
        )
        .unwrap();
    ingress
        .submit(button_to(
            SurfaceId::new(9000, 1),
            XAuthorityInputDeliveryId::from_raw(9001),
            272,
            true,
        ))
        .unwrap();
    assert_eq!(runner.service_turn().unwrap().observed, 1);
    drop(runner);
    assert!(matches!(
        ingress.submit(button_to(
            SurfaceId::new(9000, 1),
            XAuthorityInputDeliveryId::from_raw(9002),
            272,
            false
        )),
        Err(PrivateSendError::Disconnected(_))
    ));
    let owned = durable.inner.lock().unwrap();
    assert_eq!(owned.terminal.len(), 1);
    assert_eq!(owned.terminal[0].holds.len(), 1);
}

#[test]
fn focus_encoding_takes_input_authority_before_event_selections() {
    // Error precedence pins the actual production acquisition order. This
    // control does not claim to schedule a two-writer deadlock.
    let authority = Arc::new(Mutex::new(crate::XInputAuthorityState::default()));
    let selections = Mutex::new(XCoreEventSelectionState::default());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = authority.lock().unwrap();
        panic!("poison authority");
    }));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = selections.lock().unwrap();
        panic!("poison selections");
    }));
    let error = x11_focus_records(
        XByteOrder::LittleEndian,
        1,
        NamespaceId::from_raw(1),
        XServerFrontendClientId::from_raw(1),
        &selections,
        Some(&authority),
        0,
        X11FocusRecordRequest::Clear {
            root: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            previous_routed: XResourceId::new(2, 1),
            transition: None,
        },
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("input authority lock poisoned"),
        "{error}"
    );
}

#[test]
fn runner_accounts_a_park_once_and_never_charges_idle_or_blocked_reads() {
    let (mut runner, _durable, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    for _ in 0..3 {
        assert!(matches!(
            runner.execute_accounted_step().unwrap(),
            PrivateAccountedStep::Step {
                step: PrivateOrderedStep::Idle,
                charge: None
            }
        ));
    }
    assert_eq!(runner.service.usage().starts, 0);
    let sequence = runner
        .control_producer()
        .submit(configure(
            XServerFrontendClientId::from_raw(9000),
            SurfaceId::new(9000, 1),
            91000,
        ))
        .unwrap();
    assert!(matches!(runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Parked(s), charge: Some(_) } if s==sequence));
    assert_eq!(runner.service.usage().starts, 1);
    let charged = runner.service.usage().charged;
    for _ in 0..3 {
        assert!(matches!(runner.execute_accounted_step().unwrap(),
            PrivateAccountedStep::Step { step: PrivateOrderedStep::Blocked(s), charge: None } if s==sequence));
    }
    assert_eq!(runner.service.usage().starts, 1);
    assert_eq!(runner.service.usage().charged, charged);
    assert!(!runner.service.is_interrupted());
}

#[test]
fn runner_checks_its_allowance_before_taking_accepted_work() {
    use sophia_input_authority::{CleanupReadiness, ServiceBudget, ServiceLimits, ServiceWork};
    use std::time::Duration;
    let (mut runner, _durable, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let ingress = runner
        .ingress_for(
            XServerFrontendClientId::from_raw(9000),
            DeviceId::from_raw(1),
        )
        .unwrap();
    let sequence = ingress
        .submit(button_to(
            SurfaceId::new(9000, 1),
            XAuthorityInputDeliveryId::from_raw(91001),
            272,
            true,
        ))
        .unwrap();
    // Stage a consumed allowance, not a fake queued operation. The production
    // step below must leave the actual producer's envelope in its order.
    let now = runner.service_origin.elapsed();
    runner.service = ServiceBudget::new(
        now,
        ServiceLimits {
            interval: Duration::from_secs(60),
            starts: 1,
            charge: Duration::from_secs(1),
            cleanup_starts: 0,
            cleanup_charge: Duration::ZERO,
        },
    )
    .unwrap();
    runner
        .service
        .start(now, ServiceWork::NewWork, CleanupReadiness::Eligible)
        .unwrap()
        .finish(now)
        .unwrap();
    assert!(matches!(
        runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Yield {
            cause: sophia_input_authority::ServiceStartRefusal::StartsExhausted { .. },
            taken: None
        }
    ));
    let frontend = runner.frontend.as_ref().unwrap();
    assert!(frontend.terminal.current.is_none());
    assert!(frontend.terminal.holds.is_empty());
    let queued = frontend.admission.take_next().unwrap().unwrap();
    assert_eq!(queued.0, sequence);
    assert!(matches!(queued.2, PrivateOperation::RoutedInput(_)));
}

#[test]
fn unwatchable_work_finishes_accounting_without_becoming_an_effect() {
    let (mut runner, _durable, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let ingress = runner
        .ingress_for(
            XServerFrontendClientId::from_raw(9000),
            DeviceId::from_raw(1),
        )
        .unwrap();
    let sequence = ingress
        .submit(button_to(
            SurfaceId::new(9000, 1),
            XAuthorityInputDeliveryId::from_raw(91002),
            272,
            true,
        ))
        .unwrap();
    // Fail the actual runner-owned supervisor after acceptance. No external
    // substitute can erase this runner's permanent failure latch.
    drop(
        runner
            .watch
            .as_ref()
            .unwrap()
            .begin_dequeued(std::time::Instant::now())
            .unwrap(),
    );
    assert!(matches!(runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Unwatched(s), charge: Some(_) } if s==sequence));
    assert_eq!(runner.service.usage().starts, 1);
    assert!(!runner.service.is_interrupted());
    let frontend = runner.frontend.as_ref().unwrap();
    assert!(frontend.terminal.holds.is_empty());
    assert!(matches!(
        frontend.terminal.current,
        Some(PrivateOrderedItem::Refused {
            refusal: PrivateExecutionRefusal::NotAttempted,
            ..
        })
    ));
}

#[test]
fn runner_does_not_charge_or_resend_an_indeterminate_terminal_head() {
    let (mut runner, _durable, _registration, channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let ingress = runner
        .ingress_for(
            XServerFrontendClientId::from_raw(9000),
            DeviceId::from_raw(1),
        )
        .unwrap();
    let sequence = ingress
        .submit(button_to(
            SurfaceId::new(9000, 1),
            XAuthorityInputDeliveryId::from_raw(91003),
            272,
            true,
        ))
        .unwrap();
    assert!(matches!(runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Decided(s), .. } if s==sequence));
    // Compose the phase an interrupted send would leave on this real owned
    // decision. This is not an injected transport-unwind control.
    runner.frontend.as_mut().unwrap().terminal.emission = PrivateEmissionPhase::Indeterminate;
    let usage = runner.service.usage();
    for _ in 0..3 {
        assert!(matches!(runner.deliver_accounted_step().unwrap(),
            PrivateAccountedDelivery::Step { step: PrivateDeliveryStep::Blocked(s), charge: None, watch_failed: false, unwatched: None } if s==sequence));
    }
    assert_eq!(runner.service.usage(), usage);
    assert_eq!(channels.input.try_iter().count(), 0);
    assert_eq!(
        runner.frontend.as_ref().unwrap().terminal.delivering.len(),
        1
    );
}

#[test]
fn runner_charges_common_wait_and_supervision_can_end_it_independently() {
    use std::io::Read;
    use std::time::Duration;
    let (mut runner, _durable, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let ingress = runner
        .ingress_for(
            XServerFrontendClientId::from_raw(9000),
            DeviceId::from_raw(1),
        )
        .unwrap();
    let sequence = ingress
        .submit(button_to(
            SurfaceId::new(9000, 1),
            XAuthorityInputDeliveryId::from_raw(91004),
            272,
            true,
        ))
        .unwrap();
    let common = runner.frontend.as_ref().unwrap().controller.common.clone();
    let (transport, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let frontend = runner.frontend.as_ref().unwrap();
    let _transport_registration = frontend
        .broker
        .registry
        .input_recovery
        .watchdog
        .get()
        .unwrap()
        .attach_transport(transport)
        .unwrap();
    let gate = frontend.admission.watch.get().unwrap().clone();
    let (ready_sender, ready) = sync_channel(1);
    let holder = std::thread::spawn(move || {
        let _guard = common.lock().unwrap();
        ready_sender.send(()).unwrap();
        // This guard is released only after independent transport shutdown
        // (or a bounded test failure). No scheduling sleep chooses the race.
        peer.read(&mut [0u8; 1])
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let result = runner.execute_accounted_step().unwrap();
    assert_eq!(
        holder.join().unwrap().unwrap(),
        0,
        "watchdog closed the transport while common was held"
    );
    let PrivateAccountedStep::Step {
        step: PrivateOrderedStep::DecidedUnwatched(s),
        charge: Some(charge),
    } = result
    else {
        panic!("guard wait must end as an owned, watched refusal");
    };
    assert_eq!(s, sequence);
    assert!(charge.elapsed >= Duration::from_millis(250));
    assert!(!charge.allowance_overrun.is_zero());
    assert!(!gate.allows_execution());
    assert!(runner.frontend.as_ref().unwrap().terminal.holds.is_empty());
    assert_eq!(runner.frontend.as_ref().unwrap().terminal.turn.len(), 1);
    assert!(
        !runner.service.is_interrupted(),
        "a returned refusal still finishes accounting"
    );
}

#[test]
fn runner_failure_refuses_a_detached_producer_while_common_is_held() {
    let (mut runner, durable, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let ingress = runner
        .ingress_for(
            XServerFrontendClientId::from_raw(9000),
            DeviceId::from_raw(1),
        )
        .unwrap();
    ingress
        .submit(button_to(
            SurfaceId::new(9000, 1),
            XAuthorityInputDeliveryId::from_raw(92001),
            272,
            true,
        ))
        .unwrap();
    let frontend = runner.frontend.as_ref().unwrap();
    let gate = frontend.admission.watch.get().unwrap().clone();
    drop(
        runner
            .watch
            .as_ref()
            .unwrap()
            .begin_dequeued(std::time::Instant::now())
            .unwrap(),
    );
    assert!(!gate.allows_execution());
    let common = frontend.controller.common.clone();
    let held = common.lock().unwrap();
    let (answer, receive) = sync_channel(1);
    let submitter = std::thread::spawn(move || {
        answer
            .send(ingress.submit(button_to(
                SurfaceId::new(9000, 1),
                XAuthorityInputDeliveryId::from_raw(92002),
                272,
                false,
            )))
            .unwrap();
    });
    let result = receive.recv_timeout(Duration::from_secs(2));
    drop(held);
    submitter.join().unwrap();
    assert!(matches!(
        result.unwrap(),
        Err(PrivateSendError::Disconnected(_))
    ));
    assert_eq!(
        frontend.admission.ready.lock().unwrap().ready.len(),
        1,
        "the accepted envelope remains owned while new work is refused"
    );
    assert_eq!(durable.reserved(), Some(1));
    assert!(frontend.terminal.current.is_none());
    assert!(frontend.terminal.holds.is_empty());
}

#[test]
fn idle_runner_loss_closes_an_actual_connection_attached_after_preparation() {
    use std::io::{Read, Write};
    let (mut runner, _durable, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let ingress = runner
        .ingress_for(
            XServerFrontendClientId::from_raw(9000),
            DeviceId::from_raw(1),
        )
        .unwrap();
    let frontend = runner.frontend.as_ref().unwrap();
    let registry = frontend.broker.registry.clone();
    let gate = frontend.admission.watch.get().unwrap().clone();
    let common = frontend.controller.common.clone();
    let state = Arc::new(X11CoreSocketServerState::new());
    state
        .runtime
        .lock()
        .unwrap()
        .set_input_authority(registry.input_authority.clone());
    let context = admitted(XServerFrontendClientId::from_raw(9000));
    let (mut client, mut server) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let (finished, receive) = sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = serve_x11_core_socket_client_with_trace_observer_and_input(
            &mut server,
            context.namespace.id,
            &state,
            X11ClientConnectionInputs {
                input_receiver: None,
                control_channels: None,
                client_routing: Some(registry),
            },
            X11ClientAdmissionContext {
                authorization: &XServerFrontendSetupAuthorization::default(),
                admission_policy: Some(Arc::new(LifecycleSetupPolicy(context))),
                worker_admission: None,
            },
            |_| Ok(None),
        );
        finished.send(result).unwrap();
    });
    let setup = [b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    client.write_all(&setup).unwrap();
    let mut prefix = [0; 8];
    client.read_exact(&mut prefix).unwrap();
    assert_eq!(prefix[0], 1);
    let mut body = vec![0; usize::from(u16::from_le_bytes([prefix[6], prefix[7]])) * 4];
    client.read_exact(&mut body).unwrap();
    // A synchronous request demonstrates the actual route registration and
    // all writer startup completed after runner preparation.
    client.write_all(&[43, 0, 1, 0]).unwrap();
    let mut reply = [0; 32];
    client.read_exact(&mut reply).unwrap();
    assert_eq!(reply[0], 1);
    assert!(gate.allows_execution());
    assert_eq!(gate.failure(), None);
    let (held, receive_held) = sync_channel(1);
    let holder = std::thread::spawn(move || {
        let _common = common.lock().unwrap();
        held.send(()).unwrap();
        // Frontend teardown cannot acquire common until the independent
        // supervisor closes this real, already-started client transport.
        client.read(&mut [0])
    });
    receive_held.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(runner);
    assert!(!gate.allows_execution());
    assert_eq!(holder.join().unwrap().unwrap(), 0);
    receive
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert!(matches!(
        ingress.submit(button_to(
            SurfaceId::new(9000, 1),
            XAuthorityInputDeliveryId::from_raw(92003),
            272,
            true
        )),
        Err(PrivateSendError::Disconnected(_))
    ));
    assert_eq!(
        gate.failure(),
        None,
        "idle owner stop is not an input outcome"
    );
}

#[test]
fn a_refused_actual_setup_returns_its_watchdog_slot() {
    let (runner, _durable, _registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let registry = runner.frontend.as_ref().unwrap().broker.registry.clone();
    let registrar = registry.input_recovery.watchdog.get().unwrap().clone();
    let (mut server, _peer) = UnixStream::pair().unwrap();
    // This is the real private setup refusal before authentication: no
    // production admission policy was supplied. No workers are started.
    let refused = serve_x11_core_socket_client_with_trace_observer_and_input(
        &mut server,
        NamespaceId::from_raw(9000),
        &X11CoreSocketServerState::new(),
        X11ClientConnectionInputs {
            input_receiver: None,
            control_channels: None,
            client_routing: Some(registry),
        },
        X11ClientAdmissionContext {
            authorization: &XServerFrontendSetupAuthorization::default(),
            admission_policy: None,
            worker_admission: None,
        },
        |_| Ok(None),
    )
    .unwrap_err();
    assert!(refused.to_string().contains("current admission policy"));
    let mut registrations = Vec::new();
    let mut peers = Vec::new();
    for _ in 0..16 {
        let (socket, peer) = UnixStream::pair().unwrap();
        registrations.push(registrar.attach_transport(socket).unwrap());
        peers.push(peer);
    }
    let (extra, _peer) = UnixStream::pair().unwrap();
    assert!(matches!(
        registrar.attach_transport(extra),
        Err((
            private_watchdog::PrivateWatchdogRefusal::TransportCapacity,
            _
        ))
    ));
}

#[test]
fn unprepared_frontend_teardown_closes_actual_setup_before_waiting_for_common() {
    use std::io::{Read, Write};
    for explicit_shutdown in [false, true] {
        let private = private_for_roles();
        let registry = private.broker.registry.clone();
        let common = private.controller.common.clone();
        let state = Arc::new(X11CoreSocketServerState::new());
        state
            .runtime
            .lock()
            .unwrap()
            .set_input_authority(registry.input_authority.clone());
        let context = admitted(XServerFrontendClientId::from_raw(92010));
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let (finished, receive) = sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = serve_x11_core_socket_client_with_trace_observer_and_input(
                &mut server,
                context.namespace.id,
                &state,
                X11ClientConnectionInputs {
                    input_receiver: None,
                    control_channels: None,
                    client_routing: Some(registry),
                },
                X11ClientAdmissionContext {
                    authorization: &XServerFrontendSetupAuthorization::default(),
                    admission_policy: Some(Arc::new(LifecycleSetupPolicy(context))),
                    worker_admission: None,
                },
                |_| Ok(None),
            );
            finished.send(result).unwrap();
        });
        let setup = [b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        client.write_all(&setup).unwrap();
        let mut prefix = [0; 8];
        client.read_exact(&mut prefix).unwrap();
        assert_eq!(prefix[0], 1);
        let mut body = vec![0; usize::from(u16::from_le_bytes([prefix[6], prefix[7]])) * 4];
        client.read_exact(&mut body).unwrap();
        // A synchronous request demonstrates the actual route registration and
        // all writer startup completed after runner preparation.
        client.write_all(&[43, 0, 1, 0]).unwrap();
        let mut reply = [0; 32];
        client.read_exact(&mut reply).unwrap();
        assert_eq!(reply[0], 1);

        // No runner was prepared and no execution occurred. The actual setup
        // nevertheless registered its socket with the constructor's owner.
        assert!(private.pending_watch.is_some());
        assert!(private.admission.watch.get().is_none());
        let (held, receive_held) = sync_channel(1);
        let holder = std::thread::spawn(move || {
            let _common = common.lock().unwrap();
            held.send(()).unwrap();
            client.read(&mut [0])
        });
        receive_held.recv_timeout(Duration::from_secs(3)).unwrap();
        if explicit_shutdown {
            drop(private.shutdown());
        } else {
            drop(private);
        }
        assert_eq!(
            holder.join().unwrap().unwrap(),
            0,
            "setup transport closes before teardown enters common"
        );
        receive
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
    }
}
