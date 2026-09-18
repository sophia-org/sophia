// Controls for the private routed service entry point: who owns it, and how
// it ends. Every one runs the actual entry point, or its crate-internal serve
// over a pre-built frontend, on a real listening socket with a real admitted
// connection. The owners are FINITE: a launch closure owns them, borrows them
// into the service, inspects through them after the invocation has returned,
// errored or unwound, and then lets them go -- and a control watches the
// custody graph release when they do.

/// The admission a private-socket connection gets on this service.
struct PrivateServiceAdmission(sophia_protocol::ClientAdmissionContext);

impl XServerFrontendAdmissionPolicy for PrivateServiceAdmission {
    fn admit(
        &self,
        _request: XServerFrontendAdmissionRequest,
    ) -> Result<sophia_protocol::ClientAdmissionContext, XServerFrontendAdmissionError> {
        Ok(self.0)
    }
    fn revoke(
        &self,
        _context: sophia_protocol::ClientAdmissionContext,
    ) -> Result<(), XServerFrontendAdmissionError> {
        Ok(())
    }
}

fn private_service_socket(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sophia-private-service-{tag}-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock")
            .as_nanos()
    ))
}

fn private_service_config(
    socket_path: &std::path::Path,
    namespace: NamespaceId,
    clients: usize,
) -> XServerFrontendConfig {
    XServerFrontendConfig::new(socket_path, namespace)
        .expect("a config")
        .with_max_concurrent_clients(NonZeroUsize::new(clients).expect("a bound"))
        .with_admission_policy(Arc::new(PrivateServiceAdmission(namespaced(
            XServerFrontendClientId(9300),
            namespace,
        ))))
}

fn private_service_parts(clients: usize) -> crate::PrivateFrontendParts {
    let (control_acknowledgements, _acks) = sync_channel(4);
    let (input_deliveries, _deliveries) = channel();
    let (authority, issuer, submit) = private_authority();
    // The receivers are dropped here on purpose: this service exposes no
    // producer, so nothing sends on these and nothing has to drain them.
    crate::PrivateFrontendParts {
        max_concurrent_clients: NonZeroUsize::new(clients).expect("a bound"),
        input_capacity: NonZeroUsize::new(4).expect("a bound"),
        control_acknowledgements,
        input_deliveries,
        authority,
        issuer,
        submit,
    }
}

fn connect_private_client(socket_path: &std::path::Path) -> UnixStream {
    assert!(
        waited_for(|| socket_path.exists()),
        "the service binds its listener"
    );
    let client = waited_for_value(|| UnixStream::connect(socket_path).ok())
        .expect("the listener accepts");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("a bounded read");
    client
}

