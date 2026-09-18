// Controls for registered ordered-output worker attachment and collection in
// the private routed service. The service-exit controls run the crate-internal
// serve over a real private frontend on a real listening socket with real
// Unix connections, the original owner kept by the launch scope through
// release and collection and inspected afterwards. The startup-transaction
// and interrupt controls in `private_worker_attachment_exits.rs` call the
// attachment and collection functions directly over a fixture registration;
// each says so.
//
// SUPPLIED CAPSULES ARE A LABELLED SEAM. The private producer that would
// address an ordered capsule to a connection is not attached yet, so a
// control takes a resolved emission from the native fixture, re-addresses it
// to the real connection's own served endpoint through the test-only seam,
// and puts it on that connection's gated queue. What the worker then does
// with it -- the frames on the wire, the completion it answers -- is the
// product's, not the control's.
//
// TEST-SIDE REACHES ARE LABELLED where they appear: the owner's inventory is
// read through the registry's keeper to find the connection's custody, and
// the served endpoint is read from the promoted home.

/// The custodies this registry published, reached test-side through the
/// registry's keeper. Same selection the service uses.
fn kept_custodies(registry: &XServerFrontendRouteRegistry) -> Vec<Arc<PrivateEvidenceCustody>> {
    let Some(keeper) = registry.custody_keeper.get() else {
        return Vec::new();
    };
    let Some(inventory) = keeper.inventory.upgrade() else {
        return Vec::new();
    };
    let kept = inventory.kept.lock().expect("a readable inventory");
    kept.places
        .iter()
        .flatten()
        .filter(|custody| custody.cleanup_record().published_by(registry))
        .map(Arc::clone)
        .collect()
}

/// Wait for a connection of this registry to have its worker started.
fn wait_attached(registry: &XServerFrontendRouteRegistry) -> Arc<PrivateEvidenceCustody> {
    waited_for_value(|| {
        kept_custodies(registry)
            .into_iter()
            .find(|custody| custody.attachment() == Some(PrivateAttachment::Started))
    })
    .unwrap_or_else(|| {
        let states: Vec<String> = kept_custodies(registry)
            .iter()
            .map(|custody| {
                format!(
                    "attachment={:?} readiness={} home={:?}",
                    custody.attachment(),
                    custody.cleanup_record().worker_readiness().is_some(),
                    custody.cleanup_record().ordered_home.standing()
                )
            })
            .collect();
        panic!("the service visits the ready connection and starts its worker: {states:?}")
    })
}

/// The endpoint the promoted home serves, read test-side from the home.
fn served_endpoint(custody: &PrivateEvidenceCustody) -> PrivateEndpointIdentity {
    let found = custody
        .cleanup_record()
        .ordered_home
        .borrow_live(|continuation| match continuation {
            PrivateOrderedContinuation::Serving { owner, .. } => {
                Some(owner.served.endpoint().clone())
            }
            PrivateOrderedContinuation::Setup { .. } => None,
        });
    match found {
        PrivateHomeBorrow::Acted(Some(endpoint)) => endpoint,
        _ => panic!("the home is promoted and serving"),
    }
}

/// A resolved capsule, re-addressed to this connection, with its own
/// answerable completion.
fn supplied_capsule(
    delivery: u64,
    custody: &PrivateEvidenceCustody,
) -> (
    XAuthorityOrderedDelivery,
    Arc<PrivateDeliveryCompletion>,
    InputRecovery,
    Receiver<XAuthorityClientInputDelivery>,
) {
    let endpoint = served_endpoint(custody);
    let client = custody.cleanup_record().client;
    let (emission, _fixture_endpoint) =
        private_native_tests::emission_and_endpoint_for_writer_fixture(delivery);
    let generation = endpoint.generation;
    let emission = emission.readdressed(client, generation, endpoint);
    let mut capsule = XAuthorityOrderedDelivery::from_emission(emission)
        .unwrap_or_else(|(refusal, _)| panic!("a delivery-bearing emission: {refusal:?}"));
    let id = capsule.delivery();
    let (recovery, receipts) = claim_fixture(id);
    let completion = recovery
        .completion_for(id)
        .expect("a readable ledger")
        .expect("the admission minted its completion");
    capsule.carry_finalizer(Arc::new(finalizer_from_held(
        &recovery,
        &completion,
        id,
        capsule.client(),
    )));
    (capsule, completion, recovery, receipts)
}

