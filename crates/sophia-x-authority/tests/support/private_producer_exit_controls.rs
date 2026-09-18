// Cancelling-stop and graceful-drain controls, included by the existing
// private routing test module so they exercise the actual loop and adapter.

// REVIEW PROBES: existing actual-service frame pause, with the two normal
// loop exits instead of the author's error/unwind path. Observations are
// captured while paused; the frame is released and service joined before
// any final assertion. No production source is changed.
fn review_producers_close_during_loop_exit(channel_lost: bool) {
    let tag = if channel_lost {
        "review-channel-close"
    } else {
        "review-ordinary-close"
    };
    let (mut launched, socket_path) = launch_producing(tag, 9950 + u64::from(channel_lost), 4);
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .expect("readiness");
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.registry);
    let client_id = custody.cleanup_record().client;
    let owner = Arc::clone(&launched.owner);
    let lease = owner.lease();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .expect("ingress");
    let control = launched.access.control_producer(&lease).expect("control");
    let release = pause_after_registration_drop(&launched.registry, client_id);
    if channel_lost {
        let (dummy, _receiver) = sync_channel(1);
        drop(std::mem::replace(&mut launched.commands, dummy));
    } else {
        launched
            .commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect)
            .expect("command");
    }
    let paused = waited_for(|| {
        matches!(
            custody.cleanup_record().destruction_standing(),
            PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(_))
        )
    });
    let standing = launched.access.standing();
    let input = ingress
        .submit(
            &lease,
            button_to(
                SurfaceId::new(1, 1),
                XAuthorityInputDeliveryId::from_raw(99501),
                272,
                true,
            ),
        )
        .map(|_| ())
        .map_err(|e| format!("{e:?}"));
    let command = control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client: client_id,
                command: XAuthorityControlCommand::ClearFocus {
                    transaction: TransactionId::from_raw(99502),
                    surface: SurfaceId::new(1, 1),
                },
            },
        )
        .map(|_| ())
        .map_err(|(e, _)| format!("{e:?}"));
    let release_sent = release.send(()).is_ok();
    let client_ended = eof_within(&mut client, 3);
    let outcome = produced_outcome(launched, tag);
    let _ = std::fs::remove_file(&socket_path);
    let diagnostic = format!(
        "channel_lost={channel_lost} paused={paused} port={standing:?} input={input:?} control={command:?} release_sent={release_sent} client_ended={client_ended} service_ok={:?}",
        outcome.ok
    );
    assert!(
        paused && release_sent && client_ended,
        "the schedule reached its bounded pause and was collected"
    );
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    assert_eq!(
        standing,
        PrivatePortStanding::Ended,
        "normal loop exit must close issuance before waiting for its connection frames: {diagnostic}"
    );
    assert!(
        command.is_err(),
        "an issued control producer must refuse during shutdown"
    );
    assert!(
        input.is_err(),
        "an issued ingress must refuse during shutdown"
    );
}

#[test]
fn review_ordinary_stop_closes_producers_before_waiting_for_connection_frames() {
    review_producers_close_during_loop_exit(false);
}

#[test]
fn review_command_channel_loss_closes_producers_before_waiting_for_connection_frames() {
    review_producers_close_during_loop_exit(true);
}

