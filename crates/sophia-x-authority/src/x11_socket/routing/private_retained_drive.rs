// Permission to advance one retained connection, separate from its inert
// identity, committed responsibility, and namespace clearance.

#[cfg(unix)]
#[derive(Default)]
struct PrivateRetainedDriveCursor {
    next: usize,
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateRetainedFrame {
    Incomplete { sent: usize, len: usize },
    Indeterminate { from: usize, len: usize },
    CompleteUnretired { len: usize },
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateRetainedDriveRefusal {
    ExecutionNotRetained,
    ForeignServiceOwner,
    InventoryUnreadable,
    Prerequisite(PrivateDeferredCleanupRefusal),
    CleanupUnreadable,
    CleanupInterrupted,
    FenceUnestablished,
    StoreUnreadable,
    Uncommitted,
    ForeignCommitment,
    Stale,
    HomeUnreadable,
    HomeLive,
    HomeEmpty,
    HomeEvidenceMismatch,
    /// No new send or close is attempted over this stored frame. An already
    /// established close may adjudicate its own in-flight capsule at source.
    FramePreserved(PrivateRetainedFrame),
    SupervisorMissing,
    Supervisor(private_watchdog::PrivateWatchdogRefusal),
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateRetainedVisit {
    EmptyInventory,
    EmptyPlace,
    OtherRegistry,
    Driven { settled: bool },
}

#[cfg(unix)]
#[derive(Debug)]
enum PrivateRetainedDriveStep {
    Refused(PrivateRetainedDriveRefusal),
    Yield(sophia_input_authority::ServiceStartRefusal),
    Charged {
        outcome: Result<PrivateRetainedVisit, PrivateRetainedDriveRefusal>,
        charge: Result<
            sophia_input_authority::ServiceCharge,
            sophia_input_authority::ServiceAccountingError,
        >,
        /// A late failure leaves effects and the cursor where they reached.
        supervision: Result<(), private_watchdog::PrivateWatchdogRefusal>,
    },
}

/// A one-use permission pinned through the exact externally leased custody.
/// Construction verifies the committed occupant and evidence under the store
/// guard. The aggregate is released before the home is ever acquired.
#[cfg(unix)]
struct PrivateRetainedDriveAuthority<'o> {
    custody: PrivateCustodyPin<'o>,
    home: Arc<PrivateOrderedHome>,
    closed: PrivateHandoverFence,
}

#[cfg(unix)]
impl<'o> PrivateRetainedDriveAuthority<'o> {
    fn authorize(
        custody: PrivateCustodyPin<'o>,
        registry: &XServerFrontendRouteRegistry,
        collected: Option<&PrivateConnectionsCollected>,
    ) -> Result<Self, PrivateRetainedDriveRefusal> {
        use PrivateRetainedDriveRefusal as Refusal;
        if !custody.cleanup_record().published_by(registry) {
            return Err(Refusal::ForeignCommitment);
        }
        custody
            .deferred_cleanup_prerequisites(collected)
            .map_err(Refusal::Prerequisite)?;
        {
            let cleanup = custody
                .source
                .deferred_cleanup
                .lock()
                .map_err(|_| Refusal::CleanupUnreadable)?;
            if matches!(*cleanup, PrivateDeferredCleanupStanding::Claimed { .. }) {
                return Err(Refusal::CleanupInterrupted);
            }
        }
        let closed = custody
            .fence_evidence()
            .fence()
            .filter(|fence| *fence != PrivateHandoverFence::Unreadable)
            .ok_or(Refusal::FenceUnestablished)?;
        let home = {
            let store = custody
                .store()
                .inner
                .lock()
                .map_err(|_| Refusal::StoreUnreadable)?;
            let identity = custody.identity();
            let Some(PrivateOrderedContinuationPlace::Taken(home)) =
                store.continuations.get(identity.index)
            else {
                return Err(Refusal::Stale);
            };
            if !std::ptr::eq(Arc::as_ptr(home), identity.home.as_ptr()) {
                return Err(Refusal::Stale);
            }
            let Some(PrivateHolderPlace::Taken(holder)) = store.holders.get(identity.index) else {
                return Err(Refusal::Uncommitted);
            };
            let obligation = holder.obligation.as_ref().ok_or(Refusal::Uncommitted)?;
            if !obligation.identity.same_as(identity)
                || !holder.credit.armed
                || !holder.credit.maintenance_identity().same_as(identity)
                || obligation.closed != closed
                || !std::ptr::eq(obligation.join.as_ptr(), Arc::as_ptr(custody.join()))
            {
                return Err(Refusal::ForeignCommitment);
            }
            Arc::clone(home)
        };
        Ok(Self {
            custody,
            home,
            closed,
        })
    }

    fn visit(self) -> Result<PrivateRetainedVisit, PrivateRetainedDriveRefusal> {
        use PrivateRetainedDriveRefusal as Refusal;
        let settled = {
            // Standing, evidence and the effect share this acquisition. A
            // poisoned payload is retained untouched, never recovered to run.
            let mut home = self
                .home
                .state
                .lock()
                .map_err(|_| Refusal::HomeUnreadable)?;
            if home.standing != PrivateHomeStanding::Retained {
                return Err(Refusal::HomeLive);
            }
            let continuation = home.payload.as_mut().ok_or(Refusal::HomeEmpty)?;
            let evidence = match continuation {
                PrivateOrderedContinuation::Setup { evidence, .. }
                | PrivateOrderedContinuation::Serving { evidence, .. } => evidence,
            };
            if evidence.source_poisoned
                || evidence.fence != Some(self.closed)
                || !matches!(&evidence.worker, PrivateOrderedWorkerExit::Joined(join)
                    if std::ptr::eq(join.as_ptr(), Arc::as_ptr(self.custody.join())))
            {
                return Err(Refusal::HomeEvidenceMismatch);
            }
            Self::preserve_unresolved_frame(continuation)?;
            continuation.visit();
            continuation.settled()
        };
        if settled {
            // The source's settled predicate is the only disposal condition.
            // Returning rechecks the exact occupant, and never reads a client
            // number, namespace-clearance result, or a substitute receipt.
            self.custody
                .store()
                .return_ordered_continuation(self.custody.identity().index, &self.home);
        }
        Ok(PrivateRetainedVisit::Driven { settled })
    }

    fn preserve_unresolved_frame(
        continuation: &PrivateOrderedContinuation,
    ) -> Result<(), PrivateRetainedDriveRefusal> {
        let PrivateOrderedContinuation::Serving { owner, .. } = continuation else {
            return Ok(());
        };
        // A fully sent but unretired frame is reported separately. Supporting
        // its retirement without an established close remains future work;
        // it is not silently classified as an indeterminate write.
        if !owner
            .closing()
            .is_some_and(|close| close.termination == X11OrderedTermination::Established)
            && let Some(frame) = owner.in_flight().and_then(Self::frame_state)
        {
            return Err(PrivateRetainedDriveRefusal::FramePreserved(frame));
        }
        // The existing close cannot adjudicate these retained slots. Keep
        // their cursor and origin, and do not drain more behind them.
        if let Some(frame) = owner
            .retained_unanswered()
            .iter()
            .find_map(Self::frame_state)
        {
            return Err(PrivateRetainedDriveRefusal::FramePreserved(frame));
        }
        Ok(())
    }

    fn frame_state(held: &X11OrderedInFlight) -> Option<PrivateRetainedFrame> {
        let frame = held.send.frame.as_ref()?;
        let len = frame.bytes.as_ref().len();
        Some(match frame.progress {
            X11OrderedSendProgress::Unknown { from } => {
                PrivateRetainedFrame::Indeterminate { from, len }
            }
            X11OrderedSendProgress::Sent(sent) if sent < len => {
                PrivateRetainedFrame::Incomplete { sent, len }
            }
            X11OrderedSendProgress::Sent(_) => PrivateRetainedFrame::CompleteUnretired { len },
        })
    }
}

#[cfg(unix)]
impl PrivateServiceLease<'_> {
    /// Exactly one physical inventory place, including empty/foreign places.
    /// No vector snapshot, search past a refusal, or unbounded store scan.
    fn drive_retained_place(
        &self,
        registry: &XServerFrontendRouteRegistry,
        collected: Option<&PrivateConnectionsCollected>,
        cursor: &mut PrivateRetainedDriveCursor,
    ) -> Result<PrivateRetainedVisit, PrivateRetainedDriveRefusal> {
        let custody = {
            let kept = self
                .owner
                .inventory
                .kept
                .lock()
                .map_err(|_| PrivateRetainedDriveRefusal::InventoryUnreadable)?;
            if kept.places.is_empty() {
                return Ok(PrivateRetainedVisit::EmptyInventory);
            }
            let index = cursor.next % kept.places.len();
            cursor.next = (index + 1) % kept.places.len();
            kept.places[index]
                .as_ref()
                .map(|custody| PrivateCustodyPin {
                    custody: Arc::clone(custody),
                    owner: std::marker::PhantomData,
                })
        };
        let Some(custody) = custody else {
            return Ok(PrivateRetainedVisit::EmptyPlace);
        };
        if !custody.cleanup_record().published_by(registry) {
            return Ok(PrivateRetainedVisit::OtherRegistry);
        }
        PrivateRetainedDriveAuthority::authorize(custody, registry, collected)?.visit()
    }
}

#[cfg(unix)]
impl PrivateRetainedExecutionResources {
    /// One serial post-collection visit. This consumes the original cleanup
    /// allowance and uses the original supervisor before taking any guard.
    /// The original token stays in this occupied invocation's keeper; callers
    /// cannot supply a replacement registry, collection claim or budget.
    fn drive_retained_output_step(
        &mut self,
        service_owner: &PrivateServiceLease<'_>,
        cursor: &mut PrivateRetainedDriveCursor,
    ) -> PrivateRetainedDriveStep {
        use PrivateRetainedDriveRefusal as Refusal;
        use sophia_input_authority::{CleanupReadiness, ServiceWork};
        if self.lifetime.0.reading().availability != PrivateExecutionAvailability::Retained {
            return PrivateRetainedDriveStep::Refused(Refusal::ExecutionNotRetained);
        }
        if !self.origin.leased_by(service_owner) {
            return PrivateRetainedDriveStep::Refused(Refusal::ForeignServiceOwner);
        }
        let Some(watch) = self.watch.as_ref() else {
            return PrivateRetainedDriveStep::Refused(Refusal::SupervisorMissing);
        };
        let admission = match self.service.prepare(
            self.service_origin.elapsed(),
            ServiceWork::Cleanup,
            CleanupReadiness::Eligible,
        ) {
            Ok(admission) => admission,
            Err(cause) => return PrivateRetainedDriveStep::Yield(cause),
        };
        let began = std::time::Instant::now();
        let Some(elapsed) = began.checked_duration_since(self.service_origin) else {
            return PrivateRetainedDriveStep::Yield(
                sophia_input_authority::ServiceStartRefusal::ClockRegressed,
            );
        };
        let run = match admission.dequeued(elapsed, CleanupReadiness::Eligible) {
            Ok(run) => run,
            Err(cause) => return PrivateRetainedDriveStep::Yield(cause),
        };
        let (outcome, supervision) = match watch.begin_dequeued(began) {
            Err(cause) => (Err(Refusal::Supervisor(cause)), Err(cause)),
            Ok(mut watched) => match watched.applying() {
                Err(cause) => (Err(Refusal::Supervisor(cause)), Err(cause)),
                Ok(()) => {
                    let outcome = service_owner.drive_retained_place(
                        &self.origin,
                        self.collected.as_ref(),
                        cursor,
                    );
                    let supervision = watched.finish();
                    (outcome, supervision)
                }
            },
        };
        let charge = run.finish(self.service_origin.elapsed());
        PrivateRetainedDriveStep::Charged {
            outcome,
            charge,
            supervision,
        }
    }
}
