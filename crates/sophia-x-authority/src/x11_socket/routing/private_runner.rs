/// Why an act that continues a service was refused.
///
/// TWO DIFFERENT KINDS OF REFUSAL, kept apart. One is about this connection's
/// admission and is the participant's own answer; the other is about the
/// SERVICE -- the owner offered is not the one keeping this service's
/// connections' evidence -- and is decided before the participant is asked at
/// all. A caller told the wrong one would look at the wrong thing.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateServiceRefusal {
    /// The owner offered is not this service's keeper. Nothing was reserved,
    /// taken or advanced.
    ForeignServiceOwner,
    /// The participant's own answer about this connection.
    Admission(PrivateAdmissionRefusal),
}

/// Why a frontend could not become an execution runner. The frontend is
/// returned intact; no producer is exposed by a failed preparation.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateRunnerRefusal {
    ProducerAlreadyExposed,
    /// The owner offered is not the one this frontend's registry keeps its
    /// connections' evidence with.
    ///
    /// Distinct from every other refusal here: nothing is wrong with the
    /// frontend or the thread, and what is refused is the association.
    ForeignServiceOwner,
    /// A prepared native origin cannot be replaced, even before the first turn.
    AlreadyPrepared,
    Keyboard(PrivateKeyboardsRefusal),
    StateUnavailable,
}

/// One executing thread owns the frontend and its continuing keyboard history.
/// Preparation completes before this value can expose a reserving ingress.
/// Dropping it first closes its watchdog gate and transports, then drops the
/// frontend, handing obligations to its durable owner before the thread-local
/// keyboard state is destroyed. Transport shutdown cannot wait on common.
///
/// This value cannot be sent to a different executing thread:
/// ```compile_fail
/// fn move_runner(runner: sophia_x_authority::PrivatePreparedRunner) {
///     std::thread::spawn(move || drop(runner));
/// }
/// ```
#[cfg(unix)]
pub struct PrivatePreparedRunner {
    // Publish loss of original execution history before resources disappear.
    lifetime: PrivateExecutionLifetimeOwner,
    // Close the gate and transports before frontend Drop can enter common.
    watch: Option<private_watchdog::PrivateWatchdogOwner>,
    frontend: Option<PrivateXServerFrontend>,
    keyboards: PrivateKeyboards,
    namespace: NamespaceId,
    seat: SeatId,
    service_origin: std::time::Instant,
    service: sophia_input_authority::ServiceBudget,
    prefer_cleanup: bool,
    /// Where the bounded reclamation visit resumes in the outstanding list.
    reclaim_cursor: usize,
}

/// How many outstanding identities one accounted reclamation visit observes
/// at most.
#[cfg(unix)]
const PRIVATE_RECLAIM_VISIT_BOUND: usize = 8;

/// What one accounted reclamation visit did.
#[cfg(unix)]
enum PrivateAccountedReclaim {
    /// Nothing outstanding: no start spent, nothing watched.
    Idle,
    /// The cleanup allowance refused the visit; nothing observed.
    Yield {
        cause: sophia_input_authority::ServiceStartRefusal,
    },
    /// The visit ran under a charged cleanup start.
    Visited {
        observed: usize,
        reclaimed: usize,
        charge: Option<sophia_input_authority::ServiceCharge>,
        /// The supervisor would not take the visit (nothing observed) or
        /// its finish (what was reclaimed stands).
        unwatched: bool,
    },
}

