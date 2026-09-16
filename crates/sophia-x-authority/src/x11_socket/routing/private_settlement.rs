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
    /// Obligations a sweep is part-way through.
    ///
    /// Owned here rather than in a local, so a sweep that unwinds leaves them
    /// in this owner's inventory instead of dropping them with a stack frame.
    /// Moving work out to settle it and putting back what could not be settled
    /// is the whole shape of a sweep, and the window in between is exactly
    /// where an interruption loses it.
    ///
    /// Anything found here belongs to a sweep that did not finish. It is
    /// unsettled by definition -- a settled obligation is removed as it is
    /// settled -- so recovering it is returning it, never answering it again.
    in_flight: Vec<(XServerFrontendRouteRegistry, PrivateOperation)>,
    /// The same, for the routed work whose credits a sweep is checking.
    outstanding_in_flight: Vec<(XServerFrontendRouteRegistry, PrivateIdentity)>,
    /// The same, for failed instances a recovery is part-way through.
    failed_in_flight: Vec<FailedInstance>,
    /// Whether a sweep is inside the call that answers an obligation.
    ///
    /// Written before that call can emit anything, so an unwind inside leaves
    /// it set. The entry it refers to is the last of `in_flight`, because a
    /// sweep always settles from the end.
    settling: bool,
    /// Obligations whose settlement was interrupted after it could have
    /// emitted.
    ///
    /// Separate from `in_flight` because the two are different facts. Work
    /// interrupted before its attempt is unsettled and can be driven again.
    /// Work interrupted during its attempt may already have had its outcome
    /// go out, and nothing here can tell which. Driving it again would answer
    /// twice; discarding it would repair a count by dropping an obligation;
    /// releasing its credit would be a receipt nobody issued. So it is kept,
    /// keeps its credit, and is reported as what it is.
    indeterminate: Vec<(XServerFrontendRouteRegistry, PrivateOperation)>,
    /// Routed work whose handle was abandoned before it finished.
    ///
    /// Carries the registry that can observe its terminal outcome, so the
    /// credit it already holds is released exactly when the work is genuinely
    /// answered. No fresh credit is taken at transfer: these already have one.
    outstanding: Vec<(XServerFrontendRouteRegistry, PrivateIdentity)>,
    /// Places reserved for connections' ordered continuations.
    ///
    /// The storage exists from the moment a place is reserved, so installing
    /// into one allocates nothing at the point where an accepted connection
    /// would otherwise be held by nobody.
    continuations: Vec<PrivateOrderedContinuationPlace>,
    /// How many of those places are taken.
    ///
    /// Live reservations and retained continuations share this: a connection
    /// keeps its place while it runs AND while its leftovers are here, so a
    /// churn of connections cannot grow retention past the bound by returning
    /// slots it still owes work against.
    continuation_slots: usize,
    /// Whether the bound below was declared, as against merely zero.
    ///
    /// Zero is a real bound -- an owner that admits no connection -- so it
    /// cannot also mean "not yet told". Without this, declaring a bound and
    /// re-declaring one would be the same operation, and the second would
    /// move the number under places already held.
    continuation_bound_declared: bool,
    /// How many places may be taken at once.
    continuation_capacity: usize,
    /// Where the next round of visits starts.
    ///
    /// Retained so a record that cannot progress does not take every visit:
    /// the next call begins after the one served last, not at the front.
    // Read only by the fair drive, which is not attached yet.
    #[cfg_attr(not(test), allow(dead_code))]
    continuation_cursor: usize,
    /// Places for holders the store itself keeps.
    ///
    /// One per connection place at most: a holder exists to be responsible for
    /// a place, so there cannot be more of them than there are places. The
    /// storage for one exists from the moment it is promised, so committing a
    /// conversion into it allocates nothing.
    // Read by conversions and by a maintenance driver that is not attached yet.
    #[cfg_attr(not(test), allow(dead_code))]
    holders: Vec<PrivateHolderPlace>,
    /// How many holder places are spoken for, promised and filled together.
    #[cfg_attr(not(test), allow(dead_code))]
    holders_taken: usize,
    /// Places whose holder went without disposing of them.
    ///
    /// Counted rather than reclaimed. Handing the capacity out again would
    /// promise it against work nobody accounted for.
    continuations_abandoned: usize,
    /// Terminal inventories handed over by instances that went.
    ///
    /// Kept as inventories rather than unpacked into the abandoned-work list:
    /// what is in them has already applied, or may already be on a client's
    /// queue, so turning it back into commands would replay effects. What is
    /// carried is the right to finish answering for them.
    terminal: Vec<PrivateTerminalInventory>,
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
impl AbandonedSettlements {
    /// Settle everything in flight, returning how many were answered.
    ///
    /// The obligation stays in this owner's list for the whole attempt. It is
    /// removed once the attempt has returned and said what happened, so there
    /// is no moment where it exists only as a local: a fault before the
    /// attempt leaves it in flight and replayable, and a fault during the
    /// attempt leaves it in flight and marked, which is the difference between
    /// work that can be driven again and work whose outcome nobody can prove.
    fn sweep_in_flight(&mut self) -> usize {
        let mut answered = 0usize;
        while !self.in_flight.is_empty() {
            // Established before anything is emitted. A settlement that could
            // not show it was the only owner has no business publishing, and
            // what it does instead depends on why.
            let ownership = {
                let (origin, operation) = self.in_flight.last().expect("not empty");
                ownership_of(origin, operation)
            };
            match ownership {
                SettlementOwnership::Elsewhere => {
                    // Someone else will publish for it. The command is not
                    // carried on as replayable work -- that is the duplicate
                    // this check exists to prevent -- but the credit it holds
                    // is not released either, because nothing here observed an
                    // outcome. Its identity moves to the routed list, where a
                    // credit is freed exactly when the registry retires it.
                    let (origin, operation) = self.in_flight.pop().expect("not empty");
                    let identity = PrivateIdentity::of(&operation);
                    self.outstanding.push((origin, identity));
                    continue;
                }
                SettlementOwnership::Unprovable => {
                    // Kept whole, credit and all. A retained obligation costs
                    // capacity until someone can read the registry again; the
                    // alternatives cost a client either a duplicate outcome or
                    // none at all.
                    let carried = self.in_flight.pop().expect("not empty");
                    self.held.push(carried);
                    continue;
                }
                SettlementOwnership::Ours => {}
            }
            // Marked before the attempt rather than after it. The attempt is
            // where the outcome is emitted, so an unwind inside leaves an
            // obligation nobody can classify by looking at it: it is still
            // here, and whether its acknowledgement went out is exactly what
            // was lost. That a settlement was under way can only be captured
            // before the effect it describes can happen.
            self.settling = true;
            let settled = {
                let (origin, operation) = self.in_flight.last().expect("not empty");
                settle_one(origin, operation)
            };
            self.settling = false;
            let carried = self.in_flight.pop().expect("not empty");
            if settled {
                // Answered, so its credit is free for new work.
                self.reserved = self.reserved.saturating_sub(1);
                answered = answered.saturating_add(1);
            } else {
                self.held.push(carried);
            }
        }
        answered
    }
}

