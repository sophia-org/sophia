// Settling work a private instance accepted and could not answer.
//
// Split from the admission surface by subject: what is owed, who holds the
// means to answer it, and what happens when the holder goes away.

/// Where obligations go when the handle holding them is abandoned.
///
/// A handle that is dropped with work still owed cannot retry forever in its
/// own `Drop`, and must not destroy what it holds either: a full channel with
/// a live receiver is congestion, not teardown, and removing the last owner is
/// the defect rather than proof the obligation ended. So the work moves here,
/// with the capability that can answer it, and stays until something drives
/// it.
///
/// Bounded, but never by refusing a transfer. Credits for abandoned work and
/// slots for failed instances are both taken before the work or the instance
/// exists, so arriving here is always into space already set aside. Refusing
/// at the moment of transfer would have nowhere to put what it declined.
#[cfg(unix)]
#[derive(Clone)]
pub struct PrivateSettlementOwner {
    inner: Arc<Mutex<AbandonedSettlements>>,
}

#[cfg(unix)]
struct AbandonedSettlements {
    held: Vec<(XServerFrontendRouteRegistry, PrivateOperation)>,
    /// Routed work whose handle was abandoned before it finished.
    ///
    /// Carries the registry that can observe its terminal outcome, so the
    /// credit it already holds is released exactly when the work is genuinely
    /// answered. No fresh credit is taken at transfer: these already have one.
    outstanding: Vec<(XServerFrontendRouteRegistry, PrivateIdentity)>,
    /// Instances whose queue could not be read when they closed.
    ///
    /// The queue itself is kept, not a tally of how many there were: a counter
    /// cannot be asked anything later, and cannot be shown to have been
    /// resolved. Nothing here resumes execution on a poisoned queue.
    ///
    /// A leaf, deliberately. Holding the admission itself would close a cycle
    /// -- this owner holds the record, the record held the admission, and the
    /// admission holds this owner -- so nothing would ever be freed. The queue
    /// alone refers back to nothing.
    failed: Vec<FailedInstance>,
    /// The most failed instances this will hold.
    ///
    /// Bounded separately from credits, because a poisoned instance can arrive
    /// having accepted nothing at all, so credits do not account for it.
    failed_capacity: usize,
    /// Failure slots taken by live instances.
    ///
    /// Reserved before an instance is exposed and held for its whole life, so
    /// a transfer after failure can never be refused. Checking capacity when
    /// a failed queue arrives would be refusing after the failure, with
    /// nowhere to put what is refused -- the same shape as counting an
    /// overflowing obligation as lost.
    failure_slots: usize,
    /// Credits taken when work was accepted, held until it is discharged.
    ///
    /// Reserved before acceptance rather than checked at transfer. A bound
    /// applied when abandoned work arrives has nowhere to put what it refuses,
    /// so refusing there destroys something already accepted -- the same
    /// defect as dropping a payload, wearing a capacity check. Refusing at
    /// acceptance costs a producer only work it was never told was taken.
    reserved: usize,
    capacity: usize,
}

#[cfg(unix)]
impl Default for PrivateSettlementOwner {
    fn default() -> Self {
        Self::with_capacity(PRIVATE_ABANDONED_CAPACITY)
    }
}