// REVIEW PROBE: the existing cancellation observer is paused on the real
// service's Shutdown report, after stop is consumed and before socket
// shutdown. This is a supplied observer, not a production scheduling hook.
#[test]
fn review_ordinary_stop_closes_admission_before_reporting_cancellation() {
    let seen: SeenTelemetry = Arc::new(Mutex::new(Vec::new()));
    let service_thread = Arc::new(Mutex::new(None));
    let (arrived, arrival) = sync_channel(1);
    let (release, released) = sync_channel(1);
    let released = Mutex::new(released);
    let observed = Arc::clone(&seen);
    let observer: Arc<XAuthorityBackpressureObserver> = Arc::new(move |telemetry| {
        observed
            .lock()
            .expect("telemetry")
            .push((telemetry.kind, telemetry.client));
        if telemetry.kind == XAuthorityBackpressureTelemetryKind::Shutdown
            && telemetry.client.is_none()
        {
            let _ = arrived.try_send(());
            let _ = released
                .lock()
                .expect("pause")
                .recv_timeout(Duration::from_secs(10));
        }
    });
    let (launched, socket_path) = launch_producing_observed(
        "review-stop-observer",
        9953,
        4,
        false,
        observer,
        1,
        service_thread,
    );
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .expect("ready");
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.registry);
    let client_id = custody.cleanup_record().client;
    let owner = Arc::clone(&launched.owner);
    let lease = owner.lease();
    let control = launched
        .access
        .control_producer(&lease)
        .expect("control producer");
    let surface = draw_and_learn_surface(&mut client, &launched.transactions);
    let worker_waiting =
        waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true));
    let waits_before = seen.lock().expect("seen").len();
    launched
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("raster requirement");
    let service_waiting = waited_for(|| saw_service_wait(&seen, waits_before));
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("stop");
    let paused = arrival.recv_timeout(Duration::from_secs(5)).is_ok();
    let standing = launched.access.standing();
    let gate_open = control.admission.lifecycle_open();
    let writer_present = control.routing.control_writer_present(client_id);
    let accepted = control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client: client_id,
                command: XAuthorityControlCommand::ClearFocus {
                    transaction: TransactionId::from_raw(99530),
                    surface,
                },
            },
        )
        .map(|_| ())
        .map_err(|(error, _)| format!("{error:?}"));
    let _ = release.send(());
    let ended = eof_within(&mut client, 3);
    let outcome = produced_outcome(launched, "stop observer");
    let _ = std::fs::remove_file(&socket_path);
    let diagnostic = format!(
        "worker_waiting={worker_waiting} service_waiting={service_waiting} paused={paused} port={standing:?} gate_open={gate_open} writer_present={writer_present} submit_after_stop={accepted:?} ended={ended} service_ok={:?}",
        outcome.ok
    );
    assert!(worker_waiting && service_waiting && paused && ended);
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    assert!(
        accepted.is_err(),
        "the stop command was consumed before this new submission: {diagnostic}"
    );
    assert!(!gate_open);
    assert_eq!(standing, PrivatePortStanding::Ended);
}

/// Exercise the actual loop and private adapter with egress already cancelled.
/// No post-loop collection runs until after the observations: closure inside
/// the `!cancelled()` branch or only in the guard cannot satisfy this control.
fn producers_close_with_egress_already_cancelled(channel_lost: bool) {
    let (mut runner, owner, registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let lease = owner.lease();
    let control = runner.control_producer(&lease).expect("control");
    let (mut port, access) = PrivateProducerAccess::for_service();
    port.publish_ready();
    assert!(control.admission.lifecycle_open());
    let socket = private_service_socket("producer-pre-cancelled");
    let mut frontend = XServerFrontend::bind(private_service_config(
        &socket,
        NamespaceId::from_raw(9000),
        4,
    ))
    .expect("a bound frontend");
    let (transactions, _receiver) = sync_channel(1);
    let egress = XAuthorityOrderedEgress::new(
        transactions,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    );
    egress.cancel();
    let (commands, received) = sync_channel(1);
    let commands = if channel_lost {
        drop(commands);
        None
    } else {
        commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect)
            .expect("stop queued");
        Some(commands)
    };
    let mut order = PrivateOrderTally::default();
    let observer: Arc<X11CoreTraceObserver> = Arc::new(|_| Ok(None));
    let result = drive_routed_service(
        &mut frontend,
        &mut LeasedPrivateBroker {
            runner: &mut runner,
            port: &mut port,
            order: &mut order,
            service: &lease,
        },
        &received,
        &egress,
        &observer,
        &mut None,
    );
    let standing = access.standing();
    let gate_open = control.admission.lifecycle_open();
    let refused = control
        .submit(
            &lease,
            configure(
                XServerFrontendClientId::from_raw(9000),
                SurfaceId::new(9000, 1),
                99540,
            ),
        )
        .map_err(|(refusal, _)| refusal);
    drop(commands);
    drop(registration);
    drop(runner.shutdown());
    drop(frontend);
    let _ = std::fs::remove_file(socket);
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        standing,
        PrivatePortStanding::Ended,
        "closure precedes post-loop collection"
    );
    assert!(!gate_open, "the existing producer's gate closed too");
    assert_eq!(refused.err(), Some(AdmissionRefusal::ConsumerGone));
    assert_eq!(
        order,
        PrivateOrderTally::default(),
        "no accepted order was driven"
    );
}