/// Processing, enqueue and settlement are counted separately. An enqueued
/// event is not evidence that a writer flushed it.
#[cfg(unix)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PrivateRunnerProgress {
    pub taken: usize,
    pub refused: usize,
    pub enqueued: usize,
    pub observed: usize,
    pub settled: usize,
    /// How many proof-recording visits put a native bit in during this turn.
    ///
    /// Separate from `settled`: a native bit is one half of a debt, and a
    /// recording is not a receipt.
    pub recorded: usize,
    pub activation_pairs_observed: usize,
    pub activations_joined: usize,
    pub transient_observed: usize,
    /// Charged live native-custody visits and completed records disposed.
    pub native_disposal_observed: usize,
    pub native_disposed: usize,
    pub transient_disposed: usize,
    /// Whether the supervisor failed during this turn.
    ///
    /// Separate from `unwatched`, which names an entry: native work has none,
    /// and a failure reported only as an absent name is a failure nobody is
    /// told about.
    pub watch_failed: bool,
    /// How many delivery capsules reached a recipient's queue this turn.
    ///
    /// Separate from `recorded` and from `settled`: enqueueing is not a
    /// receipt, and a receipt is not the whole debt.
    pub dispatched: usize,
    /// How many claimed attempts were given back to the ledger this turn.
    ///
    /// Counted apart from `dispatched`: giving a slot back is not delivering,
    /// and a turn spent doing it made progress of a different kind.
    pub relinquished: usize,
    /// How many recipient halves were settled on established proof this turn.
    ///
    /// Only a flush or a terminated connection counts. A write that failed and
    /// a wait that ran out establish nothing and are counted nowhere.
    pub recipient_settled: usize,
    pub blocked: Option<crate::ReadySequence>,
    /// The service allowance stopped this turn. It will be checked again on
    /// the next owner-loop turn; waiting is not part of an operation.
    pub allowance: Option<sophia_input_authority::ServiceStartRefusal>,
    /// Elapsed execution and cleanup charged on this turn, including waits
    /// for execution guards. An overrun is measured, never called preemption.
    pub charged: std::time::Duration,
    pub overrun: std::time::Duration,
    pub unwatched: Option<crate::ReadySequence>,
    /// All charged starts, including service of previously decided work.
    pub starts: usize,
    pub terminal_steps: usize,
    /// Control and lease-release operations handed on from the order this
    /// turn: routed, not answered.
    pub routed: usize,
    /// Credits given back this turn by the accounted, watched, bounded visit
    /// of outstanding work's terminal outcomes: control records retired,
    /// public deliveries ended. Lease identities and unknown outcomes are
    /// never among them.
    pub reclaimed: usize,
    /// Outstanding identities the visit looked at this turn (reclaimed or
    /// still pending).
    pub reclaim_observed: usize,
    /// The cleanup allowance's refusal of the visit, if it refused.
    pub reclaim_refusal: Option<sophia_input_authority::ServiceStartRefusal>,
    /// The supervisor would not take the visit or its finish.
    pub reclaim_unwatched: bool,
    /// The last control or lease release routed this turn, by its position
    /// in the order.
    pub last_routed: Option<crate::ReadySequence>,
    /// Why the last refused execution this turn was refused.
    pub(crate) last_refusal: Option<PrivateExecutionRefusal>,
}

#[cfg(unix)]
impl PrivateRunnerProgress {
    fn record_charge(&mut self, charge: Option<sophia_input_authority::ServiceCharge>) -> bool {
        let Some(charge) = charge else {
            return false;
        };
        self.starts += 1;
        self.charged = self.charged.saturating_add(charge.elapsed);
        self.overrun = self.overrun.max(charge.allowance_overrun);
        !charge.allowance_overrun.is_zero()
            || !charge.cleanup_reservation_overrun.is_zero()
            || !charge.interval_boundary_overrun.is_zero()
    }
}

#[cfg(unix)]
enum PrivateAccountedStep {
    Yield {
        cause: sophia_input_authority::ServiceStartRefusal,
        taken: Option<crate::ReadySequence>,
    },
    Step {
        step: PrivateOrderedStep,
        charge: Option<sophia_input_authority::ServiceCharge>,
    },
}

