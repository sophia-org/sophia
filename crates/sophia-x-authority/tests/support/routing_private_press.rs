// A private press from a desired source: who is entitled to make one, what the
// broker and the admission gate refuse, and what an accepted one leaves.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn a_synthetic_press_with_nobody_entitled_is_refused_without_effect() {
    let namespace = NamespaceId::from_raw(36);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer, submit) = control_gate_with_submit();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let connection = sophia_input_authority::ConnectionIdentity {
        recipient: 97,
        connection_generation: 3,
    };
    let (token, _capability) = reserved_request(&mut instance, &issuer, &submit, connection);

    // No grab and no focus: nobody is entitled to this input.
    let outcome = broker.execute_synthetic_input(
        &mut instance,
        &issuer,
        crate::SyntheticRequest {
            token,
            connection,
            namespace,
            connection_generation: 3,
            action: crate::SyntheticAction::Press,
            input: sophia_input_authority::Input::key(38).expect("a keycode"),
        },
        None,
    );

    let outcome = outcome.expect("the refusal to be reported rather than raised");
    assert_eq!(
        outcome.completion,
        sophia_input_authority::RequestCompletion::Refused(
            sophia_input_authority::RegistrationError::RoutingUnavailable
        ),
        "an unentitled press is refused before any effect"
    );
    assert!(
        outcome.record.is_none(),
        "a refused press resolved nobody, so it recorded nobody"
    );
}

#[test]
fn a_server_grab_does_not_make_its_holder_the_recipient() {
    let namespace = NamespaceId::from_raw(37);
    let server_holder = XServerFrontendClientId(40);
    let focused = XServerFrontendClientId(41);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer, submit) = control_gate_with_submit();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    // Another client holds the server. That schedules requests -- it decides
    // who may proceed while others wait -- and entitles it to nothing.
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_server(namespace, server_holder.raw())
        .expect("the server grab to be taken");

    let connection = sophia_input_authority::ConnectionIdentity {
        recipient: 96,
        connection_generation: 11,
    };
    let (token, _capability) = reserved_request(&mut instance, &issuer, &submit, connection);

    let outcome = broker
        .execute_synthetic_input(
            &mut instance,
            &issuer,
            crate::SyntheticRequest {
                token,
                connection,
                namespace,
                connection_generation: 11,
                action: crate::SyntheticAction::Press,
                input: sophia_input_authority::Input::key(38).expect("a keycode"),
            },
            Some(focused.raw()),
        )
        .expect("the request to execute");

    let record = outcome.record.expect("a recorded press");
    assert_eq!(
        record.incarnation.recipient,
        focused.raw(),
        "a server grab schedules requests; it does not receive input"
    );
    assert!(
        !record.proposed.grabbed,
        "no device grab was held, so this followed the route"
    );
}

