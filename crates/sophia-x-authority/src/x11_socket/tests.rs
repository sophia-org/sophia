#![cfg(all(test, unix))]

use super::*;
use crate::XAuthorityControlKind;
use sophia_protocol::{DeviceId, Point};
use std::sync::mpsc::sync_channel;

#[test]
fn pending_control_wins_the_request_runtime_lock_race() {
    let runtime = Arc::new(Mutex::new(XAuthorityRuntime::new()));
    let control_pending = Arc::new(AtomicUsize::new(0));
    let initial_guard = runtime.lock().unwrap();
    let (request_started_sender, request_started_receiver) = sync_channel(1);
    let (order_sender, order_receiver) = sync_channel(2);

    let request_runtime = runtime.clone();
    let request_pending = control_pending.clone();
    let request_order = order_sender.clone();
    let request = std::thread::spawn(move || {
        request_started_sender.send(()).unwrap();
        let _guard = lock_x11_request_runtime(&request_runtime, &request_pending).unwrap();
        request_order.send("request").unwrap();
    });
    request_started_receiver.recv().unwrap();

    control_pending.fetch_add(1, Ordering::AcqRel);
    let control_runtime = runtime.clone();
    let control_count = control_pending.clone();
    let control = std::thread::spawn(move || {
        let _guard = control_runtime.lock().unwrap();
        control_count.fetch_sub(1, Ordering::AcqRel);
        order_sender.send("control").unwrap();
    });

    drop(initial_guard);
    assert_eq!(
        order_receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
        "control"
    );
    assert_eq!(
        order_receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
        "request"
    );
    control.join().unwrap();
    request.join().unwrap();
}

#[test]
fn xi_key_selection_bypasses_core_keyboard_startup_wait() {
    assert!(x11_keyboard_route_ready(true, true, false, false));
    assert!(x11_keyboard_route_ready(true, false, true, false));
    assert!(!x11_keyboard_route_ready(true, false, false, false));
    assert!(x11_keyboard_route_ready(true, false, false, true));
    assert!(x11_keyboard_route_ready(false, false, false, false));
}

#[test]
fn input_delivery_notifications_do_not_backpressure_x11_writers() {
    let client = XServerFrontendClientId::from_raw(1);
    let (_route_sender, route_receiver) = sync_channel(1);
    let (delivery_sender, delivery_receiver) = channel();
    let receiver = X11InputEventReceiver::Routed {
        receiver: route_receiver,
        deliveries: Some(delivery_sender),
        recovery: None,
    };

    for raw in 1..=1_024 {
        receiver
            .send_delivery(
                client,
                Some(XAuthorityInputDeliveryId::from_raw(raw)),
                XAuthorityInputDeliveryOutcome::Flushed,
            )
            .unwrap();
    }

    assert_eq!(delivery_receiver.try_iter().count(), 1_024);
}

#[test]
fn listener_transaction_ids_are_global_across_client_workers() {
    let state = X11CoreSocketServerState::new();
    let first_worker = state.clone();
    let second_worker = state.clone();

    let first = first_worker.allocate_transaction().unwrap();
    let second = second_worker.allocate_transaction().unwrap();
    let third = first_worker.allocate_transaction().unwrap();

    assert_ne!(first, second);
    assert_ne!(second, third);
    assert_eq!(first.raw() + 1, second.raw());
    assert_eq!(second.raw() + 1, third.raw());
}

