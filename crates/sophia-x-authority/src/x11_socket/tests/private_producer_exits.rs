// Controls for what the runner-driven service keeps and refuses at its
// exits: a routing attempt interrupted before or after its effect, a held
// press through an ordinary stop, an error and an unwind, a service whose
// runner cannot be prepared, and producer admission closed before anything
// is waited for. Harness in `private_producer_service.rs`.

/// STAGE-ONLY: the routing attempt interrupted at each of its two points,
/// over a prepared runner fixture. What is read is the exact custody:
/// sequence, identity, whether the effect may have begun, the outstanding
/// list, and what shutdown hands over.
/// What an interrupted routing attempt left: the attempt in custody, the
/// outstanding count, the order's answer afterwards, and the durable owner's
/// owed / outstanding counts before and after shutdown.
struct InterruptedRouting {
    attempt: Option<(crate::ReadySequence, PrivateIdentity, bool)>,
    outstanding: usize,
    next: Result<PrivateOrderedStep, XServerFrontendRouteError>,
    owed_before: Option<usize>,
    owed_after: Option<usize>,
    outstanding_before: Option<usize>,
    outstanding_after: Option<usize>,
}

fn interrupted_routing(point: PrivateRoutingPoint) -> InterruptedRouting {
    let (mut runner, owner, registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let lease = owner.lease();
    let sequence = runner
        .control_producer(&lease)
        .expect("its own owner")
        .submit(&lease, configure(XServerFrontendClientId::from_raw(9000), SurfaceId::new(9000, 1), 97000))
        .expect("accepted");
    let store = owner.store();
    let owed_before = store.owed();
    let outstanding_before = store.outstanding();
    stage_routing_at(point, move || panic!("the routing attempt is lost at {point:?}"));
    let lost = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner.execute_accounted_step()
    }));
    assert!(lost.is_err(), "the seam unwound the step");
    let attempt = runner.frontend().routing_attempt();
    let outstanding = runner.frontend().outstanding.len();
    // The unwind dropped the budget's run, which closes the budget for good;
    // the order itself is asked directly, past that closed budget.
    let budget_interrupted = runner.service.is_interrupted();
    let next = {
        let PrivatePreparedRunner {
            frontend,
            keyboards,
            watch,
            ..
        } = &mut runner;
        frontend.as_mut().expect("a live runner").step_once(
            keyboards,
            &mut |_, _| Ok(()),
            watch.as_ref().expect("a sealed watch"),
        )
    };
    assert!(budget_interrupted, "the unwind closed the budget");
    assert_eq!(attempt.map(|(found, _, _)| found), Some(sequence), "the attempt is this control's");
    drop(registration);
    let settlement = runner.shutdown();
    let owed_after = store.owed();
    // The identities the instance still owed go with the settlement itself.
    let outstanding_after = outstanding_before.map(|before| before + settlement.outstanding.len());
    drop((settlement, owner));
    InterruptedRouting {
        attempt,
        outstanding,
        next,
        owed_before,
        owed_after,
        outstanding_before,
        outstanding_after,
    }
}

#[test]
fn a_routing_attempt_lost_before_its_effect_is_owned_unattempted_and_handed_over_once() {
    let InterruptedRouting {
        attempt,
        outstanding,
        next,
        owed_before,
        owed_after,
        outstanding_before,
        outstanding_after,
    } = interrupted_routing(PrivateRoutingPoint::AfterAdmitted);
    assert_eq!(attempt.map(|(_, _, attempted)| attempted), Some(false), "un-attempted");
    assert_eq!(outstanding, 0, "owing nothing yet");
    assert!(
        matches!(next, Err(XServerFrontendRouteError::OrderedItemUnresolved)),
        "the order is blocked on it: {}",
        next.as_ref().err().map(|error| error.to_string()).unwrap_or_else(|| "a step".into())
    );
    assert_eq!(owed_after, owed_before.map(|owed| owed + 1), "handed over once, as a parked operation");
    assert_eq!(outstanding_after, outstanding_before, "and not also as an outstanding identity");
}

