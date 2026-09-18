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
// loop is shared with the public entry point and driven here against the
// private frontend's own broker, but every reach to that broker
// goes through the checked service lease: a frontend whose registry is not
// kept by the leased owner refuses before a listener is bound. The runner is
// prepared after binding and stays owned by the collection guard on this
// thread. Producers are issued through its port. The ordered worker a ready
// connection gets is started by this loop's own visit (`attach_ready`),
// stopped and collected by its collection, and its deferred cleanup
// discharged after.
//
// EXIT ORDER IS CONTROL FLOW, NOT CONVENTION. A cancelling stop or loss of
// the command channel closes producer issuance and acceptance at the loop's
// decision, before reporting cancellation or stopping sockets. The guard
// repeats that closure at collection, including after an error or unwind:
// admission stops, the egress paths a worker can be blocked in are cancelled,
// every current legacy client worker is told to
// stop and then waited for, and only then is the private frontend finalised.
// The ordinary and error paths do this explicitly and report each cleanup
// failure without letting one skip the next or replace the original error;
// the unwind path does it through the collection guard's `Drop`, which the
// existing public frontend has no equivalent of. The guard owns the runner
// and its private frontend, so they drop after the guard's collection body.
// Graceful draining keeps its existing egress and execution policy until
// collection; StopAccepting only stops accepting new connections.

/// The routed service loop, shared by the public and private entry points.
///
/// The adapter preserves the public path's behavior while giving the private
/// path its leased runner, producer handoff, and admission closure at a
/// cancelling stop decision.
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
                // Close even if a worker already cancelled egress: nothing
                // may be issued or accepted into an order this loop stopped
                // serving. The guard repeats this before later collection.
                broker.close_private_producers();
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
            // PRODUCERS FIRST, THEN THE ORDER: a request answered this turn
            // can submit work the same turn's service takes.
            progressed |= broker.answer_producers()? != 0;
            let routed = broker.serve_order()?;
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
    /// What each custody's deferred cleanup visit established or refused,
    /// after collection. A refusal here is an owned duty, readable through
    /// the owner's custody; it is not a settled connection.
    pub maintenance: Vec<PrivateDeferredCleanupOutcome>,
    /// What the prepared runner's order did over the invocation: counts for
    /// the owner to read beside the exact work in the store and the homes.
    pub order: PrivateOrderTally,
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
        maintenance: Vec<PrivateDeferredCleanupOutcome>,
        /// Boxed only for size; the same tally a success returns inline.
        order: Box<PrivateOrderTally>,
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
        /// What cancellation, interruption and reporting failed with, kept
        /// beside the service's own error rather than folded into it.
        collection_failures: Vec<String>,
        maintenance: Vec<PrivateDeferredCleanupOutcome>,
        /// Boxed only for size; the same tally a success returns inline.
        order: Box<PrivateOrderTally>,
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
    execution: &'s mut PrivateServiceExecutionKeeper,
    connections: Option<PrivateConnectionsCollected>,
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
    /// What visiting each custody's deferred cleanup after collection
    /// established or refused.
    maintenance: Vec<PrivateDeferredCleanupOutcome>,
    /// The instance's own record of the same, written here so that disposal
    /// -- after a return the caller drops, or after an unwind that returns
    /// nothing -- retains the instance rather than settling over the actor.
    uncollected_mark: Arc<Mutex<Vec<usize>>>,
    /// The prepared runner, owned here for the invocation: the loop borrows
    /// it, the exit closes its admission first, and it is taken out (to be
    /// shut down or released) only after collection. On unwind its original
    /// execution resources move to the caller's same-thread keeper.
    runner: Option<PrivatePreparedRunner>,
    /// The producer port, closed as the exit's first act.
    port: PrivateProducerPort,
    order: PrivateOrderTally,
}