/// The bytes the worker must put on the wire for this capsule, encoded with
/// the byte order and sequence the connection published.
fn wire_bytes(capsule: &XAuthorityOrderedDelivery, readiness: &PrivateWorkerReadiness) -> Vec<u8> {
    let emission = capsule.emission();
    let sequence = readiness.sequence.load(std::sync::atomic::Ordering::SeqCst);
    (0..emission.frame_count())
        .flat_map(|index| {
            emission
                .encode_frame(index, readiness.byte_order, sequence)
                .expect("an encodable frame")
                .as_bytes()
                .to_vec()
        })
        .collect()
}

fn registry_sender(
    registry: &XServerFrontendRouteRegistry,
    client: XServerFrontendClientId,
) -> PrivateGatedOrderedSender {
    let guard = registry.clients.lock().expect("a readable registry");
    guard
        .get(&client)
        .expect("this client has a row")
        .ordered
        .clone()
}

/// Read exactly `wanted` bytes from the client within the bound.
fn read_within(client: &mut UnixStream, wanted: usize, seconds: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    client
        .set_read_timeout(Some(Duration::from_secs(seconds)))
        .expect("a bounded read");
    let mut buffer = vec![0u8; wanted];
    client.read_exact(&mut buffer).ok().map(|()| buffer)
}

/// What one connection's worker evidence says, read once.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkerSeen {
    attachment: Option<PrivateAttachment>,
    life: PrivateWorkerLife,
    handle_in_slot: bool,
    left: bool,
    exit: PrivateExitReading,
    join_phase: PrivateReapingPhase,
    join: Option<PrivateJoinKind>,
    standing: PrivateDestructionStanding,
    number: Option<PrivateNumberStanding>,
    row: bool,
    readiness: bool,
}

fn observe_worker(
    custody: &PrivateEvidenceCustody,
    registry: &XServerFrontendRouteRegistry,
) -> WorkerSeen {
    let record = custody.cleanup_record();
    let client = record.client;
    let (life, handle_in_slot) = match custody.worker_slot().lock() {
        Ok(slot) => (slot.life, slot.handle.is_some()),
        Err(poisoned) => {
            let slot = poisoned.into_inner();
            (slot.life, slot.handle.is_some())
        }
    };
    let join = custody.join().result().map(|result| match result {
        PrivateJoinResult::Returned => PrivateJoinKind::Returned,
        PrivateJoinResult::Panicked(_) => PrivateJoinKind::Panicked,
    });
    WorkerSeen {
        attachment: custody.attachment(),
        life,
        handle_in_slot,
        left: custody.exit_sink().left(),
        exit: custody.exit_sink().reading(),
        join_phase: custody.join().phase(),
        join,
        standing: record.destruction_standing(),
        number: registry.occupancy.state_of(client),
        row: registry
            .clients
            .lock()
            .map(|clients| clients.contains_key(&client))
            .unwrap_or(false),
        readiness: record.worker_readiness().is_some(),
    }
}

/// What the service invocation returned, whole, plus the owner's state after.
struct AttachedOutcome {
    unwound: bool,
    ok: Option<bool>,
    error: Option<String>,
    workers: Vec<PrivateWorkerCollection>,
    uncollected: Vec<usize>,
    /// What an explicit `shutdown()` of an `Uncollected` frontend answered,
    /// when the launch chose to call it.
    shutdown_retained: Option<RetentionSeen>,
    after: AfterService,
}

/// What the retention handle an uncollected frontend's `shutdown()` returns
/// answers, asked inside the launch scope.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RetentionSeen {
    uncollected: Vec<usize>,
    settled_before: bool,
    retried: usize,
    reclaimed: usize,
    republished: usize,
    settled_after: bool,
    terminal_outstanding: Option<usize>,
}

