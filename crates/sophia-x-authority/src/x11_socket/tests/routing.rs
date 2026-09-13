#[test]
fn routed_pointer_grab_reports_sanitized_lease_confirmation_and_release() {
    let namespace = NamespaceId::from_raw(21);
    let client = XServerFrontendClientId(17);
    let surface = SurfaceId::new(31, 2);
    let admission = sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(8),
        sophia_protocol::NamespaceContext::new(
            namespace,
            sophia_protocol::NamespaceProfile::Confined,
            sophia_protocol::NamespaceCapabilities::NONE,
        )
        .unwrap(),
        sophia_protocol::ClientAuthProvenance::new(
            sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
            5,
        )
        .unwrap(),
    )
    .unwrap();
    let identity = sophia_protocol::ApplicationRouteLeaseIdentity {
        id: sophia_protocol::ApplicationRouteLeaseId::from_raw(3),
        seat: SeatId::from_raw(1),
        frontend_sequence: 4,
        control_epoch: 2,
    };
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (lease_sender, lease_receiver) = sync_channel(4);
    let mut broker = XServerFrontendRouteBroker::with_route_capacities_xkb_and_lease_updates(
        XServerFrontendRouteCapacities::uniform(NonZeroUsize::new(4).unwrap()),
        control_ack_sender,
        delivery_sender,
        lease_sender,
        crate::XkbRmlvoConfig::default(),
    )
    .unwrap();
    let (_registration, channels) = broker
        .registry
        .register_client_with_admission(client, Some(admission))
        .unwrap();
    broker
        .registry
        .register_surface(
            client,
            namespace,
            surface,
            XResourceId::new(0x200001, 1),
        )
        .unwrap();

    broker
        .routed_input_sender()
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 1,
                seat: identity.seat,
                device: DeviceId::from_raw(2),
                time_msec: 1,
                target_surface: surface,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::PointerButton {
                    button: 0x110,
                    pressed: true,
                },
            },
            route_lease: Some(identity),
            delivery: None,
            mode: XAuthorityRoutedInputMode::Deliver,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    assert_eq!(
        lease_receiver.recv().unwrap(),
        XAuthorityRouteLeaseUpdate {
            identity,
            target_surface: surface,
            admission,
            kind: XAuthorityRouteLeaseUpdateKind::Confirmed,
        }
    );
    let _ = channels.input.recv().unwrap();
    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace)
            .is_some()
    );

    broker
        .route_lease_release_sender()
        .send(XAuthorityRouteLeaseRelease {
            identity,
            admission,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    assert_eq!(
        lease_receiver.recv().unwrap(),
        XAuthorityRouteLeaseUpdate {
            identity,
            target_surface: surface,
            admission,
            kind: XAuthorityRouteLeaseUpdateKind::Released,
        }
    );
    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace)
            .is_none()
    );
}

/// Input caught by an epoch advance is revoked, not rejected.
///
/// The session closes the epoch itself, so the events it strands are reported
/// as its own doing. Reporting them as route failures ended a live session the
/// moment the pointer moved during an output policy change.
#[test]
fn security_epoch_revokes_queued_input_and_clears_active_grabs() {
    let namespace = NamespaceId::from_raw(22);
    let client = XServerFrontendClientId(18);
    let surface = SurfaceId::new(32, 1);
    let window = XResourceId::new(0x200020, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, delivery_receiver) = channel();
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

    let sender = broker.routed_input_sender();
    let delivery = XAuthorityInputDeliveryId::from_raw(44);
    sender
        .send(XAuthorityRoutedInput {
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
        })
        .unwrap();
    assert!(sender.advance_control_epoch(2));

    assert_eq!(broker.route_pending(), Ok(1));
    assert_eq!(channels.input.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(
        delivery_receiver.recv().unwrap(),
        XAuthorityClientInputDelivery {
            client,
            delivery,
            outcome: XAuthorityInputDeliveryOutcome::EpochRevoked,
        }
    );
    assert!(
        broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace)
            .is_none()
    );
}

#[test]
fn route_broker_reports_rejected_delivery_for_an_unknown_client() {
    let client = XServerFrontendClientId(12);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(1);
    let (delivery_sender, delivery_receiver) = channel();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(1).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    let delivery = XAuthorityInputDeliveryId::from_raw(7);
    broker
        .input_sender()
        .expect("an ungated broker to expose raw ingress")
        .send(XAuthorityClientInputEvent {
            client,
            event: XAuthorityKeyEvent {
                keycode: 38,
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
            delivery: Some(delivery),
        })
        .unwrap();

    assert_eq!(broker.route_pending(), Ok(0));
    assert_eq!(
        delivery_receiver.recv().unwrap(),
        XAuthorityClientInputDelivery {
            client,
            delivery,
            outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
        }
    );
}

#[test]
fn routed_input_queue_saturation_quarantines_only_the_stalled_client() {
    let stalled = XServerFrontendClientId(30);
    let healthy = XServerFrontendClientId(31);
    let stalled_surface = SurfaceId::new(0x200101, 1);
    let healthy_surface = SurfaceId::new(0x400101, 1);
    let namespace = NamespaceId::from_raw(17);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(1);
    let (delivery_sender, delivery_receiver) = channel();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(1).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    let (_stalled_registration, stalled_channels) =
        broker.registry.register_client(stalled).unwrap();
    let (_healthy_registration, healthy_channels) =
        broker.registry.register_client(healthy).unwrap();
    broker
        .registry
        .register_surface(
            stalled,
            namespace,
            stalled_surface,
            XResourceId::new(0x200101, 1),
        )
        .unwrap();
    broker
        .registry
        .register_surface(
            healthy,
            namespace,
            healthy_surface,
            XResourceId::new(0x400101, 1),
        )
        .unwrap();

    for (serial, delivery) in [(1, None), (2, Some(XAuthorityInputDeliveryId::from_raw(9)))] {
        broker
            .routed_input_sender()
            .send(XAuthorityRoutedInput {
                request: RoutedInputRequest {
                    serial,
                    seat: SeatId::from_raw(1),
                    device: DeviceId::from_raw(1),
                    time_msec: serial,
                    target_surface: stalled_surface,
                    global_position: Point::default(),
                    local_position: Point::default(),
                    kind: if serial == 1 {
                        InputEventKind::PointerMotion
                    } else {
                        InputEventKind::PointerAxis {
                            horizontal_v120: 0,
                            vertical_v120: 120,
                        }
                    },
                },
                route_lease: None,
                delivery,
                mode: XAuthorityRoutedInputMode::Deliver,
            })
            .unwrap();
        assert_eq!(broker.route_pending(), Ok(usize::from(serial == 1)));
    }

    assert_eq!(broker.registered_client_count(), 1);
    assert_eq!(
        delivery_receiver.recv().unwrap(),
        XAuthorityClientInputDelivery {
            client: stalled,
            delivery: XAuthorityInputDeliveryId::from_raw(9),
            outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
        }
    );

    broker
        .routed_input_sender()
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 3,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 3,
                target_surface: healthy_surface,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::PointerMotion,
            },
            route_lease: None,
            delivery: None,
            mode: XAuthorityRoutedInputMode::Deliver,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    assert_eq!(
        healthy_channels.input.recv().unwrap().event,
        XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Motion,
            surface: healthy_surface,
            root_x: 0,
            root_y: 0,
            event_x: 0,
            event_y: 0,
            state: 0,
            time_msec: 3,
        })
    );
    assert!(stalled_channels.input.recv().is_ok());
    assert_eq!(
        stalled_channels.input.try_recv(),
        Err(TryRecvError::Disconnected)
    );
}

#[test]
fn route_broker_retires_control_after_client_disconnect() {
    let client = XServerFrontendClientId(13);
    let surface = SurfaceId::new(14, 1);
    let transaction = TransactionId::from_raw(15);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(1).unwrap());
    let (registration, _channels) = broker.registry.register_client(client).unwrap();
    drop(registration);
    broker
        .control_sender()
        .send(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction,
                surface,
            },
        })
        .unwrap();

    assert_eq!(broker.route_pending(), Ok(1));
    assert_eq!(
        broker.recv_control_ack_timeout(Duration::from_millis(10)),
        Ok(XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::FocusSurface,
                transaction,
                surface,
                outcome: XAuthorityControlOutcome::ClientGone,
            },
        })
    );
}

#[test]
fn thawed_route_cannot_cross_a_destroy_recreate_surface_generation() {
    let namespace = NamespaceId::from_raw(14);
    let client = XServerFrontendClientId(20);
    let old_surface = SurfaceId::new(0x200101, 1);
    let replacement = SurfaceId::new(0x200101, 2);
    let window = XResourceId::new(0x200101, 1);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, old_surface, window)
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
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 1,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 1,
                target_surface: old_surface,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::PointerMotion,
            },
            route_lease: None,
            delivery: None,
            mode: XAuthorityRoutedInputMode::Deliver,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    assert!(channels.input.try_recv().is_err());

    assert!(
        broker.registry.remove_surface(old_surface).unwrap()
    );
    broker
        .registry
        .register_surface(client, namespace, replacement, window)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .ungrab_pointer(namespace, client.raw());

    assert_eq!(broker.route_pending(), Ok(0));
    assert!(channels.input.try_recv().is_err());

    broker
        .routed_input_sender()
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 2,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 2,
                target_surface: replacement,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::PointerMotion,
            },
            route_lease: None,
            delivery: None,
            mode: XAuthorityRoutedInputMode::Deliver,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    assert_eq!(channels.input.recv().unwrap().target_window, Some(window));
}

#[test]
fn duplicate_surface_registration_cannot_replace_its_owner() {
    let namespace = NamespaceId::from_raw(15);
    let owner = XServerFrontendClientId(21);
    let peer = XServerFrontendClientId(22);
    let surface = SurfaceId::new(0x200201, 1);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let (_owner_registration, _owner_channels) = broker.registry.register_client(owner).unwrap();
    let (_peer_registration, _peer_channels) = broker.registry.register_client(peer).unwrap();
    broker
        .registry
        .register_surface(
            owner,
            namespace,
            surface,
            XResourceId::new(0x200201, 1),
        )
        .unwrap();

    assert_eq!(
        broker.registry.register_surface(
            peer,
            namespace,
            surface,
            XResourceId::new(0x400201, 1),
        ),
        Err(XServerFrontendRouteError::DuplicateSurface { surface })
    );
    assert_eq!(
        broker.registry.surface_route_observation(surface).unwrap(),
        Some(XAuthoritySurfaceRouteObservation {
            surface,
            client: owner,
            admission: None,
        })
    );
}

