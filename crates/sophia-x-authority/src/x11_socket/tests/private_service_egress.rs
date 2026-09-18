// Controls for the private routed service's egress custody: what happens to
// an observed raster batch the service was still waiting to send when it
// stopped, erred, unwound or was cancelled, and how the store bounds what it
// keeps. The harness, admission and lifetime controls live in
// private_service.rs; this file shares them.

/// Draw enough that a raster requirement for the window is SATISFIED with an
/// observed batch, and return that surface. Drains the transport only up to
/// the batch that names it.
fn draw_and_learn_surface(
    client: &mut UnixStream,
    transactions: &Receiver<XAuthorityObservedTransactionBatch>,
) -> SurfaceId {
    let window: u32 = 0x0020_0d01;
    let gc: u32 = 0x0020_0d02;
    create_window(client, 0);
    create_gc(client, gc, window);
    image_text8(client, window, gc, b"AaZz");
    let drawn = waited_for_value(|| {
        transactions
            .recv_timeout(Duration::from_millis(50))
            .ok()
            .filter(|batch| batch.cpu_buffer_updates.len() == 1 && batch.transactions.len() == 1)
    })
    .expect("the draw is observed as one CPU buffer update");
    // Two more draws: one lands and fills the transport, one parks the worker.
    image_text8(client, window, gc, b"AaZz");
    image_text8(client, window, gc, b"AaZz");
    drawn.transactions[0].surface
}

fn raster_requirement_for(surface: SurfaceId) -> sophia_protocol::SurfaceRasterRequirements {
    sophia_protocol::SurfaceRasterRequirements {
        surface,
        committed_content_generation: 2,
        requirement_generation: 1,
        logical_extent: Size {
            width: 8,
            height: 8,
        },
        classes: vec![sophia_protocol::SurfaceRasterClass {
            density_millis: 1000,
            transform: sophia_protocol::SurfaceRasterTransform::Normal,
        }],
    }
}

