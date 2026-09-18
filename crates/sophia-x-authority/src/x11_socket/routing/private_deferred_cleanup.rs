// The deferred connection cleanup: after the private service has collected
// its connection frames and joined a connection's registered worker, the
// cleanup that connection's destruction deferred is discharged through the
// custody the owner keeps -- and only where every prerequisite is
// established.
//
// THREE FACTS, THREE OWNERS. The destruction decision on the cleanup record
// says the registration ran and deferred; it is NOT the end of the connection
// frame, which goes on past the registration (the disconnect observer, the
// revocation, the completion the frontend reaps). That the frames have ended
// is the collection owner's fact, and it is carried here as a token only the
// collection mints, bound to the registry it collected. The custody's join
// evidence says the worker was collected. Nothing here takes a replacement
// number, gate, join record, home, lease or cleanup record.
//
// THREE ACTS WITH THREE MEANINGS. The fence is the custody's single attempt
// through `PrivateFenceRecord`; what it recorded, Unreadable included, is
// kept as it is and never retried into something else. The commitment is
// `PrivateCommitmentContext` over the connection's own lease and its
// pre-reserved holder destination, and those two are owned by a restorer for
// every step of the interval between leaving the record and being committed.
// The namespace cleanup is the same number-keyed body the never-started path
// runs, inside the same number interval, only where closure is established.
//
// PROGRESS IS NEVER RESET. A visit that refuses after a phase completed keeps
// that phase and its exact result; a later visit continues from it. A visit
// interrupted after its claim stays Interrupted, visibly, with what it had.

/// The collection owner's word that this registry's connection frames have
/// all been collected.
///
/// MINTED ONLY BY THE COLLECTION, after its wait, and only when no frame is
/// still active; bound to the registry by the identity of its client table.
/// A destruction decision is not this fact, and neither is a caller's word.
#[cfg(unix)]
pub struct PrivateConnectionsCollected {
    registry: Arc<Mutex<BTreeMap<XServerFrontendClientId, XServerFrontendClientRouteSenders>>>,
}

#[cfg(unix)]
impl PrivateConnectionsCollected {
    /// Whether this token was minted for the registry that published this
    /// record.
    fn governs(&self, record: &PrivateCleanupRecord) -> bool {
        Arc::ptr_eq(&self.registry, &record.clients)
    }
}

/// Points inside the execution where a control may interrupt it.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateDeferredCleanupPoint {
    AfterClaim,
    AfterLeaseTaken,
    AfterDestinationPrepared,
}

/// Why a visit did not discharge a connection's deferred cleanup.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateDeferredCleanupRefusal {
    /// The service's connection frames were not established as collected,
    /// or the token is another registry's.
    ConnectionsUncollected,
    /// No destruction was ever requested by a registration.
    DestructionNotRequested,
    /// A destruction was requested and has not decided. Not reinterpreted.
    DestructionUndecided,
    /// The destruction decided to run synchronously; nothing was deferred.
    NotDeferred,
    /// The destruction deferred without establishing that a worker was
    /// started and where its handle is. Not reinterpreted.
    DestructionUnestablished(PrivateDestructionDeferral),
    /// The custody's join evidence holds no published result.
    JoinUnpublished,
    /// The custody's single fence attempt left no recorded fence.
    FenceUnrecorded(PrivateFencePhase),
    /// The connection holds no place lease to commit.
    LeaseMissing,
    /// The store refused a holder destination; the lease is back on the
    /// record.
    HolderRefused(PrivateHolderRefusal),
    /// The commitment was refused; the lease is back on the record.
    CommitRefused(PrivateCommitted),
    /// The fence recorded Unreadable: responsibility is committed, closure
    /// is not established, and the namespace cleanup requires it.
    ClosureUnestablished,
    /// An earlier visit claimed the execution and did not finish.
    Interrupted,
}

/// What the phases of one connection's deferred cleanup have established so
/// far. Never reset; a later visit continues from it.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrivateDeferredCleanupProgress {
    /// What the custody's fence recorded, once read.
    pub closure: Option<PrivateHandoverFence>,
    /// Whether the home's evidence was written and the home retained.
    pub home_retained: bool,
    /// Whether the home still held its continuation when it was retained.
    pub home_occupied: bool,
    /// Whether the maintenance obligation is committed into the store.
    pub committed: bool,
    /// What the number-keyed cleanup established, once attempted. Never
    /// attempted twice: `Unestablished` is final.
    pub namespace: Option<PrivateNamespaceClearance>,
}

