// Included in the private routing test module. All input crosses the actual
// producer port, prepared runner, source and registered connection writer.
// Native/rendering facts are supplied by the existing device-hidden fixture.

#[derive(Debug)]
struct KeyServiceReleaseObservation {
    reached: PrivateReachedResources,
    incarnation: sophia_input_authority::HoldIncarnation,
    proof: Option<sophia_input_authority::HoldIncarnation>,
    status: private_native::Status,
    native_recorded: bool,
    delivery: Option<XAuthorityInputDeliveryId>,
    answer: Option<XAuthorityClientInputDelivery>,
    receipt_seen: Option<XAuthorityInputDeliveryOutcome>,
}

fn observe_key_service_releases(
    terminal: Option<&PrivateTerminalInventory>,
) -> Vec<KeyServiceReleaseObservation> {
    terminal
        .into_iter()
        .flat_map(|terminal| terminal.settling.iter())
        .filter_map(|release| {
            let hold = release.native.as_ref()?.key()?;
            Some(KeyServiceReleaseObservation {
                reached: release.reached,
                incarnation: release.incarnation,
                proof: hold.proof().map(private_native::Proof::incarnation),
                status: hold.status(),
                native_recorded: release.native_recorded,
                delivery: release.delivery,
                answer: release.completion_answer(),
                receipt_seen: release.custody.outcome_seen,
            })
        })
        .collect()
}

fn key_service_route(
    surface: SurfaceId,
    id: u64,
    evdev: u32,
    pressed: bool,
) -> XAuthorityRoutedInput {
    let mut route = button_to(
        surface,
        XAuthorityInputDeliveryId::from_raw(id),
        272,
        pressed,
    );
    route.request.kind = InputEventKind::Key {
        keycode: evdev,
        pressed,
    };
    route
}

fn expected_key_service_event(
    sequence: u16,
    window: u32,
    detail: u8,
    pressed: bool,
    state: u16,
) -> [u8; 32] {
    // Independently encode the core wire fields, never the native encoder's
    // output: key type, X keycode, pre-transition mask, inherited window.
    let mut event = [0u8; 32];
    event[0] = if pressed { 2 } else { 3 };
    event[1] = detail;
    event[2..4].copy_from_slice(&sequence.to_le_bytes());
    event[4..8].copy_from_slice(&1u32.to_le_bytes());
    event[8..12].copy_from_slice(&X_SETUP_DEFAULT_ROOT.to_le_bytes());
    event[12..16].copy_from_slice(&window.to_le_bytes());
    event[28..30].copy_from_slice(&state.to_le_bytes());
    event[30] = 1;
    event
}

fn key_service_pointer_pair(
    launched: &ProducingLaunch,
    ingress: &PrivateIngress,
    client: &mut UnixStream,
    surface: SurfaceId,
    sequence: u16,
    window: u32,
    first_id: u64,
) {
    for (offset, pressed) in [(0, true), (1, false)] {
        ingress
            .submit(
                &launched.owner.lease(),
                button_to(
                    surface,
                    XAuthorityInputDeliveryId::from_raw(first_id + offset),
                    272,
                    pressed,
                ),
            )
            .expect("the real producer accepts the pointer input");
        assert_eq!(
            read_event(client, 5),
            Some(expected_button_event(pressed, sequence, window, 1))
        );
    }
}

fn wait_for_key_service_settlement(launched: &ProducingLaunch) {
    // A writer's Flushed cell can precede the runner's receipt visit. Wait on
    // the actual common ledger, read-only, before requesting service exit.
    // Empty debt is meaningful here only after every submitted release has
    // reached its writer; each caller establishes that first.
    assert!(
        waited_for(|| {
            launched
                .controller
                .under_common(|authority| authority.next_debt(&mut 0).is_none())
                .unwrap_or(false)
        }),
        "the continuing runner must settle both halves of every ended hold"
    );
}