#[test]
fn an_unwind_inside_the_private_service_keeps_the_unsent_raster_envelope() {
    // THE EXACT WORK, THROUGH THE ORIGINAL OWNER. The service thread submits
    // an observed raster batch behind a parked worker on a full transport; its
    // wait reports to the observer, which unwinds on that thread. The batch was
    // never accepted, and after the unwind the store the owner is established
    // over holds exactly that envelope -- the surface it names -- unsent.
    let namespace = NamespaceId::from_raw(9304);
    let socket_path = private_service_socket("unwind-retains");
    let service_thread = Arc::new(Mutex::new(None));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(
        Arc::clone(&seen),
        Some(XAuthorityBackpressureTelemetryKind::Wait),
        Arc::clone(&service_thread),
    );
    let Launched {
        handle,
        finished,
        handles,
        commands: _commands,
        transactions,
    } = launch_held(socket_path.clone(), namespace, 1, observer, service_thread);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    // The setup reply can precede the registration that fills the custody
    // place, so this is a bounded wait, not a single read.
    let before = waited_for_value(|| live_custody_identity(&handles.registry))
        .unwrap_or_else(|| {
            panic!(
                "the admitted connection's custody (keeper installed: {}, inventory alive: {})",
                handles.registry.custody_keeper.get().is_some(),
                handles
                    .registry
                    .custody_keeper
                    .get()
                    .is_some_and(|keeper| keeper.inventory.upgrade().is_some())
            )
        });
    let surface = draw_and_learn_surface(&mut client, &transactions);
    assert!(
        waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true)),
        "the worker's own wait is reported (and does not unwind: wrong thread)"
    );
    handles
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("the requirement is queued");
    let client_ended = eof_within(&mut client, 3);
    let (unwound, _error, reported, after) = launch_outcome(handle, &finished, false, "unwind retains");
    assert!(unwound, "the injected panic unwound the operation");
    assert!(client_ended, "the guard's Drop stopped the worker");
    assert_kept_exactly_one(&after);
    assert_same_custody(before, &after);
    // An unwind returns nothing, so the service's own account is empty; the
    // store's is what a reader gets.
    assert!(reported.is_empty());
    assert_eq!(after.shelf.len(), 1, "exactly the one unsent envelope is retained");
    let entry = &after.shelf[0];
    assert!(entry.holds_batch, "an unsent envelope still holds its batch");
    assert!(entry.observed_batch, "and it is the observed raster batch");
    assert_eq!(entry.transaction_count, 1, "carrying its one surface transaction");
    assert_eq!(entry.surface, Some(surface), "naming the surface the requirement was for");
    assert_eq!(entry.raster_response_count, 1, "and its raster response, intact");
    assert!(
        transactions.try_recv().is_ok(),
        "the transport still holds the worker's item, so the raster batch was never accepted"
    );
    assert_released(&after);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_unwind_after_the_transport_accepted_the_batch_retains_nothing_as_unsent() {
    // WHAT HAPPENED AT THE EFFECT. The observer unwinds on Resume -- after the
    // transport has taken the batch and the ticket has advanced. That is
    // delivered work with an unfinished report, and the store must not be
    // told it was unsent.
    let namespace = NamespaceId::from_raw(9305);
    let socket_path = private_service_socket("unwind-delivered");
    let service_thread = Arc::new(Mutex::new(None));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(
        Arc::clone(&seen),
        Some(XAuthorityBackpressureTelemetryKind::Resume),
        Arc::clone(&service_thread),
    );
    let Launched {
        handle,
        finished,
        handles,
        commands: _commands,
        transactions,
    } = launch_held(socket_path.clone(), namespace, 1, observer, service_thread);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let surface = draw_and_learn_surface(&mut client, &transactions);
    assert!(waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true)));
    let waits_before = seen.lock().expect("readable").len();
    handles
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("the requirement is queued");
    assert!(waited_for(|| saw_service_wait(&seen, waits_before)), "the service's own raster wait");
    // Now the transport is drained, bounded, until the launch scope returns:
    // the parked worker lands its remaining batches in ticket order, then the
    // service's envelope is accepted and its Resume report unwinds the
    // service thread. Draining here is the point of this control -- it is the
    // acceptance being observed -- not a rescue of collection.
    let mut delivered = Vec::new();
    let mut launch_finished = false;
    for _ in 0..500 {
        delivered.extend(transactions.try_recv());
        if finished.recv_timeout(Duration::from_millis(10)).is_ok() {
            launch_finished = true;
            break;
        }
    }
    let client_ended = eof_within(&mut client, 3);
    let (unwound, _error, reported, after) =
        launch_outcome(handle, &finished, launch_finished, "unwind after acceptance");
    while let Ok(batch) = transactions.try_recv() {
        delivered.push(batch);
    }
    assert!(unwound, "the injected panic unwound the operation after acceptance");
    assert!(client_ended);
    assert!(after.shelf.is_empty(), "a batch the transport took is not shelved as unsent");
    assert!(reported.is_empty());
    assert!(
        delivered.iter().any(|batch| !batch.raster_responses.is_empty()
            && batch.transactions.first().is_some_and(|t| t.surface == surface)),
        "the raster batch really was delivered to the transport"
    );
    assert_kept_exactly_one(&after);
    assert_released(&after);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_error_while_a_raster_envelope_waits_cancels_its_wait_and_retains_it() {
    // THE ERROR RETURN'S HALF OF THE SAME RULE: the pending envelope's wait is
    // cancelled (reported to the observer as Shutdown) and the unsent batch
    // still goes to the store, not to the floor.
    let namespace = NamespaceId::from_raw(9306);
    let socket_path = private_service_socket("error-retains");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(Arc::clone(&seen), None, Arc::new(Mutex::new(None)));
    let Launched {
        handle,
        finished,
        handles,
        commands,
        transactions,
    } = launch_held(socket_path.clone(), namespace, 1, observer, Arc::new(Mutex::new(None)));
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let surface = draw_and_learn_surface(&mut client, &transactions);
    assert!(waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true)));
    let waits_before = seen.lock().expect("readable").len();
    handles
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("the requirement is queued");
    assert!(waited_for(|| saw_service_wait(&seen, waits_before)), "the service's own raster wait");
    let (acknowledgement, acknowledged) = sync_channel(1);
    drop(acknowledged);
    commands
        .send(XServerFrontendServiceCommand::UpdateOutputTopology {
            snapshot: sophia_protocol::OutputTopologySnapshot {
                generation: 1,
                primary: sophia_protocol::OutputId::from_raw(1),
                outputs: Vec::new(),
            },
            acknowledgement,
        })
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let (unwound, error, reported, after) = launch_outcome(handle, &finished, false, "error retains");
    assert!(!unwound);
    assert!(client_ended);
    assert!(error.is_some_and(|text| text.contains("acknowledgement")));
    assert!(
        saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Shutdown, false),
        "the pending envelope's wait was cancelled and reported"
    );
    assert_unresolved_accounted(&reported, &after);
    assert_eq!(after.shelf.len(), 1);
    assert!(after.shelf[0].holds_batch, "cancelling a wait does not deliver the batch");
    assert_eq!(after.shelf[0].surface, Some(surface));
    assert_eq!(after.shelf[0].transaction_count, 1);
    assert_eq!(after.shelf[0].raster_response_count, 1, "its payload is intact");
    assert_kept_exactly_one(&after);
    assert_released(&after);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn a_lease_on_a_different_owner_is_refused_before_a_listener_is_bound() {
    let namespace = NamespaceId::from_raw(9307);
    let socket_path = private_service_socket("foreign");
    let durable_a = PrivateSettlementOwner::default();
    let owner_a = service_owner(&durable_a, 4);
    let durable_b = PrivateSettlementOwner::default();
    let owner_b = service_owner(&durable_b, 4);
    let private = crate::PrivateXServerFrontend::new(private_service_parts(4), &owner_a)
        .unwrap_or_else(|(refusal, _)| panic!("a frontend over owner A: {refusal:?}"));
    let (transaction_sender, _transactions) = sync_channel(4);
    let (_commands, service_commands) = sync_channel::<XServerFrontendServiceCommand>(1);
    let config = private_service_config(&socket_path, namespace, 4);
    let mut execution = PrivateServiceExecutionKeeper::new();
    let refused = serve_private_frontend_until_stopped(
        private,
        &owner_b.lease(),
        &mut execution,
        config,
        transaction_sender,
        service_commands,
            PrivateProducerPort::unattended(),
        Arc::new(|_| {}),
    );
    let PrivateServiceFailure::Failed {
        error, settlement, ..
    } = refused.err().expect("a foreign lease is refused")
    else {
        panic!("refused, not unbuilt")
    };
    assert!(error.to_string().contains("lease"), "{error}");
    assert!(!socket_path.exists(), "no listener was bound for a service that never began");
    drop(settlement);
    assert_eq!(owner_a.custodies_kept(), 0);
    assert_eq!(owner_b.custodies_kept(), 0);
    drop((owner_a, owner_b, durable_a, durable_b));
}

