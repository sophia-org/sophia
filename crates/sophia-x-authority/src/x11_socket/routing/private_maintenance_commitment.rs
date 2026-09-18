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
    /// ALL THREE STAY DISTINCT, Unreadable included. It says closure was not
    /// established, which is not the same as saying the gate is open: the lock
    /// WAS acquired -- that is what poisoning means -- so nothing here licenses
    /// a reader to conclude that handovers are still being admitted, and an
    /// obligation recording it as an establishment would be claiming one
    /// nobody made.
    ///
    /// It is also not a reason to refuse to record the responsibility --
    /// something is still owed, and that case is the one this retention exists
    /// for.
    closed: PrivateHandoverFence,
    /// The join this obligation rests on: the evidence itself, named weakly.
    ///
    /// THE EVIDENCE AND NOT A COPY OF ITS SHAPE, so a worker that panicked
    /// still has its payload here after the frames that joined it have gone. A
    /// copied flag with the payload dropped alongside the caller's record
    /// would be a different and much weaker thing.
    ///
    /// AND NAMED, NOT OWNED, FOR A REASON THAT IS WORTH STATING PLAINLY. A
    /// panic payload is whatever the panicking frame was carrying -- `Any +
    /// Send` and nothing more -- so it may itself hold a strong handle to this
    /// very store. A store that OWNED such a payload would be on a ring with
    /// itself: store, holder, obligation, evidence, payload, store. Nothing
    /// here can prevent that by inspecting the payload without discarding or
    /// special-casing it, and neither is acceptable.
    ///
    /// SO THE KEEPER IS OUTSIDE, AND IT IS THE CUSTODIAN. The evidence home
    /// belongs to the custody that existed before any of this ran; the store
    /// names it and does not keep it. Committing hands its caller nothing --
    /// the answer carries no handle at all -- because an obligation that
    /// depended on the caller storing one returned would rest on something
    /// this cannot check.
    ///
    /// FOR AT LEAST AS LONG AS THE CUSTODIAN LASTS, which is a floor and not
    /// an instant. The custodian guarantees the evidence is here while it
    /// lives; it does not decide when the evidence goes, because a reader may
    /// be holding a strong handle of its own. This weak name stops resolving
    /// when the LAST strong owner lets go, whichever that turns out to be --
    /// which is not a disposition and not a fresh fact about the join. A
    /// payload holding a store handle makes a chain from those owners rather
    /// than a ring through the store, and the store is released when the last
    /// of them goes.
    ///
    /// WHAT IS STILL MISSING. No production constructor makes a custody, so
    /// nothing here establishes who holds one across the running server's
    /// shutdown and error paths. At integration that must be the durable outer
    /// service owner -- the same authority that keeps the store across service
    /// exits -- and that owner does not exist yet. This is the narrower
    /// boundary that keeping an opaque payload forced, named here rather than
    /// absorbed into a claim that one ownership shape satisfied both
    /// requirements.
    join: std::sync::Weak<PrivateJoinEvidence>,
}

/// What a commitment did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateCommitted {
    /// The obligation is in this connection's destination and the duty is the
    /// store's.
    ///
    /// NOTHING IS HANDED BACK, AND THAT IS THE POINT. An earlier shape
    /// returned the evidence and said the caller must keep it, which offered a
    /// keeper rather than making one: a caller could drop it or unwind holding
    /// it, and the evidence would go while the obligation stayed outstanding.
    /// The custody this commitment borrowed already owns that home and still
    /// does, so there is nothing here whose loss could cost the result.
    Committed,
    /// The evidence this rests on is not complete yet.
    ///
    /// NOTHING WAS CONSUMED AND NOTHING WAS CHANGED, the prepared destination
    /// included: it stays in this context, still promised to this place, so
    /// the same context asks again when its evidence arrives. Nothing here
    /// joins, closes, cancels or retries to make itself eligible.
    NotYetEvidenced,
    /// This connection's place is not the lease's any more, or the destination
    /// is not the one prepared for it.
    ///
    /// The successor keeps its place, its destination and its work, and
    /// nothing is marked against it.
    Stale,
    /// The destination or the outer holder is not of this lease's store.
    Foreign,
    /// This context has already committed. Its evidence stands.
    AlreadyCommitted,
}

