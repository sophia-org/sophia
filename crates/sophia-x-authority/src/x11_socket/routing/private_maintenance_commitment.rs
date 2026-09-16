// Committing one connection's maintenance obligation into the destination its
// reservation set aside, and keeping the evidence that obligation rests on.
//
// Split from the reservation by subject: reserving is storage and a name, and
// this is the act that puts a responsibility in it.
//
// COMMITTED IS NOT RUNNABLE. Nothing here says who may drive this connection's
// home. That is a separate authority which needs production start and teardown
// arbitration and a durable outer owner, and neither exists yet.

/// One connection's maintenance obligation, committed.
///
/// WHAT IT IS: a statement that this exact connection is owed maintenance,
/// that its worker has finished, and what closing its gate returned. It is
/// evidence and responsibility, not permission.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a driver no production site has yet.
struct PrivateCommittedObligation {
    /// Which connection's obligation this is.
    identity: PrivateMaintenanceIdentity,
    /// What closing this connection's gate returned.
    ///
    /// ALL THREE STAY DISTINCT, Unreadable included. A gate whose lock carried
    /// a panic out of somebody's handover is not a closed gate, and an
    /// obligation that recorded it as one would be claiming an establishment
    /// nobody made. It is also not a reason to refuse to record the
    /// responsibility -- something is still owed, and that case is the one
    /// this retention exists for.
    closed: PrivateHandoverFence,
    /// The join this obligation rests on, kept rather than copied.
    ///
    /// THE EVIDENCE ITSELF, so a worker that panicked still has its payload
    /// here after the frames that joined it have gone. A copied flag with the
    /// payload dropped alongside the caller's record would be a different and
    /// much weaker thing.
    join: Arc<PrivateJoinEvidence>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a driver no production site has yet.
impl PrivateCommittedObligation {
    fn identity(&self) -> &PrivateMaintenanceIdentity {
        &self.identity
    }

    fn closed(&self) -> PrivateHandoverFence {
        self.closed
    }

