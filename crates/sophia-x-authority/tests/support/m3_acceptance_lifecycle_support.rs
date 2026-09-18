use super::*;

struct Actor {
    origin: usize,
    thread: std::thread::ThreadId,
    kind: &'static str,
    joined: bool,
}

static TRACKED_ORIGINS: Mutex<Vec<usize>> = Mutex::new(Vec::new());
static ACTORS: Mutex<Vec<Actor>> = Mutex::new(Vec::new());

pub(crate) fn actor_started(
    registry: &XServerFrontendRouteRegistry,
    thread: std::thread::ThreadId,
    kind: &'static str,
) {
    let origin = Arc::as_ptr(&registry.clients) as usize;
    if !TRACKED_ORIGINS.lock().unwrap().contains(&origin) {
        return;
    }
    let mut actors = ACTORS.lock().unwrap();
    if let Some(old) = actors.iter().find(|actor| actor.thread == thread) {
        assert_eq!((old.origin, old.kind), (origin, kind));
        return;
    }
    actors.push(Actor {
        origin,
        thread,
        kind,
        joined: false,
    });
}

pub(crate) fn actor_joined(thread: std::thread::ThreadId) {
    if let Some(actor) = ACTORS
        .lock()
        .unwrap()
        .iter_mut()
        .find(|actor| actor.thread == thread)
    {
        assert!(!actor.joined, "actor joined only once");
        actor.joined = true;
    }
}

pub(crate) fn writers_started(
    writers: &X11ClientWriters,
    registry: Option<&XServerFrontendRouteRegistry>,
) {
    let Some(registry) = registry else {
        return;
    };
    for (thread, kind) in [
        (
            writers
                .input
                .as_ref()
                .map(|writer| writer.thread.thread().id()),
            "legacy-input",
        ),
        (
            writers
                .control
                .as_ref()
                .map(|writer| writer.thread.thread().id()),
            "legacy-control",
        ),
        (
            writers
                .protocol
                .as_ref()
                .map(|writer| writer.thread.thread().id()),
            "legacy-protocol",
        ),
    ] {
        if let Some(thread) = thread {
            actor_started(registry, thread, kind);
        }
    }
}

fn collected_actors(registry: &XServerFrontendRouteRegistry) -> Vec<String> {
    let origin = Arc::as_ptr(&registry.clients) as usize;
    let mut actors = ACTORS.lock().unwrap();
    let mut evidence = Vec::new();
    actors.retain(|actor| {
        if actor.origin != origin {
            return true;
        }
        assert!(
            actor.joined,
            "actual started actor remains uncollected: {} {:?}",
            actor.kind, actor.thread
        );
        evidence.push(format!(
            "{}:{:?}:origin={origin:x}",
            actor.kind, actor.thread
        ));
        false
    });
    drop(actors);
    TRACKED_ORIGINS
        .lock()
        .unwrap()
        .retain(|candidate| *candidate != origin);
    evidence
}

pub(super) struct Pause {
    entered: SyncSender<std::thread::ThreadId>,
    release: Receiver<()>,
}

pub(super) struct Release {
    entered: Receiver<std::thread::ThreadId>,
    release: Option<SyncSender<()>>,
}

impl Pause {
    pub(super) fn pair() -> (Self, Release) {
        let (entered, observed) = sync_channel(1);
        let (release, held) = sync_channel(1);
        (
            Self {
                entered,
                release: held,
            },
            Release {
                entered: observed,
                release: Some(release),
            },
        )
    }

    pub(super) fn wait(self) {
        self.entered.send(std::thread::current().id()).unwrap();
        self.release
            .recv_timeout(Duration::from_secs(5))
            .expect("bounded fault seam released");
    }
}

impl Release {
    pub(super) fn entered(&self) -> std::thread::ThreadId {
        self.entered
            .recv_timeout(Duration::from_secs(5))
            .expect("actual source reached fault seam")
    }

    pub(super) fn release(mut self) {
        self.release
            .take()
            .unwrap()
            .send(())
            .expect("waiting source");
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.try_send(());
        }
    }
}

