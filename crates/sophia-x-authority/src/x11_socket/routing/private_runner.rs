/// Why a frontend could not become an execution runner. The frontend is
/// returned intact; no producer is exposed by a failed preparation.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateRunnerRefusal {
    ProducerAlreadyExposed,
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
    // Close the gate and transports before frontend Drop can enter common.
    watch: Option<private_watchdog::PrivateWatchdogOwner>,
    frontend: Option<PrivateXServerFrontend>,
    keyboards: PrivateKeyboards,
    namespace: NamespaceId,
    seat: SeatId,
    service_origin: std::time::Instant,
    service: sophia_input_authority::ServiceBudget,
    prefer_cleanup: bool,
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
        unwatched: Option<crate::ReadySequence>,
    },
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Prepare on the thread that will execute turns, before granting an
    /// ingress. The instance's issuer supplies the seat; callers cannot choose
    /// a different seat for the same authority.
    #[allow(clippy::result_large_err)] // Refusal returns the caller's owned frontend without boxing.
    pub fn prepare_runner(
        mut self,
        namespace: NamespaceId,
    ) -> Result<PrivatePreparedRunner, (PrivateRunnerRefusal, Self)> {
        if self.ordered_runner {
            return Err((PrivateRunnerRefusal::ProducerAlreadyExposed, self));
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
        let watch = self.pending_watch.take();
        Ok(PrivatePreparedRunner {
            watch,
            frontend: Some(self),
            keyboards,
            namespace,
            seat,
            service_origin: std::time::Instant::now(),
            service: sophia_input_authority::ServiceBudget::planned(std::time::Duration::ZERO),
            prefer_cleanup: true,
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
        let mut unwatched = None;
        let result = frontend
            .as_mut()
            .expect("live runner")
            .deliver_one(&mut |sequence, began| {
                chosen = Some(sequence);
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
                        unwatched = Some(sequence);
                        return Err(XServerFrontendRouteError::OrderedItemUnresolved);
                    }
                };
                watched = Some(guard);
                if watched.as_mut().expect("installed").applying().is_err() {
                    unwatched = Some(sequence);
                    return Err(XServerFrontendRouteError::OrderedItemUnresolved);
                }
                Ok(())
            });
        if let Some(watched) = watched
            && watched.finish().is_err()
        {
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
            Err(error) => return Err(error),
        };
        Ok(PrivateAccountedDelivery::Step {
            step,
            charge,
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
            return Ok(PrivateAccountedStep::Yield { cause, taken });
        }
        Ok(PrivateAccountedStep::Step {
            step: result?,
            charge,
        })
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

    pub fn control_producer(&self) -> PrivateControlProducer {
        self.frontend
            .as_ref()
            .expect("live runner")
            .control_producer()
    }

    pub fn ingress_for(
        &mut self,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
    ) -> Result<PrivateIngress, PrivateAdmissionRefusal> {
        self.frontend
            .as_mut()
            .expect("live runner")
            .ingress_for(client, device)
    }

    /// Consume the actual shared order using this runner's continuing state.
    /// Writer settlement is supplied by the terminal owner, not inferred from
    /// a turn returning or from a successful queue handoff.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn service_turn(
        &mut self,
    ) -> Result<PrivateRunnerProgress, XServerFrontendRouteError> {
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
                        unwatched,
                    } => {
                        let overran = progress.record_charge(charge);
                        progress.unwatched = unwatched;
                        match step {
                            PrivateDeliveryStep::Idle => cleanup_idle = true,
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
                                    progress.enqueued += usize::from(delivered.enqueued);
                                    progress.observed +=
                                        usize::from(delivered.completion.is_some());
                                    progress.settled += usize::from(delivered.debt_settled);
                                }
                                self.prefer_cleanup = false;
                                if overran || unwatched.is_some() {
                                    break;
                                }
                                continue;
                            }
                        }
                    }
                }
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
            progress.taken += usize::from(charge.is_some());
            let overran = progress.record_charge(charge);
            self.prefer_cleanup = true;
            match step {
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
                PrivateOrderedStep::Decided(sequence)
                | PrivateOrderedStep::DecidedUnwatched(sequence) => {
                    let frontend = self.frontend.as_ref().expect("live runner");
                    progress.refused += usize::from(matches!(frontend.terminal.turn.last(),
                        Some(PrivateOrderedItem::Refused { sequence: stored, .. }) if *stored==sequence));
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
        let frontend = self.frontend.as_ref().expect("live runner");
        progress.blocked = progress.blocked.or_else(|| frontend.blocked());
        Ok(progress)
    }

    pub fn shutdown(mut self) -> PrivateSettlement {
        drop(self.watch.take());
        self.frontend.take().expect("live runner").shutdown()
    }
}
