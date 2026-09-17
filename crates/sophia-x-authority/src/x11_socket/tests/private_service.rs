// Controls for the private routed service entry point: who owns it, and how
// it ends. Every one runs the actual entry point over a real listening socket
// with a real admitted connection; none builds a private frontend beside an
// unrelated public loop.

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
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("a bounded read");
    client
}

/// The X11 setup handshake over a connected socket; returns once the setup
/// reply's body has been read, so the connection is admitted and its worker
/// is serving.
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

/// A CreateWindow request: a dispatch the authority observes, so the
/// connection's worker publishes a transaction batch for it. (NoOperation
/// publishes nothing, and a control that sends only those never fills the
/// transport.) Window ids follow the first client's resource range, as the
/// integration controls do.
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

/// One bounded read: EOF within the socket's read timeout, or not.
/// CreateGC with the value set the integration controls use.
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

fn saw_eof(client: &mut UnixStream) -> bool {
    use std::io::Read;
    let mut byte = [0u8; 1];
    matches!(client.read(&mut byte), Ok(0))
}

/// EOF within a small number of bounded reads.
///
/// NOT INSIDE `waited_for`: each read already blocks for the socket's timeout,
/// and three thousand of those would be the hang this harness exists to
/// report instead.
fn eof_within(client: &mut UnixStream, reads: usize) -> bool {
    (0..reads).any(|_| saw_eof(client))
}

/// The owner and store a service borrows, kept for the whole process.
///
/// LEAKED ON PURPOSE. The service borrows the owner across a thread the
/// control must be able to give up on: a service that never returns is the
/// failure these controls report, and a scoped thread could never report it,
/// only hang. A `'static` owner lets the service run on an unscoped thread
/// whose completion is waited for with a bound.
fn leaked_owner(bound: usize) -> (&'static PrivateSettlementOwner, &'static PrivateServiceOwner) {
    let durable: &'static PrivateSettlementOwner = Box::leak(Box::new(PrivateSettlementOwner::default()));
    let owner: &'static PrivateServiceOwner = Box::leak(Box::new(service_owner(durable, bound)));
    (durable, owner)
}