#[derive(Debug)]
pub(super) struct PanicIdentity(pub(super) u64);

pub(super) enum AttachFault {
    SpawnRefused,
    PermitRefused,
    DuringSpawn(Pause),
    Body {
        pause: Pause,
        panic: Option<Arc<PanicIdentity>>,
    },
}

type BodyFault = (usize, Pause, Option<Arc<PanicIdentity>>);
static ATTACH_FAULTS: Mutex<Vec<(usize, AttachFault)>> = Mutex::new(Vec::new());
static BODY_FAULTS: Mutex<Vec<BodyFault>> = Mutex::new(Vec::new());

pub(crate) fn acceptance_start<S>(
    context: &PrivateControlContext<'_>,
    custody: &PrivateEvidenceCustody,
    spawn: S,
) -> PrivateStartupOutcome
where
    S: FnOnce() -> std::io::Result<std::thread::JoinHandle<()>>,
{
    let key = Arc::as_ptr(&custody.cleanup_record().clients) as usize;
    let fault = {
        let mut faults = ATTACH_FAULTS.lock().unwrap();
        faults
            .iter()
            .position(|(candidate, _)| *candidate == key)
            .map(|i| faults.remove(i).1)
    };
    let outcome = match fault {
        None => context.start(spawn),
        Some(AttachFault::SpawnRefused) => {
            context.start(|| Err(std::io::Error::other("labelled acceptance spawn refusal")))
        }
        Some(AttachFault::PermitRefused) => {
            // The actual notice is poisoned before the actual spawn. Startup
            // must retain the real handle when its later permit fails.
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _held = context.notice().state.lock().unwrap();
                    panic!("labelled acceptance notice poison");
                }))
                .is_err()
            );
            context.start(spawn)
        }
        Some(AttachFault::DuringSpawn(pause)) => context.start(|| {
            pause.wait();
            spawn()
        }),
        Some(AttachFault::Body { pause, panic }) => {
            BODY_FAULTS.lock().unwrap().push((
                Arc::as_ptr(&custody.cleanup_record().ordered_home) as usize,
                pause,
                panic,
            ));
            context.start(spawn)
        }
    };
    // THE ORIGIN IS ANSWERED FIRST, AND AN UNTRACKED ONE IS NOT TOUCHED.
    //
    // Another fixture deliberately poisons this slot to establish that an
    // unreadable one is collected rather than skipped. Locking the slot here
    // before asking whose it was panicked inside that fixture's own start and
    // changed the outcome it was measuring: an observer that alters what it
    // observes is not one. Nothing below runs for an origin no acceptance case
    // is watching.
    let origin = Arc::as_ptr(&custody.cleanup_record().clients) as usize;
    if TRACKED_ORIGINS.lock().unwrap().contains(&origin) {
        // THE ACTUAL HANDLE, and nothing in its place. A poisoned slot still
        // holds the handle the start retained -- poisoning says a holder
        // unwound, not that the contents are gone -- so this reads it rather
        // than either panicking or recording a thread nobody looked at. A
        // permit refused after the spawn still owns one, which is why this
        // does not ask what the outcome was.
        let slot = custody
            .worker_slot()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(handle) = slot.handle.as_ref() {
            ACTORS.lock().unwrap().push(Actor {
                origin,
                thread: handle.thread().id(),
                kind: "ordered-worker",
                joined: false,
            });
        }
    }
    outcome
}

pub(crate) fn worker_body_entry(home: &Arc<PrivateOrderedHome>) {
    let key = Arc::as_ptr(home) as usize;
    let fault = {
        let mut faults = BODY_FAULTS.lock().unwrap();
        faults
            .iter()
            .position(|(candidate, _, _)| *candidate == key)
            .map(|i| faults.remove(i))
    };
    if let Some((_, pause, panic)) = fault {
        pause.wait();
        if let Some(payload) = panic {
            std::panic::panic_any(payload);
        }
    }
}

pub(super) type RunnerHook =
    Box<dyn for<'a> FnOnce(&mut PrivatePreparedRunner, &PrivateServiceLease<'a>) + Send>;