/// What a completed visit established.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateDeferredCleanupReport {
    pub closure: PrivateHandoverFence,
    pub committed: bool,
    pub namespace: PrivateNamespaceClearance,
    pub home_occupied: bool,
}

/// Where one connection's deferred cleanup stands.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateDeferredCleanupStanding {
    NotVisited,
    /// The last visit refused, and why, keeping whatever phases had
    /// completed. A later visit may find the prerequisite met and continue.
    Refused {
        refusal: PrivateDeferredCleanupRefusal,
        progress: PrivateDeferredCleanupProgress,
    },
    /// A visit claimed the execution and has not published its end.
    Claimed {
        progress: PrivateDeferredCleanupProgress,
    },
    /// The execution ran to its end and this is what it established.
    Done(PrivateDeferredCleanupReport),
}

/// One custody's answer to a visit, by place.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateDeferredCleanupOutcome {
    pub place: usize,
    pub result: Result<PrivateDeferredCleanupReport, PrivateDeferredCleanupRefusal>,
}

/// Owns the connection's lease and holder destination for the whole interval
/// between leaving the record and being committed.
///
/// EVERY WAY OUT RESTORES. A refusal returns, an unwind drops: either way,
/// whatever was not committed goes back on the record -- the lease into the
/// record's own slot if that slot is still empty (a successor's is never
/// overwritten), the destination back to Reserved by its own Drop -- without
/// holding the store aggregate or the record's slot mutex across anything.
#[cfg(unix)]
struct PrivateLeaseRestorer<'a> {
    record: &'a PrivateCleanupRecord,
    lease: Option<PrivateOrderedContinuationSlot>,
    destination: Option<PrivateHolderDestination>,
    context: Option<PrivateCommitmentContext<'a>>,
    committed: bool,
}

#[cfg(unix)]
impl Drop for PrivateLeaseRestorer<'_> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Some(context) = self.context.take()
            && let Some((lease, destination)) = context.into_parts()
        {
            self.record.put_back_lease(lease);
            drop(destination);
            return;
        }
        drop(self.destination.take());
        if let Some(lease) = self.lease.take() {
            self.record.put_back_lease(lease);
        }
    }
}