#[test]
fn metadata_candidate_uses_the_surface_owner_route() {
    let namespace = NamespaceId::from_raw(16);
    let owner = XServerFrontendClientId(23);
    let peer = XServerFrontendClientId(24);
    let surface = SurfaceId::new(0x200202, 1);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let metadata = broker.take_metadata_candidate_receiver().unwrap();
    let (_owner_registration, _owner_channels) = broker.registry.register_client(owner).unwrap();
    let (_peer_registration, _peer_channels) = broker.registry.register_client(peer).unwrap();
    broker
        .registry
        .register_surface(
            owner,
            namespace,
            surface,
            XResourceId::new(0x200202, 1),
        )
        .unwrap();

    broker
        .registry
        .emit_metadata_candidate(sophia_protocol::ReducedMetadataCandidate {
            surface,
            label: None,
            disclosure: sophia_protocol::MetadataDisclosure::None,
            generation: 1,
        })
        .unwrap();

    assert_eq!(metadata.recv().unwrap().client, owner);
}

#[test]
fn control_router_bypasses_broker_ingress() {
    let client = XServerFrontendClientId(14);
    let surface = SurfaceId::new(15, 1);
    let transaction = TransactionId::from_raw(16);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(1).unwrap());
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    let command = XAuthorityControlCommand::FocusSurface {
        transaction,
        surface,
    };

    broker
        .control_router()
        .route_control(XAuthorityClientControlCommand { client, command })
        .unwrap();

    assert_eq!(
        channels
            .control
            .try_recv()
            .map(|route| route.authority_command()),
        Ok(Some(command))
    );
}

#[test]
fn active_keyboard_grab_redirects_engine_routed_input_and_window() {
    let namespace = NamespaceId::from_raw(9);
    let focused = XServerFrontendClientId(1);
    let grabber = XServerFrontendClientId(2);
    let surface = SurfaceId::new(10, 1);
    let grab_window = XResourceId::new(0x400001, 1);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(4).unwrap());
    let (_focused_registration, focused_channels) =
        broker.registry.register_client(focused).unwrap();
    let (_grab_registration, grab_channels) = broker.registry.register_client(grabber).unwrap();
    broker
        .registry
        .register_surface(focused, namespace, surface, XResourceId::new(0x200001, 1))
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_keyboard(
            namespace,
            crate::XActiveInputGrab {
                owner: grabber.raw(),
                window: grab_window,
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: 0,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .select_xi_events(namespace, grabber.raw(), grab_window, &[(1, vec![1 << 2])]);
    broker
        .routed_input_sender()
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 1,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 1,
                target_surface: surface,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::Key {
                    keycode: 30,
                    pressed: true,
                },
            },
            route_lease: None,
            delivery: None,
            mode: XAuthorityRoutedInputMode::Deliver,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    assert!(matches!(
        focused_channels.input.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    let routed = grab_channels.input.recv().unwrap();
    assert_eq!(routed.client, grabber);
    assert_eq!(routed.target_window, Some(grab_window));
    assert_eq!(routed.xi_event_type, Some(2));
}

#[test]
fn routed_axis_emits_one_smooth_xi_motion_and_one_legacy_button_pair() {
    let namespace = NamespaceId::from_raw(11);
    let client = XServerFrontendClientId(4);
    let surface = SurfaceId::new(12, 1);
    let window = XResourceId::new(0x200001, 1);
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .register_window_parent(client, window, root)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .select_xi_events(
            namespace,
            client.raw(),
            root,
            &[(1, vec![(1 << 4) | (1 << 5) | (1 << 6)])],
        );
    broker
        .routed_input_sender()
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 3,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 3,
                target_surface: surface,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::PointerAxis {
                    horizontal_v120: 0,
                    vertical_v120: 120,
                },
            },
            route_lease: None,
            delivery: None,
            mode: XAuthorityRoutedInputMode::Deliver,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    let pressed = channels.input.recv().unwrap();
    assert_eq!(pressed.xi_event_type, Some(6));
    assert_eq!(pressed.xi_event_window, Some(root));
    assert_eq!(pressed.xi_emulated_button_type, Some(4));
    assert_eq!(pressed.xi_emulated_button_window, Some(root));
    assert!(matches!(
        pressed.event,
        XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Axis {
                button: 5,
                pressed: true,
                horizontal_position_v120: None,
                vertical_position_v120: Some(120),
            },
            ..
        })
    ));
    let released = channels.input.recv().unwrap();
    assert_eq!(released.xi_event_type, None);
    assert_eq!(released.xi_event_window, None);
    assert_eq!(released.xi_emulated_button_type, Some(5));
    assert_eq!(released.xi_emulated_button_window, Some(root));
    assert!(matches!(
        released.event,
        XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Axis {
                button: 5,
                pressed: false,
                horizontal_position_v120: None,
                vertical_position_v120: None,
            },
            ..
        })
    ));
}

#[test]
fn routed_axis_resolves_smooth_and_emulated_button_selections_independently() {
    let namespace = NamespaceId::from_raw(12);
    let client = XServerFrontendClientId(5);
    let surface = SurfaceId::new(13, 1);
    let window = XResourceId::new(0x200002, 1);
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .register_window_parent(client, window, root)
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .select_xi_events(
            namespace,
            client.raw(),
            root,
            &[(1, vec![(1 << 4) | (1 << 5)])],
        );
    broker
        .routed_input_sender()
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 4,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 4,
                target_surface: surface,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::PointerAxis {
                    horizontal_v120: 0,
                    vertical_v120: -120,
                },
            },
            route_lease: None,
            delivery: None,
            mode: XAuthorityRoutedInputMode::Deliver,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    let pressed = channels.input.recv().unwrap();
    assert_eq!(pressed.xi_event_type, None);
    assert_eq!(pressed.xi_event_window, None);
    assert_eq!(pressed.xi_emulated_button_type, Some(4));
    assert_eq!(pressed.xi_emulated_button_window, Some(root));
    let released = channels.input.recv().unwrap();
    assert_eq!(released.xi_event_type, None);
    assert_eq!(released.xi_emulated_button_type, Some(5));
    assert_eq!(released.xi_emulated_button_window, Some(root));
}

#[test]
fn synchronous_keyboard_grab_queues_until_allow_events() {
    let namespace = NamespaceId::from_raw(10);
    let client = XServerFrontendClientId(3);
    let surface = SurfaceId::new(11, 1);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, XResourceId::new(0x200001, 1))
        .unwrap();
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_keyboard(
            namespace,
            crate::XActiveInputGrab {
                owner: client.raw(),
                window: XResourceId::new(0x200001, 1),
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 0,
                event_mask: 0,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .unwrap();
    broker
        .routed_input_sender()
        .send(XAuthorityRoutedInput {
            request: RoutedInputRequest {
                serial: 2,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 2,
                target_surface: surface,
                global_position: Point::default(),
                local_position: Point::default(),
                kind: InputEventKind::Key {
                    keycode: 30,
                    pressed: true,
                },
            },
            route_lease: None,
            delivery: None,
            mode: XAuthorityRoutedInputMode::Deliver,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    assert!(matches!(
        channels.input.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .allow_events(namespace, client.raw(), 3)
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    assert_eq!(channels.input.recv().unwrap().client, client);
}

#[test]
fn xi2_device_event_uses_xge_header_and_fp1616_local_coordinates() {
    let motion = XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
        kind: XAuthorityPointerEventKind::Motion,
        surface: SurfaceId::new(1, 1),
        root_x: 11,
        root_y: 12,
        event_x: 3,
        event_y: -4,
        state: 5,
        time_msec: 9,
    });
    let bytes = encode_xi_device_event(
        XByteOrder::LittleEndian,
        7,
        6,
        motion,
        XResourceId::new(0x200001, 1),
        XResourceId::new(0x200002, 1),
        7,
        -8,
        0,
    );
    assert_eq!(bytes.len(), 80);
    assert_eq!(bytes[0], 35);
    assert_eq!(bytes[1], crate::X_INPUT_MAJOR_OPCODE);
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 6);
    assert_eq!(u16::from_le_bytes([bytes[10], bytes[11]]), 2);
    assert_eq!(
        i32::from_le_bytes(bytes[40..44].try_into().unwrap()),
        7 << 16
    );
    assert_eq!(
        i32::from_le_bytes(bytes[44..48].try_into().unwrap()),
        -8 << 16
    );
    assert_eq!(
        u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
        0x200002
    );
    let axis = XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
        kind: XAuthorityPointerEventKind::Axis {
            button: 5,
            pressed: true,
            horizontal_position_v120: None,
            vertical_position_v120: Some(120),
        },
        surface: SurfaceId::new(1, 1),
        root_x: 11,
        root_y: 12,
        event_x: 3,
        event_y: -4,
        state: 5,
        time_msec: 10,
    });
    let scroll = encode_xi_device_event(
        XByteOrder::LittleEndian,
        8,
        6,
        axis,
        XResourceId::new(0x200001, 1),
        XResourceId::NONE,
        3,
        -4,
        0,
    );
    assert_eq!(scroll.len(), 92);
    assert_eq!(u32::from_le_bytes(scroll[4..8].try_into().unwrap()), 15);
    assert_eq!(u32::from_le_bytes(scroll[16..20].try_into().unwrap()), 0);
    assert_eq!(u32::from_le_bytes(scroll[56..60].try_into().unwrap()), 0);
    assert_eq!(u16::from_le_bytes(scroll[50..52].try_into().unwrap()), 1);
    assert_eq!(
        &scroll[80..84],
        &[1 << crate::X_POINTER_VERTICAL_SCROLL_VALUATOR, 0, 0, 0,]
    );
    assert_eq!(
        decode_xi_fp3232(XByteOrder::LittleEndian, &scroll[84..92]),
        (120, 0)
    );
    let two_axis_scroll = encode_xi_device_event(
        XByteOrder::LittleEndian,
        9,
        6,
        XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Axis {
                button: 5,
                pressed: true,
                horizontal_position_v120: Some(-30),
                vertical_position_v120: Some(45),
            },
            surface: SurfaceId::new(1, 1),
            root_x: 11,
            root_y: 12,
            event_x: 3,
            event_y: -4,
            state: 5,
            time_msec: 11,
        }),
        XResourceId::new(0x200001, 1),
        XResourceId::NONE,
        3,
        -4,
        0,
    );
    assert_eq!(two_axis_scroll.len(), 100);
    assert_eq!(
        &two_axis_scroll[80..84],
        &[
            (1 << crate::X_POINTER_HORIZONTAL_SCROLL_VALUATOR)
                | (1 << crate::X_POINTER_VERTICAL_SCROLL_VALUATOR),
            0,
            0,
            0,
        ]
    );
    assert_eq!(
        decode_xi_fp3232(XByteOrder::LittleEndian, &two_axis_scroll[84..92]),
        (-30, 0)
    );
    assert_eq!(
        decode_xi_fp3232(XByteOrder::LittleEndian, &two_axis_scroll[92..100]),
        (45, 0)
    );
    let emulated_button = encode_xi_device_event(
        XByteOrder::LittleEndian,
        10,
        4,
        XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Axis {
                button: 5,
                pressed: true,
                horizontal_position_v120: None,
                vertical_position_v120: Some(120),
            },
            surface: SurfaceId::new(1, 1),
            root_x: 11,
            root_y: 12,
            event_x: 3,
            event_y: -4,
            state: 5,
            time_msec: 12,
        }),
        XResourceId::new(0x200001, 1),
        XResourceId::NONE,
        3,
        -4,
        XI_POINTER_EMULATED,
    );
    assert_eq!(emulated_button.len(), 80);
    assert_eq!(
        u16::from_le_bytes(emulated_button[8..10].try_into().unwrap()),
        4
    );
    assert_eq!(
        u32::from_le_bytes(emulated_button[16..20].try_into().unwrap()),
        5
    );
    assert_eq!(
        u32::from_le_bytes(emulated_button[56..60].try_into().unwrap()),
        XI_POINTER_EMULATED
    );
    assert_eq!(
        u16::from_le_bytes(emulated_button[50..52].try_into().unwrap()),
        0
    );
    let crossing = encode_xi_crossing_event(
        XByteOrder::LittleEndian,
        8,
        7,
        XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
            kind: XAuthorityPointerEventKind::Motion,
            surface: SurfaceId::new(1, 1),
            root_x: 11,
            root_y: 12,
            event_x: 3,
            event_y: -4,
            state: 5,
            time_msec: 9,
        }),
        XResourceId::new(0x200001, 1),
    );
    assert_eq!(crossing.len(), 72);
    assert_eq!(u16::from_le_bytes([crossing[8], crossing[9]]), 7);
    assert_eq!(crossing[48], 1);
}