/// How a launch disposes of an `Uncollected` return, inside the scope.
#[derive(Clone, Copy)]
enum UncollectedDisposal {
    /// Drop the returned failure, frontend and all.
    Drop,
    /// Call the returned frontend's `shutdown()` and keep its answer.
    Shutdown,
}

struct AttachedLaunch {
    handle: std::thread::JoinHandle<AttachedOutcome>,
    finished: Receiver<()>,
    handles: Handles,
    commands: SyncSender<XServerFrontendServiceCommand>,
    transactions: Receiver<XAuthorityObservedTransactionBatch>,
}

/// Run the crate-internal serve over a real private frontend on a launch
/// thread that owns the store and the service owner, and keep the whole
/// return.
fn launch_attached(
    socket_path: std::path::PathBuf,
    namespace: NamespaceId,
    transport_capacity: usize,
    observer: Arc<XAuthorityBackpressureObserver>,
    service_thread: Arc<Mutex<Option<std::thread::ThreadId>>>,
) -> AttachedLaunch {
    launch_attached_with(
        socket_path,
        namespace,
        transport_capacity,
        observer,
        service_thread,
        false,
        UncollectedDisposal::Drop,
    )
}

/// The same, choosing whether every connection gets its own admission id.
fn launch_attached_with(
    socket_path: std::path::PathBuf,
    namespace: NamespaceId,
    transport_capacity: usize,
    observer: Arc<XAuthorityBackpressureObserver>,
    service_thread: Arc<Mutex<Option<std::thread::ThreadId>>>,
    distinct_admissions: bool,
    disposal: UncollectedDisposal,
) -> AttachedLaunch {
    let (transaction_sender, transactions) = sync_channel(transport_capacity);
    let (commands, service_commands) = sync_channel(4);
    let (handles_out, handles_in) = channel();
    let (handle, finished) = launch(4, move |durable, owner| {
        *service_thread.lock().expect("a writable slot") = Some(std::thread::current().id());
        let private = crate::PrivateXServerFrontend::new(private_service_parts(4), owner)
            .unwrap_or_else(|(refusal, _)| panic!("a frontend over this owner: {refusal:?}"));
        let _ = handles_out.send(Handles {
            registry: private.broker.registry.clone(),
            raster: private.broker.raster_router(),
        });
        let lease = owner.lease();
        let config = if distinct_admissions {
            distinct_config(&socket_path, namespace, 4)
        } else {
            private_service_config(&socket_path, namespace, 4)
        };
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            serve_private_frontend_until_stopped(
                private,
                &lease,
                config,
                transaction_sender,
                service_commands,
                observer,
            )
        }));
        let unwound = outcome.is_err();
        let mut shutdown_retained = None;
        let (ok, error, workers, uncollected) = match outcome.ok() {
            Some(Ok(ret)) => (Some(true), None, ret.workers, Vec::new()),
            Some(Err(PrivateServiceFailure::Failed { error, workers, .. })) => {
                (Some(false), Some(error.to_string()), workers, Vec::new())
            }
            Some(Err(PrivateServiceFailure::Uncollected {
                error,
                workers,
                uncollected,
                frontend,
                collection_failures,
                ..
            })) => {
                // THE RETURNED FRONTEND IS DISPOSED OF HERE, inside the scope,
                // with the owner alive: dropped, or shut down explicitly.
                match disposal {
                    UncollectedDisposal::Drop => drop(frontend),
                    UncollectedDisposal::Shutdown => {
                        let mut handle = frontend.shutdown();
                        let settled_before = handle.is_settled();
                        let retried = handle.retry();
                        let reclaimed = handle.reclaim_outstanding();
                        let republished = handle.republish_owed_acknowledgements();
                        shutdown_retained = Some(RetentionSeen {
                            uncollected: handle.uncollected().to_vec(),
                            settled_before,
                            retried,
                            reclaimed,
                            republished,
                            settled_after: handle.is_settled(),
                            terminal_outstanding: handle.terminal_outstanding(),
                        });
                    }
                }
                (
                    Some(false),
                    Some(format!("uncollected: {error:?} {collection_failures:?}")),
                    workers,
                    uncollected,
                )
            }
            Some(Err(failure)) => (Some(false), Some(format!("{failure:?}")), Vec::new(), Vec::new()),
            None => (None, None, Vec::new(), Vec::new()),
        };
        let after = inspect_after(owner, durable);
        AttachedOutcome {
            unwound,
            ok,
            error,
            workers,
            uncollected,
            shutdown_retained,
            after,
        }
    });
    let handles = handles_in
        .recv_timeout(Duration::from_secs(15))
        .expect("the launch scope built its frontend");
    AttachedLaunch {
        handle,
        finished,
        handles,
        commands,
        transactions,
    }
}

