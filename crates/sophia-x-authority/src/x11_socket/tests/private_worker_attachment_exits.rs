// Controls for the harder exits of registered ordered-output worker
// attachment: a worker blocked in a wire write, a second service origin and a
// sibling under one owner, the visit's refusals at the registered startup
// transaction, a worker collection could not join, and the retention that
// follows. Harness in `private_worker_attachment.rs`. The controls that call
// `attach_ready_workers` / `stop_attached_workers` / `collect_attached_workers`
// directly run them over a fixture registration, not the service; the
// service-level controls say which entry point they use.

/// Whether the home is held right now -- by the worker's serving visit,
/// which holds it across the blocked write. Read with `try_lock`, never by
/// borrowing: a borrow would wait behind the very visit it is asking about.
fn home_held(custody: &PrivateEvidenceCustody) -> bool {
    custody.cleanup_record().ordered_home.state.try_lock().is_err()
}

/// Supply capsules until the connection's queue stays full: the worker is
/// then blocked writing to a peer that does not read.
///
/// THE CAPSULES ARE BUILT BEFORE ANY IS SENT. Building one takes a whole
/// native fixture, and a blocked send has its own bound; sending a batch
/// already built keeps the interval between the first blocked frame and
/// the observation well inside that bound. Count and time are harness
/// limits, not product ones.
/// Returns how many were accepted, whether the queue stayed full, and the
/// completions and recoveries behind every capsule made, so what was never
/// answered can be read after the service has gone.
fn fill_until_blocked(
    registry: &XServerFrontendRouteRegistry,
    custody: &PrivateEvidenceCustody,
    first_delivery: u64,
) -> (usize, bool, Vec<(Arc<PrivateDeliveryCompletion>, InputRecovery)>) {
    let client = custody.cleanup_record().client;
    let sender = registry_sender(registry, client);
    let mut kept: Vec<(Arc<PrivateDeliveryCompletion>, InputRecovery)> = Vec::new();
    let mut batch = Vec::new();
    for offset in 0..360u64 {
        let (capsule, completion, recovery, _receipts) =
            supplied_capsule(first_delivery + offset, custody);
        kept.push((completion, recovery));
        batch.push(capsule);
    }
    let mut sent = 0;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    for capsule in batch {
        let mut capsule = Some(capsule);
        let mut full_since: Option<std::time::Instant> = None;
        loop {
            if std::time::Instant::now() > deadline {
                return (sent, false, kept);
            }
            let notice = sender.arm_wake();
            match gated_send(&sender, capsule.take().expect("held")) {
                Ok(()) => {
                    drop(notice);
                    break;
                }
                Err(std::sync::mpsc::TrySendError::Full(back)) => {
                    drop(notice);
                    capsule = Some(back);
                    let since = *full_since.get_or_insert_with(std::time::Instant::now);
                    if since.elapsed() > Duration::from_millis(400) {
                        // Full and staying full: the worker is not draining.
                        return (sent, true, kept);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    return (sent, false, kept);
                }
            }
        }
        sent += 1;
    }
    (sent, false, kept)
}

#[test]
fn a_service_stop_collects_a_worker_blocked_in_a_wire_write() {
    let (launched, socket_path) = quiet_launch("attach-blocked-write", 9405);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.handles.registry);
    let registry = &launched.handles.registry;
    // THE WIRE FILLS FIRST. The client reads nothing, so the worker's frames
    // pile up until a send blocks inside its frame, holding the output.
    let (sent, blocked, cells) = fill_until_blocked(registry, &custody, 94050);
    let while_blocked = observe_worker(&custody, registry);
    let held = home_held(&custody);
    // The connection thread stays alive: its own teardown flushes under the
    // same output mutex the blocked write holds, so it cannot end first. The
    // service stop below is what has to reach the worker.
    let stopped_at = std::time::Instant::now();
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "blocked write");
    let took = stopped_at.elapsed();
    let _ = client.shutdown(std::net::Shutdown::Both);
    let seen = observe_worker(&custody, registry);
    assert!(
        blocked,
        "the queue stayed full behind a blocked write after {sent} capsules: {while_blocked:?}"
    );
    assert_eq!(while_blocked.life, PrivateWorkerLife::Running);
    assert!(
        !while_blocked.left,
        "the worker was alive, blocked (after {sent} capsules): {while_blocked:?}"
    );
    assert!(held, "the home, and with it the output, was held by the blocked visit");
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    let collected = one_collected(&outcome, "blocked write");
    assert_eq!(collected.place, custody.identity().index);
    assert!(
        took < Duration::from_secs(4),
        "the interrupt freed the write at once, not after the blocked-send bound: {took:?}"
    );
    assert_collected_running(&seen, "blocked write");
    // THE ACCEPTED ORDERED WORK IS RETAINED, NOT ANSWERED: the namespace
    // cleanup gave the number back, and the capsules the worker accepted but
    // never delivered stay in the retained home with their completions
    // unanswered. Nothing drained them, nothing answered for them.
    let unanswered = cells
        .iter()
        .filter(|(completion, _)| completion.answer().is_none())
        .count();
    assert!(
        unanswered > 0 && unanswered <= sent,
        "accepted work stays unanswered after the cleanup: {unanswered} of {sent}"
    );
    assert_eq!(seen.home, Some(PrivateHomeStanding::Retained), "in its original home");
    // Kept as before: the cells are the fixture's to hold, never dropped
    // into a recovery answer the control did not make.
    std::mem::forget(cells);
    assert!(
        matches!(
            seen.exit,
            PrivateExitReading::Classified(PrivateWorkerOutcome {
                trigger: PrivateWorkerTrigger::OwnerStep,
                last: Some(PrivateWorkerAsk::Said(X11OrderedServeStep::Ended {
                    outcome: XAuthorityInputDeliveryOutcome::WriteFailed,
                    ..
                })),
            })
        ),
        "the write failed under the interrupt rather than timing out: {:?}",
        seen.exit
    );
    drop(client);
    let _ = std::fs::remove_file(&socket_path);
}