#[test]
fn keyboard_focus_propagates_only_through_its_ancestor_chain() {
    let mut selections = XCoreEventSelectionState::default();
    let parent = XResourceId::new(0x200007, 1);
    let child = XResourceId::new(0x200001, 1);
    selections.register(
        child,
        parent,
        Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        },
    );
    assert_eq!(selections.selected_keyboard_target(child), None);
    selections.update(parent, Some(1), None);

    assert_eq!(selections.keyboard_target(child), parent);
    assert_eq!(selections.selected_keyboard_target(child), Some(parent));

    assert_eq!(
        selections.keyboard_target(XResourceId::new(0x200009, 1)),
        XResourceId::new(0x200009, 1)
    );
}

#[test]
fn keyboard_delivery_falls_back_to_engine_focused_surface() {
    let selections = XCoreEventSelectionState::default();

    assert_eq!(
        selections.keyboard_target(XResourceId::new(0x200001, 1)),
        XResourceId::new(0x200001, 1)
    );
}

#[test]
fn root_focus_uses_mapped_stacking_order_and_restacking() {
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let lower = XResourceId::new(0x200001, 1);
    let upper = XResourceId::new(0x200002, 1);
    let mut selections = XCoreEventSelectionState::default();
    for window in [lower, upper] {
        selections.register(
            window,
            root,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
        );
        selections.update(window, Some(1), None);
        selections.observe_mapped(window);
    }
    assert_eq!(selections.keyboard_target(root), upper);

    selections.restack(lower, Some(upper), Some(0));
    assert_eq!(selections.keyboard_target(root), lower);

    selections.observe_unmapped(lower);
    assert_eq!(selections.keyboard_target(root), upper);
}

#[test]
fn pending_control_gets_the_next_runtime_lock() {
    let runtime = Arc::new(Mutex::new(XAuthorityRuntime::default()));
    let held = runtime.lock().expect("initial runtime lock");
    let control_runtime_pending = Arc::new(AtomicUsize::new(0));
    let (order_sender, order_receiver) = std::sync::mpsc::channel();

    let control_runtime = runtime.clone();
    let control_pending = control_runtime_pending.clone();
    let control_sender = order_sender.clone();
    let control = std::thread::spawn(move || {
        let _guard = lock_x11_control_runtime(&control_runtime, &control_pending)
            .expect("control runtime lock");
        control_sender.send("control").expect("control order");
    });
    while control_runtime_pending.load(Ordering::Acquire) == 0 {
        std::thread::yield_now();
    }

    let request_runtime = runtime.clone();
    let request_pending = control_runtime_pending.clone();
    let request = std::thread::spawn(move || {
        wait_for_x11_control_runtime(&request_pending);
        let _guard = request_runtime.lock().expect("request runtime lock");
        order_sender.send("request").expect("request order");
    });
    drop(held);

    assert_eq!(order_receiver.recv().expect("first owner"), "control");
    assert_eq!(order_receiver.recv().expect("second owner"), "request");
    control.join().expect("control owner");
    request.join().expect("request owner");
}

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
        let _guard = lock_x11_non_control_output(&normal_stream, &normal_pending)
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
    let (gate, _instance, _issuer) = control_gate();
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
    let (gate, _instance, _issuer) = control_gate();
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
    let (gate, _instance, _issuer) = control_gate();
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
    let (gate, _instance, _issuer) = control_gate();
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
    let (gate, _instance, _issuer) = control_gate();
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
    let (gate, mut instance, issuer, _submit) = control_gate_with_submit();

    // The coordinator exists before the broker does, so there is no interval
    // in which a handle could be taken from an ungated instance.
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate: gate.clone(),
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    // No client or surface registered: enqueue is an admission decision, and
    // admission does not depend on there being somewhere to route to yet.
    //
    // Actually send, rather than asking the gate a question the sender was
    // never involved in. Open: admitted.
    let sender = private.ingress();
    sender
        .submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(95)))
        .expect("an open coordinator to admit work");

    // Close THIS gate. If the sender were stamping from anything else, it
    // would carry on admitting.
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

    assert!(
        sender
            .submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(96)))
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
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
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
            .submit(motion_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
            ))
            .expect("an open coordinator to admit work");
    }

    let ran = private.route_pending().expect("the ordered pass to run");
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
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));

    // Raw ingress is not one of the sources the ordered pass reads, and a
    // private instance will not hand out a handle to it either.
    assert_eq!(
        private.broker.input_sender().err(),
        Some(crate::ActivationRefused::RawIngressRefusedUnderGate)
    );
    assert_eq!(
        private.route_pending().expect("an empty ordered pass").len(),
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
    let (gate, mut instance, issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate: gate.clone(),
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let delivery = XAuthorityInputDeliveryId::from_raw(110);
    private
        .ingress()
        .submit(motion_to(surface, delivery))
        .expect("an open coordinator to admit work");

    // The revision it was stamped under closes before the ordered pass runs.
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

    private.route_pending().expect("the ordered pass to run");

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
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    // Ingress capacity larger than the ready stream's ordinary share, so more
    // can be sent than one pass can admit.
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(16).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
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
            .submit(motion_to(
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
        delivered += private.route_pending().expect("an ordered pass").len();
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
    let (gate, mut instance, issuer, _submit) = control_gate_with_submit();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(2).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate: gate.clone(),
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    // Open: accepted.
    private
        .submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(300)))
        .expect("an open coordinator to accept work");

    // Fill the bounded ingress. These are saturation, not policy.
    let mut saturated = false;
    for delivery in 301..320u64 {
        match private.submit(motion_to(
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

    match private.submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(330))) {
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
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    // The production constructor at its smallest: ready capacity six, of
    // which four are held for cleanup, so ordinary work has room for two.
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
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
        .submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(9001)))
        .expect("an open coordinator to accept work");
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
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
        ran += private.route_pending().expect("an ordered pass").len();
    }
    assert_eq!(ran, 2, "both accepted operations ran across the passes");
    assert!(channels.input.try_recv().is_ok(), "the input was delivered");
    assert!(
        channels.control.try_recv().is_ok(),
        "the control reached its client rather than being destroyed"
    );
}

#[test]
fn two_producer_classes_share_one_order() {
    let namespace = NamespaceId::from_raw(51);
    let client = XServerFrontendClientId(68);
    let surface = SurfaceId::new(55, 1);
    let window = XResourceId::new(0x200190, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(16);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(16).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let input = private.ingress();
    let control = private.control_producer();

    // Alternating, from two genuinely different producer facades. Consumer
    // staging would have grouped these by source no matter how they arrived.
    let mut expected = Vec::new();
    for step in 0..4u64 {
        let at = input
            .submit(motion_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(400 + step),
            ))
            .expect("an open coordinator to accept input");
        expected.push(at.raw());
        let at = control
            .submit(XAuthorityClientControlCommand {
                client,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(step + 1),
                    surface,
                },
            })
            .expect("the shared admission to accept control");
        expected.push(at.raw());
    }

    // Positions rise in acceptance order across both producers, not grouped.
    assert!(
        expected.windows(2).all(|pair| pair[0] < pair[1]),
        "positions must rise in acceptance order across producers: {expected:?}"
    );
    let ran = private.route_pending().expect("the shared order to run");
    assert_eq!(ran.len(), 8);

    // What the CONSUMER took, not what the producers were told. A consumer
    // that grouped entries someone else had already numbered would satisfy the
    // assertion above and fail this one.
    let taken: Vec<_> = ran.iter().map(|run| run.class).collect();
    assert_eq!(
        taken,
        vec![
            crate::ReadyClass::RoutedInput,
            crate::ReadyClass::Control,
            crate::ReadyClass::RoutedInput,
            crate::ReadyClass::Control,
            crate::ReadyClass::RoutedInput,
            crate::ReadyClass::Control,
            crate::ReadyClass::RoutedInput,
            crate::ReadyClass::Control,
        ],
        "the consumer must see the alternation the producers created"
    );
    let positions: Vec<_> = ran.iter().map(|run| run.sequence.raw()).collect();
    assert_eq!(positions, expected, "and at their own positions");

    // And carrying the identities that were actually submitted. A right-looking
    // class record can otherwise accompany the wrong payload entirely.
    let identities: Vec<_> = ran
        .iter()
        .map(|run| match run.identity {
            crate::PrivateIdentity::Delivery(delivery) => (delivery, None),
            crate::PrivateIdentity::Control {
                transaction,
                completion,
            } => {
                assert!(
                    completion.is_some(),
                    "an admitted control carries the registration that answers for it"
                );
                (None, Some(transaction))
            }
            crate::PrivateIdentity::Lease(_) => (None, None),
        })
        .collect();
    assert_eq!(
        identities,
        vec![
            (Some(XAuthorityInputDeliveryId::from_raw(400)), None),
            (None, Some(TransactionId::from_raw(1))),
            (Some(XAuthorityInputDeliveryId::from_raw(401)), None),
            (None, Some(TransactionId::from_raw(2))),
            (Some(XAuthorityInputDeliveryId::from_raw(402)), None),
            (None, Some(TransactionId::from_raw(3))),
            (Some(XAuthorityInputDeliveryId::from_raw(403)), None),
            (None, Some(TransactionId::from_raw(4))),
        ],
        "each run must name the operation its producer submitted"
    );
}