#[test]
fn a_routing_attempt_lost_after_its_effect_is_owned_attempted_and_answered_for_once() {
    let InterruptedRouting {
        attempt,
        outstanding,
        next,
        owed_before,
        owed_after,
        outstanding_before,
        outstanding_after,
    } = interrupted_routing(PrivateRoutingPoint::AfterEffect);
    assert_eq!(attempt.map(|(_, _, attempted)| attempted), Some(true), "attempted");
    assert_eq!(outstanding, 1, "its credit owed exactly once");
    assert!(
        matches!(next, Err(XServerFrontendRouteError::OrderedItemUnresolved)),
        "the order is blocked on it: {}",
        next.as_ref().err().map(|error| error.to_string()).unwrap_or_else(|| "a step".into())
    );
    assert_eq!(owed_after, owed_before, "not handed over as a parked operation");
    assert_eq!(
        outstanding_after,
        outstanding_before.map(|outstanding| outstanding + 1),
        "answered for by its outstanding identity, carried in the settlement"
    );
}

/// One admitted, focused connection with a press delivered and its release
/// never submitted: the runner holds the press. What every exit must keep.
fn held_press(
    launched: &ProducingLaunch,
    socket_path: &std::path::Path,
    delivery: u64,
    transaction: u64,
) -> (UnixStream, Arc<PrivateEvidenceCustody>, u32, Option<[u8; 32]>) {
    launched.access.await_ready(Duration::from_secs(15)).expect("readiness");
    let (mut client, surface, sequence, custody, window) = admitted_connection(launched, socket_path, 0x0e61);
    let client_id = custody.cleanup_record().client;
    let lease = launched.owner.lease();
    let control = launched.access.control_producer(&lease).expect("the control producer");
    let (focus, focus_in) = apply_focus(launched, &control, &mut client, client_id, surface, transaction);
    assert_eq!(focus, Some(XAuthorityControlOutcome::Delivered));
    assert_eq!(focus_in, Some(expected_focus_in(sequence, window)));
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .expect("an ingress");
    ingress
        .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(delivery), 272, true))
        .expect("accepted");
    let press = read_event(&mut client, 5);
    assert_eq!(press, Some(expected_button_event(true, sequence, window, 1)), "the press was delivered");
    let cell = delivery_cell(&launched.registry, delivery);
    assert!(cell.is_some_and(|cell| waited_for(|| cell.answer().is_some())), "and answered");
    (client, custody, window, press)
}

