// Routed input reaching a surface: the pointer grab's sanitized lease, keyboard
// routing, and what a route does when its target moves or goes.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.

impl PrivateXServerFrontend {
    /// Unaccounted fixture turn. Production uses the original service budget
    /// around each `deliver_one`; this helper owns only a finite test visit.
    fn deliver_turn(&mut self, items: Vec<PrivateOrderedItem>) -> Vec<PrivateDelivered> {
        // Appending preserves custody left by an earlier interruption.
        self.terminal.delivering.extend(items);
        let mut delivered = Vec::with_capacity(self.terminal.delivering.len());
        // Advanced includes an unsuccessful retained-request observation.
        // Those visits rotate custody, so waiting for a non-Advanced result
        // would spin forever on an unresolved item. Give every entry-owned
        // request a visit, including a ready one behind an unresolved head.
        // The final probe preserves native handover after normal delivery.
        let visits = self.terminal.delivering.len()
            + self.terminal.turn.len()
            + self.terminal.undelivered.len()
            + 1;
        for _ in 0..visits {
            let Ok(PrivateDeliveryStep::Advanced { report, .. }) =
                self.deliver_one(None, &mut |_, _| Ok(()))
            else {
                break;
            };
            delivered.extend(report);
        }
        delivered
    }
}

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
            origin: XAuthorityRoutedInputOrigin::Physical,
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
            origin: XAuthorityRoutedInputOrigin::Physical,
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
            grab_crossing: None,
            grab_target: None,
            propagation_stop: None,
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
                origin: XAuthorityRoutedInputOrigin::Physical,
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
            origin: XAuthorityRoutedInputOrigin::Physical,
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
            origin: XAuthorityRoutedInputOrigin::Physical,
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
            origin: XAuthorityRoutedInputOrigin::Physical,
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
            origin: XAuthorityRoutedInputOrigin::Physical,
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
            origin: XAuthorityRoutedInputOrigin::Physical,
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
            origin: XAuthorityRoutedInputOrigin::Physical,
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
            origin: XAuthorityRoutedInputOrigin::Physical,
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
    assert_eq!(selections.selected_keyboard_target(child, true), None);
    selections.update(parent, Some(1), None);

    assert_eq!(selections.keyboard_target(child, true), parent);
    assert_eq!(selections.selected_keyboard_target(child, true), Some(parent));

    assert_eq!(
        selections.keyboard_target(XResourceId::new(0x200009, 1), true),
        XResourceId::new(0x200009, 1)
    );
}

#[test]
fn keyboard_delivery_falls_back_to_engine_focused_surface() {
    let selections = XCoreEventSelectionState::default();

    assert_eq!(
        selections.keyboard_target(XResourceId::new(0x200001, 1), true),
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
    assert_eq!(selections.keyboard_target(root, true), upper);

    selections.restack(lower, Some(upper), Some(0));
    assert_eq!(selections.keyboard_target(root, true), lower);

    selections.observe_unmapped(lower);
    assert_eq!(selections.keyboard_target(root, true), upper);
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
