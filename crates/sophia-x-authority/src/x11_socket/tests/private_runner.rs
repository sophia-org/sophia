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
    (runner, durable, registration, channels, acks, deliveries)
}

#[test]
fn a_prepared_runner_owns_state_before_exposing_its_real_producer() {
    let (mut runner, _durable, _registration, channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let client = XServerFrontendClientId::from_raw(9000);
    let seat = SeatId::from_raw(1);
    assert_eq!(runner.seat(), seat);
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
    let first = runner.service_turn(&control_watchdog()).unwrap();
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
    let second = runner.service_turn(&control_watchdog()).unwrap();
    assert_eq!(second.taken, 0);
    assert_eq!(second.enqueued, 0);
    assert_eq!(second.starts, 0);
    assert_eq!(second.terminal_steps, 0);
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
    assert_eq!(
        runner.service_turn(&control_watchdog()).unwrap().observed,
        1
    );
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
    let watch = control_watchdog();
    for _ in 0..3 {
        assert!(matches!(
            runner.execute_accounted_step(&watch).unwrap(),
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
    assert!(matches!(runner.execute_accounted_step(&watch).unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Parked(s), charge: Some(_) } if s==sequence));
    assert_eq!(runner.service.usage().starts, 1);
    let charged = runner.service.usage().charged;
    for _ in 0..3 {
        assert!(matches!(runner.execute_accounted_step(&watch).unwrap(),
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
    let watch = control_watchdog();
    assert!(matches!(
        runner.execute_accounted_step(&watch).unwrap(),
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
    let watch = private_watchdog::PrivateWatchdogOwner::prepare(0).unwrap(); // Deliberately not sealed.
    assert!(matches!(runner.execute_accounted_step(&watch).unwrap(),
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
    let watch = control_watchdog();
    assert!(matches!(runner.execute_accounted_step(&watch).unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Decided(s), .. } if s==sequence));
    // Compose the phase an interrupted send would leave on this real owned
    // decision. This is not an injected transport-unwind control.
    runner.frontend.as_mut().unwrap().terminal.emission = PrivateEmissionPhase::Indeterminate;
    let usage = runner.service.usage();
    for _ in 0..3 {
        assert!(matches!(runner.deliver_accounted_step(&watch).unwrap(),
            PrivateAccountedDelivery::Step { step: PrivateDeliveryStep::Blocked(s), charge: None, unwatched: None } if s==sequence));
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
    let mut watch = private_watchdog::PrivateWatchdogOwner::prepare(1).unwrap();
    let _transport_registration = watch.attach_transport(transport).unwrap();
    let gate = watch.seal().unwrap();
    let (ready_sender, ready) = sync_channel(1);
    let holder = std::thread::spawn(move || {
        let _guard = common.lock().unwrap();
        ready_sender.send(()).unwrap();
        // This guard is released only after independent transport shutdown
        // (or a bounded test failure). No scheduling sleep chooses the race.
        peer.read(&mut [0u8; 1])
    });
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    let result = runner.execute_accounted_step(&watch).unwrap();
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
    assert!(
        !runner.service.is_interrupted(),
        "a returned refusal still finishes accounting"
    );
}
