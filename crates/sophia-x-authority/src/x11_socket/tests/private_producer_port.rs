// Controls for the producer port and the addressing it enforces: refusals
// before readiness and after closure, the bounded backlog, a vanished
// requester, a foreign owner's lease at issuance and at acceptance, a gone
// or never-admitted connection, and two connections or two origins that
// never cross-target. Harness in `private_producer_service.rs`.

/// STAGE-ONLY: a port and access with no service, so the port's own
/// standing transitions can be driven by the control.
#[test]
fn the_port_refuses_before_readiness_bounds_its_backlog_and_answers_every_waiter_when_closed() {
    let (mut port, access) = PrivateProducerAccess::for_service();
    let durable = PrivateSettlementOwner::default();
    let owner = service_owner(&durable, 4);
    // BEFORE READINESS: refused at the caller's side, nothing queued.
    assert_eq!(access.standing(), PrivatePortStanding::NotReady);
    assert_eq!(
        access.control_producer(&owner.lease()).err(),
        Some(PrivateProducerRefusal::NotReady)
    );
    assert_eq!(
        access
            .await_ready(Duration::from_millis(50))
            .err(),
        Some(PrivateProducerRefusal::ReadinessTimedOut)
    );
    // READY, WITH NOBODY SERVING: one more requester than the backlog holds.
    // Exactly one of them is refused Backlogged with nothing queued -- which
    // one is the scheduler's -- and the rest wait in the backlog until the
    // port closes, when every one of them is answered Unanswered.
    port.publish_ready();
    let access = Arc::new(access);
    let waiters: Vec<_> = (0..=PRODUCER_REQUEST_BACKLOG)
        .map(|_| {
            let access = Arc::clone(&access);
            let durable = durable.clone();
            std::thread::spawn(move || {
                let owner = service_owner(&durable, 4);
                access.control_producer(&owner.lease()).err()
            })
        })
        .collect();
    let backlogged = waited_for_value(|| {
        // The refused one returns at once; the others are parked.
        waiters
            .iter()
            .filter(|waiter| waiter.is_finished())
            .count()
            .eq(&1)
            .then_some(())
    });
    assert!(backlogged.is_some(), "exactly one requester was refused at the bound");
    port.close();
    assert_eq!(access.standing(), PrivatePortStanding::Ended);
    let mut outcomes: Vec<Option<PrivateProducerRefusal>> = waiters
        .into_iter()
        .map(|waiter| waiter.join().expect("a waiter returns"))
        .collect();
    outcomes.sort_by_key(|outcome| matches!(outcome, Some(PrivateProducerRefusal::Backlogged)));
    let mut expected = vec![Some(PrivateProducerRefusal::Unanswered); PRODUCER_REQUEST_BACKLOG];
    expected.push(Some(PrivateProducerRefusal::Backlogged));
    assert_eq!(outcomes, expected, "the backlog's worth answered Unanswered, one Backlogged");
    assert_eq!(
        access.control_producer(&owner.lease()).err(),
        Some(PrivateProducerRefusal::Ended)
    );
    // Closing again changes nothing; dropping the port is a close.
    port.close();
    drop(port);
    assert_eq!(access.standing(), PrivatePortStanding::Ended);
    let (port, access) = PrivateProducerAccess::for_service();
    drop(port);
    assert_eq!(access.standing(), PrivatePortStanding::Ended, "a dropped port has ended");
    assert_eq!(
        access.await_ready(Duration::from_secs(5)).err(),
        Some(PrivateProducerRefusal::Ended)
    );
    drop((owner, durable));
}

