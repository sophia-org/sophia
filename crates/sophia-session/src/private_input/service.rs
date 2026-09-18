//! The service thread, and everything Session keeps beside it.
//!
//! THE ORDER HERE IS THE CONTRACT. The owner, the durable store and the
//! authority are established before the frontend exists. The frontend is
//! constructed from parts that already carry the issuer, so the identity that
//! serves and the identity that executes are the same by construction. The
//! execution keeper is made on the serving thread, outside the unwind
//! boundary, so a service that fails or unwinds still hands its history,
//! supervisor and accounting to a keeper that is still standing. The owner and
//! the maintenance channel outlive the invocation for the same reason, and
//! nothing reconstructs a keeper afterwards.

use sophia_protocol::ClientAdmissionId;
use sophia_runtime::NamespaceRegistry;
use sophia_x_authority::{
    PrivateAdmissionParticipant, PrivatePortStanding, PrivateProducerAccess, PrivateServiceBinding,
    PrivateServiceFailure, PrivateServiceOwner, PrivateSettlementOwner, PrivateXServerFrontend,
    XAuthorityClientControlAck, XAuthorityClientInputDelivery, XAuthorityObservedTransactionBatch,
    XServerFrontendConfig, XServerFrontendServiceCommand,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex};

use super::admission::{PrivateInputAdmissionPolicy, PrivateInputAdmissionRecord};
use super::config::PrivateInputConfig;
use super::handle::{
    PrivateInputOutcome, PrivateInputReadiness, PrivateInputRefusal, PrivateInputSettlement,
    PrivateInputThreadJoin, PrivateInputTopologyRefusal, PrivateInputVisit,
};

/// Everything Session owns for one private input service.
///
/// The issuer never appears here, and that is deliberate: it goes into the
/// frontend's parts at construction and is not kept where anything could reach
/// it afterwards.
pub(super) struct PrivateInputRuntime {
    pub(super) socket_path: PathBuf,
    /// The seat every submission is routed under, taken from the binding this
    /// service's authority was built with rather than chosen per request.
    pub(super) seat: sophia_protocol::SeatId,
    /// When this service started, so a submission can carry the millisecond it
    /// was minted at. Not a budget or watchdog clock: those stay the service's
    /// own and are untouched by this.
    pub(super) started: std::time::Instant,
    pub(super) next_delivery: AtomicU64,
    /// This Session's own headless backend assembly.
    ///
    /// THE REAL ONE, held here rather than by a caller or an example, built
    /// from the configured topology with the configured frame clock. Its
    /// committed surface state is the only source of the geometry a map or a
    /// configure carries, so a caller cannot supply one and cannot seed
    /// applied state without a real transaction going through it.
    pub(super) assembly: Mutex<sophia_engine::QueuedHeadlessCompositorBackendAssembly>,
    /// Committed decisions waiting to reach the order, in the order they were
    /// decided.
    ///
    /// ORDERED AND POPPED ONLY ON TRANSFER. A later effect must not overtake
    /// an earlier one that the order refused: accepting a configure before its
    /// own admission would apply an update to a window that was never mapped.
    pub(super) bridge: Mutex<super::committed::PrivateInputBridge>,
    /// Whether a stop has already been performed.
    ///
    /// ONE STOP, WHOEVER ASKS. The controller stops on drop even while an
    /// adapter still holds the runtime, so an explicit stop followed by the
    /// controller going out of scope must not stop and join twice.
    pub(super) stopped: AtomicBool,
    /// Surfaces this service has already admitted, with the exact connection
    /// each was admitted for. Keyed by the whole SurfaceId, whose own
    /// generation is the incarnation, so a surface destroyed and created again
    /// is a new entry rather than a stale one. The connection is retained
    /// because a withdrawing batch no longer carries the route that admitted
    /// it.
    pub(super) admitted_surfaces: Mutex<
        std::collections::BTreeMap<sophia_protocol::SurfaceId, super::PrivateInputConnection>,
    >,
    pub(super) owner: Arc<PrivateServiceOwner>,
    pub(super) store: PrivateSettlementOwner,
    pub(super) participant: PrivateAdmissionParticipant,
    pub(super) access: PrivateProducerAccess,
    pub(super) registry: Arc<Mutex<NamespaceRegistry>>,
    pub(super) admitted: Arc<Mutex<BTreeMap<ClientAdmissionId, PrivateInputAdmissionRecord>>>,
    pub(super) readiness: Arc<Mutex<PrivateInputReadiness>>,
    /// A handle to the invocation's execution witness, readable after the
    /// serving thread has been joined.
    ///
    /// NOT A READING TAKEN ON THE THREAD. A reading taken while the keeper
    /// still existed says the execution is retained however the thread then
    /// ends, so reporting it would make a joined thread look like a live
    /// execution. The keeper's own drop publishes abandonment into this
    /// witness, so a reader that joins first and reads afterwards is told what
    /// is true after the join.
    pub(super) execution_witness:
        Arc<Mutex<Option<sophia_x_authority::PrivateExecutionWitnessHandle>>>,
    pub(super) grants: super::PrivateInputGrantPolicy,
    pub(super) commands: SyncSender<XServerFrontendServiceCommand>,
    /// The receiving ends Session owns.
    ///
    /// EACH BEHIND ITS OWN LOCK. A receiver is not `Sync`, and the handle is
    /// shared, so draining through a shared reference needs one. Separate
    /// locks rather than one, because draining acknowledgements must not wait
    /// behind a bounded wait for deliveries.
    pub(super) acknowledgements: Mutex<Receiver<XAuthorityClientControlAck>>,
    pub(super) deliveries: Mutex<Receiver<XAuthorityClientInputDelivery>>,
    pub(super) transactions: Mutex<Receiver<XAuthorityObservedTransactionBatch>>,
    pub(super) closed: Mutex<Receiver<ServiceClosed>>,
    pub(super) thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Session's own transaction clock for the controls it submits. Never a
    /// caller's number: two callers choosing the same one would make the
    /// acknowledgements ambiguous.
    pub(super) next_transaction: AtomicU64,
}