#[test]
fn service_shift_a_chord_uses_one_history_and_settles_exact_native_and_writer_obligations() {
    let (launched, socket_path) = launch_producing("producer-key-chord", 9620, 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .unwrap();
    let mut client = connect_private_client(&socket_path);
    let window = handshake_ids(&mut client) | 0x0a01;
    let masks = 3 | (1 << 2) | (1 << 3) | (1 << 21);
    let (surface, _) = selecting_window(&mut client, &launched.transactions, window, masks);
    let (decoy_surface, _) =
        selecting_window(&mut client, &launched.transactions, window + 4, masks);
    let sequence = 8;
    assert_ne!(surface, decoy_surface);
    let client_id = wait_attached(&launched.registry).cleanup_record().client;
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).unwrap();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .unwrap();
    assert_eq!(
        apply_focus(&launched, &control, &mut client, client_id, surface, 98200),
        (
            Some(XAuthorityControlOutcome::Delivered),
            Some(expected_focus_in(sequence, window))
        )
    );
    // The key source's pointer observation comes only from these real pointer
    // source operations. No test writes a query scope or substitutes history.
    key_service_pointer_pair(
        &launched,
        &ingress,
        &mut client,
        surface,
        sequence,
        window,
        98201,
    );
    let mut cells = Vec::new();
    for (id, evdev, detail, pressed, state) in [
        (98210, 42, 50, true, 0),
        (98211, 30, 38, true, 1),
        (98212, 30, 38, false, 1),
        (98213, 42, 50, false, 1),
        (98214, 30, 38, true, 0),
        (98215, 30, 38, false, 0),
    ] {
        ingress
            .submit(&lease, key_service_route(decoy_surface, id, evdev, pressed))
            .unwrap();
        let event = read_event(&mut client, 5);
        assert_eq!(
            event,
            Some(expected_key_service_event(
                sequence, window, detail, pressed, state
            )),
            "exact wire event for delivery {id}, despite a different requested surface"
        );
        let cell = delivery_cell(&launched.registry, id).expect("the original completion cell");
        waited_for(|| cell.answer().is_some());
        assert_eq!(
            cell.answer()
                .map(|answer| (answer.delivery, answer.outcome)),
            Some((
                XAuthorityInputDeliveryId::from_raw(id),
                XAuthorityInputDeliveryOutcome::Flushed
            ))
        );
        cells.push(cell);
    }
    wait_for_key_service_settlement(&launched);
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "native key chord");
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    assert!(
        !outcome.unwound && outcome.retained_holds.is_empty() && outcome.store_holds.is_empty()
    );
    let order = outcome.order.unwrap();
    assert_eq!(
        (order.taken, order.refused, order.dispatched),
        (9, 0, 8),
        "{order:?}"
    );
    assert!(
        order.turns >= 7,
        "each event waited for its writer before the next submission"
    );
    assert!(outcome.key_releases.len() <= 3);
    assert_eq!(order.native_disposed + outcome.terminal.unwrap().1, 4,
        "all three key releases and the pointer release are either exactly disposed or still owned");
    for release in &outcome.key_releases {
        assert_eq!(release.reached.client(), client_id);
        assert_eq!(release.reached.window().local.raw(), u64::from(window));
        assert_eq!(release.reached.surface(), Some(surface));
        assert_eq!(release.status, private_native::Status::NativeReconciled);
        assert_eq!(
            release.proof,
            Some(release.incarnation),
            "source proof for this exact incarnation"
        );
        assert!(release.native_recorded, "{release:?}");
        assert_eq!(
            release.answer.map(|answer| answer.delivery),
            release.delivery
        );
        assert_eq!(
            release.answer.map(|answer| answer.outcome),
            Some(XAuthorityInputDeliveryOutcome::Flushed)
        );
        assert_eq!(
            release.receipt_seen,
            Some(XAuthorityInputDeliveryOutcome::Flushed)
        );
    }
    assert_eq!(
        cells.len(),
        6,
        "all original admission cells remained observable"
    );
    let _ = std::fs::remove_file(socket_path);
}