/// An admission policy that admits every connection under its own admission
/// id: the frontend refuses one admission attached to two workers, so a
/// control with two connections on one service needs this.
struct DistinctAdmission {
    namespace: NamespaceId,
    next: std::sync::atomic::AtomicU64,
}

impl XServerFrontendAdmissionPolicy for DistinctAdmission {
    fn admit(
        &self,
        _request: XServerFrontendAdmissionRequest,
    ) -> Result<sophia_protocol::ClientAdmissionContext, XServerFrontendAdmissionError> {
        let ordinal = self.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(namespaced(XServerFrontendClientId(9300 + ordinal), self.namespace))
    }
    fn revoke(
        &self,
        _context: sophia_protocol::ClientAdmissionContext,
    ) -> Result<(), XServerFrontendAdmissionError> {
        Ok(())
    }
}

fn distinct_config(
    socket_path: &std::path::Path,
    namespace: NamespaceId,
    clients: usize,
) -> XServerFrontendConfig {
    XServerFrontendConfig::new(socket_path, namespace)
        .expect("a config")
        .with_max_concurrent_clients(NonZeroUsize::new(clients).expect("a bound"))
        .with_admission_policy(Arc::new(DistinctAdmission {
            namespace,
            next: std::sync::atomic::AtomicU64::new(0),
        }))
}

/// Two services over one owner and store, each on its own socket, run on
/// scoped threads inside the launch scope so both borrow the same lease.
/// (A returned ok, B returned ok, custodies kept, A's collected places, B's).
type TwoOriginsOutcome = (Option<bool>, Option<bool>, usize, Vec<usize>, Vec<usize>);

struct TwoOrigins {
    handle: std::thread::JoinHandle<TwoOriginsOutcome>,
    finished: Receiver<()>,
    registries: (XServerFrontendRouteRegistry, XServerFrontendRouteRegistry),
    commands: (
        SyncSender<XServerFrontendServiceCommand>,
        SyncSender<XServerFrontendServiceCommand>,
    ),
}

fn launch_two_origins(
    socket_a: std::path::PathBuf,
    socket_b: std::path::PathBuf,
) -> TwoOrigins {
    let (commands_a, service_commands_a) = sync_channel(4);
    let (commands_b, service_commands_b) = sync_channel(4);
    let (registries_out, registries_in) = channel();
    let (handle, finished) = launch(6, move |_durable, owner| {
        let private_a = crate::PrivateXServerFrontend::new(private_service_parts(3), owner)
            .unwrap_or_else(|(refusal, _)| panic!("origin A: {refusal:?}"));
        let private_b = crate::PrivateXServerFrontend::new(private_service_parts(3), owner)
            .unwrap_or_else(|(refusal, _)| panic!("origin B: {refusal:?}"));
        let _ = registries_out.send((
            private_a.broker.registry.clone(),
            private_b.broker.registry.clone(),
        ));
        let lease = owner.lease();
        let run = |private, socket: &std::path::Path, namespace, commands| {
            let (transaction_sender, _transactions) = sync_channel(64);
            let mut execution = PrivateServiceExecutionKeeper::new();
            serve_private_frontend_until_stopped(
                private,
                &lease,
                &mut execution,
                distinct_config(socket, NamespaceId::from_raw(namespace), 3),
                transaction_sender,
                commands,
            PrivateProducerPort::unattended(),
                Arc::new(|_| {}),
            )
        };
        let (outcome_a, outcome_b) = std::thread::scope(|scope| {
            let a = scope.spawn(|| run(private_a, &socket_a, 9406, service_commands_a));
            let b = scope.spawn(|| run(private_b, &socket_b, 9407, service_commands_b));
            (a.join().expect("origin A returns"), b.join().expect("origin B returns"))
        });
        let places = |outcome: &Result<PrivateServiceReturn, PrivateServiceFailure>| match outcome {
            Ok(ret) => ret.workers.iter().map(|w| w.place).collect::<Vec<_>>(),
            Err(PrivateServiceFailure::Failed { workers, .. }) => {
                workers.iter().map(|w| w.place).collect()
            }
            Err(_) => Vec::new(),
        };
        (
            Some(outcome_a.is_ok()),
            Some(outcome_b.is_ok()),
            owner.custodies_kept(),
            places(&outcome_a),
            places(&outcome_b),
        )
    });
    let registries = registries_in
        .recv_timeout(Duration::from_secs(15))
        .expect("both origins built");
    TwoOrigins {
        handle,
        finished,
        registries,
        commands: (commands_a, commands_b),
    }
}