#[cfg(unix)]
impl PrivateServiceCollection<'_, '_> {
    /// THE FIRST ACT OF EVERY EXIT: nothing more is issued from the port and
    /// the producers already issued refuse. The port closes (standing Ended,
    /// its request channel gone) and the runner's supervising watchdog owner
    /// closes production through the independent gate every producer's
    /// acceptance consults. That is a gate, not the admission's queue: no
    /// lock of the accepted order is taken and nothing is drained; an
    /// acceptance already inside the queue stays there for the settlement to
    /// account for. Nothing is waited for and nothing retained is touched.
    /// Safe to repeat.
    fn close_producer_admission(&mut self) {
        self.port.close();
        if let Some(runner) = self.runner.as_mut() {
            runner.close_admission();
        }
        // STAGE-ONLY SCHEDULING HOOK, TEST BUILDS ONLY: the interval after
        // admission is closed and before anything is stopped or waited for.
        #[cfg(all(test, unix))]
        routing_tests::stage_after_admission_closed(&self.registry);
    }
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
        self.close_producer_admission();
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
        self.note_uncollected(&uncollected);
        self.workers = workers;
        self.uncollected = uncollected;
        // THEN THE DEFERRED CLEANUPS, after every collection above and under
        // this collection's own word that the connection frames are gone:
        // each custody discharges its own where its prerequisites are
        // established and refuses, visibly, where not.
        self.connections = self.connections_collected();
        self.maintenance = run_deferred_cleanups(&self.service, &self.registry, self.connections.as_ref());
        // MARKED DONE ONLY AT THE END. A collection that unwound part-way
        // (the envelope cancellation reports to an observer that can panic)
        // is not a collection, and the guard below must still stop and wait.
        self.collected = true;
        (failures, unresolved)
    }

    /// The collection's word that this registry's connection frames are all
    /// collected: minted only after the wait, and only when no frame is
    /// still active. A wait that returned an error after reaping every
    /// frame still mints it; one that returned without reaping does not.
    fn connections_collected(&self) -> Option<PrivateConnectionsCollected> {
        (self.frontend.active_client_worker_count() == 0).then(|| PrivateConnectionsCollected {
            registry: Arc::clone(&self.registry.clients),
        })
    }

    /// Leave the uncollected places with the instance itself.
    fn note_uncollected(&self, uncollected: &[usize]) {
        if uncollected.is_empty() {
            return;
        }
        let mut mark = match self.uncollected_mark.lock() {
            Ok(mark) => mark,
            Err(poisoned) => poisoned.into_inner(),
        };
        mark.extend_from_slice(uncollected);
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
        // the private frontend owned inside this guard's runner is not
        // finalised over a worker still running: fields drop after this body.
        if !self.collected {
            self.close_producer_admission();
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
            self.note_uncollected(&uncollected);
            self.workers = workers;
            self.uncollected = uncollected;
            if self.connections.is_none() {
                self.connections = self.connections_collected();
            }
            self.maintenance =
                run_deferred_cleanups(&self.service, &self.registry, self.connections.as_ref());
            let _ = self.retain_pending();
            self.collected = true;
        }
        // Handoff still happens when collection reported failures. The
        // frontend's fallback owns the inventory; the outer keeper owns the
        // exact execution resources until a later authorized visit or loss.
        if let Some(runner) = self.runner.take() {
            drop(self.execution.retain(runner, self.connections.take()));
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
///
/// The execution keeper is established on this thread outside this call and
/// any catch_unwind scope. Collection hands it the original keyboard history,
/// supervisor and accounting even when the service fails or unwinds. An
/// occupied keeper refuses a new invocation before binding. Dropping the
/// keeper records history loss beside the independently retained obligations.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)] // Separate durable and same-thread owners are mandatory.
pub fn run_x_server_frontend_private_until_stopped(
    config: XServerFrontendConfig,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    parts: PrivateFrontendParts,
    owner: &PrivateServiceOwner,
    execution: &mut PrivateServiceExecutionKeeper,
    service_commands: Receiver<XServerFrontendServiceCommand>,
    producers: PrivateProducerPort,
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
        execution,
        config,
        transaction_sender,
        service_commands,
        producers,
        backpressure_observer,
    )
}

/// What a caller supplies to serve a private frontend it already holds.
///
/// A bundle rather than loose arguments, so the method below stays inside the
/// argument count the style guide allows without an exemption, and so a caller
/// assembles the service's channels in one place instead of at a call site.
/// Nothing here is the broker or a raw client sender: those stay inside the
/// frontend, which is the whole point of handing one over rather than its
/// parts.
#[cfg(unix)]
pub struct PrivateServiceBinding {
    pub config: XServerFrontendConfig,
    pub transactions: SyncSender<XAuthorityObservedTransactionBatch>,
    pub commands: Receiver<XServerFrontendServiceCommand>,
    pub producers: PrivateProducerPort,
    pub backpressure: Arc<XAuthorityBackpressureObserver>,
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Serve this frontend until it is stopped.
    ///
    /// FOR A CALLER THAT ALREADY HOLDS THE FRONTEND. The convenience entry
    /// beside this one builds the frontend from its parts and never hands it
    /// back, so a caller that must read the boundary it is about to serve --
    /// Session, which keeps the admission participant -- cannot use it. This
    /// takes the frontend the caller constructed and made its own arrangements
    /// against.
    ///
    /// Every check the other path performs still runs: this forwards to the
    /// same internal service, which refuses a lease that is not on the owner
    /// keeping this frontend's registry, and refuses an execution keeper that
    /// already retains an invocation, both before anything is bound.
    pub fn serve_until_stopped(
        self,
        service: &PrivateServiceLease<'_>,
        execution: &mut PrivateServiceExecutionKeeper,
        binding: PrivateServiceBinding,
    ) -> Result<PrivateServiceReturn, PrivateServiceFailure> {
        serve_private_frontend_until_stopped(
            self,
            service,
            execution,
            binding.config,
            binding.transactions,
            binding.commands,
            binding.producers,
            binding.backpressure,
        )
    }
}

/// The service over an already-made private frontend and a lease.
///
/// Separated from the entry point so that a lease on the wrong owner is a
/// case a control can arrange: the association between the frontend and its
/// owner exists from construction, and a lease that is not on that owner is
/// refused before a listener is bound.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)] // Internal form keeps the same ownership boundary.
pub(crate) fn serve_private_frontend_until_stopped(
    private: PrivateXServerFrontend,
    service: &PrivateServiceLease<'_>,
    execution: &mut PrivateServiceExecutionKeeper,
    config: XServerFrontendConfig,
    transaction_sender: SyncSender<XAuthorityObservedTransactionBatch>,
    service_commands: Receiver<XServerFrontendServiceCommand>,
    mut producers: PrivateProducerPort,
    backpressure_observer: Arc<XAuthorityBackpressureObserver>,
) -> Result<PrivateServiceReturn, PrivateServiceFailure> {
    // THE LEASE IS CHECKED BEFORE ANYTHING IS BOUND. A foreign lease is not
    // a service that failed; it is a service that never began, and the
    // frontend it was handed is finalised into its own owner's store.
    if !private.broker.registry.leased_by(service) || execution.resources.is_some() {
        let message = if execution.resources.is_some() {
            "private execution keeper already retains an invocation"
        } else {
            "private service lease is not on the owner that keeps this frontend's registry"
        };
        producers.close();
        let settlement = private.shutdown();
        return Err(PrivateServiceFailure::Failed {
            error: X11SetupSocketError::new(message),
            settlement: Box::new(settlement),
            unresolved_egress: Vec::new(),
            workers: Vec::new(),
            maintenance: Vec::new(),
            order: Box::default(),
        });
    }
    let namespace = config.namespace();
    // EXCLUSIVE, ALWAYS, AND NOT A CHOICE THE CALLER MAKES. A private service
    // that found its socket occupied and reclaimed it would displace whatever
    // was serving there, leaving that service on an unlinked inode no client
    // can reach while both believe they own the path. An occupied path here is
    // a refusal to start.
    let frontend = match XServerFrontend::bind_exclusive(config) {
        Ok(frontend) => frontend,
        Err(error) => {
            producers.close();
            let settlement = private.shutdown();
            return Err(PrivateServiceFailure::Failed {
                error,
                settlement: Box::new(settlement),
                unresolved_egress: Vec::new(),
                workers: Vec::new(),
                maintenance: Vec::new(),
                order: Box::default(),
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
        producers.close();
        let settlement = private.shutdown();
        return Err(PrivateServiceFailure::Failed {
            error,
            settlement: Box::new(settlement),
            unresolved_egress: Vec::new(),
            workers: Vec::new(),
            maintenance: Vec::new(),
            order: Box::default(),
        });
    }
    // THE ONE CONTINUING EXECUTION OWNER IS PREPARED HERE, after the
    // listener is bound and before any connection can be admitted or any
    // producer asked for: one keyboard history, namespace, seat and native
    // association for the invocation, and the applied owner installed once
    // as part of it (promotion establishes each connection's served endpoint
    // through that owner). The runner is made on this thread and never
    // leaves it. A refusal hands the frontend back, and it is finalised into
    // its settlement like every other setup refusal; the port is closed, so
    // a caller sees Ended, never a service that stays NotReady for good.
    let runner = match private.prepare_runner(namespace, service.owner()) {
        Ok(runner) => runner,
        Err((refusal, private)) => {
            producers.close();
            let settlement = private.shutdown();
            return Err(PrivateServiceFailure::Failed {
                error: X11SetupSocketError::new(format!(
                    "private runner could not be prepared: {refusal:?}"
                )),
                settlement: Box::new(settlement),
                unresolved_egress: Vec::new(),
                workers: Vec::new(),
                maintenance: Vec::new(),
                order: Box::default(),
            });
        }
    };
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

    // THE RUNNER LIVES IN THE COLLECTION GUARD: the guard's Drop body
    // collects first and hands its original execution resources to the outer
    // keeper before finalizing the frontend. The same handoff runs on unwind,
    // including when collection has failures. The loop borrows the runner.
    let instance = runner.frontend().instance;
    let registry = runner.frontend().broker.registry.clone();
    let uncollected_mark = runner.frontend().uncollected_mark();
    let mut collection = PrivateServiceCollection {
        execution,
        connections: None,
        frontend,
        egress: ordered_egress.clone(),
        pending_raster_egress: None,
        store: service.store(),
        instance,
        collected: false,
        service: *service,
        registry,
        workers: Vec::new(),
        uncollected: Vec::new(),
        maintenance: Vec::new(),
        uncollected_mark,
        runner: Some(runner),
        port: producers,
        order: PrivateOrderTally::default(),
    };
    // READY ONLY NOW: prepared (the applied owner installed with it), bound,
    // and guarded so that every exit from here closes the port. A caller
    // asking before this was refused at its own side, nothing queued.
    collection.port.publish_ready();
    let service_result = {
        let PrivateServiceCollection {
            frontend,
            pending_raster_egress,
            runner,
            port,
            order,
            ..
        } = &mut collection;
        let mut broker = LeasedPrivateBroker {
            runner: runner.as_mut().expect("owned until collection"),
            port,
            order,
            service,
        };
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
    let maintenance = std::mem::take(&mut collection.maintenance);
    let order = collection.order;
    let boxed_order = Box::new(order);
    // OUT OF THE GUARD ONLY AFTER COLLECTION: admission was closed as the
    // exit's first act; what remains is to finalise or release the frontend
    // the runner still holds, below, once the guard is gone.
    let runner = collection.runner.take().expect("owned until collection");
    let private = collection.execution.retain(runner, collection.connections.take());
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
        let mut collection_failures = cleanup_failures;
        if let Err(error) = report {
            collection_failures.push(format!("authority egress report failed: {error}"));
        }
        return Err(PrivateServiceFailure::Uncollected {
            error: service_result.err(),
            frontend: Box::new(private),
            unresolved_egress,
            workers,
            uncollected,
            collection_failures,
            maintenance,
            order: boxed_order,
        });
    }
    let settlement = private.shutdown();
    match (service_result, report) {
        (Ok(()), Ok(_)) if cleanup_failures.is_empty() => Ok(PrivateServiceReturn {
            settlement,
            unresolved_egress,
            workers,
            maintenance,
            order,
        }),
        (Ok(()), Ok(_)) => Err(PrivateServiceFailure::Failed {
            error: X11SetupSocketError::new("private service stopped, but collection failed")
                .with_cleanup_failures(cleanup_failures),
            settlement: Box::new(settlement),
            unresolved_egress,
            workers,
            maintenance,
            order: Box::new(order),
        }),
        (Ok(()), Err(error)) => Err(PrivateServiceFailure::Failed {
            error: error.with_cleanup_failures(cleanup_failures),
            settlement: Box::new(settlement),
            unresolved_egress,
            workers,
            maintenance,
            order: Box::new(order),
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
                maintenance,
                order: Box::new(order),
            })
        }
    }
}