#[cfg(unix)]
impl PrivateSettlementOwner {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AbandonedSettlements {
                held: Vec::with_capacity(capacity),
                outstanding: Vec::with_capacity(capacity),
                failed: Vec::with_capacity(capacity),
                failed_capacity: capacity,
                failure_slots: 0,
                reserved: 0,
                capacity,
            })),
        }
    }

    /// How many obligations are waiting for someone to drive them.
    pub fn owed(&self) -> usize {
        self.inner.lock().map(|held| held.held.len()).unwrap_or(0)
    }

    /// How many abandoned operations are still waiting on a terminal outcome.
    pub fn outstanding(&self) -> usize {
        self.inner
            .lock()
            .map(|held| held.outstanding.len())
            .unwrap_or(0)
    }

    fn take_outstanding(
        &self,
        origin: &XServerFrontendRouteRegistry,
        outstanding: Vec<PrivateIdentity>,
    ) {
        // Cannot refuse, for the same reason abandoned obligations cannot:
        // every one of these already holds a credit taken before its work was
        // accepted, so this is a move into space already its own.
        if let Ok(mut held) = self.inner.lock() {
            for identity in outstanding {
                held.outstanding.push((origin.clone(), identity));
            }
        }
    }

    /// How many instances closed holding a queue nobody could read.
    ///
    /// Each is retained with its queue and its registry, so it can be examined
    /// rather than merely counted.
    pub fn failed_instances(&self) -> usize {
        self.inner.lock().map(|held| held.failed.len()).unwrap_or(0)
    }

    fn take_failed_instance(
        &self,
        origin: &XServerFrontendRouteRegistry,
        queue: &Arc<Mutex<SharedQueue>>,
    ) {
        if let Ok(mut held) = self.inner.lock() {
            // No capacity check. This instance reserved its slot before it was
            // exposed, so the space is already its own; refusing here would be
            // refusing after the failure, with nowhere to put what is refused.
            held.failed.push(FailedInstance {
                origin: origin.clone(),
                queue: Arc::clone(queue),
            });
        }
    }

    /// Recover what a failed instance's queue still holds, and answer it.
    ///
    /// A poisoned lock stays poisoned, but the data behind it is intact, so
    /// the obligations are readable even though the instance that accepted
    /// them is not usable. Nothing is resumed: what comes out is settled
    /// against the registry that accepted it, exactly as abandoned work is.
    /// This is what retaining the queue was for -- a tally could have been
    /// counted but never discharged.
    pub fn recover_failed(&self) -> usize {
        let Ok(mut held) = self.inner.lock() else {
            return 0;
        };
        // Drained in place rather than taken: mem::take would swap in a fresh
        // vector of capacity zero and drop the buffer reserved at
        // construction, so the next failure would allocate during cleanup --
        // exactly what reserving it was meant to avoid.
        let failed: Vec<FailedInstance> = held.failed.drain(..).collect();
        let mut recovered = 0usize;
        for instance in failed {
            let mut queue = match instance.queue.lock() {
                Ok(queue) => queue,
                // The guard is recoverable even though the lock is not: the
                // work is still there and is still owed an answer.
                Err(poisoned) => poisoned.into_inner(),
            };
            let mut pending = Vec::new();
            while let Some((_, _, operation)) = queue.ready.take_next() {
                pending.push(operation);
            }
            drop(queue);
            let before = pending.len();
            let survivors = settle_against(&instance.origin, pending);
            for _ in 0..before.saturating_sub(survivors.len()) {
                held.reserved = held.reserved.saturating_sub(1);
            }
            recovered = recovered.saturating_add(before.saturating_sub(survivors.len()));
            // The failure is resolved, so its slot is free for another
            // instance.
            held.failure_slots = held.failure_slots.saturating_sub(1);
            for operation in survivors {
                held.held.push((instance.origin.clone(), operation));
            }
        }
        recovered
    }

    /// How many credits are outstanding, across every instance sharing this.
    ///
    /// A credit is taken when work is accepted and released only when that
    /// work is answered, so it covers pending, in-flight and abandoned alike.
    pub fn reserved(&self) -> usize {
        self.inner.lock().map(|held| held.reserved).unwrap_or(0)
    }

    /// Take a failure slot for an instance about to be exposed.
    ///
    /// Taken before exposure, so an instance that exists can always hand over
    /// its queue if it fails. An instance that cannot get one is never built.
    fn reserve_failure_slot(&self) -> Result<(), AdmissionRefusal> {
        let Ok(mut held) = self.inner.lock() else {
            return Err(AdmissionRefusal::Unavailable);
        };
        if held.failure_slots >= held.failed_capacity {
            return Err(AdmissionRefusal::Saturated);
        }
        held.failure_slots = held.failure_slots.saturating_add(1);
        Ok(())
    }

    /// Release a failure slot whose instance closed without failing, or whose
    /// failure has been resolved.
    fn release_failure_slot(&self) {
        if let Ok(mut held) = self.inner.lock() {
            held.failure_slots = held.failure_slots.saturating_sub(1);
        }
    }

    /// Take a credit for work about to be accepted, if one is free.
    ///
    /// Shared across instances on purpose: the storage that will hold
    /// abandoned work is shared, so the accounting for it has to be.
    fn reserve(&self) -> Result<(), AdmissionRefusal> {
        // An unreachable owner and a full one are different answers. Reporting
        // both as saturation tells a caller to retry something that will not
        // improve, and hides that the accounting itself is broken.
        let Ok(mut held) = self.inner.lock() else {
            return Err(AdmissionRefusal::Unavailable);
        };
        if held.reserved >= held.capacity {
            return Err(AdmissionRefusal::Saturated);
        }
        held.reserved = held.reserved.saturating_add(1);
        Ok(())
    }

    /// Release a credit whose work has been answered.
    fn release(&self) {
        if let Ok(mut held) = self.inner.lock() {
            held.reserved = held.reserved.saturating_sub(1);
        }
    }

    /// Try to discharge everything waiting.
    ///
    /// Each obligation is retried against the registry that accepted it, never
    /// against another instance's. What still cannot be answered stays here.
    ///
    /// Two kinds of progress, reported separately because they are different
    /// facts. Answering an obligation emits a receipt or an acknowledgement;
    /// reclaiming one only notices that work someone else finished is done.
    /// A single number would let a caller read a drive that reclaimed several
    /// credits as having achieved nothing.
    pub fn drive(&self) -> DriveProgress {
        let Ok(mut held) = self.inner.lock() else {
            return DriveProgress::default();
        };
        let taken = std::mem::take(&mut held.held);
        let before = taken.len();
        for (origin, operation) in taken {
            let mut remaining = settle_against(&origin, vec![operation]);
            if let Some(operation) = remaining.pop() {
                held.held.push((origin, operation));
            } else {
                // Answered, so its credit is free for new work.
                held.reserved = held.reserved.saturating_sub(1);
            }
        }
        // Routed work that has since finished releases its credit here, once
        // and only on a genuine terminal outcome.
        let carried: Vec<_> = held.outstanding.drain(..).collect();
        let mut reclaimed = 0usize;
        for (origin, identity) in carried {
            let ended = match identity {
                PrivateIdentity::Delivery(Some(delivery)) => {
                    matches!(
                        origin.input_recovery.delivery_state(delivery),
                        DeliveryState::Ended
                    )
                }
                // Carried control is still observable: this owner holds the
                // failed instance's route registry, and that is where its
                // completion registry lives.
                PrivateIdentity::Control {
                    completion: Some(token),
                    ..
                } => matches!(
                    origin.control_completion().map(|owner| owner.state_of(token)),
                    Some(ControlRecordState::Retired)
                ),
                // Nothing observable yet, so nothing to conclude.
                PrivateIdentity::Delivery(None)
                | PrivateIdentity::Control { completion: None, .. }
                | PrivateIdentity::Lease(_) => false,
            };
            if ended {
                held.reserved = held.reserved.saturating_sub(1);
                reclaimed = reclaimed.saturating_add(1);
            } else {
                held.outstanding.push((origin, identity));
            }
        }
        DriveProgress {
            answered: before.saturating_sub(held.held.len()),
            reclaimed,
        }
    }

    /// Take responsibility for abandoned work.
    ///
    /// Cannot refuse. Every operation here already holds a credit taken when
    /// it was accepted, so the storage for it is reserved and this is a move
    /// into space that was set aside rather than a request for space.
    fn take(&self, origin: &XServerFrontendRouteRegistry, pending: Vec<PrivateOperation>) {
        let Ok(mut held) = self.inner.lock() else {
            // The owner itself is unreachable. Nothing can be moved into it,
            // and pretending otherwise would lose the work silently; the
            // caller keeps it and reports.
            return;
        };
        for operation in pending {
            held.held.push((origin.clone(), operation));
        }
    }
}