fn quiet_launch(tag: &str, namespace: u64) -> (AttachedLaunch, std::path::PathBuf) {
    let socket_path = private_service_socket(tag);
    let launched = launch_attached(
        socket_path.clone(),
        NamespaceId::from_raw(namespace),
        64,
        Arc::new(|_| {}),
        Arc::new(Mutex::new(None)),
    );
    (launched, socket_path)
}

/// What every collected worker must show after the service returned: joined
/// through the custody, its handle handed to that join, its destruction
/// deferred as a running worker, its number still held and its row still
/// the custodian's.
fn assert_collected_running(seen: &WorkerSeen, what: &str) {
    assert_eq!(seen.attachment, Some(PrivateAttachment::Started), "{what}");
    assert_eq!(seen.life, PrivateWorkerLife::HandedToJoiner, "{what}: handed to the join");
    assert!(!seen.handle_in_slot, "{what}: the join took the handle");
    assert_eq!(seen.join_phase, PrivateReapingPhase::Joined, "{what}: joined");
    assert_eq!(seen.join, Some(PrivateJoinKind::Returned), "{what}: the frame returned");
    assert!(seen.left, "{what}: the body left its mark");
    assert_eq!(
        seen.standing,
        PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(
            PrivateDestructionDeferral::WorkerRunning
        )),
        "{what}: the connection's own destruction deferred over its running worker"
    );
    assert_eq!(seen.number, Some(PrivateNumberStanding::Held), "{what}: the number stays");
    assert!(seen.row, "{what}: the row is the custodian's to remove");
}

fn one_collected(outcome: &AttachedOutcome, what: &str) -> PrivateWorkerCollection {
    assert_eq!(outcome.workers.len(), 1, "{what}: one attached worker was collected");
    assert!(outcome.uncollected.is_empty(), "{what}: nothing was left uncollected");
    let collected = outcome.workers[0];
    assert!(collected.joined, "{what}: the collection joined it");
    assert_eq!(collected.join, Some(PrivateJoinKind::Returned), "{what}");
    collected
}