/// One connection's commitment, bound once to everything it acts on.
///
/// CAPTURED, NOT PASSED PER VISIT. A commitment that took a destination and a
/// fence at every call is a context with no connection of its own: an early
/// visit hands the lease back, and the next visit can be given another
/// connection's recorded fence -- committing this connection's obligation on
/// the strength of a thread that served a different one, with this
/// connection's gate still open. There is no substitute to pass here.
///
/// THE PREPARED DESTINATION STAYS. An early visit is retryable on this same
/// context, so the destination it was given is kept rather than dropped and
/// re-prepared; dropping it would put the entry back to reserved and change
/// the very accounting the outcome says it has not touched.
///
/// THAT THE PARTS BELONG TO ONE CONNECTION IS THE CALLER'S OBLIGATION, as
/// everywhere else here: binding them once prevents the set being changed
/// afterwards, which is a smaller and different thing from establishing that
/// it was right to begin with.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Built by a caller no production site has yet.
struct PrivateCommitmentContext<'a> {
    /// The custody that owns this connection's evidence and its store.
    ///
    /// BORROWED, AND NOT EMPTIED BY FINISHING. A commitment publishes an
    /// obligation naming the home this custody owns; succeeding, refusing,
    /// being asked twice or unwinding changes nothing about who owns it.
    custody: &'a PrivateEvidenceCustody,
    /// The fencing whose recorded answer this commitment rests on, and which
    /// already names the join beneath it.
    fence: &'a PrivateFenceRecord<'a>,
    /// This connection's lease and the destination prepared for it, until a
    /// commitment consumes them.
    held: Mutex<Option<(PrivateOrderedContinuationSlot, PrivateHolderDestination)>>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Built by a caller no production site has yet.
impl<'a> PrivateCommitmentContext<'a> {
    fn bound_to(
        custody: &'a PrivateEvidenceCustody,
        fence: &'a PrivateFenceRecord<'a>,
        lease: PrivateOrderedContinuationSlot,
        destination: PrivateHolderDestination,
    ) -> Self {
        Self {
            custody,
            fence,
            held: Mutex::new(Some((lease, destination))),
        }
    }