#[test]
fn service_state_only_press_is_refused_before_native_application() {
    assert_unsupported_key_service_mode(
        XAuthorityRoutedInputMode::StateOnly,
        PrivateExecutionRefusal::StateOnlyUnsupported,
        "producer-key-state-only",
    );
}

#[test]
fn service_repeat_key_is_explicitly_refused_before_native_application() {
    assert_unsupported_key_service_mode(
        XAuthorityRoutedInputMode::Repeat,
        PrivateExecutionRefusal::RepeatUnsupported,
        "producer-key-repeat",
    );
}

fn assert_unsupported_key_service_mode(
    mode: XAuthorityRoutedInputMode,
    expected: PrivateExecutionRefusal,
    tag: &str,
) {
    let (launched, socket_path) = launch_producing(tag, 9621, 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .unwrap();
    let mut client = connect_private_client(&socket_path);
    let window = handshake_ids(&mut client) | 0x0a21;
    let (surface, _) = selecting_window(&mut client, &launched.transactions, window, 3);
    let client_id = wait_attached(&launched.registry).cleanup_record().client;
    let lease = launched.owner.lease();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .unwrap();
    let mut route = key_service_route(surface, 98310, 42, true);
    route.mode = mode;
    ingress.submit(&lease, route).unwrap();
    assert_eq!(read_event(&mut client, 2), None);
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "unsupported key mode");
    let order = outcome.order.unwrap();
    assert_eq!((order.taken, order.refused, order.dispatched), (1, 1, 0));
    assert_eq!(order.last_refusal, Some(expected));
    assert!(outcome.retained_holds.is_empty() && outcome.store_holds.is_empty());
    assert!(outcome.key_releases.is_empty());
    let _ = std::fs::remove_file(socket_path);
}

#[test]
fn service_exit_with_shift_held_retains_an_unresolved_native_obligation() {
    let (launched, socket_path) = launch_producing("producer-key-held-exit", 9622, 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .unwrap();
    let mut client = connect_private_client(&socket_path);
    let window = handshake_ids(&mut client) | 0x0a41;
    let (surface, sequence) = selecting_window(
        &mut client,
        &launched.transactions,
        window,
        3 | (1 << 2) | (1 << 3) | (1 << 21),
    );
    let client_id = wait_attached(&launched.registry).cleanup_record().client;
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).unwrap();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .unwrap();
    assert_eq!(
        apply_focus(&launched, &control, &mut client, client_id, surface, 98400).0,
        Some(XAuthorityControlOutcome::Delivered)
    );
    key_service_pointer_pair(
        &launched,
        &ingress,
        &mut client,
        surface,
        sequence,
        window,
        98401,
    );
    ingress
        .submit(&lease, key_service_route(surface, 98410, 42, true))
        .unwrap();
    assert_eq!(
        read_event(&mut client, 5),
        Some(expected_key_service_event(sequence, window, 50, true, 0))
    );
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "held key exit");
    assert_eq!(
        outcome.retained_holds,
        vec![(client_id, u64::from(window), false)]
    );
    assert!(
        outcome.key_releases.is_empty(),
        "exit minted no cleanup proof or key release"
    );
    assert_eq!(outcome.terminal.map(|counts| counts.0), Some(1));
    let _ = std::fs::remove_file(socket_path);
}