#[test]
fn a_send_that_returned_is_never_overtaken_by_one_that_started_later() {
    let namespace = NamespaceId::from_raw(52);
    let client = XServerFrontendClientId(69);
    let surface = SurfaceId::new(56, 1);
    let window = XResourceId::new(0x2001a0, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(16);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(16).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let first = private.ingress();
    let second = private.control_producer();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

    // A's send completes before B's begins, enforced rather than hoped for.
    let a_at = first
        .submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(500)))
        .expect("an open coordinator to accept input");
    let gate_for_b = std::sync::Arc::clone(&barrier);
    let b = std::thread::spawn(move || {
        gate_for_b.wait();
        second.submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(9),
                surface,
            },
        })
    });
    barrier.wait();
    let b_at = b.join().expect("the second producer").expect("accepted");

    assert!(
        a_at.raw() < b_at.raw(),
        "a completed send cannot be overtaken by one that started afterwards"
    );

    // And the consumer sees that precedence, not just the numbers.
    let mut private = private;
    let ran = private.route_pending().expect("the shared order to run");
    let taken: Vec<_> = ran.iter().map(|run| run.sequence.raw()).collect();
    assert_eq!(taken, vec![a_at.raw(), b_at.raw()]);
}

#[test]
fn a_refused_control_comes_back_to_its_producer() {
    let client = XServerFrontendClientId(70);
    let surface = SurfaceId::new(57, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    // Ordinary share of two at the smallest production size.
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let control = private.control_producer();
    let command = |transaction| XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(transaction),
            surface,
        },
    };

    control.submit(command(1)).expect("room for the first");
    control.submit(command(2)).expect("room for the second");

    // Nothing has drained, so the third has nowhere to go. Controls have no
    // recovery ticket capping them, so this is reachable by ordinary use.
    let (refusal, returned) = control
        .submit(command(3))
        .expect_err("the ordinary share is full");
    assert_eq!(refusal, crate::AdmissionRefusal::Saturated);
    assert_eq!(
        returned, command(3),
        "a refused control is handed back, not destroyed"
    );
}

#[test]
fn producers_are_refused_once_their_consumer_is_gone() {
    let client = XServerFrontendClientId(71);
    let surface = SurfaceId::new(58, 1);
    let window = XResourceId::new(0x2001b0, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, NamespaceId::from_raw(53), surface, window)
        .unwrap();

    // Producers outlive the frontend, which is ordinary: they are handles.
    let input = private.ingress();
    let control = private.control_producer();
    drop(private);

    // Accepting now would tell a producer its work is queued when nothing can
    // ever run it.
    match input.submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(600))) {
        Err(crate::PrivateSendError::Disconnected(_)) => {}
        other => panic!("a gone consumer is a disconnection, not {other:?}"),
    }
    let (refusal, _returned) = control
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(1),
                surface,
            },
        })
        .expect_err("a gone consumer refuses control too");
    assert_eq!(refusal, crate::AdmissionRefusal::ConsumerGone);
}

#[test]
fn an_unreachable_queue_is_not_reported_as_a_finished_one() {
    let namespace = NamespaceId::from_raw(54);
    let client = XServerFrontendClientId(72);
    let surface = SurfaceId::new(59, 1);
    let window = XResourceId::new(0x2001c0, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    // Accepted, and owed a run.
    private
        .ingress()
        .submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(8201)))
        .expect("an open coordinator to accept work");

    // The queue becomes unreachable while that work is still in it.
    let admission = std::sync::Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();

    // Reporting an empty run here would say the pass finished while accepted
    // work sat in a queue nobody can open.
    assert!(
        private.route_pending().is_err(),
        "an unreachable queue is not a drained one"
    );
    assert!(
        channels.input.try_recv().is_err(),
        "and nothing was delivered from it"
    );
}

#[test]
fn accepted_work_is_answered_when_its_consumer_goes_away() {
    let namespace = NamespaceId::from_raw(55);
    let client = XServerFrontendClientId(73);
    let surface = SurfaceId::new(60, 1);
    let window = XResourceId::new(0x2001d0, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let delivery = XAuthorityInputDeliveryId::from_raw(8300);
    private
        .ingress()
        .submit(motion_to(surface, delivery))
        .expect("an open coordinator to accept work");

    // The consumer goes away with that work still accepted and never run.
    drop(private);

    // It was promised a consumer, so it is owed an answer. Naming it in a
    // local and letting that local drop would be the same loss as dropping it
    // unnamed.
    // Bounded, so a failure to settle fails this test rather than hanging it.
    assert_eq!(
        delivery_receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("accepted work must be settled when its consumer disappears"),
        XAuthorityClientInputDelivery {
            client,
            delivery,
            // Not TargetGone: the client and its surface are still
            // registered, and it is the authority that stopped. A receipt that
            // is merely terminal is weaker than one that is true.
            outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
        }
    );
}

#[test]
fn one_turn_of_service_is_bounded_while_a_producer_keeps_refilling() {
    const PRIVATE_CLEANUP_RESERVE_FOR_TESTS: usize = 4;
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4096);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    // The constructor's own sizing at an ingress capacity of one.
    let budget = 2 + PRIVATE_CLEANUP_RESERVE_FOR_TESTS;

    // A producer that keeps putting work back as fast as the turn takes it.
    // Draining until empty would never end here, and the report would grow
    // without limit.
    let control = private.control_producer();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_for_producer = std::sync::Arc::clone(&stop);
    let refiller = std::thread::spawn(move || {
        let mut transaction = 1u64;
        while !stop_for_producer.load(std::sync::atomic::Ordering::Acquire) {
            let _ = control.submit(XAuthorityClientControlCommand {
                client: XServerFrontendClientId(999),
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(transaction),
                    surface: SurfaceId::new(61, 1),
                },
            });
            transaction = transaction.wrapping_add(1);
        }
    });

    let outcome = private.route_pending();
    stop.store(true, std::sync::atomic::Ordering::Release);
    refiller.join().expect("the refilling producer");

    // Stopping on a routing failure is bounded too; the unbounded case is a
    // turn that runs for as long as a producer keeps feeding it.
    if let Ok(ran) = outcome {
        assert!(
            ran.len() <= budget,
            "a turn must not exceed its budget while work keeps arriving: ran {}",
            ran.len()
        );
    }
}

#[test]
fn accepted_control_is_acknowledged_when_its_consumer_goes_away() {
    let namespace = NamespaceId::from_raw(57);
    let client = XServerFrontendClientId(75);
    let surface = SurfaceId::new(62, 1);
    let window = XResourceId::new(0x2001f0, 1);
    let (control_ack_sender, control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(4242),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    drop(private);

    // Control has its own acknowledgement contract, so it gets that rather
    // than nothing and rather than a fabricated input receipt.
    let ack = control_ack_receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("accepted control must be acknowledged when its consumer goes");
    assert_eq!(ack.client, client);
    assert_eq!(ack.acknowledgement.transaction, TransactionId::from_raw(4242));
    assert_eq!(
        ack.acknowledgement.outcome,
        XAuthorityControlOutcome::AuthorityRejected,
        "the authority stopped; the client did not go anywhere"
    );
}

#[test]
fn every_control_run_names_its_own_transaction() {
    let client = XServerFrontendClientId(76);
    let surface = SurfaceId::new(63, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(16);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, NamespaceId::from_raw(58), surface, XResourceId::new(0x200200, 1))
        .unwrap();

    // Not FocusSurface. Every control command carries a transaction, so
    // recognising one variant and calling the rest untracked lost the identity
    // of all the others.
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::ClearFocus {
                transaction: TransactionId::from_raw(5150),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    let ran = private.route_pending().expect("a turn");
    assert!(
        matches!(
            ran.first().map(|run| run.identity),
            Some(crate::PrivateIdentity::Control {
                transaction,
                completion: Some(_),
            }) if transaction == TransactionId::from_raw(5150)
        ),
        "a control that is not FocusSurface still names its transaction"
    );
}

#[test]
fn a_full_acknowledgement_channel_retains_the_obligation() {
    let namespace = NamespaceId::from_raw(59);
    let client = XServerFrontendClientId(77);
    let surface = SurfaceId::new(64, 1);
    let window = XResourceId::new(0x200210, 1);
    // One slot, filled before shutdown, so the terminal ack cannot be sent.
    let (control_ack_sender, control_ack_receiver) = sync_channel(1);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender.clone(),
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(777),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    // Prefill the only slot.
    control_ack_sender
        .try_send(XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::FocusSurface,
                transaction: TransactionId::from_raw(1),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        })
        .expect("the empty slot");

    // The obligation cannot be discharged, so it is handed back rather than
    // counted and dropped. A caller that is still alive can do something with
    // it; a count in a log cannot.
    let report = private.shutdown();
    assert!(!report.is_settled());
    assert_eq!(report.owed(), 1, "the control is still owed");
    assert!(
        matches!(
            report.pending.first(),
            Some(PrivateOperation::Control(_, _))
        ),
        "and it is the control itself, not a note about it"
    );
    let _ = control_ack_receiver;
}

#[test]
fn an_unresolved_target_is_handed_back_rather_than_attributed() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));

    // No surface registered, so nothing resolves this target.
    private
        .ingress()
        .submit(motion_to(
            SurfaceId::new(65, 1),
            XAuthorityInputDeliveryId::from_raw(8400),
        ))
        .expect("an open coordinator to accept work");

    let report = private.shutdown();
    assert!(!report.is_settled());
    assert_eq!(
        report.owed(),
        1,
        "a receipt nobody can attribute is not a settlement"
    );
}

#[test]
fn a_retained_handle_settles_once_the_channel_drains() {
    let namespace = NamespaceId::from_raw(60);
    let client = XServerFrontendClientId(78);
    let surface = SurfaceId::new(66, 1);
    let window = XResourceId::new(0x200220, 1);
    let (control_ack_sender, control_ack_receiver) = sync_channel(1);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender.clone(),
            input_deliveries: delivery_sender,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(910),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    // The only slot is taken by something else.
    control_ack_sender
        .try_send(XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::FocusSurface,
                transaction: TransactionId::from_raw(1),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        })
        .expect("the empty slot");

    let mut settlement = private.shutdown();
    assert_eq!(settlement.owed(), 1, "the channel was full");

    // Drain the original, then retry through the HANDLE. It keeps the registry
    // that accepted the work, so nothing else has to be supplied.
    let first = control_ack_receiver.recv().expect("the prefilled ack");
    assert_eq!(first.acknowledgement.transaction, TransactionId::from_raw(1));

    assert_eq!(settlement.retry(), 1, "the obligation is discharged now");
    assert!(settlement.is_settled());

    let owed = control_ack_receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the retained acknowledgement");
    assert_eq!(owed.acknowledgement.transaction, TransactionId::from_raw(910));
    assert_eq!(
        owed.acknowledgement.outcome,
        XAuthorityControlOutcome::AuthorityRejected
    );

    // Retrying again answers nothing a second time.
    assert_eq!(settlement.retry(), 0);
    assert!(
        control_ack_receiver
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err(),
        "a settled obligation is not answered twice"
    );
}