#[test]
fn a_refused_private_frontend_returns_its_parts_and_binds_nothing() {
    let namespace = NamespaceId::from_raw(9308);
    let socket_path = private_service_socket("refused");
    let durable = PrivateSettlementOwner::with_capacity(1);
    let owner = service_owner(&durable, 4);
    let first = crate::PrivateXServerFrontend::new(private_service_parts(4), &owner)
        .unwrap_or_else(|(refusal, _)| panic!("the first frontend: {refusal:?}"));
    let (transaction_sender, _transactions) = sync_channel(4);
    let (_commands, service_commands) = sync_channel::<XServerFrontendServiceCommand>(1);
    let config = private_service_config(&socket_path, namespace, 4);
    let mut execution = PrivateServiceExecutionKeeper::new();
    let refused = run_x_server_frontend_private_until_stopped(
        config,
        transaction_sender,
        private_service_parts(4),
        &owner,
        &mut execution,
        service_commands,
            PrivateProducerPort::unattended(),
        Arc::new(|_| {}),
    );
    let PrivateServiceFailure::Refused { refusal, parts } =
        refused.err().expect("a store with no failure slot left refuses")
    else {
        panic!("refused at construction, not later")
    };
    assert!(matches!(refusal, AdmissionRefusal::Saturated), "{refusal:?}");
    assert_eq!(parts.max_concurrent_clients.get(), 4, "the caller's parts come back");
    assert!(!socket_path.exists(), "no listener was bound");
    drop((parts, first.shutdown(), owner, durable));
}