    fn join(&self) -> &Arc<PrivateJoinEvidence> {
        &self.join
    }
}

/// What a commitment did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[must_use]
enum PrivateCommitted {
    /// The obligation is in this connection's destination and the duty is the
    /// store's.
    Committed,
    /// The evidence this rests on is not complete yet.
    ///
    /// NOTHING WAS CONSUMED AND NOTHING WAS CHANGED. No duty moved, no
    /// destination was touched, and this same context may ask again when its
    /// evidence arrives. Nothing here joins, closes, cancels or retries to
    /// make itself eligible.
    NotYetEvidenced(PrivateOrderedContinuationSlot),
    /// This connection's place is not the lease's any more, or the destination
    /// is not the one prepared for it.
    ///
    /// The successor keeps its place, its destination and its work, and
    /// nothing is marked against it.
    Stale(PrivateOrderedContinuationSlot),
    /// The destination or the outer holder is not of this lease's store.
    Foreign(PrivateOrderedContinuationSlot),
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Committed by a caller no production site has yet.
impl PrivateOrderedContinuationSlot {
    /// Commit this connection's maintenance obligation into the destination
    /// reserved for it.
    ///
    /// ELIGIBILITY IS COMPLETED, PUBLISHED EVIDENCE AND NOTHING WEAKER. The
    /// bound fence must have recorded what the gate said, and the join it
    /// names must have published its result. Returned and Panicked are both
    /// completed joins, and all three close outcomes may be recorded: an
    /// unreadable gate still leaves something owed, and refusing to record
    /// that responsibility would hide the case this exists for.
    ///
    /// NOTHING IS DRIVEN TO MAKE IT ELIGIBLE. This does not join, close,
    /// cancel or retry anything, and it reads the published evidence without
    /// touching the optional exit diagnostic or the panic payload -- a
    /// commitment that waited on either would be hostage to whoever was
    /// reading them.
    ///
    /// THE FENCE ALREADY NAMES ITS JOIN, which is why no join is passed
    /// separately: a commitment that accepted one alongside could be given an
    /// unrelated completed join as though it were this connection's evidence.
    ///
    /// ONE ACCOUNTING TRANSITION. The occupant and the expected promise are
    /// checked again in the acquisition that installs the obligation and
    /// transfers the duty, for the same reason the conversion beside it does:
    /// a place whose work settled in the gap goes back, and the next
    /// connection takes the number. At the moment this becomes observable the
    /// identity and the evidence are already in place, the lease owes nothing,
    /// and exactly one holder owes this place's disposal.
    ///
    /// AND IT AUTHORISES NOBODY. The home's standing is untouched, no
    /// teardown evidence is written, no attention is retired, and nothing is
    /// taught to drive this record.
    fn commit_maintenance_obligation(
        mut self,
        outer: &PrivateSettlementOwner,
        mut destination: PrivateHolderDestination,
        fence: &PrivateFenceRecord<'_>,
    ) -> PrivateCommitted {
        if !outer.is_same_store(&self.owner)
            || !destination.owner.is_same_store(&self.owner)
            || destination.for_place != self.index
            || !std::ptr::eq(destination.for_record.as_ptr(), self.record.as_ptr())
        {
            return PrivateCommitted::Foreign(self);
        }
        // ASKED BEFORE ANYTHING IS CLAIMED OR CONSUMED, so an early request
        // leaves this context exactly as it found it.
        let Some(closed) = fence.fence() else {
            return PrivateCommitted::NotYetEvidenced(self);
        };
        let join = fence.join_evidence();
        if join.result().is_none() {
            // DEFENCE IN DEPTH, AND NO CONTROL REACHES IT. A fencing refuses
            // to ask the gate until its own join has published, so a recorded
            // fence already implies a published result. It is asked anyway
            // because what this obligation keeps is the join's evidence, and a
            // commitment that stated one over an empty result home would be
            // resting on the fence's guard rather than on what it is storing.
            return PrivateCommitted::NotYetEvidenced(self);
        }
        // EVERYTHING FALLIBLE IS DONE. What follows is one acquisition, two
        // assignments and nothing that can refuse.
        let identity = self.maintenance_identity();
        let index = self.index;
        {
            let mut held = outer.records_even_if_poisoned();
            // BOTH HALVES ARE ASKED, and only one of them is reachable on its
            // own. Returning a place frees its destination in the same act, so
            // every schedule that moves the occupant also breaks the promise,
            // and no control here can separate them. The occupant is asked
            // regardless: what this installs is an obligation naming a home,
            // and a promise surviving while the home it was made for did not
            // is exactly the case that must not be committed into.
            let current = matches!(
                held.continuations.get(index),
                Some(PrivateOrderedContinuationPlace::Taken(home))
                    if std::ptr::eq(Arc::as_ptr(home), self.record.as_ptr())
            );
            let promised = matches!(
                held.holders.get(destination.index),
                Some(PrivateHolderPlace::Promised(promised))
                    if *promised == destination.for_place
            );
            if !current || !promised {
                drop(held);
                return PrivateCommitted::Stale(self);
            }
            // Built only now: a credit's own Drop re-enters the store, so one
            // that came into existence under this guard and was then dropped
            // would be this thread waiting for itself.
            let holder = PrivateHolderPlace::Taken(PrivateStoreOwnedHolder {
                credit: PrivateInternalCredit {
                    owner: outer.settlement_ref(),
                    index,
                    record: Arc::downgrade(&self.record.upgrade().unwrap_or_else(|| {
                        unreachable!("the occupant check above upgraded this home")
                    })),
                    armed: true,
                },
                obligation: Some(PrivateCommittedObligation {
                    identity,
                    closed,
                    join,
                }),
            });
            held.holders[destination.index] = holder;
            // THE DUTY MOVES HERE, in the same acquisition: the credit is
            // armed and the lease is disarmed together, so there is exactly
            // one holder of it at every instant.
            self.armed = false;
            destination.armed = false;
        }
        PrivateCommitted::Committed
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl PrivateSettlementOwner {
    /// The obligation committed for a place, if one has been.
    ///
    /// BORROWED UNDER THE STORE and answered as a copy of what it says rather
    /// than by handing the record out: an obligation belongs to the store that
    /// keeps it.
    fn committed_obligation<R>(
        &self,
        index: usize,
        act: impl FnOnce(&PrivateCommittedObligation) -> R,
    ) -> Option<R> {
        let held = self.records_even_if_poisoned();
        let PrivateHolderPlace::Taken(holder) = held.holders.get(index)? else {
            return None;
        };
        holder.obligation.as_ref().map(act)
    }
}