static RUNNER_HOOKS: Mutex<Vec<(usize, RunnerHook)>> = Mutex::new(Vec::new());
static TURNS: Mutex<Vec<(usize, Vec<PrivateRunnerProgress>)>> = Mutex::new(Vec::new());

pub(super) fn arm_runner(registry: &XServerFrontendRouteRegistry, hook: RunnerHook) {
    RUNNER_HOOKS
        .lock()
        .unwrap()
        .push((Arc::as_ptr(&registry.clients) as usize, hook));
}

pub(crate) fn before_service_turn(
    runner: &mut PrivatePreparedRunner,
    lease: &PrivateServiceLease<'_>,
) {
    let key = Arc::as_ptr(&runner.frontend().broker.registry.clients) as usize;
    let hook = {
        let mut hooks = RUNNER_HOOKS.lock().unwrap();
        hooks
            .iter()
            .position(|(candidate, _)| *candidate == key)
            .map(|i| hooks.remove(i).1)
    };
    if let Some(hook) = hook {
        hook(runner, lease);
    }
}

pub(crate) fn after_service_turn(
    registry: &XServerFrontendRouteRegistry,
    progress: &PrivateRunnerProgress,
) {
    if let Some((_, turns)) = TURNS
        .lock()
        .unwrap()
        .iter_mut()
        .find(|(key, _)| *key == Arc::as_ptr(&registry.clients) as usize)
        && turns.len() < 4096
    {
        turns.push(*progress);
    }
}

pub(super) fn observe_turns(registry: &XServerFrontendRouteRegistry) {
    TURNS
        .lock()
        .unwrap()
        .push((Arc::as_ptr(&registry.clients) as usize, Vec::new()));
}

pub(super) fn take_turns(registry: &XServerFrontendRouteRegistry) -> Vec<PrivateRunnerProgress> {
    let mut turns = TURNS.lock().unwrap();
    let at = turns
        .iter()
        .position(|(key, _)| *key == Arc::as_ptr(&registry.clients) as usize)
        .unwrap();
    turns.remove(at).1
}

type DequeueReading = (
    crate::ReadySequence,
    sophia_input_authority::CleanupReadiness,
    Option<sophia_input_authority::ServiceCharge>,
);
static DEQUEUES: Mutex<Vec<(usize, Vec<DequeueReading>)>> = Mutex::new(Vec::new());
static DEQUEUE_DELAYS: Mutex<Vec<(usize, Duration)>> = Mutex::new(Vec::new());

pub(super) fn delay_next_dequeue(registry: &XServerFrontendRouteRegistry) {
    DEQUEUE_DELAYS.lock().unwrap().push((
        Arc::as_ptr(&registry.clients) as usize,
        Duration::from_millis(3),
    ));
}

pub(crate) fn dequeue_started(registry: &XServerFrontendRouteRegistry) {
    let delay = {
        let mut held = DEQUEUE_DELAYS.lock().unwrap();
        held.iter()
            .position(|(key, _)| *key == Arc::as_ptr(&registry.clients) as usize)
            .map(|index| held.remove(index).1)
    };
    if let Some(delay) = delay {
        // A labelled bounded delay inside the original admitted operation,
        // without changing its clock, counters, request, or effect.
        std::thread::sleep(delay);
    }
}

pub(crate) fn dequeue_finished(
    registry: &XServerFrontendRouteRegistry,
    sequence: crate::ReadySequence,
    charge: sophia_input_authority::ServiceCharge,
) {
    if let Some((_, readings)) = DEQUEUES
        .lock()
        .unwrap()
        .iter_mut()
        .find(|(key, _)| *key == Arc::as_ptr(&registry.clients) as usize)
    {
        let last = readings
            .iter_mut()
            .rev()
            .find(|(at, _, _)| *at == sequence)
            .expect("actual budget start was recorded");
        assert!(last.2.replace(charge).is_none());
    }
}

