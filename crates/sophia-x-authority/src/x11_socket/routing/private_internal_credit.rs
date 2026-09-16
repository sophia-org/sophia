// A place-reference the store itself keeps, and the conversion that makes one.
//
// Split from the reservation by subject: a lease held OUTSIDE the store and a
// credit held INSIDE it are different shapes with different lifetime rules,
// and the step between them is a transaction of its own.

/// A place in the store, named by a holder the store itself keeps.
///
/// NO STRONG ROUTE BACK, AND THAT IS THE WHOLE POINT. This lives inside the
/// store -- inside a holder that is one of the store's own entries -- so an
/// owner here would close a ring with itself: store, holder, credit, store.
/// Nothing would ever drop, and the obligations inside would stay readable for
/// ever, which reads as a store still settling rather than as a leak.
///
/// A CREDIT IS NOT CUSTODY. The work this names is in the place, which is in
/// the same store; the credit carries a name, not a payload. That is why a
/// store that has gone costs this nothing to say: the place went with it, and
/// so did everything in it. An external lease may not reason that way, because
/// what it holds at teardown is work the store has not got yet.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Named by a holder whose driver is not attached yet.
struct PrivateInternalCredit {
    /// The store this place is in, held the only way an inside holder may.
    owner: PrivateSettlementRef,
    /// Which place. The same index the external lease named: a conversion
    /// carries the place across, it does not take another one.
    index: usize,
    /// WHOSE PLACE, not merely which one.
    ///
    /// An index is not an identity. A place is returned when the work in it is
    /// settled and handed to the next connection that reserves one, so a
    /// credit that named only a number would, from that moment, be naming
    /// somebody else's connection -- reading its queue, and freeing its place
    /// out from under it. The record this credit was made for is held here and
    /// compared against what is in the place at every use, return and
    /// abandonment. It is the same comparison the settlement's own return path
    /// makes, for the same reason.
    ///
    /// AN IDENTITY, NOT CUSTODY. Weak, because a credit carries a name and
    /// the work itself stays in the place: holding the record strongly would
    /// make a holder keep retained work alive after the store that was
    /// responsible for it had gone, which is the opposite of what this whole
    /// component is for. It is enough for the comparison -- a weak handle
    /// keeps the allocation alive, so its address stays this record's and
    /// nothing else can be allotted it while this credit exists.
    record: std::sync::Weak<Mutex<Option<PrivateOrderedContinuation>>>,
    /// Whether this credit still has to be disposed of.
    ///
    /// Cleared by whichever disposal actually happens, so a credit cannot
    /// release and then be counted abandoned, or be counted twice.
    armed: bool,
}

/// What a credit found when it went to its place.
///
/// FOUR ANSWERS, NOT ONE ABSENCE. A store that has gone, a place that has
/// moved on to another connection and a record with nothing in it are
/// different facts about different things, and a caller acts on them
/// differently. Collapsing them into `None` would let "this is not mine any
/// more" read as "there is nothing to do".
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a driver that is not attached yet.
enum PrivateCreditReach<R> {
    /// The place held this credit's record, and it held a continuation.
    Reached(R),
    /// This credit's record is in its place and holds nothing.
    Empty,
    /// The place is not this credit's any more. Nothing was touched.
    Moved,
    /// The store has gone, and the place went with it.
    StoreGone,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a driver that is not attached yet.
impl<R> PrivateCreditReach<R> {
    /// What the act returned, if it ran at all.
    ///
    /// For a caller that has already decided the other three answers mean the
    /// same thing to it. Kept separate from the enum so that decision is made
    /// where it is made, rather than by the shape of what is returned.
    fn reached(self) -> Option<R> {
        match self {
            Self::Reached(value) => Some(value),
            Self::Empty | Self::Moved | Self::StoreGone => None,
        }
    }
}

/// What a credit's release did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a driver that is not attached yet.
#[must_use]
enum PrivateCreditRelease {
    /// The place is free again, and this credit is disposed of.
    Released,
    /// The record still owes work. Nothing is freed and nothing is destroyed.
    StillOwed,
    /// The place holds nothing yet: the hand-over it was reserved for has not
    /// happened. Nothing is freed -- work that is still on its way would lose
    /// its destination.
    NotHandedOver,
    /// The place is not this credit's any more, so it is not this credit's to
    /// free. Nothing was touched.
    NotOurs,
    /// The store has gone, and so has the place and everything in it.
    StoreGone,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Named by a holder whose driver is not attached yet.
impl PrivateInternalCredit {
    /// Which place this credit names.
    fn place(&self) -> usize {
        self.index
    }