/// STAGE-ONLY: the port answered directly over a prepared runner fixture,
/// for a requester that disappeared before its reply and for the lease
/// identity check the loop makes.
#[test]
fn the_port_answers_a_vanished_requester_without_blocking_and_refuses_a_foreign_keeper() {
    let (mut runner, owner, registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let (mut port, access) = PrivateProducerAccess::for_service();
    port.publish_ready();
    // A requester that stopped waiting: its reply receiver is gone before
    // the loop answers. The answer is issued and dropped; nothing blocks.
    let (reply, gone) = sync_channel(1);
    drop(gone);
    access
        .requests
        .try_send(PrivateProducerRequest {
            keeper: owner.lease().keeper(),
            ask: PrivateProducerAsk::Control,
            reply,
        })
        .expect("queued");
    // A foreign keeper: another owner's lease identity, refused at the loop
    // without asking the runner.
    let other = PrivateSettlementOwner::default();
    let foreign = service_owner(&other, 4);
    let (reply, answered) = sync_channel(1);
    access
        .requests
        .try_send(PrivateProducerRequest {
            keeper: foreign.lease().keeper(),
            ask: PrivateProducerAsk::Ingress {
                client: XServerFrontendClientId::from_raw(9000),
                device: DeviceId::from_raw(1),
                // This control is about a foreign owner being refused before
                // anything is issued, so it names no admission and the
                // refusal it expects happens before one would be checked.
                expected: None,
            },
            reply,
        })
        .expect("queued");
    let lease = owner.lease();
    assert_eq!(port.answer(&mut runner, &lease), (1, 1));
    assert!(matches!(
        answered.try_recv(),
        Ok(Err(PrivateProducerRefusal::ForeignServiceOwner))
    ));
    // The runner exposed its control producer once for the vanished
    // requester and nothing for the foreign one: an ingress it did not
    // issue leaves the older route unengaged.
    assert!(!runner.frontend().ordered_runner, "no ingress was issued");
    drop(registration);
    drop((runner.shutdown(), owner));
}

#[test]
fn a_producer_asked_under_a_foreign_owners_lease_or_for_a_gone_connection_is_refused() {
    let (launched, socket_path) = launch_producing("producer-refusals", 9603, 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .expect("readiness");
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.registry);
    let client_id = custody.cleanup_record().client;
    // A FOREIGN OWNER'S LEASE at issuance: refused by the loop's keeper
    // check, the runner never asked.
    let other = PrivateSettlementOwner::default();
    let foreign_owner = service_owner(&other, 4);
    let foreign = foreign_owner.lease();
    assert_eq!(
        launched.access.control_producer(&foreign).err(),
        Some(PrivateProducerRefusal::ForeignServiceOwner)
    );
    assert_eq!(
        launched
            .access
            .ingress_for(&foreign, client_id, DeviceId::from_raw(1))
            .err(),
        Some(PrivateProducerRefusal::ForeignServiceOwner)
    );
    // A FOREIGN OWNER'S LEASE at acceptance: the producer issued under the
    // service's lease still checks the lease it is asked to submit under.
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).expect("issued");
    let refused = control
        .submit(
            &foreign,
            XAuthorityClientControlCommand {
                client: client_id,
                command: XAuthorityControlCommand::ClearFocus {
                    transaction: TransactionId::from_raw(96301),
                    surface: SurfaceId::new(1, 1),
                },
            },
        )
        .expect_err("a foreign lease cannot submit");
    assert_eq!(refused.0, AdmissionRefusal::ForeignServiceOwner);
    // A CONNECTION NEVER ADMITTED: its number is a stranger to the runner,
    // and the ingress the port would issue is for exactly the number asked.
    let stranger = launched
        .access
        .ingress_for(&lease, XServerFrontendClientId::from_raw(999), DeviceId::from_raw(1))
        .err();
    assert!(
        matches!(stranger, Some(PrivateProducerRefusal::Runner(PrivateServiceRefusal::Admission(_)))),
        "an ingress for a number never admitted is the runner's own admission refusal: {stranger:?}"
    );
    // A CONNECTION THAT IS GONE: its number is a stranger to the runner.
    drop(client);
    assert!(
        waited_for(|| launched.registry.occupancy.state_of(client_id).is_none()
            || custody.join().phase() == PrivateReapingPhase::Joined
            || custody.exit_sink().left()),
        "the connection ended"
    );
    let gone = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .err();
    assert!(
        matches!(gone, Some(PrivateProducerRefusal::Runner(PrivateServiceRefusal::Admission(_)))),
        "an ingress for a gone connection is the runner's own admission refusal: {gone:?}"
    );
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let outcome = produced_outcome(launched, "producer refusals");
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    let order = outcome.order.expect("the tally");
    assert_eq!(order.producers_issued, 1, "{order:?}");
    assert_eq!(order.producers_refused, 4, "{order:?}");
    let _ = std::fs::remove_file(&socket_path);
}