#[test]
fn stopping_one_origin_leaves_a_sibling_origin_on_the_same_owner_serving() {
    let socket_a = private_service_socket("attach-origin-a");
    let socket_b = private_service_socket("attach-origin-b");
    let two = launch_two_origins(socket_a.clone(), socket_b.clone());
    let (registry_a, registry_b) = &two.registries;
    let mut first_a = connect_private_client(&socket_a);
    handshake(&mut first_a);
    let custody_a1 = wait_attached(registry_a);
    let mut second_a = connect_private_client(&socket_a);
    handshake(&mut second_a);
    let custody_a2 = waited_for_value(|| {
        kept_custodies(registry_a).into_iter().find(|custody| {
            custody.attachment() == Some(PrivateAttachment::Started)
                && !Arc::ptr_eq(custody, &custody_a1)
        })
    })
    .unwrap_or_else(|| {
        let states: Vec<String> = kept_custodies(registry_a)
            .iter()
            .map(|custody| {
                format!(
                    "client={:?} attachment={:?} readiness={}",
                    custody.cleanup_record().client,
                    custody.attachment(),
                    custody.cleanup_record().worker_readiness().is_some()
                )
            })
            .collect();
        panic!("the sibling connection on origin A gets its own worker: {states:?}")
    });
    let mut client_b = connect_private_client(&socket_b);
    handshake(&mut client_b);
    let custody_b = wait_attached(registry_b);
    assert_eq!(kept_custodies(registry_a).len(), 2, "origin A's own custodies");
    assert_eq!(kept_custodies(registry_b).len(), 1, "origin B's own custody");
    two.commands
        .0
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("origin A is listening");
    let a1_ended = eof_within(&mut first_a, 3);
    let a2_ended = eof_within(&mut second_a, 3);
    assert!(
        waited_for(|| custody_a1.join().phase() == PrivateReapingPhase::Joined
            && custody_a2.join().phase() == PrivateReapingPhase::Joined),
        "origin A collected both of its workers"
    );
    // B'S WORK IS UNTOUCHED: its worker is still live, its wire still open,
    // and a capsule supplied now still goes out through that worker.
    let b_during = observe_worker(&custody_b, registry_b);
    let readiness_b = custody_b
        .cleanup_record()
        .worker_readiness()
        .expect("B published its readiness");
    let (capsule, completion, _recovery, _receipts) = supplied_capsule(94070, &custody_b);
    let expected = wire_bytes(&capsule, readiness_b);
    let sender = registry_sender(registry_b, custody_b.cleanup_record().client);
    let notice = sender.arm_wake();
    gated_send(&sender, capsule).expect("B's open endpoint");
    drop(notice);
    let on_b_wire = read_within(&mut client_b, expected.len(), 5);
    let b_answered = waited_for(|| completion.answer().is_some());
    two.commands
        .1
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("origin B is listening");
    let b_ended = eof_within(&mut client_b, 3);
    let (ok_a, ok_b, kept, places_a, places_b) =
        launch_outcome(two.handle, &two.finished, false, "two origins");
    let seen_b = observe_worker(&custody_b, registry_b);
    assert!(a1_ended && a2_ended, "origin A's connections were shut down");
    assert_eq!(b_during.life, PrivateWorkerLife::Running, "B's worker was untouched by A's stop");
    assert!(!b_during.left && b_during.handle_in_slot);
    assert_eq!(b_during.attachment, Some(PrivateAttachment::Started));
    assert_eq!(
        b_during.deferred,
        PrivateDeferredCleanupStanding::NotVisited,
        "A's exit visited none of B's custodies"
    );
    assert_eq!(b_during.standing, PrivateDestructionStanding::NotRequested);
    assert_eq!(b_during.number, Some(PrivateNumberStanding::Held));
    assert!(b_during.row && !b_during.gate_fenced && !b_during.committed, "{b_during:?}");
    assert_eq!(on_b_wire.as_deref(), Some(expected.as_slice()), "B still delivers");
    assert!(b_answered);
    assert!(b_ended);
    assert_eq!((ok_a, ok_b), (Some(true), Some(true)));
    assert_eq!(kept, 3, "every custody stays with the owner");
    let mut expected_a = vec![custody_a1.identity().index, custody_a2.identity().index];
    expected_a.sort_unstable();
    let mut places_a = places_a;
    places_a.sort_unstable();
    assert_eq!(places_a, expected_a, "A collected exactly its own two");
    assert_eq!(places_b, vec![custody_b.identity().index], "B collected exactly its own");
    assert_collected_running(&seen_b, "B at its own stop");
    let _ = std::fs::remove_file(&socket_a);
    let _ = std::fs::remove_file(&socket_b);
}