#[cfg(unix)]
enum PrivateAccountedDelivery {
    Yield {
        cause: sophia_input_authority::ServiceStartRefusal,
        taken: Option<crate::ReadySequence>,
    },
    Step {
        step: PrivateDeliveryStep,
        charge: Option<sophia_input_authority::ServiceCharge>,
        /// THAT the supervisor failed, kept apart from WHICH entry it was.
        ///
        /// Native work has no ordered entry to name, so a failure carried only
        /// as an optional sequence would arrive as None and be indistinguishable
        /// from no failure at all. Nothing invents a sequence to make the
        /// failure reportable; the fact travels on its own.
        watch_failed: bool,
        unwatched: Option<crate::ReadySequence>,
    },
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Prepare on the thread that will execute turns, before granting an
    /// ingress. The instance's issuer supplies the seat; callers cannot choose
    /// a different seat for the same authority.
    /// THE OWNER IS BORROWED FOR THE PREPARATION, and it must be the one this
    /// frontend was built over. An execution scope that could be started with
    /// no live keeper, or with a different one, would be a service whose
    /// connections' evidence belongs to an inventory nobody can name from
    /// here.
    ///
    /// AND THE ACTS THAT CONTINUE THE SERVICE ASK AGAIN. Preparing says a live
    /// owner existed when the scope began; it does not by itself stop that
    /// owner being dropped afterwards, and a runner that went on taking
    /// ingresses and serving turns with no keeper would be accepting work
    /// whose evidence nothing outside it holds. So every such act takes a
    /// lease of its own: the borrow is what makes serving without a live
    /// keeper impossible to write, rather than merely unsupported.
    ///
    /// WHAT A SCOPE'S ENDING CANNOT DO, however it ends, is take the custodies
    /// with it, because it never owned them.
    #[allow(clippy::result_large_err)] // Refusal returns the caller's owned frontend without boxing.
    pub fn prepare_runner(
        mut self,
        namespace: NamespaceId,
        owner: &PrivateServiceOwner,
    ) -> Result<PrivatePreparedRunner, (PrivateRunnerRefusal, Self)> {
        if self.ordered_runner {
            return Err((PrivateRunnerRefusal::ProducerAlreadyExposed, self));
        }
        // ASKED BEFORE ANYTHING IS INSTALLED, so a refusal leaves the frontend
        // exactly as it arrived.
        if !self
            .broker
            .registry
            .custody_keeper_is(owner)
        {
            return Err((PrivateRunnerRefusal::ForeignServiceOwner, self));
        }
        if self.native_owner.is_some() {
            return Err((PrivateRunnerRefusal::AlreadyPrepared, self));
        }
        // Installation takes common itself and binds the actual connection
        // projections before any producer can reserve against this runner.
        if self
            .broker
            .registry
            .install_private_applied(&self.participant, namespace)
            .is_err()
        {
            return Err((PrivateRunnerRefusal::StateUnavailable, self));
        }
        let seat = self.submit.binding().seat();
        // These allocations initialize the supported namespace before any
        // execution can consume its state. No keymap work happens under common.
        let prepared = self.controller.under_common(|_| {
            let mut pointers = self.broker.registry.pointer_state.lock().map_err(|_| ())?;
            let mut grabs = self
                .broker
                .registry
                .input_authority
                .lock()
                .map_err(|_| ())?;
            pointers
                .entry((namespace, seat))
                .or_insert_with(crate::XCorePointerMapper::new);
            grabs.prepare_ordered_namespace(namespace);
            Ok::<(), ()>(())
        });
        if !matches!(prepared, Ok(Ok(()))) {
            return Err((PrivateRunnerRefusal::StateUnavailable, self));
        }
        // Construct this before keyboard issuance and before the watchdog can
        // admit producers. Until preparation succeeds no turn or hold can
        // borrow it, so a later refusal may drop this unexposed construction.
        // Installation below is the only assignment of the continuing origin.
        let native_owner = match private_native::Owner::prepare(
            &self.controller,
            &self.broker.registry,
            namespace,
            seat,
        ) {
            Ok(owner) => owner,
            Err(_) => return Err((PrivateRunnerRefusal::StateUnavailable, self)),
        };
        let mut keyboards = match self.keyboards() {
            Ok(keyboards) => keyboards,
            Err(refusal) => return Err((PrivateRunnerRefusal::Keyboard(refusal), self)),
        };
        if !keyboards.prepare(seat) {
            // The history never escaped and never executed an input.
            self.keyboards_issued.store(false, Ordering::Release);
            return Err((
                PrivateRunnerRefusal::Keyboard(PrivateKeyboardsRefusal::KeymapUnavailable),
                self,
            ));
        }
        let gate = match self
            .pending_watch
            .as_mut()
            .expect("constructed supervisor")
            .seal()
        {
            Ok(gate) => gate,
            Err(_) => return Err((PrivateRunnerRefusal::StateUnavailable, self)),
        };
        if self.admission.watch.set(gate).is_err() {
            return Err((PrivateRunnerRefusal::StateUnavailable, self));
        }
        self.native_owner = Some(native_owner);
        let witness = Arc::new(PrivateExecutionWitness {
            instance: self.instance,
            state: std::sync::atomic::AtomicU8::new(0),
        });
        self.terminal.execution = Some(witness.clone());
        let watch = self.pending_watch.take();
        Ok(PrivatePreparedRunner {
            lifetime: PrivateExecutionLifetimeOwner(witness),
            watch,
            frontend: Some(self),
            keyboards,
            namespace,
            seat,
            service_origin: std::time::Instant::now(),
            service: sophia_input_authority::ServiceBudget::planned(std::time::Duration::ZERO),
            prefer_cleanup: true,
            reclaim_cursor: 0,
        })
    }
}

