// A full per-client control channel on the private path is backpressure:
// the control is kept with its credit and sent on a later turn, in order,
// and the invocation is never ended for it. Before this, a client that was
// merely descheduled with two controls undrained made route_to_client answer
// ClientQueueFull, nothing on the private turn matched it, and the service
// died with the operation consumed and its credit outstanding (t130).

/// A registered, admitted client whose channels the control keeps, so its
/// control channel can be left undrained on purpose.
#[cfg(unix)]
fn admitted_client_with_channels(
    private: &crate::PrivateXServerFrontend,
    client: XServerFrontendClientId,
    surface: SurfaceId,
    window: u32,
) -> (XServerFrontendClientRouteRegistration, XServerFrontendClientRouteChannels) {
    admitted_as_with_channels(private, client, admitted(client), surface, window)
}

/// The same, under an admission the caller names: a successor under a
/// reused number is a different admission, as it is for a real connection.
#[cfg(unix)]
fn admitted_as_with_channels(
    private: &crate::PrivateXServerFrontend,
    client: XServerFrontendClientId,
    admission: sophia_protocol::ClientAdmissionContext,
    surface: SurfaceId,
    window: u32,
) -> (XServerFrontendClientRouteRegistration, XServerFrontendClientRouteChannels) {
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admission))
        .expect("a fresh client to register");
    private
        .broker
        .registry
        .attach_private_lifecycle(&registration, admission)
        .expect("a fresh client to be admitted to the boundary");
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(u64::from(window), 1),
        )
        .expect("the surface to register");
    (registration, channels)
}

/// Serve the order until a turn moves nothing. A full channel is not a
/// route error, so every turn must be readable.
#[cfg(unix)]
fn serve_until_idle(private: &mut crate::PrivateXServerFrontend, keyboards: &mut PrivateKeyboards) {
    loop {
        let ran = private
            .route_pending_ordered(keyboards, &control_watchdog())
            .expect("a full control channel is backpressure, not a route error")
            .len();
        if ran == 0 {
            return;
        }
    }
}

#[cfg(unix)]
fn transaction_of(control: &X11RoutedControl) -> u64 {
    control
        .authority_command()
        .expect("an authority control, not a focus-out")
        .transaction()
        .raw()
}

#[cfg(unix)]
#[test]
fn a_control_that_meets_a_full_channel_is_kept_and_sent_in_order_when_the_channel_drains() {
    let client = XServerFrontendClientId(901);
    let surface = SurfaceId::new(901, 1);
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = private_for_roles(&service_keeper);
    let (_registration, channels) = admitted_client_with_channels(&private, client, surface, 0x200901);
    let producer = private.control_producer();
    // Six controls into a channel four deep, with nobody reading it.
    for transaction in 9010..9016 {
        producer
            .submit(&service_keeper.lease(), configure(client, surface, transaction))
            .expect("the order accepts each");
    }
    let mut keyboards = private.keyboards().expect("this instance's state");
    serve_until_idle(&mut private, &mut keyboards);
    assert_eq!(
        private.broker.registry.deferred_controls_for(client),
        2,
        "the two the channel would not take are kept, not lost and not fatal"
    );

    // THE CLIENT READS FOUR, IN ORDER, and nothing more until a turn sends
    // what was kept.
    for transaction in 9010..9014 {
        let control = channels.control.recv_timeout(Duration::from_secs(1)).expect("routed");
        assert_eq!(transaction_of(&control), transaction);
    }
    assert!(channels.control.try_recv().is_err(), "nothing more until the flush");
    assert_eq!(private.broker.registry.flush_control_backlog().expect("readable"), 2);
    for transaction in 9014..9016 {
        let control = channels.control.recv_timeout(Duration::from_secs(1)).expect("sent on the flush");
        assert_eq!(transaction_of(&control), transaction, "in the order they were routed");
    }
    assert_eq!(private.broker.registry.deferred_controls_for(client), 0);
    assert_eq!(private.broker.registry.flush_control_backlog().expect("readable"), 0);
    assert!(channels.control.try_recv().is_err(), "nothing is sent twice");
}