/// A connection with a mapped window selecting buttons and focus changes,
/// its surface learned, its worker attached: what every routed-input control
/// starts from.
fn admitted_connection(
    launched: &ProducingLaunch,
    socket_path: &std::path::Path,
    ordinal: u32,
) -> (UnixStream, SurfaceId, u16, Arc<PrivateEvidenceCustody>, u32) {
    let mut client = connect_private_client(socket_path);
    let window = handshake_ids(&mut client) | ordinal;
    let (surface, sequence) = selecting_window(
        &mut client,
        &launched.transactions,
        window,
        (1 << 2) | (1 << 3) | (1 << 21),
    );
    let custody = waited_for_value(|| {
        kept_custodies(&launched.registry).into_iter().find(|custody| {
            custody.attachment() == Some(PrivateAttachment::Started)
                && custody.cleanup_record().worker_readiness().is_some()
                && registry_window_of(&launched.registry, custody.cleanup_record().client) == Some(window)
        })
    })
    .expect("this connection's worker started");
    (client, surface, sequence, custody, window)
}

/// The window this connection's surface route names, from the registry.
fn registry_window_of(registry: &XServerFrontendRouteRegistry, client: XServerFrontendClientId) -> Option<u32> {
    registry.surfaces.lock().ok().and_then(|routes| {
        routes
            .values()
            .find(|route| route.client == client)
            .map(|route| u32::try_from(route.window.local.raw()).expect("a core window"))
    })
}

/// Focus applied on one connection, acknowledged and seen on its wire.
fn apply_focus(
    launched: &ProducingLaunch,
    control: &PrivateControlProducer,
    client: &mut UnixStream,
    client_id: XServerFrontendClientId,
    surface: SurfaceId,
    transaction: u64,
) -> (Option<XAuthorityControlOutcome>, Option<[u8; 32]>) {
    control
        .submit(
            &launched.owner.lease(),
            XAuthorityClientControlCommand {
                client: client_id,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(transaction),
                    surface,
                },
            },
        )
        .expect("the order accepts the control");
    (
        ack_for(&launched.acks, transaction).map(|ack| ack.acknowledgement.outcome),
        read_event(client, 5),
    )
}

