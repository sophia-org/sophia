// Actual private producer/runner/writer controls. Native/render facts come
// from the device-hidden authority fixture; wire expectations are independent.

fn expected_transient_event(
    kind: u8,
    detail: u8,
    sequence: u16,
    window: u32,
    state: u16,
    time: u32,
) -> [u8; 32] {
    let mut event = [0u8; 32];
    event[0] = kind;
    event[1] = detail;
    event[2..4].copy_from_slice(&sequence.to_le_bytes());
    event[4..8].copy_from_slice(&time.to_le_bytes());
    event[8..12].copy_from_slice(&X_SETUP_DEFAULT_ROOT.to_le_bytes());
    event[12..16].copy_from_slice(&window.to_le_bytes());
    for (offset, coordinate) in [(20, 2i16), (22, 3), (24, 2), (26, 3)] {
        event[offset..offset + 2].copy_from_slice(&coordinate.to_le_bytes());
    }
    event[28..30].copy_from_slice(&state.to_le_bytes());
    event[30] = 1;
    event
}

#[test]
fn service_motion_and_axis_use_the_original_request_and_reclaim_exact_writer_receipts() {
    let (launched, socket) = launch_producing("producer-transients", 9651, 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .unwrap();
    let mut client = connect_private_client(&socket);
    let window = handshake_ids(&mut client) | 0x0b01;
    let (surface, sequence) = selecting_window(
        &mut client,
        &launched.transactions,
        window,
        (1 << 2) | (1 << 3) | (1 << 6) | (1 << 21),
    );
    let client_id = wait_attached(&launched.registry).cleanup_record().client;
    let owner = Arc::clone(&launched.owner);
    let lease = owner.lease();
    let control = launched.access.control_producer(&lease).unwrap();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .unwrap();
    let focus = apply_focus(&launched, &control, &mut client, client_id, surface, 98500);
    let mut seen = Vec::new();
    // The instance reserves twice its four input slots plus the cleanup
    // reserve. Exceed that actual bound, not just its four wire queue slots.
    const TRANSIENTS: u64 = 32;
    assert!(TRANSIENTS as usize > 2 * 4 + PRIVATE_CLEANUP_RESERVE);
    for offset in 0..TRANSIENTS {
        let id = 98501 + offset;
        let axis = offset % 2 == 1;
        let mut route = motion_to(surface, XAuthorityInputDeliveryId::from_raw(id));
        route.request.global_position = Point { x: 2.0, y: 3.0 };
        route.request.local_position = route.request.global_position;
        route.request.time_msec = 30 + offset;
        if axis {
            route.request.kind = InputEventKind::PointerAxis {
                horizontal_v120: 0,
                vertical_v120: 120,
            };
        }
        let submitted = ingress
            .submit(&lease, route)
            .map(|_| ())
            .map_err(|r| format!("{r:?}"));
        let first = read_event(&mut client, 3);
        let second = if axis {
            read_event(&mut client, 3)
        } else {
            None
        };
        let cell = delivery_cell(&launched.registry, id);
        if let Some(cell) = &cell {
            waited_for(|| cell.answer().is_some());
        }
        let answer = cell.and_then(|cell| cell.answer());
        seen.push((id, axis, submitted, first, second, answer));
        if first.is_none() {
            break;
        }
    }
    // Read a copy; probing the next accumulator value cannot change the
    // original mapper the runner owns.
    let mapper = launched
        .registry
        .pointer_state
        .lock()
        .unwrap()
        .get(&(NamespaceId::from_raw(9651), SeatId::from_raw(1)))
        .copied();
    let query = launched
        .registry
        .input_authority
        .lock()
        .unwrap()
        .pointer_query_state(NamespaceId::from_raw(9651));
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .unwrap();
    let outcome = produced_outcome(launched, "motion and axis");
    assert_eq!(
        focus,
        (
            Some(XAuthorityControlOutcome::Delivered),
            Some(expected_focus_in(sequence, window))
        )
    );
    assert_eq!(
        seen.len(),
        TRANSIENTS as usize,
        "order {:?}; error {:?}; seen {seen:?}",
        outcome.order,
        outcome.error
    );
    for (offset, (id, axis, submitted, first, second, answer)) in seen.iter().enumerate() {
        assert_eq!(*submitted, Ok(()));
        let time = 30 + offset as u32;
        assert_eq!(
            *first,
            Some(expected_transient_event(
                if *axis { 4 } else { 6 },
                if *axis { 5 } else { 0 },
                sequence,
                window,
                0,
                time
            ))
        );
        assert_eq!(
            *second,
            axis.then(|| expected_transient_event(5, 5, sequence, window, 1 << 12, time))
        );
        assert_eq!(
            answer.map(|a| (a.delivery, a.outcome)),
            Some((
                XAuthorityInputDeliveryId::from_raw(*id),
                XAuthorityInputDeliveryOutcome::Flushed
            ))
        );
    }
    assert_eq!(
        mapper
            .unwrap()
            .map_axis(0, 120)
            .unwrap()
            .vertical_position_v120,
        Some((TRANSIENTS as i32 / 2 + 1) * 120)
    );
    assert_eq!(
        query.mask, 0,
        "a synthetic wheel pair never leaves a physical button held"
    );
    assert_eq!(query.vertical_scroll_v120, TRANSIENTS as i32 / 2 * 120);
    assert_eq!(outcome.ok, Some(true));
    assert!(outcome.retained_holds.is_empty() && outcome.store_holds.is_empty());
    let order = outcome.order.unwrap();
    assert_eq!(
        (order.taken, order.refused, order.dispatched),
        (TRANSIENTS as usize + 1, 0, TRANSIENTS as usize)
    );
}

fn execute_transient_fixture(
    f: &mut PreparedOrderedFixture,
    id: u64,
    kind: InputEventKind,
) -> sophia_input_authority::RequestToken {
    let mut route = motion_to(f.surface, XAuthorityInputDeliveryId::from_raw(id));
    route.request.kind = kind;
    f.ingress.submit(&f.keeper.lease(), route).unwrap();
    let PrivatePreparedRunner {
        frontend,
        keyboards,
        watch,
        ..
    } = &mut f.runner;
    let private = frontend.as_mut().unwrap();
    assert!(matches!(
        private
            .step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap())
            .unwrap(),
        PrivateOrderedStep::Decided(_)
    ));
    let Some(PrivateOrderedItem::Ran { custody, run, .. }) = private.terminal.turn.pop() else {
        panic!("actual transient execution must run");
    };
    assert!(run.owes_event && !run.first_press && run.release.is_none());
    let token = custody.token;
    assert!(custody.observe().unwrap().is_some());
    token
}