/// An ordinary stop that arrives while the service's own raster envelope is
/// waiting on a full transport. Shared by the two stop shapes.
fn stop_with_a_waiting_envelope(
    tag: &str,
    namespace: NamespaceId,
    stop: impl FnOnce(SyncSender<XServerFrontendServiceCommand>),
) {
    let socket_path = private_service_socket(tag);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(Arc::clone(&seen), None, Arc::new(Mutex::new(None)));
    let Launched {
        handle,
        finished,
        handles,
        commands,
        transactions,
    } = launch_held(socket_path.clone(), namespace, 1, observer, Arc::new(Mutex::new(None)));
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    // The setup reply can precede the registration that fills the custody
    // place, so this is a bounded wait, not a single read.
    let before = waited_for_value(|| live_custody_identity(&handles.registry))
        .unwrap_or_else(|| {
            panic!(
                "the admitted connection's custody (keeper installed: {}, inventory alive: {})",
                handles.registry.custody_keeper.get().is_some(),
                handles
                    .registry
                    .custody_keeper
                    .get()
                    .is_some_and(|keeper| keeper.inventory.upgrade().is_some())
            )
        });
    let surface = draw_and_learn_surface(&mut client, &transactions);
    assert!(waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true)));
    let waits_before = seen.lock().expect("readable").len();
    handles
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("the requirement is queued");
    assert!(waited_for(|| saw_service_wait(&seen, waits_before)), "the service's own raster wait");
    stop(commands);
    let client_ended = eof_within(&mut client, 3);
    let (unwound, error, reported, after) = launch_outcome(handle, &finished, false, tag);
    assert!(!unwound);
    assert!(client_ended);
    assert!(error.is_none(), "an ordinary stop, not an error: {error:?}");
    assert!(
        saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Shutdown, false),
        "the envelope's wait was cancelled and reported"
    );
    // CANCELLING THE WAIT IS NOT DELIVERING THE BATCH: it is still unsent, so
    // it is on the shelf, and the service says so by transaction.
    assert_unresolved_accounted(&reported, &after);
    assert_eq!(after.shelf.len(), 1, "the unsent envelope survives an ordinary stop");
    assert!(after.shelf[0].holds_batch);
    assert_eq!(after.shelf[0].surface, Some(surface));
    assert_eq!(after.shelf[0].transaction_count, 1);
    assert_eq!(after.shelf[0].raster_response_count, 1, "its payload is intact");
    assert!(
        transactions.try_recv().is_ok(),
        "the transport still holds the worker's item: the batch was never accepted"
    );
    assert_kept_exactly_one(&after);
    assert_same_custody(before, &after);
    assert_released(&after);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_ordinary_stop_keeps_the_unsent_raster_envelope() {
    stop_with_a_waiting_envelope("stop-retains", NamespaceId::from_raw(9311), |commands| {
        commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect)
            .expect("the service is listening for commands");
    });
}

#[test]
fn losing_the_command_channel_keeps_the_unsent_raster_envelope() {
    stop_with_a_waiting_envelope("channel-loss-retains", NamespaceId::from_raw(9312), drop);
}

