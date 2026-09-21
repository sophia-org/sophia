#[test]
fn b_applied_focus() {
    let mut service =
        LifecycleService::launch_with_capacity("b-applied-focus", 11200, None, false, 2);
    service.start();
    let mut connection = BConnection::open(&service, 0x0d01);
    let ingress = connection.ingress(&service, 1);
    let control = service
        .access
        .control_producer(&service.owner.lease())
        .unwrap();
    let (pause, release) = Pause::pair();
    FOCUS_PAUSES
        .lock()
        .unwrap()
        .push((Arc::as_ptr(&service.registry.clients) as usize, pause));
    let (route_focus, allow_focus_route) = Pause::pair();
    let (try_key, allow_key) = Pause::pair();
    let (pending_done, pending_observed) = sync_channel(1);
    arm_runner(
        &service.registry,
        Box::new(move |runner, lease| {
            route_focus.wait();
            // Stay inside the already-entered real runner while the control
            // writer retains outer runtime. The ordinary outer service loop
            // also uses that runtime and cannot itself enter a second turn.
            runner.service_turn(lease).unwrap();
            try_key.wait();
            for _ in 0..100 {
                runner.execute_accounted_step().unwrap();
                if !runner.frontend().terminal.turn.is_empty() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            let Some(PrivateOrderedItem::Refused {
                refusal,
                custody,
                sequence,
                ..
            }) = runner.frontend().terminal.turn.last()
            else {
                panic!("the original pending-focus input must be refused");
            };
            // The instance's initial focus, the root, is published from
            // preparation; what is not applied is the focus this key was
            // sent for. The refusal names that: the key has no applied
            // focus window to go to, and the pending one is not consulted.
            assert!(matches!(
                refusal,
                PrivateExecutionRefusal::Native(private_native::Refusal::Resolution(
                    PrivateAppliedRefusal::FocusNotApplied
                ))
            ));
            let completion = custody.observe().unwrap();
            assert!(matches!(
                completion,
                Some(sophia_input_authority::RequestCompletion::Refused(_))
            ));
            let evidence = json!({"refusal":format!("{refusal:?}"),"completion":format!("{completion:?}"),"token":format!("{:?}",custody.token()),"sequence":format!("{sequence:?}")});
            for _ in 0..100 {
                runner.service_turn(lease).unwrap();
                let cell = delivery_cell(&runner.frontend().broker.registry, 112001).unwrap();
                if cell.answer().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            pending_done.send(evidence).unwrap();
        }),
    );
    allow_focus_route.entered();
    control
        .submit(
            &service.owner.lease(),
            XAuthorityClientControlCommand {
                client: connection.client(),
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(112000),
                    surface: connection.surface,
                },
            },
        )
        .unwrap();
    allow_focus_route.release();
    let focus_worker = release.entered();
    allow_key.entered();
    assert!(
        service
            .registry
            .private_applied
            .get()
            .unwrap()
            .publication
            .lock()
            .unwrap()
            .focus
            .is_none(),
        "the queued focus is not applied until its writer applies it"
    );
    ingress
        .submit(
            &service.owner.lease(),
            key_service_route(connection.surface, 112001, 30, true),
        )
        .unwrap();
    let refused = delivery_cell(&service.registry, 112001).unwrap();
    allow_key.release();
    let refused_source = pending_observed
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    assert!(waited_for(|| refused.answer().is_some()));
    assert_eq!(
        refused.answer().unwrap().outcome,
        XAuthorityInputDeliveryOutcome::RouteRejected
    );
    let pending = json!({"source":refused_source,"writer":format!("{focus_worker:?}"),"queued_focus":112000,"key_cell":Arc::as_ptr(&refused) as usize,"outcome":"RouteRejected","focus_applied":false});
    release.release();
    assert_eq!(
        ack_for(&service.acks, 112000)
            .unwrap()
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert_eq!(
        read_event(&mut connection.peer, 3),
        Some(expected_focus_in(connection.sequence, connection.window))
    );
    connection.pointer_pair(&service, &ingress, 112002);
    let mut frames = Vec::new();
    for (id, pressed) in [(112004, true), (112005, false)] {
        ingress
            .submit(
                &service.owner.lease(),
                key_service_route(connection.surface, id, 30, pressed),
            )
            .unwrap();
        let frame = read_event(&mut connection.peer, 3).unwrap();
        assert_eq!(
            frame,
            expected_key_service_event(connection.sequence, connection.window, 38, pressed, 0)
        );
        b_flushed(&service, id, connection.client());
        frames.push(frame);
    }
    b_empty_debt(&service);
    let histories = b_history_refusals(&service, &ingress, connection.surface, 112006);
    let actors = b_finish(service, &[Arc::clone(&connection.custody)]);
    emit_case(
        "B.applied_focus",
        &[
            ("pending_focus_refuses", pending),
            (
                "applied_focus_permits",
                json!({"window":connection.window,"frames":frames}),
            ),
            ("foreign_history_refuses", histories["foreign"].clone()),
            ("replacement_history_refuses", histories["factory"].clone()),
        ],
        &actors,
    );
}

#[test]
fn b_keyboard_history() {
    let mut service =
        LifecycleService::launch_with_capacity("b-keyboard-history", 11201, None, false, 2);
    service.start();
    let mut connection = BConnection::open(&service, 0x0d11);
    connection.focus(&service, 112010, crate::X_FOCUS_DETAIL_ANCESTOR);
    let ingress = connection.ingress(&service, 1);
    connection.pointer_pair(&service, &ingress, 112011);
    connection.select_xkb();
    let original = b_history(&service);
    assert_eq!(original.1, 0);
    let mut frames = Vec::new();
    let mut receipts = Vec::new();
    let mut histories = Vec::new();
    for (id, evdev, key, pressed, before, after, notify) in [
        (112013, 42, 50, true, 0, 1, true),
        (112014, 30, 38, true, 1, 1, false),
        (112015, 30, 38, false, 1, 1, false),
        (112016, 42, 50, false, 1, 0, true),
    ] {
        ingress
            .submit(
                &service.owner.lease(),
                key_service_route(connection.surface, id, evdev, pressed),
            )
            .unwrap();
        let key_frame = read_event(&mut connection.peer, 3).unwrap();
        assert_eq!(
            key_frame,
            expected_key_service_event(
                connection.sequence,
                connection.window,
                key,
                pressed,
                before
            )
        );
        frames.push(key_frame);
        if notify {
            let state = read_event(&mut connection.peer, 3).unwrap();
            assert_eq!(
                state,
                b_state_notify(connection.sequence, key, pressed, after)
            );
            frames.push(state);
        }
        receipts.push(b_flushed(&service, id, connection.client()));
        let history = b_history(&service);
        assert_eq!(history, (original.0, u16::from(after)));
        histories.push(history);
    }
    b_empty_debt(&service);
    let focus_window = connection.window;
    let tail = b_no_input_tail(&mut connection, focus_window);
    let refused = b_history_refusals(&service, &ingress, connection.surface, 112017);
    let actors = b_finish(service, &[Arc::clone(&connection.custody)]);
    emit_case(
        "B.keyboard_history",
        &[
            (
                "modifier_continuity_across_turns",
                json!({"original":original,"observed":histories,"receipts":receipts}),
            ),
            (
                "exact_key_and_state_notify_bytes",
                json!({"window":connection.window,"event_sequence":connection.sequence-1,"frames":frames,"final_barrier":tail}),
            ),
            ("recreated_history_refuses", refused["factory"].clone()),
            ("foreign_history_refuses", refused["foreign"].clone()),
        ],
        &actors,
    );
}

#[test]
fn b_shared_hold() {
    let mut service =
        LifecycleService::launch_with_capacity("b-shared-key-hold", 11202, None, false, 3);
    service.start();
    let mut first = BConnection::open(&service, 0x0d21);
    let mut second = BConnection::open(&service, 0x0d31);
    first.focus(&service, 112020, crate::X_FOCUS_DETAIL_ANCESTOR);
    let ingress = first.ingress(&service, 1);
    let joining = second.ingress(&service, 1);
    first.pointer_pair(&service, &ingress, 112021);
    ingress
        .submit(
            &service.owner.lease(),
            key_service_route(first.surface, 112023, 42, true),
        )
        .unwrap();
    assert_eq!(
        read_event(&mut first.peer, 3),
        Some(expected_key_service_event(
            first.sequence,
            first.window,
            50,
            true,
            0
        ))
    );
    b_flushed(&service, 112023, first.client());
    let duplicate = b_observed_input(
        &service,
        &ingress,
        key_service_route(first.surface, 112024, 42, true),
    );
    let joined = b_observed_input(
        &service,
        &joining,
        key_service_route(second.surface, 112025, 42, true),
    );
    let survivor = b_observed_input(
        &service,
        &ingress,
        key_service_route(first.surface, 112026, 42, false),
    );
    for observation in [&duplicate, &joined, &survivor] {
        assert_eq!(observation["keyboard_applied"], false);
        assert_eq!(observation["owes_event"], false);
        assert_eq!(observation["modifiers"], 1);
    }
    // Continuing A uses the held Shift; any duplicate key output would
    // appear before these independently expected frames.
    for (id, pressed) in [(112027, true), (112028, false)] {
        ingress
            .submit(
                &service.owner.lease(),
                key_service_route(first.surface, id, 30, pressed),
            )
            .unwrap();
        assert_eq!(
            read_event(&mut first.peer, 3),
            Some(expected_key_service_event(
                first.sequence,
                first.window,
                38,
                pressed,
                1
            ))
        );
        b_flushed(&service, id, first.client());
    }
    second.focus(&service, 112029, crate::X_FOCUS_DETAIL_NONLINEAR);
    assert_eq!(read_event(&mut first.peer, 3).unwrap()[0], 10);
    joining
        .submit(
            &service.owner.lease(),
            key_service_route(second.surface, 112030, 42, false),
        )
        .unwrap();
    let released = read_event(&mut first.peer, 3).unwrap();
    assert_eq!(
        released,
        expected_key_service_event(first.sequence, first.window, 50, false, 1)
    );
    let receipt = b_flushed(&service, 112030, first.client());
    assert_eq!(read_event(&mut second.peer, 1), None);
    b_empty_debt(&service);
    let tail = b_no_input_tail(&mut first, second.window);
    let actors = b_finish(
        service,
        &[Arc::clone(&first.custody), Arc::clone(&second.custody)],
    );
    emit_case(
        "B.shared_hold",
        &[
            (
                "duplicate_join_no_repeat",
                json!({"duplicate":duplicate,"joined":joined}),
            ),
            ("survivor_release_no_repeat", survivor),
            (
                "final_release_original_recipient",
                json!({"original":first.client().raw(),"new_focus":second.client().raw(),"frame":released,"receipt":receipt,"final_barrier":tail}),
            ),
        ],
        &actors,
    );
}