/// A worker fixture connection made visitable: readiness published test-side
/// over the fixture's own bound, promoted home.
fn visitable(f: &PrivateWorkerFixture) {
    let interrupt = f
        ._output
        .lock()
        .expect("the fixture's output")
        .try_clone()
        .expect("an independent handle");
    assert!(f.fixture.registration.publish_worker_readiness(PrivateWorkerReadiness {
        byte_order: XByteOrder::LittleEndian,
        sequence: Arc::new(AtomicU16::new(0)),
        stop: Arc::clone(&f.stop),
        interrupt,
    }));
}

fn fixture_frontend(f: &PrivateWorkerFixture) -> &crate::PrivateXServerFrontend {
    f.fixture.runner.frontend.as_ref().expect("a live runner")
}

#[test]
fn a_visit_does_not_start_a_departed_connection_and_does_not_retry() {
    let f = worker_fixture(XServerFrontendClientId(9408));
    visitable(&f);
    let custody = custody_for(&f, &f.fixture.keeper);
    assert_eq!(
        custody.depart_registered(),
        PrivateDeparted::Decided(PrivateDeparture::NothingStarted)
    );
    let lease = f.fixture.keeper.lease();
    let started = attach_ready_workers(fixture_frontend(&f), &lease);
    let first = custody.attachment();
    let again = attach_ready_workers(fixture_frontend(&f), &lease);
    assert_eq!(started, 0);
    assert_eq!(
        first,
        Some(PrivateAttachment::Refused(PrivateAttachmentRefusal::Startup(
            PrivateStartupOutcome::NoLongerStartable
        ))),
        "the registered startup refused a departed connection"
    );
    assert_eq!(
        custody.worker_slot().lock().expect("a readable slot").life,
        PrivateWorkerLife::NeverStarted,
        "nothing was spawned"
    );
    assert_eq!(again, 0, "a second visit does not retry");
    assert_eq!(custody.attachment(), first, "and does not rewrite the answer");
    let (workers, uncollected) = collect_attached_workers(&lease, &fixture_frontend(&f).broker.registry);
    assert!(workers.is_empty() && uncollected.is_empty(), "nothing to collect");
    drop(custody);
}

#[test]
fn a_permit_refused_after_the_spawn_leaves_a_handle_the_collection_joins() {
    let f = worker_fixture(XServerFrontendClientId(9409));
    visitable(&f);
    let custody = custody_for(&f, &f.fixture.keeper);
    // STAGE-ONLY: a holder unwinds inside the notice, so the startup
    // transaction spawns its thread and is then refused its permit.
    let wake = Arc::clone(&f.wake);
    let poisoner = std::thread::spawn(move || {
        let _inside = wake.state.lock().expect("an open notice");
        panic!("a holder unwound inside the notice");
    });
    assert!(poisoner.join().is_err());
    let lease = f.fixture.keeper.lease();
    let started = attach_ready_workers(fixture_frontend(&f), &lease);
    let attachment = custody.attachment();
    let (life, handle) = {
        let slot = custody.worker_slot().lock().expect("a readable slot");
        (slot.life, slot.handle.is_some())
    };
    let registry = &fixture_frontend(&f).broker.registry;
    let failures = stop_attached_workers(&lease, registry);
    let (workers, uncollected) = collect_attached_workers(&lease, registry);
    assert_eq!(started, 0, "no worker counts as started");
    assert_eq!(
        attachment,
        Some(PrivateAttachment::Refused(PrivateAttachmentRefusal::Startup(
            PrivateStartupOutcome::PermitRefused
        )))
    );
    assert_eq!(life, PrivateWorkerLife::Running, "but a thread was spawned into the slot");
    assert!(handle, "and its handle was retained there");
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(workers.len(), 1, "collection found the refused start's thread");
    assert!(workers[0].joined, "and joined it");
    assert_eq!(workers[0].join, Some(PrivateJoinKind::Returned));
    assert!(uncollected.is_empty());
    assert_eq!(
        custody.worker_slot().lock().expect("a readable slot").life,
        PrivateWorkerLife::HandedToJoiner
    );
    drop(custody);
}