#[test]
fn a_real_connections_worker_delivers_supplied_capsules_on_its_own_wire_and_is_collected_at_stop() {
    let (launched, socket_path) = quiet_launch("attach-deliver", 9401);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.handles.registry);
    let client_id = custody.cleanup_record().client;
    let readiness = custody
        .cleanup_record()
        .worker_readiness()
        .expect("the connection published its readiness");
    assert_eq!(readiness.byte_order, XByteOrder::LittleEndian, "the handshake's order");
    let (capsule, completion, _recovery, _receipts) = supplied_capsule(94010, &custody);
    let expected = wire_bytes(&capsule, readiness);
    assert!(!expected.is_empty(), "the capsule encodes to something");
    // The producer's own protocol: a wake armed before the send and published
    // when it is dropped, so the worker looks again.
    let sender = registry_sender(&launched.handles.registry, client_id);
    let notice = sender.arm_wake();
    gated_send(&sender, capsule).expect("the connection's open endpoint");
    drop(notice);
    let on_the_wire = read_within(&mut client, expected.len(), 5);
    let answered = waited_for(|| completion.answer().is_some());
    let live = observe_worker(&custody, &launched.handles.registry);
    // The no-wire diagnostic is gathered here and raised only after the
    // service has been stopped and collected below.
    let no_wire_diagnostic = if on_the_wire.is_none() {
        let refusal = custody
            .cleanup_record()
            .ordered_home
            .borrow_live(|continuation| match continuation {
                PrivateOrderedContinuation::Serving { owner, .. } => Some((
                    owner.refused.as_ref().map(|refused| refused.cause),
                    owner.in_flight.is_some(),
                    owner.unterminated,
                    owner.closing.is_some(),
                )),
                PrivateOrderedContinuation::Setup { .. } => None,
            });
        let refusal = match refusal {
            PrivateHomeBorrow::Acted(found) => format!("{found:?}"),
            _ => "home not readable".to_owned(),
        };
        Some(format!("nothing on the wire: worker={live:?} home={refusal}"))
    } else {
        None
    };
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "attached stop");
    let seen = observe_worker(&custody, &launched.handles.registry);
    if let Some(diagnostic) = no_wire_diagnostic {
        panic!("{diagnostic}");
    }
    assert_eq!(on_the_wire.as_deref(), Some(expected.as_slice()), "the exact frames arrived");
    assert!(answered, "and the worker answered the capsule's completion");
    assert_eq!(live.life, PrivateWorkerLife::Running, "the worker was live while it served");
    assert!(live.handle_in_slot && !live.left);
    assert_eq!(
        live.standing,
        PrivateDestructionStanding::NotRequested,
        "the connection was not destroyed while it served"
    );
    assert!(client_ended, "the connection's socket was shut down");
    assert!(!outcome.unwound);
    assert_eq!(outcome.ok, Some(true), "an ordinary stop returns: {:?}", outcome.error);
    let collected = one_collected(&outcome, "stop");
    assert_eq!(collected.place, custody.identity().index);
    assert_collected_running(&seen, "stop");
    assert!(
        matches!(
            seen.exit,
            PrivateExitReading::Classified(PrivateWorkerOutcome {
                trigger: PrivateWorkerTrigger::Stopped,
                ..
            })
        ),
        "the body classified its own exit as the stop it was told: {:?}",
        seen.exit
    );
    assert_eq!(outcome.after.custodies_kept, 1, "the custody stays with the owner");
    assert!(outcome.after.kept_number_right);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn losing_the_command_channel_collects_the_attached_worker() {
    let (launched, socket_path) = quiet_launch("attach-channel-loss", 9402);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.handles.registry);
    drop(launched.commands);
    let client_ended = eof_within(&mut client, 3);
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "attached channel loss");
    let seen = observe_worker(&custody, &launched.handles.registry);
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    one_collected(&outcome, "channel loss");
    assert_collected_running(&seen, "channel loss");
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn a_loop_error_after_a_worker_started_collects_it_and_keeps_the_error() {
    let (launched, socket_path) = quiet_launch("attach-error", 9403);
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    let custody = wait_attached(&launched.handles.registry);
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
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "attached error");
    let seen = observe_worker(&custody, &launched.handles.registry);
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(false));
    let error = outcome.error.clone().expect("the loop's own error is reported");
    assert!(error.contains("acknowledgement"), "the original error is kept: {error}");
    one_collected(&outcome, "error");
    assert_collected_running(&seen, "error");
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_unwind_after_a_worker_started_collects_it_through_the_guard() {
    let namespace = NamespaceId::from_raw(9404);
    let socket_path = private_service_socket("attach-unwind");
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
    let surface = draw_and_learn_surface(&mut client, &launched.transactions);
    assert!(
        waited_for(|| saw_kind(&seen_telemetry, XAuthorityBackpressureTelemetryKind::Wait, true)),
        "the connection worker reached its egress wait"
    );
    launched
        .handles
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("the requirement is queued");
    let client_ended = eof_within(&mut client, 3);
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "attached unwind");
    let seen = observe_worker(&custody, &launched.handles.registry);
    assert!(outcome.unwound, "the injected panic unwound the operation");
    assert!(client_ended, "the guard's Drop stopped and interrupted the connection");
    // The unwind returns nothing, so the collection's report is read from
    // the custody the owner still keeps.
    assert_collected_running(&seen, "unwind");
    assert_eq!(outcome.after.custodies_kept, 1);
    let _ = std::fs::remove_file(&socket_path);
}