/// The X11 setup handshake; returns once the setup reply's body has been
/// read, so the connection is registered and its worker is serving.
fn handshake(client: &mut UnixStream) {
    use std::io::{Read, Write};
    client
        .write_all(&[b'l', 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        .expect("the setup request is sent");
    let mut prefix = [0u8; 8];
    client.read_exact(&mut prefix).expect("a setup reply prefix");
    assert_eq!(prefix[0], 1, "setup succeeded");
    let mut body = vec![0u8; usize::from(u16::from_le_bytes([prefix[6], prefix[7]])) * 4];
    client.read_exact(&mut body).expect("the setup reply body");
}

/// A CreateWindow request: a dispatch the authority observes, so the worker
/// publishes a transaction batch for it. (NoOperation publishes nothing.)
fn create_window(client: &mut UnixStream, ordinal: u32) {
    use std::io::Write;
    let window: u32 = 0x0020_0d01 + ordinal;
    let mut out: Vec<u8> = vec![1, 24];
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&window.to_le_bytes());
    out.extend_from_slice(&0x20u32.to_le_bytes());
    out.extend_from_slice(&0i16.to_le_bytes());
    out.extend_from_slice(&0i16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    client.write_all(&out).expect("a CreateWindow request is sent");
}

fn create_gc(client: &mut UnixStream, gc: u32, drawable: u32) {
    use std::io::Write;
    let mut out: Vec<u8> = vec![55, 0];
    out.extend_from_slice(&10u16.to_le_bytes());
    out.extend_from_slice(&gc.to_le_bytes());
    out.extend_from_slice(&drawable.to_le_bytes());
    let value_mask: u32 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 14);
    out.extend_from_slice(&value_mask.to_le_bytes());
    for value in [3u32, u32::MAX, 0x00ff_ffff, 0, 0, 0] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    client.write_all(&out).expect("a CreateGC request is sent");
}

/// ImageText8: a draw the authority observes as a CPU buffer update, which
/// is what makes a later raster envelope an OBSERVED batch.
fn image_text8(client: &mut UnixStream, drawable: u32, gc: u32, text: &[u8]) {
    use std::io::Write;
    let padded = text.len().div_ceil(4) * 4;
    let mut out: Vec<u8> = vec![76, u8::try_from(text.len()).expect("short text")];
    out.extend_from_slice(&u16::try_from((16 + padded) / 4).expect("short").to_le_bytes());
    out.extend_from_slice(&drawable.to_le_bytes());
    out.extend_from_slice(&gc.to_le_bytes());
    out.extend_from_slice(&4i16.to_le_bytes());
    out.extend_from_slice(&16i16.to_le_bytes());
    out.extend_from_slice(text);
    out.resize(out.len() + (padded - text.len()), 0);
    client.write_all(&out).expect("an ImageText8 request is sent");
}

/// One bounded read: EOF within the socket's read timeout, or not. EOF is a
/// socket shutdown, not a collection; the join-order controls observe the
/// collection itself.
fn saw_eof(client: &mut UnixStream) -> bool {
    use std::io::Read;
    let mut byte = [0u8; 1];
    matches!(client.read(&mut byte), Ok(0))
}

fn eof_within(client: &mut UnixStream, reads: usize) -> bool {
    (0..reads).any(|_| saw_eof(client))
}

fn waited_for_value<T>(mut observed: impl FnMut() -> Option<T>) -> Option<T> {
    for _ in 0..3_000 {
        if let Some(value) = observed() {
            return Some(value);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}

/// The exact identity of one kept custody: its registered client, its
/// maintenance place, and the ALLOCATIONS of its cleanup record and its join
/// and exit evidence homes. Two readings that agree on every one of these
/// are readings of the same custody; a strong count could never say that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CustodyIdentity {
    client: XServerFrontendClientId,
    place: usize,
    record: usize,
    join: usize,
    exit: usize,
}

fn custody_identity(custody: &Arc<PrivateEvidenceCustody>) -> CustodyIdentity {
    CustodyIdentity {
        client: custody.cleanup_record().client,
        place: custody.identity().place(),
        record: Arc::as_ptr(custody.cleanup_record()) as usize,
        join: Arc::as_ptr(custody.join()) as usize,
        exit: Arc::as_ptr(custody.exit_sink()) as usize,
    }
}

/// The identity of the one live custody, read through the registry's keeper
/// WHILE the service runs -- the "before" of a before/after comparison.
fn live_custody_identity(registry: &XServerFrontendRouteRegistry) -> Option<CustodyIdentity> {
    let keeper = registry.custody_keeper.get()?;
    let inventory = keeper.inventory.upgrade()?;
    let kept = inventory.kept.lock().ok()?;
    kept.places.iter().flatten().next().map(custody_identity)
}

/// What the ORIGINAL owner shows after the service invocation is over.
///
/// Exact identity, not a count: the kept custody's client, place and
/// allocations, its number right, and what the store retained. The weak
/// handle is how a control watches the custody graph release once the
/// owners themselves go.
#[derive(Debug)]
struct AfterService {
    custodies_kept: usize,
    kept: Option<CustodyIdentity>,
    kept_number_right: bool,
    /// The store's shelf, read without taking: each obligation, whether its
    /// envelope still holds a batch, whether that batch was observed, and
    /// the surface it names. The work and its charge stay in the store.
    shelf: Vec<ShelvedEgress>,
    custody: Option<std::sync::Weak<PrivateEvidenceCustody>>,
    /// What the service established for the first kept custody's worker,
    /// and what its connection's destruction decided.
    attachment: Option<PrivateAttachment>,
    standing: Option<PrivateDestructionStanding>,
    /// Whether that custody's worker was joined, read while the owner lives.
    join_phase: Option<PrivateReapingPhase>,
    /// The store's failed-instance retention and its charged failure slots,
    /// read while the owner lives.
    failed_instances: Option<usize>,
    failure_slots: Option<usize>,
    /// The store's retained instances whose actor is uncollected, by places.
    uncollected_instances: Option<Vec<Vec<usize>>>,
    /// Where the first custody's deferred cleanup stands, what its fence
    /// recorded, whether its obligation is committed, its home's standing
    /// and its number's standing, all read through the owner.
    deferred_cleanup: Option<PrivateDeferredCleanupStanding>,
    fence: Option<PrivateHandoverFence>,
    committed: Option<bool>,
    home: Option<PrivateHomeStanding>,
    number: Option<Option<PrivateNumberStanding>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ShelvedEgress {
    obligation: PrivateUnresolvedEgress,
    holds_batch: bool,
    observed_batch: bool,
    surface: Option<SurfaceId>,
    /// The batch's payload shape, so a shelved raster batch is compared as
    /// what it carried and not only as present: its transaction count and
    /// its raster-response count.
    transaction_count: usize,
    raster_response_count: usize,
}

fn read_shelf(durable: &PrivateSettlementOwner) -> Vec<ShelvedEgress> {
    durable
        .with_unresolved_egress(|entries| {
            entries
                .iter()
                .map(|(instance, envelope)| ShelvedEgress {
                    obligation: PrivateUnresolvedEgress {
                        instance: *instance,
                        transaction: envelope.transaction,
                    },
                    holds_batch: envelope.batch.is_some(),
                    observed_batch: envelope.observed_batch,
                    surface: envelope
                        .batch
                        .as_ref()
                        .and_then(|batch| batch.transactions.first())
                        .map(|transaction| transaction.surface),
                    transaction_count: envelope
                        .batch
                        .as_ref()
                        .map_or(0, |batch| batch.transactions.len()),
                    raster_response_count: envelope
                        .batch
                        .as_ref()
                        .map_or(0, |batch| batch.raster_responses.len()),
                })
                .collect()
        })
        .expect("a readable store")
}

fn inspect_after(owner: &PrivateServiceOwner, durable: &PrivateSettlementOwner) -> AfterService {
    let kept = owner.inventory.kept.lock().expect("a readable inventory");
    let first = kept.places.iter().flatten().next();
    AfterService {
        custodies_kept: kept.taken,
        kept: first.map(custody_identity),
        kept_number_right: first.is_some_and(|custody| custody.cleanup_record().number.get().is_some()),
        shelf: read_shelf(durable),
        custody: first.map(Arc::downgrade),
        attachment: first.and_then(|custody| custody.attachment()),
        standing: first.map(|custody| custody.cleanup_record().destruction_standing()),
        join_phase: first.map(|custody| custody.join().phase()),
        failed_instances: durable.failed_instances(),
        failure_slots: durable.failure_slots_charged(),
        uncollected_instances: durable.uncollected_instances(),
        deferred_cleanup: first.map(|custody| custody.deferred_cleanup_standing()),
        fence: first.and_then(|custody| custody.fence_evidence().fence()),
        committed: first.map(|custody| {
            custody
                .store()
                .committed_obligation(custody.identity().index)
                .is_some()
        }),
        home: first.map(|custody| custody.cleanup_record().ordered_home.standing()),
        number: first.map(|custody| {
            custody
                .cleanup_record()
                .number
                .get()
                .map(|right| right.occupancy.state_of(custody.cleanup_record().client))
                .unwrap_or(None)
        }),
    }
}

/// A finite launch scope: this thread OWNS the store and the owner, lends
/// the owner to the service, inspects through it afterwards, and lets both
/// go when the closure returns. Nothing is made static.
fn launch<T: Send + 'static>(
    connections: usize,
    body: impl FnOnce(&PrivateSettlementOwner, &PrivateServiceOwner) -> T + Send + 'static,
) -> (std::thread::JoinHandle<T>, Receiver<()>) {
    let (done, finished) = channel();
    let handle = std::thread::spawn(move || {
        let durable = PrivateSettlementOwner::default();
        let owner = service_owner(&durable, connections);
        let result = body(&durable, &owner);
        let _ = done.send(());
        result
    });
    (handle, finished)
}

/// Join the launch thread with a bound; report, never hang.
///
/// `already_finished` is a completion that an earlier observation consumed:
/// it is honoured here so an early return is reported as an early return and
/// still joined, not waited for twice and called a hang.
fn launch_outcome<T>(
    handle: std::thread::JoinHandle<T>,
    finished: &Receiver<()>,
    already_finished: bool,
    what: &str,
) -> T {
    if !already_finished && finished.recv_timeout(Duration::from_secs(15)).is_err() {
        panic!("{what}: the launch scope did not return within the harness bound (its thread is left running)");
    }
    handle.join().expect("the launch thread returned")
}

/// Every telemetry report an observer saw: its kind and the client it named.
type SeenTelemetry =
    Arc<Mutex<Vec<(XAuthorityBackpressureTelemetryKind, Option<XServerFrontendClientId>)>>>;

/// A production-typed observer that records every telemetry kind it sees and
/// panics for one kind, only on one thread.
fn recording_observer(
    seen: SeenTelemetry,
    panic_on: Option<XAuthorityBackpressureTelemetryKind>,
    only_on: Arc<Mutex<Option<std::thread::ThreadId>>>,
) -> Arc<XAuthorityBackpressureObserver> {
    Arc::new(move |telemetry: XAuthorityBackpressureTelemetry| {
        seen.lock()
            .expect("a writable record")
            .push((telemetry.kind, telemetry.client));
        let here = only_on
            .lock()
            .ok()
            .and_then(|held| *held)
            .is_some_and(|id| id == std::thread::current().id());
        if here && panic_on.is_some_and(|kind| kind == telemetry.kind) {
            panic!("injected unwind inside the private service operation");
        }
    })
}

fn saw_kind(
    seen: &SeenTelemetry,
    kind: XAuthorityBackpressureTelemetryKind,
    with_client: bool,
) -> bool {
    seen.lock()
        .expect("a readable record")
        .iter()
        .any(|(seen, client)| *seen == kind && (!with_client || client.is_some()))
}

/// The SERVICE's own raster wait: kind Wait with no client, reported after
/// `from`. A worker's wait names its client; this is the one that does not.
fn saw_service_wait(seen: &SeenTelemetry, from: usize) -> bool {
    seen.lock()
        .expect("a readable record")
        .iter()
        .skip(from)
        .any(|(kind, client)| *kind == XAuthorityBackpressureTelemetryKind::Wait && client.is_none())
}

/// The parts of a launch the test thread needs while a pre-built frontend
/// serves: its registry (to hold a worker's ending open) and its raster router.
struct Handles {
    registry: XServerFrontendRouteRegistry,
    raster: XServerFrontendRasterRouter,
}

fn assert_kept_exactly_one(after: &AfterService) {
    assert_eq!(after.custodies_kept, 1, "the ended connection's custody is retained by the owner");
    let kept = after.kept.expect("the kept custody names its registered client");
    assert!(after.kept_number_right, "and holds that connection's number right");
    assert_eq!(kept.place, 0, "at the one place this service used");
}

/// The custody read after the invocation is the SAME custody that was
/// registered while it served: same client, same place, same cleanup record,
/// join and exit allocations. Not a count.
fn assert_same_custody(before: CustodyIdentity, after: &AfterService) {
    assert_eq!(
        after.kept,
        Some(before),
        "the original registered cleanup/join/exit allocations are what the owner keeps"
    );
}

/// The service's own account of what it left unsent agrees with the store's
/// shelf, obligation by obligation -- invocation and transaction both.
fn assert_unresolved_accounted(reported: &[PrivateUnresolvedEgress], after: &AfterService) {
    let shelved: Vec<PrivateUnresolvedEgress> =
        after.shelf.iter().map(|entry| entry.obligation).collect();
    assert_eq!(reported, shelved.as_slice());
}

fn assert_released(after: &AfterService) {
    assert!(
        after.custody.as_ref().is_some_and(|weak| weak.upgrade().is_none()),
        "the custody graph is released once its legitimate keepers have gone"
    );
}

#[test]
fn a_private_service_admits_a_real_connection_and_stops_in_order() {
    let namespace = NamespaceId::from_raw(9301);
    let socket_path = private_service_socket("stop");
    let (transaction_sender, _transactions) = sync_channel(64);
    let (commands, service_commands) = sync_channel(4);
    let config = private_service_config(&socket_path, namespace, 4);
    let (handle, finished) = launch(4, move |durable, owner| {
        let mut execution = PrivateServiceExecutionKeeper::new();
        let outcome = run_x_server_frontend_private_until_stopped(
            config,
            transaction_sender,
            private_service_parts(4),
            owner,
            &mut execution,
            service_commands,
            PrivateProducerPort::unattended(),
            Arc::new(|_| {}),
        );
        let reported = outcome.as_ref().map(|ret| ret.unresolved_egress.clone()).ok();
        (outcome.is_ok(), reported, inspect_after(owner, durable))
    });
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    let client_ended = eof_within(&mut client, 3);
    let (ok, reported, after) = launch_outcome(handle, &finished, false, "ordinary stop");
    assert!(client_ended, "the worker's socket was shut down");
    assert!(ok, "an ordinary stop returns the settlement");
    assert_kept_exactly_one(&after);
    assert_unresolved_accounted(&reported.expect("returned"), &after);
    assert!(after.shelf.is_empty());
    assert_released(&after);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn losing_the_command_channel_stops_the_private_service_in_the_same_order() {
    let namespace = NamespaceId::from_raw(9302);
    let socket_path = private_service_socket("channel-loss");
    let (transaction_sender, _transactions) = sync_channel(64);
    let (commands, service_commands) = sync_channel::<XServerFrontendServiceCommand>(4);
    let config = private_service_config(&socket_path, namespace, 4);
    let (handle, finished) = launch(4, move |durable, owner| {
        let mut execution = PrivateServiceExecutionKeeper::new();
        let outcome = run_x_server_frontend_private_until_stopped(
            config,
            transaction_sender,
            private_service_parts(4),
            owner,
            &mut execution,
            service_commands,
            PrivateProducerPort::unattended(),
            Arc::new(|_| {}),
        );
        (outcome.is_ok(), inspect_after(owner, durable))
    });
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    drop(commands);
    let client_ended = eof_within(&mut client, 3);
    let (ok, after) = launch_outcome(handle, &finished, false, "command-channel loss");
    assert!(client_ended);
    assert!(ok, "losing the command channel is an ordinary stop");
    assert_kept_exactly_one(&after);
    assert_released(&after);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_error_after_a_connection_exists_collects_a_worker_blocked_on_egress() {
    // BLOCKED EGRESS, OBSERVED, NOT DRAINED. The transport holds one item and
    // this control never reads it. The worker's blocked submission is
    // established by the production-typed observer reporting a Wait for a
    // client; only then is the error injected on the service thread, and
    // collection has to unblock the worker itself.
    let namespace = NamespaceId::from_raw(9303);
    let socket_path = private_service_socket("loop-error");
    let (transaction_sender, transactions) = sync_channel(1);
    let (commands, service_commands) = sync_channel(4);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = recording_observer(Arc::clone(&seen), None, Arc::new(Mutex::new(None)));
    let config = private_service_config(&socket_path, namespace, 4);
    let (handle, finished) = launch(4, move |durable, owner| {
        let mut execution = PrivateServiceExecutionKeeper::new();
        let outcome = run_x_server_frontend_private_until_stopped(
            config,
            transaction_sender,
            private_service_parts(4),
            owner,
            &mut execution,
            service_commands,
            PrivateProducerPort::unattended(),
            observer,
        );
        let error = outcome.err().map(|failure| format!("{failure:?}"));
        (error, inspect_after(owner, durable))
    });
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    for ordinal in 0..3 {
        create_window(&mut client, ordinal);
    }
    assert!(
        waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true)),
        "the connection's worker reached its egress wait"
    );
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
    let (error, after) = launch_outcome(handle, &finished, false, "loop error with blocked egress");
    assert!(client_ended, "the parked worker was unblocked and stopped");
    let error = error.expect("a real error inside the loop is reported");
    assert!(error.contains("acknowledgement"), "the original error is preserved: {error}");
    assert!(
        transactions.try_recv().is_ok(),
        "the transport still holds the worker's item: this control drained nothing"
    );
    assert_kept_exactly_one(&after);
    assert!(after.shelf.is_empty(), "no raster envelope was pending");
    assert_released(&after);
    let _ = std::fs::remove_file(&socket_path);
}

/// A launched service over a pre-built frontend, with what the test thread
/// drives it through.
struct Launched {
    handle: std::thread::JoinHandle<(bool, Option<String>, Vec<PrivateUnresolvedEgress>, AfterService)>,
    finished: Receiver<()>,
    handles: Handles,
    commands: SyncSender<XServerFrontendServiceCommand>,
    transactions: Receiver<XAuthorityObservedTransactionBatch>,
}

/// Start a service over a pre-built frontend and hand its registry and raster
/// router to the test thread before serving.
fn launch_held(
    socket_path: std::path::PathBuf,
    namespace: NamespaceId,
    transport_capacity: usize,
    observer: Arc<XAuthorityBackpressureObserver>,
    service_thread: Arc<Mutex<Option<std::thread::ThreadId>>>,
) -> Launched {
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
        let config = private_service_config(&socket_path, namespace, 4);
        let mut execution = PrivateServiceExecutionKeeper::new();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            serve_private_frontend_until_stopped(
                private,
                &lease,
                &mut execution,
                config,
                transaction_sender,
                service_commands,
            PrivateProducerPort::unattended(),
                observer,
            )
        }));
        let unwound = outcome.is_err();
        let (error, reported) = match outcome.ok() {
            Some(Ok(ret)) => (None, ret.unresolved_egress),
            Some(Err(PrivateServiceFailure::Failed {
                error,
                unresolved_egress,
                ..
            })) => (Some(error.to_string()), unresolved_egress),
            Some(Err(failure)) => (Some(format!("{failure:?}")), Vec::new()),
            None => (None, Vec::new()),
        };
        let after = inspect_after(owner, durable);
        (unwound, error, reported, after)
    });
    let handles = handles_in
        .recv_timeout(Duration::from_secs(15))
        .expect("the launch scope built its frontend");
    Launched {
        handle,
        finished,
        handles,
        commands,
        transactions,
    }
}