#[cfg(unix)]
impl Default for PrivateSettlementOwner {
    fn default() -> Self {
        Self::with_capacity(PRIVATE_ABANDONED_CAPACITY)
    }
}

#[cfg(unix)]
impl PrivateSettlementOwner {
    /// The same owner, with the connection bound configured separately.
    ///
    /// A CONNECTION IS NOT AN OPERATION. The abandoned-work capacity counts
    /// obligations an instance accepted; this counts connections that may
    /// exist at once, live and retained together, and it comes from the
    /// declared client limit. Inheriting one for the other made the bound a
    /// coincidence.
    pub fn with_capacities(capacity: usize, connections: usize) -> Self {
        let owner = Self::with_capacity(capacity);
        {
            let mut held = owner.records_even_if_poisoned();
            held.continuations = Vec::with_capacity(connections);
            held.continuation_capacity = connections;
            held.continuation_bound_declared = true;
        }
        owner
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(AbandonedSettlements {
                held: Vec::with_capacity(capacity),
                in_flight: Vec::with_capacity(capacity),
                outstanding_in_flight: Vec::with_capacity(capacity),
                failed_in_flight: Vec::with_capacity(capacity),
                terminal: Vec::with_capacity(capacity),
                settling: false,
                indeterminate: Vec::with_capacity(capacity),
                outstanding: Vec::with_capacity(capacity),
                failed: Vec::with_capacity(capacity),
                failed_capacity: capacity,
                failure_slots: 0,
                // One place per connection this instance may admit, taken
                // from the same declared limit rather than a pool of its own:
                // a connection that cannot be handed over is one that must not
                // be exposed, so the two numbers have to be the same number.
                // NONE UNTIL CONFIGURED. A connection bound is not the
                // abandoned-work capacity, and inheriting one for the other
                // made the number a coincidence. An owner built without one
                // admits no connection rather than admitting as many as it
                // happens to allow obligations.
                continuations: Vec::new(),
                continuation_slots: 0,
                continuation_capacity: 0,
                continuation_bound_declared: false,
                continuation_cursor: 0,
                continuations_abandoned: 0,
                // No holder before a connection bound: holders are bounded by
                // the places they are responsible for, and a store told no
                // bound has no places to be responsible for.
                holders: Vec::new(),
                holders_taken: 0,
                reserved: 0,
                capacity,
            })),
        }
    }

    /// Reach this owner's records even through poison.
    ///
    /// For the paths that cannot refuse. Taking abandoned work, taking a
    /// failed instance's queue, and releasing a credit are all moves into
    /// space the work already reserved, so declining is not a refusal: there
    /// is nowhere to put what is declined, and the payload would be lost along
    /// with the credits it holds. Silently doing nothing on a poisoned lock is
    /// exactly that loss, and it is the shape this owner exists to prevent.
    ///
    /// This preserves work being handed over now. It does not establish that
    /// what was already here survived, and the distinction matters: the credit
    /// count spans held, outstanding and the instances still live, and the
    /// failure slots span live and retained failed instances. These are
    /// coupled, so each operation being individually safe says nothing about
    /// the inventory as a whole.
    ///
    /// The sweeps now move inventory between two places this owner holds
    /// rather than through a local, so an interruption leaves the untouched
    /// remainder here to be returned. What that does not cover is the one
    /// obligation a sweep has already moved into the call that settles it:
    /// `settle_against` takes it by value, so an unwind inside drops it, and
    /// that call is also where an acknowledgement is emitted. Retaining that
    /// one requires the settling call not to own it.
    ///
    /// Everything that can refuse still refuses rather than coming through
    /// here. Taking a credit or a failure slot on an owner nobody can read is
    /// declined, because that is a refusal before acceptance and the caller
    /// keeps what it has.
    fn records_even_if_poisoned(&self) -> std::sync::MutexGuard<'_, AbandonedSettlements> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// How many operations are waiting for someone to drive them.
    ///
    /// Drivable operations only. Inventories handed over by instances that
    /// closed owing something are not driven and not counted here; ask
    /// `terminal_inventories` for those.
    ///
    /// `None` where the owner cannot be read: nothing owed and nothing
    /// knowable are different answers.
    pub fn owed(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.held.len())
    }

    /// How many abandoned operations are still waiting on a terminal outcome.
    ///
    /// Abandoned operations only, with the same scope as `owed`.
    ///
    /// `None` where the owner cannot be read.
    pub fn outstanding(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.outstanding.len())
    }

    /// One at a time, so a caller hands over from a list it still owns rather
    /// than emptying itself into an argument first.
    fn take_one_outstanding(&self, origin: &XServerFrontendRouteRegistry, identity: PrivateIdentity) {
        // Cannot refuse, for the same reason abandoned obligations cannot:
        // every one of these already holds a credit taken before its work was
        // accepted, so this is a move into space already its own.
        let mut held = self.records_even_if_poisoned();
        held.outstanding.push((origin.clone(), identity));
    }

    /// Take an obligation whose settlement was interrupted while it was
    /// emitting.
    ///
    /// Cannot refuse, and does not answer. This is not a place work goes to be
    /// discharged: it is where an obligation goes when nobody can say whether
    /// it was. It keeps the credit it already holds, because releasing one
    /// here would be a receipt for an outcome nobody observed, and it is kept
    /// out of the lists a sweep drives so that it cannot be answered a second
    /// time.
    fn take_indeterminate(
        &self,
        origin: &XServerFrontendRouteRegistry,
        operation: PrivateOperation,
    ) {
        let mut held = self.records_even_if_poisoned();
        held.indeterminate.push((origin.clone(), operation));
    }

    /// How many instances closed holding a queue nobody could read.
    ///
    /// Each is retained with its queue and its registry, so it can be examined
    /// rather than merely counted.
    /// `None` where the owner cannot be read.
    pub fn failed_instances(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.failed.len())
    }

    fn take_failed_instance(
        &self,
        origin: &XServerFrontendRouteRegistry,
        queue: &Arc<Mutex<SharedQueue>>,
    ) {
        let mut held = self.records_even_if_poisoned();
        {
            // No capacity check. This instance reserved its slot before it was
            // exposed, so the space is already its own; refusing here would be
            // refusing after the failure, with nowhere to put what is refused.
            held.failed.push(FailedInstance {
                origin: origin.clone(),
                queue: Arc::clone(queue),
                slot: FailureSlot::Held,
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
    /// `None` where the owner cannot be read: recovering nothing and being
    /// unable to try are different answers, and only one of them says a later
    /// attempt might do something.
    pub fn recover_failed(&self) -> Option<usize> {
        let Ok(mut held) = self.inner.lock() else {
            return None;
        };
        // Moved into the owner's own in-flight list rather than a local, and
        // taken one at a time, so an unwind part-way through leaves the rest
        // here instead of dropping them with the frame. Appended rather than
        // taken, so the buffer reserved at construction survives and the next
        // failure does not allocate during cleanup -- exactly what reserving
        // it was meant to avoid.
        {
            let AbandonedSettlements {
                failed,
                failed_in_flight,
                ..
            } = &mut *held;
            failed_in_flight.append(failed);
        }
        let mut recovered = 0usize;
        while !held.failed_in_flight.is_empty() {
            {
                // Out of the failed queue and into this owner's in-flight list
                // directly. Collecting them into a local first is the widest
                // window of the three sweeps: the queue no longer has them and
                // nothing else does either, so an unwind loses a whole
                // instance's worth of accepted work at once. Each operation
                // keeps the origin it was accepted against, so what answers it
                // is still the registry that took it.
                let AbandonedSettlements {
                    failed_in_flight,
                    in_flight,
                    ..
                } = &mut *held;
                let instance = failed_in_flight.last().expect("not empty");
                let mut queue = match instance.queue.lock() {
                    Ok(queue) => queue,
                    // The guard is recoverable even though the lock is not: the
                    // work is still there and is still owed an answer.
                    Err(poisoned) => poisoned.into_inner(),
                };
                while let Some((_, _, operation)) = queue.ready.take_next() {
                    in_flight.push((instance.origin.clone(), operation));
                }
            }
            // Drained, so this instance's failure is resolved and its slot is
            // free for another. Marked on the record before the count moves,
            // and only if this record still holds it: an emptied record that
            // is restored and recovered a second time must not hand back a
            // slot another live instance is holding.
            let releasing = {
                let instance = held.failed_in_flight.last_mut().expect("not empty");
                let releasing = instance.slot == FailureSlot::Held;
                instance.slot = FailureSlot::Released;
                releasing
            };
            if releasing {
                held.failure_slots = held.failure_slots.saturating_sub(1);
            }
            let _emptied = held.failed_in_flight.pop().expect("not empty");
            recovered = recovered.saturating_add(held.sweep_in_flight());
        }
        Some(recovered)
    }

    /// How many credits are outstanding, across every instance sharing this.
    ///
    /// A credit is taken when work is accepted and released only when that
    /// work is answered, so it covers pending, in-flight and abandoned alike.
    /// `None` where the owner cannot be read.
    pub fn reserved(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.reserved)
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
        let mut held = self.records_even_if_poisoned();
        held.failure_slots = held.failure_slots.saturating_sub(1);
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
        // A release that does not happen is capacity lost for as long as this
        // owner lives, so this is one of the moves that cannot decline.
        let mut held = self.records_even_if_poisoned();
        held.reserved = held.reserved.saturating_sub(1);
    }

    /// Return whatever an interrupted sweep left part-way through.
    ///
    /// A sweep moves obligations out of the inventory it settles from and puts
    /// back what it could not settle. If it unwinds in between, the work is in
    /// this owner's in-flight lists: still owned, still holding its credit,
    /// and by construction unsettled, because a settled obligation is removed
    /// as it is settled rather than at the end.
    ///
    /// So this returns them and answers nothing. Poison is not permission to
    /// resume execution, and an obligation found here is not evidence of what
    /// happened to it -- only that a sweep did not finish with it.
    ///
    /// Reaches through poison, because refusing here refuses in exactly the
    /// case this exists for. The unwind that strands a sweep happens while the
    /// sweep holds this lock, so the interruption and the poison are the same
    /// event: a restore that declines to read a poisoned owner declines every
    /// time it is needed and succeeds only when there is nothing to do. This
    /// is also a move between lists this owner already holds, into space the
    /// work reserved before it was accepted, so there is nothing to refuse
    /// with and nowhere to put what is refused.
    ///
    /// Poison is still not permission to execute. Nothing here runs an
    /// operation or emits an outcome; it moves obligations back to where a
    /// later drive can consider them, and parks the one that cannot be
    /// considered again.
    ///
    /// Returns how many were returned. Doing it twice returns nothing the
    /// second time.
    pub fn restore_interrupted(&self) -> usize {
        let mut held = self.records_even_if_poisoned();
        let returned =
            held.in_flight.len() + held.outstanding_in_flight.len() + held.failed_in_flight.len();
        // Interrupted inside the attempt, so its outcome may already have gone
        // out. It is not returned to be driven again: that would answer it
        // twice if it was answered, and there is no way here to find out which
        // happened. Parked instead, keeping its credit, counted as an
        // obligation whose outcome nobody can prove. Discarding it would
        // repair the count by dropping it and releasing its credit would be a
        // receipt nobody issued.
        if held.settling {
            let AbandonedSettlements {
                in_flight,
                indeterminate,
                ..
            } = &mut *held;
            if let Some(unproved) = in_flight.pop() {
                indeterminate.push(unproved);
            }
            held.settling = false;
        }
        // Moved between two owned lists rather than through a local, for the
        // reason this whole path exists: an interruption part-way leaves the
        // remainder in the list it has not reached yet, and the buffers
        // reserved at construction survive so a later recovery does not
        // allocate.
        let AbandonedSettlements {
            held: settling,
            in_flight,
            outstanding,
            outstanding_in_flight,
            failed,
            failed_in_flight,
            ..
        } = &mut *held;
        settling.append(in_flight);
        outstanding.append(outstanding_in_flight);
        failed.append(failed_in_flight);
        returned
    }

    /// Obligations whose settlement was interrupted while it was emitting.
    ///
    /// Retained rather than resolved. Each still holds its credit, because an
    /// obligation nobody can prove was answered has not been shown to be
    /// discharged, and releasing on a guess is the manufactured receipt this
    /// owner exists to avoid. `None` where the owner cannot be read.
    pub fn indeterminate(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.indeterminate.len())
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
            // Not a drive that achieved nothing: one that could not look.
            return DriveProgress {
                readable: false,
                ..DriveProgress::default()
            };
        };
        // Moved between two owned places rather than into a local, and taken
        // one at a time, so an unwind part-way through leaves the rest here
        // rather than dropping them with the frame.
        {
            // Appended rather than taken into a local and extended from it.
            // A take leaves the work in a temporary for as long as it takes to
            // put it somewhere owned, however short that is, and swaps in a
            // vector of capacity zero -- so the buffer reserved at
            // construction goes too, and the next sweep allocates. This moves
            // the elements in one step and leaves the emptied list its own
            // capacity.
            let AbandonedSettlements {
                held: settling,
                in_flight,
                ..
            } = &mut *held;
            in_flight.append(settling);
        }
        let answered = held.sweep_in_flight();
        // Routed work that has since finished releases its credit here, once
        // and only on a genuine terminal outcome.
        //
        // Each origin settles its own before its records are read. Bound to
        // the registry that issued the work rather than to anything handed in,
        // and the scan it calls allocates nothing.
        for (origin, _) in &held.outstanding {
            if let Some(owner) = origin.control_completion() {
                let _settled = owner.reconcile_unstarted();
            }
        }
        // Into the owner's own in-flight list rather than a local, for the
        // same reason as above: an unwind part-way through leaves the rest
        // here instead of dropping them with the frame.
        {
            let AbandonedSettlements {
                outstanding,
                outstanding_in_flight,
                ..
            } = &mut *held;
            outstanding_in_flight.append(outstanding);
        }
        let mut reclaimed = 0usize;
        while !held.outstanding_in_flight.is_empty() {
            // Read while it is still in the list. This sweep emits nothing, so
            // an interruption here is always before an effect, and leaving the
            // item where it is keeps it replayable rather than lost.
            let ended = {
                let (origin, identity) = held.outstanding_in_flight.last().expect("not empty");
                match *identity {
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
                }
            };
            let carried = held.outstanding_in_flight.pop().expect("not empty");
            if ended {
                held.reserved = held.reserved.saturating_sub(1);
                reclaimed = reclaimed.saturating_add(1);
            } else {
                held.outstanding.push(carried);
            }
        }
        let mut lifecycle_readable = true;
        // Clone a capability, never move the terminal inventory out of its
        // durable owner. Common must not be acquired under the owner mutex:
        // dropping connection custody may enter this owner from common.
        let terminal_count = held.terminal.len();
        for index in 0..terminal_count {
            let lifecycle = held.terminal.get(index).map(|terminal| terminal.lifecycle.clone());
            drop(held);
            if let Some(lifecycle) = lifecycle
                && lifecycle.drive(NonZeroUsize::new(1).unwrap()).is_err()
            {
                lifecycle_readable = false;
            }
            held = match self.inner.lock() {
                Ok(held) => held,
                Err(_) => return DriveProgress { readable: false, answered, reclaimed },
            };
        }
        held.terminal.retain(|terminal| !terminal.is_empty());
        DriveProgress {
            readable: lifecycle_readable,
            answered,
            reclaimed,
        }
    }

    /// Take an instance's terminal inventory.
    ///
    /// Cannot refuse. These are obligations already accepted, and the space
    /// for them was reserved before any of it was; declining would destroy
    /// what an instance was handing over precisely because it could no longer
    /// answer for it.
    fn take_terminal(&self, inventory: PrivateTerminalInventory) {
        let mut held = self.records_even_if_poisoned();
        held.terminal.push(inventory);
    }

    /// How many instances handed over obligations they could not finish.
    /// `None` where the owner cannot be read.
    pub fn terminal_inventories(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.terminal.len())
    }

    /// One at a time, so the handle hands over from a list it still owns. A
    /// caller that emptied itself into an argument first would have nothing
    /// left to keep if the handover did not return.
    fn take_one(&self, origin: &XServerFrontendRouteRegistry, operation: PrivateOperation) {
        // The caller here is a drop, which cannot keep what it is handing
        // over or report that it failed to. Declining would be the silent loss
        // this owner exists to prevent, and the space is already this work's
        // own, so this is one of the moves that cannot refuse.
        let mut held = self.records_even_if_poisoned();
        held.held.push((origin.clone(), operation));
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
    /// Whether the owner could be read at all. A drive that achieved nothing
    /// and a drive that could not look are different answers.
    pub readable: bool,
    /// Obligations discharged by this drive.
    pub answered: usize,
    /// Credits released because their work reached a terminal outcome.
    pub reclaimed: usize,
}

