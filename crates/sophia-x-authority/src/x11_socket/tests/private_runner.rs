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
    assert_eq!(first.enqueued, 1);
    assert_eq!(first.observed, 1);
    assert_eq!(first.settled, 0);
    assert_eq!(channels.input.try_iter().count(), 1);
    let second = runner.service_turn(&control_watchdog()).unwrap();
    assert_eq!(second.taken, 0);
    assert_eq!(second.enqueued, 0);
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
    assert_eq!(runner.service_turn(&control_watchdog()).unwrap().observed, 1);
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