/// Wait for the service thread with a bound; report, never hang.
///
/// On timeout the thread is left running and the failure says so, with what
/// the owner shows at that moment.
fn service_outcome<T: Send + 'static>(
    handle: std::thread::JoinHandle<T>,
    finished: &Receiver<()>,
    owner: &PrivateServiceOwner,
    what: &str,
) -> T {
    if finished.recv_timeout(Duration::from_secs(15)).is_err() {
        panic!(
            "{what}: the private service did not return within the harness bound \
             (its thread is left running; custodies_kept={})",
            owner.custodies_kept()
        );
    }
    handle.join().expect("the service thread returned")
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

#[test]
fn a_private_service_admits_a_real_connection_and_stops_in_order() {
    // THE OWNER IS THE TEST'S, NOT THE SERVICE'S. It exists before the call
    // and is inspected after the call returns, with no clone rescued from
    // inside the service.
    let namespace = NamespaceId::from_raw(9301);
    let socket_path = private_service_socket("stop");
    let (_durable, owner) = leaked_owner(4);
    let (transaction_sender, _transactions) = sync_channel(64);
    let (commands, service_commands) = sync_channel(4);
    let (done, finished) = channel();

    let config = private_service_config(&socket_path, namespace, 4);
    let service = std::thread::spawn(move || {
        let outcome = run_x_server_frontend_private_until_stopped(
            config,
            transaction_sender,
            private_service_parts(4),
            owner,
            service_commands,
            Arc::new(|_| {}),
        );
        let _ = done.send(());
        outcome
    });
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    // ADMITTED THROUGH THIS FRONTEND'S REGISTRY: its custody is kept by the
    // outer owner, observed through that owner while it serves.
    let kept_while_serving = waited_for(|| owner.custodies_kept() == 1);
    commands
        .send(XServerFrontendServiceCommand::StopAndDisconnect)
        .expect("the service is listening for commands");
    // COLLECTED BEFORE THE SERVICE RETURNS: the client sees its worker end,
    // and only then does the invocation come back with a settlement.
    let client_ended = eof_within(&mut client, 3);
    let outcome = service_outcome(service, &finished, owner, "ordinary stop");
    assert!(kept_while_serving, "the connection's custody is kept by the outer owner");
    assert!(client_ended, "the worker was stopped and collected");
    let settlement = outcome.expect("an ordinary stop returns the settlement");
    assert!(
        settlement.is_settled() || settlement.outstanding() == 0,
        "nothing was accepted for a producer this service does not expose"
    );
    // STILL INSPECTABLE THROUGH THE ORIGINAL OWNER after the service is gone:
    // the connection's evidence is retained, not disposed of with the frame.
    assert_eq!(
        owner.custodies_kept(),
        1,
        "the ended connection's custody is retained by the owner the caller kept"
    );
    let _ = std::fs::remove_file(&socket_path);
    drop(settlement);
}

#[test]
fn losing_the_command_channel_stops_the_private_service_in_the_same_order() {
    let namespace = NamespaceId::from_raw(9302);
    let socket_path = private_service_socket("channel-loss");
    let (_durable, owner) = leaked_owner(4);
    let (transaction_sender, _transactions) = sync_channel(64);
    let (commands, service_commands) = sync_channel::<XServerFrontendServiceCommand>(4);
    let (done, finished) = channel();

    let config = private_service_config(&socket_path, namespace, 4);
    let service = std::thread::spawn(move || {
        let outcome = run_x_server_frontend_private_until_stopped(
            config,
            transaction_sender,
            private_service_parts(4),
            owner,
            service_commands,
            Arc::new(|_| {}),
        );
        let _ = done.send(());
        outcome
    });
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    assert!(waited_for(|| owner.custodies_kept() == 1));
    // THE CONTROLLER GOES AWAY. Nobody sends a stop; the service treats the
    // lost channel as one.
    drop(commands);
    let client_ended = eof_within(&mut client, 3);
    let outcome = service_outcome(service, &finished, owner, "command-channel loss");
    assert!(client_ended, "the worker was stopped and collected");
    assert!(
        outcome.is_ok(),
        "losing the command channel is an ordinary stop: {:?}",
        outcome.as_ref().err()
    );
    assert_eq!(owner.custodies_kept(), 1);
    let _ = std::fs::remove_file(&socket_path);
    drop(outcome);
}

#[test]
fn an_error_after_a_connection_exists_collects_a_worker_blocked_on_egress() {
    // BLOCKED EGRESS, NOT DRAINED. The transaction transport holds one item
    // and this control never reads it, so the connection's worker parks in
    // its egress wait. The error is injected on the service thread by a
    // command whose acknowledgement nobody can receive; collection then has
    // to unblock the worker itself.
    let namespace = NamespaceId::from_raw(9303);
    let socket_path = private_service_socket("loop-error");
    let (_durable, owner) = leaked_owner(4);
    let (transaction_sender, transactions) = sync_channel(1);
    let (commands, service_commands) = sync_channel(4);
    let (done, finished) = channel();

    let config = private_service_config(&socket_path, namespace, 4);
    let service = std::thread::spawn(move || {
        let outcome = run_x_server_frontend_private_until_stopped(
            config,
            transaction_sender,
            private_service_parts(4),
            owner,
            service_commands,
            Arc::new(|_| {}),
        );
        let _ = done.send(());
        outcome
    });
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    assert!(waited_for(|| owner.custodies_kept() == 1));
    for ordinal in 0..3 {
        create_window(&mut client, ordinal);
    }
    // The transport is full and stays full.
    assert!(
        waited_for(|| transactions.try_recv().is_ok()),
        "the connection's worker published into the transport"
    );
    // A second item cannot be sent: the worker is parked, or will be.
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
    let outcome = service_outcome(service, &finished, owner, "loop error with blocked egress");
    assert!(
        client_ended,
        "the parked worker was unblocked, stopped and collected before the service returned"
    );
    let failure = outcome.err().expect("a real error inside the loop is reported");
    let PrivateServiceFailure::Failed { error, settlement } = failure else {
        panic!("the service ran: {failure:?}")
    };
    assert!(
        error.to_string().contains("acknowledgement"),
        "the original error is preserved: {error}"
    );
    drop(settlement);
    // THE TRANSPORT WAS NEVER DRAINED BY THIS CONTROL: the item it holds is
    // still there, so collection did not depend on a receiver.
    assert!(transactions.try_recv().is_ok(), "the transport still holds the worker's item");
    assert_eq!(owner.custodies_kept(), 1, "custody is inspectable after the error");
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_unwind_inside_the_private_service_still_collects_its_workers() {
    // THE UNWIND IS PRODUCTION-TYPED: the backpressure observer is a real
    // parameter, and the service thread calls it when a raster envelope has
    // to wait on a full transport. It panics only on the service thread, so a
    // worker's own wait never fires it. The test catches the unwind to
    // inspect the postcondition; nothing in the service does.
    let namespace = NamespaceId::from_raw(9304);
    let socket_path = private_service_socket("unwind");
    let (durable, owner) = leaked_owner(4);
    let (transaction_sender, transactions) = sync_channel(1);
    let (done, finished) = channel();
    let (_commands, service_commands) = sync_channel::<XServerFrontendServiceCommand>(4);
    let service_thread: Arc<Mutex<Option<std::thread::ThreadId>>> = Arc::new(Mutex::new(None));
    let panicking_on_service = {
        let service_thread = Arc::clone(&service_thread);
        Arc::new(move |telemetry: XAuthorityBackpressureTelemetry| {
            let is_service = service_thread
                .lock()
                .ok()
                .and_then(|held| *held)
                .is_some_and(|id| id == std::thread::current().id());
            if is_service && matches!(telemetry.kind, XAuthorityBackpressureTelemetryKind::Wait) {
                panic!("injected unwind inside the private service operation");
            }
        })
    };
    let private = crate::PrivateXServerFrontend::new(private_service_parts(4), owner)
        .unwrap_or_else(|(refusal, _)| panic!("a frontend over this owner: {refusal:?}"));
    let raster = private.broker.raster_router();

    let config = private_service_config(&socket_path, namespace, 4);
    let service_thread_slot = Arc::clone(&service_thread);
    // The owner is borrowed into the service thread, never moved: it stays
    // the test's, which is what makes it inspectable after the unwind.
    let service = std::thread::spawn(move || {
        *service_thread_slot.lock().expect("a writable slot") = Some(std::thread::current().id());
        let lease = owner.lease();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            serve_private_frontend_until_stopped(
                private,
                &lease,
                config,
                transaction_sender,
                service_commands,
                panicking_on_service,
            )
        }));
        let _ = done.send(());
        outcome
    });
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    assert!(waited_for(|| owner.custodies_kept() == 1));
    // A window with committed content, so that a raster requirement for it
    // is SATISFIED and its envelope carries an observed batch. This is the
    // integration controls' sequence; the transport is drained only up to the
    // batch that names the surface.
    let window: u32 = 0x0020_0d01;
    let gc: u32 = 0x0020_0d02;
    create_window(&mut client, 0);
    create_gc(&mut client, gc, window);
    image_text8(&mut client, window, gc, b"AaZz");
    let drawn = waited_for_value(|| {
        transactions
            .recv_timeout(Duration::from_millis(50))
            .ok()
            .filter(|batch| batch.cpu_buffer_updates.len() == 1 && batch.transactions.len() == 1)
    })
    .expect("the draw is observed as one CPU buffer update");
    let surface = drawn.transactions[0].surface;
    // NOW THE TRANSPORT FILLS AND STAYS FULL: two more draws, one lands and
    // one parks the worker. Nothing reads the transport from here on.
    image_text8(&mut client, window, gc, b"AaZz");
    image_text8(&mut client, window, gc, b"AaZz");
    // A harness wait for the worker to reach its park; not a product bound.
    std::thread::sleep(Duration::from_millis(300));
    // A raster requirement the service thread must submit behind the parked
    // ticket and into the full transport: its wait reports an observed batch
    // to the observer, which unwinds on that thread.
    raster
        .try_route(sophia_protocol::SurfaceRasterRequirements {
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
        })
        .expect("the requirement is queued");
    let client_ended = eof_within(&mut client, 3);
    let unwound = service_outcome(service, &finished, owner, "unwind inside the operation").is_err();
    assert!(unwound, "the injected panic unwound the service operation");
    assert!(
        client_ended,
        "the worker was collected by the guard's Drop before the private frontend was finalised"
    );
    // STILL REACHABLE THROUGH THE ORIGINAL OWNER: the unwind disposed of the
    // frame, not of the custody or the store.
    assert_eq!(owner.custodies_kept(), 1);
    assert!(
        durable.continuations_retained().is_some(),
        "the store is readable and its retained work is reachable after the unwind"
    );
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn a_lease_on_a_different_owner_is_refused_before_a_listener_is_bound() {
    // THE ASSOCIATION EXISTS FROM CONSTRUCTION. A frontend made over one
    // owner and served under another's lease is refused before anything is
    // bound, and the frontend is finalised into the owner it belongs to.
    let namespace = NamespaceId::from_raw(9305);
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

    let refused = serve_private_frontend_until_stopped(
        private,
        &owner_b.lease(),
        config,
        transaction_sender,
        service_commands,
        Arc::new(|_| {}),
    );
    let PrivateServiceFailure::Failed { error, settlement } =
        refused.err().expect("a foreign lease is refused")
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
    // A store with one failure slot admits one frontend. The second is
    // refused by the mandatory-owner constructor before anything is bound,
    // and the caller's parts come back untouched.
    let namespace = NamespaceId::from_raw(9306);
    let socket_path = private_service_socket("refused");
    let durable = PrivateSettlementOwner::with_capacity(1);
    let owner = service_owner(&durable, 4);
    let first = crate::PrivateXServerFrontend::new(private_service_parts(4), &owner)
        .unwrap_or_else(|(refusal, _)| panic!("the first frontend: {refusal:?}"));
    let (transaction_sender, _transactions) = sync_channel(4);
    let (_commands, service_commands) = sync_channel::<XServerFrontendServiceCommand>(1);
    let config = private_service_config(&socket_path, namespace, 4);

    let refused = run_x_server_frontend_private_until_stopped(
        config,
        transaction_sender,
        private_service_parts(4),
        &owner,
        service_commands,
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

/// A service started over a pre-built private frontend, with its registry
/// kept by the test so that a worker's ending can be held open from outside.
struct HeldService {
    registry: XServerFrontendRouteRegistry,
    raster: XServerFrontendRasterRouter,
    handle: std::thread::JoinHandle<
        std::thread::Result<Result<PrivateSettlement, PrivateServiceFailure>>,
    >,
    finished: Receiver<()>,
    commands: SyncSender<XServerFrontendServiceCommand>,
    transactions: Receiver<XAuthorityObservedTransactionBatch>,
}

fn held_private_service(
    owner: &'static PrivateServiceOwner,
    socket_path: &std::path::Path,
    namespace: NamespaceId,
    transport_capacity: usize,
    observer: Arc<XAuthorityBackpressureObserver>,
    service_thread: Arc<Mutex<Option<std::thread::ThreadId>>>,
) -> HeldService {
    let private = crate::PrivateXServerFrontend::new(private_service_parts(4), owner)
        .unwrap_or_else(|(refusal, _)| panic!("a frontend over this owner: {refusal:?}"));
    let registry = private.broker.registry.clone();
    let raster = private.broker.raster_router();
    let (transaction_sender, transactions) = sync_channel(transport_capacity);
    let (commands, service_commands) = sync_channel(4);
    let (done, finished) = channel();
    let config = private_service_config(socket_path, namespace, 4);
    let handle = std::thread::spawn(move || {
        *service_thread.lock().expect("a writable slot") = Some(std::thread::current().id());
        let lease = owner.lease();
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
        let _ = done.send(());
        outcome
    });
    HeldService {
        registry,
        raster,
        handle,
        finished,
        commands,
        transactions,
    }
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
    // THE JOIN, OBSERVED. A stop signal alone produces the EOF the other
    // controls see; what this one holds is the worker's own ending, at the
    // parent table its cleanup takes late. A service that joins its workers
    // cannot return while that table is held, and must not have finalised
    // the private frontend either. A service that only signalled would
    // return with the worker still inside its ending.
    let namespace = NamespaceId::from_raw(9307);
    let socket_path = private_service_socket("join-error");
    let (_durable, owner) = leaked_owner(4);
    let service = held_private_service(
        owner,
        &socket_path,
        namespace,
        1,
        Arc::new(|_| {}),
        Arc::new(Mutex::new(None)),
    );
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    assert!(waited_for(|| owner.custodies_kept() == 1));

    let parents = service
        .registry
        .window_parents
        .lock()
        .expect("a readable parent table");
    let (acknowledgement, acknowledged) = sync_channel(1);
    drop(acknowledged);
    service
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
    // The worker is stopped and its ending reaches the held table.
    let client_ended = eof_within(&mut client, 3);
    let returned_while_held = service
        .finished
        .recv_timeout(Duration::from_secs(1))
        .is_ok();
    let accepting_while_held = lifecycle_still_accepting(&service.registry);
    drop(parents);
    let outcome = service_outcome(service.handle, &service.finished, owner, "error join order")
        .expect("the service did not unwind");
    let occupancy_after = service
        .registry
        .occupancy
        .held
        .lock()
        .expect("a readable record")
        .len();

    assert!(client_ended, "the worker was told to stop");
    assert!(
        !returned_while_held,
        "the service does not return while a worker is still inside its ending"
    );
    assert!(
        accepting_while_held,
        "and the private frontend is not finalised before that worker is joined"
    );
    assert!(outcome.is_err(), "the injected error is preserved");
    assert_eq!(occupancy_after, 0, "the worker's ending completed before the service returned");
    drop(service.transactions);
    let _ = std::fs::remove_file(&socket_path);
}

#[test]
fn an_unwind_joins_every_worker_before_the_private_frontend_is_finalised() {
    // THE SAME ORDER ON THE UNWIND PATH, where nothing but the collection
    // guard's Drop can establish it.
    let namespace = NamespaceId::from_raw(9308);
    let socket_path = private_service_socket("join-unwind");
    let (_durable, owner) = leaked_owner(4);
    let service_thread: Arc<Mutex<Option<std::thread::ThreadId>>> = Arc::new(Mutex::new(None));
    let panicking_on_service = {
        let service_thread = Arc::clone(&service_thread);
        Arc::new(move |telemetry: XAuthorityBackpressureTelemetry| {
            let is_service = service_thread
                .lock()
                .ok()
                .and_then(|held| *held)
                .is_some_and(|id| id == std::thread::current().id());
            if is_service && matches!(telemetry.kind, XAuthorityBackpressureTelemetryKind::Wait) {
                panic!("injected unwind inside the private service operation");
            }
        })
    };
    let service = held_private_service(
        owner,
        &socket_path,
        namespace,
        1,
        panicking_on_service,
        service_thread,
    );
    let mut client = connect_private_client(&socket_path);
    handshake(&mut client);
    assert!(waited_for(|| owner.custodies_kept() == 1));
    let window: u32 = 0x0020_0d01;
    let gc: u32 = 0x0020_0d02;
    create_window(&mut client, 0);
    create_gc(&mut client, gc, window);
    image_text8(&mut client, window, gc, b"AaZz");
    let drawn = waited_for_value(|| {
        service
            .transactions
            .recv_timeout(Duration::from_millis(50))
            .ok()
            .filter(|batch| batch.cpu_buffer_updates.len() == 1 && batch.transactions.len() == 1)
    })
    .expect("the draw is observed as one CPU buffer update");
    let surface = drawn.transactions[0].surface;
    image_text8(&mut client, window, gc, b"AaZz");
    image_text8(&mut client, window, gc, b"AaZz");
    std::thread::sleep(Duration::from_millis(300));

    let parents = service
        .registry
        .window_parents
        .lock()
        .expect("a readable parent table");
    service
        .raster
        .try_route(sophia_protocol::SurfaceRasterRequirements {
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
        })
        .expect("the requirement is queued");
    let client_ended = eof_within(&mut client, 3);
    let returned_while_held = service
        .finished
        .recv_timeout(Duration::from_secs(1))
        .is_ok();
    let accepting_while_held = lifecycle_still_accepting(&service.registry);
    drop(parents);
    let unwound = service_outcome(service.handle, &service.finished, owner, "unwind join order")
        .is_err();
    let occupancy_after = service
        .registry
        .occupancy
        .held
        .lock()
        .expect("a readable record")
        .len();

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
    assert_eq!(occupancy_after, 0);
    drop(service.transactions);
    let _ = std::fs::remove_file(&socket_path);
}
