// Publication and security transitions: what a privileged input may do across
// one, what a stamp means afterwards, and what is refused inside the gap.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


#[test]
fn pending_control_gets_the_next_output_lock() {
    let (socket, _peer) = UnixStream::pair().expect("socket pair");
    let stream = Arc::new(Mutex::new(socket));
    let held = stream.lock().expect("initial output lock");
    let control_pending = Arc::new(AtomicUsize::new(0));
    let (normal_started_sender, normal_started_receiver) = sync_channel(1);
    let (order_sender, order_receiver) = sync_channel(2);

    let normal_stream = stream.clone();
    let normal_pending = control_pending.clone();
    let normal_order = order_sender.clone();
    let normal = std::thread::spawn(move || {
        normal_started_sender.send(()).expect("normal started");
        let normal_wire = X11WirePermission::open();
        let _guard =
            lock_x11_non_control_output(&normal_stream, &normal_wire, &normal_pending, None)
            .expect("normal output lock");
        normal_order.send("normal").expect("normal order");
    });
    normal_started_receiver.recv().expect("normal waiting");

    let control_stream = stream.clone();
    let control_pending_probe = control_pending.clone();
    let control = std::thread::spawn(move || {
        let _priority = X11ControlOutputPriority::new(control_pending);
        let _guard = control_stream.lock().expect("control output lock");
        order_sender.send("control").expect("control order");
    });
    while control_pending_probe.load(Ordering::Acquire) == 0 {
        std::thread::yield_now();
    }
    drop(held);

    assert_eq!(
        order_receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
        "control"
    );
    assert_eq!(
        order_receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
        "normal"
    );
    control.join().expect("control owner");
    normal.join().expect("normal owner");
}

/// A coordinator over a fresh authority, plus the issuer that drives it.
fn control_gate() -> (
    crate::ControlEpochGate,
    sophia_input_authority::AuthorityInstance,
    sophia_input_authority::IssuerHandle,
) {
    let (gate, instance, issuer, _submit) = control_gate_with_submit();
    (gate, instance, issuer)
}

/// The same authority the gate drives, with its submit handle.
///
/// Execution tests must use this one. Building a second AuthorityInstance and
/// executing against that proves nothing about the broker under this gate --
/// it is the cross-instance confusion the control path was repaired for.
fn control_gate_with_submit() -> (
    crate::ControlEpochGate,
    sophia_input_authority::AuthorityInstance,
    sophia_input_authority::IssuerHandle,
    sophia_input_authority::SubmitHandle,
) {
    let binding = sophia_input_authority::SeatBinding::new(
        sophia_input_authority::InstanceId::new(1),
        sophia_protocol::SeatId::from_raw(1),
    );
    let (instance, issuer, submit) = sophia_input_authority::AuthorityInstance::new(
        binding,
        sophia_input_authority::Capacity::PLANNED,
        9,
    )
    .expect("planned capacity");
    let coordinator = crate::ControlEpochCoordinator::derive(&instance, &issuer)
        .expect("a published revision to derive from");
    (
        crate::ControlEpochGate::new(coordinator),
        instance,
        issuer,
        submit,
    )
}

/// The authority a private frontend owns, with both of its roles.
///
/// No gate: the frontend derives its own from this instance, so a test that
/// built one here and handed it over would be pairing a coordinator with an
/// authority it does not describe.
fn private_authority() -> (
    sophia_input_authority::AuthorityInstance,
    sophia_input_authority::IssuerHandle,
    sophia_input_authority::SubmitHandle,
) {
    let binding = sophia_input_authority::SeatBinding::new(
        sophia_input_authority::InstanceId::new(1),
        sophia_protocol::SeatId::from_raw(1),
    );
    sophia_input_authority::AuthorityInstance::new(
        binding,
        sophia_input_authority::Capacity::PLANNED,
        9,
    )
    .expect("planned capacity")
}

/// Drive a transition against the authority a private frontend holds.
///
/// The coordinator is already held by the caller and common is acquired
/// inside, which is the documented order. Reaching the other way -- taking
/// common and then asking for the coordinator -- is the inversion.
trait TransitionThroughPrivate {
    fn request_through(
        &mut self,
        private: &crate::PrivateXServerFrontend,
        kind: crate::TransitionKind,
        control_epoch: u64,
        publication: u64,
    ) -> Result<crate::TransitionToken, crate::ControlEpochRefusal>;
}