#[test]
fn two_frontends_with_colliding_client_ids_never_cross_receivers() {
    let namespace = NamespaceId::from_raw(61);
    let client = XServerFrontendClientId(79);
    let surface = SurfaceId::new(67, 1);
    let window = XResourceId::new(0x200230, 1);
    let (first_ack, first_ack_receiver) = sync_channel(8);
    let (second_ack, second_ack_receiver) = sync_channel(8);
    let (first_delivery, _first_delivery_receiver) = channel();
    let (second_delivery, _second_delivery_receiver) = channel();
    let (first_gate, _i1, _s1, _u1) = control_gate_with_submit();
    let (second_gate, _i2, _s2, _u2) = control_gate_with_submit();

    let build = |ack, delivery, gate| {
        let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: ack,
            input_deliveries: delivery,
            gate,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
        let (registration, channels) = private.broker.registry.register_client(client).unwrap();
        private
            .broker
            .registry
            .register_surface(client, namespace, surface, window)
            .unwrap();
        (private, registration, channels)
    };
    let (first, _r1, _c1) = build(first_ack, first_delivery, first_gate);
    let (second, _r2, _c2) = build(second_ack, second_delivery, second_gate);

    // The same client id in both, which is ordinary: ids are unique per
    // frontend, not across frontends.
    first
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(1001),
                surface,
            },
        })
        .expect("the first frontend to accept");
    second
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(2002),
                surface,
            },
        })
        .expect("the second frontend to accept");

    let first_settlement = first.shutdown();
    let second_settlement = second.shutdown();
    assert!(first_settlement.is_settled());
    assert!(second_settlement.is_settled());

    // Each frontend's obligation went to its own receiver, and only its own.
    assert_eq!(
        first_ack_receiver.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1001)
    );
    assert!(first_ack_receiver.try_recv().is_err());
    assert_eq!(
        second_ack_receiver.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(2002)
    );
    assert!(second_ack_receiver.try_recv().is_err());
}

#[test]
fn an_abandoned_handle_leaves_its_work_with_a_durable_owner() {
    let namespace = NamespaceId::from_raw(62);
    let client = XServerFrontendClientId(80);
    let surface = SurfaceId::new(68, 1);
    let window = XResourceId::new(0x200240, 1);
    let (control_ack_sender, control_ack_receiver) = sync_channel(1);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let durable = crate::PrivateSettlementOwner::default();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender.clone(),
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(1234),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    // The only slot is taken, and the receiver is alive. Congestion, not
    // teardown.
    control_ack_sender
        .try_send(XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::FocusSurface,
                transaction: TransactionId::from_raw(1),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        })
        .expect("the empty slot");

    // The handle is abandoned while still full, which is the case that used to
    // destroy what it held.
    drop(private.shutdown());
    assert_eq!(
        durable.owed(),
        1,
        "an abandoned obligation outlives the handle that held it"
    );

    // Capacity frees afterwards, and the durable owner discharges it against
    // the registry that accepted it.
    let first = control_ack_receiver.recv().expect("the prefilled ack");
    assert_eq!(first.acknowledgement.transaction, TransactionId::from_raw(1));

    assert_eq!(durable.drive().answered, 1, "the durable owner answers it");
    assert_eq!(durable.owed(), 0);

    let owed = control_ack_receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the obligation the abandoned handle left behind");
    assert_eq!(
        owed.acknowledgement.transaction,
        TransactionId::from_raw(1234)
    );
    assert_eq!(
        owed.acknowledgement.outcome,
        XAuthorityControlOutcome::AuthorityRejected
    );

    // Driving again answers nothing twice, and no other instance was involved.
    assert_eq!(durable.drive().answered, 0);
    assert!(
        control_ack_receiver
            .recv_timeout(std::time::Duration::from_millis(200))
            .is_err()
    );
    assert_eq!(durable.reserved(), 0, "the answered credit is free again");
}

#[test]
fn an_unreadable_queue_is_owned_by_something_that_outlives_it() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let durable = crate::PrivateSettlementOwner::default();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));

    let admission = std::sync::Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();

    drop(private.shutdown());

    // The fact outlives the handle rather than going away as a boolean on
    // something that has gone.
    assert_eq!(durable.failed_instances(), 1);
}

/// One private instance that accepts a control and immediately shuts down,
/// leaving the acknowledgement owed. Ported from the independent review.
fn review_settlement_queue(
    sender: SyncSender<XAuthorityClientControlAck>,
    durable: &crate::PrivateSettlementOwner,
    transaction: u64,
) -> (
    crate::PrivateSettlement,
    XServerFrontendClientRouteRegistration,
    XServerFrontendClientRouteChannels,
) {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _authority, _issuer) = control_gate();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            gate,
        },
        durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(251),
            surface,
            XResourceId::new(0x200251, 1),
        )
        .unwrap();
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::ConfigureSurface {
                transaction: TransactionId::from_raw(transaction),
                surface,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 60,
                },
            },
        })
        .unwrap();
    (private.shutdown(), registration, channels)
}

fn review_settlement_expected(transaction: u64) -> XAuthorityClientControlAck {
    XAuthorityClientControlAck {
        client: XServerFrontendClientId(251),
        acknowledgement: XAuthorityControlAck {
            kind: XAuthorityControlKind::ConfigureSurface,
            transaction: TransactionId::from_raw(transaction),
            surface: SurfaceId::new(251, 1),
            outcome: XAuthorityControlOutcome::AuthorityRejected,
        },
    }
}

#[test]
fn review_settlement_two_pending_origins_reverse_retry_cannot_cross() {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender_a, receiver_a) = sync_channel(1);
    let (sender_b, receiver_b) = sync_channel(1);
    let (prefill_a, _r0a, _c0a) = review_settlement_queue(sender_a.clone(), &durable, 9500);
    let (prefill_b, _r0b, _c0b) = review_settlement_queue(sender_b.clone(), &durable, 9600);
    assert!(prefill_a.is_settled() && prefill_b.is_settled());
    let (mut a, _ra, _ca) = review_settlement_queue(sender_a.clone(), &durable, 9501);
    let (mut b, _rb, _cb) = review_settlement_queue(sender_b.clone(), &durable, 9601);
    assert_eq!((a.owed(), b.owed()), (1, 1));

    // Both retained, with identical numeric client and surface identities.
    // Free and retry B first; A's queue stays occupied throughout.
    assert_eq!(
        receiver_b
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9600)
    );
    assert_eq!(b.retry(), 1);
    assert_eq!(a.retry(), 0);
    assert_eq!(a.owed(), 1);
    assert_eq!(
        receiver_b
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9601)
    );
    assert_eq!(
        receiver_a
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9500)
    );
    assert_eq!(a.retry(), 1);
    assert_eq!(
        receiver_a
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9501)
    );
    assert!(a.is_settled() && b.is_settled());
    assert_eq!((a.retry(), b.retry()), (0, 0));
    assert!(receiver_a.try_recv().is_err());
    assert!(receiver_b.try_recv().is_err());
}

#[test]
fn review_settlement_dropped_pending_handle_preserves_accepted_outcome() {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, receiver) = sync_channel(1);
    let (prefill, _r0, _c0) = review_settlement_queue(sender.clone(), &durable, 9700);
    assert!(prefill.is_settled());
    let (pending, _r1, _c1) = review_settlement_queue(sender.clone(), &durable, 9701);
    assert_eq!(pending.owed(), 1);
    assert!(!pending.is_settled());

    // The only unsettled handle is abandoned while the channel is still full.
    drop(pending);

    // The original is intact.
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9700)
    );

    // Adapted as instructed: responsibility for the accepted work did not end
    // with the handle, so the durable owner still holds it and can discharge
    // it now that there is room.
    assert_eq!(durable.owed(), 1);
    assert_eq!(durable.drive().answered, 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("dropping the only unsettled handle must preserve responsibility for 9701"),
        review_settlement_expected(9701)
    );
    assert_eq!(durable.drive().answered, 0);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn settlement_storage_is_reserved_before_work_is_accepted() {
    // One credit for the whole owner, shared across every instance below.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, receiver) = sync_channel(1);

    // The first settles straight away, filling the acknowledgement channel and
    // freeing its credit.
    let (filled, _r0, _c0) = review_settlement_queue(sender.clone(), &durable, 9800);
    assert!(filled.is_settled());
    assert_eq!(durable.reserved(), 0);

    // The second cannot settle, because the channel is now full, so it keeps
    // the only credit.
    let (owed, _r1, _c1) = review_settlement_queue(sender.clone(), &durable, 9801);
    assert_eq!(owed.owed(), 1);
    assert_eq!(durable.reserved(), 1);

    // A third cannot even be accepted: the storage that would have to hold its
    // work if abandoned is spoken for. Refusing here costs a producer only
    // work it was never told had been taken.
    let (gate, _authority, _issuer) = control_gate();
    let (delivery_sender, _delivery_receiver) = channel();
    let third = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    assert!(
        third
            .control_producer()
            .submit(XAuthorityClientControlCommand {
                client: XServerFrontendClientId(251),
                command: XAuthorityControlCommand::ConfigureSurface {
                    transaction: TransactionId::from_raw(9802),
                    surface: SurfaceId::new(251, 1),
                    geometry: Rect {
                        x: 0,
                        y: 0,
                        width: 80,
                        height: 60,
                    },
                },
            })
            .is_err(),
        "work must not be accepted without storage to answer it"
    );

    // The second is still answerable: nothing was destroyed to make room.
    drop(owed);
    assert_eq!(durable.owed(), 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9800)
    );
    assert_eq!(durable.drive().answered, 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("the accepted work survives"),
        review_settlement_expected(9801)
    );
    assert_eq!(durable.reserved(), 0, "the answered credit is free again");
}

#[test]
fn a_failed_instance_hands_over_its_queue_not_a_tally() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let durable = crate::PrivateSettlementOwner::default();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));

    let admission = std::sync::Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();

    drop(private.shutdown());

    // The queue and its registry are retained under the owner, so the failure
    // belongs to something that can be asked about it later rather than to a
    // number that cannot.
    assert_eq!(durable.failed_instances(), 1);
}