    /// Whether the place still holds the record this credit was made for.
    ///
    /// Asked under the store, and about the PLACE only: reading the record
    /// here would take one beneath the aggregate, which is the order the
    /// drive relies on being the other way round.
    fn ours(&self, held: &AbandonedSettlements) -> bool {
        matches!(
            held.continuations.get(self.index),
            Some(PrivateOrderedContinuationPlace::Taken(record))
                if std::ptr::eq(Arc::as_ptr(record), self.record.as_ptr())
        )
    }

    /// Act on the continuation in the place this credit names.
    ///
    /// THE UPGRADED OWNER IS PINNED FOR THE WHOLE OPERATION. A credit holds no
    /// owner, so every operation begins by upgrading; a lookup that clones the
    /// record handle and lets the owner go before acting leaves an interval in
    /// which the last outside holder can drop. The record survives in the
    /// clone, so the operation would finish and report success into storage
    /// nobody can reach -- the failure looks exactly like the success. The
    /// owner is a local here, so it outlives the borrow, the act and the
    /// return. (The pin is the local, not the absence of a chain: a temporary
    /// in a method chain also lives to the end of its statement.)
    fn with_place<R>(
        &self,
        act: impl FnOnce(&mut PrivateOrderedContinuation) -> R,
    ) -> PrivateCreditReach<R> {
        let Some(owner) = self.owner.owner() else {
            return PrivateCreditReach::StoreGone;
        };
        // Upgraded under the store, where the place is holding it: a record
        // that is in its place is there to be taken hold of, and finding
        // otherwise would mean the place did not hold what it says it does.
        let Some(record) = ({
            let held = owner.records_even_if_poisoned();
            if !self.ours(&held) {
                return PrivateCreditReach::Moved;
            }
            self.record.upgrade()
        }) else {
            return PrivateCreditReach::Moved;
        };
        let mut destination = record
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match destination.as_mut() {
            Some(continuation) => PrivateCreditReach::Reached(act(continuation)),
            None => PrivateCreditReach::Empty,
        }
    }