#[test]
fn desired_release_after_focus_disappears_still_releases_recorded_hold() {
    let namespace = NamespaceId::from_raw(38);
    let grab_holder = XServerFrontendClientId(42);
    let surface = SurfaceId::new(45, 1);
    let window = XResourceId::new(0x2000f0, 1);
    let (gate, mut instance, issuer, submit) = control_gate_with_submit();
    let (broker, _deliveries, _lease) =
        gated_broker_with_grab(&gate, namespace, grab_holder, surface, window);
    let connection = sophia_input_authority::ConnectionIdentity {
        recipient: 95,
        connection_generation: 13,
    };
    let (token, capability) = reserved_request(&mut instance, &issuer, &submit, connection);
    let input = sophia_input_authority::Input::button(1, 9).expect("button one");

    let pressed = broker
        .execute_synthetic_input(
            &mut instance,
            &issuer,
            crate::SyntheticRequest {
                token,
                connection,
                namespace,
                connection_generation: 13,
                action: crate::SyntheticAction::Press,
                input,
            },
            None,
        )
        .expect("the press to execute");
    assert_eq!(
        pressed.record.expect("a recorded press").incarnation.recipient,
        grab_holder.raw()
    );

    // The cell has to be drained before another request can be reserved.
    instance
        .take_completion(&submit, token, connection)
        .expect("the completion to be taken");
    // The grab that chose the recipient is gone before the release runs.
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .cleanup_owner(grab_holder.raw());

    let context = sophia_input_authority::ExecutionContext {
        generation: capability.generation(),
        connection,
        epoch: 0,
        publication: 0,
        request: 2,
    };
    let release_token = instance
        .reserve_request(&submit, capability, context)
        .expect("a second request to be reserved");
    let released = broker
        .execute_synthetic_input(
            &mut instance,
            &issuer,
            crate::SyntheticRequest {
                token: release_token,
                connection,
                namespace,
                connection_generation: 13,
                action: crate::SyntheticAction::Release,
                input,
            },
            None,
        )
        .expect("the release to execute");

    // No recipient was resolved for it, and none was needed: the hold knows
    // where it went. Re-resolving would have found nobody and refused.
    assert!(released.record.is_none());
    let Some(sophia_input_authority::ReleaseOutcome::DeliverTo(incarnation)) = released.release
    else {
        panic!("the last holder letting go owes a delivery: {:?}", released.release);
    };
    assert_eq!(
        incarnation.recipient,
        grab_holder.raw(),
        "a release answers to the recipient the press reached, not to the current route"
    );
    assert_eq!(incarnation.connection_generation, 13);
}

#[test]
fn desired_duplicate_press_does_not_report_new_delivery() {
    let namespace = NamespaceId::from_raw(39);
    let grab_holder = XServerFrontendClientId(43);
    let surface = SurfaceId::new(46, 1);
    let window = XResourceId::new(0x200100, 1);
    let (gate, mut instance, issuer, submit) = control_gate_with_submit();
    let (broker, _deliveries, _lease) =
        gated_broker_with_grab(&gate, namespace, grab_holder, surface, window);
    let connection = sophia_input_authority::ConnectionIdentity {
        recipient: 94,
        connection_generation: 17,
    };
    let (token, capability) = reserved_request(&mut instance, &issuer, &submit, connection);
    let input = sophia_input_authority::Input::button(1, 9).expect("button one");

    let request = |token| crate::SyntheticRequest {
        token,
        connection,
        namespace,
        connection_generation: 17,
        action: crate::SyntheticAction::Press,
        input,
    };

    let first = broker
        .execute_synthetic_input(&mut instance, &issuer, request(token), None)
        .expect("the first press to execute");
    assert!(
        first.record.expect("a recorded press").first_press,
        "the first press begins the hold"
    );

    instance
        .take_completion(&submit, token, connection)
        .expect("the completion to be taken");
    let context = sophia_input_authority::ExecutionContext {
        generation: capability.generation(),
        connection,
        epoch: 0,
        publication: 0,
        request: 2,
    };
    let again = instance
        .reserve_request(&submit, capability, context)
        .expect("a second request to be reserved");

    // The same source pressing the same input again moves the ledger without
    // being a delivery. A caller that read every success as an event would
    // emit this twice.
    let second = broker
        .execute_synthetic_input(&mut instance, &issuer, request(again), None)
        .expect("the second press to execute");
    assert!(
        !second.record.expect("a recorded press").first_press,
        "a repeated press joins the hold rather than beginning one"
    );
}

