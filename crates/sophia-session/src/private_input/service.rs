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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex};

use super::admission::{PrivateInputAdmissionPolicy, PrivateInputAdmissionRecord};
use super::config::PrivateInputConfig;
use super::handle::{
    PrivateInputOutcome, PrivateInputReadiness, PrivateInputRefusal, PrivateInputSettlement,
    PrivateInputThreadJoin,
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
    pub(super) owner: Arc<PrivateServiceOwner>,
    pub(super) store: PrivateSettlementOwner,
    pub(super) participant: PrivateAdmissionParticipant,
    pub(super) access: PrivateProducerAccess,
    pub(super) registry: Arc<Mutex<NamespaceRegistry>>,
    pub(super) admitted: Arc<Mutex<BTreeMap<ClientAdmissionId, PrivateInputAdmissionRecord>>>,
    pub(super) readiness: Arc<Mutex<PrivateInputReadiness>>,
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
            binding,
            cookie,
            grants,
            max_concurrent_clients,
            input_capacity,
            advertised_buttons,
            output_topology,
            session_generation,
        } = config;

        // THE AUTHORITY FIRST, with its issuer and submit handles. They are
        // bound together at construction so the gate the frontend installs is
        // built from this instance rather than paired with it afterwards.
        let (authority, issuer, submit) = sophia_input_authority::AuthorityInstance::new(
            binding,
            sophia_input_authority::Capacity::PLANNED,
            advertised_buttons,
        )
        .map_err(PrivateInputRefusal::Capacity)?;

        let registry = Arc::new(Mutex::new(
            NamespaceRegistry::new(session_generation).map_err(PrivateInputRefusal::Namespace)?,
        ));
        let admitted = Arc::new(Mutex::new(BTreeMap::new()));
        let policy = Arc::new(PrivateInputAdmissionPolicy::new(
            Arc::clone(&registry),
            namespace,
            binding.instance(),
            Arc::clone(&admitted),
        ));

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
                report.execution = keeper.execution();
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
            grants,
            commands,
            acknowledgements: Mutex::new(acknowledgements),
            deliveries: Mutex::new(deliveries),
            transactions: Mutex::new(transactions),
            closed: Mutex::new(closed),
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
    pub(super) fn next_delivery(&self) -> (sophia_x_authority::XAuthorityInputDeliveryId, u64) {
        let delivery = sophia_x_authority::XAuthorityInputDeliveryId::from_raw(
            self.next_delivery.fetch_add(1, Ordering::AcqRel),
        );
        let time_msec = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        (delivery, time_msec)
    }

    /// Session's next control transaction.
    pub(super) fn next_transaction(&self) -> sophia_protocol::TransactionId {
        sophia_protocol::TransactionId::from_raw(
            self.next_transaction.fetch_add(1, Ordering::AcqRel),
        )
    }

    /// Stop the service and collect it.
    ///
    /// Producer admission closes first, so nothing new is accepted while the
    /// invocation is being ended. The command then stops the service, the
    /// report is taken, allowed maintenance runs, and only then is the thread
    /// joined. The thread's own join is reported apart from the workers it
    /// collected.
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
        let settlement = self.settlement();
        let mut outcome = PrivateInputOutcome {
            service_thread,
            settlement,
            ..PrivateInputOutcome::default()
        };
        if let Some(closed) = closed {
            outcome.invocation = closed.invocation;
            outcome.failure = closed.failure;
            outcome.unresolved_egress = closed.unresolved_egress;
            outcome.workers = closed.workers;
            outcome.execution = closed.execution;
            outcome.maintenance = closed.maintenance;
            outcome.interrupted = closed.interrupted;
        }
        outcome
    }
}

/// How many maintenance visits a stop drives.
///
/// A bound rather than a wait for quiescence: the keeper refuses a visit it is
/// not allowed to make, and driving until it stops refusing would turn an
/// interrupted budget into a spin.
const PRIVATE_INPUT_MAINTENANCE_VISITS: usize = 8;

fn describe_panic(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "the serving thread panicked".to_string())
}