#[cfg(unix)]
impl PrivatePreparedRunner {
    fn deliver_accounted_step(
        &mut self,
    ) -> Result<PrivateAccountedDelivery, XServerFrontendRouteError> {
        use sophia_input_authority::{CleanupReadiness, ServiceWork};
        let Self {
            watch,
            frontend,
            service_origin,
            service,
            ..
        } = self;
        let admission = match service.prepare(
            service_origin.elapsed(),
            ServiceWork::Cleanup,
            CleanupReadiness::Eligible,
        ) {
            Ok(admission) => admission,
            Err(cause) => return Ok(PrivateAccountedDelivery::Yield { cause, taken: None }),
        };
        let mut admission = Some(admission);
        let mut running = None;
        let mut watched = None;
        let mut refused = None;
        let mut chosen = None;
        let mut watch_failed = false;
        let mut unwatched = None;
        let result = frontend
            .as_mut()
            .expect("live runner")
            .deliver_one(&mut |sequence, began| {
                // None is a proof-recording visit: real work with no ordered
                // entry to name. It is admitted and watched the same way, and
                // simply has nothing to report as taken or blocked.
                chosen = sequence;
                let admission = admission
                    .take()
                    .ok_or(XServerFrontendRouteError::OrderedItemUnresolved)?;
                let Some(elapsed) = began.checked_duration_since(*service_origin) else {
                    refused = Some(sophia_input_authority::ServiceStartRefusal::ClockRegressed);
                    return Err(XServerFrontendRouteError::OrderedItemUnresolved);
                };
                match admission.dequeued(elapsed, CleanupReadiness::Eligible) {
                    Ok(run) => running = Some(run),
                    Err(cause) => {
                        refused = Some(cause);
                        return Err(XServerFrontendRouteError::OrderedItemUnresolved);
                    }
                }
                // Terminal observation can acquire common; watch it before that
                // acquisition too. The item already belongs to the inventory.
                let guard = match watch
                    .as_ref()
                    .expect("prepared supervisor")
                    .begin_dequeued(began)
                {
                    Ok(guard) => guard,
                    Err(_) => {
                        watch_failed = true;
                        unwatched = sequence;
                        return Err(XServerFrontendRouteError::OrderedItemUnresolved);
                    }
                };
                watched = Some(guard);
                if watched.as_mut().expect("installed").applying().is_err() {
                    watch_failed = true;
                    unwatched = sequence;
                    return Err(XServerFrontendRouteError::OrderedItemUnresolved);
                }
                Ok(())
            });
        if let Some(watched) = watched
            && watched.finish().is_err()
        {
            watch_failed = true;
            unwatched = chosen;
        }
        let charge = running
            .map(|run| run.finish(service_origin.elapsed()))
            .transpose()
            .map_err(|_| XServerFrontendRouteError::OrderedItemUnresolved)?;
        if let Some(cause) = refused {
            return Ok(PrivateAccountedDelivery::Yield {
                cause,
                taken: chosen,
            });
        }
        let step = match result {
            Ok(step) => step,
            Err(_) if unwatched.is_some() => {
                PrivateDeliveryStep::Blocked(unwatched.expect("checked"))
            }
            // The supervisor failed on work that has no ordered entry to name.
            // No step was taken -- the failure happened during admission, so
            // nothing was observed, sent or recorded -- and the fact travels
            // beside this rather than as a sequence nobody could supply.
            Err(_) if watch_failed => PrivateDeliveryStep::Idle,
            Err(error) => return Err(error),
        };
        Ok(PrivateAccountedDelivery::Step {
            step,
            charge,
            watch_failed,
            unwatched,
        })
    }

    /// Check the allowance before dequeue, and charge only the item actually
    /// taken. Both accounting guards are local; the work they describe is
    /// already in the frontend before either can fail or unwind.
    fn execute_accounted_step(
        &mut self,
    ) -> Result<PrivateAccountedStep, XServerFrontendRouteError> {
        use sophia_input_authority::{CleanupReadiness, ServiceWork};
        let Self {
            watch,
            frontend,
            keyboards,
            service_origin,
            service,
            ..
        } = self;
        // Until every native/recipient cleanup source supplies an eligibility
        // observation, keep the cleanup reservation. An unavailable scan or
        // an unwired consumer is not evidence that the allowance can be donated.
        let cleanup = CleanupReadiness::Eligible;
        let admission =
            match service.prepare(service_origin.elapsed(), ServiceWork::NewWork, cleanup) {
                Ok(admission) => admission,
                Err(cause) => return Ok(PrivateAccountedStep::Yield { cause, taken: None }),
            };
        let mut admission = Some(admission);
        let mut running = None;
        let mut refused = None;
        let mut taken = None;
        let result = frontend.as_mut().expect("live runner").step_once(
            keyboards,
            &mut |sequence, taken_at| {
                taken = Some(sequence);
                let admission = admission
                    .take()
                    .ok_or(XServerFrontendRouteError::OrderedItemUnresolved)?;
                let Some(elapsed) = taken_at.checked_duration_since(*service_origin) else {
                    refused = Some(sophia_input_authority::ServiceStartRefusal::ClockRegressed);
                    return Err(XServerFrontendRouteError::OrderedItemUnresolved);
                };
                // Fresh readiness is deliberately conservative here too; an
                // earlier empty snapshot must not authorize a later donation.
                match admission.dequeued(elapsed, CleanupReadiness::Eligible) {
                    Ok(run) => {
                        running = Some(run);
                        Ok(())
                    }
                    Err(cause) => {
                        refused = Some(cause);
                        Err(XServerFrontendRouteError::OrderedItemUnresolved)
                    }
                }
            },
            watch.as_ref().expect("prepared supervisor"),
        );
        // Idle/Blocked never called the hook. Dropping that admission neither
        // consumes an interval nor marks an interrupted execution.
        // Every returned Result finishes accounting, including a refused
        // execution. An unwind instead drops the ServiceRun and permanently
        // closes this budget while the accepted item remains instance-owned.
        let charge = running
            .map(|run| run.finish(service_origin.elapsed()))
            .transpose()
            .map_err(|_| XServerFrontendRouteError::OrderedItemUnresolved)?;
        if let Some(cause) = refused {
            if frontend.as_ref().expect("live runner").terminal.current_is_frozen {
                taken = None;
            }
            return Ok(PrivateAccountedStep::Yield { cause, taken });
        }
        Ok(PrivateAccountedStep::Step {
            step: result?,
            charge,
        })
    }