#[test]
fn desired_joined_press_reports_the_incarnation_not_the_proposal() {
    let namespace = NamespaceId::from_raw(40);
    let first_focus = XServerFrontendClientId(77);
    let later_focus = XServerFrontendClientId(88);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer, submit) = control_gate_with_submit();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let connection = sophia_input_authority::ConnectionIdentity {
        recipient: 93,
        connection_generation: 19,
    };
    let (token, capability) = reserved_request(&mut instance, &issuer, &submit, connection);
    let input = sophia_input_authority::Input::key(38).expect("a keycode");
    let request = |token| crate::SyntheticRequest {
        token,
        connection,
        namespace,
        connection_generation: 19,
        action: crate::SyntheticAction::Press,
        input,
    };

    let first = broker
        .execute_synthetic_input(
            &mut instance,
            &issuer,
            request(token),
            Some(first_focus.raw()),
        )
        .expect("the first press to execute");
    assert_eq!(
        first.record.expect("a recorded press").incarnation.recipient,
        first_focus.raw()
    );

    instance
        .take_completion(&submit, token, connection)
        .expect("the completion to be taken");
    let context = sophia_input_authority::ExecutionContext {
        generation: capability.generation(),
        connection,
        epoch: 0,
        publication: 0,
        request: 2,
    };
    let again = instance
        .reserve_request(&submit, capability, context)
        .expect("a second request to be reserved");

    // Focus has moved. Resolution proposes the new client; the hold still
    // answers to the old one, and it is the hold that the eventual release is
    // owed to.
    let second = broker
        .execute_synthetic_input(
            &mut instance,
            &issuer,
            request(again),
            Some(later_focus.raw()),
        )
        .expect("the second press to execute");
    let record = second.record.expect("a recorded press");

    assert!(!record.first_press, "this joined rather than began the hold");
    assert_eq!(
        record.proposed.recipient.recipient,
        later_focus.raw(),
        "resolution did propose the client focus now names"
    );
    assert_eq!(
        record.incarnation.recipient,
        first_focus.raw(),
        "but the hold, and the release it owes, still belong to the first"
    );
}

#[test]
fn desired_foreign_authority_cannot_execute_through_bound_broker() {
    let namespace = NamespaceId::from_raw(41);
    let focused = XServerFrontendClientId(50);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _bound_instance, _bound_issuer, _bound_submit) = control_gate_with_submit();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");

    // A second authority, with its own legitimate issuer, submit handle,
    // grant and reserved request. Everything about it is valid; none of it
    // belongs to this broker.
    let (_other_gate, mut other_instance, other_issuer, other_submit) =
        control_gate_with_submit();
    let connection = sophia_input_authority::ConnectionIdentity {
        recipient: 92,
        connection_generation: 23,
    };
    let (token, _capability) =
        reserved_request(&mut other_instance, &other_issuer, &other_submit, connection);
    let input = sophia_input_authority::Input::key(38).expect("a keycode");

    let refused = broker.execute_synthetic_input(
        &mut other_instance,
        &other_issuer,
        crate::SyntheticRequest {
            token,
            connection,
            namespace,
            connection_generation: 23,
            action: crate::SyntheticAction::Press,
            input,
        },
        Some(focused.raw()),
    );

    assert!(
        refused.is_err(),
        "a foreign authority must not execute through this broker"
    );

    // And it left no hold behind. Refusing after the ledger moved would be a
    // contribution nothing will ever release.
    assert!(
        other_instance
            .take_completion(&other_submit, token, connection)
            .expect("the cell to be readable")
            .is_none(),
        "a refusal before execution leaves the completion cell untouched"
    );
}