#[test]
fn a_held_press_is_retained_exactly_through_an_ordinary_stop() {
    let (launched, socket_path) = launch_producing("producer-held-stop", 9607, 4);
    let (mut client, custody, window, _) = held_press(&launched, &socket_path, 96710, 96701);
    let client_id = custody.cleanup_record().client;
    launched.commands.send(XServerFrontendServiceCommand::StopAndDisconnect).expect("listening");
    let client_ended = eof_within(&mut client, 3);
    let registry = launched.registry.clone();
    let outcome = produced_outcome(launched, "held press, stop");
    let seen = observe_worker(&custody, &registry);
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    // THE HOLD IS THE REMAINDER: its press was delivered and answered, its
    // release never came, and the settlement carries the hold with exactly
    // this client and window, its press custody no longer pending. Nothing
    // inferred a release.
    assert_eq!(
        outcome.retained_holds,
        vec![(client_id, u64::from(window), false)],
        "the exact hold, through the settlement"
    );
    // The settlement handle went out of scope in the launch: what it could
    // not finish is with the store the owner keeps, exactly as it was.
    assert_eq!(
        outcome.store_holds,
        vec![(client_id, u64::from(window), false)],
        "and through the store after the handle is gone"
    );
    assert_collected_running(&seen, "held press, stop");
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn a_held_press_is_retained_exactly_through_a_service_error() {
    let (launched, socket_path) = launch_producing("producer-held-error", 9608, 4);
    let (mut client, custody, window, _) = held_press(&launched, &socket_path, 96810, 96801);
    let client_id = custody.cleanup_record().client;
    let (acknowledgement, acknowledged) = sync_channel(1);
    drop(acknowledged);
    launched
        .commands
        .send(XServerFrontendServiceCommand::UpdateOutputTopology {
            snapshot: sophia_protocol::OutputTopologySnapshot {
                generation: 1,
                primary: sophia_protocol::OutputId::from_raw(1),
                outputs: Vec::new(),
            },
            acknowledgement,
        })
        .expect("listening");
    let client_ended = eof_within(&mut client, 3);
    let registry = launched.registry.clone();
    let outcome = produced_outcome(launched, "held press, error");
    let seen = observe_worker(&custody, &registry);
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(false));
    let error = outcome.error.clone().expect("the loop's own error");
    assert!(error.contains("acknowledgement"), "the original error is kept: {error}");
    assert_eq!(
        outcome.retained_holds,
        vec![(client_id, u64::from(window), false)],
        "the exact hold, through the failed return's settlement"
    );
    assert_eq!(
        outcome.store_holds,
        vec![(client_id, u64::from(window), false)],
        "and through the store after the handle is gone"
    );
    assert_collected_running(&seen, "held press, error");
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn a_held_press_is_retained_exactly_through_an_unwind() {
    let service_thread = Arc::new(Mutex::new(None));
    let seen_telemetry = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(
        Arc::clone(&seen_telemetry),
        Some(XAuthorityBackpressureTelemetryKind::Wait),
        Arc::clone(&service_thread),
    );
    let (launched, socket_path) =
        launch_producing_observed("producer-held-unwind", 9609, 4, false, observer, 1, service_thread);
    let (mut client, custody, window, _) = held_press(&launched, &socket_path, 96910, 96901);
    let client_id = custody.cleanup_record().client;
    // THE UNWIND, as the egress controls arrange it: a raster requirement
    // behind a parked worker on a full transport makes the service report
    // a wait, and the observer unwinds on the service thread.
    let surface = draw_and_learn_surface(&mut client, &launched.transactions);
    assert!(
        waited_for(|| saw_kind(&seen_telemetry, XAuthorityBackpressureTelemetryKind::Wait, true)),
        "the connection worker reached its egress wait"
    );
    launched
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("the requirement is queued");
    let client_ended = eof_within(&mut client, 3);
    let registry = launched.registry.clone();
    let outcome = produced_outcome(launched, "held press, unwind");
    let seen = observe_worker(&custody, &registry);
    assert!(outcome.unwound, "the injected panic unwound the operation");
    assert!(client_ended, "the guard's Drop stopped and interrupted the connection");
    // NOTHING RETURNED, AND THE HOLD IS STILL EXACT: the unwound runner's
    // frontend handed its terminal inventory to the store the owner keeps.
    assert!(outcome.retained_holds.is_empty(), "no return carried it");
    assert_eq!(
        outcome.store_holds,
        vec![(client_id, u64::from(window), false)],
        "the exact hold, through the store"
    );
    assert_collected_running(&seen, "held press, unwind");
    let _ = std::fs::remove_file(&socket_path);
}

/// STAGE-ONLY: a frontend whose producer was already exposed, so the
/// service's runner preparation refuses. The service never reaches its
/// loop: the failure names the refusal, the settlement comes back, and the
/// access sees Ended -- never Ready, never NotReady for good.
#[test]
fn a_service_whose_runner_cannot_be_prepared_fails_before_its_loop_and_ends_its_port() {
    let socket_path = private_service_socket("producer-unprepared");
    let (transaction_sender, _transactions) = sync_channel(4);
    let (_commands, service_commands) = sync_channel(4);
    let (port, access) = PrivateProducerAccess::for_service();
    let (parts, _acks, _deliveries) = producing_parts(4);
    let durable = PrivateSettlementOwner::default();
    let owner = service_owner(&durable, 4);
    let mut private = crate::PrivateXServerFrontend::new(parts, &owner)
        .unwrap_or_else(|(refusal, _)| panic!("a frontend: {refusal:?}"));
    // The producer exposed ahead of the service: preparation refuses
    // ProducerAlreadyExposed.
    let (registration, _channels) = private
        .broker
        .registry
        .register_client_with_admission(XServerFrontendClientId::from_raw(7), Some(admitted(XServerFrontendClientId::from_raw(7))))
        .expect("registers");
    private
        .broker
        .registry
        .attach_private_lifecycle(&registration, admitted(XServerFrontendClientId::from_raw(7)))
        .expect("attaches");
    let _exposed = private
        .ingress_for(XServerFrontendClientId::from_raw(7), DeviceId::from_raw(1))
        .expect("the frontend exposes an ingress");
    let lease = owner.lease();
    let outcome = serve_private_frontend_until_stopped(
        private,
        &lease,
        private_service_config(&socket_path, NamespaceId::from_raw(9612), 4),
        transaction_sender,
        service_commands,
        port,
        Arc::new(|_| {}),
    );
    match outcome {
        Err(PrivateServiceFailure::Failed { error, workers, maintenance, order, .. }) => {
            assert!(
                error.to_string().contains("private runner could not be prepared: ProducerAlreadyExposed"),
                "the refusal is named: {error}"
            );
            assert!(workers.is_empty() && maintenance.is_empty());
            assert_eq!(*order, PrivateOrderTally::default());
        }
        Err(other) => panic!("a named failure, not {other:?}"),
        Ok(_) => panic!("a named failure, not a settlement"),
    }
    assert_eq!(access.standing(), PrivatePortStanding::Ended, "ended, not left NotReady");
    assert_eq!(access.control_producer(&lease).err(), Some(PrivateProducerRefusal::Ended));
    assert_eq!(access.await_ready(Duration::from_secs(1)).err(), Some(PrivateProducerRefusal::Ended));
    drop(registration);
    drop((owner, durable));
    let _ = std::fs::remove_file(&socket_path);
}

/// The frame paused inside the service's collection (after its registration
/// has gone, before its completion), so the collection is observably in
/// progress: an issued ingress and control producer refuse from here, and
/// the port has already ended -- producer admission is closed before
/// anything is waited for.
fn producers_refuse_during_collection(unwind: bool) {
    let service_thread = Arc::new(Mutex::new(None));
    let seen_telemetry = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(
        Arc::clone(&seen_telemetry),
        unwind.then_some(XAuthorityBackpressureTelemetryKind::Wait),
        Arc::clone(&service_thread),
    );
    let tag = if unwind { "producer-collecting-unwind" } else { "producer-collecting-error" };
    let namespace = if unwind { 9614 } else { 9613 };
    let (launched, socket_path) =
        launch_producing_observed(tag, namespace, 4, false, observer, 1, service_thread);
    launched.access.await_ready(Duration::from_secs(15)).expect("readiness");
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.registry);
    let client_id = custody.cleanup_record().client;
    let owner = Arc::clone(&launched.owner);
    let lease = owner.lease();
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .expect("an ingress");
    let control = launched.access.control_producer(&lease).expect("the control producer");
    let release = pause_after_registration_drop(&launched.registry, client_id);
    if unwind {
        let surface = draw_and_learn_surface(&mut client, &launched.transactions);
        assert!(
            waited_for(|| saw_kind(&seen_telemetry, XAuthorityBackpressureTelemetryKind::Wait, true)),
            "the connection worker reached its egress wait"
        );
        launched
            .raster
            .try_route(raster_requirement_for(surface))
            .expect("the requirement is queued");
    } else {
        // The error path: the loop returns with the frame alive, so the
        // guard's explicit collection is what stops and waits for it. (An
        // ordinary stop winds the frames down inside the loop itself, before
        // the collection; it has no such window.)
        let (acknowledgement, acknowledged) = sync_channel(1);
        drop(acknowledged);
        launched
            .commands
            .send(XServerFrontendServiceCommand::UpdateOutputTopology {
                snapshot: sophia_protocol::OutputTopologySnapshot {
                    generation: 1,
                    primary: sophia_protocol::OutputId::from_raw(1),
                    outputs: Vec::new(),
                },
                acknowledgement,
            })
            .expect("listening");
    }
    assert!(
        waited_for(|| matches!(
            custody.cleanup_record().destruction_standing(),
            PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(_))
        )),
        "the frame reached its pause inside the collection"
    );
    let standing_while_collecting = launched.access.standing();
    let submit_while_collecting = ingress
        .submit(&lease, button_to(SurfaceId::new(1, 1), XAuthorityInputDeliveryId::from_raw(96130), 272, true))
        .map(|_| ())
        .map_err(|refusal| format!("{refusal:?}"));
    let control_while_collecting = control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client: client_id,
                command: XAuthorityControlCommand::ClearFocus {
                    transaction: TransactionId::from_raw(96131),
                    surface: SurfaceId::new(1, 1),
                },
            },
        )
        .map(|_| ())
        .map_err(|(refusal, _)| format!("{refusal:?}"));
    release.send(()).expect("the frame is waiting at its pause");
    let client_ended = eof_within(&mut client, 3);
    let registry = launched.registry.clone();
    let outcome = produced_outcome(launched, tag);
    let seen = observe_worker(&custody, &registry);
    assert_eq!(standing_while_collecting, PrivatePortStanding::Ended, "the port ended first");
    assert!(submit_while_collecting.is_err(), "an issued ingress refuses during collection");
    assert!(control_while_collecting.is_err(), "an issued control producer refuses during collection");
    assert!(client_ended);
    assert_eq!(outcome.unwound, unwind);
    if !unwind {
        assert_eq!(outcome.ok, Some(false));
        let error = outcome.error.clone().expect("the loop's own error");
        assert!(error.contains("acknowledgement"), "the original error is kept: {error}");
    }
    assert_collected_running(&seen, tag);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn producers_refuse_while_the_service_collects_after_an_error() {
    producers_refuse_during_collection(false);
}