impl TransitionThroughPrivate for crate::TransitionAccess<'_> {
    fn request_through(
        &mut self,
        private: &crate::PrivateXServerFrontend,
        kind: crate::TransitionKind,
        control_epoch: u64,
        publication: u64,
    ) -> Result<crate::TransitionToken, crate::ControlEpochRefusal> {
        private
            .authority()
            .under_common_as_origin(|authority, issuer| {
                self.request(authority, issuer, kind, control_epoch, publication)
            })
            .expect("the authority to be reachable")
    }
}

fn button_to(
    surface: SurfaceId,
    delivery: XAuthorityInputDeliveryId,
    button: u32,
    pressed: bool,
) -> XAuthorityRoutedInput {
    XAuthorityRoutedInput {
        request: RoutedInputRequest {
            serial: 1,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(2),
            time_msec: 1,
            target_surface: surface,
            global_position: Point::default(),
            local_position: Point::default(),
            kind: InputEventKind::PointerButton { button, pressed },
        },
        route_lease: None,
        delivery: Some(delivery),
        mode: XAuthorityRoutedInputMode::Deliver,
        origin: XAuthorityRoutedInputOrigin::Physical,
    }
}

fn motion_to(surface: SurfaceId, delivery: XAuthorityInputDeliveryId) -> XAuthorityRoutedInput {
    XAuthorityRoutedInput {
        request: RoutedInputRequest {
            serial: 1,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(2),
            time_msec: 1,
            target_surface: surface,
            global_position: Point::default(),
            local_position: Point::default(),
            kind: InputEventKind::PointerMotion,
        },
        route_lease: None,
        delivery: Some(delivery),
        mode: XAuthorityRoutedInputMode::Deliver,
        origin: XAuthorityRoutedInputOrigin::Physical,
    }
}