#[test]
fn two_connections_on_one_service_each_receive_only_their_own_presses() {
    let (launched, socket_path) =
        launch_producing_with("producer-two-connections", 9604, 4, true, Arc::new(|_| {}));
    launched.access.await_ready(Duration::from_secs(15)).expect("readiness");
    let (mut first, surface_1, sequence_1, custody_1, window_1) =
        admitted_connection(&launched, &socket_path, 0x0e21);
    let (mut second, surface_2, sequence_2, custody_2, window_2) =
        admitted_connection(&launched, &socket_path, 0x0e31);
    assert_ne!(window_1 & 0xffff_f000, window_2 & 0xffff_f000, "each connection's own id range");
    let client_1 = custody_1.cleanup_record().client;
    let client_2 = custody_2.cleanup_record().client;
    assert_ne!(client_1, client_2);
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).expect("the control producer");
    // Focus to the second window: the first window's FocusIn never comes,
    // and the applied state names the second.
    let (focus_2, focus_in_2) = apply_focus(&launched, &control, &mut second, client_2, surface_2, 96401);
    let ingress_1 = launched
        .access
        .ingress_for(&lease, client_1, DeviceId::from_raw(1))
        .expect("an ingress for the first");
    let ingress_2 = launched
        .access
        .ingress_for(&lease, client_2, DeviceId::from_raw(1))
        .expect("an ingress for the second");
    // A PRESS AT THE SECOND'S SURFACE THROUGH THE SECOND'S INGRESS: exact
    // bytes on the second wire, nothing on the first.
    ingress_2
        .submit(&lease, button_to(surface_2, XAuthorityInputDeliveryId::from_raw(96420), 272, true))
        .expect("accepted");
    let on_second = read_event(&mut second, 5);
    let on_first = read_event(&mut first, 1);
    // The second's button comes up before the first presses: one seat, one
    // pointer history, and a button already held is not pressed again.
    ingress_2
        .submit(&lease, button_to(surface_2, XAuthorityInputDeliveryId::from_raw(96421), 272, false))
        .expect("accepted");
    let release_on_second = read_event(&mut second, 5);
    // THE FIRST NEEDS ITS OWN APPLIED STATE: a press at its surface before
    // its projection is published is refused, not routed elsewhere. Focus
    // moves to the first (the second sees FocusOut, the first FocusIn), then
    // a press at the first's surface reaches the first alone.
    let (focus_1, focus_in_1) = apply_focus(&launched, &control, &mut first, client_1, surface_1, 96402);
    let focus_out_2 = read_event(&mut second, 5);
    ingress_1
        .submit(&lease, button_to(surface_1, XAuthorityInputDeliveryId::from_raw(96410), 272, true))
        .expect("accepted");
    let on_first_now = read_event(&mut first, 5);
    let on_second_now = read_event(&mut second, 1);
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("listening");
    let registry = launched.registry.clone();
    let outcome = produced_outcome(launched, "two connections");
    let seen_1 = observe_worker(&custody_1, &registry);
    let seen_2 = observe_worker(&custody_2, &registry);
    assert_eq!(focus_2, Some(XAuthorityControlOutcome::Delivered));
    assert_eq!(focus_in_2, Some(expected_focus_in(sequence_2, window_2)));
    assert_eq!(
        on_second,
        Some(expected_button_event(true, sequence_2, window_2, 1)),
        "the second's own press on its own wire"
    );
    assert_eq!(on_first, None, "nothing of the second's on the first wire");
    assert_eq!(
        release_on_second,
        Some(expected_button_event(false, sequence_2, window_2, 1)),
        "the second's release on its own wire"
    );
    assert_eq!(focus_1, Some(XAuthorityControlOutcome::Delivered));
    assert_eq!(focus_in_1, Some(expected_focus_in_from_another_window(sequence_1, window_1)));
    assert_eq!(
        focus_out_2.map(|event| (event[0], u32::from_le_bytes([event[4], event[5], event[6], event[7]]))),
        Some((10, window_2)),
        "the second saw its FocusOut"
    );
    assert_eq!(
        on_first_now,
        Some(expected_button_event(true, sequence_1, window_1, 1)),
        "the first's own press on its own wire: {:?}",
        outcome.order
    );
    assert_eq!(on_second_now, None, "nothing of the first's on the second wire");
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    let order = outcome.order.expect("the tally");
    assert_eq!((order.taken, order.refused, order.routed, order.dispatched), (5, 0, 2, 3), "{order:?}");
    assert_eq!(order.producers_issued, 3);
    assert_collected_running(&seen_1, "first");
    assert_collected_running(&seen_2, "second");
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn two_origins_each_over_its_own_owner_issue_producers_only_for_their_own_connections() {
    let (origin_a, socket_a) = launch_producing("producer-origin-a", 9605, 4);
    let (origin_b, socket_b) = launch_producing("producer-origin-b", 9606, 4);
    origin_a.access.await_ready(Duration::from_secs(15)).expect("A ready");
    origin_b.access.await_ready(Duration::from_secs(15)).expect("B ready");
    let (mut client_a, surface_a, sequence_a, custody_a, window_a) =
        admitted_connection(&origin_a, &socket_a, 0x0e41);
    let (mut client_b, surface_b, sequence_b, custody_b, window_b) =
        admitted_connection(&origin_b, &socket_b, 0x0e51);
    let id_a = custody_a.cleanup_record().client;
    let id_b = custody_b.cleanup_record().client;
    // These fresh origins deliberately collide on the local client number.
    // Their different windows produce distinct surface IDs, so B's surface
    // is absent from A even though B's client number names A's own client.
    // Each port still requires its own owner's lease.
    assert_eq!(id_a, id_b, "the same local client number in two origins");
    assert_ne!(surface_a, surface_b, "distinct surfaces in the two origins");
    assert_ne!(window_a, window_b, "distinct wire targets despite local ID collisions");
    let owner_a = Arc::clone(&origin_a.owner);
    let owner_b = Arc::clone(&origin_b.owner);
    let lease_a = owner_a.lease();
    let lease_b = owner_b.lease();
    assert_eq!(
        origin_a.access.ingress_for(&lease_b, id_b, DeviceId::from_raw(1)).err(),
        Some(PrivateProducerRefusal::ForeignServiceOwner),
        "B's lease at A's port"
    );
    let crossed = origin_a.access.ingress_for(&lease_a, id_b, DeviceId::from_raw(1))
        .expect("B's local number names A's own connection at A's port");
    let control_a = origin_a.access.control_producer(&lease_a).expect("A's control");
    let control_b = origin_b.access.control_producer(&lease_b).expect("B's control");
    let (focus_a, focus_in_a) = apply_focus(&origin_a, &control_a, &mut client_a, id_a, surface_a, 96501);
    let (focus_b, focus_in_b) = apply_focus(&origin_b, &control_b, &mut client_b, id_b, surface_b, 96601);
    let ingress_a = origin_a.access.ingress_for(&lease_a, id_a, DeviceId::from_raw(1)).expect("A's ingress");
    let ingress_b = origin_b.access.ingress_for(&lease_b, id_b, DeviceId::from_raw(1)).expect("B's ingress");
    ingress_a
        .submit(&lease_a, button_to(surface_a, XAuthorityInputDeliveryId::from_raw(96510), 272, true))
        .expect("accepted by A");
    let on_a = read_event(&mut client_a, 5);
    let on_b_from_a = read_event(&mut client_b, 1);
    ingress_b
        .submit(&lease_b, button_to(surface_b, XAuthorityInputDeliveryId::from_raw(96610), 272, true))
        .expect("accepted by B");
    let on_b = read_event(&mut client_b, 5);
    let on_a_from_b = read_event(&mut client_a, 1);
    // Return each pointer to neutral before the crossed request. Repeating a
    // held press could join an existing hold without owing an event and make
    // silence look like cross-target refusal.
    ingress_a
        .submit(&lease_a, button_to(surface_a, XAuthorityInputDeliveryId::from_raw(96511), 272, false))
        .expect("A's release accepted");
    let release_a = read_event(&mut client_a, 5);
    ingress_b
        .submit(&lease_b, button_to(surface_b, XAuthorityInputDeliveryId::from_raw(96611), 272, false))
        .expect("B's release accepted");
    let release_b = read_event(&mut client_b, 5);
    let releases_answered = waited_for(|| {
        delivery_cell(&origin_a.registry, 96511).is_some_and(|cell| cell.answer().is_some())
            && delivery_cell(&origin_b.registry, 96611).is_some_and(|cell| cell.answer().is_some())
    });
    // Both services are still serving. A's ingress carries A's origin, so an
    // accepted press naming B's surface cannot resolve through B's registry.
    // Capture its actual admission and both live wires before either stop;
    // the returned tally below establishes its execution refusal.
    let crossed_submission = crossed
        .submit(&lease_a, button_to(surface_b, XAuthorityInputDeliveryId::from_raw(96520), 272, true))
        .map_err(|refusal| format!("{refusal:?}"));
    let crossed_cell = delivery_cell(&origin_a.registry, 96520).expect("A's original admission");
    let crossed_on_a = read_event(&mut client_a, 1);
    let crossed_on_b = read_event(&mut client_b, 1);
    let crossed_answer = delivery_cell(&origin_a.registry, 96520).and_then(|cell| cell.answer());
    let exact_crossed_cell = delivery_cell(&origin_a.registry, 96520)
        .is_some_and(|cell| Arc::ptr_eq(&cell, &crossed_cell));
    let absent_from_b = delivery_cell(&origin_b.registry, 96520).is_none();
    let ports_live = (origin_a.access.standing(), origin_b.access.standing());
    let frames_live = (custody_a.cleanup_record().destruction_standing(), custody_b.cleanup_record().destruction_standing());
    let writers_live = (control_a.routing.control_writer_present(id_a), control_b.routing.control_writer_present(id_b));
    origin_a.commands.send(XServerFrontendServiceCommand::StopAndDisconnect).expect("A listening");
    origin_b.commands.send(XServerFrontendServiceCommand::StopAndDisconnect).expect("B listening");
    let registry_a = origin_a.registry.clone();
    let registry_b = origin_b.registry.clone();
    let outcome_a = produced_outcome(origin_a, "origin A");
    let outcome_b = produced_outcome(origin_b, "origin B");
    let seen_a = observe_worker(&custody_a, &registry_a);
    let seen_b = observe_worker(&custody_b, &registry_b);
    assert_eq!(release_a, Some(expected_button_event(false, sequence_a, window_a, 1)));
    assert_eq!(release_b, Some(expected_button_event(false, sequence_b, window_b, 1)));
    assert!(releases_answered, "both original holds released and answered before crossing");
    assert!(crossed_submission.is_ok(), "crossed admission: {crossed_submission:?}");
    assert_eq!(crossed_on_a, None, "the foreign surface is not re-addressed to A's window");
    assert_eq!(crossed_on_b, None, "nothing reaches B's live wire");
    assert_eq!(crossed_answer, Some(XAuthorityClientInputDelivery {
        client: id_a,
        delivery: XAuthorityInputDeliveryId::from_raw(96520),
        outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
    }), "A publishes the actual common refusal only to its original admission");
    assert!(exact_crossed_cell && absent_from_b, "the original cell belongs only to A");
    assert_eq!(crossed_cell.answer(), crossed_answer, "collection preserves A's original answer");
    assert_eq!(ports_live, (PrivatePortStanding::Ready, PrivatePortStanding::Ready));
    assert_eq!(frames_live, (PrivateDestructionStanding::NotRequested, PrivateDestructionStanding::NotRequested));
    assert_eq!(writers_live, (true, true), "both wires still had their live connection writers");
    assert_eq!((focus_a, focus_b), (Some(XAuthorityControlOutcome::Delivered), Some(XAuthorityControlOutcome::Delivered)));
    assert_eq!(focus_in_a, Some(expected_focus_in(sequence_a, window_a)));
    assert_eq!(focus_in_b, Some(expected_focus_in(sequence_b, window_b)));
    assert_eq!(on_a, Some(expected_button_event(true, sequence_a, window_a, 1)));
    assert_eq!(on_b, Some(expected_button_event(true, sequence_b, window_b, 1)));
    assert_eq!((on_b_from_a, on_a_from_b), (None, None), "no cross-targeting");
    assert_eq!(outcome_a.ok, Some(true), "{:?}", outcome_a.error);
    assert_eq!(outcome_b.ok, Some(true), "{:?}", outcome_b.error);
    let order_a = outcome_a.order.expect("A's actual execution tally");
    assert_eq!((order_a.taken, order_a.refused, order_a.routed, order_a.dispatched), (4, 1, 1, 2), "{order_a:?}");
    assert_eq!(order_a.last_refusal, Some(PrivateExecutionRefusal::NotDecided(
        sophia_input_authority::RequestCompletion::Refused(sophia_input_authority::RegistrationError::RoutingUnavailable),
    )), "the crossed surface was refused during A's guarded resolution: {order_a:?}");
    assert_collected_running(&seen_a, "A");
    assert_collected_running(&seen_b, "B");
    let _ = std::fs::remove_file(&socket_a);
    let _ = std::fs::remove_file(&socket_b);
}