    /// Take back what this context was holding, if it has not committed.
    ///
    /// For a caller whose commitment was refused and which has something else
    /// to do with the lease and the destination it prepared.
    fn into_parts(self) -> Option<(PrivateOrderedContinuationSlot, PrivateHolderDestination)> {
        self.held
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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
    /// ONE ACCOUNTING TRANSITION. The occupant and the expected promise are
    /// checked in the acquisition that installs the obligation and transfers
    /// the duty, for the same reason the conversion beside it does: a place
    /// whose work settled in the gap goes back, and the next connection takes
    /// the number. At the moment this becomes observable the identity and the
    /// evidence are already in place, the lease owes nothing, and exactly one
    /// holder owes this place's disposal.
    ///
    /// AND IT AUTHORISES NOBODY. The home's standing is untouched, no teardown
    /// evidence is written, no attention is retired, and nothing is taught to
    /// drive this record.
    fn commit(&self) -> PrivateCommitted {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some((lease, destination)) = held.as_mut() else {
            return PrivateCommitted::AlreadyCommitted;
        };
        if !self.custody.store().is_same_store(&lease.owner)
            || !destination.owner.is_same_store(&lease.owner)
            || destination.for_place != lease.index
            || !std::ptr::eq(destination.for_record.as_ptr(), lease.record.as_ptr())
        {
            return PrivateCommitted::Foreign;
        }
        // ASKED BEFORE ANYTHING IS CONSUMED, so an early visit leaves this
        // context exactly as it found it -- its lease and its prepared
        // destination included.
        let Some(closed) = self.fence.fence() else {
            return PrivateCommitted::NotYetEvidenced;
        };
        // THE CUSTODY MUST BE THIS CONNECTION'S, BY NAME AND BY HOME. A
        // commitment that took any live evidence owner could be given another
        // connection's completed join alongside this one's reservation and
        // state this connection's obligation over it; one that compared only
        // the home could be given a custody whose name was somebody else's.
        if !self
            .custody
            .identity()
            .same_as(&lease.maintenance_identity())
        {
            return PrivateCommitted::Foreign;
        }
        let evidence = self.fence.join_evidence();
        if !Arc::ptr_eq(&evidence, self.custody.join()) {
            return PrivateCommitted::Foreign;
        }
        if evidence.result().is_none() {
            // DEFENCE IN DEPTH, AND NO CONTROL REACHES IT. A fencing refuses
            // to ask the gate until its own join has published, so a recorded
            // fence already implies a published result. It is asked anyway
            // because this obligation NAMES that evidence, and resting on the
            // fence's guard instead would be resting on somebody else's check.
            return PrivateCommitted::NotYetEvidenced;
        }
        let identity = lease.maintenance_identity();
        let index = lease.index;
        {
            let mut store = self.custody.store().records_even_if_poisoned();
            // BOTH HALVES ARE ASKED, AND EITHER CAN FAIL ALONE. A place whose
            // work settled goes back, the next connection takes the number and
            // prepares a destination of its own -- and that promise matches by
            // number while the home does not. A commitment that asked only
            // about the promise would find it satisfied and state this
            // connection's obligation into the successor's destination.
            let current = matches!(
                store.continuations.get(index),
                Some(PrivateOrderedContinuationPlace::Taken(home))
                    if std::ptr::eq(Arc::as_ptr(home), lease.record.as_ptr())
            );
            let promised = matches!(
                store.holders.get(destination.index),
                Some(PrivateHolderPlace::Promised(promised))
                    if *promised == destination.for_place
            );
            if !current || !promised {
                drop(store);
                return PrivateCommitted::Stale;
            }
            // Built only now: a credit's own Drop re-enters the store, so one
            // that came into existence under this guard and was then dropped
            // would be this thread waiting for itself. Everything that can
            // refuse has already happened.
            let holder = PrivateHolderPlace::Taken(PrivateStoreOwnedHolder {
                credit: PrivateInternalCredit {
                    owner: self.custody.store().settlement_ref(),
                    index,
                    record: lease.record.clone(),
                    armed: true,
                },
                obligation: Some(PrivateCommittedObligation {
                    identity,
                    closed,
                    // Named weakly, and owned by the custody that was taken
                    // before any of this began.
                    join: Arc::downgrade(self.custody.join()),
                }),
            });
            store.holders[destination.index] = holder;
            // THE DUTY MOVES HERE, in the same acquisition: the credit is
            // armed and the lease is disarmed together, so there is exactly
            // one holder of it at every instant.
            lease.armed = false;
            destination.armed = false;
        }
        // Consumed: this context has committed and cannot do so again.
        *held = None;
        let _ = evidence;
        PrivateCommitted::Committed
    }
}

/// What a store says about one connection's committed obligation.
///
/// A SNAPSHOT, TAKEN UNDER THE AGGREGATE AND READ OUTSIDE IT. Handing a
/// borrowed obligation to a callback ran that callback beneath the store's own
/// lock, so anything it did -- resolving the identity, reading the payload --
/// was another acquisition underneath this one. Everything here is cheap to
/// copy, so nothing is lost by taking it out first.
///
/// IT PINS NOTHING. The store was held by whoever asked for this, and stays
/// held only for as long as they hold it; the evidence is named weakly here as
/// it is in the obligation. Resolving the identity upgrades and pins the store
/// for that act, and asking for the evidence upgrades it for the caller -- but
/// holding this is not holding either.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
struct PrivateCommittedSnapshot {
    identity: PrivateMaintenanceIdentity,
    closed: PrivateHandoverFence,
    join: std::sync::Weak<PrivateJoinEvidence>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl PrivateCommittedSnapshot {
    fn identity(&self) -> &PrivateMaintenanceIdentity {
        &self.identity
    }

    fn closed(&self) -> PrivateHandoverFence {
        self.closed
    }

    fn join(&self) -> Option<Arc<PrivateJoinEvidence>> {
        self.join.upgrade()
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl PrivateSettlementOwner {
    /// What this store says about the obligation committed for a place.
    ///
    /// TAKEN OUT, NOT ACTED ON UNDER THE STORE. The aggregate is released
    /// before a caller does anything with this. What keeps the store alive
    /// meanwhile is the caller's own handle -- the one it asked through -- and
    /// not this: see the snapshot's own note, which pins neither the store nor
    /// the evidence.
    fn committed_obligation(&self, index: usize) -> Option<PrivateCommittedSnapshot> {
        let held = self.records_even_if_poisoned();
        let PrivateHolderPlace::Taken(holder) = held.holders.get(index)? else {
            return None;
        };
        holder
            .obligation
            .as_ref()
            .map(|obligation| PrivateCommittedSnapshot {
                identity: obligation.identity.clone(),
                closed: obligation.closed,
                join: obligation.join.clone(),
            })
    }
}