#[test]
fn pointer_target_prefers_mapped_button_selecting_content_child() {
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let top_level = XResourceId::new(0x200001, 1);
    let key_child = XResourceId::new(0x200002, 1);
    let content_child = XResourceId::new(0x200003, 1);
    let mut selections = XCoreEventSelectionState::default();
    selections.register(
        top_level,
        root,
        Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
    );
    selections.register(
        key_child,
        top_level,
        Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 80,
        },
    );
    selections.register(
        content_child,
        top_level,
        Rect {
            x: 0,
            y: 80,
            width: 800,
            height: 520,
        },
    );
    selections.update(top_level, Some((1 << 2) | (1 << 3)), None);
    selections.update(key_child, Some((1 << 0) | (1 << 1)), None);
    selections.update(content_child, Some((1 << 2) | (1 << 3)), None);
    selections.observe_mapped(top_level);
    selections.observe_mapped(key_child);
    selections.observe_mapped(content_child);

    assert_eq!(
        selections.selected_pointer_target(top_level, false, 100, 200),
        Some(content_child)
    );
    assert_eq!(
        selections.selected_pointer_target(top_level, true, 100, 200),
        None
    );
    assert_eq!(
        selections.pointer_event_coordinates(top_level, content_child, 100, 200),
        (100, 120)
    );
    assert_eq!(
        selections.pointer_event_target(top_level, 100, 200),
        content_child
    );
    assert_eq!(
        selections.ancestry_including(content_child),
        vec![content_child, top_level, root]
    );
}

#[test]
fn pointer_event_target_does_not_depend_on_core_event_selection() {
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let top_level = XResourceId::new(0x200001, 1);
    let content_child = XResourceId::new(0x200002, 1);
    let mut selections = XCoreEventSelectionState::default();
    selections.register(
        top_level,
        root,
        Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
    );
    selections.register(
        content_child,
        top_level,
        Rect {
            x: 0,
            y: 80,
            width: 800,
            height: 520,
        },
    );
    selections.observe_mapped(top_level);
    selections.observe_mapped(content_child);

    assert_eq!(
        selections.pointer_event_target(top_level, 100, 200),
        content_child
    );
    assert_eq!(
        selections.selected_pointer_target(top_level, false, 100, 200),
        None
    );

    let namespace = NamespaceId::from_raw(17);
    let owner = 4;
    let mut authority = crate::XInputAuthorityState::default();
    authority.select_xi_events(namespace, owner, content_child, &[(2, vec![1 << 6])]);
    assert_eq!(
        x11_selected_xi_event_window(
            &authority,
            namespace,
            owner,
            &selections.ancestry_including(content_child),
            2,
            6,
        ),
        Some(content_child)
    );
}

#[test]
fn pointer_event_target_follows_window_hierarchy_after_parent_restack() {
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let top_level = XResourceId::new(0x200001, 1);
    let content_child = XResourceId::new(0x200002, 1);
    let nested_child = XResourceId::new(0x200003, 1);
    let mut selections = XCoreEventSelectionState::default();
    selections.register(
        top_level,
        root,
        Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
    );
    selections.register(
        content_child,
        top_level,
        Rect {
            x: 0,
            y: 80,
            width: 800,
            height: 520,
        },
    );
    selections.register(
        nested_child,
        content_child,
        Rect {
            x: 20,
            y: 20,
            width: 760,
            height: 480,
        },
    );
    selections.observe_mapped(top_level);
    selections.observe_mapped(content_child);
    selections.observe_mapped(nested_child);

    // A WM may restack the managed top-level after its client-owned children.
    // That must not make the parent win a flat-stack hit test over its child.
    selections.restack(top_level, None, Some(0));

    assert_eq!(
        selections.pointer_event_target(top_level, 100, 200),
        nested_child
    );
}

#[test]
fn core_pointer_selection_propagates_only_through_hit_target_ancestors() {
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let top_level = XResourceId::new(0x200001, 1);
    let content_child = XResourceId::new(0x200002, 1);
    let overlay_sibling = XResourceId::new(0x200003, 1);
    let mut selections = XCoreEventSelectionState::default();
    selections.register(
        top_level,
        root,
        Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        },
    );
    selections.register(
        content_child,
        top_level,
        Rect {
            x: 0,
            y: 80,
            width: 800,
            height: 520,
        },
    );
    selections.register(
        overlay_sibling,
        top_level,
        Rect {
            x: 0,
            y: 0,
            width: 800,
            height: 80,
        },
    );
    selections.update(top_level, Some(1 << 2), None);
    selections.update(overlay_sibling, Some(1 << 2), None);
    selections.observe_mapped(top_level);
    selections.observe_mapped(content_child);
    selections.observe_mapped(overlay_sibling);

    assert_eq!(
        selections.pointer_event_target(top_level, 100, 200),
        content_child
    );
    assert_eq!(
        selections.selected_pointer_target(top_level, false, 100, 200),
        Some(top_level)
    );
}