#[test]
fn review_owner_saturation_cannot_discard_two_already_accepted_controls() {
    // The independent negative, repaired as instructed: with reservation
    // before acceptance, either a submission is refused with its payload
    // before being accepted, or everything accepted settles exactly once.
    // Counting a loss is not an allowed outcome.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, receiver) = sync_channel(1);
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let (gate, _authority, _issuer) = control_gate();
    let (delivery_sender, _delivery_receiver) = channel();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(2).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(251),
            surface,
            XResourceId::new(0x200251, 1),
        )
        .unwrap();

    let submit = |transaction: u64| {
        private
            .control_producer()
            .submit(XAuthorityClientControlCommand {
                client,
                command: XAuthorityControlCommand::ConfigureSurface {
                    transaction: TransactionId::from_raw(transaction),
                    surface,
                    geometry: Rect {
                        x: 0,
                        y: 0,
                        width: 80,
                        height: 60,
                    },
                },
            })
    };

    // Fill the acknowledgement channel with something real first.
    sender
        .try_send(review_settlement_expected(9900))
        .expect("the empty slot");

    let mut accepted = Vec::new();
    for transaction in [9901u64, 9902] {
        match submit(transaction) {
            Ok(_) => accepted.push(transaction),
            // Refused BEFORE acceptance, carrying its own command back. The
            // producer keeps work it was never told had been taken.
            Err((crate::AdmissionRefusal::Saturated, returned)) => {
                assert_eq!(returned.command.transaction(), TransactionId::from_raw(transaction));
            }
            Err(other) => panic!("unexpected refusal: {other:?}"),
        }
    }
    assert!(!accepted.is_empty(), "at least one was accepted");

    drop(private.shutdown());
    let owed_before = durable.owed();

    // Drain the prefill, then drive.
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap(),
        review_settlement_expected(9900)
    );
    let mut settled = Vec::new();
    for _ in 0..4 {
        let _ = durable.drive();
        while let Ok(ack) = receiver.recv_timeout(std::time::Duration::from_millis(100)) {
            settled.push(ack.acknowledgement.transaction.raw());
        }
    }

    // Everything accepted was answered, exactly once each. Nothing was
    // counted off as lost to make room.
    let mut expected: Vec<_> = accepted.clone();
    expected.sort_unstable();
    settled.sort_unstable();
    assert_eq!(
        settled, expected,
        "every accepted control is answered exactly once"
    );
    assert_eq!(durable.owed(), 0);
    let _ = owed_before;
}

#[test]
fn a_failed_instances_queue_can_still_be_answered() {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, receiver) = sync_channel(4);
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let (gate, _authority, _issuer) = control_gate();
    let (delivery_sender, _delivery_receiver) = channel();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(251),
            surface,
            XResourceId::new(0x200251, 1),
        )
        .unwrap();
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::ConfigureSurface {
                transaction: TransactionId::from_raw(9950),
                surface,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 60,
                },
            },
        })
        .expect("the shared admission to accept control");

    // The queue becomes unreadable with that work still in it.
    let admission = std::sync::Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();

    drop(private.shutdown());
    assert_eq!(durable.failed_instances(), 1);

    // Retaining the queue was for this. A poisoned lock stays poisoned, but
    // the obligations behind it are intact and still owed, so they can be
    // answered against the registry that accepted them. A tally could have
    // been counted and never discharged.
    assert_eq!(durable.recover_failed(), 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("the work the failed instance had accepted"),
        review_settlement_expected(9950)
    );
    assert_eq!(durable.failed_instances(), 0);
    assert_eq!(durable.reserved(), 0, "its credit is free again");
}

#[test]
fn a_failure_slot_is_reserved_before_an_instance_is_exposed() {
    // Room for one failed instance across the whole owner.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _authority, _issuer) = control_gate();

    let first = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("the only failure slot: {refusal:?}"));

    // A second cannot be built: if it failed, there would be nowhere to hand
    // its queue. Refusing construction costs a caller an instance it never
    // had; refusing the transfer afterwards would drop responsibility for one
    // that existed and accepted work.
    let (second_delivery, _second_delivery_receiver) = channel();
    let (second_gate, _a2, _i2) = control_gate();
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: second_delivery,
            gate: second_gate,
        },
        &durable,
    )
        .is_err(),
        "an instance without a failure slot must not be exposed"
    );

    // The first closes without failing, so its slot returns and another can
    // be built.
    drop(first);
    let (third_delivery, _third_delivery_receiver) = channel();
    let (third_gate, _a3, _i3) = control_gate();
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: third_delivery,
            gate: third_gate,
        },
        &durable,
    )
        .is_ok(),
        "a slot returns when its instance closes without failing"
    );
}

#[test]
fn review_failed_empty_first_instance_cannot_evict_later_accepted_work() {
    // The independent negative, adapted as instructed: with reservation before
    // exposure, B is refused construction rather than being exposed, accepting
    // work, and then having nowhere to hand its queue.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _authority, _issuer) = control_gate();

    // A is built, accepts nothing, and fails. Its slot is spent on a failure
    // that carries no credit, which is why failure slots are counted apart
    // from credits.
    let empty = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("the only failure slot: {refusal:?}"));
    let admission = std::sync::Arc::clone(&empty.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning A");
    })
    .join();
    drop(empty.shutdown());
    assert_eq!(durable.failed_instances(), 1);
    assert_eq!(durable.reserved(), 0, "A accepted nothing");

    // B is refused before exposure, so it never accepts work that would be
    // evicted. This is the whole difference: a refusal here costs a caller an
    // instance it never had.
    let (b_delivery, _b_delivery_receiver) = channel();
    let (b_gate, _ba, _bi) = control_gate();
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: b_delivery,
            gate: b_gate,
        },
        &durable,
    )
        .is_err(),
        "B must not be exposed without room to hand over its queue"
    );

    // Resolving A returns the slot, and B can then be built.
    assert_eq!(durable.recover_failed(), 0, "A had accepted nothing");
    assert_eq!(durable.failed_instances(), 0);
    let (c_delivery, _c_delivery_receiver) = channel();
    let (c_gate, _ca, _ci) = control_gate();
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: c_delivery,
            gate: c_gate,
        },
        &durable,
    )
        .is_ok(),
        "a resolved failure returns its slot"
    );
}

#[test]
fn review_credit_control_writer_pending_retains_credit_and_refuses_next() {
    // The independent negative: a control handed to a client writer is not an
    // acknowledgement, so its credit must not be freed when the consumer takes
    // it.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, _receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _authority, _issuer) = control_gate();
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(2).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(251),
            surface,
            XResourceId::new(0x200251, 1),
        )
        .unwrap();

    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::ConfigureSurface {
                transaction: TransactionId::from_raw(10201),
                surface,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 60,
                },
            },
        })
        .expect("the shared admission to accept control");
    assert_eq!(durable.reserved(), 1);

    // The consumer routes it to the client's writer queue. Nothing has
    // acknowledged it.
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    assert_eq!(
        durable.reserved(),
        1,
        "enqueueing to a writer is not an acknowledgement"
    );
    assert_eq!(
        private.reclaim_settled(),
        0,
        "nothing has reached a terminal outcome"
    );

    // And the capacity is genuinely still held: the next admission is refused.
    let refused = private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::ConfigureSurface {
                transaction: TransactionId::from_raw(10202),
                surface,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 80,
                    height: 60,
                },
            },
        });
    assert!(
        refused.is_err(),
        "capacity held by unanswered work must not admit more"
    );

    // The command really did reach the client's queue.
    assert!(channels.control.try_recv().is_ok());
}

#[test]
fn review_terminal_recorded_then_observed_reclaims_exactly_once() {
    let namespace = NamespaceId::from_raw(63);
    let client = XServerFrontendClientId(81);
    let surface = SurfaceId::new(69, 1);
    let window = XResourceId::new(0x200250, 1);
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();

    let delivery = XAuthorityInputDeliveryId::from_raw(10300);
    private
        .ingress()
        .submit(motion_to(surface, delivery))
        .expect("an open coordinator to accept work");
    assert_eq!(durable.reserved(), 1);

    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    // Routed to the client, not yet delivered: the ledger still holds a
    // ticket for it, so the credit stays with the work.
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(durable.reserved(), 1);

    // The delivery reaches a terminal outcome and that outcome is observed,
    // which is when the ledger stops tracking it. Both halves matter: a
    // recorded terminal nobody has seen is not yet an answer.
    private
        .broker
        .registry
        .send_input_delivery(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .expect("the terminal outcome to be recorded");
    assert_eq!(
        private.reclaim_settled(),
        0,
        "recorded but unobserved is not yet answered"
    );
    assert!(
        private
            .broker
            .registry
            .input_recovery
            .observe(XAuthorityClientInputDelivery {
                client,
                delivery,
                outcome: XAuthorityInputDeliveryOutcome::Flushed,
            }),
        "the outcome is observed"
    );

    assert_eq!(private.reclaim_settled(), 1, "answered, so reclaimed");
    assert_eq!(durable.reserved(), 0);
}

#[test]
fn review_terminal_unreadable_recovery_cannot_prove_live_delivery_settled() {
    let namespace = NamespaceId::from_raw(64);
    let client = XServerFrontendClientId(82);
    let surface = SurfaceId::new(70, 1);
    let window = XResourceId::new(0x200260, 1);
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    private
        .ingress()
        .submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(10400)))
        .expect("an open coordinator to accept work");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    assert_eq!(durable.reserved(), 1);

    // The ledger becomes unreadable while that delivery is still live.
    let recovery = private.broker.registry.input_recovery.clone();
    let _ = std::thread::spawn(move || {
        let _guard = recovery.state.lock().expect("the ledger");
        panic!("poisoning the recovery ledger");
    })
    .join();

    // Silence is not completion. Treating an unreadable ledger as every
    // delivery having ended is the most dangerous reading available, because
    // it frees whatever they were holding.
    assert_eq!(
        private.reclaim_settled(),
        0,
        "an unreadable ledger must not free a live credit"
    );
    assert_eq!(durable.reserved(), 1);
}

#[test]
fn independent_terminal_kept_shutdown_handle_reclaims_late_completion_once() {
    let namespace = NamespaceId::from_raw(65);
    let client = XServerFrontendClientId(83);
    let surface = SurfaceId::new(71, 1);
    let window = XResourceId::new(0x200270, 1);
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    let delivery = XAuthorityInputDeliveryId::from_raw(10500);
    private
        .ingress()
        .submit(motion_to(surface, delivery))
        .expect("an open coordinator to accept work");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);

    // The instance goes away with that work routed and unanswered.
    let mut settlement = private.shutdown();
    assert_eq!(
        settlement.outstanding(),
        1,
        "routed work is carried, not destroyed with the instance"
    );
    assert!(!settlement.is_settled());
    assert_eq!(durable.reserved(), 1);

    // It can still finish afterwards, and the handle notices.
    settlement
        .origin
        .send_input_delivery(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .expect("the terminal outcome to be recorded");
    assert!(settlement.origin.input_recovery.observe(
        XAuthorityClientInputDelivery {
            client,
            delivery,
            outcome: XAuthorityInputDeliveryOutcome::Flushed,
        }
    ));
    assert_eq!(settlement.reclaim_outstanding(), 1);
    assert_eq!(durable.reserved(), 0);
    assert!(settlement.is_settled());
}