    /// Whether this turn moved anything: took, decided, routed, delivered
    /// or settled. An allowance refusal or a blocked order is not progress.
    pub(crate) fn advanced(progress: &PrivateRunnerProgress) -> bool {
        progress.starts != 0
            || progress.terminal_steps != 0
            || progress.taken != 0
            || progress.routed != 0
            || progress.dispatched != 0
            || progress.recipient_settled != 0
            || progress.reclaimed != 0
    }

    /// The frontend this runner executes over, for the service loop that
    /// admits connections through its broker and attaches their workers.
    /// Borrowed from the runner, never taken: the runner stays the one owner
    /// of the prepared path on this thread.
    pub(crate) fn frontend(&self) -> &PrivateXServerFrontend {
        self.frontend.as_ref().expect("live runner")
    }

    /// Close producer admission and interrupt transports without destroying
    /// the original cleanup supervisor. Accepted work and its accounting stay
    /// owned; later turns can service terminal work but cannot dequeue new
    /// operations. Final runner disposal still stops the supervisor.
    pub(crate) fn close_admission(&mut self) {
        if let Some(watch) = self.watch.as_ref() {
            watch.close_production();
        }
        self.prefer_cleanup = true;
    }

    pub fn seat(&self) -> SeatId {
        self.seat
    }
    pub fn namespace(&self) -> NamespaceId {
        self.namespace
    }

    pub fn admission_participant(&self) -> &PrivateAdmissionParticipant {
        &self.frontend.as_ref().expect("live runner").participant
    }

    /// ISSUED ONLY ON A LIVE LEASE, and used only on one: the producer this
    /// hands out asks again at every acceptance.
    pub fn control_producer(
        &self,
        service: &PrivateServiceLease<'_>,
    ) -> Result<PrivateControlProducer, PrivateServiceRefusal> {
        let frontend = self.frontend.as_ref().expect("live runner");
        if !frontend.broker.registry.leased_by(service) {
            return Err(PrivateServiceRefusal::ForeignServiceOwner);
        }
        Ok(frontend.control_producer())
    }

    /// THE OWNER IS BORROWED FOR THIS ACT, and for every one like it. A
    /// producer handed out here reserves work for a connection whose evidence
    /// this service does not keep, so a runner that could issue one after its
    /// keeper had gone would be accepting work it could never leave an
    /// account of.
    ///
    /// AND IT MUST BE THIS SERVICE'S OWNER. A lease on some other owner proves
    /// that some other keeper is alive, which is not the same fact.
    pub fn ingress_for(
        &mut self,
        service: &PrivateServiceLease<'_>,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
    ) -> Result<PrivateIngress, PrivateServiceRefusal> {
        let frontend = self.frontend.as_mut().expect("live runner");
        if !frontend.broker.registry.leased_by(service) {
            return Err(PrivateServiceRefusal::ForeignServiceOwner);
        }
        frontend
            .ingress_for(client, device)
            .map_err(PrivateServiceRefusal::Admission)
    }

