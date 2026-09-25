// The socket server's core tests: the request runtime lock, startup waits,
// writer backpressure, listener identities and the rest that once sat
// inline in `x11_socket/tests.rs`; included from there (t026).

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
        selections.selected_pointer_target(top_level, XPointerSelection::Press, 0, 100, 200),
        Some(content_child)
    );
    assert_eq!(
        selections.selected_pointer_target(top_level, XPointerSelection::Motion, 0, 100, 200),
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

/// A drag is motion with a button down, and the core protocol reports it to a
/// window that selected ButtonMotion or the held button's own ButtonNMotion,
/// whether or not it asked for PointerMotion. xterm's text widget asks for
/// Button1Motion alone; delivering motion only to PointerMotion selectors
/// left it blind until the release, so a selection was highlighted only once
/// the button came up (t162).
#[test]
fn motion_with_a_button_down_reaches_a_window_selecting_only_button_motion() {
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let top_level = XResourceId::new(0x200001, 1);
    let button1_child = XResourceId::new(0x200002, 1);
    let any_button_child = XResourceId::new(0x200003, 1);
    let mut selections = XCoreEventSelectionState::default();
    let column = |y: i32| Rect {
        x: 0,
        y,
        width: 800,
        height: 300,
    };
    selections.register(top_level, root, column(0));
    selections.register(button1_child, top_level, column(0));
    selections.register(any_button_child, top_level, column(300));
    // Button1Motion only; ButtonMotion only. Neither selects PointerMotion.
    selections.update(button1_child, Some(1 << 8), None);
    selections.update(any_button_child, Some(1 << 13), None);
    for window in [top_level, button1_child, any_button_child] {
        selections.observe_mapped(window);
    }
    const BUTTON1_STATE: u16 = 0x100;
    const BUTTON2_STATE: u16 = 0x200;

    // No button down: neither window asked for plain motion.
    assert_eq!(
        selections.selected_pointer_target(top_level, XPointerSelection::Motion, 0, 100, 100),
        None
    );
    assert_eq!(
        selections.selected_pointer_target(top_level, XPointerSelection::Motion, 0, 100, 400),
        None
    );
    // Button 1 down: the drag reaches both.
    assert_eq!(
        selections.selected_pointer_target(top_level, XPointerSelection::Motion, BUTTON1_STATE, 100, 100),
        Some(button1_child)
    );
    assert_eq!(
        selections.selected_pointer_target(top_level, XPointerSelection::Motion, BUTTON1_STATE, 100, 400),
        Some(any_button_child)
    );
    // Button 2 down: only the window that asked for any button's motion.
    assert_eq!(
        selections.selected_pointer_target(top_level, XPointerSelection::Motion, BUTTON2_STATE, 100, 100),
        None
    );
    assert_eq!(
        selections.selected_pointer_target(top_level, XPointerSelection::Motion, BUTTON2_STATE, 100, 400),
        Some(any_button_child)
    );
    // Buttons themselves never consult the motion masks.
    assert_eq!(
        selections.selected_pointer_target(top_level, XPointerSelection::Press, BUTTON1_STATE, 100, 100),
        None
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
        selections.selected_pointer_target(top_level, XPointerSelection::Press, 0, 100, 200),
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
        selections.selected_pointer_target(top_level, XPointerSelection::Press, 0, 100, 200),
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
            grab_crossing: None,
            grab_target: None,
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
            grab_crossing: None,
            grab_target: None,
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
            None,
            None,
        )
    );
}