#[test]
fn a_shutdown_report_that_unwinds_during_a_stop_still_keeps_the_envelope() {
    // THE OBSERVER UNWINDS ON THE CANCELLATION REPORT ITSELF. The envelope is
    // cancelled in place in the loop's stop arm; the unwind there leaves it
    // in the guard's slot, and the guard's Drop shelves it.
    let namespace = NamespaceId::from_raw(9313);
    let socket_path = private_service_socket("stop-unwind-retains");
    let service_thread = Arc::new(Mutex::new(None));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(
        Arc::clone(&seen),
        Some(XAuthorityBackpressureTelemetryKind::Shutdown),
        Arc::clone(&service_thread),
    );
    let Launched {
        handle,
        finished,
        handles,
        commands,
        transactions,
    } = launch_held(socket_path.clone(), namespace, 1, observer, service_thread);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    // The setup reply can precede the registration that fills the custody
    // place, so this is a bounded wait, not a single read.
    let before = waited_for_value(|| live_custody_identity(&handles.registry))
        .unwrap_or_else(|| {
            panic!(
                "the admitted connection's custody (keeper installed: {}, inventory alive: {})",
                handles.registry.custody_keeper.get().is_some(),
                handles
                    .registry
                    .custody_keeper
                    .get()
                    .is_some_and(|keeper| keeper.inventory.upgrade().is_some())
            )
        });
    let surface = draw_and_learn_surface(&mut client, &transactions);
    assert!(waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true)));
    let waits_before = seen.lock().expect("readable").len();
    handles
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("the requirement is queued");
    assert!(waited_for(|| saw_service_wait(&seen, waits_before)));
    commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let (unwound, _error, reported, after) =
        launch_outcome(handle, &finished, false, "stop with an unwinding shutdown report");
    assert!(unwound, "the cancellation report unwound the operation");
    assert!(client_ended, "the guard's Drop collected the worker");
    assert!(reported.is_empty());
    assert_eq!(after.shelf.len(), 1, "the envelope survives the unwind");
    assert_eq!(after.shelf[0].surface, Some(surface));
    assert_eq!(after.shelf[0].transaction_count, 1);
    assert_eq!(after.shelf[0].raster_response_count, 1, "its payload is intact");
    assert_kept_exactly_one(&after);
    assert_same_custody(before, &after);
    assert_released(&after);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn a_cancelled_submission_keeps_its_batch_in_the_slot() {
    // A DIRECT EGRESS SEAM, labelled as such. Cancellation is already
    // published when the submission is attempted; the wait ends, the
    // envelope and its batch stay where the owner can reach them.
    let (transaction_sender, transactions) = sync_channel::<XAuthorityObservedTransactionBatch>(1);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let egress = XAuthorityOrderedEgress::new(
        transaction_sender,
        Arc::new(AtomicBool::new(false)),
        recording_observer(Arc::clone(&seen), None, Arc::new(Mutex::new(None))),
    );
    // A batch with nothing in it but its identity: the seam is about custody
    // of the envelope, not about what the batch carries.
    let batch = XAuthorityObservedTransactionBatch {
        client: None,
        admission: None,
        surface_routes: Vec::new(),
        transaction: TransactionId::from_raw(9314),
        transactions: Vec::new(),
        surface_presentations: Vec::new(),
        presentation_intents: Vec::new(),
        removed_surfaces: Vec::new(),
        surface_output_reservations: Vec::new(),
        cpu_buffer_updates: Vec::new(),
        raster_responses: Vec::new(),
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
        protocol_errors: Vec::new(),
        expected_protocol_errors: Vec::new(),
        metadata: Vec::new(),
        selection_owner_change: false,
        selection_conversion: false,
    };
    let mut slot = Some(XAuthorityBoundedEgressEnvelope::new(
        TransactionId::from_raw(9314),
        Some(batch),
    ));
    egress.cancel();
    egress.try_submit(&mut slot).expect("a cancelled submission is not an error");
    let envelope = slot.as_ref().expect("the envelope stays in the slot");
    assert!(envelope.cancelled, "its wait ended in cancellation");
    assert!(envelope.batch.is_some(), "and it still holds its batch");
    assert!(transactions.try_recv().is_err(), "nothing was sent");
    // And asking again reports nothing twice: exactly one Shutdown across
    // both submissions.
    egress.try_submit(&mut slot).expect("idempotent");
    assert!(slot.is_some());
    let shutdowns = seen
        .lock()
        .expect("readable")
        .iter()
        .filter(|(kind, _)| *kind == XAuthorityBackpressureTelemetryKind::Shutdown)
        .count();
    assert_eq!(shutdowns, 1, "a cancelled wait is reported once");
}