/// A big-endian setup request, and the reply read in that order.
fn handshake_big_endian(client: &mut UnixStream) {
    use std::io::{Read, Write};
    client
        .write_all(&[b'B', 0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 0])
        .expect("the setup request is sent");
    let mut prefix = [0u8; 8];
    client.read_exact(&mut prefix).expect("a setup reply prefix");
    assert_eq!(prefix[0], 1, "setup succeeded");
    let mut body = vec![0u8; usize::from(u16::from_be_bytes([prefix[6], prefix[7]])) * 4];
    client.read_exact(&mut body).expect("the setup reply body");
}

/// GetInputFocus in big-endian order; its reply carries the request's
/// sequence number, read from the wire.
fn get_input_focus_big_endian(client: &mut UnixStream) -> u16 {
    use std::io::{Read, Write};
    client
        .write_all(&[43, 0, 0, 1])
        .expect("a GetInputFocus request is sent");
    let mut reply = [0u8; 32];
    client.read_exact(&mut reply).expect("a GetInputFocus reply");
    assert_eq!(reply[0], 1, "a reply, not an error");
    u16::from_be_bytes([reply[2], reply[3]])
}

#[test]
fn a_big_endian_connection_with_an_established_sequence_gets_frames_the_wire_itself_predicts() {
    let (launched, socket_path) = quiet_launch("attach-big-endian", 9419);
    let mut client = connect_private_client(&socket_path);
    handshake_big_endian(&mut client);
    // THE SEQUENCE IS ESTABLISHED ON THE WIRE: three requests with replies,
    // the last reply naming its own sequence number in the negotiated order.
    let mut last_sequence = 0;
    for _ in 0..3 {
        last_sequence = get_input_focus_big_endian(&mut client);
    }
    let custody = wait_attached(&launched.handles.registry);
    let client_id = custody.cleanup_record().client;
    let (capsule, completion, _recovery, _receipts) = supplied_capsule(94190, &custody);
    // THE EXPECTATION IS INDEPENDENT OF THE READINESS: the order the
    // handshake asked for and the sequence the wire reported.
    let emission = capsule.emission();
    let expected: Vec<u8> = (0..emission.frame_count())
        .flat_map(|index| {
            emission
                .encode_frame(index, XByteOrder::BigEndian, last_sequence)
                .expect("an encodable frame")
                .as_bytes()
                .to_vec()
        })
        .collect();
    let little_endian_shape: Vec<u8> = (0..emission.frame_count())
        .flat_map(|index| {
            emission
                .encode_frame(index, XByteOrder::LittleEndian, last_sequence)
                .expect("an encodable frame")
                .as_bytes()
                .to_vec()
        })
        .collect();
    let sender = registry_sender(&launched.handles.registry, client_id);
    let notice = sender.arm_wake();
    gated_send(&sender, capsule).expect("the connection's open endpoint");
    drop(notice);
    let on_the_wire = read_within(&mut client, expected.len(), 5);
    let answered = waited_for(|| completion.answer().is_some());
    launched
        .commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let outcome = launch_outcome(launched.handle, &launched.finished, false, "big-endian stop");
    let seen = observe_worker(&custody, &launched.handles.registry);
    assert_eq!(last_sequence, 3, "the wire reported the third request's sequence");
    assert_ne!(expected, little_endian_shape, "the two orders encode differently");
    assert_eq!(
        on_the_wire.as_deref(),
        Some(expected.as_slice()),
        "the frames arrived in the handshake's order at the wire's sequence"
    );
    assert!(answered);
    assert!(client_ended);
    assert_eq!(outcome.ok, Some(true), "{:?}", outcome.error);
    one_collected(&outcome, "big-endian stop");
    assert_collected_running(&seen, "big-endian stop");
    let _ = std::fs::remove_file(&socket_path);
}