#[test]
fn transient_full_queue_preserves_exact_pair_token_and_accumulator_without_reselection() {
    let mut f = prepared_ordered_fixture(XServerFrontendClientId(9652));
    // Four original source events occupy the four actual ordered slots.
    // No fake Full return and no supplied filler capsule.
    for (id, button) in [(98600, 272), (98602, 273)] {
        attempt_release(&mut f, id, button);
        let private = f.runner.frontend.as_mut().unwrap();
        assert_eq!(private.dispatch_one_press(), Some(true));
        assert_eq!(private.record_one_native(), Some(true));
        assert_eq!(private.attempt_one_delivery(), Some(true));
    }
    let token = execute_transient_fixture(
        &mut f,
        98604,
        InputEventKind::PointerAxis {
            horizontal_v120: 0,
            vertical_v120: -120,
        },
    );
    let registry = f.runner.frontend.as_ref().unwrap().broker.registry.clone();
    let mapper = *registry
        .pointer_state
        .lock()
        .unwrap()
        .get(&(f.namespace, SeatId::from_raw(1)))
        .unwrap();
    let private = f.runner.frontend.as_mut().unwrap();
    assert_eq!(private.dispatch_one_press(), Some(false));
    let record = &private.terminal.transients.records[0];
    let order = record.custody.order;
    let completion = record.custody.completion.as_ref().unwrap().clone();
    let Some(PrivatePendingDelivery::Capsule(capsule)) = &record.custody.pending else {
        panic!("Full returned the original complete pair");
    };
    assert_eq!(
        capsule.incarnation(),
        None,
        "motion/axis never invent a common hold"
    );
    let private_native::PrivateEmissionIdentity::Request {
        token: actual,
        context,
    } = capsule.emission().identity()
    else {
        panic!("original request provenance");
    };
    assert_eq!(actual, token);
    assert_eq!(
        context.connection,
        f.ingress.role.as_ref().unwrap().connection()
    );
    let frames = order_pass_frames(capsule);
    assert_eq!(frames.len(), 2);
    assert_eq!(
        (frames[0][0], frames[0][1], frames[1][0], frames[1][1]),
        (4, 4, 5, 4)
    );
    // A fresh resolve now has no selected target and different geometry.
    f.selections.lock().unwrap().update(f.window, Some(0), None);
    f.selections
        .lock()
        .unwrap()
        .configure_geometry(f.window, Some(77), Some(88), None, None);
    for _ in 0..8 {
        assert_eq!(private.dispatch_one_press(), Some(false));
    }
    let record = &private.terminal.transients.records[0];
    assert_eq!(record.custody.order, order);
    assert_eq!(record.custody.dispatch, PrivateDispatchPhase::Pending);
    assert!(Arc::ptr_eq(
        record.custody.completion.as_ref().unwrap(),
        &completion
    ));
    assert!(completion.answer().is_none());
    assert_eq!(
        *registry
            .pointer_state
            .lock()
            .unwrap()
            .get(&(f.namespace, SeatId::from_raw(1)))
            .unwrap(),
        mapper
    );
    assert_eq!(
        f.channels.ordered.try_recv().unwrap().delivery(),
        XAuthorityInputDeliveryId::from_raw(98600)
    );
    assert_eq!(private.dispatch_one_press(), Some(true));
    for expected in [98601, 98602, 98603] {
        assert_eq!(
            f.channels.ordered.try_recv().unwrap().delivery(),
            XAuthorityInputDeliveryId::from_raw(expected)
        );
    }
    let capsule = f.channels.ordered.try_recv().unwrap();
    assert_eq!(
        capsule.delivery(),
        XAuthorityInputDeliveryId::from_raw(98604)
    );
    assert_eq!(order_pass_frames(&capsule), frames);
    assert!(Arc::ptr_eq(
        &capsule.finalizer().unwrap().completion,
        &completion
    ));
    assert!(matches!(
        f.channels.ordered.try_recv(),
        Err(TryRecvError::Empty)
    ));
    assert_eq!(private.terminal.transients.observe_one(), Some(false));
    assert_eq!(
        private.terminal.transients.records.len(),
        1,
        "enqueue is not a writer receipt"
    );
    assert!(completion.answer().is_none());
}