#[cfg(unix)]
#[test]
fn a_focus_change_whose_focus_out_went_out_is_not_repeated_when_its_own_control_waits() {
    let first = XServerFrontendClientId(902);
    let second = XServerFrontendClientId(903);
    let first_surface = SurfaceId::new(902, 1);
    let second_surface = SurfaceId::new(903, 1);
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = private_for_roles(&service_keeper);
    let (_first_registration, first_channels) =
        admitted_client_with_channels(&private, first, first_surface, 0x200902);
    let (_second_registration, second_channels) =
        admitted_client_with_channels(&private, second, second_surface, 0x200903);
    let producer = private.control_producer();
    let lease = service_keeper.lease();
    let focus = |client, surface, transaction| XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(transaction),
            surface,
        },
    };
    let mut keyboards = private.keyboards().expect("this instance's state");

    // Focus the first client, delivered; then fill the second's channel.
    producer.submit(&lease, focus(first, first_surface, 9020)).expect("accepted");
    serve_until_idle(&mut private, &mut keyboards);
    let control = first_channels.control.recv_timeout(Duration::from_secs(1)).expect("focused");
    assert_eq!(transaction_of(&control), 9020);
    for transaction in 9021..9025 {
        producer
            .submit(&lease, configure(second, second_surface, transaction))
            .expect("accepted");
    }
    serve_until_idle(&mut private, &mut keyboards);
    assert_eq!(private.broker.registry.deferred_controls_for(second), 0, "four fit exactly");

    // THE FOCUS MOVES TO THE SECOND CLIENT. Its FocusOut to the first went
    // out at once; its own control met the full channel and is kept. The
    // focus itself moved: the operation happened once, only its message waits.
    producer.submit(&lease, focus(second, second_surface, 9025)).expect("accepted");
    serve_until_idle(&mut private, &mut keyboards);
    let focus_out = first_channels.control.recv_timeout(Duration::from_secs(1)).expect("the first is told");
    assert!(matches!(focus_out, X11RoutedControl::FocusOut { .. }), "{focus_out:?}");
    assert!(first_channels.control.try_recv().is_err(), "told once");
    assert_eq!(private.broker.registry.deferred_controls_for(second), 1);
    assert_eq!(
        private
            .broker
            .registry
            .focused_surface
            .lock()
            .expect("readable")
            .map(|route| route.client),
        Some(second),
        "the focus moved when the operation ran, not when its message went"
    );

    // The second client drains, the kept control goes, and nothing is
    // repeated to either client.
    for transaction in 9021..9025 {
        let control = second_channels.control.recv_timeout(Duration::from_secs(1)).expect("routed");
        assert_eq!(transaction_of(&control), transaction);
    }
    assert_eq!(private.broker.registry.flush_control_backlog().expect("readable"), 1);
    let control = second_channels.control.recv_timeout(Duration::from_secs(1)).expect("sent on the flush");
    assert_eq!(transaction_of(&control), 9025);
    assert!(matches!(control, X11RoutedControl::Authority { focus: Some(_), .. }), "the transition travels with it");
    assert_eq!(private.broker.registry.flush_control_backlog().expect("readable"), 0);
    assert!(first_channels.control.try_recv().is_err() && second_channels.control.try_recv().is_err());
}

#[cfg(unix)]
#[test]
fn a_kept_control_for_a_client_whose_channel_is_gone_is_acknowledged_gone_and_never_reaches_a_successor() {
    let client = XServerFrontendClientId(904);
    let surface = SurfaceId::new(904, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let (delivery_sender, _deliveries) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(2).unwrap(),
            control_acknowledgements: acknowledgements,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let (registration, channels) = admitted_client_with_channels(&private, client, surface, 0x200904);
    let producer = private.control_producer();
    let lease = service_keeper.lease();
    for transaction in 9040..9043 {
        producer
            .submit(&lease, configure(client, surface, transaction))
            .expect("accepted");
    }
    let mut keyboards = private.keyboards().expect("this instance's state");
    serve_until_idle(&mut private, &mut keyboards);
    assert_eq!(private.broker.registry.deferred_controls_for(client), 1, "two fit, one is kept");

    // THE CLIENT'S CHANNEL GOES with two unread. The kept one is not sent
    // into a closed channel and is not lost silently: it is acknowledged as
    // gone, as a control to a departed client is, and dropped.
    drop(channels);
    assert_eq!(private.broker.registry.flush_control_backlog().expect("readable"), 0);
    assert_eq!(private.broker.registry.deferred_controls_for(client), 0);
    let ack = acks
        .recv_timeout(Duration::from_secs(1))
        .expect("the kept control is acknowledged");
    assert_eq!(ack.client, client);
    assert_eq!(ack.acknowledgement.transaction, TransactionId::from_raw(9042));
    assert_eq!(ack.acknowledgement.outcome, XAuthorityControlOutcome::ClientGone);
    assert!(acks.try_recv().is_err(), "acknowledged once");

    // A SUCCESSOR UNDER THE SAME NUMBER receives nothing that was routed to
    // its predecessor. The predecessor departs the way a connection does:
    // its admission revoked on the boundary, its registration gone, the
    // lifecycle drained; the successor is a different admission.
    private
        .admission_participant()
        .revoke_admission(client, sophia_protocol::ClientAdmissionId::from_raw(904))
        .expect("the boundary to revoke");
    drop(registration);
    lifecycle_drain(&private.terminal.lifecycle);
    let successor = sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(9904),
        sophia_protocol::NamespaceContext::new(
            NamespaceId::from_raw(904),
            sophia_protocol::NamespaceProfile::Confined,
            sophia_protocol::NamespaceCapabilities::NONE,
        )
        .unwrap(),
        sophia_protocol::ClientAuthProvenance::new(
            sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
            ROLE_SESSION_GENERATION,
        )
        .unwrap(),
    )
    .unwrap();
    let (_successor_registration, successor_channels) =
        admitted_as_with_channels(&private, client, successor, SurfaceId::new(904, 2), 0x200905);
    assert_eq!(private.broker.registry.flush_control_backlog().expect("readable"), 0);
    assert!(successor_channels.control.try_recv().is_err(), "nothing crosses to the successor");
}

/// The public broker keeps its meaning: a full control queue is the fault
/// it always was, answered at the send and decided by the broker, and
/// nothing is kept for a later turn there.
#[cfg(unix)]
#[test]
fn the_public_broker_still_answers_a_full_control_queue_as_the_fault_it_was() {
    let client = XServerFrontendClientId(905);
    let surface = SurfaceId::new(905, 1);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(1).unwrap());
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    let focus = |transaction| XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(transaction),
            surface,
        },
    };
    broker
        .control_router()
        .route_control(focus(9050))
        .expect("the one place in the queue");
    let refused = broker.control_router().route_control(focus(9051));
    assert!(
        matches!(refused, Err(XServerFrontendRouteError::ClientQueueFull { client: full }) if full == client),
        "the public queue's fullness is still the fault it was, got {refused:?}"
    );
    assert_eq!(
        broker.registry.deferred_controls_for(client),
        0,
        "and nothing is kept for later on the public path"
    );
    drop(channels);
}
