// A connection's own claim on a place in the continuation store.
//
// Split from the store by subject: the store is capacity and places, and this
// is the one connection's hold on one of them -- taken before it is exposed,
// accounted for exactly once, and consumed by whatever disposes of it.

/// A reserved place for one connection's ordered continuation.
///
/// TAKEN BEFORE THE CONNECTION IS EXPOSED, because a connection that cannot be
/// handed over is one whose accepted work has nowhere to go the moment
/// anything fails. A refused reservation leaves the connection unbuilt.
///
/// NEITHER COPY NOR CLONE. It is one place, held by one connection for the
/// whole of its ownership interval, and it is not given back because setup
/// failed, because the client went, or because the owner moved into retained
/// storage -- retained work still occupies the place it was promised. It comes
/// back when that work is finished and its disposition established, and the
/// only other way out is an explicit transfer to somewhere already reserved.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
struct PrivateOrderedContinuationSlot {
    /// The store this place belongs to, held as an owner.
    ///
    /// AN EXTERNAL RESERVATION IS A LEGITIMATE OWNER. This slot lives outside
    /// the store, in the registration of a connection that has been exposed
    /// and can already have work accepted for it. Nothing inside the store
    /// reaches it, so it is on no ring: it is one end of a lifetime, and the
    /// store must outlive it because there is a place in there this connection
    /// still has to dispose of.
    ///
    /// AND NOTHING ELSE ENFORCES THAT. The constructor takes the store by
    /// reference, but a reference parameter does not bind the instance or the
    /// registrations it hands out to the caller's binding, and the caller may
    /// drop its own holder while an exposed connection is still live. Held
    /// here, the connection carries its own guarantee, which is the only place
    /// it can be carried from.
    ///
    /// AN INTERNAL CREDIT IS NOT THIS SHAPE. A place-reference that lives
    /// INSIDE the store -- what a reaper record will need in order to name the
    /// place it is finishing -- closes a ring: store, record, credit, store.
    /// That holder must not be an owner, and converting it is a separate step
    /// with its own controls. Nothing here makes that conversion, and this
    /// field must not be copied into a holder the store itself keeps.
    owner: PrivateSettlementOwner,
    /// Which reserved place this is. The storage exists from the moment the
    /// slot does, so installing into it allocates nothing.
    index: usize,
    /// WHOSE PLACE, not merely which one.
    ///
    /// An index is not an identity. A place goes back when what was in it is
    /// settled, and the next connection to reserve one takes the same number,
    /// so a lease that outlived its place and still named only the number
    /// would install this connection's work into that one's record -- where
    /// its own teardown then overwrites it. The record this lease was made for
    /// is compared against what is in the place at every use and disposal.
    ///
    /// WEAK, because this is a name and not custody: the work is in the place,
    /// and a lease that kept a record alive would keep retained work alive
    /// past the store responsible for it. A weak handle keeps the allocation,
    /// so the address stays this record's while this lease exists.
    record: std::sync::Weak<PrivateOrderedHome>,
    /// Whether this slot still has to be disposed of.
    ///
    /// Cleared by whichever disposal actually happens, so the fallback in Drop
    /// cannot transfer or account for the same place twice.
    armed: bool,
}

/// What committing a connection's place found.
///
/// SAID, NOT ASSUMED. A caller that treated every return as a retention would
/// be asserting a premise rather than reading a result, and what is being
/// decided is whether anything is owed through this place at all.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
enum PrivateContinuationCommit {
    /// The place is retained: its home holds this connection's output, and
    /// what is in it is owed to whoever drives it.
    Retained,
    /// The place is not this lease's any more. Nothing was touched.
    NoPlace,
    /// The home is empty -- this connection never bound. Marked abandoned.
    NothingBound,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by a driver that is not attached yet.
impl PrivateOrderedContinuationSlot {
    /// Say that this connection has ended, and leave its place holding what
    /// it accepted.
    ///
    /// NOTHING MOVES, WHICH IS THE POINT. This used to take the payload out of
    /// the registration and put it in the place -- a move across the store's
    /// lock and the record's, with accepted work in a stack frame for the
    /// length of it, and every early return on the way a place where it could
    /// be lost. The payload has been in its home since this connection bound,
    /// and the home has been in this place since the place was reserved. What
    /// ends here is the connection, not the work's residence.
    ///
    /// THE HOME IS CHECKED, NOT THE NUMBER. A place that has gone on to
    /// another connection is not this lease's to commit; saying it were would
    /// retain that connection's home on this one's account.
    ///
    /// WHAT IS OWED IS THE HOME'S ANSWER, not this lease's guess. Teardown
    /// says the connection has ended and the home says whether anything is in
    /// it; this accounts for the place on that answer. Asking again here would
    /// be a second reading of a thing that has already changed hands.
    ///
    /// CONSUMES THE LEASE. A place is accounted for once, and a lease that
    /// stayed usable afterwards could account for it twice.
    fn commit(mut self, owed: bool) -> PrivateContinuationCommit {
        let home = {
            let held = self.owner.records_even_if_poisoned();
            match held.continuations.get(self.index) {
                Some(PrivateOrderedContinuationPlace::Taken(home))
                    if std::ptr::eq(Arc::as_ptr(home), self.record.as_ptr()) =>
                {
                    home.clone()
                }
                _ => {
                    // Not this lease's place. Nothing is retained and nothing
                    // is marked -- whoever holds it now accounts for it.
                    drop(held);
                    self.armed = false;
                    return PrivateContinuationCommit::NoPlace;
                }
            }
        };
        // The home is not consulted again here -- see above -- but it is held
        // so that what is accounted for is demonstrably the place's own home
        // and not whatever the number points at by now.
        let _ = &home;
        if owed {
            // The place stays taken and nothing is marked: what is in the home
            // is owed, and whoever drives it will say when it is not.
            self.armed = false;
            PrivateContinuationCommit::Retained
        } else {
            // Reserved, exposed, and never bound. Nothing was accepted through
            // this place, but nothing disposed of it either, so it is recorded
            // as what it is rather than handed out again.
            self.abandon();
            PrivateContinuationCommit::NothingBound
        }
    }

