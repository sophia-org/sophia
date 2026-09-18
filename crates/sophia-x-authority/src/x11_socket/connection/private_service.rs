// The private routed service: who owns it, and how it ends.
//
// THE OWNER IS BORROWED, NOT MADE HERE. A `PrivateServiceOwner` constructed
// inside this function would go with this function's frame on every return,
// error and unwind, and the custody inventory it keeps -- registered worker
// handles, join and fence evidence -- would go with it. The launch scope owns
// it before this is called and keeps it after this returns, errors or unwinds,
// so that inventory is inspectable through the ORIGINAL owner afterwards. A
// `PrivateSettlement` returned from here is the settlement store's handle; it
// is not that inventory and is not described as one.
//
// THE SAME LOOP AS THE PUBLIC SERVICE, REACHED THROUGH THE LEASE. The routed
// loop is factored out of the public entry point unchanged and driven here
// against the private frontend's own broker, but every reach to that broker
// goes through the checked service lease: a frontend whose registry is not
// kept by the leased owner refuses before a listener is bound. No producer,
// runner or ordered worker is exposed or started by this path.
//
// EXIT ORDER IS CONTROL FLOW, NOT CONVENTION. On ordinary stop, on loss of
// the command channel, on an error after a connection exists, and on an
// unwind inside the operation: admission stops, the egress paths a worker can
// be blocked in are cancelled, every current legacy client worker is told to
// stop and then waited for, and only then is the private frontend finalised.
// The ordinary and error paths do this explicitly and report each cleanup
// failure without letting one skip the next or replace the original error;
// the unwind path does it through the collection guard's `Drop`, which the
// existing public frontend has no equivalent of. The private frontend is
// declared before that guard so that it is dropped after it.

/// How the routed loop reaches its broker.
///
/// The public service owns its broker and reaches it directly. The private
/// service reaches it through the lease, and a reach that the lease does not
/// cover is refused rather than performed.
#[cfg(unix)]
trait RoutedBrokerAccess {
    fn broker(&mut self) -> Result<&XServerFrontendRouteBroker, X11SetupSocketError>;
    fn route_pending(&mut self) -> Result<usize, X11SetupSocketError>;
    /// Start a registered worker for every ready connection that has none.
    ///
    /// THE PUBLIC PATH HAS NONE: nothing is registered there and nothing is
    /// started. The private path visits from the service frame, which is the
    /// one place holding the checked lease and the frontend together.
    fn attach_ready(&mut self) -> Result<usize, X11SetupSocketError>;
}

#[cfg(unix)]
impl RoutedBrokerAccess for XServerFrontendRouteBroker {
    fn broker(&mut self) -> Result<&XServerFrontendRouteBroker, X11SetupSocketError> {
        Ok(self)
    }
    fn route_pending(&mut self) -> Result<usize, X11SetupSocketError> {
        XServerFrontendRouteBroker::route_pending(self)
            .map_err(|error| X11SetupSocketError::new(error.to_string()))
    }
    fn attach_ready(&mut self) -> Result<usize, X11SetupSocketError> {
        Ok(0)
    }
}

/// The private frontend's broker, reachable only while the lease covers it.
#[cfg(unix)]
struct LeasedPrivateBroker<'a, 'o> {
    frontend: &'a mut PrivateXServerFrontend,
    service: &'a PrivateServiceLease<'o>,
}

#[cfg(unix)]
impl LeasedPrivateBroker<'_, '_> {
    fn check(&self) -> Result<(), X11SetupSocketError> {
        if self.frontend.broker.registry.leased_by(self.service) {
            Ok(())
        } else {
            Err(X11SetupSocketError::new(
                "private service lease is not on the owner that keeps this frontend's registry",
            ))
        }
    }
}