#[test]
fn transient_axis_smooth_and_emulated_selections_are_independent_and_accumulate_once() {
    for (offset, xi_mask, core_mask, expected) in [
        (0, (1 << 4) | (1 << 5) | (1 << 6), 0, vec![6, 4, 5]),
        (1, 1 << 6, 0, vec![6]),
        (2, (1 << 4) | (1 << 5), 0, vec![4, 5]),
        (3, 1 << 5, 0, vec![5]),
        (4, 0, 1 << 3, vec![5]),
    ] {
        let mut f = prepared_ordered_fixture(XServerFrontendClientId(9660 + offset));
        f.selections
            .lock()
            .unwrap()
            .update(f.window, Some(core_mask), None);
        let registry = f.runner.frontend.as_ref().unwrap().broker.registry.clone();
        registry.input_authority.lock().unwrap().select_xi_events(
            f.namespace,
            f.client.raw(),
            f.window,
            &[(2, vec![xi_mask])],
        );
        execute_transient_fixture(
            &mut f,
            98700 + offset,
            InputEventKind::PointerAxis {
                horizontal_v120: 0,
                vertical_v120: 120,
            },
        );
        assert_eq!(
            f.runner.frontend.as_mut().unwrap().dispatch_one_press(),
            Some(true)
        );
        let capsule = f.channels.ordered.try_recv().unwrap();
        let frames = order_pass_frames(&capsule);
        let kinds: Vec<u16> = frames
            .iter()
            .map(|frame| {
                if frame[0] == 35 {
                    u16::from_le_bytes([frame[8], frame[9]])
                } else {
                    u16::from(frame[0])
                }
            })
            .collect();
        assert_eq!(
            kinds, expected,
            "only selected smooth/emulated halves, case {offset}"
        );
        let query = registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_query_state(f.namespace);
        assert_eq!((query.mask, query.vertical_scroll_v120), (0, 120));
        assert!(
            capsule.finalizer().unwrap().completion.answer().is_none(),
            "encoding is not delivery"
        );
    }
}