#[test]
fn a_started_worker_is_not_respawned_by_a_later_visit() {
    let f = worker_fixture(XServerFrontendClientId(9410));
    f.permit();
    visitable(&f);
    let custody = custody_for(&f, &f.fixture.keeper);
    let lease = f.fixture.keeper.lease();
    let started = attach_ready_workers(fixture_frontend(&f), &lease);
    let thread = custody
        .worker_slot()
        .lock()
        .expect("a readable slot")
        .handle
        .as_ref()
        .map(|handle| handle.thread().id());
    let again = attach_ready_workers(fixture_frontend(&f), &lease);
    let thread_again = custody
        .worker_slot()
        .lock()
        .expect("a readable slot")
        .handle
        .as_ref()
        .map(|handle| handle.thread().id());
    let registry = &fixture_frontend(&f).broker.registry;
    let failures = stop_attached_workers(&lease, registry);
    let (workers, uncollected) = collect_attached_workers(&lease, registry);
    assert_eq!(started, 1);
    assert_eq!(custody.attachment(), Some(PrivateAttachment::Started));
    assert_eq!(again, 0, "the second visit starts nothing");
    assert!(thread.is_some());
    assert_eq!(thread_again, thread, "the same handle stays in the slot");
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(workers.len(), 1);
    assert!(workers[0].joined);
    assert!(uncollected.is_empty());
    drop(custody);
}