    /// Give the place back, if there is nothing left owed in it.
    ///
    /// IT ASKS RATHER THAN TRUSTING ITS CALLER. The external lease's `finish`
    /// documents this as a precondition and leaves it to whoever calls; here
    /// the record is right there and the question is one lock away, and a
    /// place freed over a queue that still holds capsules destroys them
    /// unanswered -- which is the failure the precondition was supposed to
    /// prevent. So it is checked, and a caller that was wrong gets its place
    /// back untouched instead of a silent loss.
    ///
    /// AN EMPTY RECORD IS NOT A FINISHED ONE. A place whose record holds
    /// nothing is a hand-over that has not happened -- and one of the moments
    /// it has not happened yet is between this credit being published and the
    /// work arriving in the place it names. Reading that as "nothing owed"
    /// frees the place out from under work that is still on its way, and the
    /// next connection to reserve one takes it. So it is refused, and the
    /// credit says which of the two it is rather than making them one answer.
    ///
    /// THE CHECK AND THE FREE ARE NOT ONE STEP, and they cannot be: settledness
    /// is read under the record and the place is freed under the store, and
    /// this file takes those in that order everywhere. The identity is checked
    /// again under the store before anything is freed, so what a gap could cost
    /// is a release refused or a place already moved on -- not another
    /// connection's place freed.
    fn release(&mut self) -> PrivateCreditRelease {
        let Some(owner) = self.owner.owner() else {
            // The store has gone, so the place and everything in it went with
            // it. There is no capacity left to return.
            self.armed = false;
            return PrivateCreditRelease::StoreGone;
        };
        let Some(record) = ({
            let held = owner.records_even_if_poisoned();
            if !self.ours(&held) {
                return PrivateCreditRelease::NotOurs;
            }
            self.record.upgrade()
        }) else {
            return PrivateCreditRelease::NotOurs;
        };
        let standing = {
            let destination = record
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match destination.as_ref() {
                None => PrivateCreditRelease::NotHandedOver,
                Some(continuation) if continuation.settled() => PrivateCreditRelease::Released,
                Some(_) => PrivateCreditRelease::StillOwed,
            }
        };
        if !matches!(standing, PrivateCreditRelease::Released) {
            return standing;
        }
        let mut held = owner.records_even_if_poisoned();
        if !self.ours(&held) {
            return PrivateCreditRelease::NotOurs;
        }
        held.continuation_slots = held.continuation_slots.saturating_sub(1);
        held.continuations[self.index] = PrivateOrderedContinuationPlace::Free;
        self.armed = false;
        PrivateCreditRelease::Released
    }
}

#[cfg(unix)]
impl Drop for PrivateInternalCredit {
    /// A credit dropped over a place that is still its own marks it abandoned.
    ///
    /// The same rule the external lease keeps, for the same reason: the place
    /// holds work nobody accounted for, and handing the capacity out again
    /// would promise it against that work.
    ///
    /// CHECKED, LIKE EVERY OTHER DISPOSAL HERE. A credit whose place was
    /// returned by the drive that settled it owes nothing, and marking then
    /// would count an abandonment against a connection that finished -- or
    /// against whoever holds the place now.
    ///
    /// NOT DURING THE STORE'S OWN TEARDOWN. When the store is dropping, this
    /// credit is inside it and the upgrade fails, so nothing is marked. There
    /// is no account left to mark and no reader left to read it -- and no work
    /// is lost by saying so, because the place and its contents are going the
    /// same way.
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Some(owner) = self.owner.owner() else {
            return;
        };
        let mut held = owner.records_even_if_poisoned();
        if !self.ours(&held) {
            return;
        }
        held.continuations_abandoned = held.continuations_abandoned.saturating_add(1);
    }
}

/// A holder the store keeps, responsible for one of the store's own places.
///
/// This is the entry a ring would run through -- store, holder, credit -- and
/// the credit is where it is stopped. Nothing else about what a holder does is
/// decided here; what is decided is that having one cannot keep the store up.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Driven by a maintenance driver that is not attached yet.
struct PrivateStoreOwnedHolder {
    /// The place this holder is responsible for finishing.
    credit: PrivateInternalCredit,
}

/// A place in the store's holder storage.
#[cfg(unix)]
enum PrivateHolderPlace {
    /// Nobody's.
    Free,
    /// Promised to a preparation that has not committed, for one named place.
    ///
    /// CARRIES WHICH PLACE. A holder exists to be responsible for a particular
    /// place, so two promises against the same live place are two holders for
    /// one place, and whichever committed second would leave the first naming
    /// something it does not own. Distinct from free so two preparations
    /// cannot share a destination, and distinct from taken so a destination
    /// dropped before it commits is released rather than read as a holder.
    Promised(usize),
    /// A holder the store keeps.
    Taken(PrivateStoreOwnedHolder),
}

/// Why a holder place was not set aside.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller that is not attached yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateHolderRefusal {
    /// No holder place free. Every place already has one, promised or made.
    Saturated,
    /// The store could not be read.
    Unavailable,
    /// This place already has a holder, or a promise of one.
    ///
    /// Not a capacity answer: retrying changes nothing until whoever holds
    /// that one is done with it.
    AlreadyHeld,
    /// The lease is not this store's.
    Foreign,
}

/// A holder place set aside for one named place, before anything is put in it.
///
/// PREPARATION IS THE FALLIBLE HALF. The bound, an unreadable store, a place
/// that already has a holder and the storage all happen here, while the
/// connection still holds its external lease and its work is still in its own
/// slot. A preparation that refuses has touched neither.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Committed by a conversion whose caller is not attached yet.
struct PrivateHolderDestination {
    /// The store, held as an owner.
    ///
    /// EXTERNAL AND SHORT-LIVED, like the lease it is prepared alongside: this
    /// exists in a caller's frame between preparation and commitment, and
    /// nothing inside the store reaches it. It is on no ring.
    owner: PrivateSettlementOwner,
    index: usize,
    /// The place this destination was prepared for. A conversion that arrived
    /// with a lease on some other place would be using a promise made for
    /// this one.
    for_place: usize,
    /// AND WHOSE RESERVATION IT WAS. The number goes back with the place and
    /// is handed to the next connection, so a promise that named only the
    /// number could be committed against a successor's lease -- a holder over
    /// a connection nobody prepared one for. The record is the identity, and
    /// it is compared at commitment.
    for_record: std::sync::Weak<Mutex<Option<PrivateOrderedContinuation>>>,
    /// Whether this destination still has to be released.
    armed: bool,
}

