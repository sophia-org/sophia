// Work an operation owns: a writer parked on control output, a router that
// must still stop, and an acknowledgement retried after the work began.
//
// Split by subject from the routing controls, which had grown to forty
// thousand lines in one file. Mounted into the same private test module the
// rest of them share, so nothing here changed scope or visibility.


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
        X11ClientOutput::shared(stream, 0),
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
        drain: None,
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
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, _channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
    let registry = private
        .broker
        .registry
        .control_completion()
        .expect("a private instance to install one");

    // Accepted while the client was being served, and still queued.
    private
        .control_producer()
        .submit(&owner_of_durable.lease(), configure(client, surface, 38001))
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
        private.route_pending(&owner_of_durable.lease()),
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
    recovery.register(client, None).expect("a fresh ledger");
    let (events, receiver) = channel();
    let (stream, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    // Control output is registered and nobody will clear it, so the writer
    // parks before touching the socket.
    let pending = Arc::new(AtomicUsize::new(1));
    let writer = spawn_x11_input_event_writer(
        X11InputWriterState {
            input_watermark: None,
            stream: X11ClientOutput::shared(stream, 0),
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
        origin: XAuthorityRoutedInputOrigin::Physical,
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
        drain: None,
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
    let owner_of_durable = service_owner(&durable, 16);
    let (private, _channels, registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, client, surface);
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
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, channels, _registration, _deliveries) =
        private_with_client(acknowledgements.clone(), &owner_of_durable, client, surface);
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
        .submit(&owner_of_durable.lease(), XAuthorityClientControlCommand {
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
        let outcome = private.route_pending(&owner_of_durable.lease());
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
        X11ClientOutput::shared(stream, 0),
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
        drain: None,
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
    let stream = X11ClientOutput::shared(std::os::unix::net::UnixStream::pair().unwrap().0, 0);
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
    let stream = X11ClientOutput::shared(std::os::unix::net::UnixStream::pair().unwrap().0, 0);
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
            drain: None,
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
    let owner_of_durable = service_owner(&durable, 16);
    let (mut private, held_channels, _registration, _deliveries) =
        private_with_client(acknowledgements, &owner_of_durable, focused, focused_surface);
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
        .submit(&owner_of_durable.lease(), XAuthorityClientControlCommand {
            client: claimant,
            command: XAuthorityControlCommand::FocusSurface {
                transaction: TransactionId::from_raw(46002),
                surface: claimant_surface,
            },
        })
        .expect("the shared admission to accept control");
    let ran = private.route_pending(&owner_of_durable.lease()).expect("a turn");
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