/// What the serving thread reports once its invocation has ended.
pub(super) struct ServiceClosed {
    pub(super) invocation: super::handle::PrivateInputInvocation,
    pub(super) failure: Option<PrivateServiceFailure>,
    pub(super) unresolved_egress: Vec<sophia_x_authority::PrivateUnresolvedEgress>,
    /// The workers the invocation reported collecting. `None` when it made no
    /// report at all, which an unwind does not. An empty vector would say it
    /// collected none, which is a different claim.
    pub(super) workers: Option<Vec<sophia_x_authority::PrivateWorkerCollection>>,
    /// What the keeper still held once the invocation ended, however it ended.
    pub(super) execution: Option<sophia_x_authority::PrivateExecutionReading>,
    pub(super) maintenance: Vec<sophia_x_authority::PrivateDeferredCleanupOutcome>,
    /// What each maintenance visit actually reported.
    pub(super) visits: Vec<PrivateInputVisit>,
    /// Whether the budget this invocation ran under was interrupted.
    ///
    /// READ FROM A REAL VISIT, not inferred from an unwind. An interrupted
    /// budget refuses its next visit with `Interrupted`, and that refusal is
    /// what this records; a service can be interrupted without unwinding and
    /// can unwind without its budget having been interrupted first.
    pub(super) interrupted: bool,
}