#[cfg(unix)]
impl Drop for PrivateHolderDestination {
    /// A destination that never committed gives its place back.
    ///
    /// Nothing was ever put in it -- a promised place holds no holder and no
    /// credit -- so there is no work to account for and the capacity is free.
    /// This is the refusal path's other half: a preparation that is not used
    /// must not spend a holder place, or leave a place looking as though it
    /// already had a holder, on a holder that was never made.
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut held = self.owner.records_even_if_poisoned();
        if matches!(
            held.holders.get(self.index),
            Some(PrivateHolderPlace::Promised(_))
        ) {
            held.holders[self.index] = PrivateHolderPlace::Free;
            held.holders_taken = held.holders_taken.saturating_sub(1);
        }
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Prepared by a conversion whose caller is not attached yet.
impl PrivateSettlementOwner {
    /// Whether two handles name the same store.
    fn is_same_store(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Set aside a place for a holder responsible for this lease's place.
    ///
    /// BOUNDED BY THE PLACES THEMSELVES. A holder exists to be responsible for
    /// one place, so there can never be more of them than there are places,
    /// and the bound is the connection bound rather than a pool of its own. A
    /// store told no connection bound has no places, and therefore no holders.
    ///
    /// TIED TO THE LEASE'S PLACE HERE, not at commitment. A promise that named
    /// no place could be committed against any lease, and two of them against
    /// the same live place would leave one holder naming something another
    /// holder owns.
    ///
    /// The storage is grown here rather than at commitment, so what follows is
    /// an assignment into a place that already exists. That is a matter of
    /// where the allocation happens, not a refusal this can report: growing a
    /// `Vec` aborts rather than returning, and nothing here can catch that.
    fn prepare_internal_holder(
        &self,
        lease: &PrivateOrderedContinuationSlot,
    ) -> Result<PrivateHolderDestination, PrivateHolderRefusal> {
        if !self.is_same_store(&lease.owner) {
            return Err(PrivateHolderRefusal::Foreign);
        }
        let Ok(mut held) = self.inner.lock() else {
            return Err(PrivateHolderRefusal::Unavailable);
        };
        // ASKED FIRST, because it is the answer that is true regardless of
        // room: a place that already has a holder does not acquire one by
        // capacity appearing.
        let for_place = lease.index;
        if held.holders.iter().any(|place| match place {
            PrivateHolderPlace::Free => false,
            PrivateHolderPlace::Promised(promised) => *promised == for_place,
            PrivateHolderPlace::Taken(holder) => holder.credit.index == for_place,
        }) {
            return Err(PrivateHolderRefusal::AlreadyHeld);
        }
        // A BACKSTOP, AND IT SAYS SO. One holder per place and one place per
        // holder makes this unreachable from here: every promise and every
        // holder names a distinct place, so there cannot be more of them than
        // there are places, and a lease exists only because a place does. It
        // is kept because the invariant it rests on lives in two methods and
        // a caller should be refused rather than given a place off the end if
        // they ever part company.
        if !held.continuation_bound_declared || held.holders_taken >= held.continuation_capacity {
            return Err(PrivateHolderRefusal::Saturated);
        }
        let index = match held
            .holders
            .iter()
            .position(|place| matches!(place, PrivateHolderPlace::Free))
        {
            Some(index) => index,
            None => {
                held.holders.push(PrivateHolderPlace::Free);
                held.holders.len() - 1
            }
        };
        held.holders[index] = PrivateHolderPlace::Promised(for_place);
        held.holders_taken = held.holders_taken.saturating_add(1);
        drop(held);
        Ok(PrivateHolderDestination {
            owner: self.clone(),
            index,
            for_place,
            for_record: lease.record.clone(),
            armed: true,
        })
    }

    /// How many holder places are taken, promised and filled together.
    fn holders_taken(&self) -> Option<usize> {
        self.inner.lock().ok().map(|held| held.holders_taken)
    }

    /// Take a holder out of the store, for whoever is going to drive it.
    ///
    /// TAKEN, NOT BORROWED, because a driver that has one is responsible for
    /// disposing of the place it names, and a borrow could not carry that.
    /// Leaves the holder place free: the holder is out, and the place it named
    /// is still accounted for by the credit that went with it.
    fn take_internal_holder(&self, index: usize) -> Option<PrivateStoreOwnedHolder> {
        let mut held = self.records_even_if_poisoned();
        if !matches!(held.holders.get(index), Some(PrivateHolderPlace::Taken(_))) {
            return None;
        }
        let taken = std::mem::replace(&mut held.holders[index], PrivateHolderPlace::Free);
        held.holders_taken = held.holders_taken.saturating_sub(1);
        let PrivateHolderPlace::Taken(holder) = taken else {
            unreachable!("just matched as taken")
        };
        Some(holder)
    }

    /// Retire the holder responsible for a place that has just been returned.
    ///
    /// CALLED BY WHOEVER FREES A PLACE, so a returned place does not leave a
    /// holder behind naming it. The holder is taken out under the store and
    /// dropped after the store is released: a credit's own disposal takes the
    /// store, and dropping one while it is held would be this thread waiting
    /// for itself.
    ///
    /// The credit inside checks its place as it goes, finds the record it was
    /// made for is no longer there, and accounts for nothing -- which is
    /// right, because the work it was responsible for was settled by whoever
    /// returned the place.
    fn retire_holder_for(
        held: &mut AbandonedSettlements,
        index: usize,
        record: &Arc<Mutex<Option<PrivateOrderedContinuation>>>,
    ) -> Option<PrivateStoreOwnedHolder> {
        let found = held.holders.iter().position(|place| match place {
            PrivateHolderPlace::Taken(holder) => {
                holder.credit.index == index
                    && std::ptr::eq(Arc::as_ptr(record), holder.credit.record.as_ptr())
            }
            _ => false,
        })?;
        let taken = std::mem::replace(&mut held.holders[found], PrivateHolderPlace::Free);
        held.holders_taken = held.holders_taken.saturating_sub(1);
        let PrivateHolderPlace::Taken(holder) = taken else {
            unreachable!("just matched as taken")
        };
        Some(holder)
    }
}

/// What a conversion did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller that is not attached yet.
#[must_use]
enum PrivateInternalConversion {
    /// The work is in the place, and the store keeps a holder naming it.
    Held,
    /// The hand-over did not install, so no holder was made.
    ///
    /// The holder place is released -- nothing is left in it -- and what the
    /// lease did about its own place is what it says here.
    NotInstalled(PrivateContinuationInstall),
    /// Refused before anything was touched. The lease comes back armed over
    /// the same place, and the source still holds its work.
    Foreign(PrivateOrderedContinuationSlot),
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Converted by a caller that is not attached yet.
impl PrivateOrderedContinuationSlot {
    /// Hand this connection's work over and leave the store holding the place.
    ///
    /// THE OUTER HOLDER IS A PARAMETER, and that is the point of it. The lease
    /// is what has been keeping the store alive; consuming it inside this call
    /// while nothing outside the call holds one means an unwind in here drops
    /// the last owner along with the frame, and takes the work that was just
    /// installed with it. Returning an owner cannot cover that -- a return
    /// value exists only once the call has succeeded. So the caller must
    /// already hold one, in its own frame, and hand it in.
    ///
    /// WHO THAT IS: whoever drives these holders. It is not the constructor's
    /// borrow and not an assumed caller lifetime; it is a live handle that
    /// outlives this call by construction.
    ///
    /// THE HOLDER IS WRITTEN BEFORE THE HAND-OVER. The other half of the same
    /// problem: the work must not be out of the caller's slot while the place
    /// it went into has nothing naming it. Written first, an unwind between
    /// the two leaves a holder over a record that is still empty and the work
    /// still in the caller's hands, which is recoverable; written second, the
    /// same unwind leaves installed work that nothing names.
    ///
    /// THE SAME PLACE AND THE SAME CREDIT CROSS. The index is carried, not
    /// re-taken: no capacity is charged for the conversion, none is freed, and
    /// the lease is not counted abandoned on the way through. What was one
    /// connection's reservation becomes the store's own responsibility, and it
    /// was continuously somebody's.
    fn convert_to_internal(
        mut self,
        outer: &PrivateSettlementOwner,
        mut destination: PrivateHolderDestination,
        source: &mut Option<PrivateOrderedContinuation>,
    ) -> PrivateInternalConversion {
        // CHECKED, NOT ASSERTED. A destination prepared against another store
        // names an index in that store's holders; committing it here would
        // write into whatever this store happens to have at that index, take
        // over another connection's promise, and leave the other store's
        // promise held for ever. A debug assertion says so only in a build
        // that has them, and the provenance of a place is not a thing to
        // establish only in testing.
        if !outer.is_same_store(&self.owner)
            || !destination.owner.is_same_store(&self.owner)
            || destination.for_place != self.index
            || !std::ptr::eq(destination.for_record.as_ptr(), self.record.as_ptr())
        {
            return PrivateInternalConversion::Foreign(self);
        }
        let index = self.index;
        let found = {
            let held = outer.records_even_if_poisoned();
            match held.continuations.get(index) {
                Some(PrivateOrderedContinuationPlace::Taken(record))
                    if std::ptr::eq(Arc::as_ptr(record), self.record.as_ptr()) =>
                {
                    Some(record.clone())
                }
                _ => None,
            }
            // THE GUARD ENDS HERE, before anything is delegated. The hand-over
            // takes this same lock, so calling it from inside this block would
            // be this thread waiting for itself.
        };
        let Some(record) = found else {
            // No place of this lease's to be responsible for. The hand-over
            // says the same thing and accounts for the lease as it always
            // would, now that the guard is gone.
            return PrivateInternalConversion::NotInstalled(self.install(source));
        };
        {
            let mut held = outer.records_even_if_poisoned();
            // THE PLACE IS FOUND BEFORE THE HOLDER IS BUILT, and found without
            // indexing. A credit's own Drop takes the store, so a credit that
            // comes into existence under this guard and is then dropped -- by
            // an index that was out of range, which is the shape a missing
            // provenance check leaves -- would be this thread waiting for
            // itself. Asking for the place first means nothing that can fail
            // happens once the credit exists.
            let Some(place) = held.holders.get_mut(destination.index) else {
                // Unreachable from the checks above, which is why it refuses
                // rather than asserting: a destination whose index this store
                // does not have is not one to commit, however it got here.
                drop(held);
                return PrivateInternalConversion::Foreign(self);
            };
            *place = PrivateHolderPlace::Taken(PrivateStoreOwnedHolder {
                credit: PrivateInternalCredit {
                    owner: outer.settlement_ref(),
                    index,
                    record: Arc::downgrade(&record),
                    armed: true,
                },
            });
            // THE DUTY MOVES HERE: two assignments, serialized under one
            // acquisition of the store. Not one write -- they are two -- but
            // nothing can observe the store between them, and neither can
            // fail.
            //
            // The order matters if they are ever separated. Publishing the
            // credit armed and then disarming the lease leaves both armed in
            // between, so one reserved place gets marked twice. Disarming
            // first and then publishing leaves neither armed, so a place with
            // work still owed against it gets marked by nobody. Held together,
            // there is exactly one holder of the duty at every instant.
            //
            // The lease is DISARMED, not disposed of: the place is not given
            // back and not counted abandoned, because it is not going
            // anywhere. It is the store's now.
            self.armed = false;
            destination.armed = false;
        }
        let installed = self.install(source);
        if installed != PrivateContinuationInstall::Installed {
            // Nothing was handed over, so there is nothing for a holder to be
            // responsible for. The credit is taken out and dropped STILL
            // ARMED, because it is what holds the duty now: the hand-over
            // above gave it up when the credit was published, and a refusal
            // does not hand it back.
            //
            // AND IT MAY NOT BE HERE TO TAKE. Between publication and this
            // line the holder is in the store and anything that can read the
            // store can take it; whoever has it then holds the duty and
            // discharges it when they drop it. Either way it is discharged
            // once, by whichever of them actually has it -- which is why this
            // does not disarm anything and does not mark anything itself.
            //
            // Dropped after the store is released: a credit's own disposal
            // takes the store, and dropping one under this guard would be
            // this thread waiting for itself.
            let retired = {
                let mut held = outer.records_even_if_poisoned();
                PrivateSettlementOwner::retire_holder_for(&mut held, index, &record)
            };
            drop(retired);
            return PrivateInternalConversion::NotInstalled(installed);
        }
        PrivateInternalConversion::Held
    }
}