#[test]
fn the_private_adapter_serves_the_order_through_the_prepared_runner() {
    // A LABELLED ADAPTER SEAM, not a socket run. The service's order
    // adapter must be the prepared runner's own leased turn -- its budget,
    // its watchdog, its one consumer -- and not the broker's routed-input
    // drain. Nothing accepted is a turn that moved nothing; work submitted
    // through the runner's real ingress is taken by the next turn and
    // counted in the invocation's tally; a foreign lease is refused by the
    // runner's own check before anything is taken.
    let (mut runner, owner, registration, _channels, _acks, _deliveries) =
        prepared_runner_fixture();
    let lease = owner.lease();
    let mut port = PrivateProducerPort::unattended();
    let mut order = PrivateOrderTally::default();
    {
        let mut adapter = LeasedPrivateBroker {
            runner: &mut runner,
            port: &mut port,
            order: &mut order,
            service: &lease,
        };
        assert!(matches!(adapter.serve_order(), Ok(0)), "nothing accepted, nothing moved");
    }
    assert_eq!(order.turns, 1);
    assert_eq!(order.taken, 0);
    let ingress = runner
        .ingress_for(&lease, XServerFrontendClientId::from_raw(9000), DeviceId::from_raw(1))
        .expect("the runner's own producer");
    ingress
        .submit(
            &lease,
            button_to(
                SurfaceId::new(9000, 1),
                XAuthorityInputDeliveryId::from_raw(93001),
                272,
                true,
            ),
        )
        .expect("the order accepts it");
    {
        let mut adapter = LeasedPrivateBroker {
            runner: &mut runner,
            port: &mut port,
            order: &mut order,
            service: &lease,
        };
        assert!(matches!(adapter.serve_order(), Ok(1)), "the turn took it");
    }
    assert_eq!(order.turns, 2);
    assert_eq!(order.taken, 1, "the runner took the submitted work");
    let other = PrivateSettlementOwner::default();
    let foreign_owner = service_owner(&other, 4);
    let foreign = foreign_owner.lease();
    {
        let mut adapter = LeasedPrivateBroker {
            runner: &mut runner,
            port: &mut port,
            order: &mut order,
            service: &foreign,
        };
        let refused = adapter.serve_order().expect_err("a foreign lease is refused");
        assert!(
            refused.to_string().contains("kept by a different owner"),
            "the runner's own refusal: {refused}"
        );
        assert!(adapter.attach_ready().is_err());
        assert!(adapter.answer_producers().is_err());
    }
    assert_eq!(order.turns, 2, "a refused turn is not a turn");
    drop(registration);
    drop((runner.shutdown(), owner));
}