#[cfg(unix)]
impl RoutedBrokerAccess for LeasedPrivateBroker<'_, '_> {
    fn broker(&mut self) -> Result<&XServerFrontendRouteBroker, X11SetupSocketError> {
        self.check()?;
        Ok(&self.frontend.broker)
    }
    /// THE PRIVATE FRONTEND'S OWN LEASED ROUTING, not the broker's. The
    /// broker operation drains the routed-input order; the private frontend's
    /// drains the private accepted order under its own lease check, its
    /// ordered-runner guard and its budget. Reaching past it to the broker
    /// with an owner check wrapped around the reach preserved none of that.
    /// This service drives no second input order: nothing feeds the broker's
    /// routed-input queue on this path, and adding that would be a deliberate
    /// step, not housekeeping.
    fn route_pending(&mut self) -> Result<usize, X11SetupSocketError> {
        self.frontend
            .route_pending(self.service)
            .map(|ran| ran.len())
            .map_err(|error| X11SetupSocketError::new(error.to_string()))
    }
    fn attach_ready(&mut self) -> Result<usize, X11SetupSocketError> {
        self.check()?;
        Ok(attach_ready_workers(self.frontend, self.service))
    }
}

/// The routed service loop, shared by the public and private entry points.
///
/// MOVED, NOT REWRITTEN, from the public entry point: the body is the same
/// and the public path's behaviour is preserved. What differs between the two
/// callers is only how `broker` is reached, which is the trait above.
#[cfg(unix)]
fn drive_routed_service(
    frontend: &mut XServerFrontend,
    broker: &mut dyn RoutedBrokerAccess,
    service_commands: &Receiver<XServerFrontendServiceCommand>,
    ordered_egress: &XAuthorityOrderedEgress,
    observer: &Arc<X11CoreTraceObserver>,
    pending_raster_egress: &mut Option<XAuthorityBoundedEgressEnvelope>,
) -> Result<(), X11SetupSocketError> {
    let mut accepting = true;
    let mut raster_fallbacks = XRasterFallbackCoalescer::default();
    loop {
        let mut progressed = false;
        match service_commands.try_recv() {
            Ok(XServerFrontendServiceCommand::UpdateWindowAllocationPreferences { snapshot, acknowledgement }) => {
                let outcome = frontend.update_window_allocation_preferences(snapshot)?;
                let _ = acknowledgement.try_send(outcome);
                progressed = true;
            }
            Ok(XServerFrontendServiceCommand::InstallDeviceBundle { bundle, acknowledgement }) => {
                let _ = acknowledgement.try_send(frontend.install_device_bundle(bundle));
                progressed = true;
            }
            Ok(XServerFrontendServiceCommand::MarkDeviceGenerationUnavailable { generation, acknowledgement }) => {
                let _ = acknowledgement.try_send(frontend.mark_device_generation_unavailable(generation));
                progressed = true;
            }
            Ok(XServerFrontendServiceCommand::StopAccepting) => {
                if accepting {
                    accepting = false;
                    progressed = true;
                }
            }
            Ok(XServerFrontendServiceCommand::DrainAndDisconnect) => {
                accepting = false;
                // Workers retain cleanup ownership and may still be
                // publishing accepted work. Do not cancel their egress.
                frontend.shutdown_all_client_workers()?;
                progressed = true;
            }
            Ok(XServerFrontendServiceCommand::StopAndDisconnect)
            | Err(TryRecvError::Disconnected) => {
                accepting = false;
                if !ordered_egress.cancelled() {
                    ordered_egress.cancel();
                    // IN PLACE. The wait is cancelled; the envelope and its
                    // batch stay in the caller's slot for the caller to
                    // account for. Taking it out here destroyed an unsent
                    // batch on every ordinary stop.
                    if let Some(envelope) = pending_raster_egress.as_mut() {
                        ordered_egress.cancel_envelope(envelope)?;
                    }
                    frontend.shutdown_all_client_workers()?;
                    progressed = true;
                }
            }
            Ok(XServerFrontendServiceCommand::RevokeAdmission { admission }) => {
                progressed |= frontend.revoke_admission(admission)?;
            }
            Ok(XServerFrontendServiceCommand::UpdateOutputTopology {
                snapshot,
                acknowledgement,
            }) => {
                let mut outcome = frontend.update_output_topology(snapshot.clone())?;
                if matches!(outcome, XAuthorityOutputUpdateOutcome::Applied { .. }) {
                    let notifications = broker
                        .broker()?
                        .registry
                        .broadcast_randr_update(&snapshot)
                        .map_err(|error| X11SetupSocketError::new(error.to_string()))?;
                    if let XAuthorityOutputUpdateOutcome::Applied {
                        notifications: delivered,
                        ..
                    } = &mut outcome
                    {
                        *delivered = notifications;
                    }
                }
                acknowledgement.try_send(outcome).map_err(|error| {
                    X11SetupSocketError::new(format!(
                        "failed to return Engine output topology acknowledgement: {error}"
                    ))
                })?;
                progressed = true;
            }
            Err(TryRecvError::Empty) => {}
        }

        if !ordered_egress.cancelled() {
            if pending_raster_egress.is_none() {
                match broker.broker()?.try_recv_raster_requirements() {
                    Ok(requirements) => {
                        let transaction = frontend.state.allocate_transaction()?;
                        let response = frontend
                            .state
                            .runtime
                            .lock()
                            .map_err(|_| {
                                X11SetupSocketError::new("X11 authority runtime lock poisoned")
                            })?
                            .apply_surface_raster_requirements(transaction, &requirements);
                        match response {
                            Ok(crate::XSurfaceRasterOutcome::Satisfied(response)) => {
                                raster_fallbacks.report_satisfied(
                                    &requirements,
                                    response.identity.source_content_generation,
                                );
                                let batch =
                                    XAuthorityObservedTransactionBatch::from_raster_response(
                                        *response,
                                    );
                                *pending_raster_egress =
                                    Some(XAuthorityBoundedEgressEnvelope::new(
                                        transaction,
                                        Some(batch),
                                    ));
                            }
                            Ok(crate::XSurfaceRasterOutcome::SampledFallback {
                                cause,
                                observed_content_generation,
                            }) => {
                                *pending_raster_egress = Some(
                                    XAuthorityBoundedEgressEnvelope::new(transaction, None),
                                );
                                raster_fallbacks.report(
                                    &requirements,
                                    cause,
                                    observed_content_generation,
                                );
                            }
                            Err(error) => {
                                *pending_raster_egress = Some(
                                    XAuthorityBoundedEgressEnvelope::new(transaction, None),
                                );
                                tracing::warn!(
                                    "sophia_x11_raster_requirement schema=1 status=refused surface={:?} content_generation={} requirement_generation={} error={error:?}",
                                    requirements.surface,
                                    requirements.committed_content_generation,
                                    requirements.requirement_generation,
                                );
                            }
                        }
                        progressed = true;
                    }
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => {}
                }
            }
            if pending_raster_egress.is_some() {
                // IN PLACE: the envelope stays in the caller's slot while the
                // observer is consulted, so an unwind there leaves it where
                // its owner can still reach it.
                let was_waiting = pending_raster_egress
                    .as_ref()
                    .is_some_and(|envelope| envelope.waiting_since.is_some());
                ordered_egress.try_submit(pending_raster_egress)?;
                progressed |= !was_waiting && pending_raster_egress.is_none();
            }
        }

        if accepting {
            while frontend.active_client_worker_count()
                < frontend.config().max_concurrent_clients().get()
            {
                if !frontend
                    .try_serve_next_concurrently_routed_traced(broker.broker()?, observer.clone())?
                {
                    break;
                }
                progressed = true;
            }
        }
        if !ordered_egress.cancelled() {
            let routed = broker.route_pending()?;
            progressed |= routed != 0;
            // AFTER ROUTING, FROM THIS FRAME. A connection that published its
            // readiness since the last turn gets its worker here; one that
            // was already visited is not visited again.
            progressed |= broker.attach_ready()? != 0;
        }
        let workers_before_reap = frontend.active_client_worker_count();
        frontend.poll_client_workers()?;
        progressed |= workers_before_reap != frontend.active_client_worker_count();
        if ordered_egress.transport_disconnected() {
            return Err(X11SetupSocketError::new(
                "X authority observed transaction channel is disconnected",
            ));
        }

        // A cancelled envelope still in the slot is the caller's to account
        // for; it does not keep this loop open.
        if !accepting
            && frontend.active_client_worker_count() == 0
            && pending_raster_egress
                .as_ref()
                .is_none_or(|envelope| envelope.cancelled)
        {
            return Ok(());
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// One retained obligation: the invocation that left it -- the number its
/// store gave its reservation, before exposure -- and the transaction it was
/// for. Transactions restart per frontend; the pair does not, WITHIN THE
/// STORE THAT ISSUED IT. The instance number is scoped to its originating
/// store; the same pair read against another store names something else,
/// and this pair grants nothing -- no replay, no driving.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateUnresolvedEgress {
    pub instance: u64,
    pub transaction: TransactionId,
}

/// What a private service invocation returns when it ran to a stop.
///
/// THE UNRESOLVED PART IS EXPLICIT. The settlement accounts for the private
/// input order this service owns; it says nothing about authority egress the
/// service was still waiting to send when it stopped. That is here, by exact
/// transaction, and the envelopes are on the shelf of the store the leased
/// owner is established over.
#[cfg(unix)]
pub struct PrivateServiceReturn {
    pub settlement: PrivateSettlement,
    /// The obligations this invocation left unsent on the store's shelf.
    pub unresolved_egress: Vec<PrivateUnresolvedEgress>,
    /// Every registered worker this invocation started, as its collection
    /// found it. Collection is not settlement: each of these still leaves
    /// its destruction request, its number and any deferred duty with the
    /// owner the caller kept.
    pub workers: Vec<PrivateWorkerCollection>,
}

/// Why a private service invocation did not return a settlement.
#[cfg(unix)]
pub enum PrivateServiceFailure {
    /// No frontend was made. The parts come back untouched, and nothing was
    /// bound, admitted or collected.
    Refused {
        refusal: AdmissionRefusal,
        /// Boxed only for size: the parts are the caller's and come back.
        parts: Box<PrivateFrontendParts>,
    },
    /// The service ran and failed. Its settlement is here, not discarded:
    /// what the failure left is accounted for by the same handle a success
    /// returns, and the custody inventory is with the owner the caller kept.
    Failed {
        error: X11SetupSocketError,
        /// Boxed only for size; it is the same handle a success returns.
        settlement: Box<PrivateSettlement>,
        /// The obligations this invocation left unsent on the store's shelf.
        unresolved_egress: Vec<PrivateUnresolvedEgress>,
        workers: Vec<PrivateWorkerCollection>,
    },
    /// A registered worker this invocation started was not joined by its
    /// collection, so private state was NOT finalised over it.
    ///
    /// THE FRONTEND COMES BACK UNSETTLED, with the actor still admitted in the
    /// owner's custody: what to do with an actor nobody could collect is the
    /// caller's, and finalising over it would say the service ended cleanly
    /// when it did not. `error` is the service's own outcome, kept separate
    /// from the collection that failed.
    Uncollected {
        error: Option<X11SetupSocketError>,
        frontend: Box<PrivateXServerFrontend>,
        unresolved_egress: Vec<PrivateUnresolvedEgress>,
        workers: Vec<PrivateWorkerCollection>,
        uncollected: Vec<usize>,
    },
}

#[cfg(unix)]
impl std::fmt::Debug for PrivateServiceFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { refusal, .. } => formatter
                .debug_struct("Refused")
                .field("refusal", refusal)
                .finish_non_exhaustive(),
            Self::Failed { error, .. } => formatter
                .debug_struct("Failed")
                .field("error", error)
                .finish_non_exhaustive(),
            Self::Uncollected {
                error, uncollected, ..
            } => formatter
                .debug_struct("Uncollected")
                .field("error", error)
                .field("uncollected", uncollected)
                .finish_non_exhaustive(),
        }
    }
}