#[test]
fn ordinary_stop_closes_producers_when_egress_was_already_cancelled() {
    producers_close_with_egress_already_cancelled(false);
}

#[test]
fn command_channel_loss_closes_producers_when_egress_was_already_cancelled() {
    producers_close_with_egress_already_cancelled(true);
}

#[test]
fn private_drain_preserves_egress_and_producer_policy_until_connection_frames_finish() {
    let seen: SeenTelemetry = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(Arc::clone(&seen), None, Arc::new(Mutex::new(None)));
    let (launched, socket) = launch_producing_observed(
        "producer-drain",
        9955,
        4,
        false,
        observer,
        1,
        Arc::new(Mutex::new(None)),
    );
    launched
        .access
        .await_ready(Duration::from_secs(15))
        .expect("ready");
    let mut client = connect_private_client(&socket);
    handshake(&mut client);
    let custody = wait_attached(&launched.registry);
    let owner = Arc::clone(&launched.owner);
    let control = launched
        .access
        .control_producer(&owner.lease())
        .expect("control");
    let surface = draw_and_learn_surface(&mut client, &launched.transactions);
    assert!(waited_for(|| saw_kind(
        &seen,
        XAuthorityBackpressureTelemetryKind::Wait,
        true
    )));
    let waits_before = seen.lock().expect("seen").len();
    launched
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("raster queued");
    assert!(waited_for(|| saw_service_wait(&seen, waits_before)));
    let release =
        pause_after_registration_drop(&launched.registry, custody.cleanup_record().client);
    launched
        .commands
        .send(XServerFrontendServiceCommand::DrainAndDisconnect)
        .expect("drain");
    let mut batches = Vec::new();
    let paused = waited_for(|| {
        if let Ok(batch) = launched.transactions.recv_timeout(Duration::from_millis(1)) {
            batches.push(batch);
        }
        matches!(
            custody.cleanup_record().destruction_standing(),
            PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(_))
        )
    });
    let standing = launched.access.standing();
    let gate_open = control.admission.lifecycle_open();
    let released = release.send(()).is_ok();
    // Drain the remaining accepted work and teardown, keeping the client
    // handle open. A timeout is captured, then emergency cleanup joins the
    // service before assertions, so a broken drain cannot strand an actor.
    let mut drained = false;
    loop {
        match launched.transactions.recv_timeout(Duration::from_secs(3)) {
            Ok(batch) => batches.push(batch),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                drained = true;
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let _ = launched
                    .commands
                    .send(XServerFrontendServiceCommand::StopAndDisconnect);
                break;
            }
        }
    }
    let ended = eof_within(&mut client, 3);
    let outcome = produced_outcome(launched, "private drain");
    let _ = std::fs::remove_file(socket);
    assert!(
        paused && released && drained && ended,
        "drain reached and collected its bounded frame pause"
    );
    assert_eq!(
        standing,
        PrivatePortStanding::Ready,
        "drain keeps its producer policy until collection"
    );
    assert!(
        gate_open,
        "execution supervision was kept through the drain"
    );
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    assert!(
        outcome.after.shelf.is_empty(),
        "the unsent raster batch drained rather than being shelved"
    );
    assert_eq!(
        batches
            .iter()
            .map(|batch| batch.cpu_buffer_updates.len())
            .sum::<usize>(),
        2
    );
    assert_eq!(
        batches
            .iter()
            .map(|batch| batch.raster_responses.len())
            .sum::<usize>(),
        1
    );
    assert_eq!(
        batches
            .iter()
            .map(|batch| batch.removed_surfaces.len())
            .sum::<usize>(),
        1
    );
    assert!(
        !saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Shutdown, false),
        "no cancellation reported"
    );
}