#[test]
fn desired_release_after_focus_changes_reports_original_recipient() {
    let namespace = NamespaceId::from_raw(42);
    let first_focus = XServerFrontendClientId(77);
    let later_focus = XServerFrontendClientId(88);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer, submit) = control_gate_with_submit();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let connection = sophia_input_authority::ConnectionIdentity {
        recipient: 91,
        connection_generation: 29,
    };
    let (token, capability) = reserved_request(&mut instance, &issuer, &submit, connection);
    let input = sophia_input_authority::Input::key(38).expect("a keycode");

    let pressed = broker
        .execute_synthetic_input(
            &mut instance,
            &issuer,
            crate::SyntheticRequest {
                token,
                connection,
                namespace,
                connection_generation: 29,
                action: crate::SyntheticAction::Press,
                input,
            },
            Some(first_focus.raw()),
        )
        .expect("the press to execute");
    assert_eq!(
        pressed.completion,
        sophia_input_authority::RequestCompletion::Processed
    );

    instance
        .take_completion(&submit, token, connection)
        .expect("the completion to be taken");
    let context = sophia_input_authority::ExecutionContext {
        generation: capability.generation(),
        connection,
        epoch: 0,
        publication: 0,
        request: 2,
    };
    let release_token = instance
        .reserve_request(&submit, capability, context)
        .expect("a second request to be reserved");

    // Focus has moved to another client before the release runs.
    let released = broker
        .execute_synthetic_input(
            &mut instance,
            &issuer,
            crate::SyntheticRequest {
                token: release_token,
                connection,
                namespace,
                connection_generation: 29,
                action: crate::SyntheticAction::Release,
                input,
            },
            Some(later_focus.raw()),
        )
        .expect("the release to execute");
    assert_eq!(
        released.completion,
        sophia_input_authority::RequestCompletion::Processed
    );

    let Some(sophia_input_authority::ReleaseOutcome::DeliverTo(incarnation)) = released.release
    else {
        panic!("the last holder letting go owes a delivery: {:?}", released.release);
    };
    assert_eq!(
        incarnation.recipient,
        first_focus.raw(),
        "the release is owed to the client the press reached, not the one focus now names"
    );
    assert!(
        released.record.is_none(),
        "a release proposes no recipient of its own"
    );
}

#[test]
fn a_gated_broker_keeps_working_after_a_different_gate_is_refused() {
    let namespace = NamespaceId::from_raw(43);
    let client = XServerFrontendClientId(60);
    let surface = SurfaceId::new(47, 1);
    let window = XResourceId::new(0x200110, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (first_gate, _fi, _fs) = control_gate();
    let (second_gate, _si, _ss) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .try_install_control_gate(first_gate.clone())
        .expect("a broker with no gate to accept one");

    // A different coordinator is refused, and the refusal is reported rather
    // than swallowed by the cell that takes only one value.
    assert_eq!(
        broker.try_install_control_gate(second_gate),
        Err(crate::ActivationRefused::DifferentGateInstalled)
    );
    // Installing the same one again is not an error; it changes nothing.
    broker
        .try_install_control_gate(first_gate.clone())
        .expect("the installed gate to be idempotent");

    // The broker still exists and still works. A consuming form that refused
    // would have had to drop it to report, destroying the instance that was
    // supposed to stay as it was and stranding this client's queue.
    //
    // Note what this does and does not show: the broker was already under
    // first_gate before the rejection, so this is a gated instance surviving a
    // refused second gate. That an ORDINARY broker stays ungated after a
    // refused first install is a different case, and belongs with the
    // constructor work that refuses on exposed ingress or queued raw work.
    broker
        .routed_input_sender()
        .send(motion_to(surface, XAuthorityInputDeliveryId::from_raw(90)))
        .expect("the installed gate to admit work");
    assert_eq!(broker.route_pending(), Ok(1));
    assert!(
        channels.input.try_recv().is_ok(),
        "authorised work continues after a refused activation"
    );
}

#[test]
fn reinstalling_the_same_gate_does_not_disturb_a_transition_in_flight() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer, _submit) = control_gate_with_submit();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a broker with no gate to accept one");

    gate.with(|coordinator| {
        coordinator
            .request(
                &mut instance,
                &issuer,
                crate::TransitionKind::SecurityControl,
                1,
                1,
            )
            .expect("the transition to be requested");
    })
    .expect("the gate");

    // Idempotent means changed nothing, not restarted. Reopening or resetting
    // here would let a caller clear a transition in flight by reinstalling the
    // coordinator that opened it.
    broker
        .try_install_control_gate(gate.clone())
        .expect("the installed gate to be idempotent");

    gate.with(|coordinator| {
        assert!(
            !coordinator.is_open(),
            "the transition must still be in flight"
        );
        assert_eq!(coordinator.applied_control_epoch(), 0);
    })
    .expect("the gate");

    // And routing is still closed, so nothing was admitted meanwhile.
    assert!(gate.stamp().is_err());
}