/// What has to be collected before the private frontend may be finalised,
/// owned so that it is collected on every exit.
///
/// THIS IS WHAT THE PUBLIC FRONTEND LACKS. `XServerFrontend` has no `Drop`;
/// the public entry point collects its workers explicitly on its ordinary and
/// error paths, and an unwind inside its loop collects nothing. Here the same
/// explicit sequence runs on those paths and reports its failures, and the
/// `Drop` below runs it when nothing else did -- which is the unwind case.
#[cfg(unix)]
struct PrivateServiceCollection<'s, 'o> {
    frontend: XServerFrontend,
    egress: Arc<XAuthorityOrderedEgress>,
    /// The one raster envelope that can be waiting to leave. It lives HERE,
    /// in the guard, and is submitted in place, so that neither a return nor
    /// an unwind finds it in a local that has gone.
    pending_raster_egress: Option<XAuthorityBoundedEgressEnvelope>,
    /// Where unresolved egress goes when this frame ends: the store the
    /// leased owner is established over, which outlives the invocation.
    store: &'s PrivateSettlementOwner,
    /// The invocation any retained egress is shelved under.
    instance: u64,
    collected: bool,
    /// The lease this invocation runs under, for reaching its own custodies.
    service: PrivateServiceLease<'o>,
    /// This invocation's registry, by which its custodies are selected.
    registry: XServerFrontendRouteRegistry,
    /// What collecting the started workers found, read by the caller after
    /// the explicit collection and before this is dropped.
    workers: Vec<PrivateWorkerCollection>,
    /// Places whose worker this collection could not join.
    uncollected: Vec<usize>,
}

