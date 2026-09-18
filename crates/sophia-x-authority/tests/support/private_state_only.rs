fn state_only_key_route(surface: SurfaceId, serial: u64, keycode: u32) -> XAuthorityRoutedInput {
    let mut route = key_service_route(surface, serial, keycode, false);
    route.delivery = None;
    route.mode = XAuthorityRoutedInputMode::StateOnly;
    route
}

fn state_only_fixture(client: u64) -> PreparedOrderedFixture {
    let mut f = prepared_ordered_fixture(XServerFrontendClientId::from_raw(client));
    // Actual pointer source operations establish its query observation. The
    // active keyboard grab selects the fixture's real registered endpoint.
    attempt_release(&mut f, client * 100, 272);
    {
        let private = f.runner.frontend.as_mut().unwrap();
        assert_eq!(private.dispatch_one_press(), Some(true));
        assert_eq!(private.record_one_native(), Some(true));
        assert_eq!(private.attempt_one_delivery(), Some(true));
    }
    f.selections
        .lock()
        .unwrap()
        .update(f.window, Some(15), None);
    let mut grab = public_keyboard_grab(f.client);
    grab.window = f.window;
    grab.keyboard_mode = 1;
    grab.event_mask = 3;
    f.runner
        .frontend()
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .grab_keyboard(f.namespace, grab)
        .unwrap();
    f
}

fn state_only_execute(
    f: &mut PreparedOrderedFixture,
    secondary: Option<&PrivateIngress>,
    route: XAuthorityRoutedInput,
) -> PrivateOrderedRun {
    let lease = f.keeper.lease();
    let ingress = secondary.unwrap_or(&f.ingress);
    let sequence = ingress.submit(&lease, route).unwrap();
    assert!(matches!(f.runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Step { step: PrivateOrderedStep::Decided(actual), .. } if actual == sequence));
    let Some(PrivateOrderedItem::Ran {
        run, mut custody, ..
    }) = f.runner.frontend.as_mut().unwrap().terminal.turn.pop()
    else {
        panic!("the original accepted key request completes through common");
    };
    assert!(custody.observe().unwrap().is_some());
    assert!(
        custody.observe().is_err(),
        "a consumed original completion cannot be taken twice"
    );
    assert!(custody.finish_item());
    run
}

fn state_only_shift_state(f: &PreparedOrderedFixture) -> (crate::XkbPhysicalKeyState, u16) {
    let keyboard = &f.runner.keyboards.seats[&SeatId::from_raw(1)];
    (keyboard.physical_key_state(50), keyboard.modifier_mask())
}