#[test]
fn a_worker_whose_handle_went_elsewhere_leaves_the_service_unfinalised_and_reported() {
    let (launched, socket_path) = quiet_launch("attach-uncollected", 9411);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.handles.registry);
    // STAGE-ONLY: a joiner elsewhere takes the handle before the service
    // collects. The service's reaping then finds it handed on.
    let handoff = hand_worker_to_joiner(custody.worker_slot());
    let handle = handoff.handle.expect("the handle was in the slot");
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "uncollected");
    // The service stopped it through the custody's bound pair, so the
    // joiner that took the handle can collect it now.
    handle.join().expect("the worker returned");
    let seen = observe_worker(&custody, &launched.handles.registry);
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(false));
    let error = outcome.error.clone().expect("the return says why");
    assert!(error.starts_with("uncollected: None"), "not finalised, service error kept separate: {error}");
    assert_eq!(outcome.uncollected, vec![custody.identity().index]);
    assert_eq!(outcome.workers.len(), 1);
    assert!(!outcome.workers[0].joined, "the collection did not join it");
    assert_eq!(outcome.workers[0].join, None);
    assert_eq!(seen.life, PrivateWorkerLife::HandedToJoiner);
    assert_eq!(seen.join_phase, PrivateReapingPhase::NotBegun, "no custody join was published");
    // THE DEFERRED DUTY IS OWED, NOT DISCHARGED: handed on without this
    // custody's join, the visit refuses and touches nothing, and the joiner
    // elsewhere collecting the handle lifts none of it.
    assert_owed_for_want_of_a_join(&outcome, &seen, custody.identity().index, "handed on");
    assert_eq!(outcome.after.custodies_kept, 1, "the actor stays admitted in the owner's custody");
    // THE RETURN WAS DROPPED INSIDE THE SCOPE, and the instance was retained
    // rather than settled: one failed instance in the store, its failure
    // slot still charged.
    assert_eq!(outcome.after.failed_instances, Some(1), "retained as a failed instance");
    assert_eq!(outcome.after.failure_slots, Some(1), "with its instance charge kept");
    assert_eq!(
        outcome.after.uncollected_instances,
        Some(vec![vec![custody.identity().index]]),
        "and with its reason in the store"
    );
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn shutting_down_an_uncollected_frontend_retains_it_rather_than_settling() {
    let socket_path = private_service_socket("attach-uncollected-shutdown");
    let launched = launch_attached_with(
        socket_path.clone(),
        NamespaceId::from_raw(9415),
        64,
        Arc::new(|_| {}),
        Arc::new(Mutex::new(None)),
        false,
        UncollectedDisposal::Shutdown,
    );
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.handles.registry);
    // STAGE-ONLY: a joiner elsewhere takes the handle before the service
    // collects.
    let handle = hand_worker_to_joiner(custody.worker_slot())
        .handle
        .expect("the handle was in the slot");
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "uncollected shutdown");
    handle.join().expect("the worker returned");
    let seen = observe_worker(&custody, &launched.handles.registry);
    assert!(client_ended);
    assert_eq!(outcome.uncollected, vec![custody.identity().index]);
    let place = custody.identity().index;
    assert_owed_for_want_of_a_join(&outcome, &seen, place, "uncollected shutdown");
    assert_eq!(
        outcome.shutdown_retained,
        Some(RetentionSeen {
            uncollected: vec![place],
            settled_before: false,
            retried: 0,
            reclaimed: 0,
            republished: 0,
            settled_after: false,
            terminal_outstanding: Some(0),
        }),
        "shutdown answered with a retention that names the place, is not settled, and drives, reclaims and republishes nothing"
    );
    assert_eq!(outcome.after.uncollected_instances, Some(vec![vec![place]]));
    assert_eq!(outcome.after.failed_instances, Some(1));
    assert_eq!(outcome.after.failure_slots, Some(1));
    assert_eq!(outcome.after.custodies_kept, 1);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_unwind_over_an_uncollected_actor_retains_the_instance_rather_than_settling() {
    let namespace = NamespaceId::from_raw(9416);
    let socket_path = private_service_socket("attach-uncollected-unwind");
    let service_thread = Arc::new(Mutex::new(None));
    let seen_telemetry = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(
        Arc::clone(&seen_telemetry),
        Some(XAuthorityBackpressureTelemetryKind::Wait),
        Arc::clone(&service_thread),
    );
    let launched = launch_attached(socket_path.clone(), namespace, 1, observer, service_thread);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.handles.registry);
    // STAGE-ONLY: a joiner elsewhere takes the handle before the unwind.
    let handle = hand_worker_to_joiner(custody.worker_slot())
        .handle
        .expect("the handle was in the slot");
    let surface = draw_and_learn_surface(&mut client, &launched.transactions);
    assert!(waited_for(|| saw_kind(
        &seen_telemetry,
        XAuthorityBackpressureTelemetryKind::Wait,
        true
    )));
    launched
        .handles
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("the requirement is queued");
    let client_ended = eof_within(&mut client, 3);
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "uncollected unwind");
    handle.join().expect("the worker returned");
    let seen = observe_worker(&custody, &launched.handles.registry);
    assert!(outcome.unwound, "the injected panic unwound the operation");
    assert!(client_ended, "the guard's Drop stopped and interrupted the connection");
    assert_eq!(seen.life, PrivateWorkerLife::HandedToJoiner);
    assert_eq!(seen.join_phase, PrivateReapingPhase::NotBegun, "the guard could not join it");
    // The unwind returns nothing; the guard's own visit left its refusal
    // on the custody, where the owner reads it.
    assert_eq!(
        seen.deferred,
        PrivateDeferredCleanupStanding::Refused {
            refusal: PrivateDeferredCleanupRefusal::JoinUnpublished,
            progress: PrivateDeferredCleanupProgress::default(),
        }
    );
    assert_collected_but_owed(&seen, "uncollected unwind");
    // NOTHING WAS RETURNED, AND STILL NOTHING WAS SETTLED OVER THE ACTOR: the
    // unwinding frontend's own disposal retained the instance.
    assert_eq!(outcome.after.failed_instances, Some(1), "retained as a failed instance");
    assert_eq!(outcome.after.failure_slots, Some(1), "with its instance charge kept");
    assert_eq!(
        outcome.after.uncollected_instances,
        Some(vec![vec![custody.identity().index]]),
        "and with its reason in the store"
    );
    assert_eq!(outcome.after.custodies_kept, 1);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn two_connections_on_one_origin_each_get_their_own_worker_and_are_both_collected() {
    let socket_path = private_service_socket("attach-two-connections");
    let launched = launch_attached_with(
        socket_path.clone(),
        NamespaceId::from_raw(9412),
        64,
        Arc::new(|_| {}),
        Arc::new(Mutex::new(None)),
        true,
        UncollectedDisposal::Drop,
    );
    let mut first = connect_private_client(&socket_path);
    handshake(&mut first);
    let custody_1 = wait_attached(&launched.handles.registry);
    let mut second = connect_private_client(&socket_path);
    handshake(&mut second);
    let found = waited_for_value(|| {
        kept_custodies(&launched.handles.registry)
            .into_iter()
            .find(|custody| {
                custody.attachment() == Some(PrivateAttachment::Started)
                    && !Arc::ptr_eq(custody, &custody_1)
            })
    });
    let custody_2 = match found {
        Some(custody) => custody,
        None => {
            let states: Vec<String> = kept_custodies(&launched.handles.registry)
                .iter()
                .map(|custody| {
                    format!(
                        "client={:?} attachment={:?} readiness={}",
                        custody.cleanup_record().client,
                        custody.attachment(),
                        custody.cleanup_record().worker_readiness().is_some()
                    )
                })
                .collect();
            let keeper = launched.handles.registry.custody_keeper.get().is_some();
            let inventory = launched
                .handles
                .registry
                .custody_keeper
                .get()
                .is_some_and(|keeper| keeper.inventory.upgrade().is_some());
            let _ = launched
                .commands
                .send(XServerFrontendServiceCommand::StopAndDisconnect);
            let outcome = launch_outcome(launched.handle, &launched.finished, false, "diagnose");
            panic!(
                "the second connection gets its own worker: {states:?} keeper={keeper} inventory={inventory} unwound={} ok={:?} error={:?}",
                outcome.unwound, outcome.ok, outcome.error
            )
        }
    };
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let first_ended = eof_within(&mut first, 3);
    let second_ended = eof_within(&mut second, 3);
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "two connections");
    let seen_1 = observe_worker(&custody_1, &launched.handles.registry);
    let seen_2 = observe_worker(&custody_2, &launched.handles.registry);
    assert!(first_ended && second_ended);
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    assert_eq!(outcome.workers.len(), 2);
    assert!(outcome.workers.iter().all(|worker| worker.joined));
    let mut places: Vec<usize> = outcome.workers.iter().map(|worker| worker.place).collect();
    places.sort_unstable();
    let mut expected = vec![custody_1.identity().index, custody_2.identity().index];
    expected.sort_unstable();
    assert_eq!(places, expected);
    assert_collected_running(&seen_1, "first");
    assert_collected_running(&seen_2, "second");
    let mut visited: Vec<usize> = outcome
        .maintenance
        .iter()
        .map(|visit| {
            assert!(visit.result.is_ok(), "each discharged its own: {visit:?}");
            visit.place
        })
        .collect();
    visited.sort_unstable();
    assert_eq!(visited, expected, "one visit per custody, by its own place");
    assert_eq!(outcome.after.custodies_kept, 2);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn the_owned_interrupt_frees_a_registered_worker_blocked_in_a_write_the_home_holds() {
    let f = worker_fixture(XServerFrontendClientId(9413));
    f.permit();
    visitable(&f);
    let custody = custody_for(&f, &f.fixture.keeper);
    let lease = f.fixture.keeper.lease();
    let frontend = fixture_frontend(&f);
    let registry = &frontend.broker.registry;
    assert_eq!(attach_ready_workers(frontend, &lease), 1, "the fixture's worker starts");
    // THE PEER READS NOTHING, so the worker's frames pile up until a send
    // blocks inside its frame, holding the home's output.
    let (sent, blocked, cells) = fill_until_blocked(registry, &custody, 94130);
    // Kept as before: the fixture's to hold, dropped into no answer.
    std::mem::forget(cells);
    let while_blocked = observe_worker(&custody, registry);
    let held = home_held(&custody);
    // THE COLLECTION'S OWN STOP AND INTERRUPT, and nothing else: no connection
    // thread, no legacy writer shutdown, no frontend-held handle exists here.
    let interrupted_at = std::time::Instant::now();
    let failures = stop_attached_workers(&lease, registry);
    let freed = waited_for(|| custody.exit_sink().left());
    let took = interrupted_at.elapsed();
    let (workers, uncollected) = collect_attached_workers(&lease, registry);
    let seen = observe_worker(&custody, registry);
    assert!(blocked, "the queue stayed full behind a blocked write after {sent} capsules: {while_blocked:?}");
    assert!(!while_blocked.left, "blocked, not gone: {while_blocked:?}");
    assert!(held, "the home, and with it the output, was held by the blocked visit");
    assert!(failures.is_empty(), "{failures:?}");
    assert!(freed, "the interrupt freed the worker");
    assert!(
        took < Duration::from_secs(3),
        "freed at once, not after the blocked-send bound: {took:?}"
    );
    assert_eq!(workers.len(), 1);
    assert!(workers[0].joined && uncollected.is_empty());
    assert_eq!(seen.life, PrivateWorkerLife::HandedToJoiner);
    assert!(
        matches!(
            seen.exit,
            PrivateExitReading::Classified(PrivateWorkerOutcome {
                trigger: PrivateWorkerTrigger::OwnerStep,
                last: Some(PrivateWorkerAsk::Said(X11OrderedServeStep::Ended {
                    outcome: XAuthorityInputDeliveryOutcome::WriteFailed,
                    ..
                })),
            })
        ),
        "the write failed under the interrupt rather than timing out: {:?}",
        seen.exit
    );
    drop(custody);
}