#[test]
fn a_transition_in_flight_refuses_to_stamp_new_routed_input() {
    let namespace = NamespaceId::from_raw(24);
    let client = XServerFrontendClientId(20);
    let surface = SurfaceId::new(34, 1);
    let window = XResourceId::new(0x200040, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let (_registration, _channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    let sender = broker.routed_input_sender();

    // Open, so work is stamped and accepted.
    sender
        .send(motion_to(surface, XAuthorityInputDeliveryId::from_raw(50)))
        .expect("an open coordinator to admit work");

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

    // Closed. There is no stamp to give, so the work is refused here rather
    // than queued against a revision that is being replaced.
    assert!(
        sender
            .send(motion_to(surface, XAuthorityInputDeliveryId::from_raw(51)))
            .is_err(),
        "a transition in flight must refuse new routed input"
    );
}

#[test]
fn input_stamped_before_a_transition_is_not_delivered_after_it() {
    let namespace = NamespaceId::from_raw(25);
    let client = XServerFrontendClientId(21);
    let surface = SurfaceId::new(35, 1);
    let window = XResourceId::new(0x200050, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, delivery_receiver) = channel();
    let (gate, mut instance, issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    let sender = broker.routed_input_sender();
    let delivery = XAuthorityInputDeliveryId::from_raw(52);
    sender
        .send(motion_to(surface, delivery))
        .expect("an open coordinator to admit work");

    // A whole transition completes between stamping and routing.
    gate.with(|coordinator| {
        let token = coordinator
            .request(
                &mut instance,
                &issuer,
                crate::TransitionKind::SecurityControl,
                1,
                1,
            )
            .expect("the transition to be requested");
        coordinator
            .apply(token, crate::TransitionInstallation::security_control())
            .expect("the transition to apply");
        coordinator
            .reopen(&mut instance, &issuer, 1)
            .expect("the transition to reopen");
    })
    .expect("the gate");

    assert_eq!(broker.route_pending(), Ok(1));
    // The stamp it carried is the one it was given, and that revision is gone.
    assert_eq!(channels.input.try_recv(), Err(TryRecvError::Empty));
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
fn a_sender_taken_before_the_gate_was_installed_is_still_gated() {
    let namespace = NamespaceId::from_raw(26);
    let client = XServerFrontendClientId(22);
    let surface = SurfaceId::new(36, 1);
    let window = XResourceId::new(0x200060, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    let (_registration, _channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    // Taken while the broker is still ungated.
    let early_sender = broker.routed_input_sender();

    let (gate, mut instance, issuer) = control_gate();
    let mut broker = broker;
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

    // The early sender shares the broker's cell rather than a copy of it, so
    // it cannot keep stamping from the bare counter into a gated broker.
    assert!(
        early_sender
            .send(motion_to(surface, XAuthorityInputDeliveryId::from_raw(53)))
            .is_err(),
        "a sender taken before the gate must not route around it"
    );
}

#[test]
fn the_lockless_epoch_advance_is_refused_under_a_gate() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _authority, _issuer) = control_gate();
    let broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    let ungated_sender = broker.routed_input_sender();
    // Ungated, the bare advance is the ordinary mechanism and still works.
    assert!(ungated_sender.advance_control_epoch(2));

    let mut broker = broker;
    broker
        .try_install_control_gate(gate)
        .expect("a broker with no gate to accept one");
    let sender = broker.routed_input_sender();

    // Gated, a coordinator owns every transition, so this escape is closed
    // rather than left to race the ranked apply.
    assert!(!sender.advance_control_epoch(3));
    assert!(!ungated_sender.advance_control_epoch(4));
}

#[test]
fn the_privileged_apply_clears_every_population_and_reports_them_together() {
    let namespace = NamespaceId::from_raw(27);
    let client = XServerFrontendClientId(23);
    let surface = SurfaceId::new(37, 1);
    let window = XResourceId::new(0x200070, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, delivery_receiver) = channel();
    let (gate, mut instance, issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let (_registration, _channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_pointer(
            namespace,
            crate::XActiveInputGrab {
                owner: client.raw(),
                window,
                owner_events: false,
                pointer_mode: 0,
                keyboard_mode: 1,
                event_mask: u16::MAX,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();
    // A synchronous grab freezes delivery, so this lands in the frozen queue
    // rather than reaching the client.
    let delivery = XAuthorityInputDeliveryId::from_raw(60);
    broker
        .routed_input_sender()
        .send(motion_to(surface, delivery))
        .expect("an open coordinator to admit work");
    assert_eq!(broker.route_pending(), Ok(1));

    let routed = gate
        .with(|coordinator| {
            let token = coordinator
                .request(
                    &mut instance,
                    &issuer,
                    crate::TransitionKind::SecurityControl,
                    1,
                    1,
                )
                .expect("the transition to be requested");
            // Session installs the snapshot; the X side speaks for the rest.
            let outcome = broker
                .apply_control_transition(&instance.control_permit(&issuer).expect("the issuer to hold a permit"), coordinator, token, true)
                .expect("the transition to apply");
            assert!(outcome.applied());
            assert_eq!(coordinator.applied_control_epoch(), 1);
            broker
                .report_control_transition(outcome)
                .unwrap_or_else(|(_, _)| panic!("the origin broker to accept its own batch"))
        })
        .expect("the gate");

    // The grab is gone, and the frozen work was reported revoked rather than
    // delivered into the revision that replaced it.
    assert_eq!(routed, 1);
    // Taken, not copied: work left behind here would be routed again into the
    // revision that replaced it.
    assert!(
        broker.registry.frozen_input.lock().unwrap().is_empty(),
        "the frozen queue must be emptied by the transition that revoked it"
    );
    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace)
            .is_none(),
        "a security transition must clear active grabs"
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
fn the_privileged_apply_does_not_stamp_without_the_session_snapshot() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");

    gate.with(|coordinator| {
        let token = coordinator
            .request(
                &mut instance,
                &issuer,
                crate::TransitionKind::SecurityControl,
                1,
                1,
            )
            .expect("the transition to be requested");
        // Everything this path can clear is cleared, but Session has not
        // installed the snapshot, so the transition is not applied.
        assert!(
            broker
                .apply_control_transition(&instance.control_permit(&issuer).expect("the issuer to hold a permit"), coordinator, token, false)
                .is_err(),
            "an incomplete installation must not stamp the applied epoch"
        );
        assert_eq!(coordinator.applied_control_epoch(), 0);
        assert!(!coordinator.is_open());
    })
    .expect("the gate");
}

#[test]
fn a_publication_transition_preserves_grabs_and_frozen_input() {
    let namespace = NamespaceId::from_raw(28);
    let client = XServerFrontendClientId(24);
    let surface = SurfaceId::new(38, 1);
    let window = XResourceId::new(0x200080, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let (_registration, _channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_pointer(
            namespace,
            crate::XActiveInputGrab {
                owner: client.raw(),
                window,
                owner_events: false,
                pointer_mode: 0,
                keyboard_mode: 1,
                event_mask: u16::MAX,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();
    broker
        .routed_input_sender()
        .send(motion_to(surface, XAuthorityInputDeliveryId::from_raw(70)))
        .expect("an open coordinator to admit work");
    assert_eq!(broker.route_pending(), Ok(1));

    gate.with(|coordinator| {
        let token = coordinator
            .request(&mut instance, &issuer, crate::TransitionKind::Publication, 0, 1)
            .expect("a publication-only transition to be requested");
        let outcome = broker
            .apply_control_transition(&instance.control_permit(&issuer).expect("the issuer to hold a permit"), coordinator, token, true)
            .expect("the publication to apply");
        assert!(outcome.applied());
        assert_eq!(
            broker
                .report_control_transition(outcome)
                .unwrap_or_else(|(_, _)| panic!("the origin broker to accept its own batch")),
            0,
            "a publication revokes nothing, so it owes no receipts"
        );
    })
    .expect("the gate");

    // A focus change must not cost this client its grab or its frozen work.
    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace)
            .is_some(),
        "a publication-only transition must preserve active grabs"
    );
    assert_eq!(
        broker.registry.frozen_input.lock().unwrap().len(),
        1,
        "a publication-only transition must preserve frozen input"
    );
}

#[test]
fn a_foreign_token_is_refused_before_anything_is_destroyed() {
    let namespace = NamespaceId::from_raw(29);
    let client = XServerFrontendClientId(25);
    let surface = SurfaceId::new(39, 1);
    let window = XResourceId::new(0x200090, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer) = control_gate();
    let (other_gate, mut other_instance, other_issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let (_registration, _channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_pointer(
            namespace,
            crate::XActiveInputGrab {
                owner: client.raw(),
                window,
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: u16::MAX,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();

    let foreign = other_gate
        .with(|other| {
            other
                .request(
                    &mut other_instance,
                    &other_issuer,
                    crate::TransitionKind::SecurityControl,
                    1,
                    1,
                )
                .expect("the other transition to be requested")
        })
        .expect("the other gate");

    gate.with(|coordinator| {
        coordinator
            .request(
                &mut instance,
                &issuer,
                crate::TransitionKind::SecurityControl,
                1,
                1,
            )
            .expect("this transition to be requested");
        assert!(
            broker
                .apply_control_transition(&instance.control_permit(&issuer).expect("the issuer to hold a permit"), coordinator, foreign, true)
                .is_err(),
            "a token from another coordinator must not drive this transition"
        );
    })
    .expect("the gate");

    // Refused before the clearing, so the grab this token had no standing to
    // touch is still there.
    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace)
            .is_some(),
        "a refused token must not destroy live state"
    );
}

#[test]
fn an_authority_with_no_transition_open_cannot_drive_anothers() {
    let namespace = NamespaceId::from_raw(30);
    let client = XServerFrontendClientId(26);
    let surface = SurfaceId::new(40, 1);
    let window = XResourceId::new(0x2000a0, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, mut instance, issuer) = control_gate();
    // A second authority, entirely uninvolved, with its own legitimate issuer.
    let (_other_gate, mut other_instance, other_issuer) = control_gate();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let (_registration, _channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_pointer(
            namespace,
            crate::XActiveInputGrab {
                owner: client.raw(),
                window,
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: u16::MAX,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();

    gate.with(|coordinator| {
        let token = coordinator
            .request(
                &mut instance,
                &issuer,
                crate::TransitionKind::SecurityControl,
                1,
                1,
            )
            .expect("the transition to be requested");

        // The token and coordinator are this transition's own, but the
        // authority presented has nothing in flight. Its revision is published,
        // so it cannot be the one this transition was opened against.
        assert!(
            broker
                .apply_control_transition(
                    &other_instance.control_permit(&other_issuer).expect("the issuer to hold a permit"),
                    coordinator,
                    token,
                    true
                )
                .is_err(),
            "an authority with no transition open must not drive another's"
        );
    })
    .expect("the gate");

    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace)
            .is_some(),
        "the refused apply must not have cleared anything"
    );
}

/// A broker with a client, a surface and an active grab, for the cross-broker
/// negatives below.
fn gated_broker_with_grab(
    gate: &crate::ControlEpochGate,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
    surface: SurfaceId,
    window: XResourceId,
) -> (
    XServerFrontendRouteBroker,
    std::sync::mpsc::Receiver<XAuthorityClientInputDelivery>,
    impl std::any::Any,
) {
    let (control_ack_sender, control_ack_receiver) = sync_channel(4);
    let (delivery_sender, delivery_receiver) = channel();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    broker
        .try_install_control_gate(gate.clone())
        .expect("a fresh broker to accept its gate");
    let registration = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_pointer(
            namespace,
            crate::XActiveInputGrab {
                owner: client.raw(),
                window,
                owner_events: false,
                pointer_mode: 0,
                keyboard_mode: 1,
                event_mask: u16::MAX,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();
    // The registration lease retires the client when it drops, taking its
    // grabs with it, so it has to outlive the caller's assertions.
    (broker, delivery_receiver, Box::new((registration, control_ack_receiver)))
}

#[test]
fn another_coordinators_transition_cannot_clear_this_brokers_populations() {
    let namespace = NamespaceId::from_raw(31);
    let client = XServerFrontendClientId(27);
    let surface = SurfaceId::new(41, 1);
    let window = XResourceId::new(0x2000b0, 1);
    let (gate, _authority, _issuer) = control_gate();
    let (other_gate, mut other_instance, other_issuer) = control_gate();
    let (broker, _deliveries, _lease) =
        gated_broker_with_grab(&gate, namespace, client, surface, window);

    // Everything here is valid on its own terms: the other coordinator, its
    // own authority's permit, and a token it really did issue. None of it says
    // anything about this broker.
    other_gate
        .with(|other| {
            let token = other
                .request(
                    &mut other_instance,
                    &other_issuer,
                    crate::TransitionKind::SecurityControl,
                    1,
                    1,
                )
                .expect("the other transition to be requested");
            assert!(
                broker
                    .apply_control_transition(
                        &other_instance
                            .control_permit(&other_issuer)
                            .expect("the issuer to hold a permit"),
                        other,
                        token,
                        true
                    )
                    .is_err(),
                "a broker must refuse a coordinator it is not under"
            );
        })
        .expect("the other gate");

    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace)
            .is_some(),
        "another coordinator's transition must not clear this broker's grabs"
    );
}

#[test]
fn an_ungated_broker_refuses_a_privileged_transition_entirely() {
    let namespace = NamespaceId::from_raw(32);
    let client = XServerFrontendClientId(28);
    let surface = SurfaceId::new(42, 1);
    let window = XResourceId::new(0x2000c0, 1);
    let (gate, mut instance, issuer) = control_gate();
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    // Deliberately never put under a gate.
    let broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    let (_registration, _channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_pointer(
            namespace,
            crate::XActiveInputGrab {
                owner: client.raw(),
                window,
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: u16::MAX,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();

    gate.with(|coordinator| {
        let token = coordinator
            .request(
                &mut instance,
                &issuer,
                crate::TransitionKind::SecurityControl,
                1,
                1,
            )
            .expect("the transition to be requested");
        assert!(
            broker
                .apply_control_transition(
                    &instance
                        .control_permit(&issuer)
                        .expect("the issuer to hold a permit"),
                    coordinator,
                    token,
                    true
                )
                .is_err(),
            "an ungated broker has no coordinator and must refuse one"
        );
    })
    .expect("the gate");

    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace)
            .is_some(),
        "an ungated broker must not be cleared by a coordinator"
    );
}

#[test]
fn one_brokers_receipts_cannot_be_delivered_by_another() {
    let (gate, mut instance, issuer) = control_gate();
    let (mut broker, deliveries, _lease) = gated_broker_with_grab(
        &gate,
        NamespaceId::from_raw(33),
        XServerFrontendClientId(29),
        SurfaceId::new(43, 1),
        XResourceId::new(0x2000d0, 1),
    );
    // Work frozen behind that grab, so the revoked batch is not empty and the
    // return path has something to prove.
    broker
        .routed_input_sender()
        .send(motion_to(
            SurfaceId::new(43, 1),
            XAuthorityInputDeliveryId::from_raw(80),
        ))
        .expect("an open coordinator to admit work");
    assert_eq!(broker.route_pending(), Ok(1));
    // A second broker under the same coordinator, with a client whose
    // identifier collides, which is ordinary: client ids are unique per
    // frontend, not across frontends.
    let (other_broker, other_deliveries, _other_lease) = gated_broker_with_grab(
        &gate,
        NamespaceId::from_raw(33),
        XServerFrontendClientId(29),
        SurfaceId::new(43, 1),
        XResourceId::new(0x2000d0, 1),
    );

    let outcome = gate
        .with(|coordinator| {
            let token = coordinator
                .request(
                    &mut instance,
                    &issuer,
                    crate::TransitionKind::SecurityControl,
                    1,
                    1,
                )
                .expect("the transition to be requested");
            broker
                .apply_control_transition(
                    &instance
                        .control_permit(&issuer)
                        .expect("the issuer to hold a permit"),
                    coordinator,
                    token,
                    true,
                )
                .expect("the transition to apply")
        })
        .expect("the gate");

    let Err((_, returned)) = other_broker.report_control_transition(outcome) else {
        panic!("receipts must be delivered by the broker that revoked the work");
    };
    assert!(
        other_deliveries.try_recv().is_err(),
        "the other broker's clients must receive nothing"
    );

    // The refusal handed the batch back rather than consuming it, so the work
    // it revoked is still answerable. Dropping it would turn a caller's
    // mistake into receipts that no client ever gets.
    assert_eq!(
        broker
            .report_control_transition(returned)
            .unwrap_or_else(|(_, _)| panic!("the origin broker to accept its own batch")),
        1,
        "the origin must still be able to deliver what it revoked"
    );
    assert_eq!(
        deliveries.recv().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::EpochRevoked
    );
}

/// A grant, a device and a reserved request, ready to execute.
fn reserved_request(
    instance: &mut sophia_input_authority::AuthorityInstance,
    issuer: &sophia_input_authority::IssuerHandle,
    submit: &sophia_input_authority::SubmitHandle,
    connection: sophia_input_authority::ConnectionIdentity,
) -> (
    sophia_input_authority::RequestToken,
    sophia_input_authority::DeviceCapability,
) {
    let (grant, generation) = instance
        .issue_grant(issuer, connection)
        .expect("a grant to be issued");
    let capability = instance
        .allocate_device(issuer, grant, generation, sophia_protocol::DeviceId::from_raw(1))
        .expect("a device to be allocated");
    let context = sophia_input_authority::ExecutionContext {
        generation,
        connection,
        epoch: 0,
        publication: 0,
        request: 1,
    };
    let token = instance
        .reserve_request(submit, capability, context)
        .expect("a request to be reserved");
    (token, capability)
}

#[test]
fn a_synthetic_press_goes_to_the_grab_holder_rather_than_focus() {
    let namespace = NamespaceId::from_raw(34);
    let grab_holder = XServerFrontendClientId(30);
    let focused = XServerFrontendClientId(31);
    let surface = SurfaceId::new(44, 1);
    let window = XResourceId::new(0x2000e0, 1);
    let (gate, mut instance, issuer, submit) = control_gate_with_submit();
    let (broker, _deliveries, _lease) =
        gated_broker_with_grab(&gate, namespace, grab_holder, surface, window);
    let connection = sophia_input_authority::ConnectionIdentity {
        recipient: 99,
        connection_generation: 5,
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
                connection_generation: 5,
                action: crate::SyntheticAction::Press,
                input: sophia_input_authority::Input::button(1, 9).expect("button one"),
            },
            Some(focused.raw()),
        )
        .expect("the request to execute");

    let record = outcome.record.expect("a recorded press");
    assert!(record.first_press, "the first press begins the hold");
    assert!(record.proposed.grabbed, "a grab must outrank focus");
    assert_eq!(
        record.incarnation.recipient,
        grab_holder.raw(),
        "the press belongs to the client holding the grab, not the focused one"
    );
    assert_eq!(
        outcome.completion,
        sophia_input_authority::RequestCompletion::Processed
    );
}

#[test]
fn a_synthetic_press_follows_focus_when_nothing_is_grabbed() {
    let namespace = NamespaceId::from_raw(35);
    let focused = XServerFrontendClientId(32);
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
        recipient: 98,
        connection_generation: 7,
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
                connection_generation: 7,
                action: crate::SyntheticAction::Press,
                input: sophia_input_authority::Input::key(38).expect("a keycode"),
            },
            Some(focused.raw()),
        )
        .expect("the request to execute");

    let record = outcome.record.expect("a recorded press");
    assert!(!record.proposed.grabbed);
    assert_eq!(record.incarnation.recipient, focused.raw());
}