/// What one drive of a settlement owner achieved.
///
/// Answered and reclaimed are different things. The first emitted a receipt or
/// an acknowledgement to someone waiting for one; the second only observed
/// that work already finished elsewhere is finished, and freed what it held.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DriveProgress {
    /// Obligations discharged by this drive.
    pub answered: usize,
    /// Credits released because their work reached a terminal outcome.
    pub reclaimed: usize,
}

#[cfg(unix)]
impl DriveProgress {
    /// Whether this drive changed anything at all.
    pub fn made_progress(self) -> bool {
        self.answered > 0 || self.reclaimed > 0
    }
}

/// One instance that closed with a queue nobody could read.
#[cfg(unix)]
struct FailedInstance {
    origin: XServerFrontendRouteRegistry,
    queue: Arc<Mutex<SharedQueue>>,
}

/// How many abandoned obligations one owner keeps.
#[cfg(unix)]
const PRIVATE_ABANDONED_CAPACITY: usize = 64;

/// What a shutdown could not settle, and the means to settle it later.
///
/// Counting an unsettled obligation and logging it is a diagnostic, not a
/// transfer. Nor is handing back the work alone: a report holding only
/// operations would have thrown away the registry that could answer them, so
/// a caller would be left holding obligations and nothing to discharge them
/// with. This retains the originating capability along with the work.
///
/// The obligations themselves stay private. They carry the stamped envelope
/// shape, and exporting that so an out-of-crate owner could read a report
/// would be publishing the wire format to deliver a status. What a caller
/// needs is not to inspect them but to retry them, which it can.
#[cfg(unix)]
#[must_use = "unsettled work is owed an answer; retry or record the failure"]
pub struct PrivateSettlement {
    /// The capability that can answer the work, retained from the instance
    /// that accepted it. Not supplied by a caller: an external authority
    /// argument would let one instance's obligations be settled against
    /// another's registry.
    origin: XServerFrontendRouteRegistry,
    /// Where anything still owed goes if this handle is abandoned.
    durable: PrivateSettlementOwner,
    /// The queue this came from, retained so a failed instance hands over its
    /// queue rather than a note that one existed. The queue alone, not the
    /// admission that holds the owner: that would be a cycle.
    queue: Arc<Mutex<SharedQueue>>,
    pending: Vec<PrivateOperation>,
    /// Work that was routed and has not reached a terminal outcome.
    ///
    /// Carried from the instance rather than left to die with it. These hold
    /// credits, and some of them -- input with a tracked delivery -- can still
    /// finish, so destroying the identities would strand the credits and lose
    /// the only means of noticing.
    outstanding: Vec<PrivateIdentity>,
    queue_unreadable: bool,
}