#[test]
fn independent_terminal_dropped_shutdown_handle_retains_late_completion_reclamation() {
    let namespace = NamespaceId::from_raw(66);
    let client = XServerFrontendClientId(84);
    let surface = SurfaceId::new(72, 1);
    let window = XResourceId::new(0x200280, 1);
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            gate,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner: {refusal:?}"));
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    let delivery = XAuthorityInputDeliveryId::from_raw(10600);
    private
        .ingress()
        .submit(motion_to(surface, delivery))
        .expect("an open coordinator to accept work");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);

    let origin = private.broker.registry.clone();
    // Nothing is owed -- the admission queue was empty -- so the handle's own
    // Drop used to return before it reached the routed work and destroy it.
    let settlement = private.shutdown();
    assert_eq!(settlement.owed(), 0);
    assert_eq!(settlement.outstanding(), 1);
    drop(settlement);

    assert_eq!(
        durable.outstanding(),
        1,
        "routed work outlives an abandoned handle, however little else is owed"
    );
    assert_eq!(durable.reserved(), 1, "and keeps the credit it already had");

    // Driving before it finishes releases nothing: the work is still live, and
    // a drive is not a terminal outcome.
    assert!(!durable.drive().made_progress());
    assert_eq!(durable.outstanding(), 1, "still waiting on a real outcome");
    assert_eq!(durable.reserved(), 1);

    // It finishes for real, and driving the owner reclaims it once.
    origin
        .send_input_delivery(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .expect("the terminal outcome to be recorded");
    assert!(origin.input_recovery.observe(XAuthorityClientInputDelivery {
        client,
        delivery,
        outcome: XAuthorityInputDeliveryOutcome::Flushed,
    }));

    // Reclaiming is progress even though nothing was acknowledged. A single
    // count would report this drive as having achieved nothing.
    let progress = durable.drive();
    assert_eq!(progress.answered, 0, "nothing was owed an acknowledgement");
    assert_eq!(progress.reclaimed, 1, "but a credit was released");
    assert!(progress.made_progress());
    assert_eq!(durable.outstanding(), 0);
    assert_eq!(durable.reserved(), 0, "released once, on a real outcome");
    let again = durable.drive();
    assert!(!again.made_progress(), "and not a second time");
    assert_eq!(durable.reserved(), 0);
}

#[test]
fn a_control_completion_closes_only_on_a_real_acknowledgement() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4);
    let command = XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(11001),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
    };
    let token = registry.register(command).expect("a fresh registry");
    assert_eq!(registry.outstanding(), 1);

    // A delivered acknowledgement retires the record.
    let (delivered, delivered_receiver) = sync_channel(4);
    let channels = X11ControlChannels::Routed {
        receiver: channel().1,
        acknowledgements: delivered,
        completion: Some(registry.clone()),
    };
    let ack = XAuthorityControlAck {
        kind: XAuthorityControlKind::ConfigureSurface,
        transaction: TransactionId::from_raw(11001),
        surface,
        outcome: XAuthorityControlOutcome::Delivered,
    };
    channels
        .send_ack_for(client, ack, Some(token))
        .expect("a free channel to publish");
    assert!(delivered_receiver.try_recv().is_ok());
    assert_eq!(registry.outstanding(), 0, "a delivered ack retires it");
}

#[test]
fn a_full_channel_retains_the_acknowledgement_without_replaying_the_command() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4);
    let command = XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(11002),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
    };
    let token = registry.register(command).expect("a fresh registry");

    // One slot, already taken.
    let (full, full_receiver) = sync_channel(1);
    full.try_send(XAuthorityClientControlAck {
        client,
        acknowledgement: XAuthorityControlAck {
            kind: XAuthorityControlKind::ConfigureSurface,
            transaction: TransactionId::from_raw(1),
            surface,
            outcome: XAuthorityControlOutcome::Delivered,
        },
    })
    .expect("the empty slot");
    let channels = X11ControlChannels::Routed {
        receiver: channel().1,
        acknowledgements: full,
        completion: Some(registry.clone()),
    };
    let ack = XAuthorityControlAck {
        kind: XAuthorityControlKind::ConfigureSurface,
        transaction: TransactionId::from_raw(11002),
        surface,
        outcome: XAuthorityControlOutcome::Delivered,
    };
    assert!(
        channels.send_ack_for(client, ack, Some(token)).is_err(),
        "a full channel reports failure to the writer"
    );

    // The outcome is retained, not the command: the effect already happened,
    // so this is republished later and never re-run.
    assert_eq!(registry.owed(), 1);
    assert_eq!(registry.outstanding(), 1);

    // Draining lets it publish, once.
    assert!(full_receiver.try_recv().is_ok());
    let mut republished = Vec::new();
    let delivered = registry.publish_owed_with(|acknowledgement| {
        republished.push(*acknowledgement);
        ControlPublication::Delivered
    });
    assert_eq!(delivered, 1);
    assert_eq!(republished.len(), 1);
    assert_eq!(
        republished[0].acknowledgement.transaction,
        TransactionId::from_raw(11002)
    );
    assert_eq!(registry.owed(), 0);
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        0,
        "and not a second time"
    );

    // A retry that cannot publish keeps the outcome rather than consuming it.
    let held = crate::ControlCompletionRegistry::with_capacity(2);
    let token = held.register(command).expect("a fresh registry");
    held.publish(
        token,
        XAuthorityClientControlAck {
            client,
            acknowledgement: ack,
        },
        ControlPublication::Retained,
    );
    assert_eq!(held.publish_owed_with(|_| ControlPublication::Retained), 0);
    assert_eq!(held.owed(), 1, "a failed retry does not consume the outcome");
}

#[test]
fn a_gone_receiver_is_not_a_published_acknowledgement() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4);
    let command = XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(11003),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
    };
    let token = registry.register(command).expect("a fresh registry");

    // The receiver is dropped, which send_ack has always reported as Ok.
    let (gone, gone_receiver) = sync_channel(4);
    drop(gone_receiver);
    let channels = X11ControlChannels::Routed {
        receiver: channel().1,
        acknowledgements: gone,
        completion: Some(registry.clone()),
    };
    let ack = XAuthorityControlAck {
        kind: XAuthorityControlKind::ConfigureSurface,
        transaction: TransactionId::from_raw(11003),
        surface,
        outcome: XAuthorityControlOutcome::Delivered,
    };
    // Ordinary behaviour is unchanged: the writer is not failed for this.
    channels
        .send_ack_for(client, ack, Some(token))
        .expect("a gone receiver is tolerated by the writer");

    // But nothing was published, so the record is not closed. Treating the Ok
    // as publication would mark work complete whose acknowledgement nobody
    // received.
    assert_eq!(
        registry.outstanding(),
        1,
        "a gone receiver published nothing"
    );
    assert_eq!(registry.owed(), 1);
}

#[test]
fn a_cancellation_edge_does_not_call_a_partly_applied_command_unexecuted() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4);
    let command = |transaction| XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(transaction),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
    };
    let queued = registry.register(command(11004)).expect("a fresh registry");
    let started = registry.register(command(11005)).expect("a fresh registry");
    let _ = queued;
    registry.begin_applying(started);

    // Neither has an established outcome, so neither has an acknowledgement to
    // publish. A retry that produced one would be inventing the receipt these
    // records exist to avoid inventing.
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        0,
        "a record with no outcome owes no acknowledgement"
    );
    assert_eq!(registry.outstanding(), 2, "and both are still held");

    let cancellation = registry.cancel_unfinished();

    // The one that never started can be cancelled truthfully.
    assert_eq!(cancellation.cancellable.len(), 1);
    assert_eq!(
        cancellation.cancellable[0].command.transaction(),
        TransactionId::from_raw(11004)
    );
    // The one that had begun is not reported as unexecuted, because the
    // runtime may already have changed. It is retained until something
    // establishes what happened.
    assert_eq!(cancellation.indeterminate, 1);
    assert_eq!(registry.outstanding(), 1);
}

/// Build a private frontend with a client and one surface registered.
#[cfg(unix)]
fn private_with_client(
    acknowledgements: SyncSender<XAuthorityClientControlAck>,
    durable: &crate::PrivateSettlementOwner,
    client: XServerFrontendClientId,
    surface: SurfaceId,
) -> (
    crate::PrivateXServerFrontend,
    XServerFrontendClientRouteChannels,
    XServerFrontendClientRouteRegistration,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (delivery_sender, delivery_receiver) = channel();
    let (gate, _instance, _issuer, _submit) = control_gate_with_submit();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: acknowledgements,
            input_deliveries: delivery_sender,
            gate,
        },
        durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    let (registration, channels) = private.broker.registry.register_client(client).unwrap();
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(252),
            surface,
            XResourceId::new(0x200252, 1),
        )
        .unwrap();
    // The registration and the delivery receiver are returned rather than
    // leaked: dropping either is a cancellation edge, and a test that hid one
    // would be proving a different thing from the one it names.
    (private, channels, registration, delivery_receiver)
}

#[cfg(unix)]
fn configure(client: XServerFrontendClientId, surface: SurfaceId, transaction: u64) -> XAuthorityClientControlCommand {
    XAuthorityClientControlCommand {
        client,
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(transaction),
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 60,
            },
        },
    }
}

#[test]
fn a_control_credit_is_released_exactly_once_when_its_writer_answers() {
    let client = XServerFrontendClientId(252);
    let surface = SurfaceId::new(252, 1);
    let (acknowledgements, ack_receiver) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &durable, client, surface);

    private
        .control_producer()
        .submit(configure(client, surface, 12001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending().expect("a turn");
    assert_eq!(ran.len(), 1);

    // Routed is not answered: the writer still has it.
    assert_eq!(
        private.reclaim_settled(),
        0,
        "handing a command to a writer is not an outcome"
    );

    // The writer's own path, with the registry the route registry installed.
    let routed = channels
        .control
        .recv_timeout(std::time::Duration::from_millis(500))
        .expect("the routed control");
    let X11RoutedControl::Authority {
        command, completion, ..
    } = routed
    else {
        panic!("an authority control");
    };
    assert!(completion.is_some(), "the writer is given the registration");
    let writer = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: private.broker.registry.control_completion(),
    };
    assert!(
        writer.completion().is_some(),
        "a private instance installs its registry where a client writer reads it"
    );
    writer.begin_applying(completion);
    writer
        .send_ack_for(
            client,
            XAuthorityControlAck {
                kind: command.kind(),
                transaction: command.transaction(),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
            completion,
        )
        .expect("a free channel");
    assert!(ack_receiver.try_recv().is_ok());

    assert_eq!(
        private.reclaim_settled(),
        1,
        "an answered control releases its credit"
    );
    assert_eq!(
        private.reclaim_settled(),
        0,
        "and cannot release it a second time"
    );
}

#[test]
fn a_writer_that_stops_seals_only_the_client_it_served() {
    let gone = XServerFrontendClientId(253);
    let live = XServerFrontendClientId(254);
    let surface = SurfaceId::new(253, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8);
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements: sync_channel(4).0,
        completion: Some(registry.clone()),
    };

    channels.seal_completions(gone);
    assert!(registry.is_sealed(gone));
    assert!(
        !registry.is_sealed(live),
        "one writer stopping says nothing about another client's writer"
    );

    assert!(matches!(
        registry.register(configure(gone, surface, 13001)),
        Err((crate::ControlCompletionRefusal::Sealed, returned))
            if returned.command.transaction() == TransactionId::from_raw(13001)
    ));
    assert!(
        registry.register(configure(live, surface, 13002)).is_ok(),
        "a client whose writer is alive can still have work accepted"
    );
}