#[cfg(unix)]
impl PrivateServiceCollection<'_, '_> {
    /// File a pending envelope that still holds its batch, unsent, on the
    /// store's shelf under this invocation; drop one whose batch the
    /// transport already took.
    ///
    /// WHAT HAPPENED AT THE EFFECT DECIDES. The transport takes the batch
    /// out of the envelope when it accepts it, and the ticket is advanced
    /// under the order lock before the observer is told; an observer that
    /// unwinds after that leaves an envelope with no batch, and that is
    /// delivered work with an unfinished report -- not unsent work, and it
    /// is not shelved as such. An envelope still holding its batch was never
    /// accepted, and that is what the shelf keeps. NOT A SETTLEMENT EITHER
    /// WAY: cancelling a wait (where that is done) publishes that the batch
    /// was not delivered; shelving grants no replay; nothing takes it back
    /// out, and its charge stays on the store while it is there.
    fn retain_pending(&mut self) -> Vec<PrivateUnresolvedEgress> {
        if let Some(envelope) = self.pending_raster_egress.take()
            && envelope.batch.is_some()
        {
            let obligation = PrivateUnresolvedEgress {
                instance: self.instance,
                transaction: envelope.transaction,
            };
            self.store.retain_unresolved_egress(self.instance, envelope);
            return vec![obligation];
        }
        Vec::new()
    }
    /// Stop admission, unblock what a worker could be parked in, stop every
    /// current worker, then wait for every one.
    ///
    /// EACH STEP RUNS WHETHER OR NOT THE ONE BEFORE IT FAILED, and each
    /// failure is reported rather than returned early: a worker whose
    /// shutdown request failed is still waited for, and a wait that failed
    /// is still recorded. `unblock` is whether the egress paths are cancelled
    /// first; the ordinary stop keeps a draining worker's egress, as the
    /// public path does, while an error or an unwind cancels it so that
    /// collection cannot depend on a receiver anybody drains.
    fn collect(&mut self, unblock: bool) -> (Vec<String>, Vec<PrivateUnresolvedEgress>) {
        let mut failures = Vec::new();
        if unblock {
            self.egress.cancel();
            if let Some(envelope) = self.pending_raster_egress.as_mut()
                && let Err(error) = self.egress.cancel_envelope(envelope)
            {
                failures.push(format!("pending raster cancellation failed: {error}"));
            }
        }
        // ON EVERY EXIT. An ordinary stop cancelled the envelope's wait in
        // the loop and left it here; an error cancels it above. Either way an
        // unsent batch is unresolved and goes to the store, not to the floor.
        // EVERY ATTACHED WORKER IS TOLD TO STOP AND MADE INTERRUPTIBLE BEFORE
        // ANYTHING IS WAITED FOR: the legacy connection threads below wait on
        // sockets these workers may be blocked writing to.
        failures.extend(stop_attached_workers(&self.service, &self.registry));
        let unresolved = self.retain_pending();
        failures.extend(self.stop_and_wait());
        // THEN THE REGISTERED WORKERS, THROUGH THE JOIN CUSTODY, after their
        // connection threads have ended. A connection thread ending is not
        // its worker ending; only this join is.
        let (workers, uncollected) = collect_attached_workers(&self.service, &self.registry);
        self.workers = workers;
        self.uncollected = uncollected;
        // MARKED DONE ONLY AT THE END. A collection that unwound part-way
        // (the envelope cancellation reports to an observer that can panic)
        // is not a collection, and the guard below must still stop and wait.
        self.collected = true;
        (failures, unresolved)
    }