impl PrivateInputRuntime {
    /// Stand the service up.
    ///
    /// Everything that can be refused is refused here, on the caller's thread,
    /// before a thread exists to have to unwind.
    pub(super) fn start(config: PrivateInputConfig) -> Result<Self, PrivateInputRefusal> {
        let PrivateInputConfig {
            socket_path,
            namespace,
            profile,
            capabilities,
            binding,
            cookie,
            grants,
            max_concurrent_clients,
            input_capacity,
            advertised_buttons,
            output_topology,
            frame_clock,
            session_generation,
        } = config;

        // THE TOPOLOGY IS VALIDATED BEFORE ANYTHING IS CONSTRUCTED. It was
        // previously checked after the authority instance and the registry
        // existed, which meant a refusable configuration had already allocated
        // capacity and installed a namespace it would then abandon. Nothing is
        // built until the configuration it would be built from is known good.
        if output_topology.outputs.is_empty() {
            return Err(PrivateInputRefusal::Topology(
                PrivateInputTopologyRefusal::NoOutputs,
            ));
        }
        if output_topology.outputs.len() > 1 {
            return Err(PrivateInputRefusal::Topology(
                PrivateInputTopologyRefusal::MultipleOutputs {
                    count: output_topology.outputs.len(),
                },
            ));
        }
        // THE CONFIGURED PRIMARY OR NOTHING. Falling back to the first output
        // would serve a different screen from the one configured and make every
        // committed geometry a claim about the wrong output.
        let primary = *output_topology
            .outputs
            .iter()
            .find(|entry| entry.output == output_topology.primary)
            .ok_or(PrivateInputRefusal::Topology(
                PrivateInputTopologyRefusal::PrimaryAbsent {
                    primary: output_topology.primary,
                },
            ))?;

        // THE AUTHORITY FIRST, with its issuer and submit handles. They are
        // bound together at construction so the gate the frontend installs is
        // built from this instance rather than paired with it afterwards.
        let (authority, issuer, submit) = sophia_input_authority::AuthorityInstance::new(
            binding,
            sophia_input_authority::Capacity::PLANNED,
            advertised_buttons,
        )
        .map_err(PrivateInputRefusal::Capacity)?;

        // THE NAMESPACE IS INSTALLED BEFORE ANYTHING SERVES. A registry built
        // empty knows nothing of the namespace this service was told to serve,
        // so every admission would be refused as belonging to an unknown one
        // and no connection could ever be admitted.
        let context = sophia_protocol::NamespaceContext::new(namespace, profile, capabilities)
            .ok_or(PrivateInputRefusal::Namespace(
                sophia_runtime::NamespaceRegistryError::UnknownNamespace { namespace },
            ))?;
        let registry = Arc::new(Mutex::new(
            NamespaceRegistry::with_namespace(session_generation, context)
                .map_err(PrivateInputRefusal::Namespace)?,
        ));
        let admitted = Arc::new(Mutex::new(BTreeMap::new()));
        let policy = Arc::new(PrivateInputAdmissionPolicy::new(
            Arc::clone(&registry),
            namespace,
            binding.instance(),
            Arc::clone(&admitted),
        ));

        // THE ENGINE'S OUTPUT IS THE TOPOLOGY THIS SERVICE WAS GIVEN. A fixed
        // deterministic output would have committed geometry against a size
        // nobody configured, and the declared X topology would then be a claim
        // about the Engine rather than a fact about it.
        // THE PLANNED ASSEMBLY, NOT A COORDINATOR ON ITS OWN. Building the
        // coordinator directly would quietly narrow the plan to the one part
        // of it this path happens to call, and would leave the frame clock
        // whichever the convenience constructor chose.
        let output = sophia_engine::HeadlessOutput {
            id: primary.output,
            size: primary.pixel_size,
            scale: primary.scale,
        };
        // BUILT FROM THE ENTRY, INCLUDING ITS REFRESH. The assembly's own
        // convenience path fixes the head at sixty hertz; the topology this
        // service was given says what its output actually refreshes at, and
        // discarding that would make the head disagree with the topology the
        // frontend advertises.
        let mut heads = sophia_engine::EngineHeadRegistry::new();
        let _ = heads.admit(sophia_engine::HeadRenderTarget {
            head: sophia_engine::RenderHeadId::from_raw(primary.output.raw()),
            output: primary.output,
            target_generation: 1,
            native_size: primary.pixel_size,
            scale: primary.scale,
            refresh_millihz: primary.refresh_millihz,
            transform: sophia_protocol::OutputTransform::Normal,
            mapping: sophia_protocol::OutputHeadMapping::Fit,
        });
        let assembly = sophia_engine::QueuedHeadlessCompositorBackendAssembly::from_parts(
            output,
            heads,
            frame_clock,
            sophia_engine::LibinputPhysicalInputAdapter::new(
                sophia_engine::QueuedInputPoller::default(),
                sophia_engine::LibinputEventSource::new(),
            ),
            sophia_engine::RendererSelection::default(),
        );

        let frontend_config = XServerFrontendConfig::new(&socket_path, namespace)
            .map_err(PrivateInputRefusal::Configuration)?
            .with_max_concurrent_clients(max_concurrent_clients)
            .with_output_topology(output_topology)
            .map_err(PrivateInputRefusal::Configuration)?
            .with_setup_authorization(
                sophia_x_authority::XServerFrontendSetupAuthorization::PrivateInputCookie {
                    instance: cookie.instance,
                    cookie: cookie.cookie,
                },
            )
            .with_admission_policy(policy);

        let store = PrivateSettlementOwner::with_capacity(max_concurrent_clients.get() * 4);
        let owner = Arc::new(
            PrivateServiceOwner::established_over(&store, max_concurrent_clients).ok_or(
                PrivateInputRefusal::Construction(
                    sophia_x_authority::AdmissionRefusal::Unavailable,
                ),
            )?,
        );

        let (acknowledgements_tx, acknowledgements) = sync_channel(64);
        let (deliveries_tx, deliveries) = channel();
        let (transactions_tx, transactions) = sync_channel(64);
        let (commands, command_rx) = sync_channel(8);
        let (closed_tx, closed) = channel();
        let (prepared_tx, prepared) = sync_channel(1);
        let (port, access) = PrivateProducerAccess::for_service();

        let parts = sophia_x_authority::PrivateFrontendParts {
            max_concurrent_clients,
            input_capacity,
            control_acknowledgements: acknowledgements_tx,
            input_deliveries: deliveries_tx,
            authority,
            issuer,
            submit,
        };

        let readiness = Arc::new(Mutex::new(PrivateInputReadiness::Binding));
        let thread_readiness = Arc::clone(&readiness);
        let execution_witness = Arc::new(Mutex::new(None));
        let thread_witness = Arc::clone(&execution_witness);
        let thread_owner = Arc::clone(&owner);
        let observer: Arc<sophia_x_authority::XAuthorityBackpressureObserver> =
            Arc::new(|_telemetry| {});

        let thread = std::thread::Builder::new()
            .name("sophia-private-input".into())
            .spawn(move || {
                let private = match PrivateXServerFrontend::new(parts, &thread_owner) {
                    Ok(private) => private,
                    Err((refusal, _parts)) => {
                        *thread_readiness.lock().expect("readiness") =
                            PrivateInputReadiness::Refused(format!("{refusal:?}"));
                        let _ = prepared_tx.send(None);
                        return;
                    }
                };
                // Session keeps the boundary before the service consumes the
                // frontend. A clone rather than a borrow, because the frontend
                // is about to be moved into the invocation.
                let participant = private.admission_participant().clone();
                if prepared_tx.send(Some(participant)).is_err() {
                    return;
                }

                // MADE HERE, AND IT STAYS HERE. Declared before the unwind
                // boundary so a service that panics still leaves a keeper
                // holding its execution resources.
                let mut keeper = sophia_x_authority::PrivateServiceExecutionKeeper::new();
                let binding = PrivateServiceBinding {
                    config: frontend_config,
                    transactions: transactions_tx,
                    commands: command_rx,
                    producers: port,
                    backpressure: observer,
                };
                // NOT MARKED READY HERE. Binding and preparing happen inside
                // the service, so readiness is the producer port reaching
                // Ready, which the service publishes once it has prepared.
                // Marking it before the call would report a socket nothing is
                // listening on yet.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    private.serve_until_stopped(&thread_owner.lease(), &mut keeper, binding)
                }));

