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
        wait_for_x11_control_runtime(&request_pending, None);
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
        &crate::PrivateSettlementOwner::default(),
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
        .submit(motion_to(surface, XAuthorityInputDeliveryId::from_raw(95)))
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
    let (authority, issuer, submit) = private_authority();
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
    let (authority, issuer, submit) = private_authority();
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
    let (authority, issuer, submit) = private_authority();
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
        &crate::PrivateSettlementOwner::default(),
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
        .submit(motion_to(surface, delivery))
        .expect("an open coordinator to admit work");

    // The revision it was stamped under closes before the ordered pass runs.
    gate.with(|coordinator| {
        coordinator
            .request_through(&private, crate::TransitionKind::SecurityControl, 1, 1)
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
    let (authority, issuer, submit) = private_authority();
    // Ingress capacity larger than the ready stream's ordinary share, so more
    // can be sent than one pass can admit.
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
    let (authority, issuer, submit) = private_authority();
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
        &crate::PrivateSettlementOwner::default(),
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
            .request_through(&private, crate::TransitionKind::SecurityControl, 1, 1)
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
    let (authority, issuer, submit) = private_authority();
    // The production constructor at its smallest: ready capacity six, of
    // which four are held for cleanup, so ordinary work has room for two.
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
    let (authority, issuer, submit) = private_authority();
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
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(16).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    let (authority, issuer, submit) = private_authority();
    // Ordinary share of two at the smallest production size.
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a failure slot: {refusal:?}"));
    // A producer is refused for a client with no control writer, so a test
    // about admission capacity has to give it one.
    let (_registration, _channels) = private.broker.registry.register_client(client).unwrap();
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
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    let (authority, issuer, submit) = private_authority();
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
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    let (authority, issuer, submit) = private_authority();
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
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    let (authority, issuer, submit) = private_authority();
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
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    // Two authorities, so the two instances are genuinely separate: each
    // derives its own coordinator from the one it owns.
    let first_parts = private_authority();
    let second_parts = private_authority();

    let build = |ack, delivery, parts: (
        sophia_input_authority::AuthorityInstance,
        sophia_input_authority::IssuerHandle,
        sophia_input_authority::SubmitHandle,
    )| {
        let (authority, issuer, submit) = parts;
        let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: ack,
            input_deliveries: delivery,
            authority,
            issuer,
            submit,
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
    let (first, _r1, _c1) = build(first_ack, first_delivery, first_parts);
    let (second, _r2, _c2) = build(second_ack, second_delivery, second_parts);

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
    let (authority, issuer, submit) = private_authority();
    let durable = crate::PrivateSettlementOwner::default();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
        durable.owed().expect("a readable owner"),
        1,
        "an abandoned obligation outlives the handle that held it"
    );

    // Capacity frees afterwards, and the durable owner discharges it against
    // the registry that accepted it.
    let first = control_ack_receiver.recv().expect("the prefilled ack");
    assert_eq!(first.acknowledgement.transaction, TransactionId::from_raw(1));

    assert_eq!(durable.drive().answered, 1, "the durable owner answers it");
    assert_eq!(durable.owed().expect("a readable owner"), 0);

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
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "the answered credit is free again");
}

#[test]
fn an_unreadable_queue_is_owned_by_something_that_outlives_it() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let durable = crate::PrivateSettlementOwner::default();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);
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
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    assert_eq!(durable.owed().expect("a readable owner"), 1);
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
    assert_eq!(durable.reserved().expect("a readable owner"), 0);

    // The second cannot settle, because the channel is now full, so it keeps
    // the only credit.
    let (owed, _r1, _c1) = review_settlement_queue(sender.clone(), &durable, 9801);
    assert_eq!(owed.owed(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // A third cannot even be accepted: the storage that would have to hold its
    // work if abandoned is spoken for. Refusing here costs a producer only
    // work it was never told had been taken.
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    let third = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    assert_eq!(durable.owed().expect("a readable owner"), 1);
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
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "the answered credit is free again");
}

#[test]
fn a_failed_instance_hands_over_its_queue_not_a_tally() {
    let (control_ack_sender, _control_ack_receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let durable = crate::PrivateSettlementOwner::default();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: control_ack_sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);
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
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(2).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    let owed_before = durable.owed().expect("a readable owner");

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
    assert_eq!(durable.owed().expect("a readable owner"), 0);
    let _ = owed_before;
}

#[test]
fn a_failed_instances_queue_can_still_be_answered() {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, receiver) = sync_channel(4);
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);

    // Retaining the queue was for this. A poisoned lock stays poisoned, but
    // the obligations behind it are intact and still owed, so they can be
    // answered against the registry that accepted them. A tally could have
    // been counted and never discharged.
    assert_eq!(durable.recover_failed().expect("a readable owner"), 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("the work the failed instance had accepted"),
        review_settlement_expected(9950)
    );
    assert_eq!(durable.failed_instances().expect("a readable owner"), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "its credit is free again");
}

#[test]
fn recovering_a_failed_queue_takes_the_completion_record_before_it_answers() {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, receiver) = sync_channel(4);
    let surface = SurfaceId::new(253, 1);
    let client = XServerFrontendClientId(253);
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
            NamespaceId::from_raw(253),
            surface,
            XResourceId::new(0x200253, 1),
        )
        .unwrap();
    // Kept past the frontend, which shutdown consumes. The registry is what
    // knows whether an operation still has an owner able to publish for it.
    let completion = private
        .broker
        .registry
        .control_completion()
        .expect("a registry that issues completion records");
    private
        .control_producer()
        .submit(configure(client, surface, 9970))
        .expect("the shared admission to accept control");
    assert_eq!(
        completion.outstanding(),
        Some(1),
        "accepted, so a record answers for it"
    );

    let admission = std::sync::Arc::clone(&private.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();
    drop(private.shutdown());
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);
    assert_eq!(
        completion.outstanding(),
        Some(1),
        "the poisoned early return hands nothing over, so the record is still live"
    );

    assert_eq!(durable.recover_failed().expect("a readable owner"), 1);
    assert_eq!(
        receiver
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("the work the failed instance had accepted"),
        completion_ack(
            configure(client, surface, 9970),
            XAuthorityControlOutcome::AuthorityRejected
        )
    );
    // The point of the test. Emitting an outcome while a record that can also
    // publish one is still held gives the operation two owners, and the first
    // acknowledgement has already gone by the time anyone looks.
    assert_eq!(
        completion.outstanding(),
        Some(0),
        "recovery took the record before it published"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
}

#[test]
fn a_failure_slot_is_reserved_before_an_instance_is_exposed() {
    // Room for one failed instance across the whole owner.
    let durable = crate::PrivateSettlementOwner::with_capacity(1);
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();

    let first = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("the only failure slot: {refusal:?}"));

    // A second cannot be built: if it failed, there would be nowhere to hand
    // its queue. Refusing construction costs a caller an instance it never
    // had; refusing the transfer afterwards would drop responsibility for one
    // that existed and accepted work.
    let (second_delivery, _second_delivery_receiver) = channel();
    let (second_authority, second_issuer, second_submit) = private_authority();
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: second_delivery,
            authority: second_authority,
            issuer: second_issuer,
            submit: second_submit,
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
    let (third_authority, third_issuer, third_submit) = private_authority();
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: third_delivery,
            authority: third_authority,
            issuer: third_issuer,
            submit: third_submit,
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
    let (authority, issuer, submit) = private_authority();

    // A is built, accepts nothing, and fails. Its slot is spent on a failure
    // that carries no credit, which is why failure slots are counted apart
    // from credits.
    let empty = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    assert_eq!(durable.failed_instances().expect("a readable owner"), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "A accepted nothing");

    // B is refused before exposure, so it never accepts work that would be
    // evicted. This is the whole difference: a refusal here costs a caller an
    // instance it never had.
    let (b_delivery, _b_delivery_receiver) = channel();
    let (b_authority, b_issuer, b_submit) = private_authority();
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: b_delivery,
            authority: b_authority,
            issuer: b_issuer,
            submit: b_submit,
        },
        &durable,
    )
        .is_err(),
        "B must not be exposed without room to hand over its queue"
    );

    // Resolving A returns the slot, and B can then be built.
    assert_eq!(durable.recover_failed().expect("a readable owner"), 0, "A had accepted nothing");
    assert_eq!(durable.failed_instances().expect("a readable owner"), 0);
    let (c_delivery, _c_delivery_receiver) = channel();
    let (c_authority, c_issuer, c_submit) = private_authority();
    assert!(
        crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: c_delivery,
            authority: c_authority,
            issuer: c_issuer,
            submit: c_submit,
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
    let (authority, issuer, submit) = private_authority();
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(2).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // The consumer routes it to the client's writer queue. Nothing has
    // acknowledged it.
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    assert_eq!(
        durable.reserved().expect("a readable owner"),
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
    let (authority, issuer, submit) = private_authority();
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
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    // Routed to the client, not yet delivered: the ledger still holds a
    // ticket for it, so the credit stays with the work.
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

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
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
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
    let (authority, issuer, submit) = private_authority();
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
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

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
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
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
    let (authority, issuer, submit) = private_authority();
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
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

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
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
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
    let (authority, issuer, submit) = private_authority();
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
        durable.outstanding().expect("a readable owner"),
        1,
        "routed work outlives an abandoned handle, however little else is owed"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 1, "and keeps the credit it already had");

    // Driving before it finishes releases nothing: the work is still live, and
    // a drive is not a terminal outcome.
    assert!(!durable.drive().made_progress());
    assert_eq!(durable.outstanding().expect("a readable owner"), 1, "still waiting on a real outcome");
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

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
    assert_eq!(durable.outstanding().expect("a readable owner"), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "released once, on a real outcome");
    let again = durable.drive();
    assert!(!again.made_progress(), "and not a second time");
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
}

#[test]
fn a_control_completion_closes_only_on_a_real_acknowledgement() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
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
    let token = accepted(&registry, command);
    assert_eq!(registry.outstanding().expect("a readable registry"), 1);

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
    assert_eq!(registry.outstanding().expect("a readable registry"), 0, "a delivered ack retires it");
}

#[test]
fn a_full_channel_records_the_acknowledgement_rather_than_the_command() {
    // Registry contract only. Nothing here applies a command, so nothing here
    // shows an effect happening; what an applied effect looks like is
    // a_full_channel_retains_the_outcome_of_an_effect_a_writer_really_applied.
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
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
    let token = accepted(&registry, command);

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

    // The outcome is retained, not the command. Where this is reached for
    // real the effect has already happened,
    // so this is republished later and never re-run.
    assert_eq!(registry.owed().expect("a readable registry"), 1);
    assert_eq!(registry.outstanding().expect("a readable registry"), 1);

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
    assert_eq!(registry.owed().expect("a readable registry"), 0);
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        0,
        "and not a second time"
    );

    // A retry that cannot publish keeps the outcome rather than consuming it.
    let held = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let token = accepted(&held, command);
    assert_eq!(
        held.publish_with(
            token,
            XAuthorityClientControlAck {
                client,
                acknowledgement: ack,
            },
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );
    assert_eq!(held.publish_owed_with(|_| ControlPublication::Retained), 0);
    assert_eq!(held.owed().expect("a readable registry"), 1, "a failed retry does not consume the outcome");
}

#[test]
fn a_gone_receiver_is_not_a_published_acknowledgement() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
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
    let token = accepted(&registry, command);

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
        registry.outstanding().expect("a readable registry"),
        1,
        "a gone receiver published nothing"
    );
    assert_eq!(registry.owed().expect("a readable registry"), 1);
}

#[test]
fn a_cancellation_edge_does_not_call_a_partly_applied_command_unexecuted() {
    let surface = SurfaceId::new(251, 1);
    let client = XServerFrontendClientId(251);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
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
    let queued = accepted(&registry, command(11004));
    let started = accepted(&registry, command(11005));
    let _ = queued;
    assert_eq!(
        registry.claim_execution(started),
        crate::ControlExecutionClaim::Claimed
    );

    // Neither has an established outcome, so neither has an acknowledgement to
    // publish. A retry that produced one would be inventing the receipt these
    // records exist to avoid inventing.
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        0,
        "a record with no outcome owes no acknowledgement"
    );
    assert_eq!(registry.outstanding().expect("a readable registry"), 2, "and both are still held");

    let cancellation = registry.cancel_unfinished();

    // The one that never started can be cancelled truthfully.
    assert_eq!(cancellation.cancellable.len(), 1);
    assert_eq!(
        cancellation.cancellable[0].1.command.transaction(),
        TransactionId::from_raw(11004)
    );
    // The one that had begun is not reported as unexecuted, because the
    // runtime may already have changed. It is retained until something
    // establishes what happened.
    assert_eq!(cancellation.indeterminate, 1);
    assert_eq!(registry.outstanding().expect("a readable registry"), 1);
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
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            control_acknowledgements: acknowledgements,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
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
fn a_control_credit_is_released_exactly_once_when_its_outcome_is_recorded() {
    // The acknowledgement helper and the credit accounting, driven directly.
    // No command is applied here; a_writer_applies_a_control_and_its_credit_is
    // _released_once drives the production writer against the real runtime.
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
    assert_eq!(
        writer.resume_execution(completion),
        ControlExecutionClaim::Resumed,
        "routing claimed execution already; a writer only continues it"
    );
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
    let issuer = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let other = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let token = accepted(&issuer, configure(client, surface, 15001));

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
    let registry = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 16001));
    assert_eq!(
        registry.publish_with(
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
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );

    // The effect already happened, so this record holds the only copy of an
    // outcome. Handing the operation to another owner cannot apply to it.
    assert!(
        !registry.discard(token),
        "an owed outcome is not something that can be handed on"
    );
    assert_eq!(registry.owed().expect("a readable registry"), 1);
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
    let token = accepted(&registry, configure(client, surface, 17002));

    // An owed outcome, so the retry below has something to call back into.
    assert_eq!(
        registry.publish_with(
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
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
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
fn a_command_queued_to_a_writer_is_never_cancelled_as_unexecuted() {
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

    // Routing was the first authoritative effect and claimed execution, so
    // this operation is mid-flight, not unexecuted. Handing it back here
    // would give it a second owner able to answer AuthorityRejected while the
    // copy still in the writer's queue went on to apply and answer Delivered.
    let report = private.shutdown();
    let carried: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert!(
        carried.is_empty(),
        "a command a writer could still run is not handed to a second owner"
    );
    assert_eq!(
        report.outstanding_control().expect("a readable registry"),
        1,
        "it is retained, because what the runtime did is not established"
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
        report.outstanding_control().expect("a readable registry"),
        0,
        "a command handed to settlement gives up its record as it goes"
    );
}

#[test]
fn a_control_writer_records_that_its_client_has_none_when_it_stops() {
    let client = XServerFrontendClientId(261);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (registration, _client_channels) = broker.registry.register_client(client).unwrap();
    let routing = broker.registry.clone();
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
        Arc::new(X11WirePermission::open()),
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
        Some(routing.clone()),
        channels,
    )
    .expect("a writer");

    assert!(
        routing.control_writer_present(client),
        "a serving writer is present"
    );

    // The route queue goes, which is one of the ways a writer leaves.
    drop(routes);
    writer.thread.join().expect("the writer thread").unwrap();

    assert!(
        !routing.control_writer_present(client),
        "a stopped writer stops work being accepted for the client it served"
    );
    drop(registration);
}

#[test]
fn a_writer_that_unwinds_still_records_that_its_client_has_none() {
    let client = XServerFrontendClientId(262);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (registration, _channels) = broker.registry.register_client(client).unwrap();
    let routing = broker.registry.clone();
    assert!(routing.control_writer_present(client));

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _seal = X11ControlWriterSeal {
            routing: Some(&routing),
            client,
        };
        panic!("a writer leaving the only way a call at the end cannot catch");
    }));
    assert!(unwound.is_err());
    assert!(
        !routing.control_writer_present(client),
        "however a writer leaves, its client stops having work accepted"
    );
    drop(registration);
}

#[test]
fn a_recorded_outcome_is_republished_once_the_channel_drains() {
    // Registry and retry contract, driven directly. The real-writer form is
    // a_full_channel_retains_the_outcome_of_an_effect_a_writer_really_applied.
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
    assert_eq!(
        writer.resume_execution(completion),
        ControlExecutionClaim::Resumed,
        "routing claimed execution already; a writer only continues it"
    );
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

    // Unpublished is not answered, so the credit is still held.
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


/// Register and take acceptance, as a producer does under the admitting hold.
///
/// A bare `register` leaves a reservation, which is still the producer's and
/// which no part of the instance may act on. Tests about an accepted operation
/// have to take that step too, or they are testing a different phase.
#[cfg(unix)]
fn accepted(
    registry: &crate::ControlCompletionRegistry,
    command: XAuthorityClientControlCommand,
) -> ControlCompletionToken {
    // A client with a running writer is what a test means by an accepted
    // operation. Registered-and-awaiting-a-spawn is a different state and the
    // tests that mean it say so.
    registry.writer_started(command.client);
    let token = registry
        .register(command)
        .expect("a fresh registry to have room");
    registry
        .begin_acceptance(token)
        .expect("a fresh reservation is acceptable")
        .commit();
    token
}

#[cfg(unix)]
fn completion_ack(
    command: XAuthorityClientControlCommand,
    outcome: XAuthorityControlOutcome,
) -> XAuthorityClientControlAck {
    XAuthorityClientControlAck {
        client: command.client,
        acknowledgement: XAuthorityControlAck {
            kind: command.command.kind(),
            transaction: command.command.transaction(),
            surface: command.command.surface(),
            outcome,
        },
    }
}

#[test]
fn an_acknowledgement_for_another_operation_cannot_settle_this_one() {
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let mine = configure(XServerFrontendClientId(301), SurfaceId::new(301, 1), 21001);
    let theirs = configure(XServerFrontendClientId(302), SurfaceId::new(302, 1), 21002);
    let token = accepted(&registry, mine);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );

    // The token is the identity, but it is not the whole identity: what the
    // acknowledgement names must be what was registered, or one request's
    // outcome settles another's record.
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(theirs, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Err(crate::ControlPublicationRefusal::NotThisOperation),
    );
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Outstanding,
        "and the operation it was registered for is still unanswered"
    );

    // Its own acknowledgement settles it.
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(mine, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Ok(ControlPublication::Delivered),
    );
}

#[test]
fn an_established_outcome_is_not_replaced_by_a_later_contradiction() {
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = configure(XServerFrontendClientId(303), SurfaceId::new(303, 1), 21003);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let established = completion_ack(command, XAuthorityControlOutcome::Delivered);
    assert_eq!(
        registry.publish_with(token, established, |_| ControlPublication::Retained),
        Ok(ControlPublication::Retained),
    );

    // What happened does not become something else later.
    let contradiction = completion_ack(command, XAuthorityControlOutcome::ClientGone);
    assert_eq!(
        registry.publish_with(token, contradiction, |_| ControlPublication::Retained),
        Err(crate::ControlPublicationRefusal::OutcomeAlreadyEstablished),
    );
    // Repeating the same one says nothing new and is not an error.
    assert_eq!(
        registry.publish_with(token, established, |_| ControlPublication::Retained),
        Ok(ControlPublication::Retained),
    );

    let mut republished = Vec::new();
    assert_eq!(
        registry.publish_owed_with(|acknowledgement| {
            republished.push(*acknowledgement);
            ControlPublication::Delivered
        }),
        1
    );
    assert_eq!(republished, vec![established], "the first outcome stands");
}

#[test]
fn a_retired_registration_cannot_answer_for_a_later_one() {
    let registry = crate::ControlCompletionRegistry::with_capacity(1).expect("an unused origin");
    let old = configure(XServerFrontendClientId(304), SurfaceId::new(304, 1), 21004);
    // One identity left. Exhaustion is reachable only by placing the counter
    // near its end, and that setter stays here in the test rather than in
    // production src.
    registry.inner.lock().unwrap().next_incarnation = u64::MAX - 1;
    let stale = accepted(&registry, old);
    assert!(registry.discard(stale));

    // The counter cannot advance, so registration refuses rather than issue
    // the identity it just gave away.
    let new = configure(XServerFrontendClientId(305), SurfaceId::new(305, 1), 21005);
    assert!(matches!(
        registry.register(new),
        Err((crate::ControlCompletionRefusal::Exhausted, returned))
            if returned.command.transaction() == TransactionId::from_raw(21005)
    ));
    assert_eq!(
        registry.outstanding().expect("a readable registry"),
        0,
        "nothing was accepted, so nothing is owed"
    );

    // And the stale token answers for nothing.
    assert_eq!(
        registry.publish_with(
            stale,
            completion_ack(old, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Err(crate::ControlPublicationRefusal::NoLongerHeld),
    );
}

#[test]
fn an_exhausted_registry_refuses_rather_than_issue_one_identity_twice() {
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    registry.inner.lock().unwrap().next_incarnation = u64::MAX - 1;
    let first = accepted(&registry, configure(
            XServerFrontendClientId(307),
            SurfaceId::new(307, 1),
            21008,
        ));
    let second = registry.register(configure(
        XServerFrontendClientId(308),
        SurfaceId::new(308, 1),
        21009,
    ));
    let Err((refusal, returned)) = second else {
        panic!("a second identity cannot exist, so it must be refused");
    };
    assert_eq!(refusal, crate::ControlCompletionRefusal::Exhausted);
    assert_eq!(
        returned.command.transaction(),
        TransactionId::from_raw(21009),
        "and the command goes back to the caller that still owns it"
    );
    assert_eq!(
        registry.state_of(first),
        crate::ControlRecordState::Outstanding,
        "the one that was accepted keeps its own record"
    );
}

// The production control writer, on an owned socketpair, against the real
// runtime. Ported from an independent review's fixture: the acknowledgement
// helper can be exercised without any of this, and doing so proves nothing
// about whether an effect happened.

/// A runtime with one registered window, forty wide.
#[cfg(unix)]
fn writer_runtime(surface: SurfaceId) -> X11CoreSocketServerState {
    let state = X11CoreSocketServerState::new();
    state
        .runtime
        .lock()
        .unwrap()
        .apply(crate::XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(70000),
            namespace: NamespaceId::from_raw(252),
            kind: crate::XAuthorityRequestKind::CreateWindow {
                window: XResourceId::new(0x200252, 1),
                surface,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 40,
                    height: 30,
                },
                constraints: sophia_protocol::SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            },
        });
    assert_eq!(writer_window_width(&state), 40);
    state
}

#[cfg(unix)]
fn writer_window_width(state: &X11CoreSocketServerState) -> i32 {
    state
        .runtime
        .lock()
        .unwrap()
        .window_geometry(NamespaceId::from_raw(252), XResourceId::new(0x200252, 1))
        .unwrap()
        .width
}

#[cfg(unix)]
fn writer_windows(surface: SurfaceId) -> Arc<Mutex<BTreeMap<SurfaceId, XResourceId>>> {
    Arc::new(Mutex::new(BTreeMap::from([(
        surface,
        XResourceId::new(0x200252, 1),
    )])))
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn writer_start(
    registry: Option<&XServerFrontendRouteRegistry>,
    state: &X11CoreSocketServerState,
    control: Receiver<X11RoutedControl>,
    completion: Option<crate::ControlCompletionRegistry>,
    acknowledgements: SyncSender<XAuthorityClientControlAck>,
    client: XServerFrontendClientId,
    windows: Arc<Mutex<BTreeMap<SurfaceId, XResourceId>>>,
    priority: Arc<AtomicUsize>,
) -> (X11ControlWriter, std::os::unix::net::UnixStream) {
    if let Some(registry) = registry {
        registry
            .select_core_events(client, XResourceId::new(0x200252, 1), 1 << 17)
            .unwrap();
    }
    let (stream, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .unwrap();
    let writer = spawn_x11_control_writer(
        Arc::new(Mutex::new(stream)),
        priority,
        Arc::new(X11WirePermission::open()),
        XByteOrder::LittleEndian,
        Arc::new(AtomicU16::new(1)),
        Arc::new(AtomicU64::new(0)),
        windows,
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
        NamespaceId::from_raw(252),
        client,
        registry.cloned(),
        X11ControlChannels::ClientBound {
            receiver: control,
            acknowledgements,
            completion,
        },
    )
    .expect("a writer");
    (writer, peer)
}

#[cfg(unix)]
fn writer_join(writer: X11ControlWriter) -> bool {
    writer.stop.store(true, Ordering::Release);
    let limit = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !writer.thread.is_finished() && std::time::Instant::now() < limit {
        std::thread::yield_now();
    }
    assert!(writer.thread.is_finished(), "an owned writer stops bounded");
    writer.thread.join().unwrap().is_ok()
}

/// Read one ConfigureNotify and check the width it announced.
#[cfg(unix)]
fn writer_configure_notify(peer: &mut std::os::unix::net::UnixStream, width: u16) {
    let mut frame = [0_u8; 32];
    std::io::Read::read_exact(peer, &mut frame).unwrap();
    assert_eq!(frame[0] & 127, 22, "ConfigureNotify");
    assert_eq!(
        u32::from_le_bytes(frame[8..12].try_into().unwrap()),
        0x200252
    );
    assert_eq!(u16::from_le_bytes(frame[20..22].try_into().unwrap()), width);
}

#[test]
fn a_writer_applies_a_control_and_its_credit_is_released_once() {
    let client = XServerFrontendClientId(290);
    let surface = SurfaceId::new(290, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (acknowledgements, acks) = sync_channel(2);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &durable, client, surface);
    let state = writer_runtime(surface);

    private
        .control_producer()
        .submit(configure(client, surface, 71001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    assert_eq!(
        private.reclaim_settled(),
        0,
        "routed is not answered: the writer still has it"
    );

    let (writer, mut peer) = writer_start(
        Some(&private.broker.registry),
        &state,
        channels.control,
        private.broker.registry.control_completion(),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );

    let ack = acks
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("the acknowledgement");
    assert_eq!(ack.acknowledgement.transaction, TransactionId::from_raw(71001));
    assert_eq!(ack.acknowledgement.outcome, XAuthorityControlOutcome::Delivered);
    // The effect, not a description of one.
    writer_configure_notify(&mut peer, 80);
    assert_eq!(writer_window_width(&state), 80);

    assert_eq!(private.reclaim_settled(), 1);
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    assert!(writer_join(writer));
    assert!(acks.try_recv().is_err(), "and exactly one outcome");
}

#[test]
fn a_claimed_control_is_not_cancelled_out_from_under_its_writer() {
    let client = XServerFrontendClientId(293);
    let surface = SurfaceId::new(293, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (acknowledgements, acks) = sync_channel(4);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &durable, client, surface);
    let state = writer_runtime(surface);
    let windows = writer_windows(surface);
    let priority = Arc::new(AtomicUsize::new(0));

    private
        .control_producer()
        .submit(configure(client, surface, 73001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);

    // Park the writer inside the operation: it has taken the command and
    // claimed it, and cannot reach the runtime until this lock is released.
    let held = windows.lock().unwrap();
    let (writer, mut peer) = writer_start(
        Some(&private.broker.registry),
        &state,
        channels.control,
        private.broker.registry.control_completion(),
        acknowledgements.clone(),
        client,
        windows.clone(),
        priority.clone(),
    );
    let limit = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while priority.load(Ordering::Acquire) == 0 && std::time::Instant::now() < limit {
        std::thread::yield_now();
    }
    assert!(
        priority.load(Ordering::Acquire) > 0,
        "the writer has the command and is inside the operation"
    );

    // Closing the instance cannot take an operation whose execution is
    // claimed. Handing it back would answer AuthorityRejected for work this
    // writer is about to apply, and the writer would then answer Delivered
    // for the same transaction.
    let mut report = private.shutdown();
    assert!(
        report.pending.is_empty(),
        "a claimed operation is not handed to a second owner"
    );
    assert_eq!(report.retry(), 0);
    assert_eq!(
        report.outstanding_control().expect("a readable registry"),
        1,
        "it is retained: what the runtime did is not established yet"
    );
    assert!(
        acks.recv_timeout(std::time::Duration::from_millis(100))
            .is_err(),
        "and no outcome is invented for it"
    );
    assert_eq!(writer_window_width(&state), 40);

    // Released, the same writer finishes the operation it claimed.
    drop(held);
    let answered = acks
        .recv_timeout(std::time::Duration::from_millis(500))
        .expect("the writer's own outcome");
    assert_eq!(
        answered.acknowledgement.transaction,
        TransactionId::from_raw(73001)
    );
    assert_eq!(
        answered.acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );
    writer_configure_notify(&mut peer, 80);
    assert_eq!(writer_window_width(&state), 80);
    assert!(writer_join(writer));
    assert!(
        acks.try_recv().is_err(),
        "exactly one terminal outcome, and it is the one that happened"
    );
}

#[test]
fn a_writer_refused_the_claim_produces_no_effect_and_no_outcome() {
    let client = XServerFrontendClientId(294);
    let surface = SurfaceId::new(294, 1);
    let state = writer_runtime(surface);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let (acknowledgements, acks) = sync_channel(4);
    let (routes, control) = sync_channel(4);

    // Two operations a writer must refuse: one whose record was given up to
    // another owner, and one that never claimed execution, which means the
    // first authoritative effect was skipped.
    let command = configure(client, surface, 74001);
    let handed_on = accepted(&registry, command);
    assert!(registry.discard(handed_on));
    let unclaimed = accepted(&registry, configure(client, surface, 74002));

    let routed = |transaction, surface, completion| X11RoutedControl::Authority {
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
        focus: None,
        claim: None,
        completion,
    };
    routes
        .try_send(routed(74001, surface, Some(handed_on)))
        .expect("room");
    routes
        .try_send(routed(74002, surface, Some(unclaimed)))
        .expect("room");
    // A sentinel the writer does answer, so the two above are known to have
    // been reached rather than left in the queue by a writer that stopped
    // first. It names no surface this writer maps, so it changes nothing.
    routes
        .try_send(routed(74003, SurfaceId::new(999, 1), None))
        .expect("room");

    let (writer, mut peer) = writer_start(
        None,
        &state,
        control,
        Some(registry.clone()),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );

    let sentinel = acks
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("the sentinel, so the two before it were reached");
    assert_eq!(
        sentinel.acknowledgement.transaction,
        TransactionId::from_raw(74003)
    );
    assert_eq!(
        sentinel.acknowledgement.outcome,
        XAuthorityControlOutcome::UnknownSurface
    );
    assert!(writer_join(writer));

    assert_eq!(
        writer_window_width(&state),
        40,
        "a refused claim applies nothing"
    );
    assert!(
        acks.try_recv().is_err(),
        "and answers nothing: the outcome belongs to whoever refused the claim"
    );
    let mut byte = [0_u8; 1];
    assert_eq!(
        std::io::Read::read(&mut peer, &mut byte).unwrap(),
        0,
        "and writes nothing"
    );
    assert_eq!(
        registry.state_of(unclaimed),
        crate::ControlRecordState::Outstanding,
        "the record that still holds it keeps holding it"
    );
}

#[test]
fn a_full_channel_retains_the_outcome_of_an_effect_a_writer_really_applied() {
    let client = XServerFrontendClientId(291);
    let surface = SurfaceId::new(291, 1);
    let durable = crate::PrivateSettlementOwner::default();
    // One slot, filled by a real earlier instance's rejection rather than a
    // forged acknowledgement.
    let (acknowledgements, acks) = sync_channel(1);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &durable, client, surface);
    let state = writer_runtime(surface);
    let earlier_client = XServerFrontendClientId(292);
    let earlier_surface = SurfaceId::new(292, 1);
    let (earlier, _channels, _registration, _deliveries) = private_with_client(
        acknowledgements.clone(),
        &durable,
        earlier_client,
        earlier_surface,
    );
    earlier
        .control_producer()
        .submit(configure(earlier_client, earlier_surface, 72000))
        .expect("the shared admission to accept control");
    let _earlier = earlier.shutdown();

    private
        .control_producer()
        .submit(configure(client, surface, 72001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    let (writer, mut peer) = writer_start(
        Some(&private.broker.registry),
        &state,
        channels.control,
        private.broker.registry.control_completion(),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );

    // The effect happens once, and then the writer cannot publish it.
    writer_configure_notify(&mut peer, 80);
    assert_eq!(writer_window_width(&state), 80);
    assert!(!writer_join(writer), "a full channel fails this writer");
    assert_eq!(
        private.reclaim_settled(),
        0,
        "unpublished is not answered, so the credit stays"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // Drained, the retained outcome goes out once. The writer has already
    // gone, so nothing can apply the command a second time.
    assert_eq!(
        acks.recv_timeout(std::time::Duration::from_millis(500))
            .unwrap()
            .acknowledgement
            .transaction,
        TransactionId::from_raw(72000)
    );
    assert_eq!(private.republish_owed_acknowledgements(), 1);
    let republished = acks
        .recv_timeout(std::time::Duration::from_millis(500))
        .expect("the retained outcome");
    assert_eq!(
        republished.acknowledgement.transaction,
        TransactionId::from_raw(72001)
    );
    assert_eq!(
        republished.acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert_eq!(private.republish_owed_acknowledgements(), 0);
    assert_eq!(private.reclaim_settled(), 1);
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    assert!(acks.try_recv().is_err());
    let mut byte = [0_u8; 1];
    assert_eq!(
        std::io::Read::read(&mut peer, &mut byte).unwrap(),
        0,
        "republishing an outcome does not re-run the command"
    );
    assert_eq!(writer_window_width(&state), 80, "applied exactly once");

    // The registration outlived its writer: returning on a full channel is
    // exactly how that happens. Nothing is left to execute work for this
    // client, and the producer is told so, even though the client is still
    // registered and nothing about it has been revoked.
    assert!(
        !private.broker.registry.control_writer_present(client),
        "a writer that returned on a full channel is gone"
    );
    assert!(matches!(
        private
            .control_producer()
            .submit(configure(client, surface, 72002)),
        Err((crate::AdmissionRefusal::ConsumerGone, returned))
            if returned.command.transaction() == TransactionId::from_raw(72002)
    ));
}

#[test]
fn transferring_an_unexecuted_command_moves_its_credit_rather_than_freeing_it() {
    let client = XServerFrontendClientId(295);
    let surface = SurfaceId::new(295, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (acknowledgements, acks) = sync_channel(8);
    let (mut private, channels, registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);

    // One operation reaches a writer and stays there.
    private
        .control_producer()
        .submit(configure(client, surface, 75001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // A second is accepted while the client is still there, and the client
    // goes before it can be routed. It keeps its record and never claims
    // execution, because routing refuses before its first effect.
    private
        .control_producer()
        .submit(configure(client, surface, 75002))
        .expect("the shared admission to accept control");
    assert_eq!(durable.reserved().expect("a readable owner"), 2);
    drop(registration);
    assert!(matches!(
        private.route_pending(),
        Err(XServerFrontendRouteError::UnknownClient { .. })
    ));

    // Closing transfers the unexecuted one to a single new owner. Ownership
    // moving is not the operation terminating: the identity must leave
    // `outstanding` with the command, or one watcher reads the record's
    // absence as completion and frees the credit that settling the transfer
    // will free again.
    let mut report = private.shutdown();
    let carried: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert_eq!(carried, vec![75002], "handed on, with one owner");
    assert_eq!(durable.reserved().expect("a readable owner"), 2, "and nothing freed by the move");

    assert_eq!(report.retry(), 1, "the transfer settles, once");
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(75002)
    );
    // And the one that reached a writer never began a step of its own, which
    // its own reports establish, so the settlement that outlived the instance
    // applies that proof and releases its credit too. Each is released once,
    // for a different reason: one was handed on and answered, the other is
    // known to have had no effect.
    assert_eq!(report.reclaim_outstanding(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "each released exactly once");
    assert!(acks.try_recv().is_err(), "and exactly one outcome");
    let _ = channels;
}

#[test]
fn a_claim_answers_for_every_state_a_record_can_be_in() {
    let client = XServerFrontendClientId(309);
    let surface = SurfaceId::new(309, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let other = crate::ControlCompletionRegistry::with_capacity(1).expect("an unused origin");
    let command = |transaction| configure(client, surface, transaction);

    // Reserved: still the producer's, so no part of the instance may act.
    let reservation = registry.register(command(22000)).expect("a fresh registry");
    for claim in [
        registry.claim_execution(reservation),
        registry.resume_execution(reservation),
    ] {
        assert_eq!(
            claim,
            crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NotAccepted),
            "a reservation has not been handed over yet"
        );
    }
    assert!(registry.release_reservation(reservation));

    // Accepted: a beginning, and not a continuation of anything.
    let fresh = accepted(&registry, command(22001));
    assert_eq!(
        registry.resume_execution(fresh),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NotStarted),
        "a writer that finds it unclaimed means the first effect was skipped"
    );
    assert_eq!(
        registry.claim_execution(fresh),
        crate::ControlExecutionClaim::Claimed
    );

    // Applying: a continuation, and not a second beginning.
    assert_eq!(
        registry.resume_execution(fresh),
        crate::ControlExecutionClaim::Resumed
    );
    assert_eq!(
        registry.claim_execution(fresh),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::AlreadyApplying),
        "claiming twice would apply it twice"
    );

    // Answered: acting now would act after the answer.
    assert_eq!(
        registry.publish_with(
            fresh,
            completion_ack(command(22001), XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );
    for claim in [
        registry.claim_execution(fresh),
        registry.resume_execution(fresh),
    ] {
        assert_eq!(
            claim,
            crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::AlreadyAnswered)
        );
    }

    // Gone: another owner holds it.
    let handed_on = accepted(&registry, command(22002));
    assert!(registry.discard(handed_on));
    assert_eq!(
        registry.claim_execution(handed_on),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NoLongerHeld)
    );

    // Foreign: this registry never accepted it.
    assert_eq!(
        other.claim_execution(fresh),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Foreign)
    );

    // None of those permit an effect; the two that are permission do.
    for refused in [
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NotStarted),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::AlreadyApplying),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::AlreadyAnswered),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NoLongerHeld),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Foreign),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Unavailable),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NotAccepted),
    ] {
        assert!(!refused.permits_effects(), "{refused:?}");
    }
    assert!(crate::ControlExecutionClaim::Claimed.permits_effects());
    assert!(crate::ControlExecutionClaim::Resumed.permits_effects());
    assert!(crate::ControlExecutionClaim::Ungoverned.permits_effects());
}

#[test]
fn a_poisoned_registry_permits_no_effect() {
    let client = XServerFrontendClientId(310);
    let surface = SurfaceId::new(310, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = configure(client, surface, 23001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            poisoner.publish_owed_with(|_| panic!("poisoning the registry"));
        })
        .join()
        .is_err()
    );

    // A registry that cannot be read cannot establish who owns this, and
    // proceeding on silence is how an effect happens beside an answer.
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Unavailable)
    );
    assert_eq!(
        registry.resume_execution(token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Unavailable)
    );
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Err(crate::ControlPublicationRefusal::Unavailable)
    );
}

#[test]
fn a_registration_with_no_registry_to_answer_to_permits_no_effect() {
    let client = XServerFrontendClientId(311);
    let surface = SurfaceId::new(311, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 24001));

    // A writer holding a registration whose registry it cannot reach cannot
    // establish who owns the outcome, so it may not produce one.
    let orphaned = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements: sync_channel(1).0,
        completion: None,
    };
    assert_eq!(
        orphaned.resume_execution(Some(token)),
        ControlExecutionClaim::Refused(ControlClaimRefusal::Unavailable)
    );
    // An operation with no registration at all is ungoverned, exactly as the
    // ordinary path has always been.
    assert_eq!(
        orphaned.resume_execution(None),
        ControlExecutionClaim::Ungoverned
    );
}

#[test]
fn a_focus_control_that_cannot_claim_disturbs_no_one_elses_focus() {
    let focused = XServerFrontendClientId(312);
    let claimant = XServerFrontendClientId(313);
    let focused_surface = SurfaceId::new(312, 1);
    let claimant_surface = SurfaceId::new(313, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, held_channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, focused, focused_surface);
    let (claimant_registration, claimant_channels) = private
        .broker
        .registry
        .register_client(claimant)
        .expect("a second client");
    private
        .broker
        .registry
        .register_surface(
            claimant,
            NamespaceId::from_raw(252),
            claimant_surface,
            XResourceId::new(0x200253, 1),
        )
        .expect("a second surface");

    // The first client holds focus.
    private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client: focused,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(25001),
                surface: focused_surface,
            },
        })
        .expect("the first focus");
    assert!(held_channels.control.try_recv().is_ok());

    // A second client's focus command whose execution cannot be claimed.
    // Routing is the first authoritative effect for focus: it sends FocusOut
    // to whoever held focus and moves the focused surface before any writer
    // runs, so a claim taken later would leave those behind a record still
    // calling the operation unexecuted.
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    let command = XAuthorityClientControlCommand {
        client: claimant,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(25002),
            surface: claimant_surface,
        },
    };
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed,
        "already claimed, so routing cannot claim it again"
    );

    assert!(matches!(
        private
            .broker
            .registry
            .route_control_with_completion(command, Some(token)),
        Err(XServerFrontendRouteError::ControlNotClaimable { .. })
    ));
    assert!(
        held_channels.control.try_recv().is_err(),
        "the client that holds focus is not told it lost it"
    );
    assert!(
        claimant_channels.control.try_recv().is_err(),
        "and the command that could not be claimed was not enqueued"
    );
    drop(claimant_registration);
}

#[test]
fn a_producer_is_told_which_refusal_it_met() {
    let client = XServerFrontendClientId(314);
    let surface = SurfaceId::new(314, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // A client whose control writer has stopped: nothing is left to execute
    // this, so retrying is not the advice. Saturation would have said try
    // again.
    private.broker.registry.mark_control_writer_gone(client);
    assert!(matches!(
        private
            .control_producer()
            .submit(configure(client, surface, 26001)),
        Err((crate::AdmissionRefusal::ConsumerGone, returned))
            if returned.command.transaction() == TransactionId::from_raw(26001)
    ));

    // Exhausted identities: terminal, and not the same answer as a full
    // queue. The counter is placed at its end from the test, not from a
    // setter in production src.
    let elsewhere = XServerFrontendClientId(315);
    let (_elsewhere_registration, _elsewhere_channels) =
        private.broker.registry.register_client(elsewhere).unwrap();
    registry.inner.lock().unwrap().next_incarnation = u64::MAX;
    assert!(matches!(
        private
            .control_producer()
            .submit(configure(elsewhere, surface, 26002)),
        Err((crate::AdmissionRefusal::Exhausted, _))
    ));

    // An unreadable registry established nothing at all.
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );
    assert!(matches!(
        private
            .control_producer()
            .submit(configure(elsewhere, surface, 26003)),
        Err((crate::AdmissionRefusal::Unavailable, _))
    ));
}

#[test]
fn a_reservation_is_its_producers_until_the_instance_accepts_it() {
    let client = XServerFrontendClientId(316);
    let surface = SurfaceId::new(316, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // A producer paused between reserving and being accepted. This is what a
    // real submit looks like at that instant.
    let command = configure(client, surface, 27001);
    let reservation = registry.register(command).expect("a fresh registry");

    // Closing must not answer for it. The producer still holds the command
    // and is about to be told it was never taken; an outcome published here
    // would be a second owner answering for work its owner keeps.
    let mut report = private.shutdown();
    let cancelled: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert!(
        !cancelled.contains(&27001),
        "a reservation is not accepted work and is not the instance's to hand on"
    );
    assert_eq!(report.retry(), 0);
    assert!(
        acks.try_recv().is_err(),
        "and nothing answers for a command its producer still owns"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "nor is a credit released that was never taken");

    // The producer resumes, is refused, and takes its command back. The
    // reservation goes with it, leaving nothing behind to answer later.
    assert!(registry.release_reservation(reservation));
    assert_eq!(registry.outstanding().expect("a readable registry"), 0);
}

#[test]
fn an_acknowledgement_the_record_refuses_never_reaches_the_receiver() {
    let client = XServerFrontendClientId(317);
    let surface = SurfaceId::new(317, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let (acknowledgements, acks) = sync_channel(4);
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: Some(registry.clone()),
    };
    let command = configure(client, surface, 28001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );

    // An acknowledgement naming a different operation. Validating after the
    // send would have refused nothing: it would already be at the receiver.
    assert!(
        channels
            .send_ack_for(
                client,
                XAuthorityControlAck {
                    kind: XAuthorityControlKind::ConfigureSurface,
                    transaction: TransactionId::from_raw(28999),
                    surface,
                    outcome: XAuthorityControlOutcome::Delivered,
                },
                Some(token),
            )
            .is_err(),
        "the record refuses it"
    );
    assert!(
        acks.try_recv().is_err(),
        "and nothing was sent, which is what refusing has to mean"
    );

    // Its own outcome is established, and a contradicting one cannot follow
    // it out to the receiver either.
    channels
        .send_ack_for(
            client,
            completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
            Some(token),
        )
        .expect("its own outcome publishes");
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );

    // Retired now, so a duplicate is refused rather than sent again.
    assert!(
        channels
            .send_ack_for(
                client,
                completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
                Some(token),
            )
            .is_err(),
        "a retired registration publishes nothing"
    );
    assert!(acks.try_recv().is_err(), "exactly one terminal publication");
}

#[test]
fn a_contradicting_outcome_is_refused_before_it_is_sent() {
    let client = XServerFrontendClientId(318);
    let surface = SurfaceId::new(318, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    // One slot, filled, so the first outcome is established and unpublished.
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: Some(registry.clone()),
    };
    let command = configure(client, surface, 29001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    assert!(
        channels
            .send_ack_for(
                client,
                completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
                Some(token),
            )
            .is_err(),
        "a full channel is reported to the writer"
    );
    assert_eq!(registry.owed().expect("a readable registry"), 1);

    // Drain, then contradict what was established. The effect that happened
    // does not become a different effect, and the receiver never sees a claim
    // that it did.
    assert!(acks.try_recv().is_ok());
    assert!(
        channels
            .send_ack_for(
                client,
                completion_ack(command, XAuthorityControlOutcome::AuthorityRejected).acknowledgement,
                Some(token),
            )
            .is_err(),
        "the established outcome stands"
    );
    assert!(
        acks.try_recv().is_err(),
        "and the contradiction was never sent"
    );

    // The retry publishes the outcome that actually happened, once.
    let mut published = Vec::new();
    assert_eq!(
        registry.publish_owed_with(|owed| {
            published.push(*owed);
            ControlPublication::Delivered
        }),
        1
    );
    assert_eq!(
        published[0].acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );
}

#[test]
fn a_real_submit_paused_before_acceptance_is_not_answered_by_a_close() {
    let client = XServerFrontendClientId(319);
    let surface = SurfaceId::new(319, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // Stall a real submit between reserving its record and taking a credit,
    // which is where it stops being nothing and starts being something an
    // instance could mistake for its own.
    let held = durable.inner.lock().unwrap();
    let (finished, refusals) = sync_channel(1);
    let producer = private.control_producer();
    let submitting = std::thread::spawn(move || {
        let _ = finished.send(producer.submit(configure(client, surface, 83001)));
    });
    let limit = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while registry.outstanding().expect("a readable registry") == 0 && std::time::Instant::now() < limit {
        std::thread::yield_now();
    }
    let reserved_while_stalled = registry.outstanding().expect("a readable registry");

    // The instance closes while that reservation exists. It is not accepted
    // work, so it is not the instance's to answer or hand on.
    let mut report = private.settle_accepted();
    // Observed under the barrier and asserted after it. An assertion here
    // would unwind with the barrier still held, and the report's own Drop
    // reacquires the same lock.
    let cancelled: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    let answered_while_stalled = acks.try_recv().is_ok();
    drop(held);

    // The producer resumes, is refused, and takes its own command back.
    let refused = refusals
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("the producer to be answered");
    submitting.join().expect("the producer thread");

    assert_eq!(
        reserved_while_stalled, 1,
        "the submit had reserved its record and had not been accepted"
    );
    assert!(
        cancelled.is_empty(),
        "a close does not take a command its producer still owns"
    );
    assert!(
        !answered_while_stalled,
        "and nothing answers for a transaction that was never accepted"
    );
    let Err((refusal, returned)) = refused else {
        panic!("a closed instance accepts nothing");
    };
    assert_eq!(refusal, crate::AdmissionRefusal::ConsumerGone);
    assert_eq!(
        returned.command.transaction(),
        TransactionId::from_raw(83001),
        "the caller keeps the command it was never told had been taken"
    );
    assert_eq!(
        registry.outstanding().expect("a readable registry"),
        0,
        "and the reservation went back with it, leaving nothing to answer later"
    );
    assert_eq!(report.retry(), 0);
    assert!(acks.try_recv().is_err(), "exactly no outcomes");
}

#[test]
fn a_producer_reserves_nothing_for_a_client_that_has_gone() {
    let client = XServerFrontendClientId(320);
    let surface = SurfaceId::new(320, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // The client goes. Nothing is left to execute work for it, so a producer
    // is refused before anything is reserved: the seal ledger is bounded and
    // can forget a client with nothing left to protect, and this is the
    // answer that does not depend on it.
    drop(registration);
    assert!(matches!(
        private
            .control_producer()
            .submit(configure(client, surface, 84001)),
        Err((crate::AdmissionRefusal::ConsumerGone, returned))
            if returned.command.transaction() == TransactionId::from_raw(84001)
    ));
    assert_eq!(
        registry.outstanding().expect("a readable registry"),
        0,
        "nothing was reserved, so nothing is owed and no credit was taken"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
}

#[test]
fn no_outcome_is_published_for_a_record_its_producer_still_owns() {
    let client = XServerFrontendClientId(321);
    let surface = SurfaceId::new(321, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let (acknowledgements, acks) = sync_channel(4);
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: Some(registry.clone()),
    };
    let command = configure(client, surface, 30001);
    let reservation = registry.register(command).expect("a fresh registry");

    // Nothing has been handed over, so there is no outcome of it to publish.
    // Checked before the emission: a verdict reached afterwards would leave
    // the acknowledgement at the receiver whatever it then decided.
    assert_eq!(
        registry.publish_with(
            reservation,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| panic!("a reservation must not reach the emission"),
        ),
        Err(crate::ControlPublicationRefusal::NotAccepted)
    );
    assert!(
        channels
            .send_ack_for(
                client,
                completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
                Some(reservation),
            )
            .is_err(),
        "and the writer path refuses it too"
    );
    assert!(acks.try_recv().is_err(), "nothing was sent");
    assert_eq!(registry.outstanding().expect("a readable registry"), 1, "and the reservation is untouched");
}

#[test]
fn a_publication_that_fails_leaves_the_reservation_with_its_producer() {
    let client = XServerFrontendClientId(322);
    let surface = SurfaceId::new(322, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    let producer = private.control_producer();

    // Fill the shared order until it refuses.
    let mut accepted_count = 0;
    let mut refused = None;
    for transaction in 31000..31100 {
        match producer.submit(configure(client, surface, transaction)) {
            Ok(_) => accepted_count += 1,
            Err(refusal) => {
                refused = Some(refusal);
                break;
            }
        }
    }
    let Some((refusal, returned)) = refused else {
        panic!("a bounded order refuses eventually");
    };
    assert_eq!(refusal, crate::AdmissionRefusal::Saturated);

    // The queue entry could not be published, so the handover was rolled back
    // and the reservation released with the command. A promotion recorded
    // beside the publication would have left the record accepted, and
    // releasing a reservation does not remove an accepted record: it would
    // have stayed here answering for work this caller was handed back.
    assert_eq!(
        registry.outstanding().expect("a readable registry"),
        accepted_count,
        "only what was accepted has a record"
    );
    assert_eq!(
        returned.command.transaction().raw(),
        31000 + accepted_count as u64,
        "and the caller keeps the one that was refused"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), accepted_count);
}

#[test]
fn nothing_is_admitted_without_the_handover_it_was_accepted_for() {
    let client = XServerFrontendClientId(323);
    let surface = SurfaceId::new(323, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);

    // A handover that cannot be prepared -- an unreadable registry, a record
    // already gone -- means the queue entry must not be published either.
    // Admitting anyway would leave the instance owning work whose record says
    // its producer still owns it, which is the whole reason these are one
    // transaction.
    let refused = private.admission.accept_with(
        crate::ReadyClass::Control,
        PrivateOperation::Control(configure(client, surface, 32001), None),
        || None,
    );
    let Err((refusal, returned)) = refused else {
        panic!("no handover, no acceptance");
    };
    assert_eq!(refusal, crate::AdmissionRefusal::Unavailable);
    assert!(
        matches!(
            returned,
            PrivateOperation::Control(control, _)
                if control.command.transaction() == TransactionId::from_raw(32001)
        ),
        "the payload goes back to its caller"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "and so does the credit it reserved");
    assert!(
        private.route_pending().expect("a turn").is_empty(),
        "and nothing was queued"
    );
}

#[test]
fn stopping_one_writer_does_not_leave_the_others_running() {
    let client = XServerFrontendClientId(324);
    let surface = SurfaceId::new(324, 1);
    let state = writer_runtime(surface);
    let (routes, control) = sync_channel(4);
    let (acknowledgements, _acks) = sync_channel(4);
    let (control_writer, _peer) = writer_start(
        None,
        &state,
        control,
        None,
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );
    let control_stop = control_writer.stop.clone();

    // An input writer that fails the moment it is joined, ahead of the others.
    let failing_stop = Arc::new(AtomicBool::new(false));
    let failing = X11InputEventWriter {
        stop: failing_stop.clone(),
        thread: std::thread::spawn(|| {
            Err(X11SetupSocketError::new("an input writer that failed"))
        }),
    };
    let mut writers = X11ClientWriters {
        input: Some(failing),
        control: Some(control_writer),
        protocol: None,
        transport: std::os::unix::net::UnixStream::pair().unwrap().0,
    };

    let shutdown = writers.shut_down();
    assert!(shutdown.outcome.is_err(), "the first failure is reported");
    assert_eq!(
        shutdown.joined, 2,
        "and every writer is waited for, not left detached behind the failure"
    );
    // And the ones behind it were stopped and joined anyway. Returning on the
    // first failure left them running, never told to stop, holding a stream
    // and a route queue.
    assert!(
        control_stop.load(Ordering::Acquire),
        "every writer is told to stop, whatever an earlier one did"
    );
    assert!(
        writers.control.is_none() && writers.input.is_none(),
        "and every writer is joined, not abandoned"
    );
    let again = writers.shut_down();
    assert!(again.outcome.is_ok() && again.joined == 0,
        "shutting down twice finds nothing left to do");
    let _ = routes;
}

#[test]
fn a_registration_lost_while_its_writer_is_there_abandons_nothing() {
    let client = XServerFrontendClientId(325);
    let surface = SurfaceId::new(325, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, channels, registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // One reaches a writer's queue and claims execution; one never leaves the
    // shared order.
    private
        .control_producer()
        .submit(configure(client, surface, 33001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    private
        .control_producer()
        .submit(configure(client, surface, 33002))
        .expect("the shared admission to accept control");
    assert_eq!(durable.reserved().expect("a readable owner"), 2);

    // A writer is running for this client. The registration going is not
    // proof that it stopped: it may have claimed this operation and still be
    // inside it, about to establish an outcome that abandoning it here would
    // then refuse.
    registry.writer_started(client);
    drop(registration);
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty(),
        "nothing is owed a cleanup while something could still answer"
    );
    registry.writer_started(client);
    let reconciled = registry.reconcile_client(client);
    assert!(reconciled.readable);
    assert_eq!(reconciled.abandoned, 0);
    assert_eq!(
        reconciled.applying, 1,
        "it is still being applied, and that is all anyone here knows"
    );
    assert_eq!(
        reconciled.unexecuted, 1,
        "and the one that never started is still truthfully unexecuted"
    );
    assert_eq!(reconciled.owed, 0, "nothing was answered");
    assert_eq!(reconciled.reserved, 0);

    // The writer going is the last-owner edge, and it makes the transition
    // itself rather than leaving it for whoever asks next.
    registry.writer_stopped(client);
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1,
        "the last owner leaving is what owes the cleanup"
    );
    let reconciled = registry.reconcile_client(client);
    assert_eq!(reconciled.abandoned, 1);
    assert_eq!(reconciled.applying, 0);
    assert!(
        acks.try_recv().is_err(),
        "nothing is published for an operation nobody can describe"
    );
    assert_eq!(
        private.reclaim_settled(),
        0,
        "and its credit stays with it"
    );

    // It cannot be resumed, and no outcome may be published for it.
    let cleanups = registry.cleanups_owed().expect("a readable registry");
    assert_eq!(cleanups.len(), 1);
    let owed = cleanups[0];
    assert_eq!(
        owed.command.command.transaction(),
        TransactionId::from_raw(33001)
    );
    assert_eq!(
        registry.resume_execution(owed.token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Abandoned)
    );
    assert_eq!(
        registry.publish_with(
            owed.token,
            completion_ack(owed.command, XAuthorityControlOutcome::Delivered),
            |_| panic!("an abandoned operation must not reach the emission"),
        ),
        Err(crate::ControlPublicationRefusal::Abandoned)
    );

    // Until something establishes that nothing is owed, it stays owed, and
    // so does its credit.
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1
    );
    assert_eq!(private.reclaim_settled(), 0, "so the credit stays too");

    // Retiring it is not an outcome, but it is the end of what is owed.
    assert_eq!(registry.reconcile_unstarted().discharged, 1);
    assert!(registry.cleanups_owed().expect("a readable registry").is_empty());
    assert_eq!(
        private.reclaim_settled(),
        1,
        "and exactly one credit is released for it"
    );
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(
        registry.reconcile_unstarted().discharged,
        0,
        "and not a second time"
    );
    let _ = channels;
}

#[test]
fn dropping_the_writers_stops_them_even_if_nobody_shut_them_down() {
    let client = XServerFrontendClientId(326);
    let surface = SurfaceId::new(326, 1);
    let state = writer_runtime(surface);
    let (routes, control) = sync_channel(4);
    let (acknowledgements, _acks) = sync_channel(4);
    let (control_writer, _peer) = writer_start(
        None,
        &state,
        control,
        None,
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );
    let stop = control_writer.stop.clone();

    // A setup failure between two spawns leaves by dropping, not by calling
    // anything. Whatever has already started is still owned.
    drop(X11ClientWriters {
        input: None,
        control: Some(control_writer),
        protocol: None,
        transport: std::os::unix::net::UnixStream::pair().unwrap().0,
    });
    assert!(
        stop.load(Ordering::Acquire),
        "a writer nobody shut down is stopped by losing the thing that owned it"
    );
    let _ = routes;
}

#[test]
fn an_unreadable_registry_reconciles_nothing_and_says_so() {
    let client = XServerFrontendClientId(327);
    let surface = SurfaceId::new(327, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let command = configure(client, surface, 34001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );

    // Finding nothing because nothing could be looked at is not finding
    // nothing. A caller that read this as a clean teardown would walk away
    // from an operation still mid-application.
    let reconciled = registry.reconcile_client(client);
    assert!(!reconciled.readable);
    assert_eq!(reconciled.abandoned, 0);
    assert!(
        !registry.reconcile_unstarted().readable,
        "and settling nothing because nothing could be read says so"
    );
}

#[test]
fn only_an_abandoned_operation_with_nothing_queued_can_be_retired() {
    let client = XServerFrontendClientId(328);
    let surface = SurfaceId::new(328, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let applying = accepted(&registry, configure(client, surface, 35001));
    assert_eq!(
        registry.claim_execution(applying),
        crate::ControlExecutionClaim::Claimed
    );
    let accepted_only = accepted(&registry, configure(client, surface, 35002));

    // An operation whose executor is still there, and one that never started,
    // are not waiting on a cleanup. Recording one for either would retire a
    // record that is still owed something else entirely.
    let report = registry.reconcile_unstarted();
    assert_eq!(report.discharged, 0);
    for token in [applying, accepted_only] {
        assert_eq!(
            registry.state_of(token),
            crate::ControlRecordState::Outstanding
        );
    }
    assert_eq!(registry.outstanding().expect("a readable registry"), 2);

    // And another registry settles only its own: it holds no record for this
    // one, whatever the local identity happens to be.
    let other = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    assert_eq!(other.reconcile_unstarted().discharged, 0);
    assert_eq!(
        registry.state_of(applying),
        crate::ControlRecordState::Outstanding
    );
}

#[test]
fn a_writer_that_stops_mid_operation_leaves_it_owed_a_cleanup() {
    let client = XServerFrontendClientId(329);
    let surface = SurfaceId::new(329, 1);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (registration, _channels) = broker.registry.register_client(client).unwrap();
    let routing = broker.registry.clone();
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    assert!(routing.install_control_completion(registry.clone()));

    // An operation a writer claimed and is inside.
    let token = accepted(&registry, configure(client, surface, 36001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty()
    );

    // The writer goes. It is the executor, so it does not have to guess
    // whether one is still there, and nothing else will establish an outcome
    // for what it was applying.
    drop(X11ControlWriterSeal {
        routing: Some(&routing),
        client,
    });

    let owed = registry.cleanups_owed().expect("a readable registry");
    assert_eq!(owed.len(), 1, "the writer's exit is what establishes it");
    assert_eq!(
        owed[0].command.command.transaction(),
        TransactionId::from_raw(36001)
    );
    assert_eq!(
        registry.resume_execution(token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Abandoned),
        "and nothing picks it up afterwards"
    );
    drop(registration);
}

#[test]
fn an_unreadable_registry_owes_an_answer_rather_than_an_empty_list() {
    let client = XServerFrontendClientId(330);
    let surface = SurfaceId::new(330, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 37001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);
    assert_eq!(registry.cleanups_owed().map(|owed| owed.len()), Ok(1));

    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );

    // An empty list means nothing is owed. It must never also mean nobody
    // could look: an owner told the first would walk away from a cleanup it
    // is holding.
    assert_eq!(
        registry.cleanups_owed(),
        Err(crate::ControlCleanupRefusal::Unavailable)
    );
    assert_eq!(registry.outstanding(), None);
    assert_eq!(registry.owed(), None);
}

#[test]
fn a_writer_parked_on_control_output_still_stops_when_told() {
    let client = XServerFrontendClientId(331);
    let (events, receiver) = sync_channel(4);
    // Control output is registered and nobody is going to clear it. Stopping
    // every writer before joining any is necessary and is not sufficient: a
    // stop flag nothing observes leaves the join waiting on a condition only
    // another thread could ever satisfy.
    let pending = Arc::new(AtomicUsize::new(1));
    let (stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let writer = spawn_x11_protocol_event_writer(
        Arc::new(Mutex::new(stream)),
        pending.clone(),
        Arc::new(X11WirePermission::open()),
        XByteOrder::LittleEndian,
        Arc::new(AtomicU16::new(1)),
        client,
        receiver,
    )
    .expect("a writer");

    events
        .try_send(XClientEvent::UnmapNotify {
            sequence: 1,
            event: XResourceId::new(0x200252, 1),
            window: XResourceId::new(0x200252, 1),
            from_configure: false,
        })
        .expect("room");
    // Give it time to take the event and park on the pending count.
    let parked = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while std::time::Instant::now() < parked {
        std::thread::yield_now();
    }

    let mut writers = X11ClientWriters {
        input: None,
        control: None,
        protocol: Some(writer),
        transport: std::os::unix::net::UnixStream::pair().unwrap().0,
    };
    let shutdown = writers.shut_down();
    assert_eq!(shutdown.joined, 1, "the join returned without a rescue");
    assert!(shutdown.outcome.is_ok(), "and stopping is not a failure");
    assert_eq!(
        pending.load(Ordering::Acquire),
        1,
        "nothing cleared the condition it was waiting for"
    );
}

#[test]
fn a_command_cannot_claim_execution_after_its_client_is_swept() {
    let client = XServerFrontendClientId(332);
    let surface = SurfaceId::new(332, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // Accepted while the client was being served, and still queued.
    private
        .control_producer()
        .submit(configure(client, surface, 38001))
        .expect("the shared admission to accept control");

    // The writer goes. The producer's check and the claim at routing are
    // separate moments, and this is between them.
    private.broker.registry.mark_control_writer_gone(client);
    registry.writer_stopped(client);
    // No writer is expected either: this client's registration is what would
    // have carried one, and nothing is coming.
    registry.cancel_expected_writer(client);
    assert_eq!(registry.reconcile_client(client).unexecuted, 1);

    // Routing must not claim it now. A record left claimable after its sweep
    // would start producing effects for a client nothing is serving.
    assert!(matches!(
        private.route_pending(),
        Err(XServerFrontendRouteError::UnknownClient { .. })
    ));
    assert!(
        acks.try_recv().is_err(),
        "and nothing was answered on the way"
    );

    // It is still exactly what it was: accepted, unexecuted, and handed on
    // when the instance closes.
    let mut report = private.shutdown();
    let carried: Vec<_> = report
        .pending
        .iter()
        .filter_map(|operation| match operation {
            PrivateOperation::Control(control, _) => Some(control.command.transaction().raw()),
            _ => None,
        })
        .collect();
    assert_eq!(carried, vec![38001]);
    assert_eq!(report.retry(), 1);
}

#[test]
fn a_writer_exit_cannot_abandon_what_a_router_is_still_inside() {
    let client = XServerFrontendClientId(333);
    let surface = SurfaceId::new(333, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 39001));

    // A routing call in flight, holding what it needs to still produce an
    // effect. Routing is where the first authoritative effect happens: focus
    // routing sends FocusOut to whoever held focus and moves the focused
    // surface before any writer runs.
    let routing = registry
        .enter_routing(client)
        .expect("a client with a writer is executing");
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );

    // The writer stops and joins, and sweeps. It is not the whole executor,
    // so this must not abandon an operation the router is still inside: the
    // effects that follow would land after the abandonment.
    registry.writer_stopped(client);
    let reconciled = registry.reconcile_client(client);
    assert_eq!(
        reconciled.abandoned, 0,
        "a writer exiting does not license cleanup while routing can act"
    );
    assert_eq!(reconciled.applying, 1);
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty()
    );

    // Once the router is out too, nothing can establish an outcome and the
    // operation is owed its cleanup.
    drop(routing);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1
    );
}

#[test]
fn taking_the_routing_lease_is_the_liveness_check_itself() {
    let client = XServerFrontendClientId(334);
    let surface = SurfaceId::new(334, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 40001));

    // A separate precheck could be true and then false before the claim. This
    // one holds what it checked, so a sweep cannot land between them.
    registry.writer_stopped(client);
    assert!(
        registry.enter_routing(client).is_none(),
        "nothing is executing, so nothing may enter"
    );
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NoExecutor),
        "and the claim refuses under the same lock that abandons"
    );

    // A client registered and waiting for its writer to spawn is executing:
    // registration comes before the spawn, and control accepted in that
    // window is not control with nowhere to go.
    registry.expect_writer(client);
    let _routing = registry
        .enter_routing(client)
        .expect("a registered client is executing");
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
}

#[test]
fn a_cancelled_input_write_is_not_reported_as_flushed() {
    let client = XServerFrontendClientId::from_raw(1);
    let window = XResourceId::new(0x200001, 1);
    let surface = SurfaceId::new(1, 1);
    let mut selections = XCoreEventSelectionState::default();
    selections.register(
        window,
        XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
        Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        },
    );
    selections.update(window, Some(1 << 6), None);
    let (deliveries, settled) = channel();
    let recovery = InputRecovery::new(4, Some(deliveries), Arc::default());
    recovery.register(client).expect("a fresh ledger");
    let (events, receiver) = channel();
    let (stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    // Control output is registered and nobody will clear it, so the writer
    // parks before touching the socket.
    let pending = Arc::new(AtomicUsize::new(1));
    let writer = spawn_x11_input_event_writer(
        X11InputWriterState {
            stream: Arc::new(Mutex::new(stream)),
            output_control_pending: pending.clone(),
            output_wire: Arc::new(X11WirePermission::open()),
            byte_order: XByteOrder::LittleEndian,
            sequence: Arc::new(AtomicU16::new(1)),
            focused_surface_window: Arc::new(AtomicU64::new(window.local.raw())),
            core_event_selections: Arc::new(Mutex::new(selections)),
            xkb_state_details: Arc::new(AtomicU16::new(1)),
            xkb_modifiers: Arc::new(AtomicU16::new(0)),
            surface_windows: Arc::new(Mutex::new(BTreeMap::from([(surface, window)]))),
            input_authority: None,
            standalone_query_authority: None,
            namespace: NamespaceId::from_raw(1),
            client,
        },
        X11InputEventReceiver::Routed {
            receiver,
            deliveries: None,
            recovery: Some(recovery.clone()),
        },
    )
    .expect("an input writer");

    let delivery = XAuthorityInputDeliveryId::from_raw(79002);
    let request = XAuthorityRoutedInput {
        request: RoutedInputRequest {
            serial: 79002,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(1),
            time_msec: 0,
            target_surface: surface,
            global_position: Point::default(),
            local_position: Point::default(),
            kind: InputEventKind::Key {
                keycode: 30,
                pressed: false,
            },
        },
        route_lease: None,
        delivery: Some(delivery),
        mode: XAuthorityRoutedInputMode::Deliver,
    };
    recovery.admit(&request, 1, std::time::Instant::now());
    recovery
        .bind(Some(delivery), client)
        .expect("a live delivery");
    events
        .send(XAuthorityClientInputEvent {
            client,
            event: XAuthorityInputEvent::Key(XAuthorityKeyEvent {
                keycode: 30,
                pressed: false,
                state: 0,
                modifiers_after: 0,
                time_msec: 0,
            }),
            target_window: Some(window),
            xi_event_type: None,
            xi_event_window: None,
            xi_emulated_button_type: None,
            xi_emulated_button_window: None,
            xi_pointer_crossing_mask: 0,
            delivery: Some(delivery),
        })
        .expect("room");

    let parked = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while std::time::Instant::now() < parked {
        std::thread::yield_now();
    }

    let mut writers = X11ClientWriters {
        input: Some(writer),
        control: None,
        protocol: None,
        transport: std::os::unix::net::UnixStream::pair().unwrap().0,
    };
    let shutdown = writers.shut_down();
    assert_eq!(shutdown.joined, 1, "the join returned without a rescue");
    assert_eq!(
        pending.load(Ordering::Acquire),
        1,
        "nothing cleared what it was waiting for"
    );

    // Nothing reached the socket. A delivery cancelled before any write must
    // not be recorded as having reached its client, and must not be blamed on
    // the recipient either.
    let outcomes: Vec<_> = settled
        .try_iter()
        .map(|delivery| delivery.outcome)
        .collect();
    assert!(
        !outcomes.is_empty(),
        "the writer reached the delivery and settled it"
    );
    assert!(
        !outcomes.contains(&XAuthorityInputDeliveryOutcome::Flushed),
        "a cancelled write is not a flush: {outcomes:?}"
    );
    assert!(
        !outcomes.contains(&XAuthorityInputDeliveryOutcome::ClientDisconnected),
        "and it is not the recipient's doing: {outcomes:?}"
    );
}

#[test]
fn the_last_owner_leaving_is_what_owes_the_cleanup() {
    let client = XServerFrontendClientId(335);
    let surface = SurfaceId::new(335, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 41001));
    let routing = registry
        .enter_routing(client)
        .expect("a client with a writer admits routing");
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );

    // The writer goes while the router is still inside it.
    registry.writer_stopped(client);
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty(),
        "a router still inside it can establish what happened"
    );

    // The router returning is the last-owner edge. It has to make the
    // transition itself: an edge that only moves an operation to its cleanup
    // when something else calls a sweep is not an edge, and nothing in
    // production calls one here.
    drop(routing);
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1,
        "the last owner leaving owes the cleanup, with no sweep asked for"
    );
    assert_eq!(
        registry.resume_execution(token),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::Abandoned)
    );
}

#[test]
fn an_old_router_is_not_permission_to_start_new_work() {
    let client = XServerFrontendClientId(336);
    let surface = SurfaceId::new(336, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let started = accepted(&registry, configure(client, surface, 42001));
    let routing = registry.enter_routing(client).expect("a running writer");
    assert_eq!(
        registry.claim_execution(started),
        crate::ControlExecutionClaim::Claimed
    );
    let fresh = accepted(&registry, configure(client, surface, 42002));

    // Admission closes while the owners already inside drain. Retaining an
    // effect-capable owner is not permission to begin something else: that
    // borrows one operation's in-flight existence as authority for another.
    registry.writer_stopped(client);
    assert!(
        registry.enter_routing(client).is_none(),
        "nothing can start new work for a client whose writer has gone"
    );
    assert_eq!(
        registry.claim_execution(fresh),
        crate::ControlExecutionClaim::Refused(crate::ControlClaimRefusal::NoExecutor),
    );
    // And the one already inside is still protected by its own owner.
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty()
    );
    drop(routing);
}

#[test]
fn a_registration_dropped_before_its_writer_spawns_cancels_the_expectation() {
    let client = XServerFrontendClientId(337);
    let surface = SurfaceId::new(337, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // Registered, and its writer has not spawned. Control accepted in that
    // window is not control with nowhere to go.
    assert!(
        registry.enter_routing(client).is_some(),
        "a registration is a writer about to exist"
    );

    // Startup fails before any worker exists. The expectation has to be
    // cancelled by something other than the writer, because there is no
    // writer to cancel it, and an expectation nobody cancels keeps this client
    // executing for as long as the registry lives.
    drop(registration);
    assert!(
        registry.enter_routing(client).is_none(),
        "nothing is coming, so nothing may start"
    );
    assert!(!private.broker.registry.control_writer_present(client));
}

#[test]
fn a_running_writer_outlives_the_registration_that_expected_it() {
    let client = XServerFrontendClientId(338);
    let surface = SurfaceId::new(338, 1);
    let state = writer_runtime(surface);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    assert!(broker.registry.install_control_completion(registry.clone()));
    let (registration, _channels) = broker.registry.register_client(client).unwrap();
    let routing = broker.registry.clone();

    let (acknowledgements, _acks) = sync_channel(4);
    // The writer reads its own queue rather than the registration's, so that
    // losing the registration is not the same event as losing the writer.
    let (_routes, control) = sync_channel(4);
    let (writer, _peer) = writer_start(
        Some(&routing),
        &state,
        control,
        Some(registry.clone()),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );
    // The writer records itself running, so the registration's expectation is
    // no longer what is keeping this client executing.
    let started = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while std::time::Instant::now() < started {
        std::thread::yield_now();
    }

    drop(registration);
    assert!(
        registry.enter_routing(client).is_some(),
        "a running writer is what is executing now, not the registration"
    );

    assert!(writer_join(writer));
    assert!(
        registry.enter_routing(client).is_none(),
        "and when it stops, nothing is"
    );
}

#[test]
fn a_parked_router_keeps_its_operation_answerable_while_its_writer_exits() {
    let client = XServerFrontendClientId(339);
    let surface = SurfaceId::new(339, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    let state = writer_runtime(surface);
    let (writer, _peer) = writer_start(
        Some(&private.broker.registry),
        &state,
        channels.control,
        Some(registry.clone()),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );

    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(43001),
                surface,
            },
        })
        .expect("the shared admission to accept control");

    // Park the router where focus routing produces its first authoritative
    // effect, holding the lease it took before claiming.
    let focus_lock = Arc::clone(&private.broker.registry.focused_surface);
    let focused = focus_lock.lock().unwrap();
    let routed = std::thread::spawn(move || {
        let outcome = private.route_pending();
        (private, outcome)
    });
    let parked = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while std::time::Instant::now() < parked {
        std::thread::yield_now();
    }

    // The writer stops and joins while the router is inside the operation it
    // claimed. Its exit is not the last-owner edge: the router still is.
    assert!(writer_join(writer));
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty(),
        "a router still inside it keeps it answerable"
    );

    drop(focused);
    let (private, _outcome) = routed.join().expect("the routing thread");
    // The router returning is the last owner leaving, and it makes the
    // transition itself: nothing here asked for a sweep.
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1,
        "and when it returns, the operation is owed its cleanup"
    );
    drop(private);
}

#[test]
fn a_writer_blocked_in_a_write_is_still_joined() {
    let client = XServerFrontendClientId(340);
    let (events, receiver) = channel();
    let (stream, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    let transport = stream.try_clone().expect("an independent handle");
    let writer = spawn_x11_protocol_event_writer(
        Arc::new(Mutex::new(stream)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(X11WirePermission::open()),
        XByteOrder::LittleEndian,
        Arc::new(AtomicU16::new(1)),
        client,
        receiver,
    )
    .expect("a writer");

    // Nobody reads the peer, so the socket fills and the writer blocks inside
    // a write. No stop flag reaches it there.
    for sequence in 0..20_000 {
        if events
            .send(XClientEvent::UnmapNotify {
                sequence,
                event: XResourceId::new(0x200252, 1),
                window: XResourceId::new(0x200252, 1),
                from_configure: false,
            })
            .is_err()
        {
            break;
        }
    }
    let filling = std::time::Instant::now() + std::time::Duration::from_millis(300);
    while std::time::Instant::now() < filling {
        std::thread::yield_now();
    }
    assert!(
        !writer.thread.is_finished(),
        "the writer is inside a write that cannot complete"
    );

    let mut writers = X11ClientWriters {
        input: None,
        control: None,
        protocol: Some(writer),
        transport,
    };
    let started = std::time::Instant::now();
    let shutdown = writers.shut_down();
    assert_eq!(shutdown.joined, 1, "the join returned");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "and returned bounded, without the peer ever reading"
    );
    drop(peer);
}

#[test]
fn a_shutdown_handle_that_cannot_be_taken_refuses_before_any_worker_starts() {
    // A poisoned output socket stands in for the descriptor that could not be
    // had. Either way the handle is unavailable, and the moment it is
    // unavailable is exactly the moment a connection is most likely to stall.
    let stream = Arc::new(Mutex::new(
        std::os::unix::net::UnixStream::pair().unwrap().0,
    ));
    let poisoner = Arc::clone(&stream);
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("poisoning the output socket");
        })
        .join()
        .is_err()
    );

    // Refused, rather than started with a shutdown that has no way to reach
    // them. A cohort that took this best-effort would lose the guarantee
    // silently.
    assert!(
        X11ClientWriters::take_transport(&stream).is_err(),
        "no handle, no workers"
    );
}

/// Handle acquisition, at the unit boundary.
///
/// Not the end-to-end case: an independent review reaches the same refusal
/// through actual setup with an allocation failure injected at the clone, and
/// that is the evidence for the production path. This reaches it through the
/// other way the same call can fail.
#[test]
fn a_refused_cohort_leaves_no_query_owner_behind() {
    let namespace = NamespaceId::from_raw(341);
    let client = XServerFrontendClientId(341);
    let state = X11CoreSocketServerState::new();

    // A standalone client has no route registration whose drop would clean up
    // a query owner, and the device pin releases only its device bundle. So
    // the order is the whole guarantee: nothing is registered until the
    // cohort's handle is in hand.
    let stream = Arc::new(Mutex::new(
        std::os::unix::net::UnixStream::pair().unwrap().0,
    ));
    let poisoner = Arc::clone(&stream);
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("poisoning the output socket");
        })
        .join()
        .is_err()
    );
    assert!(X11ClientWriters::take_transport(&stream).is_err());

    assert!(
        !state
            .runtime
            .lock()
            .unwrap()
            .shared_input_authority()
            .lock()
            .unwrap()
            .query_namespace_active(namespace),
        "a refusal registers no owner, so there is nothing to roll back"
    );

    // And registering does take effect, so the test is not passing because
    // nothing ever would -- and giving the registration up takes it back,
    // which is what every early return after it now does.
    let active = || {
        state
            .runtime
            .lock()
            .unwrap()
            .shared_input_authority()
            .lock()
            .unwrap()
            .query_namespace_active(namespace)
    };
    let owner =
        X11QueryOwner::register(&state.runtime, namespace, client, None).expect("a readable runtime");
    assert!(active(), "registering makes the namespace report an owner");
    drop(owner);
    assert!(
        !active(),
        "and losing the registration takes that owner back, however it was lost"
    );
}

/// Composed lifetime: the production cohort, the production query guard, and
/// a writer, given up together.
///
/// Distinct from the end-to-end allocation-failure case, which reaches the
/// same types through actual setup. This one is about the order between the
/// two on the way out, which that one does not observe.
#[test]
fn losing_a_connection_gives_up_its_writers_and_then_its_registration() {
    let namespace = NamespaceId::from_raw(342);
    let client = XServerFrontendClientId(342);
    let state = X11CoreSocketServerState::new();
    let active = |state: &X11CoreSocketServerState| {
        state
            .runtime
            .lock()
            .unwrap()
            .shared_input_authority()
            .lock()
            .unwrap()
            .query_namespace_active(namespace)
    };

    // A writer that reports what it could see of this client's registration at
    // the moment it stopped. That is the only way to observe which of the two
    // was given up first, rather than only that both were.
    let (sampled, samples) = sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let writer_stop = Arc::clone(&stop);
    let runtime = Arc::clone(&state.runtime);
    let thread = std::thread::spawn(move || {
        while !writer_stop.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let seen = runtime
            .lock()
            .unwrap()
            .shared_input_authority()
            .lock()
            .unwrap()
            .query_namespace_active(namespace);
        let _ = sampled.send(seen);
        Ok(())
    });

    let owned = X11ClientLifetime {
        watchdog_transport: None,
        writers: X11ClientWriters {
            input: None,
            control: Some(X11ControlWriter { stop, thread }),
            protocol: None,
            transport: std::os::unix::net::UnixStream::pair().unwrap().0,
        },
        query_owner: X11QueryOwner::register(&state.runtime, namespace, client, None)
            .expect("a readable runtime"),
    };
    assert!(active(&state));

    // Fields are given up in declaration order, so the writers go first and
    // are stopped and joined before the registration they were serving is
    // taken back. Two locals would have had it backwards: they are given up in
    // reverse, so the registration went while its workers were still running.
    drop(owned);
    assert_eq!(
        samples.recv_timeout(std::time::Duration::from_secs(2)),
        Ok(true),
        "the writers stopped while the registration they served was still there"
    );
    assert!(!active(&state), "and then it was taken back");
}

#[test]
fn nothing_is_owed_a_cleanup_while_work_it_queued_elsewhere_can_still_run() {
    let client = XServerFrontendClientId(343);
    let surface = SurfaceId::new(343, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 44001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );

    // Routing a focus change queues a FocusOut on the previously focused
    // client's writer. It outlives this operation, and this operation's own
    // router and writer going quiet says nothing about it.
    let queued = registry.track_dependent(token).expect("an applying record");
    assert_eq!(registry.dependents_outstanding(token), Some(1));

    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty(),
        "an operation with work that can still happen is not waiting on a cleanup"
    );
    // And the point that retires refuses it too. Hiding a candidate from the
    // list is not enforcement: the caller that retires has to be the one that
    // refuses.
    let report = registry.reconcile_unstarted();
    assert_eq!(report.discharged, 0);
    assert_eq!(report.retained_unproved, 1);

    // Ended -- run by that writer, or given up unrun when its queue went. Both
    // are ends, and the guard reports either the same way, because which it
    // was is not a receipt.
    drop(queued);
    assert_eq!(registry.dependents_outstanding(token), Some(0));
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1,
        "only once nothing it started can still happen"
    );
}

#[test]
fn a_dependent_effect_reports_its_end_whether_it_ran_or_was_given_up() {
    let client = XServerFrontendClientId(344);
    let surface = SurfaceId::new(344, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");

    // Given up unrun: the queue it was sitting in went away.
    let dropped = accepted(&registry, configure(client, surface, 45001));
    assert_eq!(
        registry.claim_execution(dropped),
        crate::ControlExecutionClaim::Claimed
    );
    let queued = registry.track_dependent(dropped).expect("an applying record");
    assert_eq!(registry.dependents_outstanding(dropped), Some(1));
    drop(queued);
    assert_eq!(registry.dependents_outstanding(dropped), Some(0));

    // Run: the writer it was queued on processed it and let it go.
    let ran = accepted(&registry, configure(client, surface, 45002));
    assert_eq!(
        registry.claim_execution(ran),
        crate::ControlExecutionClaim::Claimed
    );
    let effect = registry.track_dependent(ran).expect("an applying record");
    let routed = X11RoutedControl::FocusOut {
        window: XResourceId::new(0x200252, 1),
        time_msec: 7,
        claim: None,
        origin: Some(effect),
    };
    assert_eq!(registry.dependents_outstanding(ran), Some(1));
    drop(routed);
    assert_eq!(
        registry.dependents_outstanding(ran),
        Some(0),
        "the entry carries the report, so it arrives either way"
    );

    // An origin with no record left takes no count and hands back nothing to
    // hold, rather than counting against a record that is not there.
    let foreign = crate::ControlCompletionRegistry::with_capacity(2).expect("an unused origin");
    assert!(matches!(
        foreign.track_dependent(ran),
        Err(crate::ControlDependentRefusal::Foreign)
    ));
    assert_eq!(foreign.dependents_outstanding(ran), None);
}

#[test]
fn routing_a_focus_change_counts_the_focus_out_it_queues_elsewhere() {
    let focused = XServerFrontendClientId(345);
    let claimant = XServerFrontendClientId(346);
    let focused_surface = SurfaceId::new(345, 1);
    let claimant_surface = SurfaceId::new(346, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, held_channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, focused, focused_surface);
    let (claimant_registration, claimant_channels) = private
        .broker
        .registry
        .register_client(claimant)
        .expect("a second client");
    private
        .broker
        .registry
        .register_surface(
            claimant,
            NamespaceId::from_raw(252),
            claimant_surface,
            XResourceId::new(0x200253, 1),
        )
        .expect("a second surface");

    // The first client holds focus.
    private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client: focused,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(46001),
                surface: focused_surface,
            },
        })
        .expect("the first focus");
    assert!(held_channels.control.try_recv().is_ok());

    // The second takes it, through the private path so the operation has a
    // record. Routing queues a FocusOut on the first client's writer.
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client: claimant,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(46002),
                surface: claimant_surface,
            },
        })
        .expect("the shared admission to accept control");
    let ran = private.route_pending().expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // That queued effect is counted against the operation that caused it.
    // Nothing linked the two before, so the claimant's own router and writer
    // could both go quiet while it still sat in the other connection's queue.
    assert_eq!(
        registry.dependents_outstanding(token),
        Some(1),
        "the FocusOut queued elsewhere is counted against its origin"
    );
    let queued = held_channels
        .control
        .try_recv()
        .expect("the previously focused client is told");
    assert!(matches!(queued, X11RoutedControl::FocusOut { .. }));

    // And letting that entry go is what ends it.
    drop(queued);
    assert_eq!(registry.dependents_outstanding(token), Some(0));
    assert!(claimant_channels.control.try_recv().is_ok());
    drop(claimant_registration);
}

#[test]
fn a_record_that_is_gone_has_no_dependent_count_rather_than_zero() {
    let client = XServerFrontendClientId(347);
    let surface = SurfaceId::new(347, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 47001));
    assert_eq!(registry.dependents_outstanding(token), Some(0));

    // Given up to another owner. Nothing outstanding and nothing to ask about
    // are different answers, and a caller told the first would treat a record
    // it no longer holds as one with no work left.
    assert!(registry.discard(token));
    assert_eq!(registry.dependents_outstanding(token), None);
}

#[test]
fn only_an_operation_being_applied_can_start_work_elsewhere() {
    let client = XServerFrontendClientId(348);
    let surface = SurfaceId::new(348, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");

    // A reservation is its producer's and an accepted command has not started,
    // so neither is in a position to be starting anything elsewhere.
    let reserved = registry
        .register(configure(client, surface, 48001))
        .expect("a fresh registry");
    assert!(matches!(
        registry.track_dependent(reserved),
        Err(crate::ControlDependentRefusal::NotApplying)
    ));
    registry.writer_started(client);
    registry
        .begin_acceptance(reserved)
        .expect("a fresh reservation")
        .commit();
    assert!(matches!(
        registry.track_dependent(reserved),
        Err(crate::ControlDependentRefusal::NotApplying)
    ));

    // Applying is the one that can.
    assert_eq!(
        registry.claim_execution(reserved),
        crate::ControlExecutionClaim::Claimed
    );
    let held = registry
        .track_dependent(reserved)
        .expect("an applying record");
    assert_eq!(registry.dependents_outstanding(reserved), Some(1));

    // And once it is answered, it is not starting anything more.
    let command = configure(client, surface, 48001);
    assert_eq!(
        registry.publish_with(
            reserved,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Ok(ControlPublication::Delivered)
    );
    assert!(matches!(
        registry.track_dependent(reserved),
        Err(crate::ControlDependentRefusal::NotApplying)
    ));
    drop(held);

    // A record that is gone refuses rather than counting against nothing.
    assert!(matches!(
        registry.track_dependent(reserved),
        Err(crate::ControlDependentRefusal::NoLongerHeld)
    ));
}

#[test]
fn an_answered_operation_is_still_held_while_work_it_started_can_run() {
    let client = XServerFrontendClientId(349);
    let surface = SurfaceId::new(349, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let command = configure(client, surface, 49001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let queued = registry.track_dependent(token).expect("an applying record");

    // The outcome is published once. That answers the operation; it does not
    // make everything the operation started be over, and freeing its storage
    // here would free a credit while an effect of it is still queued.
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Ok(ControlPublication::Delivered)
    );
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Outstanding,
        "answered is not retired while its queued work can still run"
    );
    assert_eq!(registry.outstanding(), Some(1));

    // Nor is it a cleanup candidate: an answered operation is not waiting on
    // one, and the record only survives for the work it started.
    assert!(
        registry
            .cleanups_owed()
            .expect("a readable registry")
            .is_empty()
    );
    assert_eq!(
        registry.reconcile_unstarted().discharged,
        0,
        "an answered operation is not one the settlement retires"
    );

    // The last dependency ending retires it, and sends nothing: the
    // acknowledgement went out when the outcome was published.
    drop(queued);
    assert_eq!(registry.state_of(token), crate::ControlRecordState::Retired);
    assert_eq!(registry.outstanding(), Some(0));
}

#[test]
fn a_retried_acknowledgement_does_not_end_work_the_operation_started() {
    let client = XServerFrontendClientId(350);
    let surface = SurfaceId::new(350, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let command = configure(client, surface, 50001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let queued = registry.track_dependent(token).expect("an applying record");

    // Established but unpublished, then published by the retry.
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        1
    );
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Outstanding,
        "the retry answered it and did not end what it started"
    );
    assert_eq!(registry.dependents_outstanding(token), Some(1));

    drop(queued);
    assert_eq!(registry.state_of(token), crate::ControlRecordState::Retired);
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        0,
        "and nothing is sent a second time when the last one ends"
    );
}

#[test]
fn publishing_an_outcome_does_not_free_a_credit_while_its_focus_out_is_queued() {
    let focused = XServerFrontendClientId(351);
    let claimant = XServerFrontendClientId(352);
    let focused_surface = SurfaceId::new(351, 1);
    let claimant_surface = SurfaceId::new(352, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, held_channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &durable, focused, focused_surface);
    let (claimant_registration, _claimant_channels) = private
        .broker
        .registry
        .register_client(claimant)
        .expect("a second client");
    private
        .broker
        .registry
        .register_surface(
            claimant,
            NamespaceId::from_raw(252),
            claimant_surface,
            XResourceId::new(0x200253, 1),
        )
        .expect("a second surface");
    private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client: focused,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(51001),
                surface: focused_surface,
            },
        })
        .expect("the first focus");
    assert!(held_channels.control.try_recv().is_ok());

    let command = XAuthorityClientControlCommand {
        client: claimant,
        command: XAuthorityControlCommand::FocusSurface {
            transaction: TransactionId::from_raw(51002),
            surface: claimant_surface,
        },
    };
    private
        .control_producer()
        .submit(command)
        .expect("the shared admission to accept control");
    let ran = private.route_pending().expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // The operation is answered while the FocusOut it queued on the other
    // client still sits in that client's writer queue.
    let channels = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements,
        completion: Some(registry.clone()),
    };
    channels
        .send_ack_for(
            claimant,
            completion_ack(command, XAuthorityControlOutcome::Delivered).acknowledgement,
            Some(token),
        )
        .expect("a free channel");

    // Answering it is not everything it started being over. Freeing the
    // credit here frees storage for work an effect of this is still queued
    // against.
    assert_eq!(
        private.reclaim_settled(),
        0,
        "no credit is released while its queued FocusOut can still run"
    );
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    assert_eq!(registry.dependents_outstanding(token), Some(1));

    // The other client's writer takes it, or its queue goes. Either way the
    // effect is over, and only then is the credit free.
    let queued = held_channels
        .control
        .try_recv()
        .expect("the previously focused client is told");
    drop(queued);
    assert_eq!(private.reclaim_settled(), 1);
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    drop(claimant_registration);
}

#[test]
fn a_count_that_cannot_advance_refuses_rather_than_saturating() {
    let client = XServerFrontendClientId(353);
    let surface = SurfaceId::new(353, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 52001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    // The counter is placed at its end from the test, not from a setter in
    // production src.
    registry.inner.lock().unwrap().records[0].dependents = usize::MAX;

    // Saturating here would take responsibility for an effect and then lose
    // it the moment the first guard ended, leaving the rest uncounted.
    assert!(matches!(
        registry.track_dependent(token),
        Err(crate::ControlDependentRefusal::Exhausted)
    ));
    assert_eq!(registry.dependents_outstanding(token), Some(usize::MAX));
}

/// Reached the way production reaches it: a genuine submit and route_pending
/// that claims, parks, and finds the registry unreadable when it comes to
/// count the effect it is about to queue.
#[test]
fn a_governed_focus_out_that_cannot_be_counted_is_not_queued() {
    let focused = XServerFrontendClientId(354);
    let claimant = XServerFrontendClientId(355);
    let focused_surface = SurfaceId::new(354, 1);
    let claimant_surface = SurfaceId::new(355, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, held_channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, focused, focused_surface);
    let (claimant_registration, claimant_channels) = private
        .broker
        .registry
        .register_client(claimant)
        .expect("a second client");
    private
        .broker
        .registry
        .register_surface(
            claimant,
            NamespaceId::from_raw(252),
            claimant_surface,
            XResourceId::new(0x200253, 1),
        )
        .expect("a second surface");
    private
        .broker
        .registry
        .route_control(XAuthorityClientControlCommand {
            client: focused,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(53001),
                surface: focused_surface,
            },
        })
        .expect("the first focus");
    assert!(held_channels.control.try_recv().is_ok());
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client: claimant,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(53002),
                surface: claimant_surface,
            },
        })
        .expect("the shared admission to accept control");

    // Park the router after it has claimed and before it produces any effect.
    let focus_lock = Arc::clone(&private.broker.registry.focused_surface);
    let focused_guard = focus_lock.lock().unwrap();
    let routed = std::thread::spawn(move || {
        let outcome = private.route_pending();
        (private, outcome)
    });
    let parked = std::time::Instant::now() + std::time::Duration::from_millis(200);
    while std::time::Instant::now() < parked {
        std::thread::yield_now();
    }

    // The registry becomes unreadable while it is parked, so the effect it is
    // about to queue cannot be counted against the operation that causes it.
    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );
    drop(focused_guard);
    let (private, outcome) = routed.join().expect("the routing thread");

    // Refused before either effect. Queueing it untracked would make the work
    // look ungoverned, which is a real state for ordinary work and a false one
    // here, and the operation would then be settled while that effect could
    // still happen.
    assert!(
        matches!(
            outcome,
            Err(XServerFrontendRouteError::DependentNotTracked {
                refusal: crate::ControlDependentRefusal::Unavailable,
                ..
            })
        ),
        "authority unavailability is reported as what it is: {outcome:?}"
    );
    assert!(
        held_channels.control.try_recv().is_err(),
        "the previously focused client is not told it lost focus"
    );
    assert!(
        claimant_channels.control.try_recv().is_err(),
        "and the target is not queued either"
    );
    drop(claimant_registration);
    drop(private);
}

#[test]
fn a_published_outcome_is_not_sent_again_while_its_record_survives() {
    let client = XServerFrontendClientId(356);
    let surface = SurfaceId::new(356, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let command = configure(client, surface, 54001);
    let token = accepted(&registry, command);
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let queued = registry.track_dependent(token).expect("an applying record");
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Ok(ControlPublication::Delivered)
    );

    // The record survives only for the work it queued elsewhere. Its outcome
    // has gone out, so nothing reaches the emitter again -- not the same
    // acknowledgement, and not a different one.
    for outcome in [
        XAuthorityControlOutcome::Delivered,
        XAuthorityControlOutcome::AuthorityRejected,
    ] {
        for publication in [
            ControlPublication::Delivered,
            ControlPublication::Retained,
            ControlPublication::ReceiverGone,
        ] {
            assert_eq!(
                registry.publish_with(token, completion_ack(command, outcome), |_| {
                    panic!("a published outcome must not reach the emitter again")
                }),
                Err(crate::ControlPublicationRefusal::AlreadyPublished),
                "{outcome:?} as {publication:?}"
            );
        }
    }

    // And the retry path finds nothing owed: a retained one here would have
    // turned a published record back into one that still owes publication.
    assert_eq!(registry.owed(), Some(0));
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        0
    );
    assert_eq!(
        registry.state_of(token),
        crate::ControlRecordState::Outstanding
    );

    drop(queued);
    assert_eq!(registry.state_of(token), crate::ControlRecordState::Retired);
}

#[test]
fn a_retry_that_lands_settles_where_it_stands() {
    let client = XServerFrontendClientId(357);
    let surface = SurfaceId::new(357, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let command = |transaction| configure(client, surface, transaction);
    let with_work = accepted(&registry, command(55001));
    let without = accepted(&registry, command(55002));
    for token in [with_work, without] {
        assert_eq!(
            registry.claim_execution(token),
            crate::ControlExecutionClaim::Claimed
        );
    }
    let queued = registry
        .track_dependent(with_work)
        .expect("an applying record");
    for (token, transaction) in [(with_work, 55001), (without, 55002)] {
        assert_eq!(
            registry.publish_with(
                token,
                completion_ack(command(transaction), XAuthorityControlOutcome::Delivered),
                |_| ControlPublication::Retained,
            ),
            Ok(ControlPublication::Retained)
        );
    }
    assert_eq!(registry.owed(), Some(2));

    // One retry pass: the one with nothing outstanding goes, and the one with
    // queued work is settled in the same pass rather than revisited.
    assert_eq!(
        registry.publish_owed_with(|_| ControlPublication::Delivered),
        2
    );
    assert_eq!(registry.owed(), Some(0));
    assert_eq!(registry.state_of(without), crate::ControlRecordState::Retired);
    assert_eq!(
        registry.state_of(with_work),
        crate::ControlRecordState::Outstanding
    );

    drop(queued);
    assert_eq!(
        registry.state_of(with_work),
        crate::ControlRecordState::Retired
    );
}

#[test]
fn an_operation_that_finished_applying_reports_it_and_owes_nothing() {
    let client = XServerFrontendClientId(358);
    let surface = SurfaceId::new(358, 1);
    let (acknowledgements, acks) = sync_channel(1);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &durable, client, surface);
    let state = writer_runtime(surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // Fill the one acknowledgement slot so the writer applies and then fails
    // to publish, leaving the operation unanswered but fully applied.
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    private
        .control_producer()
        .submit(configure(client, surface, 56001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending().expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };
    let (writer, mut peer) = writer_start(
        Some(&private.broker.registry),
        &state,
        channels.control,
        Some(registry.clone()),
        acknowledgements,
        client,
        writer_windows(surface),
        Arc::new(AtomicUsize::new(0)),
    );
    writer_configure_notify(&mut peer, 80);
    assert!(!writer_join(writer), "a full channel fails this writer");

    // It reported both steps as it took them, and the writer going is what
    // makes the operation abandoned.
    assert_eq!(
        registry.steps_of(token),
        Some(crate::ControlSteps {
            runtime: crate::ControlStepState::Completed,
            projection: crate::ControlStepState::Completed
        })
    );
    // Its outcome is established even though the channel was full, so it is
    // not abandoned and is not a cleanup candidate: what it is waiting for is
    // publication, not a reconciliation.
    let reconciled = registry.reconcile_client(client);
    assert_eq!(reconciled.abandoned, 0);
    assert_eq!(reconciled.owed, 1);
    assert_eq!(private.reconcile_abandoned().discharged, 0);
    assert_eq!(private.reclaim_settled(), 0, "and its credit stays with it");

    // Draining lets the retained outcome out, and only then is it over.
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1)
    );
    assert_eq!(private.republish_owed_acknowledgements(), 1);
    assert_eq!(private.reclaim_settled(), 1);
    assert_eq!(private.reclaim_settled(), 0);
}

#[test]
fn an_operation_that_reported_finishing_is_still_not_proved_to_agree() {
    let client = XServerFrontendClientId(361);
    let surface = SurfaceId::new(361, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(configure(client, surface, 59001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending().expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };

    // It reported finishing both steps, and then its executor went without
    // establishing an outcome.
    for progress in [
        crate::ControlProgress::RuntimeBegun,
        crate::ControlProgress::RuntimeApplied,
        crate::ControlProgress::ProjectionBegun,
        crate::ControlProgress::ProjectionApplied,
    ] {
        registry
            .record_progress(token, progress)
            .expect("an applying record in order");
    }
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    // Both steps finished, and that is history rather than a statement about
    // now. The runtime guard was released before the projection was brought
    // into line and neither report carries a revision, so a projection that
    // agreed can have been overtaken; and the operation continues through
    // fallible work after it that these reports say nothing about.
    let report = private.reconcile_abandoned();
    assert!(report.readable);
    assert_eq!(report.discharged, 0);
    assert_eq!(report.retained_unproved, 1);
    assert_eq!(private.reclaim_settled(), 0, "so its credit stays held");
    assert!(acks.try_recv().is_err(), "with nothing answered for it");
}

#[test]
fn an_operation_whose_first_step_never_began_owes_nothing() {
    let client = XServerFrontendClientId(364);
    let surface = SurfaceId::new(364, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(configure(client, surface, 62001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);

    // It never began anything, and beginning is recorded before an effect can
    // happen, so this is evidence that nothing happened rather than an absence
    // of evidence.
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    let report = private.reconcile_abandoned();
    assert_eq!(report.discharged, 1);
    assert_eq!(report.retained_in_progress, 0);
    assert_eq!(
        private.reclaim_settled(),
        1,
        "and its credit is released once"
    );
    assert_eq!(private.reclaim_settled(), 0);
    assert!(acks.try_recv().is_err(), "with nothing answered for it");
}

#[test]
fn an_operation_interrupted_inside_a_step_is_not_one_that_never_began() {
    let client = XServerFrontendClientId(365);
    let surface = SurfaceId::new(365, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(configure(client, surface, 63001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending().expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };

    // Begun and never reported finished: the writer went between noting the
    // intent and the change succeeding. Reporting only after success cannot
    // tell this from an operation that never began, and that gap is where the
    // runtime ends up changed while the record says nothing happened.
    registry
        .record_progress(token, crate::ControlProgress::RuntimeBegun)
        .expect("an applying record");
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    let report = private.reconcile_abandoned();
    assert_eq!(report.discharged, 0, "the effect may have happened");
    assert_eq!(report.retained_in_progress, 1);
    assert_eq!(private.reclaim_settled(), 0, "so its credit stays held");
}

#[test]
fn an_operation_caught_between_its_steps_keeps_its_obligation() {
    let client = XServerFrontendClientId(359);
    let surface = SurfaceId::new(359, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(configure(client, surface, 57001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending().expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };

    // Caught between changing shared state and the state derived from it
    // catching up. This is what the writer reports having done at that point.
    for progress in [
        crate::ControlProgress::RuntimeBegun,
        crate::ControlProgress::RuntimeApplied,
    ] {
        registry
            .record_progress(token, progress)
            .expect("an applying record in order");
    }
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    // It changed something that outlives the connection and left the
    // projection of it behind. Nothing here can discharge that, so it keeps
    // the obligation and the credit.
    let report = private.reconcile_abandoned();
    assert_eq!(report.retained_half_applied, 1);
    assert_eq!(report.discharged, 0);
    assert_eq!(report.retained_in_progress, 0);
    assert_eq!(private.reclaim_settled(), 0);
    assert_eq!(
        registry.cleanups_owed().expect("a readable registry").len(),
        1,
        "and it is still owed one"
    );
    assert!(acks.try_recv().is_err(), "with nothing answered for it");
}

#[test]
fn a_kind_whose_steps_are_not_reported_is_retained_rather_than_discharged() {
    let client = XServerFrontendClientId(360);
    let surface = SurfaceId::new(360, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::CloseSurface {
                transaction: TransactionId::from_raw(58001),
                surface,
            },
        })
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    // Nothing reports what closing a surface did, and an absent report is not
    // a report of nothing. Discharging it would be reading silence as proof.
    let report = private.reconcile_abandoned();
    assert_eq!(report.retained_unproved, 1);
    assert_eq!(report.discharged, 0);
    assert_eq!(report.retained_half_applied, 0);
    assert_eq!(private.reclaim_settled(), 0, "so its credit stays held");
}

#[test]
fn an_unreadable_registry_settles_nothing_and_says_so() {
    let client = XServerFrontendClientId(362);
    let surface = SurfaceId::new(362, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(configure(client, surface, 60001))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    let poisoner = registry.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the registry");
        })
        .join()
        .is_err()
    );

    // Settling nothing because nothing could be looked at is not settling
    // nothing. A caller told the first would walk away from an operation that
    // is still owed something.
    let report = private.reconcile_abandoned();
    assert!(!report.readable);
    assert_eq!(report.discharged, 0);
    assert_eq!(private.reclaim_settled(), 0, "and no credit is released");
}

#[test]
fn only_an_operation_being_applied_can_report_progress() {
    let client = XServerFrontendClientId(363);
    let surface = SurfaceId::new(363, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");
    let command = configure(client, surface, 61001);

    // A reservation is its producer's and an accepted command has not
    // started, so neither has anything to report.
    let token = registry.register(command).expect("a fresh registry");
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::RuntimeBegun),
        Err(crate::ControlProgressRefusal::NotApplying)
    );
    registry.writer_started(client);
    registry
        .begin_acceptance(token)
        .expect("a fresh reservation")
        .commit();
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::RuntimeBegun),
        Err(crate::ControlProgressRefusal::NotApplying)
    );
    assert_eq!(registry.steps_of(token), Some(crate::ControlSteps::default()));

    // Applying is when there is something to report.
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    for progress in [
        crate::ControlProgress::RuntimeBegun,
        crate::ControlProgress::RuntimeApplied,
    ] {
        registry
            .record_progress(token, progress)
            .expect("an applying record in order");
    }
    assert_eq!(
        registry.steps_of(token),
        Some(crate::ControlSteps {
            runtime: crate::ControlStepState::Completed,
            projection: crate::ControlStepState::NotStarted
        })
    );

    // And once it is answered, nothing more is reported against it: a step
    // recorded after the outcome would describe work the outcome did not
    // cover.
    assert_eq!(
        registry.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Retained,
        ),
        Ok(ControlPublication::Retained)
    );
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::ProjectionBegun),
        Err(crate::ControlProgressRefusal::NotApplying)
    );
    assert_eq!(
        registry.steps_of(token),
        Some(crate::ControlSteps {
            runtime: crate::ControlStepState::Completed,
            projection: crate::ControlStepState::NotStarted
        })
    );
}

#[test]
fn progress_cannot_be_taken_back_or_skipped() {
    let client = XServerFrontendClientId(366);
    let surface = SurfaceId::new(366, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 64001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    let progressed = |steps: crate::ControlSteps| registry.steps_of(token) == Some(steps);

    // A step cannot finish before it begins, and a projection cannot be
    // claimed without the runtime change it projects. Both were reachable
    // when a caller could set the state directly, and either one lets an
    // operation report work nothing did.
    for out_of_order in [
        crate::ControlProgress::RuntimeApplied,
        crate::ControlProgress::ProjectionBegun,
        crate::ControlProgress::ProjectionApplied,
    ] {
        assert_eq!(
            registry.record_progress(token, out_of_order),
            Err(crate::ControlProgressRefusal::OutOfOrder),
            "{out_of_order:?} before what it depends on"
        );
    }
    assert!(progressed(crate::ControlSteps::default()));

    registry
        .record_progress(token, crate::ControlProgress::RuntimeBegun)
        .expect("the first step");
    // And it cannot begin twice, which would take the record backwards from
    // whatever the first beginning already established.
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::RuntimeBegun),
        Err(crate::ControlProgressRefusal::OutOfOrder)
    );
    assert!(progressed(crate::ControlSteps {
        runtime: crate::ControlStepState::InProgress,
        projection: crate::ControlStepState::NotStarted,
    }));

    registry
        .record_progress(token, crate::ControlProgress::RuntimeApplied)
        .expect("the runtime change");
    assert_eq!(
        registry.record_progress(token, crate::ControlProgress::ProjectionApplied),
        Err(crate::ControlProgressRefusal::OutOfOrder),
        "the projection cannot finish before it begins"
    );
    registry
        .record_progress(token, crate::ControlProgress::ProjectionBegun)
        .expect("the projection");
    registry
        .record_progress(token, crate::ControlProgress::ProjectionApplied)
        .expect("the projection finishing");
    assert!(progressed(crate::ControlSteps {
        runtime: crate::ControlStepState::Completed,
        projection: crate::ControlStepState::Completed,
    }));
}

#[test]
fn an_effect_whose_intent_cannot_be_recorded_does_not_happen() {
    let client = XServerFrontendClientId(367);
    let surface = SurfaceId::new(367, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(4).expect("an unused origin");
    let token = accepted(&registry, configure(client, surface, 65001));
    assert_eq!(
        registry.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );

    // A registration whose registry a writer cannot reach. Continuing would
    // produce an effect nobody noted the intent for, and afterwards nothing
    // could tell it from one that never happened -- which is exactly the
    // state a discharge is read from.
    let orphaned = X11ControlChannels::ClientBound {
        receiver: channel().1,
        acknowledgements: sync_channel(1).0,
        completion: None,
    };
    assert_eq!(
        orphaned.record_progress(Some(token), crate::ControlProgress::RuntimeBegun),
        Err(crate::ControlProgressRefusal::Unavailable),
        "so the caller is failed rather than allowed to continue"
    );

    // An operation no record governs is not gated by this at all.
    assert_eq!(
        orphaned.record_progress(None, crate::ControlProgress::RuntimeBegun),
        Ok(())
    );
}

/// A private instance whose accepted Configure never began a step, with its
/// writer gone: the state the never-started proof is about.
///
/// Composed rather than driven. Real accepted work is routed, but the writer
/// lifecycle is stated through the registry: no writer thread runs and no
/// surface-map failure produces it. An independent review reaches the same
/// state through an actual writer that exits before its first step, and
/// through an unwind injected after a native effect; those are its evidence
/// and not this. What these establish is the owner and phase composition
/// around that state.
#[cfg(unix)]
fn unstarted_after_its_writer_went(
    durable: &crate::PrivateSettlementOwner,
    acknowledgements: SyncSender<XAuthorityClientControlAck>,
    client: XServerFrontendClientId,
    surface: SurfaceId,
    transaction: u64,
) -> (
    crate::PrivateXServerFrontend,
    XServerFrontendClientRouteChannels,
    XServerFrontendClientRouteRegistration,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (mut private, channels, registration, deliveries) =
        private_with_client(acknowledgements, durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(configure(client, surface, transaction))
        .expect("the shared admission to accept control");
    assert_eq!(private.route_pending().expect("a turn").len(), 1);
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);
    (private, channels, registration, deliveries)
}

#[test]
fn the_never_started_proof_survives_shutdown_and_reaches_the_retained_handle() {
    let client = XServerFrontendClientId(368);
    let surface = SurfaceId::new(368, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, channels, _registration, _deliveries) =
        unstarted_after_its_writer_went(&durable, acknowledgements, client, surface, 66001);

    // Shut down without reconciling first. The frontend is consumed, so
    // nothing that only it could do will ever be done.
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    let mut report = private.shutdown();
    assert_eq!(durable.reserved().expect("a readable owner"), 1, "still owed until something settles it");

    // The retained handle applies the same proof.
    assert_eq!(report.reclaim_outstanding(), 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    // Driving again finds nothing left, rather than releasing twice.
    assert_eq!(report.reclaim_outstanding(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    assert!(
        acks.try_recv().is_err(),
        "and nothing was answered for it"
    );
    // The command it was routed with is still in the queue of the writer that
    // went, which is where it was left. Settling it queued nothing further:
    // nothing is replayed.
    assert!(channels.control.try_recv().is_ok());
    assert!(
        channels.control.try_recv().is_err(),
        "and there is only ever the one"
    );
}

#[test]
fn the_never_started_proof_reaches_the_durable_owner_when_the_handle_goes() {
    let client = XServerFrontendClientId(369);
    let surface = SurfaceId::new(369, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        unstarted_after_its_writer_went(&durable, acknowledgements, client, surface, 67001);

    // The only handle goes before anything drives it, so the work is now the
    // durable owner's and the proof has to reach it there.
    drop(private.shutdown());
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    assert_eq!(durable.outstanding().expect("a readable owner"), 1);

    assert!(durable.drive().made_progress());
    assert_eq!(durable.reserved().expect("a readable owner"), 0, "released exactly once");
    assert_eq!(durable.outstanding().expect("a readable owner"), 0);
    // Driven twice, and the second finds nothing.
    assert!(!durable.drive().made_progress());
    assert_eq!(durable.reserved().expect("a readable owner"), 0);
    assert!(acks.try_recv().is_err(), "with nothing answered for it");
}

#[test]
fn an_interrupted_operation_keeps_its_credit_through_the_same_transfers() {
    let client = XServerFrontendClientId(370);
    let surface = SurfaceId::new(370, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    private
        .control_producer()
        .submit(configure(client, surface, 68001))
        .expect("the shared admission to accept control");
    let ran = private.route_pending().expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };

    // Begun and never reported finished: the effect may have happened. Seeded
    // through the registry rather than by interrupting a writer inside its
    // effect, which is an independent review's control and not this one.
    registry
        .record_progress(token, crate::ControlProgress::RuntimeBegun)
        .expect("an applying record");
    registry.writer_started(client);
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);

    // Through every transfer the never-started one is released by, this one
    // keeps its credit, because nothing about it is established.
    let mut report = private.shutdown();
    assert_eq!(report.reclaim_outstanding(), 0);
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    drop(report);
    assert!(!durable.drive().made_progress());
    assert_eq!(durable.reserved().expect("a readable owner"), 1, "still owed after the durable owner too");
    assert!(!durable.drive().made_progress());
    assert_eq!(durable.reserved().expect("a readable owner"), 1);
    assert!(acks.try_recv().is_err());
}

#[test]
fn one_instances_reconciliation_does_not_reach_anothers_identical_identity() {
    let client = XServerFrontendClientId(371);
    let surface = SurfaceId::new(371, 1);
    let (acknowledgements, _acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();

    // Two instances, each issuing its own registrations from its own counter,
    // so their local identities collide.
    let (mine, _channels, _registration, _deliveries) =
        unstarted_after_its_writer_went(&durable, acknowledgements.clone(), client, surface, 69001);
    // The same client, surface and transaction as well as the same local
    // completion counter, so nothing but the origin distinguishes the two.
    let theirs_client = client;
    let theirs_surface = surface;
    let (mut theirs, _their_channels, _their_registration, _their_deliveries) =
        private_with_client(acknowledgements, &durable, theirs_client, theirs_surface);
    let their_registry = theirs
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");
    theirs
        .control_producer()
        .submit(configure(theirs_client, theirs_surface, 69001))
        .expect("the shared admission to accept control");
    let ran = theirs.route_pending().expect("a turn");
    let Some(crate::PrivateIdentity::Control {
        completion: Some(their_token),
        ..
    }) = ran.first().map(|run| run.identity)
    else {
        panic!("a control that names its registration");
    };
    their_registry
        .record_progress(their_token, crate::ControlProgress::RuntimeBegun)
        .expect("an applying record");
    their_registry.writer_started(theirs_client);
    their_registry.writer_stopped(theirs_client);
    assert_eq!(their_registry.reconcile_client(theirs_client).abandoned, 1);
    assert_eq!(durable.reserved().expect("a readable owner"), 2);

    // Settling mine reaches only mine. The other instance's operation has the
    // same local identity and a different origin, and it is the origin that
    // decides whose records these are.
    let mut report = mine.shutdown();
    assert_eq!(report.reclaim_outstanding(), 1);
    assert_eq!(
        durable.reserved().expect("a readable owner"),
        1,
        "the other instance's interrupted operation still owes its credit"
    );
    assert_eq!(
        their_registry.steps_of(their_token).map(|steps| steps.runtime),
        Some(crate::ControlStepState::InProgress),
        "and is untouched"
    );
    drop(theirs);
}

#[test]
fn reconciliation_settles_only_abandoned_operations_with_nothing_still_queued() {
    let client = XServerFrontendClientId(373);
    let surface = SurfaceId::new(373, 1);
    let registry = crate::ControlCompletionRegistry::with_capacity(8).expect("an unused origin");

    // Applying, with no step begun. Its executor is still there, so it is not
    // abandoned and nothing about it is being settled yet.
    let applying = accepted(&registry, configure(client, surface, 70001));
    assert_eq!(
        registry.claim_execution(applying),
        crate::ControlExecutionClaim::Claimed
    );
    assert_eq!(registry.reconcile_unstarted().discharged, 0);
    assert_eq!(
        registry.state_of(applying),
        crate::ControlRecordState::Outstanding
    );

    // Abandoned with no step begun, but holding work it queued elsewhere.
    // What it left is not established whatever its own steps say.
    let queued = registry.track_dependent(applying).expect("an applying record");
    registry.writer_stopped(client);
    assert_eq!(registry.reconcile_client(client).abandoned, 1);
    let report = registry.reconcile_unstarted();
    assert_eq!(report.discharged, 0);
    assert_eq!(report.retained_unproved, 1);
    assert_eq!(
        registry.state_of(applying),
        crate::ControlRecordState::Outstanding
    );

    // Only once nothing it started can still run.
    drop(queued);
    assert_eq!(registry.reconcile_unstarted().discharged, 1);
    assert_eq!(
        registry.state_of(applying),
        crate::ControlRecordState::Retired
    );
}

#[test]
fn a_poisoned_owner_still_takes_work_that_has_nowhere_else_to_go() {
    let client = XServerFrontendClientId(374);
    let surface = SurfaceId::new(374, 1);
    let (acknowledgements, acks) = sync_channel(8);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        unstarted_after_its_writer_went(&durable, acknowledgements, client, surface, 71001);
    assert_eq!(durable.reserved(), Some(1));

    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );

    // Unreadable is not empty. Answering zero here would tell a caller nothing
    // is owed by an owner that cannot say.
    assert_eq!(durable.reserved(), None);
    assert_eq!(durable.owed(), None);
    assert_eq!(durable.outstanding(), None);
    assert_eq!(durable.failed_instances(), None);

    // The handle goes, and its work moves to the owner. This move cannot
    // refuse: the credits were taken before the work was accepted, so the
    // space is already its own and declining would lose both. Doing nothing on
    // a poisoned lock was exactly that loss.
    drop(private.shutdown());
    let held = durable.records_even_if_poisoned();
    assert_eq!(held.outstanding.len(), 1, "the work was taken, not dropped");
    assert_eq!(held.reserved, 1, "and it still holds its credit");
    drop(held);
    assert!(acks.try_recv().is_err());
}

#[test]
fn a_poisoned_owner_still_releases_a_credit_that_is_answered() {
    let durable = crate::PrivateSettlementOwner::with_capacity(2);
    durable.reserve().expect("a fresh owner");
    assert_eq!(durable.reserved(), Some(1));

    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );

    // A release that does not happen is capacity lost for as long as this
    // owner lives.
    durable.release();
    assert_eq!(durable.records_even_if_poisoned().reserved, 0);

    // But taking one still refuses, because that is a refusal before
    // acceptance and the caller keeps what it has.
    assert!(matches!(
        durable.reserve(),
        Err(crate::AdmissionRefusal::Unavailable)
    ));
    assert!(matches!(
        durable.reserve_failure_slot(),
        Err(crate::AdmissionRefusal::Unavailable)
    ));
}

#[test]
fn a_poisoned_owner_reports_unavailable_rather_than_nothing_to_do() {
    let durable = crate::PrivateSettlementOwner::with_capacity(2);
    durable.reserve().expect("a fresh owner");

    // Readable and idle first, so the difference below is the poison and not
    // the emptiness.
    let idle = durable.drive();
    assert!(idle.readable);
    assert!(!idle.made_progress());
    assert_eq!(durable.recover_failed(), Some(0));

    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );

    // A drive that could not look is not a drive that found nothing, and
    // recovering nothing is not being unable to try. Only one of each says a
    // later attempt might do something.
    let blind = durable.drive();
    assert!(!blind.readable);
    assert!(!blind.made_progress());
    assert_eq!(durable.recover_failed(), None);
}

#[test]
fn a_poisoned_owner_still_takes_pending_work_from_a_dropping_handle() {
    let client = XServerFrontendClientId(375);
    let surface = SurfaceId::new(375, 1);
    // One slot, filled, so the instance cannot answer what it accepted and
    // the obligation is carried rather than discharged.
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    private
        .control_producer()
        .submit(configure(client, surface, 72001))
        .expect("the shared admission to accept control");

    let report = private.shutdown();
    assert_eq!(report.pending.len(), 1, "it could not be answered");

    // The owner becomes unreadable before the only handle goes.
    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );

    // Dropping is the handover, and a drop cannot keep what it is handing over
    // or report that it failed to. Declining here loses an accepted obligation
    // and the credit it holds.
    drop(report);
    let held = durable.records_even_if_poisoned();
    assert_eq!(held.held.len(), 1, "the obligation was taken, not dropped");
    assert!(
        matches!(
            held.held[0].1,
            PrivateOperation::Control(control, _)
                if control.command.transaction() == TransactionId::from_raw(72001)
        ),
        "and it is the command itself, with its own identity"
    );
    assert_eq!(held.reserved, 1, "still holding its credit");
    drop(held);

    // The channel is still full, so nothing was answered on the way.
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1)
    );
    assert!(acks.try_recv().is_err());
}

#[test]
fn a_sweep_that_unwinds_leaves_its_work_owned_and_returnable() {
    let client = XServerFrontendClientId(376);
    let surface = SurfaceId::new(376, 1);
    // One slot, filled, so the obligation is carried rather than answered.
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    private
        .control_producer()
        .submit(configure(client, surface, 73001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());
    assert_eq!(durable.owed(), Some(1));
    assert_eq!(durable.reserved(), Some(1));

    // A sweep interrupted after moving its work out and before settling it.
    // The obligation is in the owner's own in-flight list, which is where it
    // has to be: a local vector goes with the frame.
    {
        let mut held = durable.records_even_if_poisoned();
        let carried = std::mem::take(&mut held.held);
        held.in_flight.extend(carried);
    }
    assert_eq!(durable.owed(), Some(0), "not in the list it settles from");
    assert_eq!(durable.reserved(), Some(1), "and still holding its credit");

    // Returning it answers nothing. An obligation found in flight is not
    // evidence of what happened to it, only that a sweep did not finish.
    assert_eq!(durable.restore_interrupted(), 1);
    assert_eq!(durable.owed(), Some(1), "returned, not answered");
    assert_eq!(durable.reserved(), Some(1));
    assert!(
        acks.try_recv().is_ok(),
        "the slot still holds what was there before"
    );
    assert!(acks.try_recv().is_err(), "and nothing was published");

    // Twice does nothing the second time.
    assert_eq!(durable.restore_interrupted(), 0);
    assert_eq!(durable.owed(), Some(1));

    // And now it can be driven, answering exactly the original obligation
    // once, with its credit returned.
    let progress = durable.drive();
    assert!(progress.readable);
    assert_eq!(progress.answered, 1);
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(73001),
        "the exact obligation that was accepted"
    );
    assert!(acks.try_recv().is_err(), "and only once");
    assert_eq!(durable.reserved(), Some(0), "its credit returned");
    assert_eq!(durable.owed(), Some(0));
}

#[test]
fn restoring_reaches_through_the_poison_the_interruption_caused() {
    let client = XServerFrontendClientId(378);
    let surface = SurfaceId::new(378, 1);
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::with_capacity(2);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    private
        .control_producer()
        .submit(configure(client, surface, 75001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());

    // Staged as an interrupted sweep leaves it, then poisoned the way the
    // interruption itself poisons it: a sweep unwinds while holding this lock,
    // so the stranded work and the poison are one event.
    {
        let mut held = durable.records_even_if_poisoned();
        let AbandonedSettlements {
            held: settling,
            in_flight,
            ..
        } = &mut *held;
        in_flight.append(settling);
    }
    let poisoner = durable.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poisoning the owner");
        })
        .join()
        .is_err()
    );
    assert_eq!(durable.owed(), None, "an ordinary read still refuses");

    // A restore that declined here would decline in every case it exists for
    // and succeed only when there was nothing to do.
    assert_eq!(
        durable.restore_interrupted(),
        1,
        "the obligation comes back despite the poison that stranded it"
    );
    {
        let held = durable.records_even_if_poisoned();
        assert_eq!(held.held.len(), 1, "returned to the list a drive settles");
        assert!(held.in_flight.is_empty());
        assert_eq!(held.reserved, 1, "still holding its credit");
        assert!(held.indeterminate.is_empty(), "it never reached an attempt");
    }
    assert_eq!(durable.restore_interrupted(), 0, "and only once");
    assert!(acks.try_recv().is_ok(), "the slot is untouched");
    assert!(acks.try_recv().is_err(), "restoring published nothing");
}

#[test]
fn an_unreadable_completion_registry_is_not_permission_to_answer() {
    let client = XServerFrontendClientId(379);
    let surface = SurfaceId::new(379, 1);
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let completion = private
        .broker
        .registry
        .control_completion()
        .expect("a registry that issues completion records");
    private
        .control_producer()
        .submit(configure(client, surface, 76001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());
    assert_eq!(durable.owed(), Some(1));

    // Freed, so the channel is not what stops the next drive. The only thing
    // standing between this obligation and an acknowledgement is whether
    // anyone can show it is owed exactly one.
    assert!(acks.try_recv().is_ok());
    let _ = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _guard = completion.inner.lock().expect("the registry");
                panic!("poisoning the completion registry");
            })
            .join()
    });

    let progress = durable.drive();
    assert!(progress.readable, "the settlement owner is still readable");
    assert_eq!(
        progress.answered, 0,
        "a registry nobody can read cannot say this has one owner"
    );
    assert!(
        acks.try_recv().is_err(),
        "so nothing was published on the strength of an unreadable record"
    );
    assert_eq!(durable.owed(), Some(1), "kept whole");
    assert_eq!(durable.reserved(), Some(1), "and still holding its credit");
    assert_eq!(
        durable.indeterminate(),
        Some(0),
        "never attempted, so not unproved -- just unresolved"
    );
}

#[test]
fn an_attempt_interrupted_while_emitting_is_not_returned_as_retryable() {
    let client = XServerFrontendClientId(380);
    let surface = SurfaceId::new(380, 1);
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    private
        .control_producer()
        .submit(configure(client, surface, 77001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());

    // Staged as an unwind inside the emitting call leaves it: moved out of the
    // list a drive settles from, and marked as having reached the attempt.
    {
        let mut held = durable.records_even_if_poisoned();
        let AbandonedSettlements {
            held: settling,
            in_flight,
            ..
        } = &mut *held;
        in_flight.append(settling);
        held.settling = true;
    }

    assert_eq!(durable.restore_interrupted(), 1);
    assert_eq!(
        durable.indeterminate(),
        Some(1),
        "whether its acknowledgement went out is what the unwind destroyed"
    );
    assert_eq!(
        durable.owed(),
        Some(0),
        "so it is not put back where a drive would send it again"
    );
    assert_eq!(
        durable.reserved(),
        Some(1),
        "and its credit is not released, because nobody observed an outcome"
    );

    // Driving now must find nothing to do with it.
    let progress = durable.drive();
    assert_eq!(progress.answered, 0);
    assert!(acks.try_recv().is_ok(), "the slot still holds what was there");
    assert!(
        acks.try_recv().is_err(),
        "an unproved outcome is never published a second time"
    );
    assert_eq!(durable.indeterminate(), Some(1), "it stays what it is");
}

#[test]
fn an_obligation_a_live_record_still_answers_for_is_not_published_here() {
    let client = XServerFrontendClientId(381);
    let surface = SurfaceId::new(381, 1);
    let (acknowledgements, acks) = sync_channel(4);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let completion = private
        .broker
        .registry
        .control_completion()
        .expect("a registry that issues completion records");

    // A record that has begun applying cannot be handed over: a writer is
    // inside the command and will publish for it.
    let command = configure(client, surface, 78001);
    let token = accepted(&completion, command);
    assert_eq!(
        completion.claim_execution(token),
        crate::ControlExecutionClaim::Claimed
    );
    assert!(
        !completion.discard(token),
        "an applying record is not free to take"
    );

    // Staged carrying that token, which is the state this gate exists for: the
    // obligation is here, and so is someone else who can answer it.
    {
        let mut held = durable.records_even_if_poisoned();
        held.held.push((
            private.broker.registry.clone(),
            PrivateOperation::Control(command, Some(token)),
        ));
        held.reserved = held.reserved.saturating_add(1);
    }

    let progress = durable.drive();
    assert!(progress.readable);
    assert_eq!(progress.answered, 0, "not ours to answer");
    assert!(
        acks.try_recv().is_err(),
        "publishing here would be the second outcome for one operation"
    );
    assert_eq!(
        durable.owed(),
        Some(0),
        "and it is not kept as a command that could be sent again"
    );
    assert_eq!(
        durable.outstanding(),
        Some(1),
        "carried as an identity, so the credit is tracked without the payload"
    );
    assert_eq!(durable.reserved(), Some(1), "nothing observed an outcome yet");

    // Once the record's real owner answers, the credit is free -- released by
    // observing the registry rather than by anything here sending a receipt.
    assert_eq!(
        completion.publish_with(
            token,
            completion_ack(command, XAuthorityControlOutcome::Delivered),
            |_| ControlPublication::Delivered,
        ),
        Ok(ControlPublication::Delivered)
    );
    let progress = durable.drive();
    assert_eq!(progress.reclaimed, 1, "its record retired");
    assert_eq!(progress.answered, 0, "reclaiming is not answering");
    assert_eq!(durable.reserved(), Some(0));
    assert!(acks.try_recv().is_err(), "and still nothing published here");
}

#[test]
fn a_full_channel_is_congestion_and_the_obligation_survives_it() {
    let client = XServerFrontendClientId(382);
    let surface = SurfaceId::new(382, 1);
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    private
        .control_producer()
        .submit(configure(client, surface, 79001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());

    // Driven repeatedly against a full channel. Each attempt establishes
    // ownership and fails to send; none of them may conclude anything from
    // that, because a full channel with a live receiver is congestion.
    for attempt in 0..3 {
        let progress = durable.drive();
        assert!(progress.readable);
        assert_eq!(progress.answered, 0, "attempt {attempt} answered nothing");
        assert_eq!(durable.owed(), Some(1), "and kept it");
        assert_eq!(durable.reserved(), Some(1), "with its credit");
        assert_eq!(durable.indeterminate(), Some(0), "no attempt was interrupted");
    }

    // Drained, and the same obligation is answered once.
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(1)
    );
    let progress = durable.drive();
    assert_eq!(progress.answered, 1);
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(79001),
        "the exact obligation that was accepted"
    );
    assert_eq!(durable.owed(), Some(0));
    assert_eq!(durable.reserved(), Some(0), "its credit returned");

    // Repeat control: nothing is answered twice and no credit is released
    // twice, whether by driving again or by restoring.
    for _ in 0..3 {
        assert_eq!(durable.drive().answered, 0);
        assert_eq!(durable.restore_interrupted(), 0);
    }
    assert!(acks.try_recv().is_err(), "and only one acknowledgement went out");
    assert_eq!(durable.reserved(), Some(0));
}

#[test]
fn a_token_from_another_registry_is_not_permission_and_is_not_taken() {
    let client = XServerFrontendClientId(383);
    let surface = SurfaceId::new(383, 1);
    let (acknowledgements, acks) = sync_channel(4);
    let durable = crate::PrivateSettlementOwner::default();
    let (mine, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &durable, client, surface);
    let (theirs, _their_channels, _their_registration, _their_deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let their_completion = theirs
        .broker
        .registry
        .control_completion()
        .expect("a registry that issues completion records");

    // Same client, same surface, same transaction. Only the registry that
    // issued the registration differs, which is the whole identity here: a
    // command's fields are not unique to one operation.
    let command = configure(client, surface, 80001);
    let foreign = accepted(&their_completion, command);
    assert_eq!(their_completion.outstanding(), Some(1));

    {
        let mut held = durable.records_even_if_poisoned();
        held.held.push((
            mine.broker.registry.clone(),
            PrivateOperation::Control(command, Some(foreign)),
        ));
        held.reserved = held.reserved.saturating_add(1);
    }

    let progress = durable.drive();
    assert_eq!(
        progress.answered, 0,
        "one registry cannot answer for another's registration"
    );
    assert!(
        acks.try_recv().is_err(),
        "and must not publish on the strength of a record it did not issue"
    );
    assert_eq!(
        their_completion.outstanding(),
        Some(1),
        "nor take a record belonging to another registry"
    );
    assert_eq!(durable.owed(), Some(1), "kept whole");
    assert_eq!(durable.reserved(), Some(1), "with its credit");
    assert_eq!(durable.outstanding(), Some(0), "and not counted as routed");

    drop(mine);
    drop(theirs);
}

#[test]
fn a_dying_handle_parks_an_attempt_that_never_returned() {
    let client = XServerFrontendClientId(384);
    let surface = SurfaceId::new(384, 1);
    let (acknowledgements, acks) = sync_channel(4);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    private
        .control_producer()
        .submit(configure(client, surface, 81001))
        .expect("the shared admission to accept control");

    // The channel has room, so shutdown answers it and the handle carries
    // nothing. Refilled first so the obligation survives into the handle.
    let mut settlement = private.shutdown();
    assert_eq!(settlement.pending.len(), 0, "answered on the way out");
    let _ = acks.try_recv();

    // Staged as an attempt that did not return leaves a handle: the obligation
    // is still in the list and the marker says one of them reached the call
    // that emits.
    settlement.pending.push(PrivateOperation::Control(
        configure(client, surface, 81002),
        None,
    ));
    settlement.settling = Some(0);
    drop(settlement);

    assert_eq!(
        durable.indeterminate(),
        Some(1),
        "a dying handle hands over what it cannot account for"
    );
    assert_eq!(
        durable.owed(),
        Some(0),
        "and not as work something would send again"
    );
}

#[test]
fn a_transfer_guard_pays_out_when_the_attempt_unwinds() {
    let client = XServerFrontendClientId(385);
    let surface = SurfaceId::new(385, 1);
    let (acknowledgements, acks) = sync_channel(4);
    let durable = crate::PrivateSettlementOwner::default();
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    let origin = private.broker.registry.clone();
    drop(private.shutdown());
    let _ = acks.try_recv();

    // Two obligations: one the attempt is inside, one it has not reached. A
    // real unwind crosses the guard, which is the path that matters -- the
    // handle's fields are destroyed immediately afterwards, so anything not
    // transferred by then is gone.
    //
    // The panic is raised here rather than from inside `settle_one`: nothing
    // on that path panics of its own accord, so injecting one needs a
    // modified copy of the source. This exercises the guard's contract, which
    // is the mechanism that makes the injected case survivable.
    let mut pending = vec![
        PrivateOperation::Control(configure(client, surface, 82001), None),
        PrivateOperation::Control(configure(client, surface, 82002), None),
    ];
    let mut settling = None;
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let transfer = SettlementTransfer {
            origin: &origin,
            durable: &durable,
            pending: &mut pending,
            settling: &mut settling,
        };
        // As the marker stands when the emitting call is entered.
        *transfer.settling = Some(1);
        panic!("interrupting the attempt");
    }));
    assert!(unwound.is_err(), "the attempt unwound");

    assert_eq!(
        durable.indeterminate(),
        Some(1),
        "the one it was inside is unproved"
    );
    assert_eq!(
        durable.owed(),
        Some(1),
        "and the one it never reached is ordinary owed work"
    );
    assert!(
        acks.try_recv().is_err(),
        "an unwinding transfer publishes nothing"
    );
}

#[test]
fn an_emptied_failed_record_cannot_release_a_second_instances_slot() {
    let durable = crate::PrivateSettlementOwner::with_capacity(4);
    let (sender, _receiver) = sync_channel(4);
    let (authority, issuer, submit) = private_authority();
    let (delivery_sender, _delivery_receiver) = channel();
    // Kept alive for the whole test. Its failure slot was reserved before it
    // was exposed and it holds it for its life.
    let live = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender.clone(),
            input_deliveries: delivery_sender.clone(),
            authority,
            issuer,
            submit,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));

    let (failing_authority, failing_issuer, failing_submit) = private_authority();
    let failing = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority: failing_authority,
            issuer: failing_issuer,
            submit: failing_submit,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a second slot: {refusal:?}"));
    let failing_origin = failing.broker.registry.clone();
    let failing_queue = std::sync::Arc::clone(&failing.admission.ready);
    assert_eq!(
        durable.records_even_if_poisoned().failure_slots,
        2,
        "one slot each, reserved before either was exposed"
    );

    let admission = std::sync::Arc::clone(&failing.admission);
    let _ = std::thread::spawn(move || {
        let _guard = admission.ready.lock().expect("the queue");
        panic!("poisoning the shared queue");
    })
    .join();
    drop(failing.shutdown());
    assert_eq!(durable.failed_instances().expect("readable"), 1);

    assert_eq!(durable.recover_failed().expect("readable"), 0, "it had accepted nothing");
    assert_eq!(
        durable.records_even_if_poisoned().failure_slots,
        1,
        "the failed instance gave its slot back; the live one keeps its own"
    );

    // As an interrupted recovery leaves it: the record is back in the
    // inventory, already marked as having given its slot up. Restoring an
    // obligation must not restore a release that already happened.
    {
        let mut held = durable.records_even_if_poisoned();
        held.failed.push(FailedInstance {
            origin: failing_origin,
            queue: failing_queue,
            slot: FailureSlot::Released,
        });
    }
    assert_eq!(durable.recover_failed().expect("readable"), 0);
    assert_eq!(
        durable.records_even_if_poisoned().failure_slots,
        1,
        "the live instance still holds the slot it reserved"
    );
    drop(live);
}

#[test]
fn a_private_frontend_gates_the_authority_it_actually_owns() {
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    // Read before the parts are handed over; the instance is moved in.
    let owned = authority
        .authority_identity(&issuer)
        .expect("an authority to name itself");
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));

    // The property, not a spelling of the constructor: whatever the frontend
    // stamps and admits under is the same identity it executes against. A gate
    // built elsewhere and handed in could name another authority, and every
    // stamp this instance issued would then describe a coordinator driving
    // something else.
    assert_eq!(
        private.control_gate().authority(),
        owned,
        "the derived gate serves the authority this instance owns"
    );
    assert_eq!(
        private
            .authority()
            .identity()
            .expect("the held authority to name itself"),
        owned,
        "and the instance it kept is the one that was read"
    );

    // A separately built authority is a different identity, which is exactly
    // what pairing one with this gate would have hidden.
    let (other, other_issuer, _other_submit) = private_authority();
    assert_ne!(
        other
            .authority_identity(&other_issuer)
            .expect("a second authority to name itself"),
        owned,
        "two authorities are never one identity"
    );
}

/// The session generation these tests admit clients under.
const ROLE_SESSION_GENERATION: u64 = 5;

/// A client admitted the way the production lookup expects to find one.
fn admitted(client: XServerFrontendClientId) -> sophia_protocol::ClientAdmissionContext {
    sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(client.raw()),
        sophia_protocol::NamespaceContext::new(
            NamespaceId::from_raw(client.raw()),
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
    .unwrap()
}

/// The same admission, in a chosen namespace.
fn namespaced(
    client: XServerFrontendClientId,
    namespace: NamespaceId,
) -> sophia_protocol::ClientAdmissionContext {
    sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(client.raw()),
        sophia_protocol::NamespaceContext::new(
            namespace,
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
    .unwrap()
}

/// Admit a client the way the production boundary expects.
///
/// Both halves, because they are different things: the frontend registers the
/// client's routes, and the admission participant is where the producer of
/// admission and revocation binds it. Execution consults the second, so a test
/// that only did the first would be exercising a path that no longer decides
/// anything.
fn admit_role_client(
    private: &crate::PrivateXServerFrontend,
    client: XServerFrontendClientId,
) -> XServerFrontendClientRouteRegistration {
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .broker.registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("a fresh client to be admitted to the boundary");
    registration
}

fn private_for_roles() -> crate::PrivateXServerFrontend {
    let (sender, _receiver) = sync_channel(4);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"))
}

/// The connection identity the ordered path will read back for this client.
///
/// Built the same way on both sides on purpose: the recipient is the client
/// and the generation is the one Session admitted it under. A test that
/// invented either would be checking that two of its own constants match.
fn role_connection(recipient: u64) -> sophia_input_authority::ConnectionIdentity {
    sophia_input_authority::ConnectionIdentity {
        recipient,
        connection_generation: ROLE_SESSION_GENERATION,
    }
}

#[test]
fn one_producer_cannot_consume_another_producers_completion() {
    let private = private_for_roles();
    // Admitted, because execution reads the live registration table rather
    // than trusting what the request remembers.
    let _first_admitted = admit_role_client(&private, XServerFrontendClientId(501));
    let _second_admitted = admit_role_client(&private, XServerFrontendClientId(502));
    let first = private
        .reservation_role(XServerFrontendClientId(501), DeviceId::from_raw(1))
        .expect("a capability for the first producer");
    let second = private
        .reservation_role(XServerFrontendClientId(502), DeviceId::from_raw(2))
        .expect("a capability for the second producer");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let first_held = first.reserve(stamp, 1).expect("the first reservation");
    let second_held = second.reserve(stamp, 1).expect("the second reservation");
    let first_request = first_held.accepted();
    let second_request = second_held.accepted();

    // Both execute, so both have an outcome waiting. The connection handed to
    // execution is the evidence a caller read now, not the one custody
    // remembers -- here they agree, because nothing has revoked either.
    for (request, client) in [
        (&first_request, XServerFrontendClientId(501)),
        (&second_request, XServerFrontendClientId(502)),
    ] {
        assert!(
            private
                .execute_ordered(request, client, |permit, _bindings| permit.begin_external_effect())
                .is_ok(),
            "each producer's own request executes"
        );
    }

    // The right is carried, not named. There is no call that lets one producer
    // point at another's request: `observe` takes the token and the connection
    // from the custody value it is invoked on. A shared submit handle
    // establishes only which authority is being addressed, and the connection
    // test compares against whatever the caller passed in -- so an API that
    // accepted both from the caller let either producer consume the other's
    // outcome, and the one whose outcome was taken then saw a stale request.
    assert!(
        matches!(
            first_request.observe(),
            Ok(Some(sophia_input_authority::RequestCompletion::Processed))
        ),
        "the first producer observes its own outcome"
    );
    assert!(
        matches!(
            second_request.observe(),
            Ok(Some(sophia_input_authority::RequestCompletion::Processed))
        ),
        "and the second still has its own to observe"
    );
}

#[test]
fn a_controller_refuses_an_authority_paired_with_another_issuer() {
    let (authority, _issuer, _submit) = private_authority();
    let (_other, other_issuer, _other_submit) = private_authority();

    // Accepting this pairing lets a reservation be taken and then never
    // disposed: disposal is an issuer act, and this issuer answers for a
    // different instance, so the cell is stranded with nothing able to
    // publish, consume or reissue it.
    let refused = crate::PrivateAuthorityController::new(authority, other_issuer);
    let Err((refusal, authority, issuer)) = refused else {
        panic!("a mismatched authority and issuer must be refused");
    };
    assert!(matches!(
        refusal,
        crate::PrivateAuthorityRefusal::Authority(_)
    ));

    // The parts come back, so a caller can still build the right pairing.
    assert!(
        crate::PrivateAuthorityController::new(authority, issuer).is_err(),
        "the returned parts are the mismatched ones, unchanged"
    );
}

#[test]
fn a_reservation_dropped_under_common_is_disposed_rather_than_deadlocking() {
    let private = private_for_roles();
    let _admitted = admit_role_client(&private, XServerFrontendClientId(503));
    let _other_admitted = admit_role_client(&private, XServerFrontendClientId(504));
    let role = private
        .reservation_role(XServerFrontendClientId(503), DeviceId::from_raw(3))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let running = role.reserve(stamp, 1).expect("a reservation to execute");
    let request = running.accepted();

    // A second producer's reservation, taken OUTSIDE any execution, and never
    // published. Its own grant, because one grant holds one cell.
    let other = private
        .reservation_role(XServerFrontendClientId(504), DeviceId::from_raw(4))
        .expect("a capability for the second producer");
    let stranded = other.reserve(stamp, 1).expect("an unpublished reservation");

    // Moved in and dropped while this thread holds common. Taking common again
    // to dispose is a deadlock rather than a rank question, so the debt is
    // recorded and paid by the next caller that holds it.
    let completion = private.execute_ordered(&request, XServerFrontendClientId(503), move |permit, _bindings| {
            drop(stranded);
            permit.begin_external_effect()
        });
    assert!(
        completion.is_ok(),
        "the execution returned rather than hanging"
    );

    assert!(
        matches!(
            request.observe(),
            Ok(Some(sophia_input_authority::RequestCompletion::Processed))
        ),
        "the executed request still answers to its own custody"
    );
    // Deferred, not skipped: the stranded cell is free for the next request on
    // that grant.
    assert!(
        other.reserve(stamp, 2).is_ok(),
        "the dropped reservation's cell was released"
    );
}

#[test]
fn two_detached_producers_reserve_against_one_authority() {
    let mut private = private_for_roles();
    let _first_admitted = admit_role_client(&private, XServerFrontendClientId(511));
    let _second_admitted = admit_role_client(&private, XServerFrontendClientId(512));
    let first = private
        .ingress_for(XServerFrontendClientId(511), DeviceId::from_raw(1))
        .expect("an ingress for the first producer");
    let second = private
        .ingress_for(XServerFrontendClientId(512), DeviceId::from_raw(2))
        .expect("an ingress for the second producer");

    // Detached is the point: each is handed off and used on its own, and they
    // contend for one order against one authority.
    let first_sequence = first
        .submit(motion_to(
            SurfaceId::new(511, 1),
            XAuthorityInputDeliveryId::from_raw(511),
        ))
        .expect("the first producer's work to be accepted");
    let second_sequence = second
        .submit(motion_to(
            SurfaceId::new(512, 1),
            XAuthorityInputDeliveryId::from_raw(512),
        ))
        .expect("the second producer's work to be accepted");
    assert_ne!(
        first_sequence, second_sequence,
        "two producers take distinct places in one order"
    );

    // Each reserved against its own grant, so neither refused the other. A
    // shared grant would have made the second submission fail for want of a
    // completion cell, because a grant holds exactly one.
    //
    // That same bound is why a producer cannot get ahead of execution: its
    // first request still holds its cell until that request is executed and
    // its outcome consumed, so a second submission on the same grant is
    // refused rather than queued behind it. Recorded here as the property it
    // is -- a producer streaming input needs its requests executed, not just
    // accepted.
    let again = first.submit(motion_to(
        SurfaceId::new(511, 1),
        XAuthorityInputDeliveryId::from_raw(513),
    ));
    // Saturated, not denied: the grant's one cell is busy until the first
    // request's outcome is observed, and busy is worth retrying. Denial would
    // tell a producer to stop when it should wait.
    assert!(
        matches!(again, Err(PrivateSendError::Saturated(_))),
        "a second request on one grant is busy while the first holds its cell, got {again:?}"
    );

    // And the refusal did not disturb the other producer, which still has its
    // own grant and its own cell.
    assert!(
        matches!(
            second.submit(motion_to(
                SurfaceId::new(512, 1),
                XAuthorityInputDeliveryId::from_raw(514),
            )),
            Err(PrivateSendError::Saturated(_))
        ),
        "the same bound applies to each producer independently"
    );
}

#[test]
fn work_refused_by_the_order_takes_its_reservation_back() {
    let (sender, _receiver) = sync_channel(64);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(1).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &crate::PrivateSettlementOwner::default(),
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));

    // The order is filled from OTHER grants, one request each, so nothing here
    // is refused for want of a completion cell. Filling it from one producer
    // could not work: that producer's second request is refused at reservation
    // long before the order is full, which is a different refusal entirely and
    // proves nothing about the queue.
    let mut fillers = Vec::new();
    let mut filler_admissions = Vec::new();
    for index in 0..32u64 {
        filler_admissions.push(admit_role_client(
            &private,
            XServerFrontendClientId(600 + index),
        ));
        let filler = private
            .ingress_for(
                XServerFrontendClientId(600 + index),
                DeviceId::from_raw(index + 1),
            )
            .expect("a capability per filling producer");
        let outcome = filler.submit(motion_to(
            SurfaceId::new(600, 1),
            XAuthorityInputDeliveryId::from_raw(800 + index),
        ));
        match outcome {
            Ok(_) => fillers.push(filler),
            Err(PrivateSendError::Saturated(_)) => break,
            Err(other) => panic!("filling the order should saturate, got {other:?}"),
        }
    }
    assert!(!fillers.is_empty(), "the order accepted work before filling");

    // A fresh grant, so its own cell is free and the only thing that can
    // refuse this is the order itself.
    let _fresh_admitted = admit_role_client(&private, XServerFrontendClientId(699));
    let fresh = private
        .ingress_for(XServerFrontendClientId(699), DeviceId::from_raw(60))
        .expect("a capability for the fresh producer");
    let refused = fresh.submit(motion_to(
        SurfaceId::new(699, 1),
        XAuthorityInputDeliveryId::from_raw(899),
    ));
    let Err(PrivateSendError::Saturated(route)) = refused else {
        panic!("a full order must saturate a fresh grant's submission, got {refused:?}");
    };
    assert_eq!(
        route.delivery,
        Some(XAuthorityInputDeliveryId::from_raw(899)),
        "the refused work is handed back intact"
    );

    // Only that reservation went back. Everything the order accepted before it
    // filled is still there -- drained across as many turns as the service
    // budget needs, and counted, so a refusal that quietly consumed an earlier
    // item would show up as a short count rather than being invisible.
    let mut keyboards = private.keyboards().expect("this instance's state");
    let mut drained = 0usize;
    loop {
        let ran = private
            .route_pending_ordered(&mut keyboards, &control_watchdog())
            .expect("a readable order")
            .len();
        if ran == 0 {
            break;
        }
        drained += ran;
    }
    assert_eq!(
        drained,
        fillers.len(),
        "the order still held exactly the work it had accepted"
    );

    // The same grant and the same delivery id go through once the order has
    // room. Both halves matter: the cell was released, so the grant can
    // reserve again, and the delivery reservation was rolled back, so the id
    // is not still tracked as live. Either one left behind would refuse this.
    assert!(
        fresh
            .submit(motion_to(
                SurfaceId::new(699, 1),
                XAuthorityInputDeliveryId::from_raw(899),
            ))
            .is_ok(),
        "the refused submission released both its cell and its delivery id"
    );
}

#[test]
fn a_deferred_disposal_records_and_pays_through_a_poisoned_debt_list() {
    let private = private_for_roles();
    let _admitted = admit_role_client(&private, XServerFrontendClientId(521));
    let _other_admitted = admit_role_client(&private, XServerFrontendClientId(522));
    let running = private
        .reservation_role(XServerFrontendClientId(521), DeviceId::from_raw(1))
        .expect("a capability");
    let other = private
        .reservation_role(XServerFrontendClientId(522), DeviceId::from_raw(2))
        .expect("a second capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = running
        .reserve(stamp, 1)
        .expect("a reservation to execute")
        .accepted();
    let stranded = other.reserve(stamp, 1).expect("an unpublished reservation");

    // Poisoned before the debt is recorded. Declining to record here is the
    // difference between deferred and dropped, and dropped means a cell
    // nothing can publish, consume or reissue.
    let debts = std::sync::Arc::clone(&private.authority().owed_disposal);
    assert!(
        std::thread::spawn(move || {
            let _guard = debts.lock().unwrap();
            panic!("poisoning the debt list");
        })
        .join()
        .is_err()
    );

    let completion = private.execute_ordered(&request, XServerFrontendClientId(521), move |permit, _bindings| {
            drop(stranded);
            permit.begin_external_effect()
        });
    assert!(completion.is_ok(), "the execution returned");

    // Recorded through the poison, and paid by the next caller holding common.
    assert!(
        other.reserve(stamp, 2).is_ok(),
        "the stranded cell was released despite the poisoned debt list"
    );
}

#[test]
fn a_debt_already_recorded_is_paid_through_a_poisoned_list() {
    let private = private_for_roles();
    let _admitted = admit_role_client(&private, XServerFrontendClientId(523));
    let _other_admitted = admit_role_client(&private, XServerFrontendClientId(524));
    let running = private
        .reservation_role(XServerFrontendClientId(523), DeviceId::from_raw(1))
        .expect("a capability");
    let other = private
        .reservation_role(XServerFrontendClientId(524), DeviceId::from_raw(2))
        .expect("a second capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = running
        .reserve(stamp, 1)
        .expect("a reservation to execute")
        .accepted();
    let stranded = other.reserve(stamp, 1).expect("an unpublished reservation");

    // Recorded first, with the list healthy.
    let completion = private.execute_ordered(&request, XServerFrontendClientId(523), move |permit, _bindings| {
            drop(stranded);
            permit.begin_external_effect()
        });
    assert!(completion.is_ok());

    // Poisoned only now, before anything has paid it. A payment that skips a
    // poisoned list leaves a debt recorded and never settled, which reads as
    // deferred and behaves as dropped.
    let debts = std::sync::Arc::clone(&private.authority().owed_disposal);
    assert!(
        std::thread::spawn(move || {
            let _guard = debts.lock().unwrap();
            panic!("poisoning the debt list");
        })
        .join()
        .is_err()
    );

    assert!(
        other.reserve(stamp, 2).is_ok(),
        "the recorded debt was paid despite the poisoned list"
    );
}

#[test]
fn recording_a_disposal_debt_does_not_allocate() {
    let private = private_for_roles();
    let _admitted = admit_role_client(&private, XServerFrontendClientId(525));
    let _other_admitted = admit_role_client(&private, XServerFrontendClientId(526));
    let reserved = private
        .authority()
        .owed_disposal
        .lock()
        .expect("a fresh debt list")
        .capacity();
    assert!(
        reserved >= 1,
        "storage for a debt is taken at construction, not when one is owed"
    );

    let running = private
        .reservation_role(XServerFrontendClientId(525), DeviceId::from_raw(1))
        .expect("a capability");
    let other = private
        .reservation_role(XServerFrontendClientId(526), DeviceId::from_raw(2))
        .expect("a second capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = running
        .reserve(stamp, 1)
        .expect("a reservation")
        .accepted();
    let stranded = other.reserve(stamp, 1).expect("an unpublished reservation");
    let _ = private.execute_ordered(&request, XServerFrontendClientId(525), move |permit, _bindings| {
            drop(stranded);
            permit.begin_external_effect()
        });

    // A debt is recorded while common is held, on a path where something has
    // already failed. Growing the list there is an allocation at the worst
    // available moment.
    assert_eq!(
        private
            .authority()
            .owed_disposal
            .lock()
            .expect("the debt list")
            .capacity(),
        reserved,
        "recording a debt used storage that was already reserved"
    );
}

#[test]
fn nothing_can_be_issued_for_a_client_the_boundary_never_admitted() {
    let private = private_for_roles();
    // Registered with the frontend, deliberately not admitted to the boundary.
    // The old check would have found this client and called it current.
    let (_registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(
            XServerFrontendClientId(531),
            Some(admitted(XServerFrontendClientId(531))),
        )
        .expect("a fresh client to register");

    let refused = private.reservation_role(XServerFrontendClientId(531), DeviceId::from_raw(1));
    assert!(
        matches!(refused, Err(crate::PrivateAdmissionRefusal::NotAdmitted)),
        "a capability issued here would be a grant revocation could never find"
    );

    // Admitted through the producer hook, and now it issues.
    private
        .admission_participant()
        .admit(
            XServerFrontendClientId(531),
            admitted(XServerFrontendClientId(531)),
        )
        .expect("the boundary to admit");
    assert!(
        private
            .reservation_role(XServerFrontendClientId(531), DeviceId::from_raw(1))
            .is_ok(),
        "and once admitted the role issues"
    );
}

#[test]
fn a_revoked_admission_stops_a_later_execution() {
    let private = private_for_roles();
    let _registration = admit_role_client(&private, XServerFrontendClientId(561));
    let role = private
        .reservation_role(XServerFrontendClientId(561), DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    // Execute-wins: reserved and executed before anything revokes.
    let first = role.reserve(stamp, 1).expect("a reservation").accepted();
    assert!(
        private
            .execute_ordered(&first, XServerFrontendClientId(561), |permit, _bindings| permit
                .begin_external_effect())
            .is_ok(),
        "work that reaches execution before revocation applies"
    );
    assert!(matches!(
        first.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));

    // A second request, reserved while still admitted.
    let second = role.reserve(stamp, 2).expect("a second reservation").accepted();

    // Revoke-wins: once the producer has revoked, a later attempt cannot
    // apply, even though the request was reserved while the client was live.
    let retired = private
        .admission_participant()
        .revoke_admission(
            XServerFrontendClientId(561),
            sophia_protocol::ClientAdmissionId::from_raw(561),
        )
        .expect("the boundary to revoke");
    assert_eq!(
        retired,
        crate::PrivateRevocation {
            closed: 1,
            retired: 1
        },
        "the binding closed and the grant it authorised was retired"
    );

    let refused = private.execute_ordered(&second, XServerFrontendClientId(561), |_permit, _bindings| {
        panic!("a revoked admission must not reach the permit");
    });
    assert!(
        matches!(
            refused,
            Err(crate::PrivateAuthorityRefusal::NoCurrentAdmission)
        ),
        "a revoked client cannot execute, got {refused:?}"
    );

    // Readmitting does not revive it. A replacement admission is a different
    // admission, whatever the generation says.
    lifecycle_drain(&private.terminal.lifecycle);
    private
        .admission_participant()
        .admit(
            XServerFrontendClientId(561),
            sophia_protocol::ClientAdmissionContext::new(
                sophia_protocol::ClientAdmissionId::from_raw(9561),
                sophia_protocol::NamespaceContext::new(
                    NamespaceId::from_raw(561),
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
            .unwrap(),
        )
        .expect("a replacement admission");
    let still_refused =
        private.execute_ordered(&second, XServerFrontendClientId(561), |_permit, _bindings| {
            panic!("an old grant must not become current under a new admission");
        });
    assert!(
        matches!(
            still_refused,
            Err(crate::PrivateAuthorityRefusal::NoCurrentAdmission)
        ),
        "a replacement admission does not make an old grant current, got {still_refused:?}"
    );
}

#[test]
fn a_namespace_closes_every_binding_in_it_whatever_it_holds() {
    let private = private_for_roles();
    let namespace = NamespaceId::from_raw(571);

    // Three shapes in one namespace: one that never issued a grant, one whose
    // grant is already retired, and one still holding a live grant. Closing is
    // what denies further work, so having nothing left to clean up is not a
    // reason to leave a namespace admitted.
    let bare = XServerFrontendClientId(571);
    let spent = XServerFrontendClientId(572);
    let live = XServerFrontendClientId(573);
    for client in [bare, spent, live] {
        private
            .admission_participant()
            .admit(client, namespaced(client, namespace))
            .expect("the boundary to admit");
    }
    let _spent_role = private
        .reservation_role(spent, DeviceId::from_raw(1))
        .expect("a capability");
    let _live_role = private
        .reservation_role(live, DeviceId::from_raw(2))
        .expect("a capability");

    // Retire one admission on its own first, so its binding is gone and the
    // namespace sweep meets a client with nothing left.
    let first = private
        .admission_participant()
        .revoke_admission(spent, sophia_protocol::ClientAdmissionId::from_raw(spent.raw()))
        .expect("the boundary to revoke");
    assert_eq!(
        first,
        crate::PrivateRevocation {
            closed: 1,
            retired: 1
        }
    );

    let swept = private
        .admission_participant()
        .revoke_namespace(namespace)
        .expect("the boundary to revoke the namespace");
    assert_eq!(
        swept,
        crate::PrivateRevocation {
            closed: 2,
            retired: 1
        },
        "both remaining bindings closed; only the live grant had anything to retire"
    );

    // A zero retired count is not evidence that nothing closed.
    for client in [bare, live] {
        assert!(
            matches!(
                private.reservation_role(client, DeviceId::from_raw(9)),
                Err(crate::PrivateAdmissionRefusal::NotAdmitted)
            ),
            "every binding in the namespace is closed"
        );
    }
}

#[test]
fn revoking_a_namespace_with_nothing_to_retire_still_closes_it() {
    let private = private_for_roles();
    let namespace = NamespaceId::from_raw(581);
    let client = XServerFrontendClientId(581);
    private
        .admission_participant()
        .admit(client, namespaced(client, namespace))
        .expect("the boundary to admit");

    let swept = private
        .admission_participant()
        .revoke_namespace(namespace)
        .expect("the boundary to revoke the namespace");
    assert_eq!(
        swept,
        crate::PrivateRevocation {
            closed: 1,
            retired: 0
        },
        "closed with nothing to retire, which is not the same as nothing closed"
    );
    assert!(matches!(
        private.reservation_role(client, DeviceId::from_raw(1)),
        Err(crate::PrivateAdmissionRefusal::NotAdmitted)
    ));
}

#[test]
fn a_boundary_nobody_can_read_is_not_a_client_nobody_admitted() {
    let private = private_for_roles();
    let _registration = admit_role_client(&private, XServerFrontendClientId(591));
    let role = private
        .reservation_role(XServerFrontendClientId(591), DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = role.reserve(stamp, 1).expect("a reservation").accepted();

    let bindings = std::sync::Arc::clone(&private.admission_participant().bindings);
    assert!(
        std::thread::spawn(move || {
            let _guard = bindings.lock().unwrap();
            panic!("poisoning the boundary");
        })
        .join()
        .is_err()
    );

    let outcome = private.execute_ordered(&request, XServerFrontendClientId(591), |_permit, _bindings| {
        panic!("an unreadable boundary must not reach the permit");
    });
    assert!(
        matches!(outcome, Err(crate::PrivateAuthorityRefusal::Unreachable)),
        "unreadable is not absent: nothing was established about who is admitted, got {outcome:?}"
    );
}

#[test]
fn a_binding_refuses_a_grant_it_could_not_account_for() {
    let private = private_for_roles();
    let client = XServerFrontendClientId(601);
    private
        .admission_participant()
        .admit(client, admitted(client))
        .expect("the boundary to admit");

    // Issued until the binding is full. The bound is the authority's own
    // supported grant count, not a number chosen here, so this is the point
    // where the authority itself would stop being able to answer for them.
    let mut issued = 0usize;
    loop {
        match private.reservation_role(client, DeviceId::from_raw(1)) {
            Ok(role) => {
                std::mem::forget(role);
                issued += 1;
            }
            Err(refusal) => {
                assert!(
                    matches!(refusal, crate::PrivateAdmissionRefusal::GrantRecordsExhausted),
                    "the binding refuses before issuing, got {refusal:?}"
                );
                break;
            }
        }
        assert!(issued <= 64, "the binding must refuse rather than grow");
    }
    assert_eq!(
        issued,
        sophia_input_authority::Capacity::PLANNED.grants,
        "the bound is the authority's supported grant count"
    );

    // Refused before the grant existed, so the binding still accounts for
    // exactly what it authorised and revocation can retire all of it.
    let revoked = private
        .admission_participant()
        .revoke_admission(client, sophia_protocol::ClientAdmissionId::from_raw(client.raw()))
        .expect("the boundary to revoke");
    assert_eq!(
        revoked.closed, 1,
        "the binding closed"
    );
    assert_eq!(
        revoked.retired, issued,
        "every grant it recorded was retired, and it recorded every grant it authorised"
    );
}

#[test]
fn cleanup_left_unresolved_is_resumed_rather_than_revisited_by_revocation() {
    let private = private_for_roles();
    let namespace = NamespaceId::from_raw(611);
    let client = XServerFrontendClientId(611);
    private
        .admission_participant()
        .admit(client, namespaced(client, namespace))
        .expect("the boundary to admit");
    let _role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");

    // Staged as an interrupted revocation leaves it: closed, so further work
    // is already denied, with its grant still recorded because retirement did
    // not finish.
    {
        let mut bindings = private
            .admission_participant()
            .bindings
            .lock()
            .expect("the boundary");
        let bound = bindings.bound.get_mut(&client).expect("the binding");
        bound.closed = true;
        assert_eq!(bound.grants.len(), 1, "its grant is still owed retirement");
    }

    // Revoking the namespace again does not finish it. Closing is not what
    // this binding is waiting for, and it is already closed.
    let swept = private
        .admission_participant()
        .revoke_namespace(namespace)
        .expect("the boundary to revoke the namespace");
    assert_eq!(
        swept,
        crate::PrivateRevocation {
            closed: 0,
            retired: 0
        },
        "a second revocation passes over work that is already closed"
    );
    assert_eq!(
        private
            .admission_participant()
            .bindings
            .lock()
            .expect("the boundary")
            .bound
            .get(&client)
            .map_or(0, |bound| bound.grants.len()),
        1,
        "so the outstanding retirement is still outstanding"
    );

    // The origin's continuation is what finishes it.
    let resumed = private
        .admission_participant()
        .resume_unresolved()
        .expect("the boundary");
    assert_eq!(resumed.retired, 1, "the owed retirement was completed");
    assert!(
        !private
            .admission_participant()
            .bindings
            .lock()
            .expect("the boundary")
            .bound
            .contains_key(&client),
        "and with nothing left owed against it the record goes"
    );

    // Nothing left to resume, and it says so rather than looping.
    assert_eq!(
        private
            .admission_participant()
            .resume_unresolved()
            .expect("the boundary"),
        crate::PrivateRevocation::default()
    );
}

#[test]
fn keyboard_state_is_applied_on_this_thread_with_both_modifier_facts() {
    let private = private_for_roles();
    let mut keyboards = private.keyboards().expect("a keymap that compiles");
    let seat = SeatId::from_raw(1);

    // Applying before preparing does not build anything. Building compiles a
    // keymap, which is neither free nor infallible, and doing it inside a
    // transaction would put that failure where refusing is no longer free.
    assert!(
        keyboards.apply(seat, 42, true).is_none(),
        "an unprepared seat applies nothing"
    );
    assert!(keyboards.prepare(seat));
    assert!(keyboards.prepare(seat), "preparing twice is not an error");

    // Left shift down, then a key while it is held. The event carries the
    // modifiers from *before* it, while what follows has to see the state it
    // produced -- two different facts, which is why both are returned.
    let (_shift_code, before_shift, after_shift) =
        keyboards.apply(seat, 42, true).expect("left shift to map");
    assert_eq!(before_shift, 0, "nothing was held before the first key");
    assert_ne!(
        after_shift, 0,
        "and the state the key produced is not the state it was reported with"
    );
    assert_eq!(
        keyboards.modifiers(seat),
        Some(after_shift),
        "the seat keeps what the key left"
    );

    let (_code, before_key, _after_key) =
        keyboards.apply(seat, 30, true).expect("a letter to map");
    assert_eq!(
        before_key, after_shift,
        "the next event reports the modifiers that were held when it happened"
    );

    // Released, and the seat follows. No worker, no channel, no deadline: this
    // is state this thread owns, so nothing here waits.
    keyboards.apply(seat, 42, false).expect("left shift release");
    assert_eq!(
        keyboards.modifiers(seat),
        Some(0),
        "releasing the modifier clears it"
    );

    // A second seat is independent, and is built the same way as the first.
    let other = SeatId::from_raw(2);
    assert!(keyboards.prepare(other));
    assert_eq!(keyboards.modifiers(other), Some(0));
    assert_eq!(
        keyboards.modifiers(seat),
        Some(0),
        "seats do not share state"
    );
}

#[test]
fn keyboard_state_answers_for_one_instance_only() {
    let first = private_for_roles();
    let second = private_for_roles();
    let mut keyboards = first.keyboards().expect("a keymap that compiles");

    let first_identity = first.authority().identity().expect("an identity");
    let second_identity = second.authority().identity().expect("an identity");
    assert_ne!(
        first_identity, second_identity,
        "two instances are two identities"
    );

    // Being owned by this thread says nothing about whose state it is. Without
    // the binding, one instance's turn could be driven with the other's
    // keyboard history and every modifier would be read from the wrong past.
    assert!(keyboards.answers_for(first_identity));
    assert!(
        !keyboards.answers_for(second_identity),
        "another instance's state cannot be substituted for this one's"
    );

    // And the state it holds is this instance's, not a fresh one: a seat
    // already prepared keeps what it is holding rather than starting again.
    let seat = SeatId::from_raw(1);
    assert!(keyboards.prepare(seat));
    keyboards.apply(seat, 42, true).expect("left shift to map");
    let held = keyboards.modifiers(seat).expect("the seat");
    assert_ne!(held, 0, "a modifier is held");
    assert!(keyboards.prepare(seat), "preparing again is not rebuilding");
    assert_eq!(
        keyboards.modifiers(seat),
        Some(held),
        "so a key held across it is still held by the same state"
    );
}

#[test]
fn an_instance_hands_out_its_keyboard_history_once() {
    let private = private_for_roles();
    let mut keyboards = private.keyboards().expect("the instance's state");
    let seat = SeatId::from_raw(1);
    assert!(keyboards.prepare(seat));
    keyboards.apply(seat, 42, true).expect("left shift to map");
    let held = keyboards.modifiers(seat).expect("the seat");
    assert_ne!(held, 0, "a modifier is held in the state that exists");

    // A second object would carry this instance's identity and pass every
    // check that identity answers, while holding none of what the first is
    // holding. The shift down above would be a key nobody released as far as
    // it could tell, so there is no second one to be given.
    let second = private.keyboards();
    assert!(
        matches!(second, Err(crate::PrivateKeyboardsRefusal::AlreadyIssued)),
        "one history per instance, got {second:?}"
    );

    // The one that exists still holds it.
    assert_eq!(keyboards.modifiers(seat), Some(held));
}

#[test]
fn an_unreadable_authority_is_not_reported_as_a_broken_keymap() {
    let private = private_for_roles();
    let poisoner = private.authority().clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.common.lock().unwrap();
            panic!("poisoning the authority");
        })
        .join()
        .is_err()
    );

    let refused = private.keyboards();
    assert!(
        matches!(
            refused,
            Err(crate::PrivateKeyboardsRefusal::AuthorityUnreadable)
        ),
        "an authority nobody can read is not a keymap that will not compile, got {refused:?}"
    );
}

#[test]
fn losing_the_handle_for_executed_work_does_not_erase_its_outcome() {
    let private = private_for_roles();
    let _admitted = admit_role_client(&private, XServerFrontendClientId(541));
    let role = private
        .reservation_role(XServerFrontendClientId(541), DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = role.reserve(stamp, 1).expect("a reservation").accepted();
    let token = request.token();

    assert!(
        private
            .execute_ordered(&request, XServerFrontendClientId(541), |permit, _bindings| permit
                .begin_external_effect())
            .is_ok()
    );

    // The handle goes without the outcome being taken. Reclaiming the cell
    // here would erase what happened: a later observation would report a stale
    // request rather than the outcome that really occurred, and capacity would
    // have been bought by destroying evidence.
    drop(request);

    let surviving = private
        .authority()
        .under_common_as_origin(|authority, _issuer| {
            // Reached through the private field rather than a production
            // accessor: taking an outcome without holding its custody is
            // exactly what production must not offer, so it does not get a
            // method for the sake of a test.
            authority.take_completion(&private.submit, token, role_connection(541))
        })
        .expect("a readable authority");
    assert!(
        matches!(
            surviving,
            Ok(Some(sophia_input_authority::RequestCompletion::Processed))
        ),
        "the terminal outcome survived the handle, got {surviving:?}"
    );
}

#[test]
fn an_interrupted_execution_is_not_mistaken_for_one_that_never_ran() {
    let private = private_for_roles();
    let _admitted = admit_role_client(&private, XServerFrontendClientId(551));
    let role = private
        .reservation_role(XServerFrontendClientId(551), DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let request = role.reserve(stamp, 1).expect("a reservation").accepted();
    let token = request.token();

    // Marks an effect, then does not return. Whether that effect happened is
    // exactly what the interruption destroyed.
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        private.execute_ordered(&request, XServerFrontendClientId(551), |permit, _bindings| {
            permit.begin_external_effect()?;
            panic!("interrupting execution after its effect was marked");
        })
    }));
    assert!(interrupted.is_err(), "the execution unwound");

    // The handle goes without an outcome ever being recorded. Treating that as
    // a request that never ran would discard the cell, and discarding it turns
    // "nobody can say whether this ran" into "this never happened".
    drop(request);

    // Read through the poison the interruption caused, because that poison and
    // the interruption are one event.
    let mut authority = private
        .authority()
        .common
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let surviving = authority.take_completion(&private.submit, token, role_connection(551));
    assert!(
        matches!(surviving, Ok(None)),
        "the record survives with no outcome -- unknown, not absent -- got {surviving:?}"
    );
}

#[test]
fn a_sweep_leaves_its_inventory_the_buffer_it_reserved() {
    let client = XServerFrontendClientId(377);
    let surface = SurfaceId::new(377, 1);
    // Full, so shutdown carries the obligation here instead of answering it.
    let (acknowledgements, acks) = sync_channel(1);
    acknowledgements
        .try_send(completion_ack(
            configure(client, surface, 1),
            XAuthorityControlOutcome::Delivered,
        ))
        .expect("the empty slot");
    let durable = crate::PrivateSettlementOwner::with_capacity(8);
    let (private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &durable, client, surface);
    private
        .control_producer()
        .submit(configure(client, surface, 74001))
        .expect("the shared admission to accept control");
    drop(private.shutdown());
    assert_eq!(durable.owed(), Some(1));

    // Freed, so the sweep can finish and the lists are the ones a completed
    // sweep left behind rather than ones it never emptied.
    assert!(acks.try_recv().is_ok());
    let progress = durable.drive();
    assert_eq!(progress.answered, 1, "swept and answered");
    assert_eq!(
        acks.try_recv().unwrap().acknowledgement.transaction,
        TransactionId::from_raw(74001)
    );

    // The buffers were taken at construction so that settling work a failing
    // instance handed over never has to allocate. A sweep that moves its
    // inventory out through a local swaps in a fresh vector and drops the
    // buffer with it, so the next sweep allocates during exactly the teardown
    // the reservation was for. Moving between two owned lists keeps it.
    let held = durable.records_even_if_poisoned();
    assert!(
        held.held.capacity() >= 8,
        "the list swept from kept its reserved buffer, had {}",
        held.held.capacity()
    );
    assert!(
        held.outstanding.capacity() >= 8,
        "and so did the list of routed work, had {}",
        held.outstanding.capacity()
    );
    assert!(held.in_flight.is_empty(), "a finished sweep carries nothing");
    assert!(
        held.in_flight.capacity() >= 8,
        "and the list it carries in keeps its own buffer too, had {}",
        held.in_flight.capacity()
    );
}

/// The completion cell an admission minted, taken at the admission boundary.
///
/// RETAINED THERE, NOT LOOKED UP LATER. A delivery id is a reusable number and
/// its ticket is pruned once answered, so asking the recovery for the cell at
/// assertion time can hand back a different admission's cell, or none -- and
/// none is not evidence that nothing was handed over. Holding the cell from
/// the start is what fixes which admission a control is talking about.
fn admitted_cell(
    private: &crate::PrivateXServerFrontend,
    delivery: u64,
) -> Arc<PrivateDeliveryCompletion> {
    private
        .broker
        .registry
        .input_recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(delivery))
        .expect("a readable recovery")
        .expect("this admission minted a cell")
}

/// Capsules an instrument took off a recipient's ordered queue.
///
/// FIXTURE-OWNED, because reading a queue consumes it. Anything taken while
/// looking for one admission's capsule is kept here rather than dropped: a
/// control asking about one event must not destroy another event's evidence,
/// and the capsules it did not ask for are still owed to whoever does.
#[derive(Default)]
struct OrderedInbox {
    taken: Vec<XAuthorityOrderedDelivery>,
}

impl OrderedInbox {
    fn collect(&mut self, queue: &Receiver<XAuthorityOrderedDelivery>) {
        self.taken.extend(queue.try_iter());
    }

    fn take_carrying(
        &mut self,
        expected: &Arc<PrivateDeliveryCompletion>,
    ) -> Option<XAuthorityOrderedDelivery> {
        let found = self.taken.iter().position(|capsule| {
            capsule
                .finalizer()
                .is_some_and(|finalizer| Arc::ptr_eq(&finalizer.completion, expected))
        })?;
        Some(self.taken.remove(found))
    }

    /// The capsule this exact admission's recipient accepted, if it has one.
    ///
    /// WHAT IS ALREADY QUEUED COUNTS, and is looked at before any visit is
    /// driven: a handover from an earlier call is still a handover, and asking
    /// only for new ones reported nothing for an event the recipient had.
    ///
    /// Identity is the retained cell the capsule carries -- the one thing that
    /// names this admission and nothing else. A driver failure is propagated
    /// rather than answered as "no handover", because it establishes neither.
    fn accepted(
        &mut self,
        private: &mut crate::PrivateXServerFrontend,
        queue: &Receiver<XAuthorityOrderedDelivery>,
        expected: &Arc<PrivateDeliveryCompletion>,
        steps: usize,
    ) -> Result<Option<XAuthorityOrderedDelivery>, XServerFrontendRouteError> {
        for _ in 0..steps {
            self.collect(queue);
            if let Some(found) = self.take_carrying(expected) {
                return Ok(Some(found));
            }
            if matches!(
                private.deliver_one(&mut |_, _| Ok(()))?,
                PrivateDeliveryStep::Idle
            ) {
                break;
            }
        }
        self.collect(queue);
        Ok(self.take_carrying(expected))
    }
}

/// The handover phase of the custody that owns exactly this admission's cell.
///
/// AN INTERNAL PHASE, for controls whose claim is about the phase itself. A
/// claim about what a recipient accepted goes through the queue instead.
fn handover_phase(
    private: &crate::PrivateXServerFrontend,
    cell: &Arc<PrivateDeliveryCompletion>,
) -> Option<PrivateDispatchPhase> {
    let owns = |custody: &PrivateDeliveryCustody| {
        custody
            .completion
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, cell))
    };
    private
        .terminal
        .holds
        .iter()
        .find(|record| owns(&record.custody))
        .map(|record| record.custody.dispatch)
        .or_else(|| {
            private.terminal.settling.iter().find_map(|release| {
                release
                    .press_custody
                    .as_ref()
                    .filter(|custody| owns(custody))
                    .map(|custody| custody.dispatch)
                    .or_else(|| owns(&release.custody).then_some(release.custody.dispatch))
            })
        })
}

/// Admit a delivery the way the ingress would, for controls that drive
/// run_ordered_input directly.
///
/// A real delivery is always admitted before it is executed -- that is where
/// its completion is minted -- so a control that presses one which was never
/// admitted is describing a route that cannot occur. Added rather than
/// loosening the executor, which now refuses a release whose answer it could
/// never recognise.
fn admit_for_direct_run(private: &crate::PrivateXServerFrontend, route: &XAuthorityRoutedInput) {
    private
        .broker
        .registry
        .input_recovery
        .admit(route, 0, std::time::Instant::now());
}

#[test]
fn an_admitted_button_runs_the_ordered_path_and_releases_to_its_recorded_hold() {
    let client = XServerFrontendClientId(701);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    // Press, through the whole ordered path: custody accepted, common then the
    // boundary then the X guards, target resolved there rather than earlier,
    // ledger transition, and an immutable record of where it went.
    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(701), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,
        
                watch,
            )
        .expect("the press to run");
    let reached = run.reached.expect("a press decides where it went");
    assert_eq!(reached.client(), client, "it reached the route's client");
    assert_eq!(reached.window(), window);
    // An assertion that no grab chose the recipient stood here. It is REMOVED,
    // not relocated: the source resolves the recipient itself now and does not
    // report whether a grab was involved, so this executor cannot establish
    // that fact and no longer records it.
    //
    // The two assertions above do NOT stand in for it. An active grab with
    // owner_events over the surface's own window reaches exactly this client
    // and this window, so they hold whether or not a grab chose the recipient,
    // and reading them as evidence of its absence would be inferring the fact
    // from a pair that cannot distinguish it.
    //
    // Nothing read the removed flag in production. The selected-event
    // authority it was watching is still decided at the source boundary, while
    // the immutable plan is built, and the control over it belongs there --
    // not to a replacement boolean reported back out to this executor.
    assert!(run.first_press, "this press began the hold");
    assert!(!run.keyboard_applied, "a button moves no keyboard state");
    assert!(matches!(
        run.completion,
        sophia_input_authority::RequestCompletion::Processed
    ));

    // Observed exactly once, which is what frees the grant's cell.
    assert!(matches!(
        pressed.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));

    // A second press of the same input joins the hold rather than beginning
    // one. The ledger says so, not the route: nothing about where this event
    // would go has changed, and treating a join as a new press would deliver
    // the same button down twice.
    let joined = role.reserve(stamp, 2).expect("a second reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(703), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &joined,
        
                watch,
            )
        .expect("the joining press to run");
    assert!(
        !run.first_press,
        "a press onto a held input joins rather than begins"
    );
    assert!(
        !run.keyboard_applied,
        "and a join moves no state, which is the rule keys will need"
    );
    assert!(matches!(
        joined.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));

    // Release. The recipient comes from the hold the press recorded, not from
    // resolving the route again -- so it still answers even though nothing
    // about the route is consulted for it.
    let released = role.reserve(stamp, 3).expect("a third reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(702), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,
        
                watch,
            )
        .expect("the release to run");
    let released_to = run.reached.expect("a delivering release names its hold");
    assert_eq!(
        released_to.client(),
        client,
        "the release answers to the recipient the press reached"
    );
    assert_eq!(
        released_to.window(),
        window,
        "and to the window that press recorded, not whatever the route says now"
    );
    assert!(matches!(
        run.release,
        Some(sophia_input_authority::ReleaseOutcome::DeliverTo(_))
    ));
    assert!(!run.first_press, "a release begins nothing");
    assert!(matches!(
        released.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));
}

#[test]
fn a_key_press_refuses_rather_than_delivering_on_queued_focus() {
    let client = XServerFrontendClientId(711);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let custody = role.reserve(stamp, 1).expect("a reservation").accepted();

    let mut key = motion_to(surface, XAuthorityInputDeliveryId::from_raw(711));
    key.request.kind = InputEventKind::Key {
        keycode: 30,
        pressed: true,
    };
    let refused = private.run_ordered_input(keyboards, &key, &custody, watch);
    assert!(
        matches!(refused, Err(crate::PrivateExecutionRefusal::FocusNotApplied)),
        "the reason is the missing applied focus, not an authority error standing in for it, got {refused:?}"
    );

    // Refused before any keyboard effect: the seat is prepared but nothing
    // moved it, so no modifier describes a key no admitted request applied.
    assert_eq!(
        keyboards.modifiers(SeatId::from_raw(1)),
        Some(0),
        "the refusal came before any keyboard transition"
    );
}

#[test]
fn another_instances_keyboard_history_cannot_drive_this_one() {
    let other = private_for_roles();
    let client = XServerFrontendClientId(721);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards: _, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let mut foreign = other.keyboards().expect("the other instance's state");
    let custody = role.reserve(stamp, 1).expect("a reservation").accepted();

    let refused = private.run_ordered_input(
        &mut foreign,
        &button_to(surface, XAuthorityInputDeliveryId::from_raw(721), 272, true),
        &custody,
    
                watch,
            );
    assert!(
        matches!(
            refused,
            Err(crate::PrivateExecutionRefusal::ForeignKeyboards)
        ),
        "one instance is not driven with another's keyboard history, got {refused:?}"
    );
}

/// A frontend with one admitted, registered client and a surface, ready to run
/// ordered input for it.
fn ordered_fixture(
    client: XServerFrontendClientId,
    surface: SurfaceId,
    window: XResourceId,
) -> (
    crate::PrivateXServerFrontend,
    XServerFrontendClientRouteRegistration,
    crate::PrivateReservationRole,
    crate::PrivateKeyboards,
) {
    let private = private_for_roles();
    let registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(client, NamespaceId::from_raw(client.raw()), surface, window)
        .expect("the surface to register");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let keyboards = private.keyboards().expect("this instance's state");
    (private, registration, role, keyboards)
}

#[test]
fn a_release_answers_its_hold_after_the_surface_is_gone() {
    let client = XServerFrontendClientId(731);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(731), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,
        
                watch,
            )
        .expect("the press to run");
    assert!(run.first_press);
    let _ = pressed.observe();

    // The surface goes. A release that consulted the route would now refuse,
    // which is exactly when a release matters most: the client still holds the
    // button and is owed the event that ends it.
    private
        .broker
        .registry
        .surfaces
        .lock()
        .expect("the surfaces")
        .remove(&surface);

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(732), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,
        
                watch,
            )
        .expect("the release to run with its target gone");
    let reached = run.reached.expect("the release names its hold");
    assert_eq!(reached.client(), client);
    assert_eq!(
        reached.window(),
        window,
        "it answers to what the press recorded"
    );
    assert!(matches!(
        run.release,
        Some(sophia_input_authority::ReleaseOutcome::DeliverTo(_))
    ));
}

#[test]
fn a_release_keeps_the_window_its_press_recorded() {
    let client = XServerFrontendClientId(741);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(741), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,
        
                watch,
            )
        .expect("the press to run");
    assert_eq!(run.reached.expect("a press decides").window(), window);
    let _ = pressed.observe();

    // The surface now maps somewhere else entirely.
    let moved = XResourceId::new(0x200742, 2);
    {
        let mut surfaces = private
            .broker
            .registry
            .surfaces
            .lock()
            .expect("the surfaces");
        let route = surfaces.get_mut(&surface).expect("the route");
        route.window = moved;
    }

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(742), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,
        
                watch,
            )
        .expect("the release to run");
    let reached = run.reached.expect("the release names its hold");
    assert_eq!(
        reached.window(),
        window,
        "the release answers the window its press reached, not where the route points now"
    );
    assert_ne!(reached.window(), moved);
}

#[test]
fn a_release_of_nothing_held_is_an_outcome_not_a_missing_target() {
    let client = XServerFrontendClientId(751);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    // Nothing was ever pressed. The target is registered and present, so a
    // refusal blaming the target would be describing a problem that is not
    // there; the ledger simply owes nobody an event.
    let released = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &button_to(surface, XAuthorityInputDeliveryId::from_raw(751), 272, false),
            &released,
        
                watch,
            )
        .expect("an unheld release is a successful outcome");
    assert!(
        matches!(
            run.release,
            Some(sophia_input_authority::ReleaseOutcome::NotHeld)
        ),
        "the ledger says it was not holding, got {:?}",
        run.release
    );
    assert!(run.reached.is_none(), "so nobody is owed an event");
    assert!(run.event.is_none());
}

/// The pointer state this seat/namespace currently projects.
fn projected_buttons(
    private: &crate::PrivateXServerFrontend,
    namespace: NamespaceId,
    seat: SeatId,
) -> u16 {
    private
        .broker
        .registry
        .pointer_state
        .lock()
        .expect("the pointer state")
        .get(&(namespace, seat))
        .map_or(0, |mapper| mapper.state())
}

#[test]
fn steady_delivery_traffic_does_not_starve_an_older_native_proof() {
    // Choosing native work only when the queues fall empty is not fairness,
    // it is a promise that never comes due: a pointer anyone is actually
    // using keeps a delivery ready at every terminal call, and the proof of a
    // release that already happened waits behind traffic for ever.
    let client = XServerFrontendClientId(2441);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // One release, left owing a proof. Its entry is delivered by hand so the
    // wrapper does not spend the visit.
    let run_by_hand = |private: &mut crate::PrivateXServerFrontend,
                           keyboards: &mut crate::PrivateKeyboards,
                           delivery: u64,
                           button: u32,
                           pressed: bool| {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
    };
    run_by_hand(private, keyboards, 2441, 272, true);
    private.deliver_one(&mut |_, _| Ok(())).expect("the press delivers");
    run_by_hand(private, keyboards, 2442, 272, false);
    private
        .deliver_one(&mut |_, _| Ok(()))
        .expect("the release delivers");
    assert_eq!(private.terminal.settling.len(), 1);
    assert!(
        !private.terminal.settling[0].native_recorded(),
        "its proof is still owed, which is what this control is about"
    );

    // Now keep a real delivery ready at EVERY terminal call, on a different
    // button so no new native debt is created, and count the visits the old
    // proof gets while that traffic runs.
    let mut visits = 0;
    let mut delivered = 0;
    for round in 0..8u64 {
        // A real entry is ready and waiting at every terminal call. The first
        // is a press of a second button; the rest join it, so the traffic
        // creates no new native debt of its own.
        run_by_hand(private, keyboards, 2450 + round, 273, true);
        // Steps until this round's entry is delivered. A native visit taking
        // one of them is exactly the fairness under test; the delivery still
        // gets its step and keeps its place.
        loop {
            match private
                .deliver_one(&mut |_, _| Ok(()))
                .expect("a terminal step")
            {
                PrivateDeliveryStep::Recorded { .. } => visits += 1,
                PrivateDeliveryStep::Advanced { .. } => {
                    delivered += 1;
                    break;
                }
                // Once its proof is in, the same release owes a delivery
                // attempt. That is more native work, and it takes its turn
                // the same bounded way.
                PrivateDeliveryStep::Dispatched { .. } => visits += 1,
                PrivateDeliveryStep::Receipt { .. } => {
                    panic!("no receipt has been published in this control")
                }
                PrivateDeliveryStep::Idle => panic!("traffic was ready, so no step is idle"),
                PrivateDeliveryStep::Blocked(_) => panic!("no entry is indeterminate here"),
            }
        }
    }
    assert_eq!(delivered, 8, "every round's entry was delivered, in its turn");

    assert!(
        visits >= 1,
        "native work got a bounded turn while deliveries stayed ready; it \
         received {visits} visits across eight rounds of traffic"
    );
    assert!(
        private.terminal.settling[0].native_recorded(),
        "and the older proof was recorded rather than waiting behind traffic"
    );
}

/// Stage what an interrupted handover leaves on a hold record: the phase
/// saying the handover began, with the exact capsule still in its slot.
///
/// Reproduces an unwind between the write-ahead and the send. It forges no
/// receipt and builds no delivery -- the capsule is the one production made.
fn stage_interrupted_head(record: &mut PrivateHoldRecord, capsule: XAuthorityOrderedDelivery) {
    record.custody.pending = Some(PrivatePendingDelivery::Capsule(capsule));
    record.custody.dispatch = PrivateDispatchPhase::Indeterminate;
}

/// Stage what an interrupted handover leaves in a settling record: the capsule
/// gone from the slot, and the phase saying the handover was begun.
///
/// Lives here rather than in the production module, where a cfg(test) helper
/// is inline test code. It fabricates no delivery and forges no receipt -- it
/// reproduces exactly the state an unwind between the take and the report
/// would leave behind.
fn stage_interrupted_handover(release: &mut PrivateSettlingRelease) {
    release.custody.pending = None;
    release.custody.dispatch = PrivateDispatchPhase::Indeterminate;
}

fn settling_slot_is_empty(release: &PrivateSettlingRelease) -> bool {
    release.custody.pending.is_none()
}

#[test]
fn an_outcome_is_owned_before_an_ordinary_observer_can_prune_it() {
    // Recovery drops a routing-finished ticket the moment an ordinary
    // observer consumes it, and that ticket is the only place the outcome
    // lives. A join that read it only when it was ready to settle would find
    // the attempt still out and its answer already gone.
    let client = XServerFrontendClientId(2491);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2491u64, true), (2492u64, false)] {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    for _ in 0..12 {
        private.deliver_one(&mut |_, _| Ok(())).expect("a step");
    }
    assert_eq!(
        private.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Enqueued
    );
    let delivery = private.terminal.settling[0]
        .delivery()
        .expect("the release knows its delivery");

    // The writer answers, and an ORDINARY OBSERVER consumes it -- which is
    // what prunes the ticket.
    let recovery = &private.broker.registry.input_recovery;
    recovery
        .finish(client, Some(delivery), XAuthorityInputDeliveryOutcome::Flushed)
        .expect("the answer is published");

    // One visit, so the terminal side takes custody of the outcome.
    private.deliver_one(&mut |_, _| Ok(())).expect("a step");

    assert_eq!(
        private.terminal.settling[0].outcome_seen(),
        Some(XAuthorityInputDeliveryOutcome::Flushed),
        "the outcome is owned here, not merely readable over there"
    );
    assert!(
        private.terminal.settling[0].attempt().is_none(),
        "and the attempt was finished against it"
    );
}

/// Run one real request from one real producer and return what it decided.
///
/// Adopted from the independent review, which found the two no-event branches
/// my own fixture could not reach: they need DISTINCT producers. One device
/// pressing and joining always leaves the same holder bit, so a release from
/// it is always a final one.
fn noevent_run(
    fixture: &mut PreparedOrderedFixture,
    ingress: &crate::PrivateIngress,
    delivery: u64,
    device: u64,
    button: u32,
    pressed: bool,
) -> PrivateOrderedRun {
    let mut route = button_to(
        fixture.surface,
        XAuthorityInputDeliveryId::from_raw(delivery),
        button,
        pressed,
    );
    route.request.device = DeviceId::from_raw(device);
    ingress.submit(route).expect("the order to accept it");
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().expect("a live runner");
    assert!(matches!(
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().expect("a sealed watch"))
            .expect("a step"),
        PrivateOrderedStep::Decided(_)
    ));
    let Some(item) = private.terminal.turn.pop() else {
        panic!("an accepted request produces a decision")
    };
    let PrivateOrderedItem::Ran { run, custody, route, .. } = item else {
        panic!("an accepted request must run, not refuse")
    };
    assert_eq!(route.request.device, DeviceId::from_raw(device));
    assert!(
        custody.observe().expect("a readable completion").is_some(),
        "the real common completion, not a fabricated writer result"
    );
    run
}

/// The two known no-event branches, reached from genuinely separate sources.
///
/// SurvivorRemains: A presses, B joins, A releases -- B's holder bit remains.
/// NotHeld: A presses, and B, which never joined, releases -- the ledger
/// finds no holder bit for B while this executor still has the record.
///
/// Neither outcome, completion cell nor holder bit is set by the control:
/// the source and the ledger produce the branch themselves.
fn noevent_from_distinct_sources(survivor: bool) {
    let (client, base) = if survivor {
        (XServerFrontendClientId(7511), 75110)
    } else {
        (XServerFrontendClientId(7521), 75210)
    };
    let mut fixture = prepared_ordered_fixture(client);
    let first = fixture
        .runner
        .frontend
        .as_mut()
        .expect("a live runner")
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("a producer");
    let second = fixture
        .runner
        .frontend
        .as_mut()
        .expect("a live runner")
        .ingress_for(client, DeviceId::from_raw(2))
        .expect("a second producer");

    let pressed = noevent_run(&mut fixture, &first, base, 1, 272, true);
    assert!(pressed.first_press && pressed.owes_event && pressed.event.is_some());
    let private = fixture.runner.frontend.as_ref().expect("a live runner");
    let recovery = private.broker.registry.input_recovery.clone();
    let original = private.terminal.holds[0]
        .custody
        .completion
        .as_ref()
        .expect("the press holds its own completion")
        .clone();
    let incarnation = private.terminal.holds[0].incarnation;
    // The obligation's own address, so "unchanged" means the same one rather
    // than merely something being present.
    let native_address =
        private.terminal.holds[0].native.as_ref().expect("a source obligation") as *const _
            as usize;

    if survivor {
        let joined = noevent_run(&mut fixture, &second, base + 1, 2, 272, true);
        assert!(!joined.first_press && !joined.owes_event && joined.event.is_none());
    }

    let noevent = noevent_run(
        &mut fixture,
        if survivor { &first } else { &second },
        base + 2,
        if survivor { 1 } else { 2 },
        272,
        false,
    );
    let expected = if survivor {
        sophia_input_authority::ReleaseOutcome::SurvivorRemains
    } else {
        sophia_input_authority::ReleaseOutcome::NotHeld
    };
    assert_eq!(noevent.release, Some(expected));
    assert!(!noevent.owes_event && noevent.event.is_none());

    // Nothing of the original press was disturbed, and nothing was invented.
    let private = fixture.runner.frontend.as_ref().expect("a live runner");
    assert_eq!(private.terminal.holds.len(), 1);
    assert!(private.terminal.settling.is_empty());
    assert_eq!(private.terminal.holds[0].incarnation, incarnation);
    assert_eq!(
        private.terminal.holds[0].native.as_ref().expect("still held") as *const _ as usize,
        native_address,
        "the same source obligation, not a replacement that merely exists"
    );
    assert!(Arc::ptr_eq(
        &original,
        private.terminal.holds[0]
            .custody
            .completion
            .as_ref()
            .expect("still held")
    ));
    let release = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(base + 2))
        .expect("a readable ledger")
        .expect("the no-event release's own admission");
    assert!(!Arc::ptr_eq(&original, &release));
    assert!(original.answer().is_none() && release.answer().is_none());
    assert!(fixture.channels.ordered.try_recv().is_err());
    // The button another genuine source still holds is untouched: the native
    // projection was not moved by a release that owed no event.
    let mask = private
        .broker
        .registry
        .pointer_state
        .lock()
        .expect("the pointer state")
        .get(&(NamespaceId::from_raw(client.raw()), SeatId::from_raw(1)))
        .expect("the original native mapper is still present")
        .state();
    assert_eq!(
        mask, 256,
        "another genuine source still holds the original button"
    );
    // Owner counts, which say who is keeping each cell alive rather than only
    // that the cells differ.
    assert_eq!(
        Arc::strong_count(&original),
        3,
        "the press cell is held by the ledger, the record and this inspection"
    );
    assert_eq!(
        Arc::strong_count(&release),
        2,
        "and the disposed release cell only by the ledger and this inspection"
    );

    // THE DISPOSAL ITSELF.
    assert!(
        private.terminal.pending_custody.is_none(),
        "a known no-event branch disposes of the custody it acquired"
    );

    // And a real new press then runs through the protected path. Accepting
    // some other refusal would not show the slot was free.
    let next = noevent_run(&mut fixture, &first, base + 3, 1, 273, true);
    assert!(next.first_press && next.owes_event && next.event.is_some());
    let private = fixture.runner.frontend.as_ref().expect("a live runner");
    assert_eq!(private.terminal.holds.len(), 2);
    assert!(private.terminal.pending_custody.is_none() && private.terminal.native_pending.is_none());
    assert!(original.answer().is_none() && release.answer().is_none());
}

#[test]
fn a_survivor_release_from_a_distinct_source_disposes_only_its_own_custody() {
    noevent_from_distinct_sources(true);
}

#[test]
fn a_not_held_release_from_a_nonparticipating_source_disposes_only_its_own_custody() {
    noevent_from_distinct_sources(false);
}

#[test]
fn a_completed_operation_leaves_no_custody_behind_for_the_next_one() {
    // The slot is emptied by an explicit transfer or disposition. A press and
    // a release that complete move their custody into the record for their
    // debt, so nothing is left to refuse the operation after them.
    //
    // WHAT THIS DOES NOT REACH: the no-event results, NotHeld and
    // SurvivorRemains, with a record present. A join adopts the hold rather
    // than adding a holder, so a release here always reports DeliverTo and
    // this fixture cannot produce the other two. Their disposal is written and
    // is NOT proved by this control.
    let client = XServerFrontendClientId(2571);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, .. } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let button = |private: &mut crate::PrivateXServerFrontend,
                  keyboards: &mut crate::PrivateKeyboards,
                  slot: u64,
                  delivery: u64,
                  pressed: bool| {
        let custody = role.reserve(stamp, slot).expect("a reservation").accepted();
        let run = private.run_ordered_input(
            keyboards,
            &{
                let route = button_to(
                    surface,
                    XAuthorityInputDeliveryId::from_raw(delivery),
                    272,
                    pressed,
                );
                admit_for_direct_run(private, &route);
                route
            },
            &custody,
            watch,
        );
        let _ = custody.observe();
        run
    };

    button(private, keyboards, 1, 2571, true).expect("the press to run");
    assert!(
        private.terminal.pending_custody.is_none(),
        "a completed press moved its custody into its record"
    );
    button(private, keyboards, 2, 2572, false).expect("the release to run");
    assert!(
        private.terminal.pending_custody.is_none(),
        "and a completed release moved its own"
    );

    // So the next operation is not refused for something left behind.
    let next = button(private, keyboards, 3, 2573, true);
    assert!(
        !matches!(
            next,
            Err(crate::PrivateExecutionRefusal::CustodyRetained)
        ),
        "nothing was left held, so nothing is refused for holding it"
    );
}

#[test]
fn a_retained_custody_refuses_the_next_operation_rather_than_being_replaced() {
    // A refusal that leaves the source holding context leaves this custody
    // attached to that same continuation. Assigning over it would drop the
    // only handle able to answer whatever that continuation still owes, with
    // nothing recorded about what became of it -- and the replacement would
    // then be the one everything else believed in.
    let client = XServerFrontendClientId(2561);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    let run = |private: &mut crate::PrivateXServerFrontend,
                   keyboards: &mut crate::PrivateKeyboards,
                   delivery: u64,
                   button: u32,
                   pressed: bool| {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("the entry delivers");
    };

    run(private, keyboards, 2561, 272, true);
    run(private, keyboards, 2562, 272, false);
    assert_eq!(private.terminal.settling.len(), 1, "a release is owed");

    // A re-press of the same button, barred by the open release debt. The
    // source refuses after taking context, so custody stays held.
    run(private, keyboards, 2563, 272, true);
    let retained = private
        .terminal
        .pending_custody
        .as_ref()
        .and_then(|custody| custody.completion.clone())
        .expect("the refused press left its custody held");

    // ANOTHER ADMITTED PRESS IS REFUSED, not allowed to overwrite it. Driven
    // directly so this asserts the guard rather than a full queue.
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let next = role.reserve(stamp, 9).expect("a reservation").accepted();
    let refused = private.run_ordered_input(
        keyboards,
        &{
            let route = button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(2564),
                273,
                true,
            );
            admit_for_direct_run(private, &route);
            route
        },
        &next,
        watch,
    );
    assert!(
        matches!(
            refused,
            Err(crate::PrivateExecutionRefusal::CustodyRetained)
        ),
        "the next operation is refused under its own cause rather than \
         replacing what is held"
    );
    let still = private
        .terminal
        .pending_custody
        .as_ref()
        .and_then(|custody| custody.completion.clone())
        .expect("and the held custody is still here");
    assert!(
        Arc::ptr_eq(&still, &retained),
        "the very handle the refused press left, not a replacement"
    );

    // And an instance holding it does not report itself empty.
    assert!(!private.terminal.is_empty());
}

/// Run one real request through the fixture's own producer and observe its
/// completion, so the grant is free for the next one.
fn fixture_run(
    fixture: &mut PreparedOrderedFixture,
    delivery: u64,
    button: u32,
    pressed: bool,
) -> PrivateOrderedRun {
    let route = button_to(
        fixture.surface,
        XAuthorityInputDeliveryId::from_raw(delivery),
        button,
        pressed,
    );
    fixture.ingress.submit(route).expect("the order to accept it");
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut fixture.runner;
    let private = frontend.as_mut().expect("a live runner");
    assert!(matches!(
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().expect("a sealed watch"))
            .expect("a step"),
        PrivateOrderedStep::Decided(_)
    ));
    let Some(PrivateOrderedItem::Ran { run, custody, .. }) = private.terminal.turn.pop() else {
        panic!("an accepted request runs")
    };
    assert!(custody.observe().expect("a readable completion").is_some());
    run
}

#[test]
fn a_blocked_head_stops_its_own_connection_while_another_recipient_progresses() {
    // Adopted from the independent review, which supplied the assertion I had
    // said was missing: that a blocked head stops ITS connection rather than
    // every connection. A second recipient is made the way one really
    // appears -- the first client gives up its grab and the second takes one
    // -- so its events are resolved by the source, not inserted by the test.
    let client = XServerFrontendClientId(7561);
    let mut fixture = prepared_ordered_fixture(client);
    let namespace = fixture.namespace;
    fixture_run(&mut fixture, 75610, 272, true);
    fixture_run(&mut fixture, 75611, 273, true);

    let other = XServerFrontendClientId(7562);
    let other_window = XResourceId::new(0x307562, 1);
    let (other_registration, other_channels) = {
        let private = fixture.runner.frontend.as_mut().expect("a live runner");
        let registry = &private.broker.registry;
        let context = namespaced(other, namespace);
        let (registration, channels) = registry
            .register_client_with_admission(other, Some(context))
            .expect("a fresh client registers");
        registry
            .attach_private_lifecycle(&registration, context)
            .expect("the boundary admits");
        let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry
            .attach_connection_state(
                &registration,
                namespace,
                selected.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .expect("its connection state attaches");
        {
            let mut state = selected.lock().expect("the selections");
            state.register(
                other_window,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                Rect {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 100,
                },
            );
            state.observe_mapped(other_window);
            state.update(other_window, Some((1 << 2) | (1 << 3)), None);
        }
        // Real owner operations, so the source resolves later presses to the
        // new owner itself. Nothing foreign is inserted into this executor.
        let mut grabs = registry.input_authority.lock().expect("the grab state");
        grabs.ungrab_pointer(namespace, client.raw());
        grabs
            .grab_pointer(
                namespace,
                crate::XActiveInputGrab {
                    owner: other.raw(),
                    window: other_window,
                    owner_events: false,
                    pointer_mode: 1,
                    keyboard_mode: 1,
                    event_mask: u16::MAX,
                    xi_event_mask: [0; 8],
                    xi_event_mask_words: 0,
                    route_lease: None,
                },
            )
            .expect("the grab to take");
        (registration, channels)
    };
    fixture_run(&mut fixture, 75612, 274, true);

    let private = fixture.runner.frontend.as_mut().expect("a live runner");
    assert_eq!(private.terminal.holds.len(), 3);
    // Both earlier presses really reached A, and the later one really reached
    // B: the source resolved them, and this control asserts that rather than
    // assuming it.
    assert_eq!(private.terminal.holds[0].reached.client(), client);
    assert_eq!(private.terminal.holds[1].reached.client(), client);
    assert_eq!(private.terminal.holds[2].reached.client(), other);
    let recovery = private.broker.registry.input_recovery.clone();
    let cells = [75610u64, 75611, 75612].map(|id| {
        recovery
            .completion_for(XAuthorityInputDeliveryId::from_raw(id))
            .expect("a readable ledger")
            .expect("its own admission")
    });

    // Stage the state between the phase write and the capsule take. No
    // interrupted send is claimed; the capsule is the one production built.
    let record = &mut private.terminal.holds[0];
    let emission = record
        .native
        .as_mut()
        .expect("a source obligation")
        .take_press_emission()
        .expect("the press built its event");
    PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, client);
    record.custody.dispatch = PrivateDispatchPhase::Indeterminate;

    let mut seen_blocked = Vec::new();
    let mut seen_other = Vec::new();
    for _ in 0..8 {
        // Each visit must succeed. Discarding the result would let a failing
        // step pass for an empty queue.
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("a terminal step");
        while let Ok(capsule) = fixture.channels.ordered.try_recv() {
            seen_blocked.push(capsule.delivery());
        }
        while let Ok(capsule) = other_channels.ordered.try_recv() {
            assert_eq!(capsule.client(), other);
            assert!(Arc::ptr_eq(
                &cells[2],
                &capsule.finalizer().expect("a carried finalizer").completion
            ));
            seen_other.push(capsule.delivery());
        }
    }

    assert!(
        seen_blocked.is_empty(),
        "neither the unresolved head nor anything behind it reaches its own \
         recipient"
    );
    assert_eq!(
        seen_other,
        [XAuthorityInputDeliveryId::from_raw(75612)],
        "a distinct recipient still progresses: one blocked connection is not \
         a barrier for every connection"
    );
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Indeterminate
    );
    // THE EXACT CAPSULE, by its admission's own completion and not by the
    // number it carries. A delivery id can be pruned and handed out again, so
    // matching one establishes nothing about custody.
    assert!(
        matches!(
            private.terminal.holds[0].custody.pending.as_ref(),
            Some(PrivatePendingDelivery::Capsule(capsule))
                if capsule.delivery() == XAuthorityInputDeliveryId::from_raw(75610)
                    && Arc::ptr_eq(
                        &cells[0],
                        &capsule.finalizer().expect("a carried finalizer").completion
                    )
        ),
        "and the exact unknown-handover capsule stays owned here"
    );
    assert!(cells.iter().all(|cell| cell.answer().is_none()));
    drop(other_registration);
}

#[test]
fn an_unresolved_head_blocks_its_connection_without_being_offered_again() {
    // An unresolved handover must stay in the ordering comparison, because it
    // is what blocks the events behind it. Being the head is not permission to
    // act on it: its slot still holding bytes is exactly what an interruption
    // after the write-ahead leaves, and offering those bytes again is a replay
    // of an event the recipient may already have.
    let client = XServerFrontendClientId(7561);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        channels,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, button) in [(75610u64, 272u32), (75611, 273)] {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                true,
            ))
            .expect("the order to accept it");
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch)
            .expect("a decided step");
        let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.pop() else {
            panic!("an accepted request runs")
        };
        assert!(custody.observe().expect("a readable completion").is_some());
    }
    assert_eq!(private.terminal.holds.len(), 2);

    // Build the first press's capsule, then stage exactly what an interruption
    // after the write-ahead leaves: the phase saying the handover began, and
    // the capsule still in the slot.
    private
        .deliver_one(&mut |_, _| Ok(()))
        .expect("a step that prepares and hands over the first press");
    let queued: Vec<_> = std::iter::from_fn(|| channels.ordered.try_recv().ok()).collect();
    assert_eq!(
        queued.len(),
        1,
        "the first press went, which is what gives us a capsule to stage"
    );
    stage_interrupted_head(&mut private.terminal.holds[0], queued.into_iter().next().unwrap());

    // NOTHING MORE IS HANDED OVER FOR THIS RECIPIENT. The staged head blocks
    // the press behind it, and is not offered again itself.
    for _ in 0..8 {
        let _ = private.deliver_one(&mut |_, _| Ok(()));
    }
    assert!(
        channels.ordered.try_recv().is_err(),
        "an unresolved head is not re-sent, and nothing behind it passes"
    );
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Indeterminate,
        "and its phase is unchanged: a slot holding bytes is not permission"
    );
    assert!(
        private.terminal.holds[0].custody.pending.is_some(),
        "the exact capsule is still held, not consumed by an offer"
    );

    // AND IT IS NOT REPORTED AS A DISPATCH EITHER. The head is refused before
    // anything is taken from it, so the visit says it had no press to hand
    // over -- not that it tried one and the queue refused. The difference is
    // what the stall allowance counts, and counting an ineligible head as a
    // refused attempt spends the press path's allowance on a head that can
    // never use it.
    assert_eq!(
        private.dispatch_one_press(),
        None,
        "an unresolved head is not an attempted dispatch"
    );
}

#[test]
fn a_carried_older_press_is_handed_over_before_a_newer_held_press() {
    // A press whose hold has ended travels into the settling record. Choosing
    // held work first handed the later press over before the earlier one that
    // had merely moved, so the recipient would have seen a second button go
    // down before the first one it was already owed.
    let client = XServerFrontendClientId(7551);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        channels,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // Decided before any terminal service: press, its release, then a second
    // press of a different button.
    for (delivery, button, pressed) in [
        (75510u64, 272u32, true),
        (75511, 272, false),
        (75512, 273, true),
    ] {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                pressed,
            ))
            .expect("the order to accept it");
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch)
            .expect("a decided step");
        // The decision's own outcome is observed here, which is what frees the
        // grant for the next request. The handover is a separate fact and has
        // not happened yet.
        let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.pop() else {
            panic!("an accepted request runs")
        };
        assert!(custody.observe().expect("a readable completion").is_some());
    }

    // Now drive the handovers and watch the order they reach the queue in.
    let mut order = Vec::new();
    for _ in 0..12 {
        let _ = private.deliver_one(&mut |_, _| Ok(()));
        while let Ok(capsule) = channels.ordered.try_recv() {
            order.push(capsule.delivery());
        }
    }

    // THE WHOLE STREAM, in the order the recipient must see it. Checking only
    // that the first press led would have missed a later press overtaking the
    // release between them -- which is exactly what happened.
    assert_eq!(
        order,
        vec![
            XAuthorityInputDeliveryId::from_raw(75510),
            XAuthorityInputDeliveryId::from_raw(75511),
            XAuthorityInputDeliveryId::from_raw(75512),
        ],
        "press, its release, then the later press: one order for one \
         recipient, across press and release custody alike"
    );
}

#[test]
fn a_final_release_carries_the_presss_own_custody_rather_than_replacing_it() {
    // A press and a release are two events owed to the same recipient, each
    // with its own admission and its own handle. Ending the physical hold
    // answers neither and transfers neither, so a release that built fresh
    // custody and dropped the record's left the press's delivery owed by
    // nobody -- its answer reachable only through owners outside this
    // instance.
    let client = XServerFrontendClientId(2551);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2551),
            272,
            true,
        ))
        .expect("the order to accept the press");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.terminal.delivering.extend(turn);
    private
        .deliver_one(&mut |_, _| Ok(()))
        .expect("the press delivers");
    let press_cell = private.terminal.holds[0]
        .custody
        .completion
        .clone()
        .expect("the press holds its own completion");

    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2552),
            272,
            false,
        ))
        .expect("the order to accept the release");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.terminal.delivering.extend(turn);
    private
        .deliver_one(&mut |_, _| Ok(()))
        .expect("the release delivers");

    assert!(
        private.terminal.holds.is_empty(),
        "the physical hold ended"
    );
    assert_eq!(private.terminal.settling.len(), 1);

    // BOTH CUSTODIES, DISTINCT. The release has its own, and the press's
    // travelled on rather than being replaced by it.
    let release_cell = private.terminal.settling[0]
        .custody
        .completion
        .clone()
        .expect("the release holds its own completion");
    let carried = private.terminal.settling[0]
        .press_custody
        .as_ref()
        .expect("the press's custody travelled with the release")
        .completion
        .clone()
        .expect("and still holds the press's own completion");
    assert!(
        Arc::ptr_eq(&carried, &press_cell),
        "the very completion the press was recorded with, not a replacement"
    );
    assert!(
        !Arc::ptr_eq(&release_cell, &press_cell),
        "and the release's is a different admission's, as it must be"
    );
}

#[test]
fn a_press_whose_answer_could_never_be_recognised_refuses_before_applying() {
    // Presses carry the same forward custody releases do, acquired on the
    // operation that creates the debt. A press that cannot get it refuses
    // before the effect rather than applying one whose answer nothing could
    // ever match to it.
    let client = XServerFrontendClientId(2541);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, .. } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    // Never admitted, so no completion was ever minted for it.
    let refused = private.run_ordered_input(
        keyboards,
        &button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2599),
            272,
            true,
        ),
        &pressed,
        watch,
    );
    assert!(
        matches!(
            refused,
            Err(crate::PrivateExecutionRefusal::CompletionMissing)
        ),
        "a press with no completion is refused under its own cause"
    );
    assert!(
        private.terminal.holds.is_empty(),
        "and nothing was applied: no hold, no obligation, nothing owed"
    );
}

#[test]
fn a_release_whose_answer_could_never_be_recognised_refuses_and_keeps_its_hold() {
    // Custody is acquired before the effect, and a release that cannot get it
    // is refused there -- before the ledger moves and before the record that
    // owns the native obligation leaves inventory. Enqueuing anyway would
    // hand over a delivery whose answer nothing could ever match to its debt.
    let client = XServerFrontendClientId(2531);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // A real press through the ingress, so a hold exists with its obligation.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2531),
            272,
            true,
        ))
        .expect("the order to accept the press");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.terminal.delivering.extend(turn);
    private
        .deliver_one(&mut |_, _| Ok(()))
        .expect("the press delivers");
    assert_eq!(private.terminal.holds.len(), 1);
    assert!(
        private.terminal.holds[0].native.is_some(),
        "the press left its source obligation on its record"
    );

    // A release whose delivery was never admitted, so no completion was ever
    // minted for it. This is the case the executor must refuse.
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let released = role.reserve(stamp, 9).expect("a reservation").accepted();
    let refused = private.run_ordered_input(
        keyboards,
        &button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2599),
            272,
            false,
        ),
        &released,
        watch,
    );
    // NAMED, not merely refused. A known-absent completion says this delivery
    // will never be answerable; an unreadable ledger says nothing was
    // established either way. Reporting both as RecoveryUnavailable discarded
    // the difference at the boundary where it decides what to do next.
    assert!(
        matches!(
            refused,
            Err(crate::PrivateExecutionRefusal::CompletionMissing)
        ),
        "a release with no completion is refused under its own cause"
    );

    // AND THE OBLIGATION IS STILL HERE. Refusing before the effect is what
    // makes that true: nothing was released, so nothing was left unowned.
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "the hold stays exactly where it was"
    );
    assert!(
        private.terminal.holds[0].native.is_some(),
        "and it still owns the activation, query scope and selection it raised"
    );
    assert!(
        private.terminal.settling.is_empty(),
        "no release record was made for a release that did not happen"
    );
}

#[test]
fn a_release_holds_the_completion_of_the_admission_it_was_recorded_for() {
    // Acquiring the handle at dispatch meant looking the delivery up by id
    // again. By then the original ticket may have been pruned -- leaving no
    // handle at all -- or the id re-admitted, leaving the replacement's
    // handle on the older debt. Taken when the release is recorded, neither
    // is reachable.
    let client = XServerFrontendClientId(2521);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2521u64, true), (2522u64, false)] {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    assert_eq!(private.terminal.settling.len(), 1);

    // BEFORE ANY TERMINAL VISIT. The handle is already held, because it was
    // taken on the operation that created this debt.
    let held = private.terminal.settling[0]
        .completion()
        .expect("the release holds its own completion from the moment it exists")
        .clone();
    let delivery = private.terminal.settling[0]
        .delivery()
        .expect("the release knows its delivery");

    // The original ticket is answered and pruned, then the id is admitted
    // again -- the two situations that defeated a dispatch-time lookup.
    let recovery = &private.broker.registry.input_recovery;
    recovery
        .finish(client, Some(delivery), XAuthorityInputDeliveryOutcome::Flushed)
        .expect("the original admission is answered");
    recovery.observe(XAuthorityClientInputDelivery {
        client,
        delivery,
        outcome: XAuthorityInputDeliveryOutcome::Flushed,
    });
    let reused = button_to(surface, delivery, 274, true);
    let readmitted = recovery.admit(&reused, 0, std::time::Instant::now());

    // Still its own, and still answering with its own outcome.
    assert!(
        Arc::ptr_eq(
            private.terminal.settling[0]
                .completion()
                .expect("still held"),
            &held
        ),
        "nothing refreshed the handle, so a re-admission did not replace it"
    );
    if readmitted {
        let fresh = recovery
            .completion_for(delivery).ok().flatten()
            .expect("the re-admission minted its own");
        assert!(
            !Arc::ptr_eq(&fresh, &held),
            "and the replacement is a different completion entirely"
        );
    }
    assert_eq!(
        private.terminal.settling[0]
            .completion_answer()
            .map(|receipt| receipt.outcome),
        Some(XAuthorityInputDeliveryOutcome::Flushed),
        "the debt reads the answer of the admission it was recorded for"
    );
}

#[test]
fn custody_of_the_answer_is_taken_before_the_handover_not_after_it_succeeds() {
    // Stored after a successful send, custody would be absent exactly when it
    // matters most: an enqueue followed by an interruption leaves a record
    // that began a handover and cannot recognise its own receipt. A send that
    // fails is the reachable case with the same shape -- the handover was
    // attempted, so the answer must already be held.
    let client = XServerFrontendClientId(2511);
    let PreparedOrderedFixture {
        mut runner,
        ingress,
        registration,
        channels,
        durable,
        surface,
        window: _window,
        selections: _selections,
        client: _client,
        namespace: _namespace,
        deliveries: _deliveries,
        _acks,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2511u64, true), (2512u64, false)] {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    // Record the proof, so the release becomes eligible for an attempt.
    for _ in 0..4 {
        private.deliver_one(&mut |_, _| Ok(())).expect("a step");
        if private.terminal.settling[0].native_recorded() {
            break;
        }
    }
    assert!(private.terminal.settling[0].native_recorded());

    // The recipient's queue goes, while its routes stay. The sender is still
    // found, so the handover is attempted -- and refused.
    drop(channels);

    let step = private.deliver_one(&mut |_, _| Ok(())).expect("a step");
    assert!(
        matches!(
            step,
            PrivateDeliveryStep::Dispatched {
                enqueued: false,
                ..
            }
        ),
        "the handover was attempted and refused"
    );
    assert!(
        private.terminal.settling[0].completion().is_some(),
        "custody of the answer was taken before the send, so a handover that \
         failed still leaves the record able to recognise its own receipt"
    );
    drop(registration);
    drop(durable);
    drop(_acks);
}

#[test]
fn a_reused_delivery_id_does_not_settle_the_debt_that_had_it_before() {
    // A delivery id can be pruned and handed out again. The same client can
    // then publish an outcome under that number for a different incarnation
    // entirely, and matching on client and id alone would let it answer a
    // debt it has nothing to do with. Same client is the demonstrated case,
    // so a wrong-client check is not the protection.
    let client = XServerFrontendClientId(2501);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2501u64, true), (2502u64, false)] {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    for _ in 0..12 {
        private.deliver_one(&mut |_, _| Ok(())).expect("a step");
    }
    let delivery = private.terminal.settling[0]
        .delivery()
        .expect("the release knows its delivery");
    assert!(
        private.terminal.settling[0].completion().is_some(),
        "custody of this admission's completion was taken before the handover"
    );

    // The delivery is answered and an ordinary observer consumes it, which
    // prunes the ticket and frees the id. Then the SAME id is admitted again
    // for different work by the same client.
    let recovery = &private.broker.registry.input_recovery;
    recovery
        .finish(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::WriteFailed,
        )
        .expect("the first admission is answered");
    assert_eq!(
        private.terminal.settling[0]
            .completion_answer()
            .map(|receipt| receipt.outcome),
        Some(XAuthorityInputDeliveryOutcome::WriteFailed),
        "the held cell carries its own admission's answer"
    );
    recovery.observe(XAuthorityClientInputDelivery {
        client,
        delivery,
        outcome: XAuthorityInputDeliveryOutcome::WriteFailed,
    });

    // A fresh admission under the reused number. IT GETS ITS OWN CELL, which
    // is what makes the number harmless: the old holder's answer can never be
    // written by later work, whether or not that work is answered first.
    let reused = button_to(surface, delivery, 274, true);
    assert!(
        recovery.admit(&reused, 0, std::time::Instant::now()),
        "the pruned id is available again, which is the situation being guarded"
    );
    let fresh = recovery
        .completion_for(delivery).ok().flatten()
        .expect("the new admission minted its own completion");
    let held = private.terminal.settling[0]
        .completion()
        .expect("the release still holds its own");
    assert!(
        !Arc::ptr_eq(&fresh, held),
        "a reused delivery id mints a new completion rather than handing back \
         the one an earlier admission is still answered through"
    );
    recovery
        .finish(client, Some(delivery), XAuthorityInputDeliveryOutcome::Flushed)
        .expect("the new admission is answered");
    assert_eq!(
        private.terminal.settling[0]
            .completion_answer()
            .map(|receipt| receipt.outcome),
        Some(XAuthorityInputDeliveryOutcome::WriteFailed),
        "and the older debt still reads its own answer, not the newer one"
    );
}

#[test]
fn a_receipt_settles_only_what_it_establishes_and_never_authorises_a_replay() {
    // Two releases with the same shape and opposite answers. A flush is proof
    // the bytes went; a failed write is the absence of proof either way, and
    // the difference is the whole of what a receipt is for.
    let client = XServerFrontendClientId(2481);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, button, pressed) in [
        (2481u64, 272u32, true),
        (2482, 272, false),
        (2483, 273, true),
        (2484, 273, false),
    ] {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    assert_eq!(private.terminal.settling.len(), 2);

    // Drive until both releases have their proof recorded and their delivery
    // handed over.
    for _ in 0..24 {
        private.deliver_one(&mut |_, _| Ok(())).expect("a step");
    }
    for index in 0..2 {
        assert_eq!(
            private.terminal.settling[index].dispatch(),
            PrivateDispatchPhase::Enqueued,
            "release {index} reached its recipient's queue"
        );
        assert!(private.terminal.settling[index].attempt().is_some());
    }

    // The writer answers: one flushed, one failed.
    let recovery = &private.broker.registry.input_recovery;
    for (index, outcome) in [
        (0usize, XAuthorityInputDeliveryOutcome::Flushed),
        (1, XAuthorityInputDeliveryOutcome::WriteFailed),
    ] {
        let delivery = private.terminal.settling[index]
            .delivery()
            .expect("the release knows which delivery carries it");
        recovery
            .finish(client, Some(delivery), outcome)
            .expect("the writer's answer is published against the delivery it names");
    }

    // Answer both receipts.
    for _ in 0..4 {
        private.deliver_one(&mut |_, _| Ok(())).expect("a step");
    }

    // THE FLUSH SETTLED THE RECIPIENT'S HALF.
    assert_eq!(
        private.terminal.settling[0].outcome_seen(),
        Some(XAuthorityInputDeliveryOutcome::Flushed)
    );
    assert_eq!(
        private.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Enqueued,
        "a flushed delivery is finished where it was, not marked unsendable"
    );

    // THE FAILED WRITE SETTLED NOTHING AND AUTHORISED NOTHING. It is not
    // proof that nobody received anything, so the debt stays owed; and it may
    // have put part of the event on the wire, so the event is never rebuilt.
    assert_eq!(
        private.terminal.settling[1].outcome_seen(),
        Some(XAuthorityInputDeliveryOutcome::WriteFailed),
        "the cause is kept apart from what it settled"
    );
    assert_eq!(
        private.terminal.settling[1].dispatch(),
        PrivateDispatchPhase::Unrepeatable,
        "and the capsule is never offered again"
    );
    assert!(
        private.terminal.settling[1].attempt().is_none(),
        "the attempt was finished, so the ledger's slot is free for others"
    );
}

#[test]
fn an_attempt_that_cannot_be_placed_is_given_back_and_keeps_its_capsule() {
    // A claim this executor cannot place is an unused reservation. It goes
    // back with neither bit -- the debt is exactly as owed as before -- and
    // the capsule stays here, because the event was decided at a moment that
    // has passed and cannot be rebuilt.
    let client = XServerFrontendClientId(2471);
    // Taken by value: this control drops the recipient's routes on purpose,
    // so it has to own them rather than borrow them from a fixture that
    // outlives them.
    let PreparedOrderedFixture {
        mut runner,
        ingress,
        registration,
        channels,
        durable,
        surface,
        window: _window,
        selections: _selections,
        client: _client,
        namespace: _namespace,
        deliveries: _deliveries,
        _acks,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2471u64, true), (2472u64, false)] {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    assert_eq!(private.terminal.settling.len(), 1);
    // THE PRESS THIS RELEASE ENDS GOES FIRST. It is the earlier event on the
    // same connection, so the release cannot overtake it.
    assert!(matches!(
        private.deliver_one(&mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Dispatched {
            enqueued: true,
            relinquished: false
        }
    ));

    assert!(matches!(
        private.deliver_one(&mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Recorded { recorded: true }
    ));

    // The recipient's routes go. Nothing this executor holds changes: the
    // release is still owed and its event is still the one that was decided.
    drop(registration);
    drop(channels);

    let step = private.deliver_one(&mut |_, _| Ok(())).expect("a step");
    assert!(
        matches!(
            step,
            PrivateDeliveryStep::Dispatched {
                enqueued: false,
                ..
            }
        ),
        "the attempt could not be placed, so nothing was enqueued"
    );
    assert!(
        private.terminal.attempt_custody.is_none(),
        "and the ledger's slot was given back rather than held for a delivery \
         nobody will make"
    );
    assert!(
        private.terminal.settling[0].attempt().is_none(),
        "the record stops naming an attempt once the ledger confirmed it back"
    );
    assert!(
        !private.terminal.is_empty(),
        "the release itself is still owed"
    );
    drop(durable);
    drop(_acks);
}

#[test]
fn a_release_that_cannot_build_does_not_hide_another_recipients() {
    // Choosing the record before asking the ledger meant one release that can
    // never produce a capsule sat at the front and returned every visit
    // before the ledger's cursor moved. Everything behind it was hidden for
    // ever. The ledger selects now, so its cursor is what carries the visit
    // past a release this executor cannot serve.
    //
    // ACROSS RECIPIENTS, which is the only place the claim can be made. On one
    // connection a release that cannot be built DOES hide what is behind it,
    // and must: the events after it are owed to the same client in order, and
    // handing one over early would show it a button going down before the one
    // it was already owed came up.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(2461));
    attempt_release(&mut f, 2461, 272);

    let other = XServerFrontendClientId(2462);
    let other_window = XResourceId::new(0x302462, 1);
    let (other_registration, _other_channels) = {
        let p = f.runner.frontend.as_mut().unwrap();
        let registry = &p.broker.registry;
        let context = namespaced(other, f.namespace);
        let (registration, channels) = registry
            .register_client_with_admission(other, Some(context))
            .unwrap();
        registry.attach_private_lifecycle(&registration, context).unwrap();
        let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry
            .attach_connection_state(
                &registration,
                f.namespace,
                selected.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap();
        {
            let mut state = selected.lock().unwrap();
            state.register(
                other_window,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                Rect { x: 0, y: 0, width: 200, height: 100 },
            );
            state.observe_mapped(other_window);
            state.update(other_window, Some((1 << 2) | (1 << 3)), None);
        }
        let mut grabs = registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace, f.client.raw());
        grabs
            .grab_pointer(
                f.namespace,
                crate::XActiveInputGrab {
                    owner: other.raw(),
                    window: other_window,
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
        (registration, channels)
    };
    attempt_release(&mut f, 2463, 273);

    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.terminal.settling.len(), 2, "two releases are owed");
    assert_eq!(private.terminal.settling[0].reached().client(), f.client);
    assert_eq!(private.terminal.settling[1].reached().client(), other);

    // The first can never produce a capsule: its event is taken away, which
    // is what an unwrappable or already-consumed emission amounts to here.
    let stolen = private.terminal.settling[0]
        .native_mut()
        .and_then(private_native::Hold::take_release_emission);
    assert!(stolen.is_some(), "the first release did have an event to lose");
    drop(stolen);

    // Drive terminal visits. The first release can never be served; the
    // second, owed to a different connection, must still get there.
    for _ in 0..24 {
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("a terminal step");
    }

    assert_eq!(
        private.terminal.settling[1].dispatch(),
        PrivateDispatchPhase::Enqueued,
        "the other recipient's release still reached it"
    );
    assert!(
        private.terminal.settling[1].attempt().is_some(),
        "and it is named by the attempt it was delivered under"
    );
    assert_eq!(
        private.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Untaken,
        "while the one that cannot be built is still owed, not abandoned"
    );
    drop(other_registration);
}

#[test]
fn an_interrupted_handover_is_not_retried_just_because_its_slot_is_empty() {
    // An empty pending slot means one of two opposite things: nothing was
    // taken yet, or a handover began and never reported. Only the phase tells
    // them apart, and retrying the second would send an event the recipient
    // may already have. This fails closed.
    let client = XServerFrontendClientId(2451);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    for (delivery, pressed) in [(2451u64, true), (2452u64, false)] {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        private.terminal.delivering.extend(turn);
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("the entry delivers");
    }
    assert_eq!(private.terminal.settling.len(), 1);

    // THE PRESS THIS RELEASE ENDS GOES FIRST. It is the earlier event on the
    // same connection, so the release cannot overtake it.
    assert!(matches!(
        private.deliver_one(&mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Dispatched {
            enqueued: true,
            relinquished: false
        }
    ));

    // Its proof goes in, which is what makes it eligible for an attempt.
    assert!(matches!(
        private.deliver_one(&mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Recorded { recorded: true }
    ));

    // Stage exactly what an interrupted handover leaves behind: the capsule
    // gone from the slot and the phase saying the handover was begun. Nothing
    // else about the record is touched.
    stage_interrupted_handover(&mut private.terminal.settling[0]);
    assert!(
        settling_slot_is_empty(&private.terminal.settling[0]),
        "the slot is empty, which is the trap this control is about"
    );

    // FAILS CLOSED. No attempt is made, and the turn reports nothing to do
    // rather than inventing work from an absence.
    assert!(matches!(
        private.deliver_one(&mut |_, _| Ok(())).expect("a step"),
        PrivateDeliveryStep::Idle
    ));
    assert!(
        private.terminal.settling[0].attempt().is_none(),
        "no attempt was claimed for a delivery nobody can say was not received"
    );
    assert_eq!(
        private.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Indeterminate,
        "and the phase still says so rather than being cleared by looking"
    );
}

#[test]
fn a_proof_recording_visit_is_charged_and_watched_like_any_other_step() {
    // A visit that returned before the charge hook would be work the service
    // never admitted and the supervisor never saw -- unbudgeted, unwatched,
    // and invisible to the accounting that bounds every other terminal step.
    let client = XServerFrontendClientId(2431);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // The press goes through the wrapper; the release is driven by hand so
    // the visit that follows it can be observed being charged.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2431),
            272,
            true,
        ))
        .expect("the order to accept the press");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2432),
            272,
            false,
        ))
        .expect("the order to accept the release");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert_eq!(turn.len(), 1, "the release is ready to deliver");
    // Installed by hand rather than through the wrapper, because the wrapper
    // charges nothing and would spend the visit this control is watching for.
    private.terminal.delivering.extend(turn);

    let charged: std::cell::RefCell<Vec<Option<crate::ReadySequence>>> =
        std::cell::RefCell::new(Vec::new());
    let mut charge = |sequence: Option<crate::ReadySequence>, _: std::time::Instant| {
        charged.borrow_mut().push(sequence);
        Ok(())
    };
    // One step to deliver the release itself, charged under its own entry.
    assert!(matches!(
        private.deliver_one(&mut charge).expect("a step"),
        PrivateDeliveryStep::Advanced { .. }
    ));
    assert!(
        charged.borrow()[0].is_some(),
        "an ordered entry charges under its own sequence"
    );
    assert_eq!(
        private.terminal.settling.len(),
        1,
        "and its release is owed a recording"
    );
    charged.borrow_mut().clear();

    // The next step has nothing left to deliver, so it spends a recording
    // visit -- and asks to be charged for it first.
    let step = private.deliver_one(&mut charge).expect("a step");
    assert!(
        matches!(step, PrivateDeliveryStep::Recorded { recorded: true }),
        "the visit recorded the release's native bit"
    );
    assert_eq!(
        charged.borrow().as_slice(),
        &[None],
        "and it asked to be charged first, naming no ordered entry because it \
         is not one"
    );

    // With the proof in, the same release owes a delivery attempt. It is
    // native work too, and it charges under None for the same reason.
    charged.borrow_mut().clear();
    assert!(matches!(
        private.deliver_one(&mut charge).expect("a step"),
        PrivateDeliveryStep::Dispatched { .. }
    ));
    assert_eq!(
        charged.borrow().as_slice(),
        &[None],
        "a delivery attempt is charged before it is made, naming no ordered \
         entry because it is not one"
    );

    // With nothing left owed, an empty order is idle again and costs nothing.
    let before = charged.borrow().len();
    assert!(matches!(
        private.deliver_one(&mut charge).expect("a step"),
        PrivateDeliveryStep::Idle
    ));
    assert_eq!(
        charged.borrow().len(),
        before,
        "an idle turn with nothing owed charges nothing"
    );
}

#[test]
fn a_release_proof_is_recorded_by_service_rather_than_by_the_next_input() {
    // The blocker this control exists for: proof recording used to happen as
    // incidental work inside a successful input execution, so a release whose
    // proof was still owed only progressed when somebody submitted ANOTHER
    // input. An idle session never finished it. Recording is terminal work
    // now, and this proves it can be asked for with an empty order.
    let client = XServerFrontendClientId(2421);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;

    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2421),
            272,
            true,
        ))
        .expect("the order to accept the press");
    for _ in 0..4 {
        runner.service_turn().expect("a serviceable turn");
    }
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2422),
            272,
            false,
        ))
        .expect("the order to accept the release");

    // NOTHING MORE IS SUBMITTED FROM HERE. Every recording counted below is
    // done by service turns alone.
    let mut recorded = 0;
    for _ in 0..12 {
        recorded += runner
            .service_turn()
            .expect("a serviceable turn")
            .recorded;
    }
    let private = runner.frontend.as_ref().expect("a live runner");
    assert_eq!(
        private.terminal.settling.len(),
        1,
        "the release is owed and its record carries the obligation"
    );
    assert!(
        private.terminal.settling[0].native_recorded(),
        "service reached the obligation with no further input submitted"
    );
    // ONE visit, ONE entry. Not a sweep, and not repeated against an entry
    // already recorded.
    assert_eq!(
        recorded, 1,
        "exactly one visit put the bit in, and nothing revisited it after"
    );
    // WHAT THIS CONTROL DOES NOT PROVE, stated rather than implied: the
    // recording happens during the same run of turns that delivers the
    // release, so this does not by itself separate "service did it" from
    // "delivering it did it". What separates them is that removing the
    // terminal visit -- the only thing that calls the recording now -- fails
    // this control and the retained-debt control with it.
}

#[test]
fn a_final_release_carries_its_source_obligation_instead_of_dropping_it() {
    // The press raises real native state: an implicit activation, a query
    // scope and a selection, all owned by the hold and reachable only through
    // the exact connection it retained. A release that removed the record and
    // left the hold behind would retire none of them and leave nothing able
    // to -- the obligation would still be live with no owner.
    let client = XServerFrontendClientId(2411);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        channels,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");

    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2411),
            272,
            true,
        ))
        .expect("the order to accept the press");
        let press_cell = admitted_cell(private, 2411);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(2411)
    );
    assert_eq!(capsule.client(), client);
    assert!(
        private.terminal.holds[0].native.is_some(),
        "the press left a source obligation on its record"
    );

    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(2412),
            272,
            false,
        ))
        .expect("the order to accept the release");
        let release_cell = admitted_cell(private, 2412);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(
        delivered.len(),
        1,
        "the entry was disposed of and its own outcome reported"
    );
    let capsule = inbox
        .accepted(private, &channels.ordered, &release_cell, 8)
        .expect("a readable terminal step")
        .expect("the release owes its recipient an event");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(2412)
    );
    assert_eq!(capsule.client(), client);
    assert!(
        private.terminal.holds.is_empty(),
        "the hold ended, so its record is gone"
    );
    assert_eq!(
        private.terminal.settling.len(),
        1,
        "and its release is owed to whoever settles it"
    );
    assert!(
        private.terminal.settling[0].native().is_some(),
        "the obligation travelled with the release rather than dying with the \
         record: nothing else holds the connection its activation, query scope \
         and selection belong to"
    );
}

#[test]
fn a_final_release_clears_what_its_press_projected_and_reports_it() {
    let client = XServerFrontendClientId(761);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(761), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,
        
                watch,
            )
        .expect("the press to run");
    assert!(run.first_press);
    let _ = pressed.observe();
    assert_eq!(
        projected_buttons(private, namespace, seat),
        256,
        "the press projects button one"
    );

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(762), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,
        
                watch,
            )
        .expect("the release to run");
    let Some(XAuthorityInputEvent::Pointer(event)) = run.event else {
        panic!("a final release owes an event");
    };
    assert!(
        matches!(
            event.kind,
            XAuthorityPointerEventKind::Button {
                button: 1,
                pressed: false
            }
        ),
        "it lifts button one"
    );
    assert_eq!(
        event.state, 256,
        "and reports the state before it, which still has that button down"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0,
        "the projection is cleared by the release, not left held"
    );
    let _ = released.observe();

    // A new press on the same input is refused until the release that just
    // happened is settled. The authority holds a barrier for it, and this path
    // has no settlement step yet -- so the projection being clean is what can
    // be shown here, and the barrier is named rather than worked around.
    let again = role.reserve(stamp, 3).expect("a reservation").accepted();
    let barred = private.run_ordered_input(
        keyboards,
        &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(763), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
        &again,
    
                watch,
            );
    assert!(
        matches!(
            barred,
            // Renamed, not reclassified: the source's press carries the
            // authority's own ReleaseBarrier out under Refusal::Authority,
            // so the barrier is still the named cause -- one layer in,
            // because the press is where the authority is now entered.
            Err(crate::PrivateExecutionRefusal::Native(
                private_native::Refusal::Authority(
                    sophia_input_authority::RegistrationError::ReleaseBarrier
                )
            ))
        ),
        "the release's debt bars the next press until it is settled, got {barred:?}"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0,
        "and the projection stayed clear, so no stale bit is hiding behind it"
    );
}

#[test]
fn a_release_with_a_survivor_leaves_the_projection_alone() {
    let client = XServerFrontendClientId(771);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    // Two presses on one input: the second joins, so two participants hold it.
    for (request, delivery) in [(1, 771), (2, 772)] {
        let custody = role.reserve(stamp, request).expect("a reservation").accepted();
        private
            .run_ordered_input(
                keyboards,
                &{
                let route = button_to(
                    surface,
                    XAuthorityInputDeliveryId::from_raw(delivery),
                    272,
                    true,
                );
                admit_for_direct_run(private, &route);
                route
            },
                &custody,
            
                watch,
            )
            .expect("the press to run");
        let _ = custody.observe();
    }
    assert_eq!(projected_buttons(private, namespace, seat), 256);

    // One release. Whether the aggregate is now clear is the ledger's to say,
    // and the projection must not be cleared while anyone still holds it.
    let released = role.reserve(stamp, 3).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(773), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,
        
                watch,
            )
        .expect("the release to run");
    match run.release.expect("a release outcome") {
        sophia_input_authority::ReleaseOutcome::SurvivorRemains => {
            assert!(run.event.is_none(), "a survivor owes nobody an event");
            assert_eq!(
                projected_buttons(private, namespace, seat),
                256,
                "and the button somebody still holds stays projected"
            );
        }
        sophia_input_authority::ReleaseOutcome::DeliverTo(_) => {
            assert_eq!(
                projected_buttons(private, namespace, seat),
                0,
                "a final release clears it"
            );
        }
        sophia_input_authority::ReleaseOutcome::NotHeld => {
            panic!("the input was held")
        }
    }
}

#[test]
fn a_ledger_owed_release_without_its_plan_refuses_rather_than_reporting_nothing() {
    let client = XServerFrontendClientId(781);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(781), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,
        
                watch,
            )
        .expect("the press to run");
    let _ = pressed.observe();

    // The record of where the press went is lost. The ledger still ends the
    // hold, so somebody is owed the event that lifts the button -- and saying
    // "nothing to emit" would settle that debt by losing the evidence of it.
    private.terminal.holds.clear();

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let refused = private.run_ordered_input(
        keyboards,
        &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(782), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
        &released,
    
                watch,
            );
    assert!(
        matches!(refused, Err(crate::PrivateExecutionRefusal::HoldPlanMissing)),
        "a hold that ended has a recipient; not knowing who is not the same as owing nobody, got {refused:?}"
    );
}

#[test]
fn a_release_whose_seat_projection_is_gone_retains_a_residual_rather_than_refusing() {
    let client = XServerFrontendClientId(791);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let role = private
        .reservation_role(client, DeviceId::from_raw(1))
        .expect("a capability");
    let stamp = private.control_gate().stamp().expect("an open coordinator");

    let pressed = role.reserve(stamp, 1).expect("a reservation").accepted();
    private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(791), 272, true);
                admit_for_direct_run(private, &route);
                route
            },
            &pressed,
        
                watch,
            )
        .expect("the press to run");
    let _ = pressed.observe();

    // The projection this press moved is gone. Building a fresh mapper here
    // would assert a clear history -- that the button was never down -- and
    // report a release against state that never held it.
    private
        .broker
        .registry
        .pointer_state
        .lock()
        .expect("the pointer state")
        .remove(&(namespace, seat));

    let released = role.reserve(stamp, 2).expect("a reservation").accepted();
    let run = private
        .run_ordered_input(
            keyboards,
            &{
                let route = button_to(surface, XAuthorityInputDeliveryId::from_raw(792), 272, false);
                admit_for_direct_run(private, &route);
                route
            },
            &released,
            watch,
        )
        // OLD CLAIM: the whole release is refused, because retained state that
        //   has become unavailable is not a fresh clear history.
        // NEW CLAIM: a missing mapper after DeliverTo is a POST-EFFECT
        //   RESIDUAL, not a refusal of the release. The aggregate release
        //   already happened in the ledger, and refusing here would discard a
        //   transition that had occurred rather than prevent one.
        // The original concern is unchanged and still asserted below: nothing
        // rebuilds the mapper, so no clear history is ever asserted.
        .expect("the aggregate release to occur despite the missing projection");

    // The aggregate release occurred, and the ledger says whose.
    assert!(
        matches!(
            run.release,
            Some(sophia_input_authority::ReleaseOutcome::DeliverTo(_))
        ),
        "the hold ended and its delivery was decided"
    );
    assert_eq!(private.terminal.settling.len(), 1);

    // The required event remains UNBUILT, and says why. Owed and absent is
    // not the same as never owed: a reader finding an empty slot with no
    // cause could not tell this release from one that owed nothing.
    assert!(run.owes_event, "the recipient is still owed its release event");
    assert!(run.event.is_none(), "and it could not be built");
    assert_eq!(
        private.terminal.settling[0].unbuilt(),
        Some(PrivateAppliedRefusal::Interrupted),
        "the cause travels with the debt rather than being flattened away"
    );

    // The exact hold is retained against what actually went missing.
    assert!(
        matches!(
            private.terminal.settling[0]
                .native()
                .expect("the release carries its source obligation")
                .status(),
            private_native::Status::Retained(private_native::Residual::MissingMapper)
        ),
        "the obligation is retained naming the projection that is gone"
    );

    // AND THE MAPPER IS NOT RECREATED. This is the whole of the original
    // concern: a fresh mapper would assert the button was never down.
    assert!(
        !private
            .broker
            .registry
            .pointer_state
            .lock()
            .expect("the pointer state")
            .contains_key(&(namespace, seat)),
        "nothing rebuilt the projection, so no clear history is asserted"
    );

    // Neither half is inferred. There is no proof -- the release ended in a
    // residual -- so nothing recorded the native bit, and no receipt has
    // arrived to settle the recipient's.
    assert!(
        !private.terminal.settling[0].native_recorded(),
        "a residual produces no proof, so the native half stays owed"
    );
    let mut cursor = 0;
    let reported = private
        .authority()
        .under_common(|authority| authority.next_debt(&mut cursor))
        .expect("the authority to be readable")
        .expect("a debt for the release that just happened");
    assert!(
        !reported.1.native_reconciled,
        "the native half is not inferred from the release having happened"
    );
    assert!(
        !reported.1.recipient_settled,
        "and neither is the recipient's"
    );
}

#[test]
fn work_sent_through_the_ingress_runs_from_the_order_it_was_accepted_into() {
    let client = XServerFrontendClientId(801);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    // The producer handle, with the reservation role bound to this client.
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");

    // Sent through the producer. Nothing here builds custody by hand: the
    // reservation is made at submission, travels on the envelope, and is what
    // the consumer runs against.
    let sequence = ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(801),
            272,
            true,
        ))
        .expect("the order to accept it");

    let mut turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert_eq!(turn.len(), 1, "the order held exactly what was sent");
    let item = turn.remove(0);
    let PrivateOrderedItem::Ran {
        sequence: ran_sequence,
        run,
        custody,
        route,
    } = item
    else {
        panic!("the queued work ran");
    };
    assert_eq!(
        ran_sequence, sequence,
        "and it is the same place in the order the producer was given"
    );
    assert_eq!(
        route.delivery,
        Some(XAuthorityInputDeliveryId::from_raw(801)),
        "the accepted work comes back with it, delivery identity and all"
    );
    let reached = run.reached.expect("a press decides where it went");
    assert_eq!(reached.client(), client);
    assert_eq!(reached.window(), window);
    assert!(run.first_press);
    assert!(run.event.is_some(), "a first press owes an event");

    // The custody came back rather than being dropped inside the turn, so the
    // outcome is still there to take.
    assert!(matches!(
        custody.observe(),
        Ok(Some(sophia_input_authority::RequestCompletion::Processed))
    ));

    // The order is empty now: the turn consumed it rather than copying it.
    assert!(
        private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order")
            .is_empty()
    );
}

#[test]
fn a_consumer_refusal_hands_back_the_custody_it_was_accepted_with() {
    let client = XServerFrontendClientId(811);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");

    // A key: accepted by the order, refused by the consumer because no applied
    // focus can name where it would go.
    let mut key = motion_to(surface, XAuthorityInputDeliveryId::from_raw(811));
    key.request.kind = InputEventKind::Key {
        keycode: 30,
        pressed: true,
    };
    ingress.submit(key).expect("the order to accept it");

    let mut turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert_eq!(turn.len(), 1);
    let PrivateOrderedItem::Refused {
        sequence,
        refusal,
        custody,
        route,
    } = turn.remove(0)
    else {
        panic!("the consumer refused this one");
    };
    assert!(
        sequence.raw() > 0,
        "the refusal names the place in the order the work held"
    );
    assert!(matches!(
        refusal,
        crate::PrivateExecutionRefusal::FocusNotApplied
    ));
    assert_eq!(
        route.request.target_surface, surface,
        "the work it was accepted for comes back whole"
    );

    // And so does the request the order took. A consumer declining to run
    // something is not the order never having accepted it, so the custody is
    // still here to be settled rather than erased by the decision.
    assert!(
        matches!(custody.observe(), Ok(None)),
        "nothing ran, so there is no outcome yet -- but the right to take one survived"
    );
}

#[test]
fn unreserved_work_in_the_order_is_handed_back_rather_than_run() {
    let client = XServerFrontendClientId(821);
    let surface = SurfaceId::new(821, 1);
    let private = private_for_roles();
    let _registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200821, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");

    // The plain ingress reserves nothing, so this reaches the order without a
    // request behind it. The ordered path runs what was reserved before it was
    // published; something else accepted this, and running it would execute
    // against a request that does not exist.
    private
        .ingress()
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(821),
            272,
            true,
        ))
        .expect("the order to accept it");

    let mut private = private;
    let mut turn = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert_eq!(turn.len(), 1);
    let PrivateOrderedItem::Parked { sequence } = turn.remove(0) else {
        panic!("work this path does not execute is parked, not run");
    };
    assert!(sequence.raw() > 0);
    assert_eq!(
        private.parked(),
        Some(sequence),
        "the report names it and the operation stays owned until something takes it"
    );

    // Still parked, so a later turn runs nothing rather than overtaking it.
    let again = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(
        matches!(again.as_slice(), [PrivateOrderedItem::Parked { .. }]),
        "a blocked turn says so rather than looking like an empty one"
    );

    // Taking it is what accepts responsibility, and it comes back whole.
    let (taken_sequence, operation) = private.take_parked().expect("the parked operation");
    assert_eq!(taken_sequence, sequence);
    assert!(matches!(operation, PrivateOperation::RoutedInput(_)));
    assert_eq!(private.parked(), None);

    // Nothing was pressed, so no hold was recorded against it.
    assert!(private.terminal.holds.is_empty());
}

#[test]
fn no_input_applies_past_an_earlier_operation_that_has_not_run() {
    let client = XServerFrontendClientId(831);
    let surface = SurfaceId::new(831, 1);
    let mut private = private_for_roles();
    let _registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200831, 1),
        )
        .expect("the surface to register");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");
    let mut keyboards = private.keyboards().expect("this instance's state");

    // A control first, then input. The control carries its own accepted
    // completion registration and this path does not execute it.
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(8310),
                surface,
            },
        })
        .expect("the order to accept the control");
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(831),
            272,
            true,
        ))
        .expect("the order to accept the input");

    let mut private = private;
    let turn = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");

    // The report being in order is not enough. The effect order is what
    // matters: the button must not have applied while an earlier operation has
    // neither executed nor been cancelled.
    assert!(
        matches!(turn.as_slice(), [PrivateOrderedItem::Parked { .. }]),
        "the turn stops at the earlier operation rather than running past it"
    );
    assert!(
        private.terminal.holds.is_empty(),
        "and no later hold was applied behind it"
    );

    // A second turn does not overtake it either. Stopping for one turn would
    // only move the problem to the next.
    let again = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(matches!(
        again.as_slice(),
        [PrivateOrderedItem::Parked { .. }]
    ));
    assert!(private.terminal.holds.is_empty());

    // Handing the operation to an owner does not lift the barrier. Taking it
    // moves it; it does not establish what becomes of it, and an owner that
    // took it and then dropped it has answered nothing. Until a path exists
    // that executes or cancels such an operation, the order stays blocked --
    // which is the honest state rather than a convenient one.
    let (_, parked) = private.take_parked().expect("the parked control");
    assert!(matches!(parked, PrivateOperation::Control(_, _)));
    let after = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(
        matches!(after.as_slice(), [PrivateOrderedItem::Parked { .. }]),
        "holding the operation is not having answered for it"
    );
    assert!(
        private.terminal.holds.is_empty(),
        "so no later hold applied behind it"
    );
    assert!(
        private.blocked().is_some(),
        "and the order says it is still blocked"
    );
}

#[test]
fn the_older_route_refuses_an_order_the_ordered_consumer_is_draining() {
    let client = XServerFrontendClientId(841);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");

    // The ordered consumer takes a turn, which claims this order.
    assert!(
        private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order")
            .is_empty()
    );

    // Work is accepted with a reservation made for it.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(841),
            272,
            true,
        ))
        .expect("the order to accept it");

    // The older route discards the reservation and applies without the
    // execution it exists for, so it must not drain this order alongside.
    let refused = private.route_pending();
    assert!(
        matches!(
            refused,
            Err(XServerFrontendRouteError::OrderedRunnerEngaged)
        ),
        "one permitted consumer per order, got {refused:?}"
    );

    // And the work is still there for the consumer that may run it.
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(
        matches!(turn.as_slice(), [PrivateOrderedItem::Ran { .. }]),
        "the refused drain took nothing away from the order"
    );
}

#[test]
fn a_turn_that_fails_part_way_keeps_what_it_already_took() {
    let client = XServerFrontendClientId(851);
    let surface = SurfaceId::new(851, 1);
    let private = private_for_roles();
    let _registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200851, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");
    let mut private = private;

    // A real sequence from a real submission, so nothing here invents an
    // identity the order never issued.
    private
        .ingress()
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(851),
            272,
            true,
        ))
        .expect("the order to accept it");
    let parked = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    let [PrivateOrderedItem::Parked { sequence }] = parked.as_slice() else {
        panic!("unreserved work parks");
    };
    let sequence = *sequence;
    let _ = private.take_parked();
    // Cleared here only to reach the case under test. Nothing in production
    // lifts this yet, which is the point of the control above; this one is
    // about what a failed turn keeps, not about when the order resumes.
    private.parked_barrier = None;

    // Staged as an earlier iteration leaves it: work already taken out of the
    // order and recorded, with the turn still in progress. Staged rather than
    // raced, because making the queue fail between two real iterations is not
    // something a test can arrange deterministically.
    private.terminal.turn.push(PrivateOrderedItem::Parked { sequence });

    // The order becomes unreadable.
    let ready = std::sync::Arc::clone(&private.admission.ready);
    assert!(
        std::thread::spawn(move || {
            let _guard = ready.lock().unwrap();
            panic!("poisoning the order");
        })
        .join()
        .is_err()
    );

    let failed = private.route_pending_ordered(&mut keyboards, &control_watchdog());
    assert!(failed.is_err(), "the turn could not read the order");

    // What it had already taken is still owned. Returning results only on
    // success would drop every earlier iteration's work on a later one's
    // failure, and that work has left the order -- nothing else holds it.
    let recovered = private.take_interrupted_turn();
    assert_eq!(
        recovered.len(),
        1,
        "the interrupted turn's items survived the failure"
    );
    assert!(matches!(
        recovered.as_slice(),
        [PrivateOrderedItem::Parked { .. }]
    ));
    assert!(
        private.take_interrupted_turn().is_empty(),
        "and recovering them twice yields nothing the second time"
    );
}

#[test]
fn queuing_an_event_is_not_the_receipt_that_closes_a_release_debt() {
    let client = XServerFrontendClientId(861);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, ingress, channels, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    // Registered with channels held, so a delivered event has somewhere to go.

    // Press, run, deliver.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(861),
            272,
            true,
        ))
        .expect("the order to accept the press");
    let press_cell = admitted_cell(private, 861);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(delivered.len(), 1);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the press was accepted onto the client's queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(861)
    );
    assert_eq!(capsule.client(), client);
    assert!(
        matches!(
            delivered[0].completion,
            Some(sophia_input_authority::RequestCompletion::Processed)
        ),
        "and its outcome was taken exactly once, which is what freed the cell"
    );
    assert!(
        !delivered[0].debt_settled,
        "a press creates no release debt to close"
    );
    assert!(
        inbox.taken.is_empty() && channels.ordered.try_recv().is_err(),
        "and nothing else was put on that queue"
    );
    // The window is the source's own resolution, which the capsule carries in
    // its encoded frames rather than exposing as a field.
    assert_eq!(private.terminal.holds[0].reached.window(), window);

    // Release, run, deliver. Queuing establishes neither half: the recipient
    // half is the writer's outcome, and what the guarded code shows is that
    // the aggregate transition and the projection it moves happen in one
    // interval -- which is not the whole of native reconciliation.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(862),
            272,
            false,
        ))
        .expect("the order to accept the release");
        let release_cell = admitted_cell(private, 862);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(delivered.len(), 1);
    let capsule = inbox
        .accepted(private, &channels.ordered, &release_cell, 8)
        .expect("a readable terminal step")
        .expect("the release was queued too");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(862)
    );
    assert_eq!(capsule.client(), client);
    assert!(
        delivered[0].sequence.raw() > 0,
        "each delivery names the place in the order it came from"
    );
    assert!(
        !delivered[0].debt_settled,
        "but no debt is closed by queuing: the recipient half is the writer's \
         outcome, and nothing here has observed one"
    );
    assert!(
        inbox.taken.is_empty() && channels.ordered.try_recv().is_err(),
        "and the release was the only thing behind it"
    );
    assert!(
        !private.terminal.settling.is_empty(),
        "so the continuation is retained, because the obligation is still open"
    );

    // And the same input still cannot press again. The release barrier stands
    // until the debt is genuinely closed, which needs a receipt this path
    // cannot yet obtain -- so the honest state is barred, not resumed.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(863),
            272,
            true,
        ))
        .expect("the order to accept the second press");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let [PrivateOrderedItem::Refused { refusal, .. }] = turn.as_slice() else {
        panic!("the second press is barred while the debt is open");
    };
    assert!(
        matches!(
            refusal,
            // Renamed, not reclassified: the authority's own ReleaseBarrier
            // travels out of the source press under Refusal::Authority.
            crate::PrivateExecutionRefusal::Native(
                private_native::Refusal::Authority(
                    sophia_input_authority::RegistrationError::ReleaseBarrier
                )
            )
        ),
        "with the authority's own barrier, got {refusal:?}"
    );
}

#[test]
fn a_refusal_is_retained_by_delivery_rather_than_discarded() {
    let client = XServerFrontendClientId(871);
    let surface = SurfaceId::new(871, 1);
    let mut private = private_for_roles();
    let _registration = admit_role_client(&private, client);
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200871, 1),
        )
        .expect("the surface to register");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");
    let mut keyboards = private.keyboards().expect("this instance's state");

    let mut key = motion_to(surface, XAuthorityInputDeliveryId::from_raw(871));
    key.request.kind = InputEventKind::Key {
        keycode: 30,
        pressed: true,
    };
    ingress.submit(key).expect("the order to accept it");

    let turn = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert!(
        delivered.is_empty(),
        "a refusal is not a delivery, so it reports none"
    );

    // And it is not nothing either. Dropping it destroyed the custody the
    // order accepted, so the same request answered stale afterwards instead of
    // saying no outcome had been taken.
    assert_eq!(
        private.terminal.undelivered.len(),
        1,
        "the refusal is retained whole, with its custody"
    );
    let [PrivateUndelivered {
        item: PrivateOrderedItem::Refused { custody, .. },
    }] = private.terminal.undelivered.as_slice()
    else {
        panic!("retained as the refusal it was");
    };
    assert!(
        matches!(custody.observe(), Ok(None)),
        "no outcome was taken, which is a different answer from a stale request"
    );
}

#[test]
fn a_later_turn_does_not_overwrite_an_unresolved_current_item() {
    let client = XServerFrontendClientId(881);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");

    // Staged as an interruption before the effect leaves it: an item taken
    // from the order, owned, with execution not attempted.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(881),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let [PrivateOrderedItem::Ran { .. }] = turn.as_slice() else {
        panic!("the first item ran");
    };
    let taken = turn.into_iter().next().expect("the item");
    private.terminal.current = Some(taken);

    // More work arrives, from another producer: the first one's cell is still
    // busy, because the outcome of the item now held was never taken. A turn
    // that dequeued into the same slot would overwrite the only record of an
    // item already taken, whose application nobody can describe.
    let other = XServerFrontendClientId(882);
    let _other_registration = admit_role_client(private, other);
    private
        .broker
        .registry
        .register_surface(
            other,
            NamespaceId::from_raw(other.raw()),
            SurfaceId::new(882, 1),
            XResourceId::new(0x200882, 1),
        )
        .expect("the second surface to register");
    private
        .ingress_for(other, DeviceId::from_raw(2))
        .expect("a second ingress")
        .submit(button_to(
            SurfaceId::new(882, 1),
            XAuthorityInputDeliveryId::from_raw(882),
            272,
            true,
        ))
        .expect("the order to accept it");
    let blocked = private.route_pending_ordered(keyboards, watch);
    assert!(
        matches!(
            blocked,
            Err(XServerFrontendRouteError::OrderedItemUnresolved)
        ),
        "the order refuses rather than overwriting it"
    );
    assert!(
        private.terminal.current.is_some(),
        "and the earlier item is still owned"
    );
}

#[test]
fn a_duplicate_that_owes_no_event_still_completes_so_its_hold_can_be_released() {
    let client = XServerFrontendClientId(891);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, ingress, channels, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");

    // The cell is taken where the admission mints it, and travels out with the
    // reports, so every claim below names the admission it came from.
    let run_one_submission = |private: &mut crate::PrivateXServerFrontend,
                                  keyboards: &mut crate::PrivateKeyboards,
                                  delivery: u64,
                                  pressed: bool| {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                272,
                pressed,
            ))
            .expect("the order to accept it");
        let cell = admitted_cell(private, delivery);
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        (private.deliver_turn(turn), cell)
    };

    // A press that begins the hold: an event is owed and delivered.
    let (delivered, press_cell) = run_one_submission(private, keyboards, 891, true);
    assert_eq!(delivered.len(), 1);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the press reached its recipient");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(891)
    );
    assert_eq!(capsule.client(), client);

    // The same input pressed again joins the hold. It owes nobody an event,
    // which is an outcome rather than a failure to emit one -- so its
    // completion is taken and the grant's cell is freed.
    let (delivered, join_cell) = run_one_submission(private, keyboards, 892, true);
    assert_eq!(
        delivered.len(),
        1,
        "a join is reported as what happened, not retained for want of an event"
    );
    assert!(
        inbox
            .accepted(private, &channels.ordered, &join_cell, 4)
            .expect("a readable terminal step")
            .is_none(),
        "nobody was owed one"
    );
    assert!(
        matches!(
            delivered[0].completion,
            Some(sophia_input_authority::RequestCompletion::Processed)
        ),
        "and its outcome was taken, which is what frees the cell"
    );
    assert!(
        private.terminal.undelivered.is_empty(),
        "nothing is owed, so nothing is retained"
    );

    // Which means the hold this grant still owns can be released. Retaining
    // the join would have left the grant unable to reserve, holding a button
    // it could never let go of.
    let (delivered, release_cell) = run_one_submission(private, keyboards, 893, false);
    assert_eq!(delivered.len(), 1, "the release reserved and ran");
    let capsule = inbox
        .accepted(private, &channels.ordered, &release_cell, 8)
        .expect("a readable terminal step")
        .expect("and it owed an event, which was queued");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(893)
    );
    assert_eq!(capsule.client(), client);
}

#[test]
fn a_parked_operation_is_handed_to_the_durable_owner_at_shutdown() {
    let client = XServerFrontendClientId(901);
    let surface = SurfaceId::new(901, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, _receiver) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let (_registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(client, admitted(client))
        .expect("the boundary to admit");
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200901, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");

    // A control the ordered path does not execute, which parks the order.
    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(9010),
                surface,
            },
        })
        .expect("the order to accept the control");
    let turn = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(matches!(
        turn.as_slice(),
        [PrivateOrderedItem::Parked { .. }]
    ));
    assert!(private.parked().is_some());

    let owed_before = durable.owed().expect("a readable owner");

    // Shutdown. The parked operation never ran and carries no custody, so an
    // instance that is going must hand it on rather than take it along: it is
    // work this instance accepted and could not answer, which is exactly what
    // the durable owner holds.
    drop(private.shutdown());
    assert!(
        durable.owed().expect("a readable owner") > owed_before,
        "the parked operation reached the owner rather than dying with the instance"
    );
}

#[test]
fn an_unreadable_observation_retains_its_entry_without_losing_the_event() {
    let client = XServerFrontendClientId(911);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, ingress, channels, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(911),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");

    // The authority becomes unreadable after the execution and before the
    // observation. Nothing is sent on this path, so the window is exactly
    // between the request applying and its completion being read.
    let poisoner = private.authority().clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poisoner.common.lock().unwrap();
            panic!("poisoning the authority");
        })
        .join()
        .is_err()
    );

    let recovery = private.broker.registry.input_recovery.clone();
    let cell = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(911))
        .expect("a readable recovery")
        .expect("the admission minted a cell");

    let delivered = private.deliver_turn(turn);
    assert!(
        delivered.is_empty(),
        "nothing could be reported: the outcome was never established"
    );

    // Retained whole, because the handle able to take that observation later
    // is the item itself. Dropping it would leave the request answerable by
    // nobody.
    assert_eq!(
        private.terminal.undelivered.len(),
        1,
        "the unobservable entry is kept, not discarded"
    );

    // AND THE EVENT IS NOT LOST WITH IT. What could not be read was the
    // request's completion; the event is owed by the record that holds its
    // custody, and that record outlives this entry. So the handover still
    // happens, exactly once, carrying the cell the admission minted.
    let handed: Vec<_> = channels.ordered.try_iter().collect();
    assert_eq!(
        handed.len(),
        1,
        "an unreadable observation is not a reason to drop or repeat the event"
    );
    assert_eq!(
        handed[0].delivery(),
        XAuthorityInputDeliveryId::from_raw(911)
    );
    assert!(
        Arc::ptr_eq(
            &cell,
            &handed[0].finalizer().expect("carried").completion
        ),
        "the exact admission's cell travels with it"
    );
    assert!(
        cell.answer().is_none(),
        "and no answer was invented for a request nobody could read"
    );
}

#[test]
fn a_parked_control_is_answered_exactly_once_after_shutdown() {
    let client = XServerFrontendClientId(921);
    let surface = SurfaceId::new(921, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, acks) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let (_registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(client, admitted(client))
        .expect("the boundary to admit");
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200921, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");

    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(9210),
                surface,
            },
        })
        .expect("the order to accept the control");
    let turn = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(matches!(
        turn.as_slice(),
        [PrivateOrderedItem::Parked { .. }]
    ));

    // The credit this operation holds, read before shutdown. This does not
    // reserve a second one -- an earlier version of this comment said it did,
    // which was false -- so the assertion below is that exactly one credit is
    // returned from a known starting count, not that a duplicate release
    // could not saturate at zero.
    let reserved_before = durable.reserved().expect("a readable owner");
    assert_eq!(
        reserved_before, 1,
        "one accepted operation, one credit held"
    );

    drop(private.shutdown());

    // Exactly one acknowledgement for transaction 9210, whichever path
    // produced it.
    let mut answers = Vec::new();
    while let Ok(ack) = acks.try_recv() {
        answers.push(ack.acknowledgement.transaction);
    }
    let _drive = durable.drive();
    while let Ok(ack) = acks.try_recv() {
        answers.push(ack.acknowledgement.transaction);
    }
    let mine: Vec<_> = answers
        .iter()
        .filter(|transaction| **transaction == TransactionId::from_raw(9210))
        .collect();
    assert_eq!(
        mine.len(),
        1,
        "one operation, one answer -- handing the command on while its record \
         stayed available gave two owners able to publish for it, got {answers:?}"
    );

    // And the credit it held was released once, not twice.
    assert_eq!(
        durable.reserved().expect("a readable owner"),
        reserved_before - 1,
        "exactly one credit returned"
    );
}

#[test]
fn an_accepted_handover_is_observed_rather_than_offered_again() {
    let client = XServerFrontendClientId(941);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, ingress, channels, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(941),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");

    private.terminal.delivering.extend(turn);

    let recovery = private.broker.registry.input_recovery.clone();
    let cell = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(941))
        .expect("a readable recovery")
        .expect("the admission minted a cell");

    assert_eq!(
        channels.ordered.try_iter().count(),
        0,
        "nothing has been handed over by this test yet"
    );

    // A REAL HANDOVER, not a staged phase. The source takes its own emission,
    // builds its own wrapper and the queue actually accepts it -- which is the
    // only thing that can make a second offer a duplicate.
    let delivered = private.deliver_turn(Vec::new());
    let queued: Vec<_> = channels.ordered.try_iter().collect();
    assert_eq!(queued.len(), 1, "the event reached its recipient exactly once");
    assert_eq!(
        queued[0].delivery(),
        XAuthorityInputDeliveryId::from_raw(941)
    );
    assert!(
        Arc::ptr_eq(
            &cell,
            &queued[0].finalizer().expect("carried").completion
        ),
        "carrying the cell this exact admission minted"
    );
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Enqueued,
        "and the custody records that it happened"
    );

    // Observed, never sent again. The entry's own step sends nothing at all,
    // and the custody that could send refuses to: an event already taken by a
    // queue owes only its outcome, and offering it again would deliver the
    // same transition twice with nothing downstream able to tell.
    for _ in 0..8 {
        private
            .deliver_one(&mut |_, _| Ok(()))
            .expect("a readable terminal step");
    }
    assert_eq!(
        channels.ordered.try_iter().count(),
        0,
        "an accepted handover is not repeated"
    );
    assert_eq!(delivered.len(), 1, "and its outcome was taken");
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Enqueued,
        "the phase that says so is not reset by a later visit"
    );
    assert!(
        private.terminal.holds[0].custody.pending.is_none(),
        "and no wrapper is rebuilt for it"
    );
    assert!(cell.answer().is_none(), "the recipient has still not answered");
    assert!(
        matches!(
            delivered[0].completion,
            Some(sophia_input_authority::RequestCompletion::Processed)
        ),
        "which is what frees the grant's cell"
    );
}

#[test]
fn a_parked_control_whose_registry_is_unreadable_is_kept_whole() {
    let client = XServerFrontendClientId(951);
    let surface = SurfaceId::new(951, 1);
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, _acks) = sync_channel(8);
    let (delivery_sender, _delivery_receiver) = channel();
    let (authority, issuer, submit) = private_authority();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let (_registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(client, admitted(client))
        .expect("the boundary to admit");
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200951, 1),
        )
        .expect("the surface to register");
    let mut keyboards = private.keyboards().expect("this instance's state");

    private
        .control_producer()
        .submit(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(9510),
                surface,
            },
        })
        .expect("the order to accept the control");
    let turn = private
        .route_pending_ordered(&mut keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(matches!(
        turn.as_slice(),
        [PrivateOrderedItem::Parked { .. }]
    ));
    assert_eq!(durable.reserved().expect("a readable owner"), 1);

    // The completion registry becomes unreadable before shutdown.
    let completion = private.completion.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = completion.inner.lock().unwrap();
            panic!("poisoning the completion registry");
        })
        .join()
        .is_err()
    );

    drop(private.shutdown());

    // An unreadable registry is not evidence that something else will answer
    // for this command. Asking by a boolean lost it: the same false covers
    // unreadable, foreign, absent and several live phases, and dropping on all
    // of them discarded an accepted command and the credit it held.
    assert_eq!(
        durable.owed().expect("a readable owner"),
        1,
        "the command is kept whole rather than dropped on an unreadable answer"
    );
    assert_eq!(
        durable.reserved().expect("a readable owner"),
        1,
        "and it still holds the credit it was accepted with"
    );
}

#[test]
fn what_an_instance_still_owes_reaches_the_durable_owner() {
    let client = XServerFrontendClientId(961);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture { runner, ingress, durable, registration: _, channels: _, surface, window, .. } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // A press that begins a hold. Its plan is recorded, and the hold is an
    // obligation: a later release answers to what this press reached.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(961),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let _delivered = private.deliver_turn(turn);
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "the hold's plan is owed to whatever releases it"
    );
    let owed = private.terminal.outstanding().expect("readable terminal inventory");
    assert_eq!(owed, 2, "one held input and one live connection cleanup owner");
    let lifecycle = private.terminal.lifecycle.clone();
    assert_eq!(lifecycle.inventory().unwrap().open, 1);

    // Shutdown. The instance can no longer answer, so what it owes travels to
    // the handle rather than dying with it.
    let settlement = frontend.take().expect("a live runner").shutdown();
    assert_eq!(
        settlement.terminal_outstanding().expect("readable terminal inventory"),
        owed - 1,
        "only the completed connection cleanup leaves; the held input remains owed"
    );
    assert_eq!(lifecycle.inventory().unwrap(), PrivateLifecycleInventory::default());
    assert_eq!(
        durable.terminal_inventories().expect("a readable owner"),
        0,
        "and the durable owner has not been given it while a handle still holds it"
    );

    // The handle is abandoned too. Now it goes to the owner behind both,
    // rather than being dropped by the last thing able to pass it on.
    drop(settlement);
    assert_eq!(
        durable.terminal_inventories().expect("a readable owner"),
        1,
        "the obligations reached the owner that outlives both"
    );
}

#[test]
fn a_retained_hold_keeps_the_capabilities_needed_to_answer_it() {
    let client = XServerFrontendClientId(971);
    // Taken by value rather than borrowed: this control drops the producer,
    // the routes and the instance and then asks whether what they were
    // keeping alive is gone. References into a fixture that outlived them
    // would answer that question about the fixture instead.
    let PreparedOrderedFixture {
        mut runner,
        ingress,
        durable,
        registration,
        channels,
        surface,
        window: _,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // Weak handles to the two capabilities a retained hold needs: the seat
    // projection its release must move, and the authority its debt answers to.
    let projection = std::sync::Arc::downgrade(&private.broker.registry.pointer_state);
    let common = std::sync::Arc::downgrade(&private.authority().common);

    // A press that begins a hold, delivered and its completion observed -- so
    // its custody is gone and only the hold plan is left.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(971),
            272,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert!(matches!(
        delivered[0].completion,
        Some(sophia_input_authority::RequestCompletion::Processed)
    ));

    // Everything that might have kept those capabilities alive incidentally
    // goes: the producer, the client's routes and channels, and the instance.
    drop(ingress);
    drop(channels);
    drop(registration);
    let settlement = frontend.take().expect("a live runner").shutdown();
    assert_eq!(
        settlement.terminal_outstanding().expect("readable terminal inventory"),
        1,
        "the hold is still owed"
    );
    assert!(
        !settlement.is_settled(),
        "and a retained obligation is not a settled report"
    );
    // The narrow counters stay narrow, which is why the settled question has
    // to be the aggregate one: a caller reading either of these alone sees an
    // instance with nothing left while it still owes a release.
    assert_eq!(settlement.owed(), 0, "no command is waiting");
    assert_eq!(settlement.outstanding(), 0, "and none is in flight");
    assert!(
        projection.upgrade().is_some(),
        "the seat projection its release must move is still reachable"
    );
    assert!(
        common.upgrade().is_some(),
        "and so is the authority its debt answers to"
    );

    // Handed on to the owner behind the handle, still with both. The
    // capability is asserted before the count: that an obligation was kept
    // somewhere is a weaker claim than that what answers it was kept with it.
    drop(settlement);
    assert!(
        projection.upgrade().is_some(),
        "an inventory that outlived its registry would describe obligations \
         nothing could act on"
    );
    assert!(common.upgrade().is_some());
    assert_eq!(durable.terminal_inventories().expect("readable"), 1);
    // And on the owner's side the same distinction holds: there is nothing to
    // drive, and driving is not what discharges this.
    assert_eq!(durable.owed(), Some(0));
    assert_eq!(durable.outstanding(), Some(0));
    let progress = durable.drive();
    assert!(progress.readable);
    assert!(
        !progress.made_progress(),
        "a drive loop ends here with the hold still owed, so 'no progress' \
         cannot be read as 'nothing owed'"
    );
    assert_eq!(durable.terminal_inventories().expect("readable"), 1);

    // And they go only when the obligations do.
    drop(durable);
    assert!(projection.upgrade().is_none());
    assert!(common.upgrade().is_none());
}

/// One instance that ends owing a hold it can no longer answer for, handed to
/// `durable`. Returns the authority that hold answers to, so a caller can ask
/// which instance a retained inventory kept.
fn instance_handing_over_a_retained_hold(
    durable: &crate::PrivateSettlementOwner,
    client: XServerFrontendClientId,
    surface: SurfaceId,
    namespace: NamespaceId,
    delivery: u64,
    button: u32,
) -> std::sync::Arc<Mutex<sophia_input_authority::AuthorityInstance>> {
    let (sender, _acks) = sync_channel(8);
    let (delivery_sender, _deliveries) = channel();
    let (authority, issuer, submit) = private_authority();
    let private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let common = std::sync::Arc::clone(&private.authority().common);
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(namespaced(client, namespace)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(client, namespaced(client, namespace))
        .expect("the boundary to admit");
    // The source resolves the recipient out of this connection's own
    // selection state, so the window has to exist there and to have selected
    // button events before anything is submitted.
    let window = XResourceId::new(0x200000 | client.raw(), 1);
    let selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    let focused = Arc::new(AtomicU64::new(0));
    private
        .broker
        .registry
        .attach_connection_state(&registration, namespace, selections.clone(), focused.clone())
        .expect("the connection state attaches");
    {
        let mut selected = selections.lock().expect("the selections");
        selected.register(
            window,
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
        );
        selected.observe_mapped(window);
        selected.update(window, Some((1 << 2) | (1 << 3)), None);
    }
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .expect("the surface to register");
    let mut runner = private
        .prepare_runner(namespace)
        .unwrap_or_else(|(cause, _)| panic!("runner refused: {cause:?}"));
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
        runtime.prepare_input_focus_namespace(namespace);
        publication
            .lock()
            .expect("the publication")
            .begin_focus_change()
            .expect("a focus change")
            .apply(&mut runtime, &focused, None)
            .expect("the clear applies");
    }
    let ingress = runner
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(delivery),
            button,
            true,
        ))
        .expect("the order to accept it");
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert!(matches!(
        delivered[0].completion,
        Some(sophia_input_authority::RequestCompletion::Processed)
    ));
    drop(ingress);
    drop(channels);
    drop(registration);
    let settlement = frontend.take().expect("a live runner").shutdown();
    assert_eq!(
        settlement.terminal_outstanding().expect("readable terminal inventory"),
        1,
        "the instance ends owing exactly the hold this control is about"
    );
    drop(settlement);
    common
}

#[test]
fn two_instances_owing_the_same_names_each_answer_through_their_own_origin() {
    // The same surface, in the same namespace, on the same seat, in two
    // unrelated instances. Nothing in the names distinguishes the two
    // obligations; only which registry and which authority accepted each does.
    let surface = SurfaceId::new(981, 1);
    let namespace = NamespaceId::from_raw(500);
    let seat = SeatId::from_raw(1);
    let durable = crate::PrivateSettlementOwner::default();
    // Different buttons, so a projection read through the wrong registry is
    // visible rather than indistinguishable.
    let first = instance_handing_over_a_retained_hold(
        &durable,
        XServerFrontendClientId(981),
        surface,
        namespace,
        981,
        272,
    );
    let second = instance_handing_over_a_retained_hold(
        &durable,
        XServerFrontendClientId(982),
        surface,
        namespace,
        982,
        273,
    );
    assert_eq!(durable.terminal_inventories().expect("readable"), 2);
    assert!(
        !std::sync::Arc::ptr_eq(&first, &second),
        "two instances, two authorities"
    );

    // The clients' routes are gone -- both registrations dropped before
    // shutdown, which is the case retention exists for. What a release still
    // has to move is the seat projection its press raised, and that is the
    // registry's.
    // Reached here rather than through a query on the owner: what each
    // retained inventory kept is a question this control asks, not one the
    // owner needs to answer.
    let held = durable.inner.lock().expect("the owner to be readable");
    let answered = held
        .terminal
        .iter()
        .map(|inventory| {
            let projected = inventory
            .origin()
            .pointer_state
            .lock()
            .expect("its own seat projection")
                .get(&(namespace, seat))
                .map_or(0, |mapper| mapper.state());
            (
                projected,
                std::sync::Arc::ptr_eq(&inventory.controller().common, &first),
                std::sync::Arc::ptr_eq(&inventory.controller().common, &second),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        answered,
        vec![(0x100, true, false), (0x400, false, true)],
        "each retained hold reaches the seat projection its own press raised \
         and the authority its own credit answers to -- reaching the other \
         would release a button this instance never pressed, in an instance \
         that never accepted it"
    );
}

#[test]
fn an_ordered_press_whose_delivery_ended_does_not_execute() {
    let client = XServerFrontendClientId(991);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        durable: _,
        channels,
        surface,
        window,
        ..
    } = &mut fixture;
    let surface = *surface;
    let _window = *window;
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let delivery = XAuthorityInputDeliveryId::from_raw(991);

    ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    assert!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .is_some(),
        "submission tracked a real delivery"
    );

    // It ends before the turn runs: the epoch is revoked while the work is
    // still sitting in the order.
    let revoked = private
        .broker
        .registry
        .input_recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert_eq!(revoked.len(), 1, "the admitted delivery is the one revoked");
    assert_eq!(
        revoked[0].delivery, delivery,
        "and it is the one this control submitted"
    );

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert!(
        delivered.is_empty(),
        "a delivery that ended is owed no event"
    );
    assert!(
        channels.input.try_recv().is_err(),
        "and nothing reached the client's queue"
    );
    assert!(
        private.terminal.holds.is_empty(),
        "the ledger never moved, so there is no hold for a release to answer"
    );
    // Refused for what actually happened. Naming it unmappable, or a target
    // that is gone, would send a reader looking at the route for a cause that
    // is not in the route at all.
    assert!(
        matches!(
            &private.terminal.undelivered[0].item,
            PrivateOrderedItem::Refused {
                refusal: PrivateExecutionRefusal::DeliveryEnded,
                ..
            }
        ),
        "and the refusal says the delivery ended"
    );
    // The request itself is still owed its observation, which is why the
    // refusal keeps custody rather than dropping it.
    let mut settlement = frontend.take().expect("a live runner").shutdown();
    assert_eq!(settlement.terminal_outstanding(), Some(2), "one refusal observation and one pending lifecycle cleanup");
    for _ in 0..16 { settlement.retry(); }
    assert_eq!(settlement.terminal_outstanding(), Some(1), "lifecycle cleanup leaves the original refusal observation owned");
}

/// One admitted client with a real ingress, kept whole.
///
/// The receivers are held because a queue with no reader refuses everything,
/// and a control that could not tell that from a refusal on the merits would
/// pass for the wrong reason.
struct OrderedIngressFixture {
    durable: crate::PrivateSettlementOwner,
    private: crate::PrivateXServerFrontend,
    ingress: crate::PrivateIngress,
    keyboards: crate::PrivateKeyboards,
    registration: XServerFrontendClientRouteRegistration,
    channels: XServerFrontendClientRouteChannels,
    _acks: Receiver<XAuthorityClientControlAck>,
    deliveries: Receiver<XAuthorityClientInputDelivery>,
}

/// A sealed watchdog for a control that is not exercising the watch itself.
///
/// Real rather than absent: nothing runs unwatched, so a control that wants to
/// exercise something else still has to supply a supervisor that will take the
/// execution. The gate is dropped because these controls do not read it.
fn control_watchdog() -> private_watchdog::PrivateWatchdogOwner {
    let mut owner = private_watchdog::PrivateWatchdogOwner::prepare(0).expect("a watchdog");
    owner.seal().expect("a sealed watchdog");
    owner
}

fn ordered_ingress_fixture(
    client: XServerFrontendClientId,
    surface: SurfaceId,
) -> OrderedIngressFixture {
    let durable = crate::PrivateSettlementOwner::default();
    let (sender, acks) = sync_channel(8);
    let (delivery_sender, deliveries) = channel();
    let (authority, issuer, submit) = private_authority();
    let mut private = crate::PrivateXServerFrontend::new(
        crate::PrivateFrontendParts {
            max_concurrent_clients: NonZeroUsize::new(16).unwrap(),
            input_capacity: NonZeroUsize::new(4).unwrap(),
            control_acknowledgements: sender,
            input_deliveries: delivery_sender,
            authority,
            issuer,
            submit,
        },
        &durable,
    )
    .unwrap_or_else(|(refusal, _parts)| panic!("a fresh owner to have a slot: {refusal:?}"));
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(admitted(client)))
        .expect("a fresh client to register");
    private
        .broker.registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("the boundary to admit");
    private
        .broker
        .registry
        .register_surface(
            client,
            NamespaceId::from_raw(client.raw()),
            surface,
            XResourceId::new(0x200000 | client.raw(), 1),
        )
        .expect("the surface to register");
    let ingress = private
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("an ingress");
    let keyboards = private.keyboards().expect("this instance's state");
    OrderedIngressFixture {
        durable,
        private,
        ingress,
        keyboards,
        registration,
        channels,
        _acks: acks,
        deliveries,
    }
}

// Keep source admission alive when a test disconnects its recipient. A
// departed source is denied at the lifecycle gate before recipient binding.
fn separate_ordered_sender(
    private: &mut crate::PrivateXServerFrontend,
    ingress: &mut crate::PrivateIngress,
    recipient: XServerFrontendClientId,
) -> (XServerFrontendClientRouteRegistration, XServerFrontendClientRouteChannels) {
    let sender = XServerFrontendClientId(recipient.raw() + 50_000);
    let context = namespaced(sender, NamespaceId::from_raw(recipient.raw()));
    let registry = &private.broker.registry;
    let (registration, channels) = registry
        .register_client_with_admission(sender, Some(context))
        .unwrap();
    registry.attach_private_lifecycle(&registration, context).unwrap();
    *ingress = private.ingress_for(sender, DeviceId::from_raw(2)).unwrap();
    (registration, channels)
}

#[test]
fn a_join_leaves_the_source_obligation_alone_so_a_later_press_still_runs() {
    // The sequence that a join asked as a press poisons: press, join the same
    // button, then press a DIFFERENT button. Asking press for the join
    // installs a second source obligation and leaves it retained on the
    // disagreement the source reports, and the third press is then refused
    // WrongPhase for a phase the executor put there itself.
    let client = XServerFrontendClientId(2401);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        channels,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");

    // The cell is taken where the admission mints it and travels out with the
    // reports, so every claim below names the admission it came from.
    let run = |private: &mut crate::PrivateXServerFrontend,
                   keyboards: &mut crate::PrivateKeyboards,
                   delivery: u64,
                   button: u32| {
        ingress
            .submit(button_to(
                surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                true,
            ))
            .expect("the order to accept it");
        let cell = admitted_cell(private, delivery);
        let turn = private
            .route_pending_ordered(keyboards, watch)
            .expect("a readable order");
        (private.deliver_turn(turn), cell)
    };

    let (first, first_cell) = run(private, keyboards, 2401, 272);
    assert_eq!(
        first.len(),
        1,
        "the press was disposed of and its own outcome reported"
    );
    let capsule = inbox
        .accepted(private, &channels.ordered, &first_cell, 8)
        .expect("a readable terminal step")
        .expect("the first press owes its recipient an event");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(2401)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(private.terminal.holds.len(), 1);

    // The join. It owes nobody an event, and it must not leave a second
    // obligation behind it.
    let (joined, join_cell) = run(private, keyboards, 2402, 272);
    let _ = &joined;
    assert!(
        inbox
            .accepted(private, &channels.ordered, &join_cell, 4)
            .expect("a readable terminal step")
            .is_none(),
        "a join owes nobody an event: the button is already down"
    );
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "and it joined rather than beginning a second hold"
    );
    assert!(
        private.terminal.native_pending.is_none(),
        "the join asked the source for a join, so nothing was installed to retain"
    );

    // The press this blocker actually kills.
    let (third, third_cell) = run(private, keyboards, 2403, 273);
    let _ = &third;
    let capsule = inbox
        .accepted(private, &channels.ordered, &third_cell, 8)
        .expect("a readable terminal step")
        .expect("a different button still presses: the join left no phase behind it");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(2403)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(
        private.terminal.holds.len(),
        2,
        "and it began its own hold rather than being refused"
    );
}

#[test]
fn an_ordered_press_binds_its_delivery_to_the_client_that_receives_it() {
    let client = XServerFrontendClientId(992);
    let delivery = XAuthorityInputDeliveryId::from_raw(992);
    let mut fixture = prepared_ordered_fixture(client);
    let PreparedOrderedFixture {
        runner,
        ingress,
        channels,
        deliveries,
        surface,
        ..
    } = &mut fixture;
    let surface = *surface;
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");

    // Before the turn the ledger tracks the delivery with no recipient: it
    // knows something was accepted, not who is waiting for it.
    ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 992);
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .expect("a tracked delivery")
            .client,
        None
    );

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(
        delivered.len(),
        1,
        "the entry was disposed of and its own outcome reported"
    );
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the press reached the client's queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(992)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .expect("still tracked")
            .client,
        Some(client),
        "and the ledger now records who it went to"
    );

    // Which is what makes the client going answerable. An unbound delivery is
    // not one a disconnect can answer: it is answered to nobody, and only a
    // deadline would ever end it.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    let receipt = deliveries
        .try_recv()
        .expect("a terminal outcome for the bound delivery");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(receipt.client, client);
    assert_eq!(
        receipt.outcome,
        XAuthorityInputDeliveryOutcome::ClientDisconnected
    );
}

#[test]
fn a_press_whose_recipient_is_already_gone_leaves_no_hold() {
    let client = XServerFrontendClientId(993);
    let surface = SurfaceId::new(993, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(993);
        let PreparedOrderedFixture {
        mut runner, mut ingress, channels, deliveries, registration, durable,
        _acks,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    let (_sender_registration, _sender_channels) = separate_ordered_sender(private, &mut ingress, client);

    // The recipient closes first. The distinct submitter remains authorized,
    // but the recipient gate refuses before binding or pressing the ledger.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    assert!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .is_some(),
        "the delivery itself has not ended"
    );

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(private.deliver_turn(turn).is_empty());
    assert!(
        private.terminal.holds.is_empty(),
        "a press that cannot be delivered must not leave a hold: the release \
         answering it would be owed to a client that was already gone"
    );
    // OLD CLAIM: the refusal is the authority's WrongConnection, raised by
    //   this executor's own precheck of the recipient's connection.
    // NEW CLAIM: the refusal is the source's own Connection(AdmissionClosed).
    //   The source resolves the recipient's connection inside the press now,
    //   and that lookup is what finds the admission closed -- so the fact
    //   belongs to the source and is reported under the source's name.
    // The claim that matters is unchanged and still asserted: this press
    // refused BECAUSE the recipient was already gone, not for some other
    // reason that happens to refuse.
    assert!(matches!(
        &private.terminal.undelivered[0].item,
        PrivateOrderedItem::Refused {
            refusal: PrivateExecutionRefusal::Native(private_native::Refusal::Connection(
                PrivateAppliedRegistryRefusal::AdmissionClosed
            )),
            ..
        }
    ));
    assert!(channels.input.try_recv().is_err());
    assert_eq!(private.broker.registry.input_recovery.ticket(delivery).unwrap().client, None,
        "recipient closure refuses before recovery binding, not through its cancellation path");
    assert!(deliveries.try_recv().is_err());

    // The hold record above is this executor's own bookkeeping. What matters
    // is the authority's ledger, and it is reachable: this refusal entered the
    // transaction, so a rejection was recorded in common and the custody it
    // kept can be observed. Observing frees the grant's completion cell, which
    // is what lets the same source ask again.
    let observed = {
        let PrivateOrderedItem::Refused { custody, .. } = &private.terminal.undelivered[0].item
        else {
            panic!("a refusal")
        };
        custody.observe().expect("the authority to be readable")
    };
    assert!(
        observed.is_some(),
        "a refusal that reached the transaction has an outcome recorded for it"
    );

    // Now ask the ledger itself. An untouched one reports nothing was held and
    // the release finishes; one that was pressed would end a hold whose plan
    // this executor never recorded, and refuse.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9931),
            272,
            false,
        ))
        .expect("the order to accept it");
        let press_cell = admitted_cell(private, 9931);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let released = private.deliver_turn(turn);
    assert_eq!(
        released.len(),
        1,
        "the release finishes, because the ledger was never pressed"
    );
    let _ = &released;
    assert!(
        inbox
            .accepted(private, &channels.ordered, &press_cell, 4)
            .expect("a readable terminal step")
            .is_none(),
        "and it owes nobody an event"
    );
    assert!(
        !private
            .terminal
            .undelivered
            .iter()
            .any(|entry| matches!(
                &entry.item,
                PrivateOrderedItem::Refused {
                    refusal: PrivateExecutionRefusal::HoldPlanMissing,
                    ..
                }
            )),
        "nothing ended a hold this executor never recorded"
    );
    drop(registration);
    drop(durable);
}

#[test]
fn a_release_to_a_gone_recipient_still_lifts_the_button() {
    let client = XServerFrontendClientId(994);
    let surface = SurfaceId::new(994, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
        let PreparedOrderedFixture {
        mut runner, mut ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    let (_sender_registration, _sender_channels) = separate_ordered_sender(private, &mut ingress, client);

    // A press that lands while the client is there.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9941),
            272,
            true,
        ))
        .expect("the order to accept it");
        let press_cell = admitted_cell(private, 9941);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(9941)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(projected_buttons(private, namespace, seat), 0x100);
    assert_eq!(private.terminal.holds.len(), 1);

    // Then it goes, and the release arrives afterwards. The ledger is told,
    // and so is the routing side: output reaches a client through the queue
    // its connection owns, so a recipient that is gone is one with no entry
    // there. This is the state route_to_client itself leaves behind when a
    // queue reports its receiver dropped.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    assert!(
        private
            .broker
            .registry
            .clients
            .lock()
            .expect("a readable registry")
            .remove(&client)
            .is_some(),
        "the connection was there to lose"
    );
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9942),
            272,
            false,
        ))
        .expect("the order to accept it");
        let release_cell = admitted_cell(private, 9942);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(
        delivered.len(),
        1,
        "the entry was disposed of and its own outcome reported"
    );

    // The hold ended and the button is up. Neither could be conditional on
    // anyone still being there to be told: a button left down because its
    // client vanished is held forever, by nobody.
    assert!(
        private.terminal.holds.is_empty(),
        "the hold ended"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0,
        "and the button this seat had down is up"
    );
    assert!(
        inbox
            .accepted(private, &channels.ordered, &release_cell, 4)
            .expect("a readable terminal step")
            .is_none(),
        "but nothing was enqueued for a client that is gone"
    );
    assert_eq!(
        private.terminal.settling.len(),
        1,
        "the debt is recorded even though nothing will be sent"
    );
    assert!(
        private.terminal.settling[0].binding() == PrivateReleaseBinding::Ended,
        "and it says which: established gone, not merely unlooked-up"
    );
    assert_eq!(
        private.terminal.settling[0].delivery(),
        Some(XAuthorityInputDeliveryId::from_raw(9942)),
        "and still records which delivery would have answered it: what is \
         unknown is the receipt, not which delivery it belongs to"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_grabbed_press_binds_its_delivery_to_the_grab_owner_not_the_surface() {
    let client = XServerFrontendClientId(995);
    let owner = XServerFrontendClientId(996);
    let surface = SurfaceId::new(995, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let delivery = XAuthorityInputDeliveryId::from_raw(995);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    // The grab owner is a real admitted client of this instance, in the same
    // namespace the grab is recorded in, with its own live queue: the press is
    // delivered to it, so the boundary has to know it and something has to be
    // there to receive it. Admitting it into its own namespace instead would
    // make the grab name a window this press's origin cannot reach.
    let (owner_registration, owner_channels) = private
        .broker
        .registry
        .register_client_with_admission(owner, Some(namespaced(owner, namespace)))
        .expect("a fresh client to register");
    private
        .admission_participant()
        .admit(owner, namespaced(owner, namespace))
        .expect("the boundary to admit");
    // The grab owner's own view of its own window. A grab names a window to
    // deliver into, and the source resolves that window through the owner's
    // selection state -- so an owner that never registered one has nothing for
    // the press to reach, however well the grab is recorded.
    let owner_selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    let owner_focused = Arc::new(AtomicU64::new(0));
    private
        .broker
        .registry
        .attach_connection_state(
            &owner_registration,
            namespace,
            owner_selections.clone(),
            owner_focused.clone(),
        )
        .expect("the owner's connection state attaches");
    {
        let mut selected = owner_selections.lock().expect("the owner's selections");
        selected.register(
            XResourceId::new(0x200996, 1),
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
        );
        selected.observe_mapped(XResourceId::new(0x200996, 1));
        selected.update(XResourceId::new(0x200996, 1), Some((1 << 2) | (1 << 3)), None);
    }

    private
        .broker
        .registry
        .input_authority
        .lock()
        .expect("the grab state")
        .grab_pointer(
            namespace,
            crate::XActiveInputGrab {
                owner: owner.raw(),
                window: XResourceId::new(0x200996, 1),
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: u16::MAX,
                xi_event_mask: [0; 8],
                xi_event_mask_words: 0,
                route_lease: None,
            },
        )
        .expect("the grab to take");

    ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 995);
    let mut inbox = OrderedInbox::default();
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(
        delivered.len(),
        1,
        "the entry was disposed of and its own outcome reported"
    );
    let to_owner = inbox
        .accepted(private, &owner_channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the grab owner is the one that received it");
    assert_eq!(to_owner.delivery(), delivery);
    assert_eq!(to_owner.client(), owner);
    assert!(
        inbox.taken.is_empty(),
        "and nothing else was on the owner's queue"
    );
    assert!(
        channels.ordered.try_recv().is_err(),
        "and the surface's own client did not"
    );

    // The route named this surface, whose client is 995. The grab sent the
    // press to 996. What the ledger records is where the event went, because
    // that is who a disconnect has to answer for -- recording the route's
    // client would answer the wrong client's departure and leave this
    // delivery owed to nobody.
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .expect("still tracked")
            .client,
        Some(owner)
    );
    assert_eq!(
        private.terminal.holds[0].reached.client(),
        owner,
        "and the hold records the same recipient"
    );
    drop(owner_registration);
    drop(owner_channels);
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn an_unreadable_ledger_is_not_a_delivery_that_ended() {
    let client = XServerFrontendClientId(997);
    let surface = SurfaceId::new(997, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(997);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");

    // The ledger becomes unreadable while that delivery is still waiting its
    // turn.
    let recovery = private.broker.registry.input_recovery.clone();
    let _ = std::thread::spawn(move || {
        let _guard = recovery.state.lock().expect("the ledger");
        panic!("poisoning the recovery ledger");
    })
    .join();

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(private.deliver_turn(turn).is_empty());
    assert!(
        private.terminal.holds.is_empty(),
        "nothing is applied on the strength of what nobody could read"
    );
    // Refused for the absence of an answer, not for an answer. Reporting this
    // as ended would say the delivery was settled, which nothing established
    // -- and a caller acting on that would stop waiting for an outcome that
    // is still owed.
    assert!(
        matches!(
            &private.terminal.undelivered[0].item,
            PrivateOrderedItem::Refused {
                refusal: PrivateExecutionRefusal::RecoveryUnavailable,
                ..
            }
        ),
        "an unreadable ledger is its own refusal"
    );
    assert!(channels.input.try_recv().is_err());
    drop(registration);
    drop(durable);
}

/// A press that ran, leaving a hold and a button down.
///
/// The cell is taken where the admission mints it and handed back, so a caller
/// can go on naming this exact admission after its ticket is answered.
fn held_button(
    private: &mut crate::PrivateXServerFrontend,
    ingress: &crate::PrivateIngress,
    keyboards: &mut crate::PrivateKeyboards,
    watch: &private_watchdog::PrivateWatchdogOwner,
    inbox: &mut OrderedInbox,
    queue: &Receiver<XAuthorityOrderedDelivery>,
    surface: SurfaceId,
    delivery: u64,
) -> (Arc<PrivateDeliveryCompletion>, XAuthorityOrderedDelivery) {
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(delivery),
            272,
            true,
        ))
        .expect("the order to accept it");
    let cell = admitted_cell(private, delivery);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, queue, &cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(delivery)
    );
    assert_eq!(private.terminal.holds.len(), 1);
    (cell, capsule)
}

#[test]
fn a_release_whose_delivery_ended_does_not_end_its_hold() {
    let client = XServerFrontendClientId(998);
    let surface = SurfaceId::new(998, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let mut inbox = OrderedInbox::default();
    let (_held_cell, _held_capsule) = held_button(
        private,
        &ingress,
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        9981,
    );

    // The release is accepted, and then its own delivery ends while it waits
    // its turn.
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9982),
            272,
            false,
        ))
        .expect("the order to accept it");
    private
        .broker
        .registry
        .input_recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(private.deliver_turn(turn).is_empty());

    // Refused before the ledger moved. This is the case the gate exists for:
    // the press path finds out by binding, but a release binds only after its
    // transition, so without a check beforehand a withdrawn release would end
    // a hold and lift a button on the strength of a request whose outcome was
    // already reported.
    assert!(
        matches!(
            &private.terminal.undelivered[0].item,
            PrivateOrderedItem::Refused {
                refusal: PrivateExecutionRefusal::DeliveryEnded,
                ..
            }
        ),
        "the release is refused for its delivery having ended"
    );
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "and the hold it would have ended is still here"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0x100,
        "with the button still down, because nothing lifted it"
    );
    // Not lost, either: the obligation is retained rather than discarded, and
    // whoever takes the inventory is the one that can still answer it.
    assert!(
        frontend
            .take()
            .expect("a live runner")
            .shutdown()
            .terminal_outstanding()
            .expect("readable terminal inventory")
            >= 1
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_release_does_not_move_the_ledger_when_nobody_can_read_the_deliveries() {
    let client = XServerFrontendClientId(999);
    let surface = SurfaceId::new(999, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let mut inbox = OrderedInbox::default();
    let (_held_cell, _held_capsule) = held_button(
        private,
        &ingress,
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        9991,
    );

    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(9992),
            272,
            false,
        ))
        .expect("the order to accept it");
    let recovery = private.broker.registry.input_recovery.clone();
    let _ = std::thread::spawn(move || {
        let _guard = recovery.state.lock().expect("the ledger");
        panic!("poisoning the recovery ledger");
    })
    .join();

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(private.deliver_turn(turn).is_empty());
    assert!(
        matches!(
            &private.terminal.undelivered[0].item,
            PrivateOrderedItem::Refused {
                refusal: PrivateExecutionRefusal::RecoveryUnavailable,
                ..
            }
        ),
        "nothing is known, and that is its own answer"
    );
    // The distinction is the whole point: this release may still be owed. Had
    // it run, the hold would be gone and the button up on the strength of
    // something nobody could read.
    assert_eq!(private.terminal.holds.len(), 1);
    assert_eq!(projected_buttons(private, namespace, seat), 0x100);
    assert!(
        private.terminal.settling.is_empty(),
        "and no debt was recorded, because no release happened"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

/// A ledger with one admitted, unbound delivery.
fn claim_fixture(
    delivery: XAuthorityInputDeliveryId,
) -> (
    InputRecovery,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (sender, receipts) = channel();
    let recovery = InputRecovery::new(
        8,
        Some(sender),
        Arc::new(Mutex::new(crate::XInputAuthorityState::default())),
    );
    recovery
        .admit_typed(
            &button_to(SurfaceId::new(1, 1), delivery, 272, true),
            1,
            std::time::Instant::now(),
        )
        .expect("a fresh delivery to be tracked");
    (recovery, receipts)
}

#[test]
fn a_cancellation_arriving_under_a_claim_does_not_publish_over_the_effect() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4001);
    let (recovery, receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );

    // The revocation producer runs in the interval the execution holds. This
    // is the gap that a precheck leaves open: the ledger's own guard is not
    // held here, and the effect has not happened yet.
    let expired = recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert!(
        expired.is_empty(),
        "a delivery being applied right now is not an abandoned one"
    );
    assert!(
        receipts.try_recv().is_err(),
        "and nothing was published for it"
    );

    // The execution applied something, so the cancellation had an effect to
    // contradict and does not become this delivery's outcome. The delivery is
    // still owed one, which its writer result or its deadline answers -- not
    // the same as it having ended.
    recovery.resolve_claim(Some(delivery), true);
    assert!(receipts.try_recv().is_err());
    assert!(
        recovery.ticket(delivery).is_some(),
        "still tracked, still owed an outcome"
    );
}

#[test]
fn a_cancellation_that_lost_to_an_execution_applying_nothing_still_stands() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4002);
    let (recovery, receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    assert!(
        recovery
            .recover(std::time::Instant::now(), true)
            .expect("readable")
            .is_empty()
    );

    // Nothing was applied under the claim, so the cancellation had nothing to
    // contradict. Dropping it here would lose a revocation on the strength of
    // an execution that did not happen.
    recovery.resolve_claim(Some(delivery), false);
    let receipt = receipts
        .try_recv()
        .expect("the revocation to be published once the claim gave way");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(
        receipt.outcome,
        XAuthorityInputDeliveryOutcome::EpochRevoked
    );
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Ended,
        "and a later execution finds it ended"
    );
}

#[test]
fn one_delivery_cannot_be_claimed_by_two_executions() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4003);
    let (recovery, _receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Contended,
        "contended is not ended: nothing finished, and the delivery is still \
         owed an outcome by whoever holds it"
    );
    recovery.resolve_claim(Some(delivery), true);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed,
        "and the claim is available again once it is given back"
    );
}

#[test]
fn an_ordered_turn_gives_its_claim_back() {
    let client = XServerFrontendClientId(1001);
    let delivery = XAuthorityInputDeliveryId::from_raw(1001);
        let PreparedOrderedFixture {
        mut runner,
        ingress,
        channels,
        deliveries,
        surface,
        durable: _durable,
        registration: _registration,
        _acks,
        selections: _selections,
        client: _client,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 1001);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(1001)
    );
    assert_eq!(capsule.client(), client);

    // Given back, so the delivery can still be cancelled. A claim nobody
    // resolves is not a delivery that is safe: it is one nothing can ever
    // answer again, because every cancellation after it defers forever.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    let receipt = deliveries
        .try_recv()
        .expect("the delivery to still be answerable");
    assert_eq!(receipt.delivery, delivery);
    drop(channels);
}

#[test]
fn a_release_whose_delivery_another_execution_holds_applies_nothing() {
    let client = XServerFrontendClientId(1002);
    let surface = SurfaceId::new(1002, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
    let release = XAuthorityInputDeliveryId::from_raw(10022);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    let mut inbox = OrderedInbox::default();
    let (_held_cell, _held_capsule) = held_button(
        private,
        &ingress,
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        10021,
    );

    ingress
        .submit(button_to(surface, release, 272, false))
        .expect("the order to accept it");
    // Something else holds this delivery. Its effect may be under way, and a
    // second one applied here would be a second effect for one request.
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .claim_execution(Some(release)),
        ExecutionClaim::Claimed
    );

    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    assert!(private.deliver_turn(turn).is_empty());
    assert!(
        matches!(
            &private.terminal.undelivered[0].item,
            PrivateOrderedItem::Refused {
                refusal: PrivateExecutionRefusal::DeliveryClaimedElsewhere,
                ..
            }
        ),
        "refused for contention, which is not the delivery having ended"
    );
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "and nothing was applied: the hold is untouched"
    );
    assert_eq!(
        projected_buttons(private, namespace, seat),
        0x100,
        "with the button still down"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_joining_press_binds_the_recipient_its_hold_reached_not_the_new_target() {
    let client = XServerFrontendClientId(1003);
    let surface = SurfaceId::new(1003, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let first = XAuthorityInputDeliveryId::from_raw(10031);
    let second = XAuthorityInputDeliveryId::from_raw(10032);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable, selections: _, window: _,
        _acks,
        deliveries: _deliveries,
        client: _client,
        surface: _surface,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");

    // A press that reaches this client and starts a hold.
    let mut inbox = OrderedInbox::default();
    let (_held_cell, _held_capsule) = held_button(
        private,
        &ingress,
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        10031,
    );
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(first)
            .expect("tracked")
            .client,
        Some(client)
    );

    // A REAL REPLACEMENT, without forcing any state. B alone cannot displace
    // the implicit grab this press took, but A can release its own first --
    // so the owner ungrabs and B grabs, and a fresh resolution of this button
    // now genuinely reaches a different client than the press did.
    let owner = XServerFrontendClientId(1004);
    let (owner_registration, owner_channels) = private
        .broker
        .registry
        .register_client_with_admission(owner, Some(namespaced(owner, namespace)))
        .expect("a fresh client to register");
    private
        .broker
        .registry
        .attach_private_lifecycle(&owner_registration, namespaced(owner, namespace))
        .expect("the boundary to admit");
    {
        let mut grabs = private
            .broker
            .registry
            .input_authority
            .lock()
            .expect("the grab state");
        // The press's own client gives up the grab it holds. Nothing is
        // forced: this is the owner releasing its own.
        grabs.ungrab_pointer(namespace, client.raw());
        grabs
            .grab_pointer(
                namespace,
                crate::XActiveInputGrab {
                    owner: owner.raw(),
                    window: XResourceId::new(0x201004, 1),
                    owner_events: false,
                    pointer_mode: 1,
                    keyboard_mode: 1,
                    event_mask: u16::MAX,
                    xi_event_mask: [0; 8],
                    xi_event_mask_words: 0,
                    route_lease: None,
                },
            )
            .expect("the grab to take");
    }

    // The same button again. The ledger joins the hold that exists: no new
    // hold, no new event, and the recipient is the one the hold already has.
    ingress
        .submit(button_to(surface, second, 272, true))
        .expect("the order to accept it");
    let join_cell = admitted_cell(private, 10032);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    let delivered = private.deliver_turn(turn);
    assert_eq!(delivered.len(), 1);
    assert!(
        inbox
            .accepted(private, &channels.ordered, &join_cell, 4)
            .expect("a readable terminal step")
            .is_none(),
        "a join owes nobody an event: the button is already down"
    );
    assert_eq!(
        private.terminal.holds.len(),
        1,
        "and it joined rather than starting a second hold"
    );

    // The binding follows what the press reached, not what the route would
    // resolve to now. Binding the grab owner would put this delivery's
    // outcome on a client it never reached: the owner's disconnect would
    // answer it, and the client that is actually holding the button would
    // not.
    assert_eq!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(second)
            .expect("tracked")
            .client,
        Some(client),
        "the join inherits the recipient its hold reached"
    );
    assert!(
        owner_channels.input.try_recv().is_err(),
        "and no other client of this instance received it either"
    );
    drop(owner_registration);
    drop(owner_channels);
    drop(registration);
    drop(channels);
    drop(durable);
}

/// What the ledger records about one delivery's claim lifetime.
fn claim_state(
    private: &crate::PrivateXServerFrontend,
    delivery: XAuthorityInputDeliveryId,
) -> (bool, bool) {
    let held = private
        .broker
        .registry
        .input_recovery
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = held.tickets.get(&delivery).expect("still tracked");
    (entry.claimed, entry.may_have_applied)
}

#[test]
fn a_refusal_before_the_effect_resolves_the_claim_as_having_applied_nothing() {
    let client = XServerFrontendClientId(1101);
    let surface = SurfaceId::new(1101, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1101);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");

    // The admission goes, so the real boundary refuses this request before the
    // effect callback is ever invoked. That refusal leaves the execution by a
    // returned error, not by deciding anything -- and everything after a
    // fallible call is skipped when that call returns an error, which is
    // exactly where a copied progress marker would be wrong.
    fixture
        .private
        .admission_participant()
        .revoke_admission(
            client,
            sophia_protocol::ClientAdmissionId::from_raw(client.raw()),
        )
        .expect("the boundary to revoke");

    let turn = fixture
        .private
        .route_pending_ordered(&mut fixture.keyboards, &control_watchdog())
        .expect("a readable order");
    assert!(fixture.private.deliver_turn(turn).is_empty());
    assert!(
        fixture.private.terminal.holds.is_empty(),
        "nothing was applied"
    );
    assert_eq!(
        claim_state(&fixture.private, delivery),
        (false, false),
        "the claim is given back, saying nothing was applied -- which the \
         guard has to know on a returned error, not only on an unwind"
    );
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn an_established_fact_is_reported_even_when_something_may_have_applied() {
    // A cancellation deferred under a claim is dropped when an effect may
    // have happened, and rightly: it says the delivery did not happen and the
    // effect contradicts it. An established fact is not that. A terminated
    // connection is not undone by an effect having occurred, and returning on
    // the same check left such a delivery deferred and then discarded --
    // answered to nobody, ever.
    let client = XServerFrontendClientId(1103);
    let delivery = XAuthorityInputDeliveryId::from_raw(1103);
    let (recovery, receipts) = claim_fixture(delivery);
    recovery.register(client).unwrap();
    assert_eq!(recovery.claim_execution(Some(delivery)), ExecutionClaim::Claimed);
    assert!(recovery.bind(Some(delivery), client).unwrap());

    // The recipient's connection ends while the claim is held.
    recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .unwrap();
    assert!(
        receipts.try_recv().is_err(),
        "held while the claim is out, like any other outcome"
    );

    // The claim resolves having MAYBE APPLIED. A cancellation would be
    // dropped here; this is not a cancellation.
    recovery.resolve_claim(Some(delivery), true);
    let receipt = receipts
        .try_recv()
        .expect("an established termination is still reported");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(receipt.client, client);
    assert_eq!(receipt.outcome, XAuthorityInputDeliveryOutcome::ClientDisconnected);
}

#[test]
fn a_cancellation_deferred_by_binding_under_a_claim_stands_when_nothing_applied() {
    // Recovery API composition, not the private consumer: that consumer now
    // rejects a closing recipient before binding. A race after recipient
    // validation still requires this claim/bind arbitration to remain correct.
    let client = XServerFrontendClientId(1102);
    let delivery = XAuthorityInputDeliveryId::from_raw(1102);
    let (recovery, receipts) = claim_fixture(delivery);
    recovery.register(client).unwrap();
    recovery.disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected).unwrap();
    assert_eq!(recovery.claim_execution(Some(delivery)), ExecutionClaim::Claimed);
    assert!(!recovery.bind(Some(delivery), client).unwrap());
    assert!(receipts.try_recv().is_err(), "the claim defers this exact binding cancellation");
    assert_eq!(recovery.ticket(delivery).unwrap().client, Some(client));
    recovery.resolve_claim(Some(delivery), false);
    let receipt = receipts.try_recv().expect("no application means the deferred cancellation stands");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(receipt.client, client);
    assert_eq!(receipt.outcome, XAuthorityInputDeliveryOutcome::ClientDisconnected);
    recovery.resolve_claim(Some(delivery), false);
    assert!(receipts.try_recv().is_err(), "resolution cannot publish twice");
}

#[test]
fn a_claim_is_given_back_even_when_the_ledger_cannot_be_read() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4004);
    let (recovery, receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");

    let poisoner = recovery.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.state.lock().expect("the ledger");
        panic!("poisoning the recovery ledger");
    })
    .join();

    // Giving back a claim is the one thing that cannot decline. Only this
    // caller holds it, and a claim nobody gives back is a delivery nothing can
    // ever cancel again and no owner knows is owed.
    recovery.resolve_claim(Some(delivery), false);
    let held = recovery
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert!(
        !held
            .tickets
            .get(&delivery)
            .expect("still tracked")
            .claimed,
        "an unreadable ledger must not leave a permanent claim nobody owns"
    );
    drop(held);
    // And the cancellation it was holding is resolved rather than stranded
    // with it.
    let receipt = receipts
        .try_recv()
        .expect("the deferred cancellation to be resolved too");
    assert_eq!(receipt.delivery, delivery);
    assert_eq!(
        receipt.outcome,
        XAuthorityInputDeliveryOutcome::EpochRevoked
    );
}

#[test]
fn a_cancellation_cannot_publish_once_the_delivery_may_have_applied() {
    let delivery = XAuthorityInputDeliveryId::from_raw(4005);
    let (recovery, receipts) = claim_fixture(delivery);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");

    // This execution applied something, so the cancellation it lost to cannot
    // become the delivery's outcome.
    recovery.resolve_claim(Some(delivery), true);
    assert!(receipts.try_recv().is_err());

    // Nor can a later execution that happens to apply nothing publish it. What
    // one claim did is not what the delivery has been through: the effect
    // already happened, and a per-claim answer cannot erase that.
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    recovery.resolve_claim(Some(delivery), false);
    assert!(
        receipts.try_recv().is_err(),
        "a second claim applying nothing does not put a stale cancellation \
         back in reach of a delivery whose effect already happened"
    );

    // Nor can a fresh one arriving through the ordinary entry.
    recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert!(
        receipts.try_recv().is_err(),
        "the same contradiction refused at the normal entry, not only the \
         deferred one"
    );

    // But what became of the delivery is still sayable. Refusing this too
    // would leave a delivery whose effect happened with no way to be answered
    // at all, which is the opposite failure.
    recovery
        .finish(
            XServerFrontendClientId(1),
            Some(delivery),
            XAuthorityInputDeliveryOutcome::Flushed,
        )
        .expect("the ledger to be readable");
    assert_eq!(
        receipts
            .try_recv()
            .expect("an established outcome to publish")
            .outcome,
        XAuthorityInputDeliveryOutcome::Flushed,
        "a writer result is not a denial that the delivery happened, so it \
         publishes"
    );
}

#[test]
fn a_press_that_applied_cannot_be_revoked_afterwards() {
    let client = XServerFrontendClientId(1203);
    let surface = SurfaceId::new(1203, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1203);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, deliveries, registration, durable,
        _acks,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 1203);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(1203)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(
        claim_state(private, delivery),
        (false, true),
        "the claim is back, and the delivery is on record as having applied"
    );

    // A sweep that would have revoked it before is too late now: the effect
    // happened, the button is down, and saying the delivery was withdrawn
    // would tell everyone waiting to stop on account of something that did
    // occur.
    let revoked = private
        .broker
        .registry
        .input_recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert!(
        revoked.is_empty(),
        "nothing is reported revoked that was not"
    );
    assert!(deliveries.try_recv().is_err());

    // The client going is still sayable, because that is what became of the
    // delivery rather than a denial that it happened.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    assert_eq!(
        deliveries
            .try_recv()
            .expect("an established recipient fact")
            .outcome,
        XAuthorityInputDeliveryOutcome::ClientDisconnected
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_retained_release_debt_is_named_the_way_the_ledger_names_it() {
    let client = XServerFrontendClientId(1201);
    let surface = SurfaceId::new(1201, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let seat = SeatId::from_raw(1);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    let (_held_cell, _held_capsule) = held_button(
        private,
        &ingress,
        keyboards,
        watch,
        &mut inbox,
        &channels.ordered,
        surface,
        12011,
    );

    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(12012),
            272,
            false,
        ))
        .expect("the order to accept it");
        let press_cell = admitted_cell(private, 12012);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(12012)
    );
    assert_eq!(capsule.client(), client);
    assert_eq!(projected_buttons(private, namespace, seat), 0);
    assert_eq!(private.terminal.settling.len(), 1);

    // What the ledger itself reports as owed. A debt is named by its whole
    // incarnation -- authority, recipient, connection generation and input --
    // and both settle and an attempt claim are matched against that name.
    let mut cursor = 0;
    let reported = private
        .authority()
        .under_common(|authority| authority.next_debt(&mut cursor))
        .expect("the authority to be readable")
        .expect("a retained debt for the release that just happened");

    // The retained record has to carry the same name. A record holding only
    // the number inside an incarnation can be compared with nothing the
    // ledger offers, so the debt it describes is one this executor could
    // never settle.
    assert_eq!(
        private.terminal.settling[0].incarnation(),
        reported.0,
        "the retained debt is named the way the ledger names it"
    );
    // And it records which delivery carries its event. A receipt arrives
    // naming a delivery and settles a debt named by an incarnation; nothing
    // else holds both, so without this a writer result could be observed and
    // still not be attributable to the debt it settles.
    assert_eq!(
        private.terminal.settling[0].delivery(),
        Some(XAuthorityInputDeliveryId::from_raw(12012)),
        "the debt knows which delivery answers it"
    );
    assert_eq!(
        reported.0.input,
        private.terminal.settling[0].incarnation().input,
        "including the input it is for"
    );
    assert!(
        !reported.1.is_settled(),
        "and nothing has settled it whole: a release having happened \
         establishes neither half by itself"
    );
    // OLD CLAIM: neither bit is set, because a release having happened
    //   establishes neither half.
    // NEW CLAIM: the native half is set, and NOT because a release happened.
    //   The source produced a proof that its own projection was reconciled,
    //   and that proof recorded the bit once the adapter guards and the
    //   common transaction had both dropped. A release that ends with a
    //   residual produces no proof and leaves this false, which is what makes
    //   the bit evidence rather than a restatement of "a release occurred".
    assert!(
        reported.1.native_reconciled,
        "the source's own proof recorded the native half"
    );
    assert!(
        !reported.1.recipient_settled,
        "and nothing here establishes the recipient's half: an event being \
         owed, built, or queued is not a receipt"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn one_step_takes_one_item_and_marks_it_before_common() {
    let client = XServerFrontendClientId(1301);
    let surface = SurfaceId::new(1301, 1);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    // A second producer, so two items can wait at once: one grant holds one
    // completion cell, and the first request keeps it until it is observed.
    let second = private
        .ingress_for(client, DeviceId::from_raw(2))
        .expect("a second ingress");
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(13011),
            272,
            true,
        ))
        .expect("the order to accept it");
    second
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(13012),
            273,
            true,
        ))
        .expect("the order to accept it");

    // Cloned rather than reached through the frontend, which the step borrows
    // mutably while the mark runs.
    let common = Arc::clone(&private.authority().common);
    let recovery = private.broker.registry.input_recovery.clone();
    let first_delivery = XAuthorityInputDeliveryId::from_raw(13011);
    let mut marked = Vec::new();
    let step = {
        let mut mark = |sequence: crate::ReadySequence,
                        _taken_at: std::time::Instant|
         -> Result<(), XServerFrontendRouteError> {
            // Common is not held: the mark sits above that guard in the rank
            // and reaching for it here would invert the order.
            assert!(
                common.try_lock().is_ok(),
                "the mark runs outside common"
            );
            // And the work is not merely un-guarded but un-attempted. This is
            // what distinguishes a mark placed before the effect from one
            // placed after the execution returned, where common is also free:
            // the ledger has not moved for this delivery yet.
            let held = recovery
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assert!(
                !held
                    .tickets
                    .get(&first_delivery)
                    .expect("tracked")
                    .may_have_applied,
                "the mark names work that has not been attempted"
            );
            drop(held);
            marked.push(sequence);
            Ok(())
        };
        private
            .step_once(keyboards, &mut mark, watch)
            .expect("a readable order")
    };
    let PrivateOrderedStep::Decided(sequence) = step else {
        panic!("one item decided")
    };
    assert_eq!(marked.len(), 1, "one step marks exactly one item");
    // Stored by the step, not handed back for the caller to hold.
    assert_eq!(private.terminal.turn.len(), 1);
    let PrivateOrderedItem::Ran {
        sequence: stored, ..
    } = private.terminal.turn[0]
    else {
        panic!("the press ran")
    };
    assert_eq!(stored, sequence);
    assert_eq!(
        marked[0], sequence,
        "and marks the item it actually took, not one it was about to"
    );

    // The second is still waiting: a step does not drain what it was not
    // charged for.
    let mut second_marked = Vec::new();
    let step = private
        .step_once(keyboards, &mut |sequence, _| {
            second_marked.push(sequence);
            Ok(())
        }, watch)
        .expect("a readable order");
    assert!(matches!(step, PrivateOrderedStep::Decided(_)));
    assert_eq!(second_marked.len(), 1);
    assert_ne!(second_marked[0], marked[0], "a different item");

    // And now the order is empty, which is its own answer rather than a
    // failure to find work.
    assert!(matches!(
        private
            .step_once(
                keyboards,
                &mut |_, _| panic!("nothing to mark"),
                watch,
            )
            .expect("a readable order"),
        PrivateOrderedStep::Idle
    ));
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_blocked_order_takes_nothing_and_marks_nothing() {
    let client = XServerFrontendClientId(1302);
        let PreparedOrderedFixture {
        mut runner,
        ingress: _,
        channels,
        deliveries: _,
        surface,
        durable: _durable,
        registration: _registration,
        _acks,
        selections: _selections,
        client: _client,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let watch = watch.as_ref().expect("a sealed watch");
    // A control command is an operation this path does not execute, so the
    // order parks behind it.
    private
        .control_producer()
        .submit(configure(client, surface, 13021))
        .expect("the order to accept it");
    let step = private
        .step_once(keyboards, &mut |_, _| Ok(()), watch)
        .expect("a readable order");
    assert!(matches!(step, PrivateOrderedStep::Parked(_)));

    // Blocked is not idle. A runner told only "no item" would charge a start
    // and mark a watchdog for work it could never have run, and would keep
    // doing so for as long as the barrier stood.
    let step = private
        .step_once(
            keyboards,
            &mut |_, _| panic!("nothing may be taken while the order is blocked"),
            watch,
        )
        .expect("a readable order");
    assert!(
        matches!(step, PrivateOrderedStep::Blocked(_)),
        "the barrier is reported as itself, not as an empty order"
    );
    drop(channels);
}

#[test]
fn a_suppressed_revocation_still_cleans_up_the_connection_it_revoked() {
    let client = XServerFrontendClientId(1303);
    let surface = SurfaceId::new(1303, 1);
    let namespace = NamespaceId::from_raw(client.raw());
    let delivery = XAuthorityInputDeliveryId::from_raw(1303);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, deliveries, registration, durable, window,
        _acks,
        selections: _selections,
        client: _client,
        surface: _surface,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    // A grab this client owns, over the window it actually has. The
    // source resolves a grab through the owner's own selection state, so
    // a grab naming a window this client never registered would leave the
    // press nothing to reach and prove nothing about cleanup.
    private
        .broker
        .registry
        .input_authority
        .lock()
        .expect("the grab state")
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
        .expect("the grab to take");

    // A press that applies, so its delivery can no longer be revoked.
    ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 1303);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(1303)
    );
    assert_eq!(capsule.client(), client);

    // The sweep revokes the connection and publishes nothing for the delivery,
    // because saying it was withdrawn would contradict the effect.
    let revoked = private
        .broker
        .registry
        .input_recovery
        .recover(std::time::Instant::now(), true)
        .expect("the ledger to be readable");
    assert!(revoked.is_empty(), "nothing reported revoked that was not");
    assert!(deliveries.try_recv().is_err());

    // The connection was still taken down, so what it owned still has to go.
    // Reading cleanup off the published list would skip exactly this case and
    // leave a grab installed for a client whose socket is gone.
    assert!(
        private
            .broker
            .registry
            .input_authority
            .lock()
            .expect("the grab state")
            .pointer_grab(namespace)
            .is_none(),
        "the revoked connection's grab is gone even though its delivery \
         published nothing"
    );
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_mark_that_panics_does_not_take_the_work_with_it() {
    let client = XServerFrontendClientId(1304);
    let surface = SurfaceId::new(1304, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1304);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let reserved_before = fixture.durable.reserved().expect("a readable owner");
    assert_eq!(reserved_before, 1, "the order accepted and reserved for it");

    // No hook: a mark that panics is the ordinary way accounting fails. What
    // matters is where the work is standing when it does.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = fixture
            .private
            .step_once(
                &mut fixture.keyboards,
                &mut |_, _| panic!("accounting failed"),
                &control_watchdog(),
            );
    }));
    assert!(outcome.is_err(), "the mark panicked");

    // The item is this instance's, not the lost frame's. Had it still been a
    // local when the mark ran, the accepted custody and the payload would have
    // gone with the unwind, and nothing would be left to say the order had
    // ever given it out.
    let Some(PrivateOrderedItem::Refused {
        sequence: _,
        refusal: PrivateExecutionRefusal::NotAttempted,
        route,
        ..
    }) = &fixture.private.terminal.current
    else {
        panic!("the exact work is still held, un-attempted")
    };
    assert_eq!(
        route.delivery,
        Some(delivery),
        "and it is the work that was accepted, not a reconstruction of it"
    );
    assert_eq!(
        fixture.durable.reserved().expect("a readable owner"),
        reserved_before,
        "its reservation is still held, so nothing was silently freed"
    );

    // And the order is honestly blocked on it rather than quietly moving on.
    assert!(matches!(
        fixture
            .private
            .step_once(&mut fixture.keyboards, &mut |_, _| Ok(()), &control_watchdog()),
        Err(XServerFrontendRouteError::OrderedItemUnresolved)
    ));

    // The obligation survives the instance, which is what retention is for.
    let settlement = fixture.private.shutdown();
    assert!(settlement.terminal_outstanding().expect("readable terminal inventory") >= 1);
    drop(settlement);
    assert_eq!(fixture.durable.terminal_inventories().expect("readable"), 1);
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn private_work_does_not_expire_because_it_waited() {
    let client = XServerFrontendClientId(1401);
    let surface = SurfaceId::new(1401, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1401);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, deliveries, registration, durable,
        _acks,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");
    let press_cell = admitted_cell(private, 1401);

    // Long past the legacy deadline, and nothing has been attempted for it --
    // so this is the age of a queue entry, not of a send. No writer has
    // blocked, because no writer has been given anything.
    let expired = private
        .broker
        .registry
        .input_recovery
        .recover(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            false,
        )
        .expect("the ledger to be readable");
    assert!(
        expired.is_empty(),
        "waiting in a queue is not a transport failure, and manufacturing an \
         outcome from it would report a delivery finished that nothing tried"
    );
    assert!(deliveries.try_recv().is_err());
    assert!(
        private
            .broker
            .registry
            .input_recovery
            .ticket(delivery)
            .is_some(),
        "the obligation is retained rather than answered"
    );

    // It still runs when its turn comes: retaining it is not shelving it.
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(1401)
    );
    assert_eq!(capsule.client(), client);

    // And a real cancellation still reaches it, because what was disabled is
    // the age producer and not the sweep.
    private
        .broker
        .registry
        .input_recovery
        .disconnect(client, XAuthorityInputDeliveryOutcome::ClientDisconnected)
        .expect("the ledger to be readable");
    assert_eq!(
        deliveries
            .try_recv()
            .expect("an established recipient fact")
            .delivery,
        delivery
    );
    drop(registration);
    drop(channels);
    drop(durable);
}
include!("review_private_deadline.rs");


#[test]
fn a_start_that_refuses_stops_before_the_effect() {
    let client = XServerFrontendClientId(1501);
    let surface = SurfaceId::new(1501, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1501);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");

    // Charging a start can refuse -- a budget is spent, and a spent budget is
    // a real answer. It has to be able to say so rather than being told after
    // the work has already run.
    let refused = fixture.private.step_once(
        &mut fixture.keyboards,
        &mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved),
        &control_watchdog(),
    );
    assert!(matches!(
        refused,
        Err(XServerFrontendRouteError::OrderedItemUnresolved)
    ));

    // Nothing was applied and nothing was lost: the work is this instance's,
    // un-attempted, and the order is honestly blocked on it.
    assert!(fixture.private.terminal.holds.is_empty());
    assert!(matches!(
        &fixture.private.terminal.current,
        Some(PrivateOrderedItem::Refused {
            refusal: PrivateExecutionRefusal::NotAttempted,
            ..
        })
    ));
    assert_eq!(claim_state(&fixture.private, delivery), (false, false));
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn nothing_runs_when_nothing_will_watch_it() {
    let client = XServerFrontendClientId(1502);
    let surface = SurfaceId::new(1502, 1);
    let delivery = XAuthorityInputDeliveryId::from_raw(1502);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(button_to(surface, delivery, 272, true))
        .expect("the order to accept it");

    // A supervisor that has not been sealed will not take an execution. The
    // watch exists for the case where a call does not come back, so running
    // without one is starting exactly the case it was meant to catch with
    // nothing left to catch it.
    let unsealed = private_watchdog::PrivateWatchdogOwner::prepare(0).expect("a watchdog");
    let step = fixture
        .private
        .step_once(&mut fixture.keyboards, &mut |_, _| Ok(()), &unsealed)
        .expect("a readable order");
    assert!(matches!(step, PrivateOrderedStep::Unwatched(_)));
    assert!(fixture.private.terminal.holds.is_empty());
    assert_eq!(
        claim_state(&fixture.private, delivery),
        (false, false),
        "the ledger never moved for it"
    );
    assert!(matches!(
        &fixture.private.terminal.current,
        Some(PrivateOrderedItem::Refused {
            refusal: PrivateExecutionRefusal::NotAttempted,
            ..
        })
    ));
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn a_send_counts_only_what_it_waited_on_this_recipient() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let mut state = X11OrderedSendState::default();

    // A recipient that is taking its bytes costs no waiting.
    state.begin_frame([7u8; 32].to_vec()).expect("nothing owed yet");
    send_pending_frame(&writer, &mut state).expect("a healthy send");
    assert!(state.frame_complete(), "the whole frame went out");
    assert_eq!(
        state.blocked(),
        Duration::ZERO,
        "nothing waited, so nothing is owed to a deadline"
    );

    // Nobody reads now. Seeded close to the limit rather than waiting out the
    // whole policy: what is under test is that real waiting accumulates onto
    // what this delivery already waited, and trips the bound.
    state.blocked = X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(60);
    let failure = loop {
        if state.frame_complete() {
            state.retire_frame().expect("the last frame finished");
        }
        if state.frame.is_none() {
            state
                .begin_frame([9u8; 1 << 16].to_vec())
                .expect("nothing owed");
        }
        match send_pending_frame(&writer, &mut state) {
            Ok(()) => continue,
            Err(failure) => break failure,
        }
    };
    let X11FrameSendFailure::Blocked { written, blocked } = failure else {
        panic!("a recipient taking nothing is the blocked case, not an io error")
    };
    assert!(
        blocked >= X_AUTHORITY_ORDERED_BLOCKED_LIMIT,
        "the bound is reached by measured waiting: {blocked:?}"
    );
    assert_eq!(blocked, state.blocked(), "the owner's accumulator is the one added to");
    assert!(written > 0 && !state.frame_complete(), "it stopped part way");
    drop(reader);
}

#[test]
fn a_frame_still_owed_bytes_cannot_be_abandoned() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    // Seeded close to the limit so the stall is reached without waiting out
    // the whole policy.
    let mut state = X11OrderedSendState {
        blocked: X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(60),
        ..X11OrderedSendState::default()
    };

    // Fill the buffer so a large frame stops part way through.
    loop {
        if state.frame_complete() {
            state.retire_frame().expect("it went whole");
        }
        if state.frame.is_none() {
            state.begin_frame([1u8; 1 << 16].to_vec()).expect("nothing owed");
        }
        if send_pending_frame(&writer, &mut state).is_err() {
            break;
        }
    }
    assert!(!state.frame_complete(), "a frame is still owed bytes");

    // Those bytes are an event's beginning and the recipient is waiting for
    // the rest of it. Writing a different frame now would put a second event's
    // opening bytes inside the first one's body, which an X11 client has no
    // way to notice.
    let refused = state
        .begin_frame([2u8; 32].to_vec())
        .expect_err("a partly sent frame cannot be walked away from");
    let X11FrameSendFailure::Incomplete { sent, len } = refused else {
        panic!("refused for being incomplete")
    };
    assert!(sent > 0 && sent < len);
    drop(reader);
}

#[test]
fn a_send_that_never_reported_blocks_everything_after_it() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let mut state = X11OrderedSendState::default();
    state.begin_frame([4u8; 16].to_vec()).expect("nothing owed yet");
    send_pending_frame(&writer, &mut state).expect("a healthy send");

    // The state a send leaves if it is interrupted between handing bytes to
    // the kernel and recording that it did.
    state
        .frame
        .as_mut()
        .expect("a frame in hand")
        .progress = X11OrderedSendProgress::Unknown { from: 8 };

    // It cannot be resumed: resuming from the offset before the send would
    // put an event's middle after its own middle.
    let failure = send_pending_frame(&writer, &mut state)
        .expect_err("an unreported send is not a resumable one");
    assert!(matches!(failure, X11FrameSendFailure::Interrupted));

    // And it cannot be stepped over either. A following frame would be
    // appended to something nobody can describe, so the unknown is not
    // something a new frame may clear.
    let refused = state
        .begin_frame([5u8; 32].to_vec())
        .expect_err("an unknown wire position is not a finished frame");
    assert!(matches!(refused, X11FrameSendFailure::Interrupted));
    assert_eq!(
        state.frame.as_ref().expect("still held").progress,
        X11OrderedSendProgress::Unknown { from: 8 },
        "and it stays unknown rather than being restored to something believable"
    );

    let error = x11_ordered_frame_error("failed to write an ordered event", failure);
    assert!(error.client_failure && !error.service_shutdown);
    drop(reader);
}

#[test]
fn the_frame_a_resume_continues_is_the_one_it_began() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let mut state = X11OrderedSendState::default();

    // The frame is the state's, not a slice a caller brings back each time.
    // There is no call that could offer different bytes behind the same
    // offset, which is what would send the tail of one event as though it
    // were the tail of another.
    state.begin_frame([0xAB; 64].to_vec()).expect("nothing owed yet");
    send_pending_frame(&writer, &mut state).expect("a healthy send");
    assert!(state.frame_complete());
    let mut seen = [0u8; 64];
    std::io::Read::read_exact(&mut &reader, &mut seen).expect("the frame");
    assert!(
        seen.iter().all(|byte| *byte == 0xAB),
        "the recipient received the frame that was begun, whole"
    );

    // And completion is derived from what was sent rather than declared: a
    // fresh frame is not complete until its own bytes have gone.
    state.retire_frame().expect("the last one went whole");
    state.begin_frame([0xCD; 8].to_vec()).expect("nothing owed");
    assert!(!state.frame_complete(), "nothing of this one has gone yet");
    drop(reader);
}

#[test]
fn the_socket_every_writer_shares_is_left_alone() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    // The ordered path must not reach its recipient by changing a socket the
    // control and protocol writers hold too. A per-call flag affects the one
    // send; a timeout or a mode belongs to the socket, and every other writer
    // would inherit it without the frame custody this path relies on.
    assert!(
        writer.write_timeout().expect("a readable socket").is_none(),
        "no send timeout is installed on the shared socket"
    );
    let mut state = X11OrderedSendState::default();
    state.begin_frame([3u8; 16].to_vec()).expect("nothing owed yet");
    send_pending_frame(&writer, &mut state).expect("a healthy send");
    assert!(
        writer.write_timeout().expect("a readable socket").is_none(),
        "and sending did not install one either"
    );
    assert!(
        !rustix::fs::fcntl_getfl(&writer)
            .expect("a readable descriptor")
            .contains(rustix::fs::OFlags::NONBLOCK),
        "nor did it put the shared socket in non-blocking mode"
    );
    drop(reader);
}

#[test]
fn a_departed_recipient_is_a_failed_recipient_not_a_failed_server() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    drop(reader);
    let mut state = X11OrderedSendState::default();
    state.begin_frame([1u8; 32].to_vec()).expect("nothing owed yet");

    // The send carries NOSIGNAL, so a peer that has gone gives an error rather
    // than killing the process with SIGPIPE -- a writer that died here would
    // take every other client's service with it.
    let failure = send_pending_frame(&writer, &mut state)
        .expect_err("a departed peer cannot take bytes");
    assert!(matches!(failure, X11FrameSendFailure::Io(_)));

    // And the reading of it keeps the failure with the connection. The
    // ordinary peer-write reading gives anything unrecognised the fatal class,
    // so one client's exit would otherwise end the service for all of them.
    let error = x11_ordered_frame_error("failed to write an ordered event", failure);
    assert!(
        error.client_disconnect || error.client_failure,
        "the failure belongs to this connection"
    );
    assert!(!error.service_shutdown, "and not to the service");
}

#[test]
fn one_terminal_step_disposes_one_entry_and_charges_for_it() {
    let client = XServerFrontendClientId(1601);
    let surface = SurfaceId::new(1601, 1);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let _watch = watch.as_ref().expect("a sealed watch");
    let second = private
        .ingress_for(client, DeviceId::from_raw(2))
        .expect("a second ingress");
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(16011),
            272,
            true,
        ))
        .expect("the order to accept it");
    second
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(16012),
            273,
            true,
        ))
        .expect("the order to accept it");
    let watch = control_watchdog();
    for _ in 0..2 {
        private
            .step_once(keyboards, &mut |_, _| Ok(()), &watch)
            .expect("a readable order");
    }
    assert_eq!(private.terminal.turn.len(), 2);

    // An empty order is not a step and costs nothing.
    let charged = std::cell::RefCell::new(Vec::new());
    let mut charge = |sequence: Option<crate::ReadySequence>, _: std::time::Instant| {
        // None is a visit that names no ordered entry -- a proof recording, or
        // handing an event over. Both are kept, because what this control is
        // about is that every visit is charged for exactly once.
        charged.borrow_mut().push(sequence);
        Ok(())
    };
    let step = private.deliver_one(&mut charge).expect("a step");
    let PrivateDeliveryStep::Advanced { sequence, report } = step else {
        panic!("one entry disposed")
    };
    assert_eq!(
        *charged.borrow(),
        vec![Some(sequence)],
        "charged once, for the entry taken"
    );
    // The entry was disposed of and its outcome observed; whether anything
    // reached a queue is the dispatch's fact, not this report's.
    assert!(
        report
            .expect("a disposed entry reports")
            .completion
            .is_some(),
        "the entry's own outcome was observed exactly once"
    );
    assert_eq!(
        private.terminal.turn.len(),
        1,
        "exactly one entry left the turn"
    );

    let step = private.deliver_one(&mut charge).expect("a step");
    assert!(matches!(step, PrivateDeliveryStep::Advanced { .. }));
    assert_eq!(charged.borrow().len(), 2);
    assert_ne!(charged.borrow()[0], charged.borrow()[1], "a different entry");
    assert!(
        charged.borrow().iter().all(Option::is_some),
        "an entry's own visit names it"
    );

    // Both entries are disposed of, and the events they decided are still
    // owed. Those handovers are visits of their own, charged for and naming no
    // entry, because the entry that decided the event is already gone.
    for expected in [true, true] {
        let step = private.deliver_one(&mut charge).expect("a step");
        assert!(matches!(
            step,
            PrivateDeliveryStep::Dispatched {
                enqueued,
                relinquished: false
            } if enqueued == expected
        ));
        assert_eq!(
            *charged.borrow().last().expect("a charge for the visit"),
            None,
            "a handover names no ordered entry"
        );
    }
    assert_eq!(channels.ordered.try_iter().count(), 2, "both events went");

    // Nothing waiting is its own answer, and takes nothing.
    let before = charged.borrow().len();
    assert!(matches!(
        private.deliver_one(&mut charge).expect("a step"),
        PrivateDeliveryStep::Idle
    ));
    assert_eq!(charged.borrow().len(), before, "an empty turn is not a step");
    drop(registration);
    drop(channels);
    drop(durable);
}

#[test]
fn a_refused_entry_advancing_is_a_step_with_nothing_to_report() {
    let client = XServerFrontendClientId(1603);
    let surface = SurfaceId::new(1603, 1);
    let mut fixture = ordered_ingress_fixture(client, surface);
    // A control command is an operation the ordered path does not execute, so
    // the order parks behind it.
    fixture
        .private
        .control_producer()
        .submit(configure(client, surface, 16031))
        .expect("the order to accept it");
    let turn = fixture
        .private
        .route_pending_ordered(&mut fixture.keyboards, &control_watchdog())
        .expect("a readable order");
    fixture.private.terminal.delivering.extend(turn);

    let mut charged = 0;
    let step = fixture
        .private
        .deliver_one(&mut |_, _| {
            charged += 1;
            Ok(())
        })
        .expect("a step");
    let PrivateDeliveryStep::Advanced { report, .. } = step else {
        panic!("the entry advanced")
    };
    assert!(
        report.is_none(),
        "nothing was delivered, so there is nothing to report"
    );
    assert_eq!(
        charged, 1,
        "moving it to retained inventory was still a real step, and a caller \
         reading no report as no work would charge nothing for work it did"
    );
    assert_eq!(fixture.private.terminal.undelivered.len(), 1);
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

#[test]
fn a_refused_charge_leaves_the_entry_where_it_was() {
    let client = XServerFrontendClientId(1604);
    let surface = SurfaceId::new(1604, 1);
    let mut fixture = ordered_ingress_fixture(client, surface);
    fixture
        .ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(1604),
            272,
            true,
        ))
        .expect("the order to accept it");
    fixture
        .private
        .step_once(&mut fixture.keyboards, &mut |_, _| Ok(()), &control_watchdog())
        .expect("a readable order");

    let refused = fixture
        .private
        .deliver_one(&mut |_, _| Err(XServerFrontendRouteError::OrderedItemUnresolved));
    assert!(matches!(
        refused,
        Err(XServerFrontendRouteError::OrderedItemUnresolved)
    ));
    assert_eq!(
        fixture.private.terminal.delivering.len(),
        1,
        "chosen and still owned: a refusal to charge is not a disposition"
    );
    assert!(fixture.private.terminal.undelivered.is_empty());
    assert!(fixture.channels.input.try_recv().is_err(), "and nothing was sent");
    drop(fixture.registration);
    drop(fixture.durable);
}
include!("private_lifecycle_integration.rs");


#[test]
fn a_failed_wait_is_not_a_recipient_that_blocked() {
    // Producing a poll failure against a live owned socket is not something a
    // control here can arrange, so what is exercised is the reading of it --
    // which is where the conflation would do its damage.
    let failure = X11FrameSendFailure::WaitFailed(std::io::Error::from(
        std::io::ErrorKind::InvalidInput,
    ));
    let error = x11_ordered_frame_error("failed to write an ordered event", failure);

    // Not a client failure. Nothing was established about this recipient: it
    // was never asked and it never declined, so ending its connection on the
    // strength of a broken syscall would blame the wrong party.
    assert!(
        !error.client_failure && !error.client_disconnect,
        "a wait that could not be performed says nothing about the recipient"
    );

    // And it is kept apart from blocking in the type itself, which is what
    // stops a deadline being built out of a failed wait.
    let blocked = x11_ordered_frame_error(
        "failed to write an ordered event",
        X11FrameSendFailure::Blocked {
            written: 8,
            blocked: X_AUTHORITY_ORDERED_BLOCKED_LIMIT,
        },
    );
    assert!(
        blocked.client_failure,
        "a recipient that would not take its bytes is the one that failed"
    );
}

/// The writer fixture uses a real resolved source emission. These controls
/// still prove writer custody, not production producer/consumer completion.
/// A capsule, and the endpoint of the registration that produced it.
///
/// The witness comes from the registration, never from the capsule: a writer
/// whose expectation was read off the capsule would admit anything.
fn capsule_and_endpoint(
    delivery: u64,
) -> (XAuthorityOrderedDelivery, PrivateEndpointIdentity) {
    let (emission, endpoint) =
        private_native_tests::emission_and_endpoint_for_writer_fixture(delivery);
    (
        XAuthorityOrderedDelivery::from_emission(emission).unwrap(),
        endpoint,
    )
}

#[test]
fn a_writer_answers_through_the_one_authority_that_owns_the_answer() {
    // Writing into a completion cell directly recorded an answer the ledger
    // never saw: its ticket stayed unanswered and no ordinary observer was
    // told, so a later disconnect could set a different terminal outcome while
    // the cell still said the first one. Two accounts of one delivery,
    // disagreeing. The finalizer adjudicates in one place, and this control
    // checks every account rather than the cell alone.
    let (capsule, endpoint, recovery, receipts) = answerable_capsule(17301);
    let delivery = capsule.delivery();
    let client = capsule.client();
    let cell = recovery
        .completion_for(delivery)
        .expect("a readable ledger")
        .expect("the admission's own completion");

    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let mut in_flight = None;
    let mut refused = None;
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("the queue to accept it");

    assert!(cell.answer().is_none(), "nothing is answered before it is sent");
    for _ in 0..32 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => break,
            other => panic!("a healthy recipient took its bytes: {other:?}"),
        }
    }

    // EVERY ACCOUNT AGREES. The cell, the ledger's own ticket, and the
    // ordinary observer.
    let answer = cell.answer().expect("the cell carries the adjudicated answer");
    assert_eq!(answer.outcome, XAuthorityInputDeliveryOutcome::Flushed);
    assert_eq!(answer.delivery, delivery);
    assert_eq!(answer.client, client);
    let notified = receipts
        .try_recv()
        .expect("the ordinary observer was told, through the same adjudication");
    assert_eq!(notified, answer, "and told the same thing");
    assert!(
        recovery.ticket(delivery).is_none() || recovery.completion_for(delivery).is_ok(),
        "the ledger's own account was updated rather than bypassed"
    );
    drop(peer);
}

#[test]
fn a_flushed_delivery_is_retired_once_so_the_next_one_can_be_served() {
    // Reporting a flush without giving the slot up would report that same
    // flush for ever and nothing behind it would ever be served. BOTH capsules
    // carry real origin-bound finalizers and nothing here clears the slot:
    // the retirement this asserts is the one production performs.
    // BOTH FROM ONE REGISTRATION. Two fixtures would be two endpoints whose
    // numbers happen to agree, and this writer serves one endpoint.
    let (first, second, endpoint, _recovery_one, _recovery_two, _receipts_one, _receipts_two) =
        two_answerable_capsules(17201, 17202);
    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let mut in_flight = None;
    let mut refused = None;
    let served = XAuthorityServedConnection::retained(endpoint);
    assert_eq!(
        second.recipient(),
        first.recipient(),
        "both are owed to the one connection this writer serves"
    );
    sender.send(first).expect("the queue to accept the first");
    sender.send(second).expect("the queue to accept the second");

    let mut flushes = 0;
    let mut delivered = Vec::new();
    for _ in 0..64 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => flushes += 1,
            X11OrderedServeStep::Idle => break,
            other => panic!("every capsule here can be answered: {other:?}"),
        }
        if let Some(held) = in_flight.as_ref() {
            let id = held.delivery().delivery();
            if delivered.last() != Some(&id) {
                delivered.push(id);
            }
        }
    }
    assert_eq!(flushes, 2, "each delivery flushed exactly once");
    assert_eq!(delivered.len(), 2, "and the second was reached after the first");
    assert!(in_flight.is_none(), "nothing is left held");
    drop(peer);
}

/// A capsule whose writer can actually answer: a real admission in a real
/// ledger, and a finalizer built from that admission's own completion.
///
/// Controls that built a completion out of thin air could not see whether the
/// ledger agreed with the writer, because there was no ledger behind it.
/// Two answerable capsules owed to ONE endpoint.
fn two_answerable_capsules(
    first: u64,
    second: u64,
) -> (
    XAuthorityOrderedDelivery,
    XAuthorityOrderedDelivery,
    PrivateEndpointIdentity,
    InputRecovery,
    InputRecovery,
    Receiver<XAuthorityClientInputDelivery>,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (one, two, endpoint) =
        private_native_tests::emissions_for_one_writer_fixture(first, second);
    let answer = |emission| {
        let mut capsule = XAuthorityOrderedDelivery::from_emission(emission).unwrap();
        let id = capsule.delivery();
        let client = capsule.client();
        let (recovery, receipts) = claim_fixture(id);
        let completion = recovery
            .completion_for(id)
            .expect("a readable ledger")
            .expect("the admission minted its completion");
        capsule.carry_finalizer(Arc::new(finalizer_from_held(
            &recovery, &completion, id, client,
        )));
        (capsule, recovery, receipts)
    };
    let (capsule_one, recovery_one, receipts_one) = answer(one);
    let (capsule_two, recovery_two, receipts_two) = answer(two);
    (
        capsule_one,
        capsule_two,
        endpoint,
        recovery_one,
        recovery_two,
        receipts_one,
        receipts_two,
    )
}

fn answerable_capsule(
    delivery: u64,
) -> (
    XAuthorityOrderedDelivery,
    PrivateEndpointIdentity,
    InputRecovery,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let (mut capsule, endpoint) = capsule_and_endpoint(delivery);
    let id = capsule.delivery();
    let client = capsule.client();
    let (recovery, receipts) = claim_fixture(id);
    let completion = recovery
        .completion_for(id)
        .expect("a readable ledger")
        .expect("the admission minted its completion");
    capsule.carry_finalizer(Arc::new(finalizer_from_held(
        &recovery, &completion, id, client,
    )));
    (capsule, endpoint, recovery, receipts)
}

#[test]
fn an_adjudication_reports_what_the_authority_did_with_it() {
    // A boolean could not say this. Reporting success whenever the authority
    // was called said an answer had been recorded when it had been silently
    // declined; reporting failure for an admission already answered stranded
    // a writer that had done everything asked of it.
    let delivery = XAuthorityInputDeliveryId::from_raw(17401);
    let client = XServerFrontendClientId(17401);
    let other = XServerFrontendClientId(17402);
    let (recovery, _receipts) = claim_fixture(delivery);
    recovery.register(client).unwrap();
    assert!(
        recovery.bind(Some(delivery), client).unwrap(),
        "the delivery is bound to the recipient it reached"
    );
    let completion = recovery
        .completion_for(delivery)
        .expect("a readable ledger")
        .expect("the admission's own completion");

    // A finalizer naming somebody else. The authority declines it, and that
    // decline is reported as a refusal rather than as a recorded answer.
    let foreign = finalizer_from_held(&recovery, &completion, delivery, other);
    assert_eq!(
        foreign.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::Refused,
        "an answer the authority declined is not an answer it recorded"
    );
    assert!(
        completion.answer().is_none(),
        "and nothing was written for the client it named"
    );

    // The right one is recorded.
    let own = finalizer_from_held(&recovery, &completion, delivery, client);
    assert_eq!(
        own.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::Answered
    );
    assert_eq!(
        completion.answer().map(|receipt| receipt.outcome),
        Some(XAuthorityInputDeliveryOutcome::Flushed)
    );

    // Asked again, it is already answered -- not refused. A writer told
    // otherwise would hold a finished delivery for ever.
    assert_eq!(
        own.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::AlreadyAnswered,
        "an admission that already has its answer owes nothing further"
    );
}

#[test]
fn a_rejected_offer_stays_refused_even_when_older_work_is_deferred() {
    // Reading the state after the fact could not tell this offer's fate from
    // somebody else's: with an earlier cancellation held under a claim, a
    // wrong-client answer was reported as Deferred and a writer took that as
    // permission to retire a delivery nothing had accepted.
    let delivery = XAuthorityInputDeliveryId::from_raw(17501);
    let client = XServerFrontendClientId(17501);
    let other = XServerFrontendClientId(17502);
    let (recovery, receipts) = claim_fixture(delivery);
    recovery.register(client).unwrap();
    assert!(recovery.bind(Some(delivery), client).unwrap());
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed
    );
    // An earlier cancellation, held because the claim is out.
    recovery
        .finish(
            client,
            Some(delivery),
            XAuthorityInputDeliveryOutcome::EpochRevoked,
        )
        .expect("the cancellation is offered");
    assert!(
        receipts.try_recv().is_err(),
        "and held rather than published, because the claim is out"
    );

    let completion = recovery
        .completion_for(delivery)
        .expect("a readable ledger")
        .expect("the admission's own completion");
    let foreign = finalizer_from_held(&recovery, &completion, delivery, other);

    // THE OFFER IS REJECTED, and the older held cancellation is not its fate.
    assert_eq!(
        foreign.finalize(XAuthorityInputDeliveryOutcome::Flushed),
        PrivateAdjudication::Refused,
        "a declined offer is refused, whatever else is held for this delivery"
    );
    assert!(
        completion.answer().is_none(),
        "and nothing was recorded for it"
    );

    // The older cancellation is still the one held, untouched by the offer.
    recovery.resolve_claim(Some(delivery), false);
    let published = receipts
        .try_recv()
        .expect("the held cancellation stands once the claim resolves");
    assert_eq!(
        published.outcome,
        XAuthorityInputDeliveryOutcome::EpochRevoked,
        "the rejected Flushed never displaced it"
    );
    assert_eq!(published.client, client);
}

#[test]
fn serving_a_whole_delivery_reports_a_flush_and_nothing_more() {
    // A flush means every frame went and the answer was adjudicated. It does
    // not mean the recipient read them.
    let (capsule, endpoint, _recovery, _receipts) = answerable_capsule(17101);
    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let mut in_flight = None;
    let mut refused = None;
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("the queue to accept it");

    let mut flushed = false;
    for _ in 0..16 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => {
                flushed = true;
                break;
            }
            other => panic!("a healthy recipient took its bytes: {other:?}"),
        }
    }
    assert!(flushed, "every frame of the delivery went out");
    drop(peer);
}

#[test]
fn a_recipient_that_will_not_take_its_bytes_ends_the_connection_before_returning() {
    // THE OBLIGATION write_one_ordered_frame REFUSES TO DISCHARGE. Every
    // failure leaves a frame owed or its extent unknown, so the socket is
    // closed before this returns. A caller that released output
    // serialization after such a step without the socket being closed would
    // admit another writer into the body of a half-written event.
    let (capsule, endpoint) = capsule_and_endpoint(17102);
    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let mut in_flight = None;
    let mut refused = None;
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("the queue to accept it");

    // A recipient that is gone: the send fails rather than blocking, which is
    // the failure this control can reach deterministically.
    drop(peer);

    let mut ended = None;
    for _ in 0..16 {
        match serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ) {
            X11OrderedServeStep::Advanced | X11OrderedServeStep::Flushed => {}
            X11OrderedServeStep::Unanswered => {
                // No finalizer on this capsule, so nothing adjudicates its
                // answer and custody is retained. Released here so the loop
                // can reach the failure it is about.
                in_flight = None;
            }
            step @ X11OrderedServeStep::Ended { .. } => {
                ended = Some(step);
                break;
            }
            X11OrderedServeStep::Idle => break,
            X11OrderedServeStep::AdmissionRefused(_) => {
                panic!("this queue carries only what this connection is owed")
            }
            X11OrderedServeStep::TransportUnavailable
            | X11OrderedServeStep::Unterminated
            | X11OrderedServeStep::Closing
            | X11OrderedServeStep::Stopped
            | X11OrderedServeStep::WireBarred => {
                panic!("this connection is live, serving, unbarred and not stopping")
            }
        }
    }
    let Some(X11OrderedServeStep::Ended { outcome, shutdown }) = ended else {
        // A departed peer may accept the bytes into a closed socket's buffer
        // on some kernels; if it did, this control has nothing to say and
        // says so rather than asserting something it did not observe.
        return;
    };
    assert_eq!(
        outcome,
        XAuthorityInputDeliveryOutcome::WriteFailed,
        "a send that failed is a writer fact, not a recipient settlement"
    );
    assert!(
        shutdown,
        "and the connection was ended before this returned, not left for a \
         caller to remember"
    );
}

#[test]
fn a_taken_delivery_lands_where_it_will_be_answered_for() {
    // Both from one registration: this control is about the slot, and a second
    // registration's capsule would be refused before the slot was consulted.
    let (one, two, endpoint) =
        private_native_tests::emissions_for_one_writer_fixture(17011, 17012);
    let first = XAuthorityOrderedDelivery::from_emission(one).unwrap();
    let second = XAuthorityOrderedDelivery::from_emission(two).unwrap();
    let client = first.client();
    let served = XAuthorityServedConnection::retained(endpoint);
    let (sender, queue) = sync_channel(4);
    let mut in_flight = None;
    let mut refused = None;

    // Nothing waiting is its own answer, and takes nothing.
    assert_eq!(
        take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused),
        Err(X11OrderedTakeRefusal::Empty)
    );
    assert!(in_flight.is_none() && refused.is_none());

    sender
        .send(first)
        .expect("the queue to accept it");
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused).expect("one waiting");
    let held = in_flight.as_ref().expect("taken into storage");
    assert_eq!(held.delivery().client(), client);
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(17011),
        "the capsule itself is held, not parts copied out of it"
    );
    assert_eq!(held.frame_index(), 0);
    assert_eq!(held.blocked(), Duration::ZERO);

    // A second is refused rather than queued behind the first. This writer
    // answers for what it holds until that is finished, and taking another
    // would leave the first owed by nobody with its frames half-written.
    sender
        .send(second)
        .expect("the queue to accept it");
    assert_eq!(
        take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused),
        Err(X11OrderedTakeRefusal::InFlight),
        "the slot is answered before anything is received, so this never \
         reaches the endpoint comparison"
    );
    assert_eq!(
        in_flight
            .as_ref()
            .expect("still held")
            .delivery()
            .delivery(),
        XAuthorityInputDeliveryId::from_raw(17011),
        "and the one in hand is untouched"
    );

    // A producer that has gone is not an empty queue: one says to look again,
    // the other says nothing more is coming.
    drop(sender);
    in_flight = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("the queued one is still there");
    in_flight = None;
    assert_eq!(
        take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused),
        Err(X11OrderedTakeRefusal::Closed)
    );
    assert!(refused.is_none(), "nothing here was for another connection");
}

#[test]
fn a_frame_index_does_not_move_past_an_unfinished_frame() {
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1702);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender
        .send(capsule)
        .expect("the queue to accept it");
    let mut in_flight = None;
    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");
    let held = in_flight.as_mut().expect("taken");

    // No frame in hand at all is not a finished one.
    assert!(held.advance_frame().is_err());
    assert_eq!(held.frame_index(), 0);

    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let frame = held
        .delivery()
        .emission()
        .encode_frame(0, XByteOrder::LittleEndian, 1)
        .expect("the emission's first frame");
    held.send.begin_frame(frame).expect("nothing owed");

    // Begun and not sent is not finished either: the recipient is waiting for
    // bytes this delivery still owes it.
    assert!(held.advance_frame().is_err());
    assert_eq!(held.frame_index(), 0);

    send_pending_frame(&writer, &mut held.send).expect("a healthy send");
    held.advance_frame().expect("the frame in hand went out whole");
    assert_eq!(held.frame_index(), 1, "and only then does the next one begin");
    drop(reader);
}

#[test]
fn one_completed_frame_is_advanced_past_exactly_once() {
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1703);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender
        .send(capsule)
        .expect("the queue to accept it");
    let mut in_flight = None;
    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");
    let held = in_flight.as_mut().expect("taken");
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");

    let frame = held
        .delivery()
        .emission()
        .encode_frame(0, XByteOrder::LittleEndian, 1)
        .expect("the emission's first frame");
    held.send.begin_frame(frame).expect("nothing owed");
    send_pending_frame(&writer, &mut held.send).expect("a healthy send");
    held.advance_frame().expect("the frame went out whole");
    assert_eq!(held.frame_index(), 1);

    // The same completed frame cannot be advanced past twice. An index that
    // moved again while nothing had been begun would count a frame that was
    // never started, and the emission frame it skipped would never be sent at
    // all -- an event silently missing from a delivery that reported itself
    // finished.
    let refused = held
        .advance_frame()
        .expect_err("nothing is in hand to advance past");
    assert!(matches!(refused, X11FrameSendFailure::NoFrame));
    assert_eq!(held.frame_index(), 1, "and the index did not move");
    drop(reader);
}

#[test]
fn two_frames_of_one_delivery_reach_the_wire_in_order_and_whole() {
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1704);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender
        .send(capsule)
        .expect("the queue to accept it");
    let mut in_flight = None;
    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");
    let held = in_flight.as_mut().expect("taken");
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");

    let emission = held.delivery().emission();
    // This fixture's emission has one record, so these are two encodings of
    // the same record with different transport sequences. That makes this a
    // control over frame CUSTODY -- two frames taken, sent, retired and read
    // back in order with nothing of the first carried into the second -- and
    // not evidence that the writer walks distinct emission records. A
    // multi-form emission is what would show that, and this fixture cannot
    // produce one.
    assert_eq!(
        emission.frame_count(),
        1,
        "stated rather than assumed: one record, encoded twice"
    );
    let first = emission
        .encode_frame(0, XByteOrder::LittleEndian, 1)
        .expect("a first frame");
    let second = emission
        .encode_frame(0, XByteOrder::LittleEndian, 2)
        .expect("a second frame");
    let first_bytes = first.as_bytes().to_vec();
    let second_bytes = second.as_bytes().to_vec();
    assert_ne!(
        first_bytes, second_bytes,
        "the sequence is in the bytes, so these are distinguishable on the wire"
    );

    held.send.begin_frame(first).expect("nothing owed");
    send_pending_frame(&writer, &mut held.send).expect("the first frame");
    held.advance_frame().expect("the first went whole");

    held.send.begin_frame(second).expect("the first was retired");
    send_pending_frame(&writer, &mut held.send).expect("the second frame");
    held.advance_frame().expect("the second went whole");
    assert_eq!(held.frame_index(), 2);

    let mut seen = vec![0u8; first_bytes.len() + second_bytes.len()];
    std::io::Read::read_exact(&mut &reader, &mut seen).expect("both frames");
    assert_eq!(
        &seen[..first_bytes.len()],
        first_bytes.as_slice(),
        "the first frame, whole and first"
    );
    assert_eq!(
        &seen[first_bytes.len()..],
        second_bytes.as_slice(),
        "then the second, with nothing of the first carried into it"
    );
    drop(reader);
}

#[test]
fn a_delivery_is_written_one_frame_at_a_time_and_then_is_written() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1801);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("accepted");
    let mut in_flight = None;

    // Nothing in flight is its own answer.
    assert_eq!(
        write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 1)
            .expect("a step"),
        X11OrderedWriteStep::Idle
    );

    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");
    let frames = in_flight
        .as_ref()
        .expect("taken")
        .delivery()
        .emission()
        .frame_count();
    for frame in 0..frames {
        assert_eq!(
            write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 1)
                .expect("a step"),
            X11OrderedWriteStep::Advanced { frame },
            "one frame per call, in order"
        );
    }

    // Every frame this delivery owed has gone. That says the bytes went and
    // nothing else: whether the recipient received them is the writer's own
    // outcome to establish, and whether the debt is settled is a question
    // neither this nor a queue can answer.
    assert_eq!(
        write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 1)
            .expect("a step"),
        X11OrderedWriteStep::Wrote
    );
    drop(reader);
}

#[test]
fn a_stalled_frame_is_resumed_rather_than_encoded_again() {
    let (writer, reader) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let (sender, queue) = sync_channel(1);
    let (capsule, endpoint) = capsule_and_endpoint(1802);
    let served = XAuthorityServedConnection::retained(endpoint);
    sender.send(capsule).expect("accepted");
    let mut in_flight = None;
    let mut refused = None;
    take_ordered_delivery(&queue, &served, &mut in_flight, &mut refused)
        .expect("one waiting");

    // Fill the recipient's buffer so this delivery's frame cannot go out
    // whole, and seed the accumulator so the stall is reached quickly.
    let filler = vec![0u8; 1 << 16];
    // Seeded so the filler gives up as soon as the buffer is full, rather
    // than waiting out its own policy: what is being arranged here is a full
    // recipient, not a measurement.
    let mut filling = X11OrderedSendState {
        blocked: X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(60),
        ..X11OrderedSendState::default()
    };
    while {
        if filling.frame_complete() {
            filling.retire_frame().expect("it went");
        }
        if filling.frame.is_none() {
            filling.begin_frame(filler.clone()).expect("nothing owed");
        }
        send_pending_frame(&writer, &mut filling).is_ok()
    } {}

    let held = in_flight.as_mut().expect("taken");
    held.send.blocked = X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(60);
    let stalled = write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 7)
        .expect_err("the recipient is taking nothing");
    assert!(matches!(
        stalled,
        X11OrderedWriteFailure::Send(X11FrameSendFailure::Blocked { .. })
    ));

    // The frame stays in hand with what the wire took of it. A second call
    // must resume that exact frame: encoding again would produce a second copy
    // of the frame those bytes came from and continue into the middle of it.
    let held = in_flight.as_mut().expect("still in flight");
    assert!(held.send.frame.is_some(), "the frame is still in hand");
    assert_eq!(held.frame_index(), 0, "and it has not been advanced past");

    // The recipient starts reading and this delivery is asked again. The
    // second call has to CONTINUE the frame in hand. A call that encoded again
    // would be asking to begin a frame while one is still owed, and the frame
    // custody refuses exactly that -- so a fresh encoding cannot even reach
    // the socket, and what it would have produced is a second copy of the
    // bytes the wire already holds part of.
    held.send.blocked = Duration::ZERO;
    let mut drained = vec![0u8; 1 << 20];
    let _ = std::io::Read::read(&mut &reader, &mut drained).expect("the recipient reads");
    let resumed = write_one_ordered_frame(&writer, &mut in_flight, XByteOrder::LittleEndian, 7)
        .expect("the frame in hand is continued");
    assert_eq!(
        resumed,
        X11OrderedWriteStep::Advanced { frame: 0 },
        "continuing finished the frame that was already part way out; a call \
         that encoded again would be asking to begin a frame while one is \
         still owed, which the frame custody refuses outright"
    );
    drop(reader);
}

#[test]
fn a_recipient_taking_nothing_leaves_another_recipient_and_the_runner_working() {
    // SCOPE, stated because the obvious reading is wider than what this
    // establishes. It shows that a writer whose recipient takes nothing
    // returns Blocked with its delivery still owned, and that a second
    // recipient's writer and the service runner both complete their work on a
    // run where that is true. It does NOT establish that the first writer was
    // inside its readiness wait while the others ran -- that interval is not
    // observable from here without a fault probe -- and it does not establish
    // that a supervisor can interrupt a writer that is still waiting.
    let (writer_a, reader_a) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let (writer_b, reader_b) = std::os::unix::net::UnixStream::pair().expect("a socketpair");
    let (sender_a, queue_a) = sync_channel(1);
    let (sender_b, queue_b) = sync_channel(1);
    let (capsule_a, endpoint_a) = capsule_and_endpoint(1901);
    let (capsule_b, endpoint_b) = capsule_and_endpoint(1902);
    let served_a = XAuthorityServedConnection::retained(endpoint_a);
    let served_b = XAuthorityServedConnection::retained(endpoint_b);
    sender_a.send(capsule_a).expect("accepted");
    sender_b.send(capsule_b).expect("accepted");

    let reached = Arc::new(AtomicBool::new(false));
    let signal = reached.clone();
    let stalled = std::thread::spawn(move || {
        let mut filling = X11OrderedSendState {
            blocked: X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(200),
            ..X11OrderedSendState::default()
        };
        let filler = vec![0u8; 1 << 16];
        while {
            if filling.frame_complete() {
                filling.retire_frame().expect("it went");
            }
            if filling.frame.is_none() {
                filling.begin_frame(filler.clone()).expect("nothing owed");
            }
            send_pending_frame(&writer_a, &mut filling).is_ok()
        } {}

        let mut in_flight = None;
        let mut refused = None;
        take_ordered_delivery(&queue_a, &served_a, &mut in_flight, &mut refused)
            .expect("one waiting");
        in_flight.as_mut().expect("taken").send.blocked =
            X_AUTHORITY_ORDERED_BLOCKED_LIMIT - Duration::from_millis(200);
        // Set before the call, and it says only that: this writer is about to
        // attempt a send to a recipient that is taking nothing. It is not a
        // claim about where inside that call the thread is.
        signal.store(true, Ordering::Release);
        let outcome =
            write_one_ordered_frame(&writer_a, &mut in_flight, XByteOrder::LittleEndian, 1);
        (outcome, in_flight, writer_a)
    });

    let limit = std::time::Instant::now() + Duration::from_secs(5);
    while !reached.load(Ordering::Acquire) && std::time::Instant::now() < limit {
        std::thread::yield_now();
    }
    assert!(reached.load(Ordering::Acquire), "A's writer began its attempt");

    // B's recipient reads, so B's delivery goes out whole -- and the bytes are
    // checked off the socket rather than inferred from the step's answer.
    let mut b_flight = None;
    let mut b_refused = None;
    take_ordered_delivery(&queue_b, &served_b, &mut b_flight, &mut b_refused)
        .expect("one waiting");
    let expected = {
        let held = b_flight.as_ref().expect("taken");
        let emission = held.delivery().emission();
        (0..emission.frame_count())
            .map(|index| {
                emission
                    .encode_frame(index, XByteOrder::LittleEndian, 1)
                    .expect("a frame")
                    .as_bytes()
                    .to_vec()
            })
            .collect::<Vec<_>>()
    };
    for _ in 0..expected.len() {
        assert!(matches!(
            write_one_ordered_frame(&writer_b, &mut b_flight, XByteOrder::LittleEndian, 1)
                .expect("B is reading"),
            X11OrderedWriteStep::Advanced { .. }
        ));
    }
    let mut seen = vec![0u8; expected.iter().map(Vec::len).sum()];
    std::io::Read::read_exact(&mut &reader_b, &mut seen).expect("B's bytes");
    assert_eq!(
        seen,
        expected.concat(),
        "B received exactly its delivery's frames, in order"
    );

    // The service runner shares nothing with either socket and completes.
    let client = XServerFrontendClientId(1903);
    let surface = SurfaceId::new(1903, 1);
        let PreparedOrderedFixture {
        mut runner, ingress, channels, registration, durable,
        _acks,
        deliveries: _deliveries,
        selections: _selections,
        client: _client,
        surface: _surface,
        window: _window,
        namespace: _namespace,
    } = prepared_ordered_fixture(client);
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut runner;
    let private = frontend.as_mut().expect("a live runner");
    let mut inbox = OrderedInbox::default();
    let watch = watch.as_ref().expect("a sealed watch");
    ingress
        .submit(button_to(
            surface,
            XAuthorityInputDeliveryId::from_raw(1903),
            272,
            true,
        ))
        .expect("the order to accept it");
        let press_cell = admitted_cell(private, 1903);
    let turn = private
        .route_pending_ordered(keyboards, watch)
        .expect("a readable order");
    private.deliver_turn(turn);
    let capsule = inbox
        .accepted(private, &channels.ordered, &press_cell, 8)
        .expect("a readable terminal step")
        .expect("the event reached its recipient's ordered queue");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(1903)
    );
    assert_eq!(capsule.client(), client);

    // A's writer reports its own delivery blocked and still owns it. Whether
    // any of the frame reached the wire depends on how full the socket already
    // was, so what is asserted is ownership and position, not partiality.
    let (outcome, in_flight, writer_a) = stalled.join().expect("A's writer returns");
    assert!(matches!(
        outcome,
        Err(X11OrderedWriteFailure::Send(X11FrameSendFailure::Blocked { .. }))
    ));
    let held = in_flight.as_ref().expect("A still owns its delivery");
    assert!(held.send.frame.is_some(), "with its frame still in hand");
    assert_eq!(held.frame_index(), 0, "and not advanced past");

    // Taking the socket down afterwards does not disturb what is owned. This
    // is shutdown of a writer that has already returned, not interruption of
    // one still waiting.
    writer_a
        .shutdown(std::net::Shutdown::Both)
        .expect("the socket can be ended");
    assert!(in_flight.as_ref().expect("still owned").send.frame.is_some());
    drop(reader_a);
    drop(registration);
    drop(channels);
    drop(durable);
}


#[test]
fn a_request_carries_the_capability_it_was_reserved_under() {
    let client = XServerFrontendClientId(2001);
    let surface = SurfaceId::new(2001, 1);
    let window = XResourceId::new(0x202001, 1);
    let (private, _registration, role, _keyboards) = ordered_fixture(client, surface, window);
    let stamp = private.control_gate().stamp().expect("an open coordinator");
    let reserved = role.reserve(stamp, 1).expect("a reservation");

    // The capability the reservation was issued with, and the one the custody
    // answers with, are the same. A native operation checks that the
    // capability and the permit name one source, and a capability read from
    // the producer at execution time would be whatever it holds then rather
    // than the one this request was accepted against -- which is the whole of
    // what makes that check mean anything.
    let expected = reserved.capability();
    let custody = reserved.accepted();
    assert_eq!(
        custody.capability().source(),
        expected.source(),
        "the custody answers with the capability the request was reserved under"
    );
    let _ = custody.observe();
}

/// One prepared runner with a recipient that has actually selected.
///
/// Built in the order the production path builds it: connection state
/// attached, the source's own window geometry and selections registered,
/// the runner prepared -- which installs the applied registry itself, so
/// nothing here reinstalls it -- and then a real initial focus clear applied
/// through the installed publication. A pointer press resolves against that
/// clear; it admits no key target and is not pretending to.
///
/// Every step unwraps. A setup that cannot reach the prepared state fails the
/// control rather than leaving its body to run against something else.
#[allow(dead_code)]
struct PreparedOrderedFixture {
    runner: PrivatePreparedRunner,
    /// Taken from the runner at construction so a control can borrow the
    /// frontend, the keyboards and the watch disjointly afterwards. The
    /// producer is owned, so holding it costs the runner no borrow.
    ingress: crate::PrivateIngress,
    durable: PrivateSettlementOwner,
    registration: XServerFrontendClientRouteRegistration,
    channels: XServerFrontendClientRouteChannels,
    _acks: Receiver<XAuthorityClientControlAck>,
    deliveries: Receiver<XAuthorityClientInputDelivery>,
    /// The source's own view of this connection's windows, so a control can
    /// change what a fresh resolution would reach.
    selections: Arc<Mutex<XCoreEventSelectionState>>,
    client: XServerFrontendClientId,
    surface: SurfaceId,
    window: XResourceId,
    namespace: NamespaceId,
}

fn prepared_ordered_fixture(client: XServerFrontendClientId) -> PreparedOrderedFixture {
    let namespace = NamespaceId::from_raw(client.raw());
    let surface = SurfaceId::new(u32::try_from(client.raw()).unwrap(), 1);
    let window = XResourceId::new(0x200000 | client.raw(), 1);
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
        .expect("a fresh client registers");
    // Admitted through the lifecycle rather than beside it: attaching the
    // lifecycle admits too, so a fixture that did both would be refused as
    // already admitted, and one that admits without it leaves anything
    // closing this connection with no gate to close.
    private
        .broker
        .registry
        .attach_private_lifecycle(&registration, admitted(client))
        .expect("the boundary admits and the lifecycle attaches");
    // The selections the resolver actually reads, and the focus projection,
    // both retained here rather than left to defaults.
    let selections = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    let focused = Arc::new(AtomicU64::new(0));
    private
        .broker
        .registry
        .attach_connection_state(&registration, namespace, selections.clone(), focused.clone())
        .expect("the connection state attaches");
    {
        // The source's own view of this window: where it sits, that it is
        // mapped, and that this recipient selected button press and release.
        // Updating the coarse subscription map instead would leave the
        // resolver reading a selection state nobody had told anything.
        let mut selected = selections.lock().expect("the selections");
        selected.register(
            window,
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
        );
        selected.observe_mapped(window);
        selected.update(window, Some((1 << 2) | (1 << 3)), None);
    }
    private
        .broker
        .registry
        .register_surface(client, namespace, surface, window)
        .expect("the surface registers");

    let mut runner = private
        .prepare_runner(namespace)
        .unwrap_or_else(|(cause, _)| panic!("runner refused: {cause:?}"));
    let ingress = runner
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("the runner exposes a producer");

    // A real initial clear through the publication the runner installed,
    // borrowed rather than installed again. Nothing here sets a published flag
    // or seeds the focus atomic as though that were the same thing.
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
        runtime.prepare_input_focus_namespace(namespace);
        publication
            .lock()
            .expect("the publication")
            .begin_focus_change()
            .expect("a focus change")
            .apply(&mut runtime, &focused, None)
            .expect("the clear applies");
    }

    PreparedOrderedFixture {
        runner,
        ingress,
        durable,
        registration,
        channels,
        _acks: acks,
        deliveries,
        selections,
        client,
        surface,
        window,
        namespace,
    }
}

#[test]
fn a_prepared_runner_presses_through_its_real_producer() {
    let client = XServerFrontendClientId(2101);
    let mut fixture = prepared_ordered_fixture(client);
    let ingress = fixture
        .runner
        .ingress_for(client, DeviceId::from_raw(1))
        .expect("the runner exposes a producer");
    ingress
        .submit(button_to(
            fixture.surface,
            XAuthorityInputDeliveryId::from_raw(2101),
            272,
            true,
        ))
        .expect("the order accepts it");

    // Driven through the runner's own turn, which is what production drives.
    let progress = fixture
        .runner
        .service_turn()
        .expect("a readable order");
    assert_eq!(progress.taken, 1, "the runner took the submitted work");
    assert_eq!(progress.refused, 0, "and did not refuse it");
    drop(fixture.registration);
    drop(fixture.channels);
    drop(fixture.durable);
}

// Disposable signed-source review controls. No production source changes.
// Each operation uses the prepared source fixture, its actual reservation,
// native guarded execution and completion observation. Terminal dispatch is
// invoked separately to expose the scheduling state without unrelated work.
fn attempt_run(f: &mut PreparedOrderedFixture, id: u64, button: u32, pressed: bool) {
    f.ingress.submit(button_to(f.surface, XAuthorityInputDeliveryId::from_raw(id), button, pressed)).unwrap();
    let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut f.runner;
    let p = frontend.as_mut().unwrap();
    assert!(matches!(p.step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap()).unwrap(), PrivateOrderedStep::Decided(_)));
    // Observe this decided request without invoking the terminal scheduler,
    // so the control can drive and inspect release dispatch separately.
    let Some(PrivateOrderedItem::Ran { run, custody, .. }) = p.terminal.turn.pop() else { panic!("real accepted input must run"); };
    if !pressed {
        assert!(matches!(run.release, Some(sophia_input_authority::ReleaseOutcome::DeliverTo(_))), "release outcome {:?}", run.release);
    }
    assert!(custody.observe().unwrap().is_some());
}

fn attempt_release(f: &mut PreparedOrderedFixture, id: u64, button: u32) {
    attempt_run(f, id, button, true);
    attempt_run(f, id + 1, button, false);
}


fn order_pass_frames(c: &XAuthorityOrderedDelivery) -> Vec<Vec<u8>> {
    let emission=c.emission();
    (0..emission.frame_count()).map(|index|emission.encode_frame(index,XByteOrder::LittleEndian,7).unwrap().as_bytes().to_vec()).collect()
}

#[test]
fn a_full_recipient_does_not_consume_a_live_recipients_turn() {
    let mut f=prepared_ordered_fixture(XServerFrontendClientId(7601));
    // Four ORIGINAL events fill A: each pair is actually reserved, executed,
    // observed at the common boundary, built and enqueued by production code.
    // Each release's actual sealed native proof is recorded once. No receipt
    // or settlement is invented, and the A receiver stays live and undrained.
    for (id,button) in [(76010,272),(76012,273)] {
        attempt_release(&mut f,id,button);
        let p=f.runner.frontend.as_mut().unwrap();
        assert!(p.terminal.settling.last().unwrap().native().unwrap().proof().is_some());
        assert_eq!(p.dispatch_one_press(),Some(true));
        assert_eq!(p.record_one_native(),Some(true));
        assert_eq!(p.attempt_one_delivery(),Some(true));
    }
    attempt_run(&mut f,76014,274,true);
    let (original,frames,order)={
        let p=f.runner.frontend.as_mut().unwrap();
        assert_eq!(p.broker.registry.per_client_input_capacity.get(),4);
        assert_eq!(p.terminal.holds.len(),1);
        assert_eq!(p.terminal.settling.len(),2);
        assert!(p.terminal.settling.iter().all(|r|r.native_recorded() && r.dispatch()==PrivateDispatchPhase::Enqueued && r.attempt().is_some()));
        let recovery=p.broker.registry.input_recovery.clone();
        let record=&mut p.terminal.holds[0];
        let original=record.custody.completion.as_ref().unwrap().clone();
        let emission=record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody,emission,&recovery,f.client);
        let Some(PrivatePendingDelivery::Capsule(capsule))=record.custody.pending.take() else{panic!("the actual fifth event built a capsule")};
        let frames=order_pass_frames(&capsule);
        let sender=p.broker.registry.clients.lock().unwrap().get(&f.client).unwrap().ordered.clone();
        // Classify the actual send refusal. Return the exact original capsule
        // to custody; this is neither synthetic filler nor a fake Full result.
        let capsule=match sender.try_send(capsule) {
            Err(std::sync::mpsc::TrySendError::Full(c))=>c,
            Err(std::sync::mpsc::TrySendError::Disconnected(_))=>panic!("A receiver remains live"),
            Ok(())=>panic!("four original capsules must fill A's four slots"),
        };
        assert!(Arc::ptr_eq(&original,&capsule.finalizer().unwrap().completion));
        record.custody.pending=Some(PrivatePendingDelivery::Capsule(capsule));
        assert_eq!(record.custody.dispatch,PrivateDispatchPhase::Pending);
        (original,frames,record.custody.order)
    };

    // B is a genuine second recipient in the same namespace, admitted with
    // its own selection/connection. Real A-ungrab/B-grab changes subsequent
    // source resolution; no foreign custody or fabricated native hold is used.
    let other=XServerFrontendClientId(7602);
    let other_window=XResourceId::new(0x307602,1);
    let (other_registration,other_channels)={
        let p=f.runner.frontend.as_mut().unwrap();let registry=&p.broker.registry;
        let context=namespaced(other,f.namespace);
        let (registration,channels)=registry.register_client_with_admission(other,Some(context)).unwrap();
        registry.attach_private_lifecycle(&registration,context).unwrap();
        let selected=Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry.attach_connection_state(&registration,f.namespace,selected.clone(),Arc::new(AtomicU64::new(0))).unwrap();
        {
            let mut s=selected.lock().unwrap();
            s.register(other_window,XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT),1),Rect{x:0,y:0,width:200,height:100});
            s.observe_mapped(other_window);s.update(other_window,Some((1<<2)|(1<<3)),None);
        }
        let mut grabs=registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace,f.client.raw());
        grabs.grab_pointer(f.namespace,crate::XActiveInputGrab{owner:other.raw(),window:other_window,owner_events:false,pointer_mode:1,keyboard_mode:1,event_mask:u16::MAX,xi_event_mask:[0;8],xi_event_mask_words:0,route_lease:None}).unwrap();
        (registration,channels)
    };
    attempt_run(&mut f,76015,275,true);
    let p=f.runner.frontend.as_mut().unwrap();
    assert_eq!(p.terminal.holds.len(),2);
    assert_eq!(p.terminal.holds[0].reached.client(),f.client);
    assert_eq!(p.terminal.holds[1].reached.client(),other);
    let original_native=p.terminal.holds[0].native.as_ref().unwrap() as *const _ as usize;
    let incarnation=p.terminal.holds[0].incarnation;
    assert_eq!(p.terminal.holds[0].custody.order,order);
    assert!(order<p.terminal.holds[1].custody.order);
    let later=p.terminal.holds[1].custody.completion.as_ref().unwrap().clone();
    let recovery=p.broker.registry.input_recovery.clone();
    let queue_cells=[76010,76011,76012,76013].map(|id|recovery.completion_for(XAuthorityInputDeliveryId::from_raw(id)).unwrap().unwrap());
    let mut seen_b=Vec::new();let mut refused=0;let mut idle=0;
    for _ in 0..32 {
        match p.deliver_one(&mut |_,_|Ok(())).unwrap() {
            PrivateDeliveryStep::Dispatched{enqueued:false,..}=>refused+=1,
            PrivateDeliveryStep::Idle=>idle+=1,
            _=>{},
        }
        // Only B is drainable during measurement. A's original four remain
        // untouched until after every actual terminal visit has completed.
        while let Ok(c)=other_channels.ordered.try_recv() {
            assert_eq!(c.delivery(),XAuthorityInputDeliveryId::from_raw(76015));
            assert_eq!(c.client(),other);
            assert!(Arc::ptr_eq(&later,&c.finalizer().unwrap().completion));
            seen_b.push(c.delivery());
        }
    }
    let old=&p.terminal.holds[0];
    assert_eq!(old.incarnation,incarnation);
    assert_eq!(old.native.as_ref().unwrap() as *const _ as usize,original_native);
    assert_eq!(old.custody.dispatch,PrivateDispatchPhase::Pending);
    assert_eq!(old.custody.order,order);
    let Some(PrivatePendingDelivery::Capsule(c))=old.custody.pending.as_ref() else{panic!("the original blocked capsule remains inventory-owned")};
    assert_eq!(c.delivery(),XAuthorityInputDeliveryId::from_raw(76014));
    assert_eq!(c.client(),f.client);
    assert!(Arc::ptr_eq(&original,&c.finalizer().unwrap().completion));
    assert_eq!(order_pass_frames(c),frames);
    assert!(original.answer().is_none() && later.answer().is_none());
    assert!(queue_cells.iter().all(|c|c.answer().is_none()));
    assert!(p.terminal.settling.iter().all(|r|r.dispatch()==PrivateDispatchPhase::Enqueued && r.attempt().is_some()));
    let mut seen_a=Vec::new();
    while let Ok(c)=f.channels.ordered.try_recv() {
        let index=seen_a.len();assert!(index<queue_cells.len());
        assert_eq!(c.client(),f.client);
        assert!(Arc::ptr_eq(&queue_cells[index],&c.finalizer().unwrap().completion));
        seen_a.push(c.delivery());
    }
    assert_eq!(seen_a,[76010,76011,76012,76013].map(XAuthorityInputDeliveryId::from_raw));
    assert!(
        refused + idle > 0,
        "the visits were real: {refused} refused dispatches, {idle} idle"
    );
    assert_eq!(seen_b,[XAuthorityInputDeliveryId::from_raw(76015)],"a genuinely Full older recipient must not consume every later live recipient's turn");

    // AND THE RETAINED CAPSULE GOES ONCE, NOW THAT THERE IS ROOM. Draining A's
    // four freed its slots; what was refused is offered again unchanged. The
    // capsule is the one that was built before the refusal -- same completion,
    // same recipient, same bytes -- because a refused queue is a handover that
    // did not happen, not one to redo.
    let mut inbox = OrderedInbox::default();
    let retried = inbox
        .accepted(p, &f.channels.ordered, &original, 8)
        .expect("a readable terminal step")
        .expect("the retained capsule is accepted once its recipient has room");
    assert_eq!(
        retried.delivery(),
        XAuthorityInputDeliveryId::from_raw(76014)
    );
    assert_eq!(retried.client(), f.client);
    assert!(Arc::ptr_eq(
        &original,
        &retried.finalizer().unwrap().completion
    ));
    assert_eq!(
        order_pass_frames(&retried),
        frames,
        "the same encoded event, not one built again"
    );
    assert_eq!(
        retried.emission().incarnation(),
        incarnation,
        "still named by the hold it came from"
    );
    assert!(
        inbox.taken.is_empty(),
        "and nothing else of A's was taken while finding it"
    );

    // Once. Further visits produce no duplicate for either recipient.
    for _ in 0..8 {
        let _ = p.deliver_one(&mut |_, _| Ok(())).unwrap();
    }
    assert!(
        f.channels.ordered.try_recv().is_err(),
        "an accepted handover is not repeated after capacity opens"
    );
    assert!(other_channels.ordered.try_recv().is_err());
    assert!(original.answer().is_none() && later.answer().is_none());
    drop(other_registration);
}

#[test]
fn an_unrecorded_release_at_the_head_spends_its_connections_turn() {
    // A release whose native half is not in yet cannot be given a ledger
    // attempt, so its handover is unfinished and it is its connection's head.
    // The press path must recognise that and leave it alone: the release
    // belongs to the attempt path, and reaching into it from here would act on
    // a debt the ledger has not authorised.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(7611));
    attempt_release(&mut f, 76110, 272);
    // The press it ends comes first and is handed over, which is what leaves
    // the release alone at the head of that connection.
    {
        let p = f.runner.frontend.as_mut().unwrap();
        assert_eq!(p.dispatch_one_press(), Some(true));
    }
    assert_eq!(
        f.channels.ordered.try_iter().count(),
        1,
        "the carried press, and only it"
    );

    // A second connection with its own press, behind the release in stamp
    // order, so a release head that stopped everything would show here.
    let other = XServerFrontendClientId(7612);
    let other_window = XResourceId::new(0x307612, 1);
    let (other_registration, other_channels) = {
        let p = f.runner.frontend.as_mut().unwrap();
        let registry = &p.broker.registry;
        let context = namespaced(other, f.namespace);
        let (registration, channels) = registry
            .register_client_with_admission(other, Some(context))
            .unwrap();
        registry.attach_private_lifecycle(&registration, context).unwrap();
        let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry
            .attach_connection_state(
                &registration,
                f.namespace,
                selected.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap();
        {
            let mut state = selected.lock().unwrap();
            state.register(
                other_window,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                Rect { x: 0, y: 0, width: 200, height: 100 },
            );
            state.observe_mapped(other_window);
            state.update(other_window, Some((1 << 2) | (1 << 3)), None);
        }
        let mut grabs = registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace, f.client.raw());
        grabs
            .grab_pointer(
                f.namespace,
                crate::XActiveInputGrab {
                    owner: other.raw(),
                    window: other_window,
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
        (registration, channels)
    };
    attempt_run(&mut f, 76112, 273, true);

    let p = f.runner.frontend.as_mut().unwrap();
    // The state this control exists for: one settling release, native not yet
    // recorded, so the ledger will refuse it an attempt and its own handover
    // has not been taken.
    assert_eq!(p.terminal.settling.len(), 1);
    assert!(!p.terminal.settling[0].native_recorded());
    assert_eq!(
        p.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Untaken,
        "the release owes a handover, so it is its connection's head"
    );
    assert!(p.terminal.settling[0].attempt().is_none());
    assert_eq!(p.terminal.holds.len(), 1);
    assert_eq!(p.terminal.holds[0].reached.client(), other);

    let recovery = p.broker.registry.input_recovery.clone();
    let press_cell = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(76110))
        .unwrap()
        .unwrap();
    let release_cell = recovery
        .completion_for(XAuthorityInputDeliveryId::from_raw(76111))
        .unwrap()
        .unwrap();
    let other_cell = p.terminal.holds[0].custody.completion.as_ref().unwrap().clone();

    // The other connection's own press goes first: the turn moved on from A
    // when A was last offered, which is what keeps a stuck connection from
    // consuming every visit.
    assert_eq!(p.dispatch_one_press(), Some(true));
    let handed: Vec<_> = other_channels.ordered.try_iter().collect();
    assert_eq!(handed.len(), 1, "the connection behind it is not blocked by it");

    // Now only the release is left unfinished, so it is what the next visit is
    // offered. The press path must recognise it and leave it alone: reaching
    // into it would act on a debt the ledger never authorised.
    assert_eq!(
        p.dispatch_one_press(),
        None,
        "a release head is left to the attempt path, and costs the visit"
    );
    assert_eq!(
        p.terminal.settling[0].dispatch(),
        PrivateDispatchPhase::Untaken,
        "nothing was taken from it"
    );
    assert_eq!(
        handed[0].delivery(),
        XAuthorityInputDeliveryId::from_raw(76112)
    );
    assert_eq!(handed[0].client(), other);
    assert!(Arc::ptr_eq(
        &other_cell,
        &handed[0].finalizer().expect("carried").completion
    ));
    assert!(f.channels.ordered.try_recv().is_err(), "and nothing of A's moved");
    assert!(
        press_cell.answer().is_none()
            && release_cell.answer().is_none()
            && other_cell.answer().is_none()
    );
    drop(other_registration);
}

#[test]
fn a_recipients_complete_event_order_is_preserved_across_press_and_release() {
    let mut f=prepared_ordered_fixture(XServerFrontendClientId(7551));
    attempt_release(&mut f,75510,272);
    attempt_run(&mut f,75512,273,true);
    let p=f.runner.frontend.as_mut().unwrap();
    let recovery=p.broker.registry.input_recovery.clone();
    let cells=[75510,75511,75512].map(|id|recovery.completion_for(XAuthorityInputDeliveryId::from_raw(id)).unwrap().unwrap());
    let mut seen=Vec::new();
    // Drive the actual terminal arbiter, including native recording/release
    // dispatch. Drain ALL queue entries after every visit, not merely presses.
    for _ in 0..16 {
        let _=p.deliver_one(&mut |_,_|Ok(())).unwrap();
        while let Ok(capsule)=f.channels.ordered.try_recv() {
            let id=capsule.delivery();
            let expected=[75510,75511,75512].iter().position(|n|id==XAuthorityInputDeliveryId::from_raw(*n)).expect("only these actual events exist");
            assert!(Arc::ptr_eq(&cells[expected],&capsule.finalizer().unwrap().completion));
            seen.push(id);
        }
    }
    assert!(cells.iter().all(|c|c.answer().is_none()));
    assert_eq!(seen,[75510,75511,75512].map(XAuthorityInputDeliveryId::from_raw),"sorting only presses cannot preserve the recipient's complete event order");
}

#[test]
fn an_indeterminate_head_keeps_its_custody_while_another_recipient_progresses() {
    let mut f=prepared_ordered_fixture(XServerFrontendClientId(7561));
    attempt_run(&mut f,75610,272,true);attempt_run(&mut f,75611,273,true);
    let other=XServerFrontendClientId(7562);let other_window=XResourceId::new(0x307562,1);
    let (other_registration,other_channels)={
        let p=f.runner.frontend.as_mut().unwrap();let registry=&p.broker.registry;
        let context=namespaced(other,f.namespace);
        let (registration,channels)=registry.register_client_with_admission(other,Some(context)).unwrap();
        registry.attach_private_lifecycle(&registration,context).unwrap();
        let selected=Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry.attach_connection_state(&registration,f.namespace,selected.clone(),Arc::new(AtomicU64::new(0))).unwrap();
        {
            let mut s=selected.lock().unwrap();
            s.register(other_window,XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT),1),Rect{x:0,y:0,width:200,height:100});
            s.observe_mapped(other_window);s.update(other_window,Some((1<<2)|(1<<3)),None);
        }
        // Genuine owner operations change subsequent source resolution. Native
        // A holds remain retained; no foreign inventory or fake recipient is
        // inserted into this executor. We make no cleanup/lifecycle claim.
        let mut grabs=registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace,f.client.raw());
        grabs.grab_pointer(f.namespace,crate::XActiveInputGrab{owner:other.raw(),window:other_window,owner_events:false,pointer_mode:1,keyboard_mode:1,event_mask:u16::MAX,xi_event_mask:[0;8],xi_event_mask_words:0,route_lease:None}).unwrap();
        (registration,channels)
    };
    attempt_run(&mut f,75612,274,true);
    let p=f.runner.frontend.as_mut().unwrap();assert_eq!(p.terminal.holds.len(),3);
    assert_eq!(p.terminal.holds[0].reached.client(),f.client);assert_eq!(p.terminal.holds[1].reached.client(),f.client);assert_eq!(p.terminal.holds[2].reached.client(),other);
    let recovery=p.broker.registry.input_recovery.clone();
    let cells=[75610,75611,75612].map(|id|recovery.completion_for(XAuthorityInputDeliveryId::from_raw(id)).unwrap().unwrap());
    let record=&mut p.terminal.holds[0];let emission=record.native.as_mut().unwrap().take_press_emission().unwrap();
    PrivateXServerFrontend::stow_press_capsule(&mut record.custody,emission,&recovery,f.client);
    // Explicitly staged at the state between the dispatch phase write and
    // capsule take. No actual interrupted send or already-sent byte is claimed.
    record.custody.dispatch=PrivateDispatchPhase::Indeterminate;
    let mut seen_a=Vec::new();let mut seen_b=Vec::new();
    for _ in 0..8 {
        let _=p.deliver_one(&mut |_,_|Ok(())).unwrap();
        while let Ok(c)=f.channels.ordered.try_recv(){
            let index=if c.delivery()==XAuthorityInputDeliveryId::from_raw(75610){0}else{assert_eq!(c.delivery(),XAuthorityInputDeliveryId::from_raw(75611));1};
            assert!(Arc::ptr_eq(&cells[index],&c.finalizer().unwrap().completion));seen_a.push(c.delivery());
        }
        while let Ok(c)=other_channels.ordered.try_recv(){
            assert_eq!(c.delivery(),XAuthorityInputDeliveryId::from_raw(75612));assert_eq!(c.client(),other);
            assert!(Arc::ptr_eq(&cells[2],&c.finalizer().unwrap().completion));seen_b.push(c.delivery());
        }
    }
    let old_phase=p.terminal.holds[0].custody.dispatch;
    let old_owned=matches!(p.terminal.holds[0].custody.pending.as_ref(),Some(PrivatePendingDelivery::Capsule(c)) if c.delivery()==XAuthorityInputDeliveryId::from_raw(75610) && Arc::ptr_eq(&cells[0],&c.finalizer().unwrap().completion));
    assert!(cells.iter().all(|c|c.answer().is_none()));
    assert_eq!(seen_b,[XAuthorityInputDeliveryId::from_raw(75612)],"a distinct exact recipient must progress under retained arbitration");
    assert!(seen_a.is_empty(),"neither the indeterminate head nor any later event may enqueue for its recipient");
    assert_eq!(old_phase,PrivateDispatchPhase::Indeterminate);assert!(old_owned,"the exact unknown-handover capsule stays inventory-owned");
    drop(other_registration);
}

fn output_refused(f:&mut PreparedOrderedFixture,route:XAuthorityRoutedInput)->PrivateExecutionRefusal {
    f.ingress.submit(route).unwrap();
    let PrivatePreparedRunner{frontend,keyboards,watch,..}=&mut f.runner;let p=frontend.as_mut().unwrap();
    assert!(matches!(p.step_once(keyboards,&mut |_,_|Ok(()),watch.as_ref().unwrap()).unwrap(),PrivateOrderedStep::Decided(_)));
    let Some(PrivateOrderedItem::Refused{refusal,custody,..})=p.terminal.turn.pop() else{panic!("typed pre-effect refusal required")};
    assert!(custody.observe().unwrap().is_some());refusal
}

#[test]
fn an_exhausted_event_order_refuses_before_any_effect() {
    for release in [false,true] {
        let mut f=prepared_ordered_fixture(XServerFrontendClientId(if release{7572}else{7571}));
        if release{attempt_run(&mut f,75720,272,true);}
        let p=f.runner.frontend.as_mut().unwrap();
        let held=if release{Some((p.terminal.holds[0].incarnation,p.terminal.holds[0].native.as_ref().unwrap() as *const _ as usize,p.terminal.holds[0].custody.completion.as_ref().unwrap().clone()))}else{None};
        // Counter exhaustion only is staged. The reservation, completion cell,
        // native source call boundary and typed refusal are real.
        p.terminal.next_event_order=u64::MAX;
        let id=XAuthorityInputDeliveryId::from_raw(if release{75721}else{75710});
        let route=button_to(f.surface,id,272,!release);
        assert!(matches!(output_refused(&mut f,route),PrivateExecutionRefusal::OrderExhausted));
        let p=f.runner.frontend.as_ref().unwrap();
        assert_eq!(p.terminal.next_event_order,u64::MAX);
        assert!(p.terminal.pending_custody.is_none() && p.terminal.native_pending.is_none() && p.terminal.settling.is_empty());
        assert_eq!(p.terminal.holds.len(),usize::from(release));
        let mask=p.broker.registry.pointer_state.lock().unwrap().get(&(f.namespace,SeatId::from_raw(1))).expect("existing mapper").state();
        assert_eq!(mask,if release{256}else{0});
        if let Some((incarnation,native,cell))=held {
            assert_eq!(p.terminal.holds[0].incarnation,incarnation);assert_eq!(p.terminal.holds[0].native.as_ref().unwrap() as *const _ as usize,native);
            assert!(Arc::ptr_eq(&cell,p.terminal.holds[0].custody.completion.as_ref().unwrap()));assert_eq!(Arc::strong_count(&cell),3);assert!(cell.answer().is_none());
        }
        let recovery=&p.broker.registry.input_recovery;let cell=recovery.completion_for(id).unwrap().unwrap();
        assert!(cell.answer().is_none());assert_eq!(Arc::strong_count(&cell),2);
        let (claimed,applied)={let s=recovery.state.lock().unwrap();let e=s.tickets.get(&id).unwrap();(e.claimed,e.may_have_applied)};
        assert!(!claimed && !applied);assert!(f.channels.ordered.try_recv().is_err());
    }
}

#[test]
fn an_instrument_recognises_its_admission_after_the_ticket_is_pruned() {
    // The helper used to ask the recovery for the cell when it was called, so
    // an admission whose ticket had been answered and pruned read as one that
    // was never handed over. The identity has to be fixed where the admission
    // mints it, and a lookup that returns nothing is not evidence.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(7621));
    attempt_run(&mut f, 76210, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let id = XAuthorityInputDeliveryId::from_raw(76210);
    let original = admitted_cell(private, 76210);
    let mut inbox = OrderedInbox::default();

    let capsule = inbox
        .accepted(private, &f.channels.ordered, &original, 8)
        .expect("a readable terminal step")
        .expect("the actual recipient accepted the original capsule");
    assert_eq!(capsule.delivery(), id);
    assert_eq!(capsule.client(), f.client);
    assert!(Arc::ptr_eq(
        &original,
        &capsule.finalizer().unwrap().completion
    ));
    assert_eq!(
        handover_phase(private, &original),
        Some(PrivateDispatchPhase::Enqueued)
    );

    // A real finish and an ordinary observation prune the ticket. Nothing
    // about the handover changes: the capsule and the custody still carry the
    // cell this admission minted.
    recovery
        .finish(f.client, Some(id), XAuthorityInputDeliveryOutcome::WriteFailed)
        .unwrap();
    let answer = original
        .answer()
        .expect("the original admission owns the established answer");
    assert_eq!(answer.delivery, id);
    assert_eq!(answer.outcome, XAuthorityInputDeliveryOutcome::WriteFailed);
    assert!(
        recovery.observe(answer),
        "an ordinary observer consumes and prunes the exact answered ticket"
    );
    assert!(
        recovery.completion_for(id).unwrap().is_none(),
        "the lookup this instrument must not depend on is gone"
    );

    assert_eq!(
        handover_phase(private, &original),
        Some(PrivateDispatchPhase::Enqueued)
    );
    assert!(Arc::ptr_eq(
        &original,
        private.terminal.holds[0].custody.completion.as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        &original,
        &capsule.finalizer().unwrap().completion
    ));

    // And the retained capsule is still recognised as this admission's, with
    // nothing replayed to find it again.
    inbox.taken.push(capsule);
    let found = inbox
        .accepted(private, &f.channels.ordered, &original, 8)
        .expect("a readable terminal step")
        .expect("an instrument holding the admission's own cell still knows it");
    assert_eq!(found.delivery(), id);
    assert!(
        f.channels.ordered.try_recv().is_err(),
        "the actual accepted event is never replayed"
    );
}

#[test]
fn an_instrument_takes_the_admission_asked_for_and_keeps_the_others() {
    // Three events owed to one recipient, asked for out of order. An
    // instrument that returned whatever capsule it found first would answer
    // every one of these with the press, and a control built on it would be
    // asserting about an event it never named.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(7631));
    let cells: Vec<_> = {
        let mut cells = Vec::new();
        for (id, button, pressed) in [(76310u64, 272u32, true), (76311, 272, false), (76312, 273, true)] {
            f.ingress
                .submit(button_to(
                    f.surface,
                    XAuthorityInputDeliveryId::from_raw(id),
                    button,
                    pressed,
                ))
                .unwrap();
            let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut f.runner;
            let private = frontend.as_mut().unwrap();
            assert!(matches!(
                private
                    .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
                    .unwrap(),
                PrivateOrderedStep::Decided(_)
            ));
            cells.push(admitted_cell(private, id));
            let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.pop() else {
                panic!("an accepted request runs")
            };
            assert!(custody.observe().unwrap().is_some());
        }
        cells
    };
    let private = f.runner.frontend.as_mut().unwrap();

    // THE BUDGET STOPS AT THE ANSWER. Nothing is queued yet, so visits have to
    // be driven -- but only until this admission's capsule appears. Spending
    // the rest would hand over events this question never mentioned, which a
    // control asking about the first one has no business causing.
    let mut inbox = OrderedInbox::default();
    assert!(
        f.channels.ordered.try_recv().is_err(),
        "nothing has been handed over before this"
    );
    let first = inbox
        .accepted(private, &f.channels.ordered, &cells[0], 8)
        .expect("a readable terminal step")
        .expect("the press is handed over and recognised");
    assert_eq!(
        first.delivery(),
        XAuthorityInputDeliveryId::from_raw(76310)
    );
    assert_ne!(
        handover_phase(private, &cells[2]),
        Some(PrivateDispatchPhase::Enqueued),
        "and the events nobody asked about are still owed"
    );
    assert!(
        inbox.taken.is_empty(),
        "nothing else was taken off that queue"
    );

    // Now let the rest go, so the whole stream is in hand.
    for _ in 0..12 {
        let _ = private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }
    inbox.collect(&f.channels.ordered);
    assert_eq!(
        inbox
            .taken
            .iter()
            .map(XAuthorityOrderedDelivery::delivery)
            .collect::<Vec<_>>(),
        [76311, 76312].map(XAuthorityInputDeliveryId::from_raw),
        "in their own order, with the one already taken out"
    );

    // ASKED FOR THE LAST ONE, out of order and with no budget at all.
    let third = inbox
        .accepted(private, &f.channels.ordered, &cells[2], 0)
        .expect("a readable terminal step")
        .expect("what is already queued counts without driving anything");
    assert_eq!(
        third.delivery(),
        XAuthorityInputDeliveryId::from_raw(76312),
        "the admission asked for, not the first capsule to hand"
    );
    assert!(Arc::ptr_eq(&cells[2], &third.finalizer().unwrap().completion));

    // The one it was not asked about is kept.
    assert_eq!(
        inbox
            .taken
            .iter()
            .map(XAuthorityOrderedDelivery::delivery)
            .collect::<Vec<_>>(),
        [76311].map(XAuthorityInputDeliveryId::from_raw),
        "unrelated capsules are retained, not consumed by someone else's question"
    );
    assert!(cells.iter().all(|cell| cell.answer().is_none()));
}

#[test]
fn a_capsule_for_another_connection_is_refused_before_any_byte_of_it_is_written() {
    // STAGED, AND SAID SO. Nothing in the routing path puts one connection's
    // capsule on another's queue today: dispatch looks the queue up by the
    // recipient the capsule names. This is the check that has to exist before
    // a per-connection loop is attached to that queue, because by the time a
    // frame for the wrong connection is on a wire the recipient has read it.
    //
    // Both capsules are real: two genuinely admitted connections, each
    // pressing through the source and building its own emission.
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(7641));
    attempt_run(&mut f, 76410, 272, true);

    let other = XServerFrontendClientId(7642);
    let other_window = XResourceId::new(0x307642, 1);
    let (other_registration, _other_channels) = {
        let p = f.runner.frontend.as_mut().unwrap();
        let registry = &p.broker.registry;
        let context = namespaced(other, f.namespace);
        let (registration, channels) = registry
            .register_client_with_admission(other, Some(context))
            .unwrap();
        registry.attach_private_lifecycle(&registration, context).unwrap();
        let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry
            .attach_connection_state(
                &registration,
                f.namespace,
                selected.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap();
        {
            let mut state = selected.lock().unwrap();
            state.register(
                other_window,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                Rect { x: 0, y: 0, width: 200, height: 100 },
            );
            state.observe_mapped(other_window);
            state.update(other_window, Some((1 << 2) | (1 << 3)), None);
        }
        let mut grabs = registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace, f.client.raw());
        grabs
            .grab_pointer(
                f.namespace,
                crate::XActiveInputGrab {
                    owner: other.raw(),
                    window: other_window,
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
        (registration, channels)
    };
    attempt_run(&mut f, 76412, 273, true);

    // Each connection's own capsule, taken out of the record that owns it.
    let p = f.runner.frontend.as_mut().unwrap();
    assert_eq!(p.terminal.holds.len(), 2);
    assert_eq!(p.terminal.holds[0].reached.client(), f.client);
    assert_eq!(p.terminal.holds[1].reached.client(), other);
    let recovery = p.broker.registry.input_recovery.clone();
    let mine = {
        let record = &mut p.terminal.holds[0];
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, f.client);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("the press built its own capsule")
        };
        capsule
    };
    let theirs = {
        let record = &mut p.terminal.holds[1];
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, other);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("the other press built its own capsule")
        };
        capsule
    };
    assert_ne!(
        mine.recipient(),
        theirs.recipient(),
        "two connections, two identities"
    );
    let their_cell = theirs.finalizer().expect("carried").completion.clone();
    let my_delivery = mine.delivery();

    // A writer serving the first connection, and the other's capsule on its
    // queue ahead of its own.
    // The writer's expectation comes from the registration it serves, not from
    // anything it is about to be asked to write.
    let served = XAuthorityServedConnection::retained(
        p.endpoint_for(&f.registration)
            .expect("this connection's own endpoint"),
    );
    let (sender, queue) = sync_channel(4);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let mut in_flight = None;
    let mut refused = None;
    sender.send(theirs).expect("the queue to accept it");
    sender.send(mine).expect("the queue to accept it");

    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::AdmissionRefused(_)
    ));

    // NOTHING WENT ON THE WIRE. That is the whole point of checking at
    // admission: a frame is read by the time anyone could regret it.
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "no byte of another connection's event reached this one"
    );
    assert!(in_flight.is_none(), "and it was never taken as work");

    // It is retained rather than dropped -- the queue has already given it up
    // -- and it is not answered here.
    let held = refused.as_ref().expect("the capsule is owned by this writer");
    assert_eq!(
        held.cause(),
        Some(X11OrderedAdmissionRefusal::ForeignEndpoint),
        "and it says exactly what was wrong: not a flush, not a failure to write"
    );
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(76412)
    );
    assert_eq!(held.client(), other);
    assert!(Arc::ptr_eq(
        &their_cell,
        &held.delivery().finalizer().expect("carried").completion
    ));
    assert!(
        their_cell.answer().is_none(),
        "a writer that was never entitled to it does not answer for it"
    );

    // And nothing behind it is served while it is held: taking another would
    // overwrite the one thing that still owns this capsule.
    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::AdmissionRefused(_)
    ));
    assert_eq!(
        refused
            .as_ref()
            .expect("still held")
            .delivery()
            .delivery(),
        XAuthorityInputDeliveryId::from_raw(76412),
        "the first is still the one held"
    );
    assert!(in_flight.is_none());

    // Disposed of, and this connection's own capsule is admitted normally.
    let _disposed = refused.take().expect("held");
    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::Advanced | X11OrderedServeStep::Flushed
    ));
    assert!(
        refused.is_none(),
        "its own capsule is not refused"
    );
    assert!(
        in_flight
            .as_ref()
            .is_none_or(|held| held.delivery().delivery() == my_delivery),
        "the connection's own event is what it serves"
    );
    drop(peer);
    drop(other_registration);
}

/// An admission for this client with a chosen admission id and generation.
fn admission_with(
    client: XServerFrontendClientId,
    admission: u64,
    generation: u64,
) -> sophia_protocol::ClientAdmissionContext {
    sophia_protocol::ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(admission),
        sophia_protocol::NamespaceContext::new(
            NamespaceId::from_raw(client.raw()),
            sophia_protocol::NamespaceProfile::Confined,
            sophia_protocol::NamespaceCapabilities::NONE,
        )
        .unwrap(),
        sophia_protocol::ClientAuthProvenance::new(
            sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
            generation,
        )
        .unwrap(),
    )
    .unwrap()
}

/// Replace this client's registration and admission, and say what the
/// replacement's exact endpoint is.
///
/// THE REGISTRATION/API SEAM, NOT AN ORDINARY RECONNECT. Nothing a client can
/// drive replaces a live registration: register_client_with_admission refuses a
/// duplicate, and admit refuses an already-bound client. This goes through the
/// registry's and the participant's own replacement path -- revoke, drop the
/// row, admit and register again -- because that is the seam an endpoint
/// identity has to survive. It is not evidence that a reconnect misdelivers.
fn replace_registration(
    private: &mut crate::PrivateXServerFrontend,
    client: XServerFrontendClientId,
    replacement: sophia_protocol::ClientAdmissionContext,
    previous: sophia_protocol::ClientAdmissionId,
    original: &XServerFrontendClientRouteRegistration,
    surface: Option<(SurfaceId, XResourceId)>,
) -> (
    XServerFrontendClientRouteRegistration,
    XServerFrontendClientRouteChannels,
    PrivateEndpointIdentity,
) {
    private
        .participant
        .revoke_admission(client, previous)
        .expect("the admission this fixture made is the one it revokes");
    // The first registration's row goes before the second exists. Its
    // channels are the caller's to dispose of, because a caller may still be
    // holding one on purpose. The registration guard itself is kept by the
    // caller too, deliberately: a holder of a stale capability is exactly who
    // must be refused rather than handed the replacement's identity.
    let _ = original;
    private
        .broker
        .registry
        .clients
        .lock()
        .expect("a readable registry")
        .remove(&client);
    // The lifecycle owner still holds a record for the closed admission. It is
    // released by the owner's own drive, not by dropping anything, so the
    // replacement is admitted only after the first one has actually finished.
    let lifecycle = private.terminal.lifecycle.clone();
    for _ in 0..16 {
        lifecycle
            .drive(NonZeroUsize::new(1).unwrap())
            .expect("a readable lifecycle owner");
    }
    private
        .admission_participant()
        .admit(client, replacement)
        .expect("the boundary admits the replacement");
    let (registration, channels) = private
        .broker
        .registry
        .register_client_with_admission(client, Some(replacement))
        .expect("a replacement registration");
    // No attach_private_lifecycle here: with the owner already installed, the
    // admit above registered the replacement's gate itself, and attaching a
    // second one for the same client is refused as a duplicate.
    let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
    if let Some((surface, window)) = surface {
        // The original registration's surface route went with it, so the
        // replacement registers its own.
        private
            .broker
            .registry
            .register_surface(client, replacement.namespace.id, surface, window)
            .expect("the replacement's surface");
        // The replacement selects the same window, so its own presses resolve.
        let mut state = selected.lock().expect("a readable selection state");
        state.register(
            window,
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            Rect { x: 0, y: 0, width: 200, height: 100 },
        );
        state.observe_mapped(window);
        state.update(window, Some((1 << 2) | (1 << 3)), None);
    }
    private
        .broker
        .registry
        .attach_connection_state(
            &registration,
            replacement.namespace.id,
            selected,
            Arc::new(AtomicU64::new(0)),
        )
        .expect("the replacement's connection state");
    let endpoint = private
        .endpoint_for(&registration)
        .expect("the replacement's own endpoint, from its own registration");
    (registration, channels, endpoint)
}

/// One real source-built capsule, and a replacement registration for the same
/// client that did not exist when it was built.
fn capsule_then_replacement(
    client: XServerFrontendClientId,
    delivery: u64,
    replacement: sophia_protocol::ClientAdmissionContext,
) -> (
    PrivatePreparedRunner,
    PrivateSettlementOwner,
    XAuthorityOrderedDelivery,
    Arc<PrivateDeliveryCompletion>,
    XServerFrontendClientRouteRegistration,
    XServerFrontendClientRouteChannels,
    PrivateEndpointIdentity,
) {
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, delivery, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let cell = admitted_cell(private, delivery);
    let original = {
        let record = &mut private.terminal.holds[0];
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, client);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("the press built its own capsule")
        };
        capsule
    };
    assert!(Arc::ptr_eq(
        &cell,
        &original.finalizer().expect("carried").completion
    ));
    let PreparedOrderedFixture {
        mut runner,
        durable,
        registration: original_registration,
        channels: _original_channels,
        ..
    } = f;
    let private = runner.frontend.as_mut().unwrap();
    let (registration, channels, endpoint) = replace_registration(
        private,
        client,
        replacement,
        admitted(client).client_id,
        &original_registration,
        None,
    );
    (
        runner,
        durable,
        original,
        cell,
        registration,
        channels,
        endpoint,
    )
}

#[test]
fn a_capsule_from_a_replaced_admission_is_refused_though_every_number_agrees() {
    // Same client, same session generation, a different admission. The tuple
    // the ledger knows this connection by is identical on both sides, which is
    // exactly why it cannot be what admission is decided on: the boundary
    // itself treats a replacement admission inside one session as a different
    // admission, and a delayed revoke naming the old one must not close the
    // new one.
    let client = XServerFrontendClientId(7651);
    let replacement = admission_with(client, 76519, ROLE_SESSION_GENERATION);
    let (runner, durable, original, cell, registration, channels, endpoint) =
        capsule_then_replacement(client, 76510, replacement);
    assert_eq!(
        original.recipient(),
        sophia_input_authority::ConnectionIdentity {
            recipient: client.raw(),
            connection_generation: ROLE_SESSION_GENERATION,
        },
        "the capsule's ledger identity"
    );

    // WHAT THE TEARDOWN ALREADY DID is recorded before the writer is asked,
    // so what follows is attributed to the refusal and not to the revocation.
    // Replacing an admission ends its outstanding deliveries -- that is the
    // recovery answering for a connection that has gone -- and this control is
    // about the writer, which must add nothing to it either way.
    let answered_by_teardown = cell.answer();

    // The replacement writer's expectation comes from its own registration.
    let served = XAuthorityServedConnection::retained(endpoint);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let (sender, queue) = sync_channel(4);
    let mut in_flight = None;
    let mut refused = None;
    sender.send(original).expect("the queue to accept it");

    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::AdmissionRefused(X11OrderedAdmissionRefusal::ForeignEndpoint)
    ));
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "not one byte of a replaced admission's event reached the replacement"
    );
    let held = refused.as_ref().expect("owned by this writer");
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(76510)
    );
    assert!(Arc::ptr_eq(
        &cell,
        &held.delivery().finalizer().expect("carried").completion
    ));
    assert_eq!(
        cell.answer(),
        answered_by_teardown,
        "and the writer that never wrote for it changed nothing about its answer"
    );
    assert!(in_flight.is_none());
    drop(peer);
    drop(channels);
    drop(registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_capsule_from_a_replaced_registration_is_refused_though_the_admission_agrees_too() {
    // The sharpest case: the replacement is admitted with THE SAME admission
    // context -- same client, same admission id, same namespace, same session
    // generation. Every number on both sides is equal. What differs is the
    // registration, and that is the whole of what distinguishes them.
    let client = XServerFrontendClientId(7661);
    let (runner, durable, original, cell, registration, channels, endpoint) =
        capsule_then_replacement(client, 76610, admitted(client));
    assert_eq!(
        original.recipient(),
        sophia_input_authority::ConnectionIdentity {
            recipient: client.raw(),
            connection_generation: ROLE_SESSION_GENERATION,
        }
    );

    let answered_by_teardown = cell.answer();
    let served = XAuthorityServedConnection::retained(endpoint);
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let (sender, queue) = sync_channel(4);
    let mut in_flight = None;
    let mut refused = None;
    sender.send(original).expect("the queue to accept it");

    assert!(matches!(
        serve_one_ordered_delivery(
            &socket,
            &served,
            &mut in_flight,
            &mut refused,
            &queue,
            XByteOrder::LittleEndian,
            7,
        ),
        X11OrderedServeStep::AdmissionRefused(X11OrderedAdmissionRefusal::ForeignEndpoint)
    ));
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "identical numbers are not an entitlement to these bytes"
    );
    assert!(
        refused.is_some() && in_flight.is_none(),
        "it is owned as refused work, not taken as work to do"
    );
    assert_eq!(
        cell.answer(),
        answered_by_teardown,
        "and the refusal is not an answer"
    );
    drop(peer);
    drop(channels);
    drop(registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_producer_does_not_send_an_old_capsule_through_a_replacement_entry() {
    // THE OTHER HALF OF THE SEAM. The writer's check catches a capsule that
    // reached the wrong queue; this catches one being put there. The row the
    // endpoint is compared against is the row the sender is cloned from, under
    // one guard, so there is no gap between deciding a row is right and taking
    // its channel.
    //
    // Registration/API seam, as above: nothing a client drives replaces a live
    // registration.
    let client = XServerFrontendClientId(7681);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 76810, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 76810);

    // The press has an event owed and a capsule built for it, still owned by
    // the custody that made it.
    let recovery = private.broker.registry.input_recovery.clone();
    {
        let record = &mut private.terminal.holds[0];
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, client);
    }
    assert_eq!(
        private.terminal.holds[0].custody.dispatch,
        PrivateDispatchPhase::Pending
    );

    let PreparedOrderedFixture {
        mut runner,
        durable,
        registration: original_registration,
        channels: _original_channels,
        ..
    } = f;
    let private = runner.frontend.as_mut().unwrap();
    let (registration, channels, _endpoint) = replace_registration(
        private,
        client,
        admitted(client),
        admitted(client).client_id,
        &original_registration,
        None,
    );
    let answered_by_teardown = cell.answer();

    // The replacement's row holds a different channel. Offering the old
    // capsule must not put it there.
    for _ in 0..8 {
        let _ = private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }
    assert!(
        channels.ordered.try_recv().is_err(),
        "a replacement registration is not handed the work of the one it replaced"
    );

    // And the capsule stays exactly where it was, unsent and unanswered by
    // this refusal.
    let custody = &private.terminal.holds[0].custody;
    assert_eq!(
        custody.dispatch,
        PrivateDispatchPhase::Pending,
        "no handover was begun for it"
    );
    let Some(PrivatePendingDelivery::Capsule(held)) = custody.pending.as_ref() else {
        panic!("the original capsule is still owned by the custody that built it")
    };
    assert_eq!(
        held.delivery(),
        XAuthorityInputDeliveryId::from_raw(76810)
    );
    assert!(Arc::ptr_eq(
        &cell,
        &held.finalizer().expect("carried").completion
    ));
    assert_eq!(
        cell.answer(),
        answered_by_teardown,
        "and offering it to a row that is not its own answers nothing"
    );
    drop(channels);
    drop(registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_producer_does_not_send_an_old_release_through_a_replacement_entry() {
    // The press control could not see this: the release reaches its queue by
    // the ledger-selected attempt path, which acquires its own sender. Both
    // paths have to check the row they send through.
    //
    // Registration/API seam, as elsewhere in this file.
    let client = XServerFrontendClientId(7691);
    let mut f = prepared_ordered_fixture(client);
    attempt_release(&mut f, 76910, 272);
    let private = f.runner.frontend.as_mut().unwrap();

    // The press goes first and normally, so what is left owed is the release.
    assert_eq!(private.dispatch_one_press(), Some(true));
    assert_eq!(
        f.channels.ordered.try_iter().count(),
        1,
        "the press this release ends"
    );
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.record_one_native(), Some(true));
    let release_cell = admitted_cell(private, 76911);
    assert_eq!(private.terminal.settling.len(), 1);
    assert!(private.terminal.settling[0].native_recorded());

    let PreparedOrderedFixture {
        mut runner,
        durable,
        registration: original_registration,
        channels: _original_channels,
        ..
    } = f;
    let private = runner.frontend.as_mut().unwrap();
    let (registration, channels, _endpoint) = replace_registration(
        private,
        client,
        admitted(client),
        admitted(client).client_id,
        &original_registration,
        None,
    );
    let answered_by_teardown = release_cell.answer();

    // The ledger may hand out an attempt; the row it would be served through
    // is not this release's, so nothing is written down and nothing is taken.
    for _ in 0..8 {
        let _ = private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }
    assert!(
        channels.ordered.try_recv().is_err(),
        "a replacement registration is not handed the release of the one it replaced"
    );
    let release = &private.terminal.settling[0];
    assert!(
        matches!(
            release.dispatch(),
            PrivateDispatchPhase::Untaken | PrivateDispatchPhase::Pending
        ),
        "no handover was begun for it: building its capsule may move Untaken to \
         Pending, but nothing past that, got {:?}",
        release.dispatch()
    );
    assert!(
        matches!(
            release.custody.pending.as_ref(),
            Some(PrivatePendingDelivery::Capsule(_))
        ),
        "and its own capsule is still there, unsent"
    );
    assert!(
        release.attempt().is_none(),
        "any attempt claimed for it was given back rather than held against an \
         unusable row"
    );
    assert_eq!(
        release_cell.answer(),
        answered_by_teardown,
        "and offering it to a row that is not its own answers nothing"
    );
    drop(channels);
    drop(registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_serving_owner_keeps_its_own_endpoint_when_its_registration_is_replaced() {
    // The comparison controls prove two deliberately different identities
    // compare unequal. This proves what matters for attachment: an owner built
    // for one registration, holding that registration's receiver and socket,
    // goes on expecting THAT registration after a replacement exists -- and
    // refuses bytes for anything else through its own serving call rather than
    // through a free function a caller could hand three unrelated things.
    //
    // Registration/API seam, as elsewhere in this file.
    let client = XServerFrontendClientId(7701);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77010, 272, true);

    // A genuinely different connection, admitted and grabbed the ordinary way,
    // with its own source-built press. Nothing about this capsule is
    // fabricated: it is what its own endpoint is owed.
    let other = XServerFrontendClientId(7702);
    let other_window = XResourceId::new(0x307702, 1);
    let (other_registration, _other_channels) = {
        let p = f.runner.frontend.as_mut().unwrap();
        let registry = &p.broker.registry;
        let context = namespaced(other, f.namespace);
        let (registration, channels) = registry
            .register_client_with_admission(other, Some(context))
            .unwrap();
        registry.attach_private_lifecycle(&registration, context).unwrap();
        let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry
            .attach_connection_state(
                &registration,
                f.namespace,
                selected.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap();
        {
            let mut state = selected.lock().unwrap();
            state.register(
                other_window,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                Rect { x: 0, y: 0, width: 200, height: 100 },
            );
            state.observe_mapped(other_window);
            state.update(other_window, Some((1 << 2) | (1 << 3)), None);
        }
        let mut grabs = registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace, f.client.raw());
        grabs
            .grab_pointer(
                f.namespace,
                crate::XActiveInputGrab {
                    owner: other.raw(),
                    window: other_window,
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
        (registration, channels)
    };
    attempt_run(&mut f, 77012, 273, true);

    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let foreign_cell = admitted_cell(private, 77012);
    let foreign = {
        let record = private
            .terminal
            .holds
            .iter_mut()
            .find(|record| record.reached.client() == other)
            .expect("the other connection's own press");
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, other);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("it built its own capsule")
        };
        capsule
    };

    // Kept before the replacement removes the row that holds it, so this
    // owner's queue can still be handed something afterwards. That is the
    // point: an incorrectly supplied capsule has to be caught at the serving
    // boundary, not only prevented at the producer.
    let original_sender = private
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .expect("the original row")
        .ordered
        .clone();

    let PreparedOrderedFixture {
        mut runner,
        durable,
        registration: original_registration,
        channels: original_channels,
        ..
    } = f;
    let private = runner.frontend.as_mut().unwrap();

    // The owner is built from the registration and receiver that exist now,
    // and owns them from here on.
    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let transport =
        XAuthorityOrderedTransport::bind(&original_registration, original_channels.ordered, &output, &wire, &pending, None)
            .unwrap_or_else(|(refusal, _)| {
                panic!("this connection's own receiver and output bind: {refusal:?}")
            });
    let mut owner =
        X11OrderedServingOwner::for_registration(private, &original_registration, transport)
            .unwrap_or_else(|(refusal, _)| {
                panic!("an owner for the registration that made this receiver: {refusal:?}")
            });

    // Its own connection's event is served normally, once.
    for _ in 0..8 {
        let _ = private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }
    let mut flushed = 0;
    for _ in 0..16 {
        match owner.serve_one(XByteOrder::LittleEndian, 7) {
            X11OrderedServeStep::Advanced => {}
            X11OrderedServeStep::Flushed => flushed += 1,
            X11OrderedServeStep::Idle => break,
            other => panic!("its own endpoint's event is served: {other:?}"),
        }
    }
    assert_eq!(flushed, 1, "the endpoint it was built for writes once");
    assert!(owner.refused().is_none() && owner.in_flight().is_none());
    // Its own event's bytes are taken off the wire, so what is read later can
    // only be something written after this point.
    let mut drained = [0u8; 4096];
    assert!(
        (&peer).read(&mut drained).is_ok_and(|read| read > 0),
        "its own event really did reach the wire"
    );
    while (&peer).read(&mut drained).is_ok_and(|read| read > 0) {}

    // Now the registration is replaced. The owner is not rebuilt and is not
    // told: it still holds what it was given.
    let (replacement_registration, replacement_channels, replacement_endpoint) =
        replace_registration(
            private,
            client,
            admitted(client),
            admitted(client).client_id,
            &original_registration,
            None,
        );

    // IT CANNOT ADOPT THE REPLACEMENT'S IDENTITY. The replacement can name its
    // own endpoint, and it is not the one this owner serves.
    assert!(
        !owner.served.endpoint().matches(&replacement_endpoint),
        "the owner serves the registration it was built for, not the current one"
    );
    assert!(
        private.endpoint_for(&replacement_registration).is_ok(),
        "and the replacement can name its own"
    );
    // AND THE STALE CAPABILITY IS REFUSED, not described. Asking with the
    // registration this owner was built for must not hand back whatever
    // registration now holds that client number -- which is the only way an
    // owner could come to expect the endpoint that replaced it.
    assert!(
        private.endpoint_for(&original_registration).is_err(),
        "a registration that is no longer the current row names no endpoint"
    );

    // AND IT CANNOT BE HANDED ANOTHER ENDPOINT'S BYTES. A real capsule owed to
    // a different connection, put on this owner's own queue, is refused
    // through the owner's own serving call before any of it is written.
    original_sender
        .send(foreign)
        .expect("this owner's queue accepts it");
    assert!(matches!(
        owner.serve_one(XByteOrder::LittleEndian, 7),
        X11OrderedServeStep::AdmissionRefused(X11OrderedAdmissionRefusal::ForeignEndpoint)
    ));
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "not one byte of another endpoint's event reached the socket this owner holds"
    );
    let held = owner.refused().expect("owned by this owner");
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(77012)
    );
    assert_eq!(held.cause(), Some(X11OrderedAdmissionRefusal::ForeignEndpoint));
    assert!(Arc::ptr_eq(
        &foreign_cell,
        &held.delivery().finalizer().expect("carried").completion
    ));
    assert!(owner.in_flight().is_none());
    assert!(
        foreign_cell.answer().is_none(),
        "and its admission is not answered by an owner that was never entitled to it"
    );

    // A second arrival while one is held is reported as itself and does not
    // overwrite the one thing that still owns the first.
    assert!(matches!(
        owner.serve_one(XByteOrder::LittleEndian, 7),
        X11OrderedServeStep::AdmissionRefused(X11OrderedAdmissionRefusal::AlreadyHolding)
    ));
    assert_eq!(
        owner
            .refused()
            .expect("still held")
            .delivery()
            .delivery(),
        XAuthorityInputDeliveryId::from_raw(77012)
    );
    drop(peer);
    drop(original_registration);
    drop(other_registration);
    drop(replacement_channels);
    drop(replacement_registration);
    drop(runner);
    drop(durable);
}

#[test]
fn a_serving_constructor_rejects_another_registrations_receiver() {
    // Two real prepared registrations and their actual registry-created
    // receivers. Independent origins make the mismatch unambiguous; no
    // capsule, outcome, socket traffic or production-loop scenario is staged.
    let a=prepared_ordered_fixture(XServerFrontendClientId(7711));
    let b=prepared_ordered_fixture(XServerFrontendClientId(7712));
    let private=a.runner.frontend.as_ref().unwrap();
    let endpoint_a=private.endpoint_for(&a.registration).unwrap();
    let endpoint_b=b.runner.frontend.as_ref().unwrap().endpoint_for(&b.registration).unwrap();
    assert!(!endpoint_a.matches(&endpoint_b));
    let (socket,_peer)=UnixStream::pair().unwrap();
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    // The association is established where the transport is bound, so that is
    // where crossing two real connections is caught.
    let bound = XAuthorityOrderedTransport::bind(&a.registration, b.channels.ordered, &output, &wire, &pending, None);
    assert!(
        bound.is_err(),
        "binding must reject B's original receiver when given registration A"
    );
    let (refusal, returned) = bound.err().expect("refused");
    assert_eq!(refusal, X11OrderedServingRefusal::ForeignReceiver);
    assert!(
        returned.minted_by(&b.registration),
        "and the receiver handed back is B's own, whole"
    );
    // And the same refusal stands at the serving owner, for a transport that
    // was bound for a different registration.
    let transport = XAuthorityOrderedTransport::bind(&b.registration, returned, &output, &wire, &pending, None)
        .unwrap_or_else(|_| panic!("B's own registration and B's own receiver bind"));
    let owner = X11OrderedServingOwner::for_registration(private, &a.registration, transport);
    assert!(
        owner.is_err(),
        "a transport bound for another registration prepares no writer here"
    );
}

#[test]
fn a_failed_serving_constructor_preserves_its_original_queued_capsule() {
    let client=XServerFrontendClientId(7721);
    let mut f=prepared_ordered_fixture(client);
    // Real admitted native press and actual original ordered queue handover.
    attempt_run(&mut f,77210,272,true);
    let private=f.runner.frontend.as_mut().unwrap();
    let original=admitted_cell(private,77210);
    assert_eq!(private.dispatch_one_press(),Some(true));
    let sender=private.broker.registry.clients.lock().unwrap().get(&client).unwrap().ordered.clone();
    let capsule=f.channels.ordered.try_recv().expect("original queue accepted the actual press");
    assert_eq!(capsule.delivery(),XAuthorityInputDeliveryId::from_raw(77210));
    assert_eq!(capsule.client(),client);
    assert!(Arc::ptr_eq(&original,&capsule.finalizer().unwrap().completion));
    assert!(capsule.endpoint().matches(&private.endpoint_for(&f.registration).unwrap()));
    assert_eq!(Arc::strong_count(capsule.finalizer().unwrap()),1,"only this nonclone capsule owns its finalizer Arc");
    // Keep ONLY a Weak finalizer witness; a strong clone here would mask
    // destruction of the original queued capsule.
    let finalizer=Arc::downgrade(capsule.finalizer().unwrap());
    assert!(sender.try_send(capsule).is_ok(),"put the exact original capsule back into its original queue");
    assert!(finalizer.upgrade().is_some());
    assert_eq!(private.terminal.holds[0].custody.dispatch,PrivateDispatchPhase::Enqueued);
    assert!(private.terminal.holds[0].custody.pending.is_none());

    // Real participant revocation makes endpoint acquisition refuse while the
    // original registration, receiver, sender and queued capsule remain held.
    // No registry/authority field, outcome or phase is forced by this control.
    private.participant.revoke_admission(client,admitted(client).client_id).unwrap();
    assert!(private.endpoint_for(&f.registration).is_err());
    let answered_before=original.answer();
    assert!(Arc::ptr_eq(&original,private.terminal.holds[0].custody.completion.as_ref().unwrap()));
    assert_eq!(private.terminal.holds[0].custody.dispatch,PrivateDispatchPhase::Enqueued);
    assert!(private.terminal.holds[0].native.is_some());
    let (socket,_peer)=UnixStream::pair().unwrap();
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    assert!(finalizer.upgrade().is_some(),"revocation did not destroy the actual queued capsule immediately before construction");
    let transport = XAuthorityOrderedTransport::bind(&f.registration, f.channels.ordered, &output, &wire, &pending, None)
        .unwrap_or_else(|_| panic!("this connection's own receiver and registration bind"));
    let returned=X11OrderedServingOwner::for_registration(private,&f.registration,transport);
    assert!(returned.is_err(),"the established endpoint refusal is returned");
    let payload_still_owned=finalizer.upgrade().is_some();
    assert_eq!(original.answer(),answered_before,"constructor failure does not change the completion answer");
    assert!(Arc::ptr_eq(&original,private.terminal.holds[0].custody.completion.as_ref().unwrap()));
    assert_eq!(private.terminal.holds[0].custody.dispatch,PrivateDispatchPhase::Enqueued);
    assert!(private.terminal.holds[0].native.is_some());
    assert!(payload_still_owned,"a refused constructor must return or durably retain its accepted receiver/capsule resources, not drop them through ?");
    drop(sender);
}

/// Drive a close to quiescence in bounded visits, reporting what each did.
fn close_to_quiet(
    owner: &mut X11OrderedServingOwner,
    cause: X11OrderedCloseCause,
) -> Vec<X11OrderedCloseStep> {
    owner
        .begin_close(cause)
        .unwrap_or_else(|kind| panic!("a socket pair ends: {kind:?}"));
    let mut steps = Vec::new();
    for _ in 0..32 {
        let step = owner.advance_close(XByteOrder::LittleEndian, 7);
        steps.push(step);
        if matches!(
            step,
            X11OrderedCloseStep::Quiet | X11OrderedCloseStep::Drained
        ) {
            break;
        }
    }
    steps
}

/// A serving owner for this fixture's own connection, with its output.
fn serving_owner_for(
    f: &mut PreparedOrderedFixture,
    socket: UnixStream,
) -> (X11OrderedServingOwner, Arc<Mutex<UnixStream>>) {
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let ordered = std::mem::replace(
        &mut f.channels.ordered,
        f.runner
            .frontend
            .as_ref()
            .unwrap()
            .broker
            .registry
            .register_client_with_admission(
                XServerFrontendClientId(f.client.raw() + 500_000),
                Some(admitted(XServerFrontendClientId(f.client.raw() + 500_000))),
            )
            .expect("a spare registration to borrow a receiver from")
            .1
            .ordered,
    );
    let transport = XAuthorityOrderedTransport::bind(&f.registration, ordered, &output, &wire, &pending, None)
        .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));
    (owner, output)
}

#[test]
fn a_close_adjudicates_the_queued_admission_before_letting_its_payload_go() {
    // (a) The disposition is an answer this close established through the
    // capsule's own finalizer, not a client-wide sweep and not a drop whose
    // consequences someone else is assumed to clean up.
    let client = XServerFrontendClientId(7731);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77310, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 77310);
    for _ in 0..8 {
        let _ = private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }
    assert!(cell.answer().is_none(), "nothing has answered it yet");

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert!(
        matches!(
            steps.first(),
            Some(X11OrderedCloseStep::Adjudicated(
                PrivateAdjudication::Answered
            ))
        ),
        "the queued admission was offered an outcome, not discarded: {steps:?}"
    );
    let closing = owner.closing().expect("a close in progress");
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (1, 0, 0)
    );
    assert!(owner.retained_unanswered().is_empty() && owner.retained_foreign().is_empty());
    assert_eq!(
        cell.answer().map(|answer| answer.outcome),
        Some(XAuthorityInputDeliveryOutcome::ClientDisconnected),
        "and the answer is on the admission's own completion"
    );
    // The socket really ended. Read with a deadline: a connection that was
    // never ended would leave this waiting for a peer that is still there,
    // and a control that hangs says nothing.
    peer.set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a deadline on the peer");
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).ok(),
        Some(0),
        "the peer sees the connection ended"
    );
}

#[test]
fn a_close_ends_the_socket_without_the_output_lock_it_may_be_stalled_under() {
    // (e) Ending a connection must not need the mutex a stalled write holds --
    // that is exactly when ending it is what is needed. The lock is held here
    // for the whole close.
    let client = XServerFrontendClientId(7741);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77410, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 77410);
    for _ in 0..8 {
        let _ = private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, output) = serving_owner_for(&mut f, socket);
    let held = output.lock().expect("the connection's own output");
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    let closing = owner.closing().expect("a close in progress");
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (1, 0, 0),
        "and what it held was still offered an outcome: {steps:?}"
    );
    assert!(owner.retained_unanswered().is_empty());
    assert!(cell.answer().is_some());
    peer.set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a deadline on the peer");
    let mut byte = [0u8; 1];
    assert_eq!(
        (&peer).read(&mut byte).ok(),
        Some(0),
        "the peer sees it ended while the output lock was never released"
    );
    drop(held);
}

#[test]
fn a_close_under_a_held_claim_transfers_a_deferral_rather_than_an_answer() {
    // (c) A deferral is the authority taking reporting responsibility under a
    // claim it holds. Counting it as an answer would tell a caller the
    // admission was settled when what actually happened is that someone else
    // now owes the report.
    let client = XServerFrontendClientId(7761);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77610, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 77610);
    let recovery = private.broker.registry.input_recovery.clone();
    for _ in 0..8 {
        let _ = private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }

    // A real execution claim on this exact delivery, taken the ordinary way.
    let delivery = XAuthorityInputDeliveryId::from_raw(77610);
    assert_eq!(
        recovery.claim_execution(Some(delivery)),
        ExecutionClaim::Claimed,
        "the claim this control needs is the ledger's own"
    );

    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    let closing = owner.closing().expect("a close in progress");
    assert!(
        owner.retained_unanswered().is_empty(),
        "the offer was taken, in one form or another: {steps:?}"
    );
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (0, 0, 1),
        "and it was taken as a deferral, counted as itself"
    );
    assert!(
        cell.answer().is_none(),
        "a deferral is not a published answer: the claim still owes the report"
    );
}

#[test]
fn a_close_does_not_answer_a_capsule_belonging_to_another_endpoint() {
    // (d) This socket ending says nothing about another endpoint's recipient,
    // so a refused capsule is carried out still held rather than answered.
    let client = XServerFrontendClientId(7751);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77510, 272, true);

    let other = XServerFrontendClientId(7752);
    let other_window = XResourceId::new(0x307752, 1);
    let (other_registration, _other_channels) = {
        let p = f.runner.frontend.as_mut().unwrap();
        let registry = &p.broker.registry;
        let context = namespaced(other, f.namespace);
        let (registration, channels) = registry
            .register_client_with_admission(other, Some(context))
            .unwrap();
        registry.attach_private_lifecycle(&registration, context).unwrap();
        let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry
            .attach_connection_state(
                &registration,
                f.namespace,
                selected.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap();
        {
            let mut state = selected.lock().unwrap();
            state.register(
                other_window,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                Rect { x: 0, y: 0, width: 200, height: 100 },
            );
            state.observe_mapped(other_window);
            state.update(other_window, Some((1 << 2) | (1 << 3)), None);
        }
        let mut grabs = registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace, f.client.raw());
        grabs
            .grab_pointer(
                f.namespace,
                crate::XActiveInputGrab {
                    owner: other.raw(),
                    window: other_window,
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
        (registration, channels)
    };
    attempt_run(&mut f, 77512, 273, true);

    let private = f.runner.frontend.as_mut().unwrap();
    let recovery = private.broker.registry.input_recovery.clone();
    let foreign_cell = admitted_cell(private, 77512);
    let foreign = {
        let record = private
            .terminal
            .holds
            .iter_mut()
            .find(|record| record.reached.client() == other)
            .expect("the other connection's own press");
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, other);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("it built its own capsule")
        };
        capsule
    };
    let sender = private
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .expect("this connection's row")
        .ordered
        .clone();

    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    // STILL QUEUED, NEVER SERVED. The close path has to ask the same admission
    // question the serving path does: a capsule nobody classified is not this
    // connection's to answer for just because it is on its queue.
    sender.send(foreign).expect("onto this owner's queue");

    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert!(
        steps.contains(&X11OrderedCloseStep::Foreign),
        "the close classified it rather than offering for it: {steps:?}"
    );
    let closing = owner.closing().expect("a close in progress");
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (0, 0, 0),
        "nothing of another endpoint's was answered by this close"
    );
    let [held] = owner.retained_foreign() else {
        panic!("the foreign capsule is retained by the close")
    };
    assert_eq!(
        held.delivery().delivery(),
        XAuthorityInputDeliveryId::from_raw(77512)
    );
    assert_eq!(
        held.cause(),
        Some(X11OrderedAdmissionRefusal::ForeignEndpoint),
        "and still says why it was never this connection's to write"
    );
    assert!(
        foreign_cell.answer().is_none(),
        "closing this socket is not evidence about another endpoint's recipient"
    );
    drop(other_registration);
}

#[test]
fn a_close_retains_the_exact_capsule_when_the_authority_cannot_answer() {
    // The ordinary Refused branch: the authority takes nothing, so the close
    // consumes nothing. The capsule stays whole -- same completion, same
    // finalizer, same encoded bytes -- and its cell stays unanswered.
    let client = XServerFrontendClientId(7781);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 77810, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 77810);
    let recovery = private.broker.registry.input_recovery.clone();
    let sender = private
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .unwrap()
        .ordered
        .clone();
    for _ in 0..8 {
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }
    assert!(cell.answer().is_none());

    let (socket, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    let capsule = owner.queue.try_recv().expect("the actual queued source press");
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(77810)
    );
    assert!(Arc::ptr_eq(&cell, &capsule.finalizer().unwrap().completion));
    assert_eq!(Arc::strong_count(capsule.finalizer().unwrap()), 1);
    let original_finalizer = Arc::downgrade(capsule.finalizer().unwrap());
    let original_frames: Vec<Vec<u8>> = (0..capsule.emission().frame_count())
        .map(|index| {
            capsule
                .emission()
                .encode_frame(index, XByteOrder::LittleEndian, 7)
                .unwrap()
                .as_bytes()
                .to_vec()
        })
        .collect();
    assert!(!original_frames.is_empty());
    sender.try_send(capsule).unwrap();

    // The real mutex, poisoned without touching any ledger entry or outcome.
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = recovery.state.lock().unwrap();
            panic!("intentional recovery-lock poison for returned-refusal fixture");
        }))
        .is_err()
    );
    assert!(recovery.state.is_poisoned());

    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert!(
        steps.contains(&X11OrderedCloseStep::Adjudicated(
            PrivateAdjudication::Refused
        )),
        "the authority took nothing: {steps:?}"
    );
    let closing = owner.closing().expect("a close in progress");
    assert_eq!(
        (closing.answered, closing.already, closing.deferred),
        (0, 0, 0)
    );
    assert!(owner.retained_foreign().is_empty());
    let [held] = owner.retained_unanswered() else {
        panic!("the exact capsule is retained")
    };
    // The whole send state is kept, not the capsule alone: how far its bytes
    // got is part of what is still unknown about it.
    assert_eq!(held.frame_index(), 0);
    assert_eq!(held.blocked(), Duration::ZERO);
    let retained = held.delivery();
    assert_eq!(
        retained.delivery(),
        XAuthorityInputDeliveryId::from_raw(77810)
    );
    assert!(Arc::ptr_eq(&cell, &retained.finalizer().unwrap().completion));
    assert!(Arc::ptr_eq(
        &original_finalizer.upgrade().unwrap(),
        retained.finalizer().unwrap()
    ));
    let retained_frames: Vec<Vec<u8>> = (0..retained.emission().frame_count())
        .map(|index| {
            retained
                .emission()
                .encode_frame(index, XByteOrder::LittleEndian, 7)
                .unwrap()
                .as_bytes()
                .to_vec()
        })
        .collect();
    assert_eq!(retained_frames, original_frames);
    assert!(cell.answer().is_none());
    let mut byte = [0u8; 1];
    assert_eq!((&peer).read(&mut byte).unwrap(), 0);
}

#[test]
fn a_close_offers_the_end_of_a_connection_whatever_caused_it() {
    // A caller used to name the terminal outcome, so a close could record a
    // flush for bytes that were never written through a real finalizer. It
    // passes a cause now, and the cause is diagnostic: whichever one it is,
    // what the recipient is told is that the connection ended.
    for cause in [
        X11OrderedCloseCause::ConnectionEnded,
        X11OrderedCloseCause::PreparationFailed,
        X11OrderedCloseCause::SupervisorStopped,
    ] {
        let client = XServerFrontendClientId(7791);
        let mut f = prepared_ordered_fixture(client);
        attempt_run(&mut f, 77910, 272, true);
        let private = f.runner.frontend.as_mut().unwrap();
        let cell = admitted_cell(private, 77910);
        for _ in 0..8 {
            private.deliver_one(&mut |_, _| Ok(())).unwrap();
        }
        let (socket, _peer) = UnixStream::pair().unwrap();
        let (mut owner, _output) = serving_owner_for(&mut f, socket);
        let _ = close_to_quiet(&mut owner, cause);
        assert_eq!(
            cell.answer().map(|answer| answer.outcome),
            Some(XAuthorityInputDeliveryOutcome::ClientDisconnected),
            "no cause names a flush, a timeout or a write failure: {cause:?}"
        );
        assert_eq!(
            owner.closing().expect("closing").cause,
            cause,
            "the cause is kept for whoever reads it, and kept out of the answer"
        );
    }
}

#[test]
fn a_quiet_close_keeps_its_receiver_until_the_producers_are_actually_gone() {
    // An empty queue is not a stopped producer. While a sender for it is held
    // anywhere, a later capsule can still arrive, so a close that treated
    // Empty as the end would drop the receiver and lose whatever came next.
    let client = XServerFrontendClientId(7801);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78010, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78010);
    let sender = private
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .get(&client)
        .expect("this connection's row")
        .ordered
        .clone();
    for _ in 0..8 {
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert_eq!(
        steps.last(),
        Some(&X11OrderedCloseStep::Quiet),
        "a held sender means quiet, not finished: {steps:?}"
    );
    assert!(cell.answer().is_some(), "and what was queued was answered");

    // The receiver was kept, so something arriving afterwards is still this
    // close's to account for rather than something it threw away.
    let late = {
        let private = f.runner.frontend.as_mut().unwrap();
        let recovery = private.broker.registry.input_recovery.clone();
        attempt_run(&mut f, 78012, 273, true);
        let private = f.runner.frontend.as_mut().unwrap();
        let record = private
            .terminal
            .holds
            .iter_mut()
            .find(|record| {
                record
                    .custody
                    .completion
                    .as_ref()
                    .is_some_and(|held| held.answer().is_none())
            })
            .expect("the later press");
        let emission = record.native.as_mut().unwrap().take_press_emission().unwrap();
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, client);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("it built its own capsule")
        };
        capsule
    };
    let late_cell = late.finalizer().expect("carried").completion.clone();
    sender.send(late).expect("the retained receiver still has a queue");
    assert!(matches!(
        owner.advance_close(XByteOrder::LittleEndian, 7),
        X11OrderedCloseStep::Adjudicated(_)
    ));
    assert!(
        late_cell.answer().is_some(),
        "a capsule that arrived after the close began is still accounted for"
    );

    // Only the producers actually going reports the end.
    drop(sender);
    f.runner
        .frontend
        .as_ref()
        .unwrap()
        .broker
        .registry
        .clients
        .lock()
        .unwrap()
        .remove(&client);
    assert_eq!(
        owner.advance_close(XByteOrder::LittleEndian, 7),
        X11OrderedCloseStep::Drained,
        "with every sender gone, nothing further can arrive"
    );
}

#[test]
fn an_unusable_output_is_not_reported_as_an_empty_queue() {
    // Idle says there was nothing to do. An output this connection cannot take
    // says the opposite: something is owed and cannot be done. A caller told
    // the first would stop asking.
    let client = XServerFrontendClientId(7811);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78110, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78110);
    for _ in 0..8 {
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, output) = serving_owner_for(&mut f, socket);

    // The real output mutex, poisoned without touching the socket or anything
    // this connection owes.
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = output.lock().unwrap();
            panic!("intentional output-lock poison for transport fixture");
        }))
        .is_err()
    );
    assert!(output.is_poisoned());

    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::TransportUnavailable
        ),
        "an unusable transport is its own answer"
    );
    assert!(
        owner.in_flight().is_none() && owner.refused().is_none(),
        "and nothing was received, written or disposed of"
    );
    assert!(
        cell.answer().is_none(),
        "a transport that could not be taken answers for nobody"
    );
}

#[test]
fn a_started_close_cannot_be_served_normally_again() {
    // Serving after a close has begun consumed the queued event and wrote at a
    // socket the close had already shut down, publishing WriteFailed through
    // the real finalizer -- a failure manufactured by a forbidden write,
    // recorded ahead of the ending the close was establishing. Two owners of
    // one admission is the whole problem.
    let client = XServerFrontendClientId(7821);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78210, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78210);
    for _ in 0..8 {
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    owner
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .expect("a socket pair ends");

    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Closing
        ),
        "ordinary serving is over from the moment a close begins"
    );
    assert!(
        owner.in_flight().is_none(),
        "and it took nothing off the queue"
    );
    assert!(
        cell.answer().is_none(),
        "nothing was published by a write that must not have happened"
    );

    // The close itself is what answers it, and with the outcome a close knows.
    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert_eq!(
        cell.answer().map(|answer| answer.outcome),
        Some(XAuthorityInputDeliveryOutcome::ClientDisconnected),
        "{steps:?}"
    );
}

#[test]
fn a_close_reports_backpressure_rather_than_retaining_past_its_bound() {
    // Retention has to come from what is being retained. Growing the store
    // while holding custody is not reservation: it is finding out at the worst
    // moment that there was no room.
    let client = XServerFrontendClientId(7831);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78310, 272, true);

    let other = XServerFrontendClientId(7832);
    let other_window = XResourceId::new(0x307832, 1);
    let (other_registration, _other_channels) = {
        let p = f.runner.frontend.as_mut().unwrap();
        let registry = &p.broker.registry;
        let context = namespaced(other, f.namespace);
        let (registration, channels) = registry
            .register_client_with_admission(other, Some(context))
            .unwrap();
        registry.attach_private_lifecycle(&registration, context).unwrap();
        let selected = Arc::new(Mutex::new(XCoreEventSelectionState::default()));
        registry
            .attach_connection_state(
                &registration,
                f.namespace,
                selected.clone(),
                Arc::new(AtomicU64::new(0)),
            )
            .unwrap();
        {
            let mut state = selected.lock().unwrap();
            state.register(
                other_window,
                XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                Rect { x: 0, y: 0, width: 200, height: 100 },
            );
            state.observe_mapped(other_window);
            state.update(other_window, Some((1 << 2) | (1 << 3)), None);
        }
        let mut grabs = registry.input_authority.lock().unwrap();
        grabs.ungrab_pointer(f.namespace, f.client.raw());
        grabs
            .grab_pointer(
                f.namespace,
                crate::XActiveInputGrab {
                    owner: other.raw(),
                    window: other_window,
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
        (registration, channels)
    };

    // Real foreign capsules, built one at a time by the source, deliberately
    // put on this connection's queue. Same staged wrong-queue boundary as the
    // other foreign controls; not a claim that the producer misroutes.
    let sender = {
        let private = f.runner.frontend.as_ref().unwrap();
        private
            .broker
            .registry
            .clients
            .lock()
            .unwrap()
            .get(&client)
            .expect("this connection's row")
            .ordered
            .clone()
    };
    let mut built = Vec::new();
    for index in 0..9u64 {
        // A distinct button each time: the same one joins the hold that exists
        // rather than beginning another, and this needs separate admissions.
        let delivery = 78320 + index;
        let button = 273 + u32::try_from(index).expect("small");
        f.ingress
            .submit(button_to(
                f.surface,
                XAuthorityInputDeliveryId::from_raw(delivery),
                button,
                true,
            ))
            .unwrap();
        {
            let PrivatePreparedRunner {
                frontend,
                keyboards,
                watch,
                ..
            } = &mut f.runner;
            let private = frontend.as_mut().unwrap();
            private
                .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
                .unwrap();
            let Some(PrivateOrderedItem::Ran { custody, .. }) = private.terminal.turn.pop() else {
                // The source will not accept another press here. However many
                // were built is what this control has to work with.
                break;
            };
            assert!(custody.observe().unwrap().is_some());
        }
        let private = f.runner.frontend.as_mut().unwrap();
        let recovery = private.broker.registry.input_recovery.clone();
        let cell = admitted_cell(private, delivery);
        let record = private
            .terminal
            .holds
            .iter_mut()
            .find(|record| {
                record
                    .custody
                    .completion
                    .as_ref()
                    .is_some_and(|held| Arc::ptr_eq(held, &cell))
            })
            .expect("this admission's own hold");
        assert_eq!(record.reached.client(), other, "owed to the other endpoint");
        let emission = record
            .native
            .as_mut()
            .unwrap()
            .take_press_emission()
            .expect("its own press emission");
        PrivateXServerFrontend::stow_press_capsule(&mut record.custody, emission, &recovery, other);
        let Some(PrivatePendingDelivery::Capsule(capsule)) = record.custody.pending.take() else {
            panic!("it built its own capsule")
        };
        built.push(capsule);
    }
    let bound = {
        let private = f.runner.frontend.as_ref().unwrap();
        private.broker.registry.per_client_input_capacity.get()
    };
    assert_eq!(
        built.len(),
        bound,
        "enough real foreign capsules to reach the bound exactly"
    );

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    assert_eq!(owner.retention, bound, "retention is the queue's own capacity");
    owner
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .expect("a socket pair ends");

    // One at a time, each classified and retained, right up to the bound.
    let mut retained = 0usize;
    for capsule in built {
        sender.try_send(capsule).expect("onto this connection's queue");
        assert_eq!(
            owner.advance_close(XByteOrder::LittleEndian, 7),
            X11OrderedCloseStep::Foreign,
            "another endpoint's capsule is classified and retained, never offered"
        );
        retained += 1;
    }
    assert_eq!(retained, bound);
    assert_eq!(owner.retained_foreign().len(), bound);

    // AT THE BOUND, NOT PAST IT. The next visit reports backpressure without
    // receiving anything, and the store is still exactly what was reserved.
    assert_eq!(
        owner.advance_close(XByteOrder::LittleEndian, 7),
        X11OrderedCloseStep::Backpressure,
        "retention at its bound is backpressure, not a bigger store"
    );
    assert_eq!(owner.retained_foreign().len(), bound, "nothing more was taken");
    assert_eq!(
        owner.retention_capacity(),
        (bound, bound),
        "and neither store ever grew to make room"
    );
    drop(other_registration);
}

#[test]
fn an_unterminated_close_publishes_nothing_and_a_real_retry_publishes_once() {
    // A close that could not end its wire recorded the fact and then nothing
    // read it: the driver tested only that a close existed, so it published
    // ClientDisconnected for an admission whose connection was still carrying
    // bytes. And asking again returned success because a close existed,
    // acknowledging an ending nobody had attempted a second time.
    //
    // STAGED AT THE STATE, AND SAID SO. std on this host gives no way to make
    // shutdown fail other than NotConnected, which is an ending; I tried an
    // already-ended handle and it reports success, so a control built that way
    // would silently exercise the happy path while looking like it covered
    // both. The refused termination is written directly instead. The retry
    // below is real: it calls the actual shutdown on the actual socket.
    let client = XServerFrontendClientId(7841);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78410, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78410);
    for _ in 0..8 {
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }

    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    owner
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .expect("the first attempt ends this socket");
    // Exactly what a refused shutdown leaves: the close is recorded, serving is
    // excluded, and termination is not a fact.
    {
        let closing = owner.closing.as_mut().expect("closing");
        closing.termination =
            X11OrderedTermination::Refused(std::io::ErrorKind::PermissionDenied);
        closing.attempts = 1;
    }

    // NOTHING DERIVED FROM AN ENDING THAT DID NOT HAPPEN.
    assert!(
        matches!(
            owner.advance_close(XByteOrder::LittleEndian, 7),
            X11OrderedCloseStep::TerminationUnconfirmed(std::io::ErrorKind::PermissionDenied)
        ),
        "the driver refuses to offer anything"
    );
    assert!(
        matches!(
            owner.adjudicate_in_flight(),
            X11OrderedCloseStep::TerminationUnconfirmed(std::io::ErrorKind::PermissionDenied)
        ),
        "and so does the offer itself, so a later caller cannot go round the driver"
    );
    assert!(
        cell.answer().is_none(),
        "an ending that did not happen answers nobody"
    );
    assert!(
        owner.in_flight().is_none(),
        "and nothing was received under an unconfirmed close"
    );
    // Serving stays excluded throughout.
    assert!(matches!(
        owner.serve_one(XByteOrder::LittleEndian, 7),
        X11OrderedServeStep::Closing
    ));

    // A REPEATED REQUEST IS A REAL RETRY. It calls the actual shutdown again,
    // keeps the original cause, and succeeds here.
    owner
        .begin_close(X11OrderedCloseCause::SupervisorStopped)
        .expect("the retry ends the wire");
    let closing = owner.closing().expect("closing");
    assert_eq!(
        closing.termination,
        X11OrderedTermination::Established,
        "and only that authorises anything"
    );
    assert_eq!(
        closing.cause,
        X11OrderedCloseCause::ConnectionEnded,
        "the original close's identity is kept, not replaced by the retry's"
    );
    assert_eq!(closing.attempts, 2, "the retry was an attempt, not a lookup");

    let steps = close_to_quiet(&mut owner, X11OrderedCloseCause::ConnectionEnded);
    assert_eq!(
        cell.answer().map(|answer| answer.outcome),
        Some(XAuthorityInputDeliveryOutcome::ClientDisconnected),
        "{steps:?}"
    );
    let published = steps
        .iter()
        .filter(|step| {
            matches!(
                step,
                X11OrderedCloseStep::Adjudicated(PrivateAdjudication::Answered)
            )
        })
        .count();
    assert_eq!(published, 1, "exactly one terminal publication: {steps:?}");
}

#[test]
fn a_close_stops_retrying_a_termination_that_keeps_refusing() {
    // The retry is bounded. A close that kept asking forever is one that never
    // finishes, and whoever is waiting for this connection to end waits with
    // it. Staged at the state, as above.
    let client = XServerFrontendClientId(7851);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78510, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78510);
    for _ in 0..8 {
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }
    let (socket, _peer) = UnixStream::pair().unwrap();
    let (mut owner, _output) = serving_owner_for(&mut f, socket);
    owner
        .begin_close(X11OrderedCloseCause::ConnectionEnded)
        .expect("the first attempt ends this socket");
    {
        let closing = owner.closing.as_mut().expect("closing");
        closing.termination =
            X11OrderedTermination::Refused(std::io::ErrorKind::PermissionDenied);
        closing.attempts = X11_ORDERED_CLOSE_ATTEMPTS;
    }
    assert_eq!(
        owner.begin_close(X11OrderedCloseCause::ConnectionEnded),
        Err(std::io::ErrorKind::PermissionDenied),
        "past its bound it reports the unresolved termination rather than trying again"
    );
    assert_eq!(
        owner.closing().expect("closing").attempts,
        X11_ORDERED_CLOSE_ATTEMPTS,
        "and does not attempt past the bound"
    );
    assert!(cell.answer().is_none(), "still nothing offered");
}

#[test]
fn a_barred_wire_stops_every_writer_of_that_socket_including_control() {
    // A latch private to one writer fences only that writer. The permission is
    // the connection's, read under the same serialization every post-exposure
    // writer takes, so barring it closes the wire to all of them -- control
    // included, because control is the writer with PRIORITY, not the writer
    // allowed to follow the beginning of an event nobody can finish.
    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = AtomicUsize::new(0);
    let sequence = AtomicU16::new(1);

    // Open: every path takes the wire.
    assert!(
        lock_x11_non_control_output(&output, &wire, &pending, None)
            .expect("a readable output")
            .is_some(),
        "the non-control path writes while the wire is open"
    );
    write_x11_control_records(
        &output,
        &wire,
        XByteOrder::LittleEndian,
        &sequence,
        vec![vec![0u8; 32]],
    )
    .expect("control writes while the wire is open");

    wire.bar();

    // Barred: every path refuses, and refuses for what it is.
    let non_control = lock_x11_non_control_output(&output, &wire, &pending, None)
        .expect_err("the non-control path is barred");
    assert!(
        non_control.client_failure,
        "a wire holding an unfinished event is this client's failure, not the service's"
    );
    let control = write_x11_control_records(
        &output,
        &wire,
        XByteOrder::LittleEndian,
        &sequence,
        vec![vec![0u8; 32]],
    )
    .expect_err("control is barred too");
    assert!(control.client_failure);
    assert!(
        !control.service_shutdown,
        "one connection's unusable wire does not end the service"
    );

    // And control never waits on its own pending counter: with a control
    // registered as pending, the control path still reaches its refusal rather
    // than spinning.
    pending.store(1, Ordering::Release);
    assert!(
        write_x11_control_records(
            &output,
            &wire,
            XByteOrder::LittleEndian,
            &sequence,
            vec![vec![0u8; 32]],
        )
        .is_err(),
        "control does not yield to control"
    );
}

#[test]
fn a_serving_owner_holds_its_connections_permission_not_one_of_its_own() {
    // What makes barring effective is that the owner holds the CONNECTION'S
    // permission. The trigger -- a shutdown that refuses after a part-written
    // frame -- is the branch this host gives me no honest way to reach, so
    // what this pins is the wiring: bar through the owner's own handle and
    // every writer of that socket is stopped.
    let client = XServerFrontendClientId(7861);
    let f = prepared_ordered_fixture(client);
    let (socket, _peer) = UnixStream::pair().expect("a socket pair");
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let transport =
        XAuthorityOrderedTransport::bind(&f.registration, f.channels.ordered, &output, &wire, &pending, None)
            .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    assert!(
        Arc::ptr_eq(&owner.wire, &wire),
        "the owner holds the connection's own permission, not a private latch"
    );
    let pending = AtomicUsize::new(0);
    assert!(
        lock_x11_non_control_output(&output, &wire, &pending, None)
            .expect("a readable output")
            .is_some()
    );

    // Barred through the handle the owner holds.
    owner.wire.bar();
    assert!(
        lock_x11_non_control_output(&output, &wire, &pending, None).is_err(),
        "barring through the owner stops the other writers of that socket"
    );
    // AND STOPS THE ORDERED STEP ITSELF. Holding the permission and never
    // asking it fenced nothing: the owner went on taking capsules and writing
    // them through the raw lock while every other writer was refused.
    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::WireBarred
        ),
        "the ordered step reads the same permission as everyone else"
    );
    assert!(
        owner.in_flight().is_none(),
        "and took nothing while it was barred"
    );
    assert!(
        write_x11_control_records(
            &output,
            &wire,
            XByteOrder::LittleEndian,
            &AtomicU16::new(1),
            vec![vec![0u8; 32]],
        )
        .is_err(),
        "control included"
    );
}

#[test]
fn an_ordered_step_yields_to_control_and_acts_on_being_stopped() {
    // Taking the raw lock skipped two things every other non-control writer
    // observes: the control-priority yield, and this writer's own stop. A stop
    // nothing acts on makes a join unbounded however carefully it was set.
    let client = XServerFrontendClientId(7871);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78710, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78710);
    for _ in 0..8 {
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let ordered = std::mem::replace(
        &mut f.channels.ordered,
        f.runner
            .frontend
            .as_ref()
            .unwrap()
            .broker
            .registry
            .register_client_with_admission(
                XServerFrontendClientId(f.client.raw() + 500_000),
                Some(admitted(XServerFrontendClientId(f.client.raw() + 500_000))),
            )
            .expect("a spare registration to borrow a receiver from")
            .1
            .ordered,
    );
    let transport = XAuthorityOrderedTransport::bind(
        &f.registration,
        ordered,
        &output,
        &wire,
        &pending,
        Some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    // TOLD TO STOP WHILE CONTROL IS PENDING. It yields, sees the stop, and
    // leaves -- taking nothing, writing nothing, answering nothing, and saying
    // so rather than reporting an empty queue.
    pending.store(1, Ordering::Release);
    stop.store(true, Ordering::Release);
    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Stopped
        ),
        "a stopped writer says it is leaving, not that there was nothing to do"
    );
    assert!(owner.in_flight().is_none(), "it took nothing");
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "and wrote nothing"
    );
    assert!(cell.answer().is_none(), "and answered nobody");

    // WITH NO CONTROL PENDING AT ALL. The shared wait only observes stop while
    // control is pending, so this is the case that saw nothing and served on.
    pending.store(0, Ordering::Release);
    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Stopped
        ),
        "stop is this writer's own question, not one control has to raise"
    );
    assert!(owner.in_flight().is_none());
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "and still nothing written"
    );
    assert!(cell.answer().is_none());

    // Cleared: ordinary resumption. This proves serving after control has
    // finished and the stop is lifted -- not live waiting, which this control
    // does not test.
    stop.store(false, Ordering::Release);
    assert!(
        matches!(
            owner.serve_one(XByteOrder::LittleEndian, 7),
            X11OrderedServeStep::Advanced | X11OrderedServeStep::Flushed
        ),
        "with no stop and no control pending, its own event is served"
    );
}

#[test]
fn a_stop_set_while_waiting_for_the_output_is_seen_before_taking_custody() {
    // An entry-only check leaves the window between passing it and acquiring
    // serialization. This closes it from the other side: the stop is set while
    // the owner is blocked on the mutex, so it can only be seen by asking
    // again under the guard.
    let client = XServerFrontendClientId(7881);
    let mut f = prepared_ordered_fixture(client);
    attempt_run(&mut f, 78810, 272, true);
    let private = f.runner.frontend.as_mut().unwrap();
    let cell = admitted_cell(private, 78810);
    for _ in 0..8 {
        private.deliver_one(&mut |_, _| Ok(())).unwrap();
    }

    let (socket, peer) = UnixStream::pair().expect("a socket pair");
    peer.set_nonblocking(true).expect("a readable peer");
    let output = Arc::new(Mutex::new(socket));
    let wire = Arc::new(X11WirePermission::open());
    let pending = Arc::new(AtomicUsize::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let ordered = std::mem::replace(
        &mut f.channels.ordered,
        f.runner
            .frontend
            .as_ref()
            .unwrap()
            .broker
            .registry
            .register_client_with_admission(
                XServerFrontendClientId(f.client.raw() + 500_000),
                Some(admitted(XServerFrontendClientId(f.client.raw() + 500_000))),
            )
            .expect("a spare registration to borrow a receiver from")
            .1
            .ordered,
    );
    let transport = XAuthorityOrderedTransport::bind(
        &f.registration,
        ordered,
        &output,
        &wire,
        &pending,
        Some(&stop),
    )
    .unwrap_or_else(|(refusal, _)| panic!("its own receiver and output bind: {refusal:?}"));
    let private = f.runner.frontend.as_ref().unwrap();
    let mut owner = X11OrderedServingOwner::for_registration(private, &f.registration, transport)
        .unwrap_or_else(|(refusal, _)| panic!("an owner for this registration: {refusal:?}"));

    // The parent takes the output, so the owner's serving step blocks on it
    // after having passed any entry check.
    let held = output.lock().expect("the connection's own output");
    let serving_stop = stop.clone();
    let (started, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        started.send(()).expect("started");
        let step = owner.serve_one(XByteOrder::LittleEndian, 7);
        (step, owner)
    });
    wait.recv().expect("the serving thread started");
    // It is now either about to take the lock or blocked on it. Setting the
    // stop here can only be observed by a check under the acquired guard.
    std::thread::sleep(Duration::from_millis(50));
    serving_stop.store(true, Ordering::Release);
    drop(held);

    let (step, owner) = server.join().expect("the serving thread finished");
    assert!(
        matches!(step, X11OrderedServeStep::Stopped),
        "a stop set while waiting for output is seen before custody is taken, got {step:?}"
    );
    assert!(owner.in_flight().is_none(), "it took nothing");
    let mut byte = [0u8; 1];
    assert!(
        matches!(
            (&peer).read(&mut byte),
            Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "and wrote nothing"
    );
    assert!(cell.answer().is_none(), "and answered nobody");
}