#[cfg(unix)]
impl PrivateSettlement {
    pub fn is_settled(&self) -> bool {
        self.pending.is_empty() && self.outstanding.is_empty() && !self.queue_unreadable
    }

    /// How many obligations remain undischarged.
    pub fn owed(&self) -> usize {
        self.pending.len()
    }

    /// How many routed operations have not reached a terminal outcome.
    pub fn outstanding(&self) -> usize {
        self.outstanding.len()
    }

    /// How many control records the instance's registry still holds.
    ///
    /// Operations caught mid-application are the ones that stay: they are
    /// retained rather than reported as unexecuted, and they are reachable
    /// rather than counted and forgotten, because the registry holding them
    /// came with the origin this settlement kept.
    pub fn outstanding_control(&self) -> usize {
        self.origin
            .control_completion()
            .map(|owner| owner.outstanding())
            .unwrap_or(0)
    }

    /// Republish acknowledgements a client writer could not deliver.
    ///
    /// Republishing only: the effects already happened, so nothing here is
    /// re-run. Returns how many reached the receiver.
    pub fn republish_owed_acknowledgements(&self) -> usize {
        let Some(owner) = self.origin.control_completion() else {
            return 0;
        };
        let sender = &self.origin.acknowledgement_sender;
        owner.publish_owed_with(|acknowledgement| match sender.try_send(*acknowledgement) {
            Ok(()) => ControlPublication::Delivered,
            Err(TrySendError::Disconnected(_)) => ControlPublication::ReceiverGone,
            Err(TrySendError::Full(_)) => ControlPublication::Retained,
        })
    }