fn lifecycle_still_accepting(registry: &XServerFrontendRouteRegistry) -> bool {
    registry
        .input_recovery
        .lifecycle
        .get()
        .expect("a private frontend installs a lifecycle owner")
        .inner
        .accepting
        .load(Ordering::Acquire)
}

#[test]
fn an_error_joins_every_worker_before_the_private_frontend_is_finalised() {
    // THE JOIN, OBSERVED. EOF is a socket shutdown; what this holds open is the
    // worker's own ending, at the parent table its cleanup takes late. A
    // service that joins cannot return while that table is held, and must not
    // have finalised the private frontend either.
    let namespace = NamespaceId::from_raw(9309);
    let socket_path = private_service_socket("join-error");
    let Launched {
        handle,
        finished,
        handles,
        commands,
        transactions,
    } = launch_held(
        socket_path.clone(),
        namespace,
        1,
        Arc::new(|_| {}),
        Arc::new(Mutex::new(None)),
    );
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    // The connection's registered ordered worker is attached before the
    // error is injected, so what this collects is a started worker; its
    // home is held by the control, so the worker's own ending -- its
    // departure visit -- cannot finish while the hold stands. A connection
    // whose destruction defers touches no table by number, so the home is
    // what stands in for the parent table the legacy control held.
    let custody = wait_attached(&handles.registry);
    let held_home = custody
        .cleanup_record()
        .ordered_home
        .state
        .lock()
        .expect("a readable home");
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
    // A completion consumed here is remembered, so an early return is joined
    // and reported as what it is.
    let returned_while_held = finished.recv_timeout(Duration::from_secs(1)).is_ok();
    let accepting_while_held = lifecycle_still_accepting(&handles.registry);
    drop(held_home);
    let (unwound, error, _reported, after) =
        launch_outcome(handle, &finished, returned_while_held, "error join order");
    let occupancy_after = handles.registry.occupancy.held.lock().expect("readable").len();
    assert!(client_ended, "the worker was told to stop");
    assert!(!unwound);
    assert!(
        !returned_while_held,
        "the service does not return while a worker is still inside its ending"
    );
    assert!(
        accepting_while_held,
        "and the private frontend is not finalised before that worker is joined"
    );
    assert!(error.is_some(), "the injected error is preserved");
    // THE DEFERRED DUTY WAS DISCHARGED AFTER THE JOIN. The connection's own
    // destruction found its registered worker running and deferred; the
    // service collected that worker and then, with the join published and
    // the fence recorded, ran the deferred cleanup, whose completed
    // namespace cleanup gave the number back. The custody's join evidence is
    // what says the worker's ending completed before the service returned.
    assert_eq!(occupancy_after, 0, "{after:?}");
    assert!(
        matches!(after.deferred_cleanup, Some(PrivateDeferredCleanupStanding::Done(_))),
        "{after:?}"
    );
    assert_eq!(after.fence, Some(PrivateHandoverFence::Established), "{after:?}");
    assert_eq!(after.committed, Some(true), "the maintenance obligation is committed");
    assert_eq!(after.home, Some(PrivateHomeStanding::Retained), "the home is retained, not drained");
    assert_eq!(after.number, Some(None), "the completed namespace cleanup released the number");
    assert_eq!(after.attachment, Some(PrivateAttachment::Started));
    assert_eq!(
        after.standing,
        Some(PrivateDestructionStanding::Decided(
            PrivateDestructionDecision::Deferred(PrivateDestructionDeferral::WorkerRunning)
        ))
    );
    assert_eq!(
        after.join_phase,
        Some(PrivateReapingPhase::Joined),
        "the worker was joined before the service returned"
    );
    assert_kept_exactly_one(&after);
    drop(transactions);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_unwind_joins_every_worker_before_the_private_frontend_is_finalised() {
    let namespace = NamespaceId::from_raw(9310);
    let socket_path = private_service_socket("join-unwind");
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
    let surface = draw_and_learn_surface(&mut client, &transactions);
    assert!(waited_for(|| saw_kind(&seen, XAuthorityBackpressureTelemetryKind::Wait, true)));
    // As in the error case: the attached worker's home is held, so its
    // departure visit cannot finish while the hold stands.
    let custody = wait_attached(&handles.registry);
    let held_home = custody
        .cleanup_record()
        .ordered_home
        .state
        .lock()
        .expect("a readable home");
    handles
        .raster
        .try_route(raster_requirement_for(surface))
        .expect("the requirement is queued");
    let client_ended = eof_within(&mut client, 3);
    let returned_while_held = finished.recv_timeout(Duration::from_secs(1)).is_ok();
    let accepting_while_held = lifecycle_still_accepting(&handles.registry);
    drop(held_home);
    let (unwound, _error, _reported, after) =
        launch_outcome(handle, &finished, returned_while_held, "unwind join order");
    let occupancy_after = handles.registry.occupancy.held.lock().expect("readable").len();
    assert!(unwound, "the injected panic unwound the operation");
    assert!(client_ended, "the guard's Drop told the worker to stop");
    assert!(
        !returned_while_held,
        "the unwinding service does not finish while a worker is still inside its ending"
    );
    assert!(
        accepting_while_held,
        "and the private frontend's own fallback has not run before that worker is joined"
    );
    // As in the error case: the deferred duty was discharged after the join
    // by the unwinding guard, and the custody's join evidence says the
    // worker was joined.
    assert_eq!(occupancy_after, 0, "{after:?}");
    assert!(
        matches!(after.deferred_cleanup, Some(PrivateDeferredCleanupStanding::Done(_))),
        "{after:?}"
    );
    assert_eq!(after.fence, Some(PrivateHandoverFence::Established), "{after:?}");
    assert_eq!(after.committed, Some(true), "the maintenance obligation is committed");
    assert_eq!(after.home, Some(PrivateHomeStanding::Retained), "the home is retained, not drained");
    assert_eq!(after.number, Some(None), "the completed namespace cleanup released the number");
    assert_eq!(after.attachment, Some(PrivateAttachment::Started));
    assert_eq!(
        after.standing,
        Some(PrivateDestructionStanding::Decided(
            PrivateDestructionDecision::Deferred(PrivateDestructionDeferral::WorkerRunning)
        ))
    );
    assert_eq!(
        after.join_phase,
        Some(PrivateReapingPhase::Joined),
        "the worker was joined before the unwinding service returned"
    );
    assert_eq!(after.shelf.len(), 1, "and the unsent raster envelope is retained");
    drop(transactions);
    let _ = std::fs::remove_file(&socket_path);
}