#[test]
fn state_only_no_holder_and_survivor_do_not_repeat_xkb_or_create_emissions() {
    use sophia_input_authority::ReleaseOutcome;
    let mut f = state_only_fixture(9781);
    let surface = f.surface;
    let secondary = f
        .runner
        .ingress_for(&f.keeper.lease(), f.client, DeviceId::from_raw(2))
        .unwrap();
    let empty = state_only_execute(&mut f, None, state_only_key_route(surface, 997810, 42));
    assert_eq!(empty.release, Some(ReleaseOutcome::NotHeld));
    assert!(!empty.keyboard_applied && !empty.owes_event && empty.event.is_none());
    assert_eq!(
        state_only_shift_state(&f),
        (crate::XkbPhysicalKeyState::Released, 0)
    );

    let press = state_only_execute(&mut f, None, key_service_route(surface, 997811, 42, true));
    assert!(press.first_press && press.keyboard_applied);
    assert_eq!(
        f.runner.frontend.as_mut().unwrap().dispatch_one_press(),
        Some(true)
    );
    let joined = state_only_execute(
        &mut f,
        Some(&secondary),
        key_service_route(surface, 997812, 42, true),
    );
    assert!(!joined.first_press && !joined.keyboard_applied && !joined.owes_event);
    let survivor = state_only_execute(&mut f, None, state_only_key_route(surface, 997813, 42));
    assert_eq!(survivor.release, Some(ReleaseOutcome::SurvivorRemains));
    assert!(!survivor.keyboard_applied && !survivor.owes_event && survivor.event.is_none());
    assert_eq!(
        state_only_shift_state(&f),
        (crate::XkbPhysicalKeyState::Held, 1)
    );
    assert!(f.runner.frontend().terminal.pending_custody.is_none());

    let released = state_only_execute(
        &mut f,
        Some(&secondary),
        state_only_key_route(surface, 997814, 42),
    );
    assert!(matches!(
        released.release,
        Some(ReleaseOutcome::DeliverTo(_))
    ));
    assert!(released.keyboard_applied && !released.owes_event && released.event.is_none());
    assert_eq!(
        state_only_shift_state(&f),
        (crate::XkbPhysicalKeyState::Released, 0)
    );
    let private = f.runner.frontend.as_mut().unwrap();
    let release = private
        .terminal
        .settling
        .iter_mut()
        .find(|release| {
            release
                .native
                .as_ref()
                .is_some_and(|native| native.key().is_some())
        })
        .unwrap();
    assert_eq!(
        release.binding,
        PrivateReleaseBinding::RecipientTerminationRequired
    );
    assert!(release.delivery.is_none() && release.custody.completion.is_none());
    assert!(release.custody.attempt.is_none() && release.custody.pending.is_none());
    let held = release.native.as_mut().unwrap().key_mut().unwrap();
    assert_eq!(
        held.release_disposition(),
        private_native::KeyReleaseDisposition::RecipientTerminationRequired
    );
    assert!(held.release_xkb_applied() && held.take_release_emission().is_none());
    assert_eq!(held.proof().unwrap().incarnation(), release.incarnation);
    assert!(release.record_native_once());
    assert_eq!(
        release.press_custody.as_ref().unwrap().dispatch,
        PrivateDispatchPhase::Enqueued
    );
    assert!(release.native_recorded);
    assert!(
        !release.owes_delivery_attempt(),
        "suppressed release cannot spend a writer attempt even after native proof and press handover"
    );
    let original = release.incarnation;
    let debt = private
        .authority()
        .under_common(|authority| {
            let mut cursor = 0;
            (0..16)
                .filter_map(|_| authority.next_debt(&mut cursor))
                .find(|(incarnation, _)| *incarnation == original)
        })
        .unwrap()
        .unwrap();
    assert!(debt.1.native_reconciled && !debt.1.recipient_settled);
}

#[test]
fn state_only_frozen_release_retains_original_request_then_applies_once_after_exact_thaw() {
    let mut f = state_only_fixture(9782);
    let surface = f.surface;
    state_only_execute(&mut f, None, key_service_route(surface, 997820, 42, true));
    install_prepared_keyboard_freeze(&f);
    let first = f
        .ingress
        .submit(&f.keeper.lease(), state_only_key_route(surface, 997821, 42))
        .unwrap();
    f.runner.execute_accounted_step().unwrap();
    let original = {
        let row = &f.runner.frontend().terminal.frozen[0];
        assert_eq!(row.sequence, first);
        assert!(
            row.custody.input_completion().is_none() && row.custody.observe().unwrap().is_none()
        );
        row.custody.token()
    };
    assert_eq!(
        state_only_shift_state(&f),
        (crate::XkbPhysicalKeyState::Held, 1)
    );
    let later = f
        .runner
        .ingress_for(&f.keeper.lease(), f.client, DeviceId::from_raw(2))
        .unwrap()
        .submit(&f.keeper.lease(), state_only_key_route(surface, 997822, 30))
        .unwrap();
    f.runner.execute_accounted_step().unwrap();
    f.runner.execute_accounted_step().unwrap();
    assert_eq!(f.runner.frontend().terminal.frozen.len(), 2);
    assert!(f.runner.frontend().terminal.turn.is_empty());
    assert_eq!(
        state_only_shift_state(&f),
        (crate::XkbPhysicalKeyState::Held, 1)
    );
    f.runner
        .frontend()
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .allow_events(f.namespace, f.client.raw(), 3)
        .unwrap();
    for expected in [first, later] {
        assert!(matches!(f.runner.execute_accounted_step().unwrap(),
            PrivateAccountedStep::Step { step: PrivateOrderedStep::Resumed { sequence, deferred: false, .. }, .. } if sequence == expected));
    }
    assert_eq!(
        state_only_shift_state(&f),
        (crate::XkbPhysicalKeyState::Released, 0)
    );
    let PrivateOrderedItem::Ran { custody, run, .. } = &f.runner.frontend().terminal.turn[0] else {
        panic!("original release completed")
    };
    assert_eq!(custody.token(), original);
    assert!(custody.input_completion().is_none());
    assert!(run.keyboard_applied && !run.owes_event && run.event.is_none());
    assert!(f.runner.frontend().terminal.frozen.is_empty());
    assert!(matches!(
        f.runner.execute_accounted_step().unwrap(),
        PrivateAccountedStep::Step {
            step: PrivateOrderedStep::Idle,
            ..
        }
    ));
    assert_eq!(
        state_only_shift_state(&f),
        (crate::XkbPhysicalKeyState::Released, 0)
    );
}