    /// Release credits for carried work that has since finished.
    ///
    /// The same rule as on a live instance: ended releases, live and
    /// unreadable do not.
    pub fn reclaim_outstanding(&mut self) -> usize {
        let recovery = &self.origin.input_recovery;
        let recovery_origin = &self.origin;
        let before = self.outstanding.len();
        self.outstanding.retain(|identity| match identity {
            PrivateIdentity::Delivery(Some(delivery)) => {
                !matches!(recovery.delivery_state(*delivery), DeliveryState::Ended)
            }
            PrivateIdentity::Control {
                completion: Some(token),
                ..
            } => !matches!(
                recovery_origin
                    .control_completion()
                    .map(|owner| owner.state_of(*token)),
                Some(ControlRecordState::Retired)
            ),
            PrivateIdentity::Delivery(None)
            | PrivateIdentity::Control { completion: None, .. }
            | PrivateIdentity::Lease(_) => true,
        });
        let reclaimed = before.saturating_sub(self.outstanding.len());
        for _ in 0..reclaimed {
            self.durable.release();
        }
        reclaimed
    }

    /// Whether the queue could not be read, so its contents were unrecoverable.
    pub fn queue_unreadable(&self) -> bool {
        self.queue_unreadable
    }

    /// Try again to discharge what remains, against the origin that accepted
    /// it.
    ///
    /// Returns how many were discharged this time. Work that still cannot be
    /// answered stays pending rather than being counted off, so retrying twice
    /// does not answer anything twice.
    pub fn retry(&mut self) -> usize {
        let pending = std::mem::take(&mut self.pending);
        let before = pending.len();
        self.pending = settle_against(&self.origin, pending);
        let answered = before.saturating_sub(self.pending.len());
        for _ in 0..answered {
            self.durable.release();
        }
        answered
    }
}

#[cfg(unix)]
impl Drop for PrivateSettlement {
    fn drop(&mut self) {
        if !self.outstanding.is_empty() {
            // Transferred whether or not anything else is owed. Returning
            // early on an empty pending list destroyed these, which is the
            // abandonment loss this handle exists to prevent, recreated in the
            // state that was added to prevent it.
            let outstanding = std::mem::take(&mut self.outstanding);
            self.durable.take_outstanding(&self.origin, outstanding);
        }
        if self.queue_unreadable {
            // Owned by something that outlives this rather than surviving as a
            // boolean on a handle that is going away.
            self.durable
                .take_failed_instance(&self.origin, &self.queue);
        }
        if self.pending.is_empty() {
            return;
        }
        // One attempt, not a loop: a Drop that retried until it succeeded
        // would block teardown on a congested channel. What that attempt
        // cannot answer moves to the durable owner rather than being
        // destroyed here -- a full channel with a live receiver is congestion,
        // and removing the last owner is not evidence the obligation ended.
        let pending = std::mem::take(&mut self.pending);
        let before = pending.len();
        let survivors = settle_against(&self.origin, pending);
        for _ in 0..before.saturating_sub(survivors.len()) {
            self.durable.release();
        }
        if !survivors.is_empty() {
            self.durable.take(&self.origin, survivors);
        }
    }
}