    /// Give up this place without disposing of it, and say so.
    fn abandon(&mut self) {
        let mut held = self.owner.records_even_if_poisoned();
        // ONLY IF THIS LEASE STILL HOLDS THE DUTY, and only over a place that
        // is still its own.
        //
        // `armed` is not bookkeeping about whether a disposal has run: it is
        // who owes one. A lease that handed the duty on -- to the credit a
        // conversion published over the same place -- owes nothing, and a
        // refusal arriving afterwards must not mark what the new holder will
        // mark. Ignoring it here meant one refused conversion was counted
        // twice: once by a lease that had already given the duty away, and
        // once by whoever ended up disposing of the credit.
        //
        // And a place that has gone on to another connection is accounted for
        // by whoever holds it now; marking would charge them an abandonment.
        if self.armed && self.holds(&held) {
            held.continuations_abandoned = held.continuations_abandoned.saturating_add(1);
        }
        self.armed = false;
    }

    /// The home this lease's place holds, if the place is still its own.
    ///
    /// A HANDLE, NOT THE PAYLOAD. Whoever asks gets a way to reach this
    /// connection's output, which is the same way the place reaches it and the
    /// same way a later borrower will; nothing is copied and nothing moves.
    fn home(&self) -> Option<Arc<PrivateOrderedHome>> {
        let held = self.owner.records_even_if_poisoned();
        match held.continuations.get(self.index) {
            Some(PrivateOrderedContinuationPlace::Taken(home))
                if std::ptr::eq(Arc::as_ptr(home), self.record.as_ptr()) =>
            {
                Some(home.clone())
            }
            _ => None,
        }
    }

    /// Whether the place still holds the record this lease was made for.
    fn holds(&self, held: &AbandonedSettlements) -> bool {
        matches!(
            held.continuations.get(self.index),
            Some(PrivateOrderedContinuationPlace::Taken(home))
                if std::ptr::eq(Arc::as_ptr(home), self.record.as_ptr())
        )
    }

    /// Give the place back, for a connection that was never exposed.
    ///
    /// A RESERVATION THAT PUBLISHED NOTHING IS NOT RETAINED WORK. Nothing could
    /// have been accepted for a connection with no row and no reachable queue,
    /// so there is nothing to account for and the capacity is free. Holding it
    /// would spend a place on a connection that never existed.
    ///
    /// Only for a KNOWN pre-publication failure. A disposition that is merely
    /// unknown keeps its place.
    fn relinquish_unexposed(self) {
        self.give_back();
    }

    /// Give the place back, for a connection that finished owing nothing.
    ///
    /// Only for that. A slot returned while its owner still holds unanswered
    /// admissions, foreign capsules or a wire whose ending was never
    /// established would be capacity handed out against work that still
    /// exists.
    fn finish(self) {
        self.give_back();
    }

    fn give_back(mut self) {
        let mut held = self.owner.records_even_if_poisoned();
        // The place is checked, NOT the record's contents: reading a record
        // here would take one beneath the aggregate, which is the order
        // driving relies on being the other way round.
        //
        // AND IT IS CHECKED FOR REAL, not asserted. Freeing a place that has
        // moved on takes it from whoever holds it now, and a check that only
        // exists in builds with debug assertions is not a check.
        if self.holds(&held) {
            held.continuation_slots = held.continuation_slots.saturating_sub(1);
            held.continuations[self.index] = PrivateOrderedContinuationPlace::Free;
            // AND THE DESTINATION RESERVED WITH IT. It was this connection's
            // and there is no obligation left to commit into it.
            PrivateSettlementOwner::release_maintenance_destination(&mut held, self.index);
        }
        self.armed = false;
    }
}