#[test]
fn explicit_pointer_window_does_not_require_a_live_surface_mapping() {
    let surface = SurfaceId::new(18, 1);
    let target = XResourceId::new(0x200001, 1);
    let surface_windows = Mutex::new(BTreeMap::new());

    assert_eq!(
        x11_pointer_surface_window(Some(target), surface, &surface_windows).unwrap(),
        Some(target)
    );
    assert_eq!(
        x11_pointer_surface_window(None, surface, &surface_windows).unwrap(),
        None
    );
}

#[test]
fn pointer_query_reports_latest_engine_routed_position_and_child() {
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let top_level = XResourceId::new(0x200001, 1);
    let content_child = XResourceId::new(0x200002, 1);
    let mut selections = XCoreEventSelectionState::default();
    selections.register(
        top_level,
        root,
        Rect {
            x: 2,
            y: 2,
            width: 800,
            height: 600,
        },
    );
    selections.register(
        content_child,
        top_level,
        Rect {
            x: 2,
            y: 2,
            width: 780,
            height: 580,
        },
    );
    selections.observe_pointer(top_level, content_child, 63, 237, 61, 235, 0);

    assert_eq!(
        selections.query_pointer(root),
        Some(XCorePointerQuery {
            child: top_level,
            root_x: 63,
            root_y: 237,
            win_x: 63,
            win_y: 237,
            mask: 0,
        })
    );
    assert_eq!(
        selections.query_pointer(top_level),
        Some(XCorePointerQuery {
            child: content_child,
            root_x: 63,
            root_y: 237,
            win_x: 61,
            win_y: 235,
            mask: 0,
        })
    );
    assert_eq!(
        selections.query_pointer(content_child),
        Some(XCorePointerQuery {
            child: XResourceId::NONE,
            root_x: 63,
            root_y: 237,
            win_x: 59,
            win_y: 233,
            mask: 0,
        })
    );
}