#[test]
fn producers_refuse_while_the_unwinding_guard_collects() {
    producers_refuse_during_collection(true);
}

/// The exit paused right after it closed producer admission, before it
/// stopped or waited for anything: the connection is alive, its grant
/// valid, and a submission through an ingress issued earlier is refused
/// whole -- not accepted into a service that is collecting. The port has
/// ended at the same point.
#[test]
fn a_submission_is_refused_the_moment_the_exit_closes_admission_before_anything_is_waited_for() {
    let (launched, socket_path) = launch_producing("producer-admission-closed", 9615, 4);
    launched.access.await_ready(Duration::from_secs(15)).expect("readiness");
    let (mut client, surface, sequence, custody, window) =
        admitted_connection(&launched, &socket_path, 0x0e91);
    let client_id = custody.cleanup_record().client;
    let owner = Arc::clone(&launched.owner);
    let lease = owner.lease();
    let control = launched.access.control_producer(&lease).expect("the control producer");
    let (focus, focus_in) = apply_focus(&launched, &control, &mut client, client_id, surface, 96151);
    assert_eq!(focus, Some(XAuthorityControlOutcome::Delivered));
    assert_eq!(focus_in, Some(expected_focus_in(sequence, window)));
    let ingress = launched
        .access
        .ingress_for(&lease, client_id, DeviceId::from_raw(1))
        .expect("an ingress");
    // A press before the exit, to show the same ingress accepts while
    // admission is open.
    ingress
        .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(96150), 272, true))
        .expect("accepted while open");
    assert_eq!(read_event(&mut client, 5), Some(expected_button_event(true, sequence, window, 1)));
    let release = pause_after_admission_closed(&launched.registry);
    let (acknowledgement, acknowledged) = sync_channel(1);
    drop(acknowledged);
    launched
        .commands
        .send(XServerFrontendServiceCommand::UpdateOutputTopology {
            snapshot: sophia_protocol::OutputTopologySnapshot {
                generation: 1,
                primary: sophia_protocol::OutputId::from_raw(1),
                outputs: Vec::new(),
            },
            acknowledgement,
        })
        .expect("listening");
    assert!(
        waited_for(|| !admission_pause_pending(&launched.registry)),
        "the exit reached its pause after closing admission"
    );
    let standing = launched.access.standing();
    let frame_alive = custody.cleanup_record().destruction_standing();
    let refused = ingress
        .submit(&lease, button_to(surface, XAuthorityInputDeliveryId::from_raw(96152), 272, false))
        .map(|_| ())
        .map_err(|refusal| format!("{refusal:?}"));
    let refused_control = control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client: client_id,
                command: XAuthorityControlCommand::ClearFocus {
                    transaction: TransactionId::from_raw(96153),
                    surface,
                },
            },
        )
        .map(|_| ())
        .map_err(|(refusal, _)| format!("{refusal:?}"));
    release.send(()).expect("the exit is waiting at its pause");
    let client_ended = eof_within(&mut client, 3);
    let registry = launched.registry.clone();
    let outcome = produced_outcome(launched, "admission closed");
    let seen = observe_worker(&custody, &registry);
    assert_eq!(frame_alive, PrivateDestructionStanding::NotRequested, "the frame was still alive");
    assert_eq!(standing, PrivatePortStanding::Ended, "the port ended at the same point");
    assert!(refused.is_err(), "the issued ingress refuses whole: {refused:?}");
    assert!(refused_control.is_err(), "the issued control producer refuses whole: {refused_control:?}");
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(false));
    assert_eq!(
        outcome.terminal,
        Some((1, 0, 0, 0, false)),
        "only the held press remains: nothing was accepted into the collecting service"
    );
    assert_collected_running(&seen, "admission closed");
    let _ = std::fs::remove_file(&socket_path);
}