    /// Consume the actual shared order using this runner's continuing state.
    /// Writer settlement is supplied by the terminal owner, not inferred from
    /// a turn returning or from a successful queue handoff.
    /// THE OWNER IS BORROWED FOR THE WHOLE TURN, for the same reason: a turn
    /// takes accepted work and advances connections whose evidence lives
    /// outside this service, and one that could run with no keeper would be
    /// serving connections nothing can afterwards answer for.
    pub(crate) fn service_turn(
        &mut self,
        service: &PrivateServiceLease<'_>,
    ) -> Result<PrivateRunnerProgress, XServerFrontendRouteError> {
        if !self
            .frontend
            .as_ref()
            .expect("live runner")
            .broker
            .registry
            .leased_by(service)
        {
            return Err(XServerFrontendRouteError::ForeignServiceOwner);
        }
        // Reap only a supervisor already known to have returned. This never
        // waits for a running supervisor or a client worker, and its result
        // cannot reopen the admission gate or settle accepted work.
        let _ = self
            .watch
            .as_mut()
            .expect("prepared supervisor")
            .reap_finished();
        let mut progress = PrivateRunnerProgress::default();
        // Even if an owner-loop turn crosses several interval boundaries,
        // producers cannot keep this call open by continuously replenishing.
        let turn_starts = self.service.limits().starts as usize;
        while progress.starts < turn_starts {
            let production_open = self.frontend().admission.lifecycle_open();
            if !production_open {
                self.prefer_cleanup = true;
            }
            let mut cleanup_idle = false;
            if self.prefer_cleanup {
                match self.deliver_accounted_step()? {
                    PrivateAccountedDelivery::Yield { cause, taken } => {
                        progress.allowance = Some(cause);
                        progress.blocked = taken;
                        break;
                    }
                    PrivateAccountedDelivery::Step {
                        step,
                        charge,
                        watch_failed,
                        unwatched,
                    } => {
                        let overran = progress.record_charge(charge);
                        progress.unwatched = unwatched;
                        // Recorded whether or not an entry could be named, so
                        // a supervisor failure on native work is not lost to
                        // an absent sequence.
                        progress.watch_failed |= watch_failed;
                        // ACCOUNTED FIRST, STOPPED AFTER. A supervisor that
                        // failed on the way out does not unmake the step that
                        // already happened: the visit was spent, the bit may
                        // have gone in, the entry may have been disposed of.
                        // Breaking before this match reported a turn in which
                        // none of that occurred, which is a worse account of
                        // the instance than the failure it was reacting to.
                        //
                        // An ADMISSION failure is different and stays
                        // different: it arrives as Idle, because nothing was
                        // taken, and nothing below counts a step for it.
                        match step {
                            PrivateDeliveryStep::Idle => {
                                if watch_failed {
                                    break;
                                }
                                cleanup_idle = true;
                            }
                            // A visit was spent, so this side of the service
                            // made progress and is not idle. Counted as the
                            // terminal step it is: the work was chosen,
                            // charged and done, whether or not the recording
                            // it attempted went in.
                            // Only confirmed facts are counted. An
                            // unanswered ledger changed nothing and is not a
                            // settlement, a return, or a delivery.
                            PrivateDeliveryStep::Receipt { step } => {
                                progress.terminal_steps += 1;
                                match step {
                                    PrivateReceiptStep::Settled { debt_settled } => {
                                        progress.recipient_settled += 1;
                                        // The whole debt closing is the
                                        // ledger's fact and is reported as
                                        // the settlement it is.
                                        progress.settled += usize::from(debt_settled);
                                    }
                                    PrivateReceiptStep::ReturnedUnsettled => {
                                        progress.relinquished += 1;
                                    }
                                    PrivateReceiptStep::Unanswered => {}
                                }
                                if watch_failed {
                                    break;
                                }
                                self.prefer_cleanup = false;
                                if overran || unwatched.is_some() {
                                    break;
                                }
                                continue;
                            }
                            PrivateDeliveryStep::Dispatched {
                                enqueued,
                                relinquished,
                            } => {
                                progress.terminal_steps += 1;
                                progress.dispatched += usize::from(enqueued);
                                // One actual queue acceptance, counted once
                                // and where it happened.
                                progress.enqueued += usize::from(enqueued);
                                progress.relinquished += usize::from(relinquished);
                                if watch_failed {
                                    break;
                                }
                                self.prefer_cleanup = false;
                                if overran || unwatched.is_some() {
                                    break;
                                }
                                continue;
                            }
                            PrivateDeliveryStep::SharedActivation { observed, joined } => {
                                progress.terminal_steps += 1;
                                progress.activation_pairs_observed += observed;
                                progress.activations_joined += joined;
                                if watch_failed {
                                    break;
                                }
                                self.prefer_cleanup = false;
                                if overran || unwatched.is_some() {
                                    break;
                                }
                                continue;
                            }
                            PrivateDeliveryStep::TransientReceipt { disposed } => {
                                progress.terminal_steps += 1;
                                progress.transient_observed += 1;
                                progress.transient_disposed += usize::from(disposed);
                                if watch_failed { break; }
                                self.prefer_cleanup = false;
                                if overran || unwatched.is_some() { break; }
                                continue;
                            }
                            PrivateDeliveryStep::NativeDisposal { disposed } => {
                                progress.terminal_steps += 1;
                                progress.native_disposal_observed += 1;
                                progress.native_disposed += usize::from(disposed);
                                if watch_failed { break; }
                                self.prefer_cleanup = false;
                                if overran || unwatched.is_some() { break; }
                                continue;
                            }
                            PrivateDeliveryStep::Recorded { recorded } => {
                                progress.terminal_steps += 1;
                                progress.recorded += usize::from(recorded);
                                if watch_failed {
                                    // Accounted and stopped. The side this
                                    // turn would have preferred next is left
                                    // as it was: a turn that ended in a
                                    // supervisor failure decides nothing
                                    // about what the next one should do.
                                    break;
                                }
                                self.prefer_cleanup = false;
                                if overran || unwatched.is_some() {
                                    break;
                                }
                                continue;
                            }
                            PrivateDeliveryStep::Blocked(sequence) => {
                                progress.blocked = Some(sequence);
                                break;
                            }
                            PrivateDeliveryStep::Advanced {
                                sequence: _,
                                report,
                            } => {
                                progress.terminal_steps += 1;
                                if let Some(delivered) = report {
                                    // Observation only. An entry advancing says
                                    // what the request recorded, not whether
                                    // anything reached a queue: that is the
                                    // dispatch's fact and is counted there,
                                    // once per actual acceptance.
                                    progress.observed +=
                                        usize::from(delivered.completion.is_some());
                                    progress.settled += usize::from(delivered.debt_settled);
                                }
                                self.prefer_cleanup = false;
                                if overran || watch_failed || unwatched.is_some() {
                                    break;
                                }
                                continue;
                            }
                        }
                    }
                }
            }
            if !production_open {
                break;
            }
            let (step, charge) = match self.execute_accounted_step()? {
                PrivateAccountedStep::Yield { cause, taken } => {
                    // A new-work reservation may yield while cleanup still
                    // has allowance. Try that side once before returning.
                    if taken.is_none()
                        && !cleanup_idle
                        && matches!(cause,
                        sophia_input_authority::ServiceStartRefusal::CleanupStartsReserved { .. }
                        | sophia_input_authority::ServiceStartRefusal::CleanupTimeReserved { .. })
                    {
                        self.prefer_cleanup = true;
                        continue;
                    }
                    progress.allowance = Some(cause);
                    progress.taken += usize::from(taken.is_some());
                    progress.blocked = taken;
                    break;
                }
                PrivateAccountedStep::Step { step, charge } => (step, charge),
            };
            progress.taken += usize::from(charge.is_some() && !matches!(step, PrivateOrderedStep::Resumed { .. }));
            let overran = progress.record_charge(charge);
            self.prefer_cleanup = true;
            match step {
                PrivateOrderedStep::Deferred { sequence, watched }
                | PrivateOrderedStep::Resumed { sequence, watched, deferred: true } => {
                    if !watched {
                        progress.unwatched = Some(sequence);
                        break;
                    }
                }
                PrivateOrderedStep::Resumed { sequence, watched, deferred: false } => {
                    let frontend = self.frontend.as_ref().expect("live runner");
                    if let Some(PrivateOrderedItem::Refused { sequence: stored, refusal, .. }) = frontend.terminal.turn.last()
                        && *stored == sequence
                    {
                        progress.refused += 1;
                        progress.last_refusal = Some(*refusal);
                    }
                    if !watched {
                        progress.unwatched = Some(sequence);
                        break;
                    }
                }
                PrivateOrderedStep::Idle => {
                    if cleanup_idle {
                        break;
                    }
                    continue;
                }
                PrivateOrderedStep::Blocked(sequence) | PrivateOrderedStep::Parked(sequence) => {
                    progress.blocked = Some(sequence);
                    break;
                }
                PrivateOrderedStep::Unwatched(sequence) => {
                    progress.unwatched = Some(sequence);
                    progress.blocked = Some(sequence);
                    break;
                }
                PrivateOrderedStep::Routed(sequence) => {
                    progress.routed += 1;
                    progress.last_routed = Some(sequence);
                }
                PrivateOrderedStep::RoutedUnwatched(sequence) => {
                    progress.routed += 1;
                    progress.last_routed = Some(sequence);
                    progress.unwatched = Some(sequence);
                    break;
                }
                PrivateOrderedStep::Decided(sequence)
                | PrivateOrderedStep::DecidedUnwatched(sequence) => {
                    let frontend = self.frontend.as_ref().expect("live runner");
                    if let Some(PrivateOrderedItem::Refused {
                        sequence: stored,
                        refusal,
                        ..
                    }) = frontend.terminal.turn.last()
                        && *stored == sequence
                    {
                        progress.refused += 1;
                        progress.last_refusal = Some(*refusal);
                    }
                    if matches!(step, PrivateOrderedStep::DecidedUnwatched(_)) {
                        progress.unwatched = Some(sequence);
                        break;
                    }
                }
            }
            if overran {
                break;
            }
        }
        // THE LIVE RECLAMATION, ON AN OTHERWISE IDLE TURN, UNDER THE CLEANUP
        // ALLOWANCE AND THE WATCHDOG: a bounded, cursor-resumed visit of the
        // outstanding list, spending one charged cleanup start when there is
        // anything to visit and none when there is not. Deliveries keep
        // their cleanup starts: a turn that moved work does not also spend
        // one here. What the visit reclaims is what its owners already
        // recorded and observed; a pending outcome is an observation, not a
        // completion. This is what lets a continuing service reuse control
        // capacity; it borrows no second budget.
        if !Self::advanced(&progress) {
            match self.reclaim_accounted_step()? {
                PrivateAccountedReclaim::Idle => {}
                PrivateAccountedReclaim::Yield { cause } => {
                    progress.reclaim_refusal = Some(cause);
                }
                PrivateAccountedReclaim::Visited {
                    observed,
                    reclaimed,
                    charge,
                    unwatched,
                } => {
                    progress.reclaim_observed = observed;
                    progress.reclaimed = reclaimed;
                    progress.reclaim_unwatched = unwatched;
                    progress.record_charge(charge);
                }
            }
        }
        let frontend = self.frontend.as_ref().expect("live runner");
        progress.blocked = progress.blocked.or_else(|| frontend.blocked());
        Ok(progress)
    }