#[test]
fn a_refused_control_leaves_no_registration_to_answer_for_it() {
    let client = XServerFrontendClientId(255);
    let surface = SurfaceId::new(255, 1);
    let (acknowledgements, _ack_receiver) = sync_channel(8);
    // One credit for the whole owner, so the second submit is refused.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);

    let producer = private.control_producer();
    producer
        .submit(configure(client, surface, 14001))
        .expect("the first to be accepted");
    let refused = producer.submit(configure(client, surface, 14002));
    let Err((_, returned)) = refused else {
        panic!("the second to be refused once the owner is saturated");
    };
    assert_eq!(
        returned.command.transaction(),
        TransactionId::from_raw(14002),
        "the caller keeps the command it was never told had been taken"
    );

    // The refused command has exactly one owner: the caller. A registration
    // left behind would answer for it at the next cancellation edge, and
    // answer twice if the caller retried.
    let report = private.shutdown();
    let answered: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert!(
        !answered.contains(&14002),
        "a refused command is not carried by the instance that refused it"
    );
}

#[test]
fn a_registration_only_answers_to_the_registry_that_issued_it() {
    let client = XServerFrontendClientId(256);
    let surface = SurfaceId::new(256, 1);
    let issuer = crate::ControlCompletionRegistry::with_capacity(2);
    let other = crate::ControlCompletionRegistry::with_capacity(2);
    let token = issuer
        .register(configure(client, surface, 15001))
        .expect("a fresh registry");

    assert_eq!(
        issuer.state_of(token),
        crate::ControlRecordState::Outstanding
    );
    // Absent here too, and absence is not an outcome: answering for it would
    // release another instance's credit on the strength of a record this
    // registry never held.
    assert_eq!(
        other.state_of(token),
        crate::ControlRecordState::Unanswerable
    );

    assert!(issuer.discard(token));
    assert_eq!(issuer.state_of(token), crate::ControlRecordState::Retired);
    assert_eq!(
        other.state_of(token),
        crate::ControlRecordState::Unanswerable
    );
}

#[test]
fn an_owed_outcome_is_not_given_up_as_though_it_were_unexecuted() {
    let client = XServerFrontendClientId(257);
    let surface = SurfaceId::new(257, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(2);
    let token = registry
        .register(configure(client, surface, 16001))
        .expect("a fresh registry");
    registry.publish(
        token,
        XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::ConfigureSurface,
                transaction: TransactionId::from_raw(16001),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        },
        ControlPublication::Retained,
    );

    // The effect already happened, so this record holds the only copy of an
    // outcome. Handing the operation to another owner cannot apply to it.
    assert!(
        !registry.discard(token),
        "an owed outcome is not something that can be handed on"
    );
    assert_eq!(registry.owed(), 1);
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Outstanding
    );
}

#[test]
fn a_poisoned_registry_answers_for_nothing_and_frees_nothing() {
    let client = XServerFrontendClientId(258);
    let surface = SurfaceId::new(258, 1);
    let (acknowledgements, _ack_receiver) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);

    private
        .control_producer()
        .submit(configure(client, surface, 17001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);

    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    let token = registry
        .register(configure(client, surface, 17002))
        .expect("room for a second");

    // An owed outcome, so the retry below has something to call back into.
    registry.publish(
        token,
        XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::ConfigureSurface,
                transaction: TransactionId::from_raw(17002),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        },
        ControlPublication::Retained,
    );

    // Poison it from inside its own lock, which is the only way it happens.
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            poisoner.publish_owed_with(|_| panic!("poisoning the registry"));
        })
        .join()
        .is_err(),
        "the panic happened while the lock was held"
    );

    // Silence is not an answer. Reading it as one would release a credit for
    // work still outstanding.
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Unanswerable
    );
    assert_eq!(
        private.reclaim_settled(),
        0,
        "an unreadable registry releases no credit"
    );
}

#[test]
fn a_command_a_writer_never_ran_is_carried_out_rather_than_dropped() {
    let client = XServerFrontendClientId(259);
    let surface = SurfaceId::new(259, 1);
    let (acknowledgements, _ack_receiver) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);

    private
        .control_producer()
        .submit(configure(client, surface, 18001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    // It left the queue, so draining the queue at shutdown cannot reach it.
    assert!(
        channels
            .control
            .recv_timeout(std::time::Duration::from_millis(500))
            .is_ok(),
        "the command reached a client writer's queue"
    );

    let report = private.shutdown();
    let carried: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert_eq!(
        carried,
        vec![18001],
        "a command whose writer never ran it is handed back, not lost with the instance"
    );
    assert_eq!(
        report.outstanding_control(),
        0,
        "and its record goes with it, so nothing answers for it twice"
    );
}

#[test]
fn a_rejected_control_leaves_no_record_behind_to_answer_again() {
    let client = XServerFrontendClientId(260);
    let surface = SurfaceId::new(260, 1);
    let (acknowledgements, ack_receiver) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);

    // Accepted and never routed, so shutdown answers it from the queue.
    private
        .control_producer()
        .submit(configure(client, surface, 19001))
        .expect("the shared admission to accept control");

    let report = private.shutdown();
    assert!(report.pending.is_empty(), "the rejection was delivered");
    assert_eq!(
        ack_receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("the rejection")
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::AuthorityRejected
    );
    assert_eq!(
        report.outstanding_control(),
        0,
        "a command handed to settlement gives up its record as it goes"
    );
}

#[test]
fn a_control_writer_seals_its_client_when_it_stops() {
    let client = XServerFrontendClientId(261);
    let registry = crate::ControlCompletionRegistry::with_capacity(4);
    let (routes, route_receiver) = sync_channel(4);
    let channels = X11ControlChannels::ClientBound {
        receiver: route_receiver,
        acknowledgements: sync_channel(4).0,
        completion: Some(registry.clone()),
    };

    let state = X11CoreSocketServerState::new();
    let (writer_stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let writer = spawn_x11_control_writer(
        Arc::new(Mutex::new(writer_stream)),
        Arc::new(AtomicUsize::new(0)),
        XByteOrder::LittleEndian,
        Arc::new(AtomicU16::new(1)),
        Arc::new(AtomicU64::new(0)),
        Arc::new(Mutex::new(BTreeMap::new())),
        Arc::new(Mutex::new(BTreeMap::new())),
        Arc::new(Mutex::new(BTreeMap::new())),
        Arc::new(Mutex::new(XCoreEventSelectionState::default())),
        Arc::new(AtomicU16::new(0)),
        state.atoms.clone(),
        state.properties.clone(),
        state.runtime.clone(),
        state.control_runtime_pending.clone(),
        crate::XWireClientResourceRange {
            base: 0x200000,
            mask: 0x1fffff,
        },
        NamespaceId::from_raw(261),
        client,
        None,
        channels,
    )
    .expect("a writer");

    assert!(
        !registry.is_sealed(client),
        "a serving writer has not sealed anything"
    );

    // The route queue goes, which is one of the ways a writer leaves.
    drop(routes);
    writer.thread.join().expect("the writer thread").unwrap();

    assert!(
        registry.is_sealed(client),
        "a stopped writer stops work being accepted for the client it served"
    );
}

#[test]
fn a_writer_that_unwinds_still_seals_the_client_it_served() {
    let client = XServerFrontendClientId(262);
    let registry = crate::ControlCompletionRegistry::with_capacity(4);
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements: sync_channel(4).0,
        completion: Some(registry.clone()),
    };

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _seal = X11ControlWriterSeal {
            channels: &channels,
            client,
        };
        panic!("a writer leaving the only way a call at the end cannot catch");
    }));
    assert!(unwound.is_err());
    assert!(
        registry.is_sealed(client),
        "however a writer leaves, its client stops having work accepted"
    );
}

#[test]
fn an_owed_acknowledgement_is_republished_once_the_channel_drains() {
    let client = XServerFrontendClientId(263);
    let surface = SurfaceId::new(263, 1);
    // One slot, filled, so the writer's acknowledgement cannot be published.
    let (acknowledgements, ack_receiver) = sync_channel(1);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &durable, client, surface);

    private
        .control_producer()
        .submit(configure(client, surface, 20001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);

    let routed = channels
        .control
        .recv_timeout(std::time::Duration::from_millis(500))
        .expect("the routed control");
    let X11RoutedControl::Authority {
        command, completion, ..
    } = routed
    else {
        panic!("an authority control");
    };

    // Prefill the only slot, then let the writer apply and answer.
    acknowledgements
        .try_send(XAuthorityClientControlAck {
            client,
            acknowledgement: XAuthorityControlAck {
                kind: XAuthorityControlKind::ConfigureSurface,
                transaction: TransactionId::from_raw(1),
                surface,
                outcome: XAuthorityControlOutcome::Delivered,
            },
        })
        .expect("the empty slot");
    let writer = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: private.broker.registry.control_completion(),
    };
    writer.begin_applying(completion);
    assert!(
        writer
            .send_ack_for(
                client,
                XAuthorityControlAck {
                    kind: command.kind(),
                    transaction: command.transaction(),
                    surface,
                    outcome: XAuthorityControlOutcome::Delivered,
                },
                completion,
            )
            .is_err(),
        "a full channel is reported to the writer"
    );

    // The effect happened, so the credit is still held and nothing is re-run.
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(
        private.republish_owed_acknowledgements(),
        0,
        "a channel that is still full publishes nothing"
    );

    // Drain the prefill and the outcome goes out, once.
    assert_eq!(
        ack_receiver.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1)
    );
    assert_eq!(private.republish_owed_acknowledgements(), 1);
    assert_eq!(
        ack_receiver.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(20001),
        "the outcome, not the command"
    );
    assert_eq!(
        private.republish_owed_acknowledgements(),
        0,
        "and not a second time"
    );
    assert_eq!(
        private.reclaim_settled(),
        1,
        "published at last, so the credit is released"
    );
    assert_eq!(private.reclaim_settled(), 0);
}