#[test]
fn an_ordinary_broker_that_exposed_raw_ingress_stays_ordinary() {
    let namespace = NamespaceId::from_raw(44);
    let client = XServerFrontendClientId(61);
    let surface = SurfaceId::new(48, 1);
    let window = XResourceId::new(0x200120, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _authority, _issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    // A raw handle is taken while the instance is ordinary. It cannot be
    // recalled, and a send through it that already returned cannot be
    // answered afterwards.
    let raw = broker
        .input_sender()
        .expect("an ungated broker to expose raw ingress");

    assert_eq!(
        broker.try_install_control_gate(gate),
        Err(crate::ActivationRefused::RawIngressAlreadyExposed),
        "an instance with an unstamped way in must not become private"
    );

    // It stays ordinary: the raw handle still works and its work still routes.
    raw.send(XAuthorityClientInputEvent {
        client,
        event: XAuthorityKeyEvent {
            keycode: 24,
            pressed: true,
            state: 0,
            modifiers_after: 0,
            time_msec: 1,
        }
        .into(),
        target_window: None,
        xi_event_type: None,
        xi_event_window: None,
        xi_emulated_button_type: None,
        xi_emulated_button_window: None,
        xi_pointer_crossing_mask: 0,
        grab_crossing: None,
        grab_target: None,
        propagation_stop: None,
        delivery: None,
    })
    .expect("the ordinary path to keep taking raw work");
    assert!(broker.route_pending().is_ok());
    assert!(
        channels.input.try_recv().is_ok(),
        "an ordinary instance keeps serving after a refused activation"
    );
}

#[test]
fn raw_ingress_is_refused_under_a_coordinator() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _authority, _issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate)
        .expect("a fresh broker to accept its gate");

    // Being absent from what a private constructor returns is not enough:
    // this is a public method on a public type and has to refuse itself.
    assert_eq!(
        broker.input_sender().err(),
        Some(crate::ActivationRefused::RawIngressRefusedUnderGate)
    );
}

#[test]
fn exposure_outlives_the_handle_that_caused_it() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _authority, _issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );

    let raw = broker
        .input_sender()
        .expect("an ungated broker to expose raw ingress");
    // Every handle gone, and nothing queued through it.
    drop(raw);

    // Still refused. Dropping a handle does not undo a send that already
    // returned, so forgetting the exposure would let this instance become
    // private with unanswerable work behind it.
    assert_eq!(
        broker.try_install_control_gate(gate),
        Err(crate::ActivationRefused::RawIngressAlreadyExposed)
    );
}