pub(crate) fn dequeue_accounting(
    registry: &XServerFrontendRouteRegistry,
    sequence: crate::ReadySequence,
    cleanup: sophia_input_authority::CleanupReadiness,
) {
    if let Some((_, readings)) = DEQUEUES
        .lock()
        .unwrap()
        .iter_mut()
        .find(|(key, _)| *key == Arc::as_ptr(&registry.clients) as usize)
    {
        assert!(readings.len() < 4096, "bounded dequeue evidence");
        readings.push((sequence, cleanup, None));
    }
}

pub(super) fn observe_dequeues(registry: &XServerFrontendRouteRegistry) {
    DEQUEUES
        .lock()
        .unwrap()
        .push((Arc::as_ptr(&registry.clients) as usize, Vec::new()));
}

pub(super) fn take_dequeues(registry: &XServerFrontendRouteRegistry) -> Vec<DequeueReading> {
    let mut held = DEQUEUES.lock().unwrap();
    let index = held
        .iter()
        .position(|(key, _)| *key == Arc::as_ptr(&registry.clients) as usize)
        .unwrap();
    held.remove(index).1
}

pub(super) enum Maintenance {
    Step,
    Finish,
}

#[derive(Debug)]
pub(super) struct Maintained {
    pub(super) phase: PrivateMaintenancePhase,
    pub(super) status: PrivateMaintenanceStatus,
    /// Why a yielded visit yielded, typed rather than only formatted. A case
    /// that must establish a closed budget cannot do it from a Debug string.
    pub(super) allowance_refusal: Option<sophia_input_authority::ServiceStartRefusal>,
    /// Why a charged retained output visit refused, typed. A case that must
    /// establish what a visit did with a frame it still held cannot take that
    /// from a formatted report either.
    pub(super) output_refusal: Option<PrivateRetainedDriveRefusal>,
    /// What a charged terminal visit did, typed. The cleanup row needs this;
    /// reading it out of a formatted report would be the same mistake twice.
    pub(super) terminal_visit: Option<PrivateTerminalVisit>,
    pub(super) terminal_refusal: Option<PrivateTerminalDriveRefusal>,
    /// Whether a charged visit kept its supervisor, in EITHER phase.
    ///
    /// This used to answer only for the output phase, so a successfully
    /// supervised terminal visit reported false and a caller could read that
    /// as a failure. It is false now only when no charged visit happened.
    pub(super) supervision_ok: bool,
    pub(super) settled: Option<bool>,
    pub(super) charged: bool,
    pub(super) modifiers: Option<u16>,
    pub(super) instance: u64,
    pub(super) detail: String,
}

#[derive(Debug)]
pub(super) struct Closed {
    pub(super) unwound: bool,
    pub(super) succeeded: bool,
    pub(super) error: Option<String>,
    pub(super) workers: Vec<PrivateWorkerCollection>,
    pub(super) order: Option<PrivateOrderTally>,
    pub(super) collected: bool,
    pub(super) instance: u64,
    pub(super) modifiers: Option<u16>,
    pub(super) execution: PrivateExecutionReading,
}