#[test]
fn service_key_join_survivor_and_focus_move_keep_the_original_history_and_endpoint() {
    let (launched, socket_path) =
        launch_producing_with("producer-key-inherited", 9623, 4, true, Arc::new(|_| {}));
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .unwrap();
    let mut connections = Vec::new();
    for ordinal in [0x0a61, 0x0a71] {
        let mut client = connect_private_client(&socket_path);
        let window = handshake_ids(&mut client) | ordinal;
        let (surface, sequence) = selecting_window(
            &mut client,
            &launched.transactions,
            window,
            3 | (1 << 2) | (1 << 3) | (1 << 21),
        );
        let client_id = waited_for_value(|| {
            kept_custodies(&launched.registry)
                .into_iter()
                .find_map(|custody| {
                    let id = custody.cleanup_record().client;
                    (custody.attachment() == Some(PrivateAttachment::Started)
                        && registry_window_of(&launched.registry, id) == Some(window))
                    .then_some(id)
                })
        })
        .unwrap();
        connections.push((client, surface, sequence, window, client_id));
    }
    let (mut second, surface_2, sequence_2, window_2, client_2) = connections.pop().unwrap();
    let (mut first, surface_1, sequence_1, window_1, client_1) = connections.pop().unwrap();
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).unwrap();
    let ingress_1 = launched
        .access
        .ingress_for(&lease, client_1, DeviceId::from_raw(1))
        .unwrap();
    let ingress_2 = launched
        .access
        .ingress_for(&lease, client_2, DeviceId::from_raw(1))
        .unwrap();
    assert_eq!(
        apply_focus(&launched, &control, &mut first, client_1, surface_1, 98500).0,
        Some(XAuthorityControlOutcome::Delivered)
    );
    key_service_pointer_pair(
        &launched, &ingress_1, &mut first, surface_1, sequence_1, window_1, 98501,
    );
    ingress_1
        .submit(&lease, key_service_route(surface_1, 98510, 42, true))
        .unwrap();
    assert_eq!(
        read_event(&mut first, 5),
        Some(expected_key_service_event(
            sequence_1, window_1, 50, true, 0
        ))
    );
    // A second actual producer joins the held key; the first producer's
    // release leaves that survivor. Neither is a second physical XKB edge.
    ingress_2
        .submit(&lease, key_service_route(surface_2, 98511, 42, true))
        .unwrap();
    ingress_1
        .submit(&lease, key_service_route(surface_1, 98512, 42, false))
        .unwrap();
    assert_eq!(read_event(&mut first, 1), None);
    assert_eq!(read_event(&mut second, 1), None);
    for (id, pressed) in [(98513, true), (98514, false)] {
        ingress_1
            .submit(&lease, key_service_route(surface_1, id, 30, pressed))
            .unwrap();
        assert_eq!(
            read_event(&mut first, 5),
            Some(expected_key_service_event(
                sequence_1, window_1, 38, pressed, 1
            ))
        );
    }
    assert_eq!(
        apply_focus(&launched, &control, &mut second, client_2, surface_2, 98520),
        (
            Some(XAuthorityControlOutcome::Delivered),
            Some(expected_focus_in_from_another_window(sequence_2, window_2))
        )
    );
    assert_eq!(read_event(&mut first, 5).map(|event| event[0]), Some(10));
    ingress_2
        .submit(&lease, key_service_route(surface_2, 98521, 42, false))
        .unwrap();
    assert_eq!(
        read_event(&mut first, 5),
        Some(expected_key_service_event(
            sequence_1, window_1, 50, false, 1
        ))
    );
    assert_eq!(
        read_event(&mut second, 1),
        None,
        "current focus receives no inherited key release"
    );
    let cell = delivery_cell(&launched.registry, 98521).unwrap();
    waited_for(|| cell.answer().is_some());
    assert_eq!(
        cell.answer().map(|answer| (answer.client, answer.outcome)),
        Some((client_1, XAuthorityInputDeliveryOutcome::Flushed))
    );
    wait_for_key_service_settlement(&launched);
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "joined key and focus move");
    let order = outcome.order.unwrap();
    assert_eq!((order.refused, order.dispatched), (0, 6), "{order:?}");
    assert!(outcome.key_releases.len() <= 2);
    assert_eq!(order.native_disposed + outcome.terminal.unwrap().1, 3,
        "both key releases and the pointer release have an exact disposition");
    for release in outcome.key_releases {
        assert_eq!(release.reached.client(), client_1);
        assert_eq!(release.reached.surface(), Some(surface_1));
        assert_eq!(release.proof, Some(release.incarnation));
        assert!(release.native_recorded);
        assert_eq!(
            release.receipt_seen,
            Some(XAuthorityInputDeliveryOutcome::Flushed)
        );
    }
    let _ = std::fs::remove_file(socket_path);
}