    /// The two steps that can never be skipped, in order.
    fn stop_and_wait(&mut self) -> Vec<String> {
        let mut failures = Vec::new();
        if let Err(error) = self.frontend.shutdown_all_client_workers() {
            failures.push(format!("worker shutdown failed: {error}"));
        }
        if let Err(error) = self.frontend.wait_for_clients() {
            failures.push(format!("worker reap failed: {error}"));
        }
        failures
    }
}

#[cfg(unix)]
impl Drop for PrivateServiceCollection<'_, '_> {
    fn drop(&mut self) {
        // Reached with `collected` false only when the operation unwound
        // before its explicit collection. Nothing here can report, so it
        // cancels and collects and says nothing; what it guarantees is that
        // the private frontend, declared before this guard and therefore
        // dropped after it, is not finalised over a worker still running.
        if !self.collected {
            // Cancel first, so a worker parked in an egress wait can end;
            // then stop and wait. The pending raster envelope, if any, is
            // NOT cancelled here -- cancelling reports to the backpressure
            // observer, and an observer that panicked once may panic again;
            // during an unwind that is an abort -- but an unsent batch IS
            // retained: it goes to the store, where the owner the caller
            // kept can reach it. Its cancellation stays unpublished; the
            // work does not go missing, and a batch the transport already
            // took is not called unsent.
            self.egress.cancel();
            let _ = stop_attached_workers(&self.service, &self.registry);
            let _ = self.stop_and_wait();
            let (workers, uncollected) =
                collect_attached_workers(&self.service, &self.registry);
            self.workers = workers;
            self.uncollected = uncollected;
            let _ = self.retain_pending();
            self.collected = true;
        }
    }
}