#[test]
fn the_shelf_keeps_its_charge_on_the_store_while_it_is_retained() {
    // ONE ORIGINAL STORE, CAPACITY ONE, TWO INVOCATIONS. The first leaves an
    // unsent envelope on the shelf and its launch owner goes. The second
    // cannot even construct a frontend: the shelved envelope keeps the one
    // failure slot charged, and NOTHING releases it -- no reader takes the
    // work out, because taking would uncharge it, and accounting for retained
    // egress is a disposition this store does not have. The shelf never
    // allocates past its capacity.
    let namespace = NamespaceId::from_raw(9315);
    let durable = Arc::new(PrivateSettlementOwner::with_capacity(1));
    assert_eq!(durable.unresolved_egress_capacity(), Some(1));

    // First invocation: a real error while the raster envelope waits.
    let socket_path = private_service_socket("charge-1");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(Arc::clone(&seen), None, Arc::new(Mutex::new(None)));
    let (transaction_sender, transactions) = sync_channel(1);
    let (commands, service_commands) = sync_channel(4);
    let (handles_out, handles_in) = channel();
    let store = Arc::clone(&durable);
    let path = socket_path.clone();
    let (done, finished) = channel();
    let first = std::thread::spawn(move || {
        let owner = service_owner(&store, 4);
        let private = crate::PrivateXServerFrontend::new(private_service_parts(4), &owner)
            .unwrap_or_else(|(refusal, _)| panic!("the first frontend: {refusal:?}"));
        let _ = handles_out.send(Handles {
            registry: private.broker.registry.clone(),
            raster: private.broker.raster_router(),
        });
        let lease = owner.lease();
        let mut execution = PrivateServiceExecutionKeeper::new();
        let outcome = serve_private_frontend_until_stopped(
            private,
            &lease,
            &mut execution,
            private_service_config(&path, namespace, 4),
            transaction_sender,
            service_commands,
            PrivateProducerPort::unattended(),
            observer,
        );
        let _ = done.send(());
        outcome.is_err()
    });
    let handles = handles_in.recv_timeout(Duration::from_secs(15)).expect("built");
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let surface = draw_and_learn_surface(&mut client, &transactions);
    assert!(waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true)));
    let waits_before = seen.lock().expect("readable").len();
    handles.raster.try_route(raster_requirement_for(surface)).expect("queued");
    assert!(waited_for(|| saw_service_wait(&seen, waits_before)));
    let (acknowledgement, acknowledged) = sync_channel(1);
    drop(acknowledged);
    commands
        .send(XServerFrontendServiceCommand::UpdateOutputTopology {
            snapshot: sophia_protocol::OutputTopologySnapshot {
                generation: 1,
                primary: sophia_protocol::OutputId::from_raw(1),
                outputs: Vec::new(),
            },
            acknowledgement,
        })
        .expect("listening");
    assert!(eof_within(&mut client, 3));
    assert!(finished.recv_timeout(Duration::from_secs(15)).is_ok(), "the first invocation returned");
    assert!(first.join().expect("joined"), "it failed");
    drop(handles);
    let _ = std::fs::remove_file(&socket_path);
    assert_eq!(durable.unresolved_egress(), Some(1));
    assert_eq!(durable.unresolved_egress_capacity(), Some(1), "no allocation past the reserved capacity");

    // Second invocation: refused at construction, before any exposure, with
    // the exact work still on the shelf under the first invocation's number.
    let shelf = read_shelf(&durable);
    assert_eq!(shelf.len(), 1);
    assert!(shelf[0].holds_batch);
    assert_eq!(shelf[0].surface, Some(surface));
    assert_eq!(shelf[0].transaction_count, 1);
    assert_eq!(shelf[0].raster_response_count, 1);
    assert_eq!(shelf[0].obligation.instance, 1, "the first reservation's number");
    let owner_two = service_owner(&durable, 4);
    let refused = crate::PrivateXServerFrontend::new(private_service_parts(4), &owner_two);
    let Err((refusal, parts)) = refused else {
        panic!("the shelved envelope keeps the failure slot charged")
    };
    assert!(matches!(refusal, AdmissionRefusal::Saturated), "{refusal:?}");
    assert_eq!(parts.max_concurrent_clients.get(), 4);
    drop((parts, owner_two));
    // Reading the shelf changed nothing: still charged, still refused.
    assert_eq!(read_shelf(&durable), shelf);
    assert_eq!(durable.unresolved_egress(), Some(1));
    assert_eq!(durable.unresolved_egress_capacity(), Some(1));
    let owner_again = service_owner(&durable, 4);
    assert!(
        crate::PrivateXServerFrontend::new(private_service_parts(4), &owner_again).is_err(),
        "no reader uncharged the retained work"
    );
    drop((owner_again, durable));
}