#[cfg(unix)]
impl DriveProgress {
    /// Whether this drive changed anything at all.
    ///
    /// Of what a drive can reach. A drive answers operations; it does not
    /// discharge what a closed instance handed over, so a loop that runs
    /// until this is false has finished driving, not finished owing.
    pub fn made_progress(self) -> bool {
        self.answered > 0 || self.reclaimed > 0
    }
}

/// One instance that closed with a queue nobody could read.
#[cfg(unix)]
struct FailedInstance {
    origin: XServerFrontendRouteRegistry,
    queue: Arc<Mutex<SharedQueue>>,
    /// Whether this instance's failure slot has been given back.
    ///
    /// Carried on the record rather than inferred from the record being gone.
    /// A slot is released for one failure, and the only thing that identifies
    /// that failure is this record, so the fact that its slot was released has
    /// to live here: recovery can be interrupted after releasing and before
    /// removing it, and a restored record with no such state is
    /// indistinguishable from one that never released. Releasing again then
    /// hands back a slot this failure does not hold, which is another live
    /// instance's, and the count admits one more instance than the bound
    /// allows.
    slot: FailureSlot,
}

/// Whether a failed instance still holds the slot it reserved.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureSlot {
    /// Reserved before the instance was exposed and not yet given back.
    Held,
    /// Given back. Marked before the count is changed, so an interruption in
    /// between under-releases -- costing this owner one slot for its life --
    /// rather than releasing twice. One direction loses capacity, the other
    /// hands out capacity that does not exist.
    Released,
}

/// How many abandoned obligations one owner keeps.
#[cfg(unix)]
const PRIVATE_ABANDONED_CAPACITY: usize = 64;

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