#[test]
fn a_frontend_built_private_stamps_from_the_gate_it_was_built_with() {
    let surface = SurfaceId::new(49, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();

    // The coordinator exists before the broker does, so there is no interval
    // in which a handle could be taken from an ungated instance.
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    // The gate this instance derived from the authority it owns, not a
    // separately built one: a coordinator paired with a different
    // authority would be driving an identity this frontend never had.
    let gate = private.control_gate().clone();
    // No client or surface registered: enqueue is an admission decision, and
    // admission does not depend on there being somewhere to route to yet.
    //
    // Actually send, rather than asking the gate a question the sender was
    // never involved in. Open: admitted.
    let sender = private.ingress();
    sender
        .submit(&service_keeper.lease(), motion_to(surface, XAuthorityInputDeliveryId::from_raw(95)))
        .expect("an open coordinator to admit work");

    // Close THIS gate. If the sender were stamping from anything else, it
    // would carry on admitting.
    gate.with(|coordinator| {
        coordinator
            .request_through(&private, crate::TransitionKind::SecurityControl, 1, 1)
            .expect("the transition to be requested");
    })
    .expect("the gate");

    assert!(
        sender
            .submit(&service_keeper.lease(), motion_to(surface, XAuthorityInputDeliveryId::from_raw(96)))
            .is_err(),
        "the sender must stamp from the coordinator this frontend was built with"
    );
}

#[test]
fn the_private_host_delivers_each_admitted_input_exactly_once() {
    let namespace = NamespaceId::from_raw(46);
    let client = XServerFrontendClientId(63);
    let surface = SurfaceId::new(50, 1);
    let window = XResourceId::new(0x200140, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    // Both from ONE sender. This says nothing about ordering across sources,
    // which is what the shared stream is for and what consumer-side staging
    // cannot establish; it says each admitted item runs once and reaches its
    // client.
    let sender = private.ingress();
    for delivery in [100u64, 101] {
        sender
            .submit(&service_keeper.lease(), motion_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
            ))
            .expect("an open coordinator to admit work");
    }

    let ran = private.route_pending(&service_keeper.lease()).expect("the ordered pass to run");
    assert_eq!(ran.len(), 2, "both accepted operations ran");
    assert!(channels.input.try_recv().is_ok());
    assert!(channels.input.try_recv().is_ok());
    assert!(
        channels.input.try_recv().is_err(),
        "each admitted item is delivered once, not twice"
    );
}

#[test]
fn the_private_host_never_drains_raw_ingress() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));

    // Raw ingress is not one of the sources the ordered pass reads, and a
    // private instance will not hand out a handle to it either.
    assert_eq!(
        private.broker.input_sender().err(),
        Some(crate::ActivationRefused::RawIngressRefusedUnderGate)
    );
    assert_eq!(
        private.route_pending(&service_keeper.lease()).expect("an empty ordered pass").len(),
        0,
        "nothing to run, and no raw source to find any in"
    );
}