    /// One accounted reclamation visit: idle with nothing outstanding;
    /// otherwise a cleanup start is asked of the budget and, if admitted,
    /// the supervisor is begun BEFORE the recovery and completion guards
    /// are read, the bounded visit runs, and the supervisor's finish is
    /// asked only after the visit's releases are done -- a late failure
    /// there is recorded, and undoes none of them. An interrupted visit
    /// (an unwind inside it) drops the run, which closes the budget, and
    /// leaves the list exactly as far as it got.
    fn reclaim_accounted_step(
        &mut self,
    ) -> Result<PrivateAccountedReclaim, XServerFrontendRouteError> {
        use sophia_input_authority::{CleanupReadiness, ServiceWork};
        let Self {
            watch,
            frontend,
            service_origin,
            service,
            reclaim_cursor,
            ..
        } = self;
        let frontend = frontend.as_mut().expect("live runner");
        if frontend.outstanding.is_empty() {
            return Ok(PrivateAccountedReclaim::Idle);
        }
        let admission = match service.prepare(
            service_origin.elapsed(),
            ServiceWork::Cleanup,
            CleanupReadiness::Eligible,
        ) {
            Ok(admission) => admission,
            Err(cause) => return Ok(PrivateAccountedReclaim::Yield { cause }),
        };
        let began = std::time::Instant::now();
        let Some(elapsed) = began.checked_duration_since(*service_origin) else {
            return Ok(PrivateAccountedReclaim::Yield {
                cause: sophia_input_authority::ServiceStartRefusal::ClockRegressed,
            });
        };
        let run = match admission.dequeued(elapsed, CleanupReadiness::Eligible) {
            Ok(run) => run,
            Err(cause) => return Ok(PrivateAccountedReclaim::Yield { cause }),
        };
        let watched = watch
            .as_ref()
            .expect("prepared supervisor")
            .begin_dequeued(began)
            .ok();
        let (observed, reclaimed, unwatched) = match watched {
            Some(mut watched) => {
                if watched.applying().is_err() {
                    (0, 0, true)
                } else {
                    // STAGE-ONLY interruption point, test builds only: inside
                    // the watched, charged visit, before its first guard.
                    #[cfg(all(test, unix))]
                    routing_tests::stage_reclaim_visit();
                    let (observed, reclaimed) =
                        frontend.reclaim_settled_visit(reclaim_cursor, PRIVATE_RECLAIM_VISIT_BOUND);
                    let unwatched = watched.finish().is_err();
                    (observed, reclaimed, unwatched)
                }
            }
            None => (0, 0, true),
        };
        let charge = run
            .finish(service_origin.elapsed())
            .map_err(|_| XServerFrontendRouteError::OrderedItemUnresolved)?;
        Ok(PrivateAccountedReclaim::Visited {
            observed,
            reclaimed,
            charge: Some(charge),
            unwatched,
        })
    }

    pub fn shutdown(mut self) -> PrivateSettlement {
        drop(self.watch.take());
        self.frontend.take().expect("live runner").shutdown()
    }
}