#[test]
fn obligations_from_two_invocations_stay_distinct_after_their_frames_are_gone() {
    // TWO INVOCATIONS, ONE STORE OF CAPACITY TWO, EACH LEAVING AN UNSENT
    // ENVELOPE. Each frontend numbers its transactions from one, so both
    // envelopes carry the same transaction number; what tells them apart --
    // after both launch scopes have returned and their frontends are gone --
    // is the instance number each reservation was given before exposure.
    let namespace = NamespaceId::from_raw(9317);
    let durable = Arc::new(PrivateSettlementOwner::with_capacity(2));
    let mut returned = Vec::new();
    for round in 0..2u32 {
        let socket_path = private_service_socket(&format!("distinct-{round}"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let observer = recording_observer(Arc::clone(&seen), None, Arc::new(Mutex::new(None)));
        let (transaction_sender, transactions) = sync_channel(1);
        let (commands, service_commands) = sync_channel(4);
        let (handles_out, handles_in) = channel();
        let store = Arc::clone(&durable);
        let path = socket_path.clone();
        let (done, finished) = channel();
        let launch = std::thread::spawn(move || {
            let owner = service_owner(&store, 4);
            let private = crate::PrivateXServerFrontend::new(private_service_parts(4), &owner)
                .unwrap_or_else(|(refusal, _)| panic!("frontend {round}: {refusal:?}"));
            let _ = handles_out.send(Handles {
                registry: private.broker.registry.clone(),
                raster: private.broker.raster_router(),
            });
            let lease = owner.lease();
            let mut execution = PrivateServiceExecutionKeeper::new();
            let outcome = serve_private_frontend_until_stopped(
                private,
                &lease,
                &mut execution,
                private_service_config(&path, namespace, 4),
                transaction_sender,
                service_commands,
            PrivateProducerPort::unattended(),
                observer,
            );
            let _ = done.send(());
            match outcome {
                Err(PrivateServiceFailure::Failed {
                    unresolved_egress, ..
                }) => unresolved_egress,
                other => panic!("an error with a pending envelope: {:?}", other.err()),
            }
        });
        let handles = handles_in.recv_timeout(Duration::from_secs(15)).expect("built");
        let mut client = connect_private_client(&socket_path);
        handshake(&mut client);
        let surface = draw_and_learn_surface(&mut client, &transactions);
        assert!(waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true)));
        let waits_before = seen.lock().expect("readable").len();
        handles.raster.try_route(raster_requirement_for(surface)).expect("queued");
        assert!(waited_for(|| saw_service_wait(&seen, waits_before)));
        let (acknowledgement, acknowledged) = sync_channel(1);
        drop(acknowledged);
        commands
            .send(XServerFrontendServiceCommand::UpdateOutputTopology {
                snapshot: sophia_protocol::OutputTopologySnapshot {
                    generation: 1,
                    primary: sophia_protocol::OutputId::from_raw(1),
                    outputs: Vec::new(),
                },
                acknowledgement,
            })
            .expect("listening");
        assert!(eof_within(&mut client, 3));
        assert!(finished.recv_timeout(Duration::from_secs(15)).is_ok(), "invocation {round} returned");
        returned.push(launch.join().expect("joined"));
        drop(handles);
        let _ = std::fs::remove_file(&socket_path);
    }
    assert_eq!(returned[0].len(), 1);
    assert_eq!(returned[1].len(), 1);
    assert_eq!(
        returned[0][0].transaction, returned[1][0].transaction,
        "each frontend numbered its transactions from one: the numbers collide"
    );
    assert_ne!(returned[0][0].instance, returned[1][0].instance, "the invocations do not");
    assert_eq!(
        durable.unresolved_egress_obligations().expect("readable"),
        vec![returned[0][0], returned[1][0]],
        "the store names each obligation by the invocation that left it"
    );
    drop(durable);
}