/// Answer what can be answered, returning what still cannot.
#[cfg(unix)]
fn settle_against(
    registry: &XServerFrontendRouteRegistry,
    pending: Vec<PrivateOperation>,
) -> Vec<PrivateOperation> {
    let mut unsettled = Vec::with_capacity(pending.len());
    for operation in pending {
        match operation {
            PrivateOperation::RoutedInput(envelope) => {
                let client = registry
                    .surfaces
                    .lock()
                    .ok()
                    .and_then(|surfaces| {
                        surfaces
                            .get(&envelope.route.request.target_surface)
                            .map(|route| route.client)
                    });
                let Some(client) = client else {
                    // Ownership is retained rather than resolved by guessing.
                    // A receipt goes to the issuer's channel rather than to the
                    // named client, so the harm is not that another client
                    // receives it; it is a receipt attributed to a client
                    // nobody resolved, which correlates with nothing. Choosing
                    // a recipient here would also choose it at the wrong
                    // moment: final target resolution belongs at execution.
                    unsettled.push(PrivateOperation::RoutedInput(envelope));
                    continue;
                };
                if registry
                    .send_input_delivery(
                        client,
                        envelope.route.delivery,
                        XAuthorityInputDeliveryOutcome::RouteRejected,
                    )
                    .is_err()
                {
                    unsettled.push(PrivateOperation::RoutedInput(envelope));
                }
            }
            PrivateOperation::Control(control, token) => {
                let acknowledgement = XAuthorityClientControlAck {
                    client: control.client,
                    acknowledgement: XAuthorityControlAck {
                        kind: control.command.kind(),
                        transaction: control.command.transaction(),
                        surface: control.command.surface(),
                        outcome: XAuthorityControlOutcome::AuthorityRejected,
                    },
                };
                // Nothing here executed the command, so what is retained on
                // failure is the command, not an outcome: the caller retries
                // this settlement, and a retry that only sends a rejection
                // replays nothing.
                //
                // No completion record is answered here. A command reaching
                // this point was handed on by an owner that gave up its record
                // as it did so, which is the one place that sees all of them
                // at once; answering again from here would give one operation
                // two owners able to publish for it.
                if registry
                    .acknowledgement_sender
                    .try_send(acknowledgement)
                    .is_err()
                {
                    unsettled.push(PrivateOperation::Control(control, token));
                }
            }
            PrivateOperation::LeaseRelease(release) => {
                if registry.release_route_lease(release).is_err() {
                    unsettled.push(PrivateOperation::LeaseRelease(release));
                }
            }
        }
    }
    unsettled
}

/// One operation the consumer took, and where it sat.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateRun {
    pub sequence: crate::ReadySequence,
    pub class: crate::ReadyClass,
    /// Which operation this was, not merely what kind.
    ///
    /// A class alone says a record was classified, not that it accompanied the
    /// work a producer actually submitted.
    pub identity: PrivateIdentity,
}

/// Which submitted operation a run corresponds to.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateIdentity {
    /// The delivery a routed input carried, when it carried one.
    Delivery(Option<XAuthorityInputDeliveryId>),
    /// The transaction a control named, with the registration that answers
    /// for it.
    ///
    /// The transaction alone is not an identity: two requests from one client
    /// can name the same one, so a credit keyed on it could be released by
    /// another request's outcome. The registration is unique to the operation.
    Control {
        transaction: TransactionId,
        completion: Option<ControlCompletionToken>,
    },
    /// The lease being retired.
    Lease(sophia_protocol::ApplicationRouteLeaseIdentity),
}

#[cfg(unix)]
impl PrivateIdentity {
    fn of(operation: &PrivateOperation) -> Self {
        match operation {
            PrivateOperation::RoutedInput(envelope) => Self::Delivery(envelope.route.delivery),
            // Every control command carries a transaction, so singling one
            // variant out and calling the rest untracked lost the identity of
            // everything except focus.
            PrivateOperation::Control(control, completion) => Self::Control {
                transaction: control.command.transaction(),
                completion: *completion,
            },
            PrivateOperation::LeaseRelease(release) => Self::Lease(release.identity),
        }
    }
}