#[test]
fn service_state_only_release_suppresses_wire_output_and_keeps_recipient_debt() {
    let (launched, socket) = launch_producing("producer-state-only-release", 9783, 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .unwrap();
    let mut client = connect_private_client(&socket);
    let window = handshake_ids(&mut client) | 0x0c31;
    let (surface, sequence) =
        selecting_window(&mut client, &launched.transactions, window, 15 | (1 << 21));
    let client_id = wait_attached(&launched.registry).cleanup_record().client;
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).unwrap();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .unwrap();
    assert_eq!(
        apply_focus(&launched, &control, &mut client, client_id, surface, 997830).0,
        Some(XAuthorityControlOutcome::Delivered)
    );
    key_service_pointer_pair(
        &launched,
        &ingress,
        &mut client,
        surface,
        sequence,
        window,
        997831,
    );
    ingress
        .submit(&lease, key_service_route(surface, 997833, 42, true))
        .unwrap();
    assert_eq!(
        read_event(&mut client, 5),
        Some(expected_key_service_event(sequence, window, 50, true, 0))
    );
    let accepted = ingress
        .submit(&lease, state_only_key_route(surface, 997834, 42))
        .unwrap();
    assert!(waited_for(|| launched
        .registry
        .input_authority
        .lock()
        .unwrap()
        .pointer_query_state(NamespaceId::from_raw(9783))
        .mask
        & 1
        == 0));
    let debt = waited_for_value(|| {
        launched
            .controller
            .under_common(|authority| {
                let mut cursor = 0;
                (0..16)
                    .filter_map(|_| authority.next_debt(&mut cursor))
                    .find(|(incarnation, bit)| {
                        incarnation.input == sophia_input_authority::Input::key(50).unwrap()
                            && bit.native_reconciled
                    })
            })
            .ok()
            .flatten()
    })
    .expect("the real native proof leaves the recipient half owed");
    assert!(!debt.1.recipient_settled);
    assert!(delivery_cell(&launched.registry, 997834).is_none());
    let following = control
        .submit(&lease, configure(client_id, surface, 997835))
        .unwrap();
    assert!(accepted < following);
    assert_eq!(
        ack_for(&launched.acks, 997835)
            .unwrap()
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered
    );
    // If StateOnly emitted a release it would be the next frame here. The
    // actual continuing history instead produces A with Shift already up.
    for (id, pressed) in [(997836, true), (997837, false)] {
        ingress
            .submit(&lease, key_service_route(surface, id, 30, pressed))
            .unwrap();
        assert_eq!(
            read_event(&mut client, 5),
            Some(expected_key_service_event(sequence, window, 38, pressed, 0))
        );
        let cell = delivery_cell(&launched.registry, id).unwrap();
        assert!(waited_for(|| cell.answer().is_some()));
        assert_eq!(
            cell.answer().unwrap().outcome,
            XAuthorityInputDeliveryOutcome::Flushed
        );
    }
    assert_eq!(
        read_event(&mut client, 1),
        None,
        "no delayed StateOnly key or StateNotify frame"
    );
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "state-only release");
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    assert!(outcome.execution_inventory_matches && outcome.execution_collected);
    let order = outcome.order.unwrap();
    assert_eq!((order.refused, order.dispatched), (0, 5), "{order:?}");
    let release = outcome
        .key_releases
        .iter()
        .find(|release| release.incarnation == debt.0)
        .unwrap();
    assert_eq!(release.proof, Some(release.incarnation));
    assert!(release.native_recorded);
    assert!(
        release.delivery.is_none() && release.answer.is_none() && release.receipt_seen.is_none()
    );
    let _ = std::fs::remove_file(socket);
}