/// Run the private routed service until it is stopped, over an owner the
/// caller keeps.
///
/// THE OWNER IS REQUIRED AND BORROWED. The private frontend is made through
/// its mandatory-owner constructor on this path; the caller establishes the
/// owner before this call and retains it afterwards, on the ordinary, error
/// and unwind returns alike, and inspects the exact custody through it. A
/// refused construction returns the parts. Session has not selected this
/// entry point; it exists beside the public one and nothing switches to it
/// here.
#[cfg(unix)]
pub fn run_x_server_frontend_private_until_stopped(
    config: XServerFrontendConfig,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    parts: PrivateFrontendParts,
    owner: &PrivateServiceOwner,
    service_commands: Receiver<XServerFrontendServiceCommand>,
    backpressure_observer: Arc<XAuthorityBackpressureObserver>,
) -> Result<PrivateServiceReturn, PrivateServiceFailure> {
    let private = match PrivateXServerFrontend::new(parts, owner) {
        Ok(private) => private,
        Err((refusal, parts)) => {
            return Err(PrivateServiceFailure::Refused {
                refusal,
                parts: Box::new(parts),
            });
        }
    };
    let service = owner.lease();
    serve_private_frontend_until_stopped(
        private,
        &service,
        config,
        transaction_sender,
        service_commands,
        backpressure_observer,
    )
}