#[test]
fn a_visit_refuses_a_readiness_whose_stop_is_not_the_homes() {
    let f = worker_fixture(XServerFrontendClientId(9414));
    f.permit();
    // STAGE-ONLY: readiness published with a stop that is not the one the
    // fixture's transport was bound with.
    let interrupt = f
        ._output
        .lock()
        .expect("the fixture's output")
        .try_clone()
        .expect("an independent handle");
    assert!(f.fixture.registration.publish_worker_readiness(PrivateWorkerReadiness {
        byte_order: XByteOrder::LittleEndian,
        sequence: Arc::new(AtomicU16::new(0)),
        stop: Arc::new(AtomicBool::new(false)),
        interrupt,
    }));
    let custody = custody_for(&f, &f.fixture.keeper);
    let lease = f.fixture.keeper.lease();
    let started = attach_ready_workers(fixture_frontend(&f), &lease);
    assert_eq!(started, 0);
    assert_eq!(
        custody.attachment(),
        Some(PrivateAttachment::Refused(PrivateAttachmentRefusal::ForeignStop)),
        "a worker is not started on a stop the home does not answer to"
    );
    assert_eq!(
        custody.worker_slot().lock().expect("a readable slot").life,
        PrivateWorkerLife::NeverStarted
    );
    drop(custody);
}

/// The launch for the recovery probe: drop the `Uncollected` return, then
/// ask the store's public recovery before the external joiner joins, and
/// again after.
/// (first recovery, store before the join, second recovery, store after).
type RecoveryProbe = (Option<usize>, AfterService, Option<usize>, AfterService);