                let mut report = ServiceClosed {
                    invocation: super::handle::PrivateInputInvocation::Returned,
                    failure: None,
                    unresolved_egress: Vec::new(),
                    workers: None,
                    execution: None,
                    maintenance: Vec::new(),
                    visits: Vec::new(),
                    interrupted: false,
                };
                match result {
                    Ok(Ok(returned)) => {
                        report.unresolved_egress = returned.unresolved_egress;
                        report.workers = Some(returned.workers);
                        report.maintenance = returned.maintenance;
                    }
                    Ok(Err(failure)) => {
                        report.invocation = super::handle::PrivateInputInvocation::Failed;
                        if let PrivateServiceFailure::Failed { workers, .. } = &failure {
                            report.workers = Some(workers.clone());
                        }
                        report.failure = Some(failure);
                    }
                    // AN UNWIND REPORTS NOTHING OF ITS OWN, so nothing is
                    // invented for it. The panic is kept as itself, the worker
                    // list stays absent rather than empty, and what the keeper
                    // still holds is read from the keeper, which is why it was
                    // made outside this boundary.
                    Err(payload) => {
                        report.invocation = super::handle::PrivateInputInvocation::Unwound(
                            describe_panic(payload.as_ref()),
                        );
                    }
                }
                // THE AT-CLOSE READING, kept as itself: what the keeper held
                // when the invocation ended, before maintenance and before the
                // keeper's own drop.
                report.execution = keeper.execution();
                // AND A HANDLE THAT OUTLIVES BOTH, so the stop can report what
                // the execution is after joining rather than what it was
                // before. Taken here because the keeper must still exist to
                // give it out; read only after the join.
                if let Some(handle) = keeper.execution_witness()
                    && let Ok(mut held) = thread_witness.lock()
                {
                    *held = Some(handle);
                }
                // ALLOWED MAINTENANCE, AFTER COLLECTION, ON THIS THREAD. The
                // keeper never leaves the thread it was made on and is never
                // rebuilt. A bounded number of visits rather than a wait for
                // quiescence: an interrupted budget refuses every visit, and
                // driving until it stopped refusing would be a spin.
                for _ in 0..PRIVATE_INPUT_MAINTENANCE_VISITS {
                    let visit = keeper.maintain_step(&thread_owner.lease());
                    if visit.allowance_refusal()
                        == Some(sophia_input_authority::ServiceStartRefusal::Interrupted)
                    {
                        report.interrupted = true;
                    }
                    // EVERY VISIT IS REPORTED AS ITSELF. The count below is a
                    // bound on attempts and nothing more: reaching it says the
                    // loop stopped asking, never that anything settled.
                    report.visits.push(PrivateInputVisit {
                        phase: visit.phase(),
                        status: visit.status(),
                        allowance_refusal: visit.allowance_refusal(),
                    });
                }
                let _ = closed_tx.send(report);
            })
            .map_err(PrivateInputRefusal::Thread)?;

        // A THREAD THAT WAS SPAWNED IS ALWAYS JOINED. If construction refused
        // or the channel was lost, returning while the handle went out of
        // scope unjoined would leave a thread running behind a caller who
        // believes nothing started.
        let participant = match prepared.recv() {
            Ok(Some(participant)) => participant,
            Ok(None) | Err(_) => {
                let refusal = match readiness.lock().map(|held| held.clone()) {
                    Ok(PrivateInputReadiness::Refused(cause)) => {
                        PrivateInputRefusal::ConstructionRefused(cause)
                    }
                    _ => PrivateInputRefusal::Construction(
                        sophia_x_authority::AdmissionRefusal::Unavailable,
                    ),
                };
                let _ = thread.join();
                return Err(refusal);
            }
        };

        Ok(Self {
            socket_path,
            owner,
            store,
            participant,
            access,
            registry,
            admitted,
            readiness,
            execution_witness,
            grants,
            commands,
            acknowledgements: Mutex::new(acknowledgements),
            deliveries: Mutex::new(deliveries),
            transactions: Mutex::new(transactions),
            closed: Mutex::new(closed),
            assembly: Mutex::new(assembly),
            bridge: Mutex::new(super::committed::PrivateInputBridge::default()),
            stopped: AtomicBool::new(false),
            admitted_surfaces: Mutex::new(std::collections::BTreeMap::new()),
            seat: binding.seat(),
            started: std::time::Instant::now(),
            next_delivery: AtomicU64::new(1),
            thread: Mutex::new(Some(thread)),
            next_transaction: AtomicU64::new(1),
        })
    }

    /// What the service has got to.
    ///
    /// READ FROM THE PORT, NOT FROM AN INTENTION. The producer port reaches
    /// Ready when the service has actually prepared and Ended when it has
    /// gone, so those are the facts reported. A refusal recorded before the
    /// loop ever ran outranks both: a service that never began is not one that
    /// is still binding.
    pub(super) fn readiness(&self) -> PrivateInputReadiness {
        if let Ok(PrivateInputReadiness::Refused(cause)) =
            self.readiness.lock().map(|held| held.clone())
        {
            return PrivateInputReadiness::Refused(cause);
        }
        match self.access.standing() {
            PrivatePortStanding::NotReady => PrivateInputReadiness::Binding,
            PrivatePortStanding::Ready => PrivateInputReadiness::Ready,
            PrivatePortStanding::Ended => PrivateInputReadiness::Stopped,
        }
    }

    /// What the durable owner is holding, read without touching it.
    ///
    /// OBSERVATION ONLY. An earlier version asked the store to drive, which
    /// performs recovery: reading a status would then have done work and
    /// changed what the next reader saw. These are plain reads, and
    /// readability is `reserved` answering at all rather than a separate
    /// question, because an unreadable store answers none of them.
    pub(super) fn settlement(&self) -> PrivateInputSettlement {
        let reserved_credits = self.store.reserved();
        PrivateInputSettlement {
            readable: reserved_credits.is_some(),
            reserved_credits,
            owed: self.store.owed(),
            indeterminate: self.store.indeterminate(),
        }
    }

    /// The next delivery identity, and the millisecond it was minted at.
    ///
    /// BOTH ARE SESSION'S AND BOTH ARE REPORTED. A caller comparing wire bytes
    /// against what it submitted needs the exact time that went into the
    /// request, so it is returned rather than left for the reader to infer or
    /// mask out.
    pub(super) fn next_delivery(
        &self,
    ) -> Option<(sophia_x_authority::XAuthorityInputDeliveryId, u64)> {
        // CHECKED, NOT WRAPPING. A counter that wraps hands a second request
        // the identity of a first one that may still be outstanding, and the
        // receipt for either would then answer both. Exhaustion is refused
        // before anything is accepted, which is what the producers already do.
        let raw = next_identity(&self.next_delivery)?;
        let time_msec = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Some((
            sophia_x_authority::XAuthorityInputDeliveryId::from_raw(raw),
            time_msec,
        ))
    }

    /// Session's next control transaction.
    pub(super) fn next_transaction(&self) -> Option<sophia_protocol::TransactionId> {
        next_identity(&self.next_transaction).map(sophia_protocol::TransactionId::from_raw)
    }

    /// Stop the service and collect it.
    ///
    /// Producer admission closes first, so nothing new is accepted while the
    /// invocation is being ended. The command then stops the service, the
    /// report is taken, allowed maintenance runs, and only then is the thread
    /// joined. The thread's own join is reported apart from the workers it
    /// collected.
    /// Stop exactly once, whoever asks first.
    ///
    /// Returns `None` when a stop has already been performed, so the
    /// controller's drop can always ask without joining a thread twice.
    pub(super) fn stop_once(&self) -> Option<PrivateInputOutcome> {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return None;
        }
        Some(self.stop())
    }

    pub(super) fn stop(&self) -> PrivateInputOutcome {
        // Producer admission closes as the service ends: the port lives inside
        // the invocation, so stopping it is what shuts the door. Nothing here
        // reaches around that to close it early, which would refuse producers
        // while the invocation was still running.
        let _ = self
            .commands
            .send(XServerFrontendServiceCommand::StopAndDisconnect);
        let closed = self.closed.lock().ok().and_then(|held| held.recv().ok());
        let service_thread = match self.thread.lock().ok().and_then(|mut held| held.take()) {
            Some(handle) => match handle.join() {
                Ok(()) => PrivateInputThreadJoin::Joined,
                Err(payload) => PrivateInputThreadJoin::Panicked(describe_panic(&payload)),
            },
            None => PrivateInputThreadJoin::NeverStarted,
        };
        // AFTER THE JOIN, AND ONLY AFTER IT. The thread has ended, so the
        // keeper has dropped and its lifetime owner has published whatever
        // became of the execution. Reading here cannot report a retained
        // execution for a thread that has already gone.
        let execution = self
            .execution_witness
            .lock()
            .ok()
            .and_then(|held| held.as_ref().map(|witness| witness.reading()));
        let settlement = self.settlement();
        // COMMITTED WORK THE ORDER NEVER TOOK. The X store has never seen it,
        // so it appears in no settlement reading; counting it here is what
        // stops a stop from looking finished while this is still owed.
        let bridge_undelivered = self.bridge.lock().map(|held| held.outstanding()).ok();
        let mut outcome = PrivateInputOutcome {
            service_thread,
            settlement,
            bridge_undelivered,
            ..PrivateInputOutcome::default()
        };
        if let Some(closed) = closed {
            outcome.invocation = closed.invocation;
            outcome.failure = closed.failure;
            outcome.unresolved_egress = closed.unresolved_egress;
            outcome.workers = closed.workers;
            outcome.execution_at_close = closed.execution;
            outcome.maintenance = closed.maintenance;
            outcome.visits = closed.visits;
            outcome.interrupted = closed.interrupted;
        }
        // LAST, so the thread's own pre-drop reading cannot overwrite it.
        outcome.execution = execution;
        outcome
    }
}

/// How many maintenance visits a stop drives.
///
/// A bound rather than a wait for quiescence: the keeper refuses a visit it is
/// not allowed to make, and driving until it stops refusing would turn an
/// interrupted budget into a spin.
const PRIVATE_INPUT_MAINTENANCE_VISITS: usize = 8;

/// One more identity from a counter that refuses rather than wraps.
///
/// `None` is exhaustion, which a caller turns into a typed refusal before it
/// accepts anything. Reusing an identity would be worse than refusing: the
/// receipt for the reused one answers whichever request the reader believes
/// it belongs to.
fn next_identity(counter: &AtomicU64) -> Option<u64> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |held| {
            held.checked_add(1)
        })
        .ok()
}

fn describe_panic(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "the serving thread panicked".to_string())
}