#[test]
fn routed_input_discards_another_clients_event() {
    let first = XServerFrontendClientId(1);
    let second = XServerFrontendClientId(2);
    let (sender, receiver) = sync_channel(2);
    sender
        .send(XAuthorityClientInputEvent {
            client: second,
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
        .unwrap();
    sender
        .send(XAuthorityClientInputEvent {
            client: first,
            event: XAuthorityKeyEvent {
                keycode: 25,
                pressed: true,
                state: 0,
                modifiers_after: 0,
                time_msec: 2,
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
        .unwrap();

    let receiver = X11InputEventReceiver::Routed {
        receiver,
        deliveries: None,
        recovery: None,
    };
    assert_eq!(receiver.recv_timeout(first), Err(RecvTimeoutError::Timeout));
    assert_eq!(
        receiver.recv_timeout(first).unwrap(),
        (
            XAuthorityInputEvent::Key(XAuthorityKeyEvent {
                keycode: 25,
                pressed: true,
                state: 0,
                modifiers_after: 0,
                time_msec: 2,
            }),
            None,
            None,
            None,
            None,
            None,
            0,
            None,
        )
    );
}

#[test]
fn routed_control_discards_another_clients_command_and_labels_its_ack() {
    let first = XServerFrontendClientId(1);
    let second = XServerFrontendClientId(2);
    let surface = SurfaceId::new(44, 1);
    let (command_sender, command_receiver) = sync_channel(2);
    let (ack_sender, ack_receiver) = sync_channel(1);
    let command = XAuthorityControlCommand::FocusSurface {
        transaction: TransactionId::from_raw(7),
        surface,
    };
    command_sender
        .send(XAuthorityClientControlCommand {
            client: second,
            command,
        })
        .unwrap();
    command_sender
        .send(XAuthorityClientControlCommand {
            client: first,
            command,
        })
        .unwrap();

    let channels = X11ControlChannels::Routed {
        receiver: command_receiver,
        acknowledgements: ack_sender,
        completion: None,
    };
    assert!(matches!(
        channels.recv_timeout(first),
        Err(RecvTimeoutError::Timeout)
    ));
    assert_eq!(
        channels.recv_timeout(first).unwrap().authority_command(),
        Some(command)
    );
    let acknowledgement = XAuthorityControlAck {
        kind: command.kind(),
        transaction: command.transaction(),
        surface: command.surface(),
        outcome: XAuthorityControlOutcome::Delivered,
    };
    channels.send_ack_for(first, acknowledgement, None).unwrap();
    assert_eq!(
        ack_receiver.recv().unwrap(),
        XAuthorityClientControlAck {
            client: first,
            acknowledgement,
        }
    );
}

#[test]
fn route_broker_delivers_to_the_registered_client_only() {
    let client = XServerFrontendClientId(9);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(2).unwrap());
    let (registration, channels) = broker.registry.register_client(client).unwrap();
    let input = XAuthorityInputEvent::Key(XAuthorityKeyEvent {
        keycode: 38,
        pressed: true,
        state: 0,
        modifiers_after: 0,
        time_msec: 3,
    });
    let command = XAuthorityControlCommand::FocusSurface {
        transaction: TransactionId::from_raw(8),
        surface: SurfaceId::new(45, 1),
    };

    broker
        .input_sender()
        .expect("an ungated broker to expose raw ingress")
        .send(XAuthorityClientInputEvent {
            client,
            event: input,
            target_window: None,
            xi_event_type: None,
            xi_event_window: None,
            xi_emulated_button_type: None,
            xi_emulated_button_window: None,
            xi_pointer_crossing_mask: 0,
            delivery: None,
        })
        .unwrap();
    broker
        .control_sender()
        .send(XAuthorityClientControlCommand { client, command })
        .unwrap();

    assert_eq!(broker.route_pending(), Ok(2));
    assert_eq!(
        channels.input.recv().unwrap(),
        XAuthorityClientInputEvent {
            client,
            event: input,
            target_window: None,
            xi_event_type: None,
            xi_event_window: None,
            xi_emulated_button_type: None,
            xi_emulated_button_window: None,
            xi_pointer_crossing_mask: 0,
            delivery: None,
        }
    );
    assert_eq!(
        channels.control.recv().unwrap().authority_command(),
        Some(command)
    );
    let acknowledgement = XAuthorityControlAck {
        kind: command.kind(),
        transaction: command.transaction(),
        surface: command.surface(),
        outcome: XAuthorityControlOutcome::Delivered,
    };
    let channels = X11ControlChannels::ClientBound {
        receiver: channels.control,
        acknowledgements: broker.registry.acknowledgement_sender.clone(),
        completion: None,
    };
    channels
        .send_ack_for(client, acknowledgement, None)
        .unwrap();
    assert_eq!(
        broker
            .recv_control_ack_timeout(Duration::from_millis(1))
            .unwrap(),
        XAuthorityClientControlAck {
            client,
            acknowledgement,
        }
    );
    assert_eq!(broker.registered_client_count(), 1);

    drop(registration);
    assert_eq!(broker.registered_client_count(), 0);
    broker
        .input_sender()
        .expect("an ungated broker to expose raw ingress")
        .send(XAuthorityClientInputEvent {
            client,
            event: input,
            target_window: None,
            xi_event_type: None,
            xi_event_window: None,
            xi_emulated_button_type: None,
            xi_emulated_button_window: None,
            xi_pointer_crossing_mask: 0,
            delivery: None,
        })
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(0));
}

#[test]
fn clearing_old_present_selection_preserves_active_window_feedback() {
    let namespace = NamespaceId::from_raw(10);
    let client = XServerFrontendClientId(9);
    let surface = SurfaceId::new(11, 1);
    let bootstrap_window = XResourceId::new(0x200009, 1);
    let bootstrap_event = XResourceId::new(0x20000d, 1);
    let main_window = XResourceId::new(0x200010, 1);
    let main_event = XResourceId::new(0x200014, 1);
    let pixmap = XResourceId::new(0x200015, 1);
    let idle_fence = XResourceId::new(0x200016, 1);
    let transaction = TransactionId::from_raw(202);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, main_window)
        .unwrap();

    broker
        .registry
        .select_present_input(client, bootstrap_event, bootstrap_window, 7)
        .unwrap();
    broker
        .registry
        .select_present_input(client, main_event, main_window, 7)
        .unwrap();
    broker
        .registry
        .select_present_input(client, bootstrap_event, bootstrap_window, 0)
        .unwrap();
    broker
        .registry
        .queue_present(
            transaction,
            client,
            main_window,
            pixmap,
            1,
            Some(idle_fence),
            false,
        )
        .unwrap();

    assert_eq!(
        broker.route_present_complete(
            transaction,
            1_188_203,
            7_668_086,
            XPresentCompletionMode::Flip,
        ),
        Ok(true)
    );
    assert_eq!(
        channels.protocol.recv().unwrap(),
        XClientEvent::PresentCompleteNotify {
            sequence: 0,
            event_id: main_event,
            window: main_window,
            serial: 1,
            ust: 1_188_203,
            msc: 7_668_086,
            kind: 0,
            mode: XPresentCompletionMode::Flip as u8,
        }
    );
    assert_eq!(broker.route_present_idle(transaction), Ok(true));
    assert_eq!(
        channels.protocol.recv().unwrap(),
        XClientEvent::PresentIdleNotify {
            sequence: 0,
            event_id: main_event,
            window: main_window,
            serial: 1,
            pixmap,
            idle_fence: Some(idle_fence),
        }
    );
}

#[test]
fn present_feedback_reaches_every_matching_event_selection() {
    let namespace = NamespaceId::from_raw(10);
    let client = XServerFrontendClientId(10);
    let surface = SurfaceId::new(12, 1);
    let window = XResourceId::new(0x300010, 1);
    let first_event = XResourceId::new(0x300014, 1);
    let second_event = XResourceId::new(0x300015, 1);
    let pixmap = XResourceId::new(0x300016, 1);
    let transaction = TransactionId::from_raw(203);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());
    let (registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    for event_id in [first_event, second_event] {
        broker
            .registry
            .select_present_input(client, event_id, window, 7)
            .unwrap();
    }
    broker
        .registry
        .queue_present(transaction, client, window, pixmap, 2, None, false)
        .unwrap();

    assert_eq!(
        broker.route_present_complete(transaction, 10, 20, XPresentCompletionMode::Flip),
        Ok(true)
    );
    for event_id in [first_event, second_event] {
        assert!(matches!(
            channels.protocol.recv().unwrap(),
            XClientEvent::PresentCompleteNotify {
                event_id: routed_event,
                ..
            } if routed_event == event_id
        ));
    }
    assert_eq!(broker.route_present_idle(transaction), Ok(true));
    for event_id in [first_event, second_event] {
        assert!(matches!(
            channels.protocol.recv().unwrap(),
            XClientEvent::PresentIdleNotify {
                event_id: routed_event,
                ..
            } if routed_event == event_id
        ));
    }

    let disconnected = TransactionId::from_raw(204);
    broker
        .registry
        .queue_present(disconnected, client, window, pixmap, 3, None, false)
        .unwrap();
    drop(registration);
    assert_eq!(
        broker.route_present_complete(disconnected, 30, 40, XPresentCompletionMode::Flip,),
        Ok(false)
    );
}

#[test]
fn present_protocol_capacity_covers_both_lifecycle_phases() {
    let namespace = NamespaceId::from_raw(11);
    let client = XServerFrontendClientId(11);
    let surface = SurfaceId::new(13, 1);
    let window = XResourceId::new(0x310010, 1);
    let event = XResourceId::new(0x310014, 1);
    let pixmap = XResourceId::new(0x310016, 1);
    let transaction = TransactionId::from_raw(205);
    let one = NonZeroUsize::new(1).unwrap();
    let two = NonZeroUsize::new(2).unwrap();
    let (acknowledgements, _) = sync_channel(1);
    let broker = XServerFrontendRouteBroker::with_transports(
        XServerFrontendRouteCapacities::new(one, one, two, one),
        acknowledgements,
        None,
        None,
        None,
    );
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .select_present_input(client, event, window, 0b110)
        .unwrap();
    broker
        .registry
        .queue_present(transaction, client, window, pixmap, 1, None, false)
        .unwrap();

    assert_eq!(
        broker.route_present_complete(transaction, 10, 20, XPresentCompletionMode::Copy),
        Ok(true)
    );
    assert_eq!(broker.route_present_idle(transaction), Ok(true));
    assert_eq!(channels.protocol.try_iter().count(), 2);
}

#[test]
fn present_configure_selection_uses_only_masked_matching_windows() {
    let client = XServerFrontendClientId(11);
    let other_client = XServerFrontendClientId(12);
    let window = XResourceId::new(0x400010, 1);
    let other_window = XResourceId::new(0x400011, 1);
    let configure = XResourceId::new(0x400014, 1);
    let feedback_only = XResourceId::new(0x400015, 1);
    let wrong_window = XResourceId::new(0x400016, 1);
    let wrong_client = XResourceId::new(0x400017, 1);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());

    broker
        .registry
        .select_present_input(client, configure, window, 0b111)
        .unwrap();
    broker
        .registry
        .select_present_input(client, feedback_only, window, 0b110)
        .unwrap();
    broker
        .registry
        .select_present_input(client, wrong_window, other_window, 0b001)
        .unwrap();
    broker
        .registry
        .select_present_input(other_client, wrong_client, window, 0b001)
        .unwrap();

    assert_eq!(
        broker
            .registry
            .present_configure_subscribers(window)
            .unwrap(),
        vec![(client, configure), (other_client, wrong_client)]
    );
}

#[test]
fn client_addressed_input_queue_saturation_does_not_fail_the_broker() {
    let stalled = XServerFrontendClientId(10);
    let healthy = XServerFrontendClientId(11);
    let mut broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(1).unwrap());
    let (_stalled_registration, _stalled_channels) =
        broker.registry.register_client(stalled).unwrap();
    let (_healthy_registration, healthy_channels) =
        broker.registry.register_client(healthy).unwrap();
    for time_msec in [4, 5] {
        broker
            .input_sender()
            .expect("an ungated broker to expose raw ingress")
            .send(XAuthorityClientInputEvent {
                client: stalled,
                event: XAuthorityKeyEvent {
                    keycode: 39,
                    pressed: true,
                    state: 0,
                    modifiers_after: 0,
                    time_msec,
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
            .unwrap();
        if time_msec == 4 {
            assert_eq!(broker.route_pending(), Ok(1));
        }
    }

    assert_eq!(broker.route_pending(), Ok(0));
    assert_eq!(broker.registered_client_count(), 1);
    broker
        .input_sender()
        .expect("an ungated broker to expose raw ingress")
        .send(XAuthorityClientInputEvent {
            client: healthy,
            event: XAuthorityKeyEvent {
                keycode: 40,
                pressed: true,
                state: 0,
                modifiers_after: 0,
                time_msec: 6,
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
        .unwrap();
    assert_eq!(broker.route_pending(), Ok(1));
    assert_eq!(
        healthy_channels.input.recv().unwrap().event,
        XAuthorityKeyEvent {
            keycode: 40,
            pressed: true,
            state: 0,
            modifiers_after: 0,
            time_msec: 6,
        }
        .into()
    );
}

#[test]
fn present_feedback_reaches_the_client_that_subscribed_not_the_one_that_presented() {
    // A browser subscribes from its GPU process for a window its browser
    // process created. X permits that, and Mesa depends on it: it blocks in
    // xcb_wait_for_special_event until an idle notify arrives, so feedback
    // withheld here is not an error the client can see -- it is a client that
    // never draws again.
    let namespace = NamespaceId::from_raw(61);
    let owner = XServerFrontendClientId::from_raw(1);
    let watcher = XServerFrontendClientId::from_raw(2);
    let surface = SurfaceId::new(41, 1);
    let window = XResourceId::new(0x300020, 1);
    let event_id = XResourceId::new(0x400021, 1);
    let pixmap = XResourceId::new(0x300022, 1);
    let transaction = TransactionId::from_raw(211);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());
    let (_owner_registration, owner_channels) = broker.registry.register_client(owner).unwrap();
    let (_watch_registration, watch_channels) = broker.registry.register_client(watcher).unwrap();
    broker
        .registry
        .register_surface(owner, namespace, surface, window)
        .unwrap();
    // The watcher subscribes on a window it does not own.
    broker
        .registry
        .select_present_input(watcher, event_id, window, 7)
        .unwrap();
    broker
        .registry
        .queue_present(transaction, owner, window, pixmap, 2, None, false)
        .unwrap();

    assert_eq!(
        broker.route_present_complete(transaction, 10, 20, XPresentCompletionMode::Flip),
        Ok(true),
    );
    assert!(
        matches!(
            watch_channels.protocol.recv().unwrap(),
            XClientEvent::PresentCompleteNotify { event_id: routed, .. } if routed == event_id,
        ),
        "the subscriber must receive the feedback it asked for",
    );
    assert!(
        owner_channels.protocol.try_recv().is_err(),
        "the presenting client did not subscribe and must not be sent an event",
    );
}

#[test]
fn a_present_from_a_client_that_did_not_create_the_window_is_admitted() {
    // The Chromium topology. The browser process creates the window; the GPU
    // process presents to it and subscribes for feedback. X permits this, and
    // Mesa blocks on the feedback -- requiring creator == presenter here
    // silently locked out every client that splits the two.
    let namespace = NamespaceId::from_raw(62);
    let owner = XServerFrontendClientId::from_raw(1);
    let presenter = XServerFrontendClientId::from_raw(2);
    let surface = SurfaceId::new(51, 1);
    let window = XResourceId::new(0x300030, 1);
    let event_id = XResourceId::new(0x400031, 1);
    let pixmap = XResourceId::new(0x400032, 1);
    let transaction = TransactionId::from_raw(221);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());
    let (_owner_registration, owner_channels) = broker.registry.register_client(owner).unwrap();
    let (_presenter_registration, presenter_channels) =
        broker.registry.register_client(presenter).unwrap();
    broker
        .registry
        .register_surface(owner, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .select_present_input(presenter, event_id, window, 7)
        .unwrap();

    broker
        .registry
        .queue_present(transaction, presenter, window, pixmap, 3, None, false)
        .expect("the window decides the surface; the presenter need not own it");

    assert_eq!(
        broker.route_present_complete(transaction, 11, 21, XPresentCompletionMode::Flip),
        Ok(true),
    );
    assert!(matches!(
        presenter_channels.protocol.recv().unwrap(),
        XClientEvent::PresentCompleteNotify { kind: 0, .. },
    ));
    assert_eq!(broker.route_present_idle(transaction), Ok(true));
    assert!(matches!(
        presenter_channels.protocol.recv().unwrap(),
        XClientEvent::PresentIdleNotify { .. },
    ));
    assert!(owner_channels.protocol.try_recv().is_err());
}

#[test]
fn a_present_to_a_window_without_a_route_names_the_window() {
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());
    let presenter = XServerFrontendClientId::from_raw(2);
    let (_registration, _channels) = broker.registry.register_client(presenter).unwrap();
    let window = XResourceId::new(0x300040, 1);

    let refused = broker.registry.queue_present(
        TransactionId::from_raw(231),
        presenter,
        window,
        XResourceId::new(0x400041, 1),
        1,
        None,
        false,
    );
    assert_eq!(
        refused,
        Err(XServerFrontendRouteError::UnknownPresentWindow { window }),
    );
}

#[test]
fn a_notify_msc_at_or_behind_the_clock_answers_immediately() {
    // Mesa blocks on this answer. Before any frame completes the clock reads
    // zero, which is exactly the case the vsync probe exercises: target zero.
    let namespace = NamespaceId::from_raw(63);
    let owner = XServerFrontendClientId::from_raw(1);
    let watcher = XServerFrontendClientId::from_raw(2);
    let surface = SurfaceId::new(52, 1);
    let window = XResourceId::new(0x300050, 1);
    let event_id = XResourceId::new(0x400051, 1);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());
    let (_owner_registration, _owner_channels) = broker.registry.register_client(owner).unwrap();
    let (_watch_registration, watch_channels) = broker.registry.register_client(watcher).unwrap();
    broker
        .registry
        .register_surface(owner, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .select_present_input(watcher, event_id, window, 7)
        .unwrap();

    for delivery in broker
        .registry
        .prepare_present_msc_notify(window, 9, 0)
        .unwrap()
    {
        broker
            .registry
            .route_protocol(delivery.recipient, delivery.event)
            .unwrap();
    }
    assert!(matches!(
        watch_channels.protocol.recv().unwrap(),
        XClientEvent::PresentCompleteNotify {
            kind: 1,
            serial: 9,
            msc: 0,
            ..
        },
    ));
}

#[test]
fn a_notify_msc_ahead_of_the_clock_waits_for_a_completion_to_ripen() {
    let namespace = NamespaceId::from_raw(64);
    let owner = XServerFrontendClientId::from_raw(1);
    let surface = SurfaceId::new(53, 1);
    let window = XResourceId::new(0x300060, 1);
    let event_id = XResourceId::new(0x300061, 1);
    let pixmap = XResourceId::new(0x300062, 1);
    let transaction = TransactionId::from_raw(241);
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(8).unwrap());
    let (_owner_registration, owner_channels) = broker.registry.register_client(owner).unwrap();
    broker
        .registry
        .register_surface(owner, namespace, surface, window)
        .unwrap();
    broker
        .registry
        .select_present_input(owner, event_id, window, 7)
        .unwrap();

    // Target 30 is ahead of a zero clock: nothing may arrive yet.
    assert!(
        broker
            .registry
            .prepare_present_msc_notify(window, 4, 30)
            .unwrap()
            .is_empty()
    );
    assert!(owner_channels.protocol.try_recv().is_err());

    // A completion at msc 42 advances the clock past the target and ripens it.
    broker
        .registry
        .queue_present(transaction, owner, window, pixmap, 5, None, false)
        .unwrap();
    assert_eq!(
        broker.route_present_complete(transaction, 100, 42, XPresentCompletionMode::Flip),
        Ok(true),
    );
    let mut kinds = Vec::new();
    while let Ok(event) = owner_channels.protocol.try_recv() {
        if let XClientEvent::PresentCompleteNotify { kind, serial, .. } = event {
            kinds.push((kind, serial));
        }
    }
    assert!(
        kinds.contains(&(1, 4)),
        "the ripened MSC notification must arrive; got {kinds:?}",
    );
    assert!(
        kinds.contains(&(0, 5)),
        "the flip completion must still arrive; got {kinds:?}",
    );
}

include!("tests/routing.rs");
include!("tests/private_runner.rs");
include!("tests/private_service.rs");
include!("tests/private_service_egress.rs");
include!("tests/private_destruction.rs");
include!("tests/private_destruction_deferral.rs");
include!("tests/private_worker_attachment.rs");
include!("tests/private_worker_attachment_exits.rs");
include!("tests/private_failed_retention.rs");
include!("tests/private_deferred_cleanup.rs");
include!("tests/private_deferred_cleanup_service.rs");

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/dispatch_ticket_failure.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/xi_fixed_point.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/xi_source_delivery.rs"
));

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/peer_write_failure.rs"
));

include!("tests/private_applied_state.rs");

include!("tests/private_applied_registry.rs");

include!("tests/private_applied_focus.rs");

include!("tests/private_native.rs");
include!("tests/private_xkb_selection.rs");
include!("tests/private_keyboard_preparation.rs");

include!("tests/ordered_codec.rs");