#[cfg(unix)]
impl PrivateEvidenceCustody {
    /// Where this connection's deferred cleanup stands.
    fn deferred_cleanup_standing(&self) -> PrivateDeferredCleanupStanding {
        match self.source.deferred_cleanup.lock() {
            Ok(standing) => *standing,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    /// Keep the claimed execution's progress current, so an interruption
    /// after any phase leaves that phase on record.
    fn note_progress(&self, progress: PrivateDeferredCleanupProgress) {
        self.set_deferred_cleanup(PrivateDeferredCleanupStanding::Claimed { progress });
    }

    fn set_deferred_cleanup(&self, standing: PrivateDeferredCleanupStanding) {
        let mut held = match self.source.deferred_cleanup.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
        *held = standing;
    }

    /// The prerequisites, from this connection's durable facts and the
    /// collection's token alone.
    fn deferred_cleanup_prerequisites(
        &self,
        collected: Option<&PrivateConnectionsCollected>,
    ) -> Result<(), PrivateDeferredCleanupRefusal> {
        use PrivateDeferredCleanupRefusal as Refusal;
        if !collected.is_some_and(|token| token.governs(self.cleanup_record())) {
            return Err(Refusal::ConnectionsUncollected);
        }
        match self.cleanup_record().destruction_standing() {
            PrivateDestructionStanding::NotRequested => return Err(Refusal::DestructionNotRequested),
            PrivateDestructionStanding::Requested => return Err(Refusal::DestructionUndecided),
            PrivateDestructionStanding::Decided(PrivateDestructionDecision::Synchronous) => {
                return Err(Refusal::NotDeferred);
            }
            PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(
                PrivateDestructionDeferral::WorkerRunning
                | PrivateDestructionDeferral::WorkerHandedOn,
            )) => {}
            PrivateDestructionStanding::Decided(PrivateDestructionDecision::Deferred(other)) => {
                return Err(Refusal::DestructionUnestablished(other));
            }
        }
        // THE JOIN IS THE CUSTODY'S PUBLISHED ONE. A collection report, a
        // left mark, a handed-on slot or a socket at EOF is not it.
        if self.join().result().is_none() {
            return Err(Refusal::JoinUnpublished);
        }
        Ok(())
    }

    /// Visit this connection's deferred cleanup, discharging it where every
    /// prerequisite is established and refusing, visibly, where not.
    ///
    /// CLAIMED BEFORE THE FIRST EFFECT, DONE AFTER THE LAST, PROGRESS KEPT
    /// IN BETWEEN AND ACROSS REFUSALS. A repeat visit finds `Done` and
    /// answers from it, finds `Claimed` and answers `Interrupted`, or finds a
    /// refusal and asks the prerequisites again, continuing from whatever
    /// phases already completed.
    fn visit_deferred_cleanup(
        &self,
        collected: Option<&PrivateConnectionsCollected>,
    ) -> PrivateDeferredCleanupOutcome {
        let place = self.identity().index;
        let result = self.discharge_deferred_cleanup(collected);
        PrivateDeferredCleanupOutcome { place, result }
    }

    fn discharge_deferred_cleanup(
        &self,
        collected: Option<&PrivateConnectionsCollected>,
    ) -> Result<PrivateDeferredCleanupReport, PrivateDeferredCleanupRefusal> {
        use PrivateDeferredCleanupRefusal as Refusal;
        let progress = match self.deferred_cleanup_standing() {
            PrivateDeferredCleanupStanding::Done(report) => return Ok(report),
            PrivateDeferredCleanupStanding::Claimed { .. } => return Err(Refusal::Interrupted),
            PrivateDeferredCleanupStanding::NotVisited => PrivateDeferredCleanupProgress::default(),
            PrivateDeferredCleanupStanding::Refused { progress, .. } => progress,
        };
        if let Err(refusal) = self.deferred_cleanup_prerequisites(collected) {
            self.set_deferred_cleanup(PrivateDeferredCleanupStanding::Refused { refusal, progress });
            return Err(refusal);
        }
        self.set_deferred_cleanup(PrivateDeferredCleanupStanding::Claimed { progress });
        #[cfg(all(test, unix))]
        routing_tests::stage_deferred_cleanup(PrivateDeferredCleanupPoint::AfterClaim);
        let mut progress = progress;
        let outcome = self.run_deferred_cleanup(&mut progress);
        self.set_deferred_cleanup(match outcome {
            Ok(report) => PrivateDeferredCleanupStanding::Done(report),
            Err(refusal) => PrivateDeferredCleanupStanding::Refused { refusal, progress },
        });
        outcome
    }

    /// The execution itself, after the claim, continuing from `progress`.
    fn run_deferred_cleanup(
        &self,
        progress: &mut PrivateDeferredCleanupProgress,
    ) -> Result<PrivateDeferredCleanupReport, PrivateDeferredCleanupRefusal> {
        use PrivateDeferredCleanupRefusal as Refusal;
        let record = self.cleanup_record();
        // THE FENCE: the custody's one attempt. A fence already read is kept;
        // an attempt already spent is read, not repeated.
        let fence = PrivateFenceRecord::bound_to(self);
        let closure = match progress.closure {
            Some(closure) => closure,
            None => {
                let _ = fence.record_fence();
                let Some(closure) = fence.fence() else {
                    return Err(Refusal::FenceUnrecorded(fence.phase()));
                };
                progress.closure = Some(closure);
                self.note_progress(*progress);
                closure
            }
        };
        // THE EVIDENCE, IN THE HOME THE WORK STAYS IN, written once: the
        // recorded fence, whether the home could be read, and the joined
        // worker named by its own join evidence. Then the home is retained,
        // not drained.
        if !progress.home_retained {
            let source_poisoned = record.ordered_home.unreadable();
            let joined = PrivateOrderedWorkerExit::Joined(Arc::downgrade(self.join()));
            let _ = record.ordered_home.borrow(|continuation| {
                let recorded = match continuation {
                    PrivateOrderedContinuation::Setup { evidence, .. }
                    | PrivateOrderedContinuation::Serving { evidence, .. } => evidence,
                };
                recorded.fence = Some(closure);
                recorded.source_poisoned = source_poisoned;
                recorded.worker = joined.clone();
            });
            progress.home_occupied = record.ordered_home.retain();
            progress.home_retained = true;
            self.note_progress(*progress);
        }
        // THE COMMITMENT, over this connection's own lease and the holder
        // destination reserved with its place, owned by the restorer for
        // the whole interval. An obligation already in the store is not
        // committed again.
        if !progress.committed {
            if self
                .store()
                .committed_obligation(self.identity().index)
                .is_some()
            {
                progress.committed = true;
            } else {
                let lease = match record.ordered_continuation.lock() {
                    Ok(mut held) => held.take(),
                    Err(poisoned) => poisoned.into_inner().take(),
                };
                let Some(lease) = lease else {
                    return Err(Refusal::LeaseMissing);
                };
                let mut restorer = PrivateLeaseRestorer {
                    record,
                    lease: Some(lease),
                    destination: None,
                    context: None,
                    committed: false,
                };
                #[cfg(all(test, unix))]
                routing_tests::stage_deferred_cleanup(PrivateDeferredCleanupPoint::AfterLeaseTaken);
                let lease = restorer.lease.as_ref().expect("held until prepared");
                match self.store().prepare_internal_holder(lease) {
                    Ok(destination) => restorer.destination = Some(destination),
                    Err(refusal) => return Err(Refusal::HolderRefused(refusal)),
                }
                #[cfg(all(test, unix))]
                routing_tests::stage_deferred_cleanup(
                    PrivateDeferredCleanupPoint::AfterDestinationPrepared,
                );
                let lease = restorer.lease.take().expect("held until bound");
                let destination = restorer.destination.take().expect("prepared above");
                restorer.context = Some(PrivateCommitmentContext::bound_to(
                    self,
                    &fence,
                    lease,
                    destination,
                ));
                let committed = restorer
                    .context
                    .as_ref()
                    .expect("bound above")
                    .commit();
                match committed {
                    PrivateCommitted::Committed => restorer.committed = true,
                    refused => return Err(Refusal::CommitRefused(refused)),
                }
                progress.committed = true;
                self.note_progress(*progress);
            }
        }
        // CLOSURE MUST BE ESTABLISHED before anything keyed by the number
        // runs. An Unreadable fence is a committed obligation and an
        // unestablished closure; the number stays with the connection.
        if closure == PrivateHandoverFence::Unreadable {
            return Err(Refusal::ClosureUnestablished);
        }
        // THE NUMBER-KEYED BODY, ONCE. It is the last phase, so a run that
        // reaches it is the one run this custody makes: what it established
        // goes into the report and the standing becomes Done, and a repeat
        // answers from that. `Unestablished` keeps the number excluded and
        // is not retried.
        let namespace = record.clear_namespace_under_number();
        progress.namespace = Some(namespace);
        Ok(PrivateDeferredCleanupReport {
            closure,
            committed: progress.committed,
            namespace,
            home_occupied: progress.home_occupied,
        })
    }
}

#[cfg(unix)]
impl PrivateCleanupRecord {
    /// Return a lease that a refused or interrupted commitment could not
    /// take, into this record's own slot if it is still empty.
    fn put_back_lease(&self, lease: PrivateOrderedContinuationSlot) {
        let mut held = match self.ordered_continuation.lock() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
        if held.is_none() {
            *held = Some(lease);
        }
    }
}

/// Visit every custody this registry published, after its collection.
///
/// THE SAME SELECTION AS COLLECTION, after it, under the collection's own
/// token: without one, every custody answers `ConnectionsUncollected` and
/// nothing runs.
#[cfg(unix)]
fn run_deferred_cleanups(
    service: &PrivateServiceLease<'_>,
    registry: &XServerFrontendRouteRegistry,
    collected: Option<&PrivateConnectionsCollected>,
) -> Vec<PrivateDeferredCleanupOutcome> {
    service
        .custodies_of(registry)
        .iter()
        .map(|pin| pin.visit_deferred_cleanup(collected))
        .collect()
}