#[test]
fn the_private_host_revokes_work_whose_revision_closed_before_it_ran() {
    let namespace = NamespaceId::from_raw(47);
    let client = XServerFrontendClientId(64);
    let surface = SurfaceId::new(51, 1);
    let window = XResourceId::new(0x200150, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    // The gate this instance derived from the authority it owns, not a
    // separately built one: a coordinator paired with a different
    // authority would be driving an identity this frontend never had.
    let gate = private.control_gate().clone();
    let (_registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let delivery = XAuthorityInputDeliveryId::from_raw(110);
    private
        .ingress()
        .submit(&service_keeper.lease(), motion_to(surface, delivery))
        .expect("an open coordinator to admit work");

    // The revision it was stamped under closes before the ordered pass runs.
    gate.with(|coordinator| {
        coordinator
            .request_through(&private, crate::TransitionKind::SecurityControl, 1, 1)
            .expect("the transition to be requested");
    })
    .expect("the gate");

    private.route_pending(&service_keeper.lease()).expect("the ordered pass to run");

    // Asking only whether a coordinator exists would have delivered this.
    assert!(
        channels.input.try_recv().is_err(),
        "work stamped under a closed revision must not reach the client"
    );
    assert_eq!(
        delivery_receiver.recv().unwrap(),
        XAuthorityClientInputDelivery {
            client,
            delivery,
            outcome: XAuthorityInputDeliveryOutcome::EpochRevoked,
        }
    );
}

#[test]
fn a_full_ready_stream_leaves_work_in_its_channel_rather_than_destroying_it() {
    let namespace = NamespaceId::from_raw(48);
    let client = XServerFrontendClientId(65);
    let surface = SurfaceId::new(52, 1);
    let window = XResourceId::new(0x200160, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(16);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    // Ingress capacity larger than the ready stream's ordinary share, so more
    // can be sent than one pass can admit.
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(16).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let sender = private.ingress();
    let sent = 16u64;
    for delivery in 0..sent {
        sender
            .submit(&service_keeper.lease(), motion_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(200 + delivery),
            ))
            .expect("an open coordinator to admit work");
    }

    // However many passes it takes, everything sent is eventually delivered.
    // Taking from a channel without room to admit would have destroyed the
    // difference, silently.
    let mut delivered = 0usize;
    for _ in 0..8 {
        delivered += private.route_pending(&service_keeper.lease()).expect("an ordered pass").len();
    }
    assert_eq!(
        delivered, sent as usize,
        "work a pass could not admit waits in its channel rather than vanishing"
    );
    let mut received = 0usize;
    while channels.input.try_recv().is_ok() {
        received += 1;
    }
    assert_eq!(received, sent as usize);
}

#[test]
fn a_private_producer_is_told_denial_apart_from_saturation() {
    let namespace = NamespaceId::from_raw(49);
    let client = XServerFrontendClientId(66);
    let surface = SurfaceId::new(53, 1);
    let window = XResourceId::new(0x200170, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(2).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    // The gate this instance derived from the authority it owns, not a
    // separately built one: a coordinator paired with a different
    // authority would be driving an identity this frontend never had.
    let gate = private.control_gate().clone();
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    // Open: accepted.
    private
        .submit(&service_keeper.lease(), motion_to(surface, XAuthorityInputDeliveryId::from_raw(300)))
        .expect("an open coordinator to accept work");

    // Fill the bounded ingress. These are saturation, not policy.
    let mut saturated = false;
    for delivery in 301..320u64 {
        match private.submit(&service_keeper.lease(), motion_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(delivery),
        )) {
            Ok(_) => {}
            Err(crate::PrivateSendError::Saturated(_)) => {
                saturated = true;
                break;
            }
            Err(other) => panic!("a full ingress is saturation, not {other:?}"),
        }
    }
    assert!(saturated, "the bounded ingress filled");

    // Now close the gate. The answer changes from 'try again' to 'refused',
    // which the ordinary path reports as Full either way.
    gate.with(|coordinator| {
        coordinator
            .request_through(&private, crate::TransitionKind::SecurityControl, 1, 1)
            .expect("the transition to be requested");
    })
    .expect("the gate");

    match private.submit(&service_keeper.lease(), motion_to(surface, XAuthorityInputDeliveryId::from_raw(330))) {
        Err(crate::PrivateSendError::Denied(_)) => {}
        other => panic!("a closed revision is a denial, not {other:?}"),
    }
}

#[test]
fn nothing_accepted_is_lost_when_a_pass_cannot_admit_it_all() {
    let namespace = NamespaceId::from_raw(50);
    let client = XServerFrontendClientId(67);
    let surface = SurfaceId::new(54, 1);
    let window = XResourceId::new(0x200180, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    // The production constructor at its smallest: ready capacity six, of
    // which four are held for cleanup, so ordinary work has room for two.
    let service_keeper = service_owner(&crate::PrivateSettlementOwner::default(), 16);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &service_keeper,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    // One routed input and one control, both accepted by producers into the
    // shared order rather than left in channels for a later pass to collect.
    private
        .ingress()
        .submit(&service_keeper.lease(), motion_to(surface, XAuthorityInputDeliveryId::from_raw(9001)))
        .expect("an open coordinator to accept work");
    private
        .control_producer()
        .submit(&service_keeper.lease(), XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(7),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    // Both arrive. Conservation is the property: work a producer was told was
    // accepted is not allowed to disappear.
    let mut ran = 0usize;
    for _ in 0..6 {
        ran += private.route_pending(&service_keeper.lease()).expect("an ordered pass").len();
    }
    assert_eq!(ran, 2, "both accepted operations ran across the passes");
    assert!(channels.input.try_recv().is_ok(), "the input was delivered");
    assert!(
        channels.control.try_recv().is_ok(),
        "the control reached its client rather than being destroyed"
    );
}