pub(super) struct LifecycleService {
    pub(super) path: std::path::PathBuf,
    pub(super) registry: XServerFrontendRouteRegistry,
    pub(super) participant: PrivateAdmissionParticipant,
    pub(super) controller: PrivateAuthorityController,
    pub(super) owner: Arc<PrivateServiceOwner>,
    pub(super) access: PrivateProducerAccess,
    pub(super) commands: Option<SyncSender<XServerFrontendServiceCommand>>,
    pub(super) transactions: Receiver<XAuthorityObservedTransactionBatch>,
    pub(super) acks: Receiver<XAuthorityClientControlAck>,
    pub(super) deliveries: Receiver<XAuthorityClientInputDelivery>,
    pub(super) raster: XServerFrontendRasterRouter,
    pub(super) telemetry: SeenTelemetry,
    begin: Option<SyncSender<()>>,
    maintenance: SyncSender<Maintenance>,
    steps: Receiver<Maintained>,
    closed: Receiver<Closed>,
    done: Receiver<()>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl LifecycleService {
    pub(super) fn launch(
        tag: &str,
        namespace: u64,
        fault: Option<AttachFault>,
        unwind: bool,
    ) -> Self {
        Self::launch_with_capacity(tag, namespace, fault, unwind, 1)
    }

    pub(super) fn launch_with_capacity(
        tag: &str,
        namespace: u64,
        fault: Option<AttachFault>,
        unwind: bool,
        capacity: usize,
    ) -> Self {
        Self::launch_over_store(
            tag,
            namespace,
            fault,
            unwind,
            capacity,
            PrivateSettlementOwner::default(),
        )
    }

    /// The same launch over a settlement store the caller keeps.
    ///
    /// The store declares the accepted-item bound every producer reserves
    /// against, so a case that must meet an exact bound states it here rather
    /// than inferring one from the default capacity. The caller's clone is
    /// the same owner the service runs over, not an observer beside it.
    pub(super) fn launch_over_store(
        tag: &str,
        namespace: u64,
        fault: Option<AttachFault>,
        unwind: bool,
        capacity: usize,
        durable: PrivateSettlementOwner,
    ) -> Self {
        let path = private_service_socket(tag);
        let config = if capacity > 1 {
            distinct_config(&path, NamespaceId::from_raw(namespace), capacity)
        } else {
            private_service_config(&path, NamespaceId::from_raw(namespace), capacity)
        };
        let (commands, command_rx) = sync_channel(8);
        let (transaction_tx, transactions) = sync_channel(if unwind { 1 } else { 64 });
        let (parts, acks, deliveries) = producing_parts(capacity);
        let owner = Arc::new(service_owner(&durable, capacity));
        let service_owner = Arc::clone(&owner);
        let (port, access) = PrivateProducerAccess::for_service();
        let (begin, begun) = sync_channel(1);
        let (prepared, ready) = sync_channel(1);
        let (maintenance, maintained) = sync_channel(4);
        let (step_tx, steps) = channel();
        let (closed_tx, closed) = channel();
        let (done_tx, done) = channel();
        let telemetry = Arc::new(Mutex::new(Vec::new()));
        let service_thread = Arc::new(Mutex::new(None));
        let observer = recording_observer(
            Arc::clone(&telemetry),
            unwind.then_some(XAuthorityBackpressureTelemetryKind::Wait),
            Arc::clone(&service_thread),
        );
        let handle = std::thread::spawn(move || {
            *service_thread.lock().unwrap() = Some(std::thread::current().id());
            let private = PrivateXServerFrontend::new(parts, &service_owner)
                .unwrap_or_else(|(cause, _)| panic!("frontend: {cause:?}"));
            let registry = private.broker.registry.clone();
            TRACKED_ORIGINS
                .lock()
                .unwrap()
                .push(Arc::as_ptr(&registry.clients) as usize);
            if let Some(fault) = fault {
                ATTACH_FAULTS
                    .lock()
                    .unwrap()
                    .push((Arc::as_ptr(&registry.clients) as usize, fault));
            }
            prepared
                .send((
                    registry.clone(),
                    private.admission_participant().clone(),
                    private.controller.clone(),
                    private.broker.raster_router(),
                ))
                .unwrap();
            begun
                .recv_timeout(Duration::from_secs(5))
                .expect("caller starts actual service");
            let mut keeper = PrivateServiceExecutionKeeper::new();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                serve_private_frontend_until_stopped(
                    private,
                    &service_owner.lease(),
                    &mut keeper,
                    config,
                    transaction_tx,
                    command_rx,
                    port,
                    observer,
                )
            }));
            let mut observed = Closed {
                unwound: result.is_err(),
                succeeded: matches!(&result, Ok(Ok(_))),
                error: None,
                workers: Vec::new(),
                order: None,
                collected: false,
                instance: 0,
                modifiers: None,
                execution: keeper.execution().expect("retained actual execution"),
            };
            match &result {
                Ok(Ok(returned)) => {
                    observed.workers = returned.workers.clone();
                    observed.order = Some(returned.order);
                }
                Ok(Err(PrivateServiceFailure::Failed {
                    error,
                    workers,
                    order,
                    ..
                })) => {
                    observed.error = Some(error.to_string());
                    observed.workers = workers.clone();
                    observed.order = Some(**order);
                }
                Ok(Err(other)) => panic!("service failed to collect: {other:?}"),
                Err(_) => {}
            }
            // Actual settlement Drop transfers its inventory. Neither tests
            // nor later visits mint a replacement collection token.
            drop(result);
            let resources = keeper.resources.as_ref().unwrap();
            observed.collected = resources
                .collected
                .as_ref()
                .is_some_and(|token| Arc::ptr_eq(&token.registry, &registry.clients));
            observed.instance = resources.lifetime.0.instance;
            observed.modifiers = resources.keyboards.modifiers(resources.seat);
            closed_tx.send(observed).unwrap();
            while let Ok(command) = maintained.recv_timeout(Duration::from_secs(5)) {
                match command {
                    Maintenance::Finish => break,
                    Maintenance::Step => {
                        let report = keeper.maintain_step(&service_owner.lease());
                        let resources = keeper.resources.as_ref().unwrap();
                        step_tx
                            .send(Maintained {
                                phase: report.phase(),
                                status: report.status(),
                                allowance_refusal: report.allowance_refusal(),
                                output_refusal: match &report.outcome {
                                    PrivateMaintenanceOutcome::Output(
                                        PrivateRetainedDriveStep::Charged { outcome, .. },
                                    ) => outcome.as_ref().err().copied(),
                                    _ => None,
                                },
                                terminal_visit: match &report.outcome {
                                    PrivateMaintenanceOutcome::Terminal(
                                        PrivateTerminalDriveStep::Charged { outcome, .. },
                                    ) => outcome.as_ref().ok().copied(),
                                    _ => None,
                                },
                                terminal_refusal: match &report.outcome {
                                    PrivateMaintenanceOutcome::Terminal(
                                        PrivateTerminalDriveStep::Charged { outcome, .. },
                                    ) => outcome.as_ref().err().copied(),
                                    PrivateMaintenanceOutcome::Terminal(
                                        PrivateTerminalDriveStep::Refused(cause),
                                    ) => Some(*cause),
                                    _ => None,
                                },
                                supervision_ok: match &report.outcome {
                                    PrivateMaintenanceOutcome::Output(
                                        PrivateRetainedDriveStep::Charged { supervision, .. },
                                    )
                                    | PrivateMaintenanceOutcome::Terminal(
                                        PrivateTerminalDriveStep::Charged { supervision, .. },
                                    ) => supervision.is_ok(),
                                    _ => false,
                                },
                                settled: report.output_settled(),
                                charged: report.charge().is_some_and(Result::is_ok),
                                modifiers: resources.keyboards.modifiers(resources.seat),
                                instance: resources.lifetime.0.instance,
                                detail: format!("{report:?}"),
                            })
                            .unwrap();
                    }
                }
            }
            let watch = keeper.resources.as_mut().unwrap().watch.as_mut().unwrap();
            // An earlier real service turn may already have reaped a failed
            // supervisor; that source also records its actual join.
            if let Some(watchdog) = watch.request_shutdown() {
                actor_started(&registry, watchdog, "watchdog");
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                loop {
                    if let Some(result) = watch.reap_finished() {
                        actor_joined(watchdog);
                        result.expect("original watchdog supervisor returned");
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "watchdog did not return in test bound"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            drop(keeper);
            done_tx.send(()).unwrap();
        });
        let (registry, participant, controller, raster) =
            ready.recv_timeout(Duration::from_secs(5)).unwrap();
        actor_started(&registry, handle.thread().id(), "service");
        Self {
            path,
            registry,
            participant,
            controller,
            owner,
            access,
            commands: Some(commands),
            transactions,
            acks,
            deliveries,
            raster,
            telemetry,
            begin: Some(begin),
            maintenance,
            steps,
            closed,
            done,
            handle: Some(handle),
        }
    }

    pub(super) fn start(&mut self) {
        self.begin.take().unwrap().send(()).unwrap();
        self.access.await_ready(Duration::from_secs(5)).unwrap();
    }

    pub(super) fn command(&self, command: XServerFrontendServiceCommand) {
        self.commands.as_ref().unwrap().send(command).unwrap();
    }

    pub(super) fn custody(&self) -> Arc<PrivateEvidenceCustody> {
        waited_for_value(|| kept_custodies(&self.registry).into_iter().next()).unwrap()
    }

    pub(super) fn connect(&self) -> (UnixStream, Arc<PrivateEvidenceCustody>) {
        let previous = kept_custodies(&self.registry);
        let mut peer = connect_private_client(&self.path);
        handshake(&mut peer);
        let custody = waited_for_value(|| {
            kept_custodies(&self.registry)
                .into_iter()
                .find(|candidate| !previous.iter().any(|old| Arc::ptr_eq(old, candidate)))
        })
        .expect("the newly accepted connection's exact custody");
        (peer, custody)
    }

    pub(super) fn closed(&self) -> Closed {
        let closed = self
            .closed
            .recv_timeout(Duration::from_secs(5))
            .expect("actual service exited");
        assert!(
            closed.collected,
            "actual zero-frame collection token retained"
        );
        assert_eq!(
            closed.execution.availability,
            PrivateExecutionAvailability::Retained
        );
        closed
    }

    pub(super) fn step(&self) -> Maintained {
        self.maintenance.send(Maintenance::Step).unwrap();
        self.steps.recv_timeout(Duration::from_secs(5)).unwrap()
    }

    pub(super) fn finish(mut self, custodies: &[Arc<PrivateEvidenceCustody>]) -> Vec<String> {
        for custody in custodies {
            if custody.ever_started() {
                let join = custody.join();
                assert_eq!(join.phase(), PrivateReapingPhase::Joined);
                assert!(join.result().is_some());
            }
        }
        self.maintenance.send(Maintenance::Finish).unwrap();
        self.done
            .recv_timeout(Duration::from_secs(5))
            .expect("same-thread keeper and watchdog finished");
        let handle = self.handle.take().unwrap();
        let id = handle.thread().id();
        handle.join().expect("service actor collected");
        actor_joined(id);
        collected_actors(&self.registry)
    }
}

impl Drop for LifecycleService {
    fn drop(&mut self) {
        if let Some(begin) = self.begin.take() {
            let _ = begin.try_send(());
        }
        if let Some(commands) = &self.commands {
            let _ = commands.try_send(XServerFrontendServiceCommand::StopAndDisconnect);
        }
        let _ = self.maintenance.try_send(Maintenance::Finish);
        if let Some(handle) = self.handle.take()
            && self.done.recv_timeout(Duration::from_secs(5)).is_ok()
        {
            let _ = handle.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) fn focus_window(
    service: &LifecycleService,
    peer: &mut UnixStream,
    window: u32,
    transaction: u64,
) -> (SurfaceId, u16, PrivateIngress) {
    let (surface, sequence) = selecting_window(
        peer,
        &service.transactions,
        window,
        (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 6) | (1 << 21),
    );
    let custody = wait_attached(&service.registry);
    let client = custody.cleanup_record().client;
    let lease = service.owner.lease();
    let control = service.access.control_producer(&lease).unwrap();
    control
        .submit(
            &lease,
            XAuthorityClientControlCommand {
                client,
                command: XAuthorityControlCommand::FocusSurface {
                    transaction: TransactionId::from_raw(transaction),
                    surface,
                },
            },
        )
        .unwrap();
    assert_eq!(
        ack_for(&service.acks, transaction)
            .unwrap()
            .acknowledgement
            .outcome,
        XAuthorityControlOutcome::Delivered
    );
    assert_eq!(
        read_event(peer, 3),
        Some(expected_focus_in(sequence, window))
    );
    let ingress = service
        .access
        .ingress_for(&lease, client, DeviceId::from_raw(1))
        .unwrap();
    (surface, sequence, ingress)
}