/// The service over an already-made private frontend and a lease.
///
/// Separated from the entry point so that a lease on the wrong owner is a
/// case a control can arrange: the association between the frontend and its
/// owner exists from construction, and a lease that is not on that owner is
/// refused before a listener is bound.
#[cfg(unix)]
pub(crate) fn serve_private_frontend_until_stopped(
    mut private: PrivateXServerFrontend,
    service: &PrivateServiceLease<'_>,
    config: XServerFrontendConfig,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    service_commands: Receiver<XServerFrontendServiceCommand>,
    backpressure_observer: Arc<XAuthorityBackpressureObserver>,
) -> Result<PrivateServiceReturn, PrivateServiceFailure> {
    // THE LEASE IS CHECKED BEFORE ANYTHING IS BOUND. A foreign lease is not
    // a service that failed; it is a service that never began, and the
    // frontend it was handed is finalised into its own owner's store.
    if !private.broker.registry.leased_by(service) {
        let settlement = private.shutdown();
        return Err(PrivateServiceFailure::Failed {
            error: X11SetupSocketError::new(
                "private service lease is not on the owner that keeps this frontend's registry",
            ),
            settlement: Box::new(settlement),
            unresolved_egress: Vec::new(),
            workers: Vec::new(),
        });
    }
    let namespace = config.namespace();
    let frontend = match XServerFrontend::bind(config) {
        Ok(frontend) => frontend,
        Err(error) => {
            let settlement = private.shutdown();
            return Err(PrivateServiceFailure::Failed {
                error,
                settlement: Box::new(settlement),
                unresolved_egress: Vec::new(),
                workers: Vec::new(),
            });
        }
    };
    if let Err(error) = frontend
        .state
        .runtime
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 authority runtime lock poisoned"))
        .map(|mut runtime| {
            runtime.set_input_authority(private.broker.registry.input_authority.clone());
        })
    {
        let settlement = private.shutdown();
        return Err(PrivateServiceFailure::Failed {
            error,
            settlement: Box::new(settlement),
            unresolved_egress: Vec::new(),
            workers: Vec::new(),
        });
    }
    // THE APPLIED REGISTRY IS PREPARED FOR THIS SERVICE'S NAMESPACE before any
    // connection can be admitted: promotion establishes each connection's
    // served endpoint through it, and an instance that never prepared it
    // could promote nothing. This is the owner alone; the private input
    // pipeline stays unattached.
    if let Err(refusal) = private.prepare_applied_for_service(namespace) {
        let settlement = private.shutdown();
        return Err(PrivateServiceFailure::Failed {
            error: X11SetupSocketError::new(format!(
                "private applied registry could not be prepared: {refusal:?}"
            )),
            settlement: Box::new(settlement),
            unresolved_egress: Vec::new(),
            workers: Vec::new(),
        });
    }
    let cancellation = Arc::new(AtomicBool::new(false));
    let ordered_egress = Arc::new(XAuthorityOrderedEgress::new(
        transaction_sender,
        cancellation,
        backpressure_observer,
    ));
    let worker_egress = ordered_egress.clone();
    let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
        if trace.failure == Some(X11ObservedDispatchFailure::UnpublishedEffects) {
            worker_egress.cancel();
            return Err(X11SetupSocketError::new(
                "X11 dispatch ended with unpublished authority effects",
            ));
        }
        let batch = XAuthorityObservedTransactionBatch::from_dispatch_observation(&trace);
        let receipt = batch.as_ref().map(|batch| batch.transaction);
        worker_egress.submit_blocking(XAuthorityBoundedEgressEnvelope::new(
            trace.transaction,
            batch,
        ))?;
        Ok(receipt)
    });

    // DECLARATION ORDER IS LOAD-BEARING: `private` above, `collection`
    // below, so that on an unwind the collection guard drops first and the
    // private frontend's own fallback runs only after every worker is
    // collected. Nothing enforces this by type.
    let mut collection = PrivateServiceCollection {
        frontend,
        egress: ordered_egress.clone(),
        pending_raster_egress: None,
        store: service.store(),
        instance: private.instance,
        collected: false,
        service: *service,
        registry: private.broker.registry.clone(),
        workers: Vec::new(),
        uncollected: Vec::new(),
    };
    let service_result = {
        let mut broker = LeasedPrivateBroker {
            frontend: &mut private,
            service,
        };
        let PrivateServiceCollection {
            frontend,
            pending_raster_egress,
            ..
        } = &mut collection;
        drive_routed_service(
            frontend,
            &mut broker,
            &service_commands,
            &ordered_egress,
            &observer,
            pending_raster_egress,
        )
    };

    // EXPLICIT COLLECTION ON THE ORDINARY AND ERROR PATHS, reported. The
    // guard's Drop is for the unwind that never reaches this line.
    let (cleanup_failures, unresolved_egress) = collection.collect(service_result.is_err());
    let workers = std::mem::take(&mut collection.workers);
    let uncollected = std::mem::take(&mut collection.uncollected);
    drop(observer);
    let report = ordered_egress.report();
    let status = if service_result.is_err() {
        "error"
    } else if ordered_egress.cancelled() {
        "cancelled"
    } else {
        "drained"
    };
    if let Ok(report) = report.as_ref() {
        tracing::info!(
            "sophia_x11_private_authority_egress schema=1 status={} tickets_advanced={} batches_delivered={} peak_waiting_producers={} wait_episodes={} resumed={} cancelled={}",
            status,
            report.tickets_advanced,
            report.batches_delivered,
            report.peak_waiting_producers,
            report.wait_episodes,
            report.resumed,
            report.cancelled,
        );
    }
    // ONLY NOW IS THE PRIVATE FRONTEND FINALISED: every worker is collected
    // above, and the collection guard is disposed of before the frontend it
    // guarded. The settlement goes back on both outcomes.
    drop(collection);
    if !uncollected.is_empty() {
        // NOT FINALISED OVER AN UNCOLLECTED ACTOR. The frontend goes back
        // unsettled with the service's own outcome beside the collection's.
        return Err(PrivateServiceFailure::Uncollected {
            error: service_result.err(),
            frontend: Box::new(private),
            unresolved_egress,
            workers,
            uncollected,
        });
    }
    let settlement = private.shutdown();
    match (service_result, report) {
        (Ok(()), Ok(_)) if cleanup_failures.is_empty() => Ok(PrivateServiceReturn {
            settlement,
            unresolved_egress,
            workers,
        }),
        (Ok(()), Ok(_)) => Err(PrivateServiceFailure::Failed {
            error: X11SetupSocketError::new("private service stopped, but collection failed")
                .with_cleanup_failures(cleanup_failures),
            settlement: Box::new(settlement),
            unresolved_egress,
            workers,
        }),
        (Ok(()), Err(error)) => Err(PrivateServiceFailure::Failed {
            error: error.with_cleanup_failures(cleanup_failures),
            settlement: Box::new(settlement),
            unresolved_egress,
            workers,
        }),
        (Err(original), report) => {
            let mut cleanup_failures = cleanup_failures;
            if let Err(error) = report {
                cleanup_failures.push(format!("authority egress report failed: {error}"));
            }
            Err(PrivateServiceFailure::Failed {
                error: original.with_cleanup_failures(cleanup_failures),
                settlement: Box::new(settlement),
                unresolved_egress,
                workers,
            })
        }
    }
}