#[test]
fn an_invocation_that_owes_no_retained_egress_releases_its_charge_for_reuse() {
    // THE OTHER HALF OF THE BOUND: a service that shelved nothing gives its
    // failure slot back when it closes, and the next CONSTRUCTION over the
    // same capacity-one store succeeds without anyone taking anything.
    // Construction is what capacity admission decides; nothing is served by
    // the second frontend here.
    let namespace = NamespaceId::from_raw(9316);
    let socket_path = private_service_socket("charge-reuse");
    let durable = PrivateSettlementOwner::with_capacity(1);
    {
        let owner = service_owner(&durable, 4);
        let (transaction_sender, _transactions) = sync_channel(4);
        let (commands, service_commands) = sync_channel(4);
        let (done, finished) = channel();
        let private = crate::PrivateXServerFrontend::new(private_service_parts(4), &owner)
            .unwrap_or_else(|(refusal, _)| panic!("the first frontend: {refusal:?}"));
        let config = private_service_config(&socket_path, namespace, 4);
        let outcome = std::thread::scope(|scope| {
            let service = scope.spawn(|| {
                let lease = owner.lease();
                let mut execution = PrivateServiceExecutionKeeper::new();
                let outcome = serve_private_frontend_until_stopped(
                    private,
                    &lease,
                    &mut execution,
                    config,
                    transaction_sender,
                    service_commands,
            PrivateProducerPort::unattended(),
                    Arc::new(|_| {}),
                );
                let _ = done.send(());
                outcome
            });
            let mut client = connect_private_client(&socket_path);
            handshake(&mut client);
            commands
                .send(XServerFrontendServiceCommand::StopAndDisconnect)
                .expect("listening");
            assert!(eof_within(&mut client, 3));
            assert!(finished.recv_timeout(Duration::from_secs(15)).is_ok(), "the service returned");
            service.join().expect("joined")
        });
        let returned = outcome.expect("an ordinary stop");
        assert!(returned.unresolved_egress.is_empty(), "nothing was owed");
        drop(returned);
        let _ = std::fs::remove_file(&socket_path);
    }
    assert_eq!(durable.unresolved_egress(), Some(0));
    let owner_two = service_owner(&durable, 4);
    let second = crate::PrivateXServerFrontend::new(private_service_parts(4), &owner_two)
        .unwrap_or_else(|(refusal, _)| panic!("the charge was released for reuse: {refusal:?}"));
    drop(second.shutdown());
    drop((owner_two, durable));
}


#[test]
fn a_store_refuses_a_reservation_before_it_would_reissue_an_identity() {
    // REPRESENTATIONAL EXHAUSTION, STAGED. The counter is put one below its
    // maximum directly -- pretending to run the enormous prefix would prove
    // nothing -- and then the seam is exercised for real: the last admissible
    // number is issued and charged; the successor is refused with the charge
    // and the counter untouched; and giving an earlier capacity charge back
    // does not bring a spent identity back.
    let durable = PrivateSettlementOwner::with_capacity(4);
    {
        let mut held = durable.inner.lock().expect("a readable store");
        held.next_instance = u64::MAX - 1;
    }
    let last = durable
        .reserve_failure_slot()
        .expect("the last admissible identity is issued");
    assert_eq!(last, u64::MAX - 1);
    let (slots_after_last, counter_after_last) = {
        let held = durable.inner.lock().expect("readable");
        (held.failure_slots, held.next_instance)
    };
    assert_eq!(slots_after_last, 1, "and charged");
    assert_eq!(counter_after_last, u64::MAX, "the counter now marks exhaustion");

    let refused = durable.reserve_failure_slot();
    assert!(
        matches!(refused, Err(AdmissionRefusal::Exhausted)),
        "the successor is refused rather than reissued: {refused:?}"
    );
    {
        let held = durable.inner.lock().expect("readable");
        assert_eq!(held.failure_slots, 1, "a refusal charges nothing");
        assert_eq!(held.next_instance, u64::MAX, "and moves nothing");
    }

    // Releasing the earlier charge frees capacity, not identity.
    durable.release_failure_slot();
    assert_eq!(durable.inner.lock().expect("readable").failure_slots, 0);
    let still_refused = durable.reserve_failure_slot();
    assert!(
        matches!(still_refused, Err(AdmissionRefusal::Exhausted)),
        "a spent identity is not restored by a returned charge: {still_refused:?}"
    );
    assert_eq!(durable.inner.lock().expect("readable").failure_slots, 0);
}