fn launch_recovering(
    socket_path: std::path::PathBuf,
    namespace: NamespaceId,
    joined: Arc<AtomicBool>,
) -> (
    std::thread::JoinHandle<RecoveryProbe>,
    Receiver<()>,
    Handles,
    SyncSender<XServerFrontendServiceCommand>,
) {
    let (transaction_sender, _transactions) = sync_channel(64);
    let (commands, service_commands) = sync_channel(4);
    let (handles_out, handles_in) = channel();
    let (handle, finished) = launch(4, move |durable, owner| {
        let private = crate::PrivateXServerFrontend::new(private_service_parts(4), owner)
            .unwrap_or_else(|(refusal, _)| panic!("a frontend over this owner: {refusal:?}"));
        let _ = handles_out.send(Handles {
            registry: private.broker.registry.clone(),
            raster: private.broker.raster_router(),
        });
        let lease = owner.lease();
        let config = private_service_config(&socket_path, namespace, 4);
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
        assert!(
            matches!(outcome, Err(PrivateServiceFailure::Uncollected { .. })),
            "the staged handoff leaves the actor uncollected"
        );
        // THE RETURN IS DROPPED HERE, frontend and all.
        drop(outcome);
        // Ordinary recovery, asked while the actor is still uncollected.
        let first = durable.recover_failed();
        let before = inspect_after(owner, durable);
        // The external joiner joins now (the control does it); then ordinary
        // recovery is asked again. A join elsewhere is not collection through
        // the custody, so the retention still stands.
        assert!(
            waited_for(|| joined.load(std::sync::atomic::Ordering::Acquire)),
            "the control joins the handle"
        );
        let second = durable.recover_failed();
        let after = inspect_after(owner, durable);
        (first, before, second, after)
    });
    let handles = handles_in
        .recv_timeout(Duration::from_secs(15))
        .expect("the launch scope built its frontend");
    (handle, finished, handles, commands)
}

#[test]
fn ordinary_recovery_leaves_an_instance_retained_over_an_uncollected_actor_standing() {
    let socket_path = private_service_socket("attach-uncollected-recover");
    let joined = Arc::new(AtomicBool::new(false));
    let (handle, finished, handles, commands) =
        launch_recovering(socket_path.clone(), NamespaceId::from_raw(9417), Arc::clone(&joined));
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&handles.registry);
    let place = custody.identity().index;
    // STAGE-ONLY: a joiner elsewhere takes the handle before the service
    // collects.
    let taken = hand_worker_to_joiner(custody.worker_slot())
        .handle
        .expect("the handle was in the slot");
    commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    // The launch scope has asked recovery once by now (or will, before it
    // waits on this); join the external handle and let it ask again.
    taken.join().expect("the worker returned");
    joined.store(true, std::sync::atomic::Ordering::Release);
    let (first, before, second, after) =
        launch_outcome(handle, &finished, false, "recovery over uncollected");
    assert!(client_ended);
    assert_eq!(first, Some(0), "recovery recovered nothing from the standing instance");
    assert_eq!(before.failed_instances, Some(1), "the instance still stands");
    assert_eq!(before.failure_slots, Some(1), "its charge is still held");
    assert_eq!(before.uncollected_instances, Some(vec![vec![place]]), "with its reason");
    assert_eq!(second, Some(0), "a join elsewhere does not make it recoverable");
    assert_eq!(after.failed_instances, Some(1));
    assert_eq!(after.failure_slots, Some(1));
    assert_eq!(after.uncollected_instances, Some(vec![vec![place]]));
    assert_eq!(after.custodies_kept, 1);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_unreadable_slot_is_collected_as_uncollected_rather_than_skipped() {
    let f = worker_fixture(XServerFrontendClientId(9418));
    f.permit();
    visitable(&f);
    let custody = custody_for(&f, &f.fixture.keeper);
    // STAGE-ONLY: a holder unwinds inside the worker slot before the service
    // visits, so the startup transaction cannot read it.
    let poisoner = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _inside = custody.worker_slot().lock().expect("a readable slot");
                panic!("a holder unwound inside the worker slot");
            })
            .join()
    });
    assert!(poisoner.is_err());
    let lease = f.fixture.keeper.lease();
    let frontend = fixture_frontend(&f);
    let registry = &frontend.broker.registry;
    let started = attach_ready_workers(frontend, &lease);
    let attachment = custody.attachment();
    let failures = stop_attached_workers(&lease, registry);
    let (workers, uncollected) = collect_attached_workers(&lease, registry);
    assert_eq!(started, 0);
    assert_eq!(
        attachment,
        Some(PrivateAttachment::Refused(PrivateAttachmentRefusal::Startup(
            PrivateStartupOutcome::Unreadable
        )))
    );
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(workers.len(), 1, "the unreadable slot was selected, not skipped");
    assert!(!workers[0].joined);
    assert!(workers[0].slot_poisoned, "and the reaping says why nothing was established");
    assert_eq!(workers[0].reaped, PrivateReaped::NothingStarted);
    assert_eq!(uncollected, vec![custody.identity().index], "so finalisation is not authorised");
    drop(custody);
}
