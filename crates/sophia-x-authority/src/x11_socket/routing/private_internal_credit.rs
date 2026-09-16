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
    /// Whether this credit still has to be disposed of.
    ///
    /// Cleared by whichever disposal actually happens, so a credit cannot
    /// release and then be counted abandoned, or be counted twice.
    armed: bool,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Named by a holder whose driver is not attached yet.
impl PrivateInternalCredit {
    /// Which place this credit names.
    fn place(&self) -> usize {
        self.index
    }

    /// Act on the retained continuation in the place this credit names.
    ///
    /// THE UPGRADED OWNER IS PINNED FOR THE WHOLE OPERATION. Upgrading to find
    /// the record and letting the owner go before acting leaves an interval in
    /// which the last outside holder can drop: the record is then an orphan,
    /// and an operation that completed against it would report success into
    /// storage nobody can reach. The owner is a local here, so it outlives the
    /// borrow, the act, and the return.
    ///
    /// `None` means the store has gone, which is also the place going. Nothing
    /// is owed and nothing was in flight -- the payload was never held here.
    fn with_place<R>(
        &self,
        act: impl FnOnce(&mut PrivateOrderedContinuation) -> R,
    ) -> Option<R> {
        // PINNED. Not `self.owner.owner()?.with_ordered_continuation(..)`,
        // which would drop the temporary at the end of the statement.
        let owner = self.owner.owner()?;
        owner.with_ordered_continuation(self.index, act)
    }

    /// Give the place back, for a holder that finished owing nothing.
    ///
    /// Only for that, and for the same reason the external lease says so: a
    /// place returned over work that still exists is capacity promised twice.
    fn relinquish(mut self) {
        let Some(owner) = self.owner.owner() else {
            // The store has gone, so the place has gone with it. There is no
            // capacity left to return and nothing left to return it to.
            self.armed = false;
            return;
        };
        let mut held = owner.records_even_if_poisoned();
        // The place is checked, NOT the record's contents, for the same reason
        // the external lease checks it that way: reading a record here would
        // take one beneath the aggregate, which inverts the order driving
        // relies on.
        debug_assert!(
            matches!(
                &held.continuations[self.index],
                PrivateOrderedContinuationPlace::Taken(_)
            ),
            "a released credit is still this credit's place"
        );
        held.continuation_slots = held.continuation_slots.saturating_sub(1);
        held.continuations[self.index] = PrivateOrderedContinuationPlace::Free;
        self.armed = false;
    }
}

#[cfg(unix)]
impl Drop for PrivateInternalCredit {
    /// A credit dropped without a disposal marks its place abandoned.
    ///
    /// The same rule the external lease keeps, for the same reason: the place
    /// holds work nobody accounted for, and handing the capacity out again
    /// would promise it against that work.
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
    /// Promised to a preparation that has not committed.
    ///
    /// Distinct from free, so two preparations cannot be given the same
    /// destination, and distinct from taken, so a destination that is dropped
    /// before it commits is released rather than read as a holder.
    Promised,
    /// A holder the store keeps.
    Taken(PrivateStoreOwnedHolder),
}

/// A holder place set aside, before anything is put in it.
///
/// PREPARATION IS THE FALLIBLE HALF. Everything that can refuse -- the bound,
/// an unreadable store, the allocation of the storage -- happens here, while
/// the connection still holds its external lease and its work is still where
/// it was. What follows is an assignment into storage that already exists.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Committed by a conversion whose caller is not attached yet.
struct PrivateHolderDestination {
    /// The store, held as an owner.
    ///
    /// EXTERNAL AND SHORT-LIVED, like the lease it is prepared alongside: this
    /// exists in a caller's frame between preparation and commitment, and
    /// nothing inside the store reaches it. It is on no ring, and it is what
    /// makes the store certainly still there when the conversion commits.
    owner: PrivateSettlementOwner,
    index: usize,
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
    /// must not spend a holder place on a holder that was never made.
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut held = self.owner.records_even_if_poisoned();
        if matches!(held.holders.get(self.index), Some(PrivateHolderPlace::Promised)) {
            held.holders[self.index] = PrivateHolderPlace::Free;
            held.holders_taken = held.holders_taken.saturating_sub(1);
        }
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Prepared by a conversion whose caller is not attached yet.
impl PrivateSettlementOwner {
    /// Set aside a place for a holder this store will keep.
    ///
    /// BOUNDED BY THE PLACES THEMSELVES. A holder exists to be responsible for
    /// one place, so there can never be more of them than there are places,
    /// and the bound is the connection bound rather than a pool of its own. A
    /// store told no connection bound has no places, and therefore no holders.
    fn prepare_internal_holder(&self) -> Result<PrivateHolderDestination, AdmissionRefusal> {
        // An unreachable store and a full one are different answers, as
        // everywhere else here.
        let Ok(mut held) = self.inner.lock() else {
            return Err(AdmissionRefusal::Unavailable);
        };
        if !held.continuation_bound_declared || held.holders_taken >= held.continuation_capacity {
            return Err(AdmissionRefusal::Saturated);
        }
        let index = match held
            .holders
            .iter()
            .position(|place| matches!(place, PrivateHolderPlace::Free))
        {
            Some(index) => index,
            None => {
                // The one allocation in the whole conversion, and it is here,
                // on the half that is allowed to fail.
                held.holders.push(PrivateHolderPlace::Free);
                held.holders.len() - 1
            }
        };
        held.holders[index] = PrivateHolderPlace::Promised;
        held.holders_taken = held.holders_taken.saturating_add(1);
        drop(held);
        Ok(PrivateHolderDestination {
            owner: self.clone(),
            index,
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
}

/// What a conversion did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller that is not attached yet.
#[must_use]
enum PrivateInternalConversion {
    /// The work is in the place, and the store keeps a holder naming it.
    ///
    /// CARRIES THE STORE. Whoever asked for this conversion is the outer
    /// holder from here: the lease that used to keep the store up is gone, and
    /// the holder that replaced it is inside the store and cannot.
    Held(PrivateSettlementOwner),
    /// The hand-over did not install, so no holder was made.
    ///
    /// The holder place is released -- nothing was put in it -- and what the
    /// lease did about its own place is what it says here.
    NotInstalled(PrivateContinuationInstall),
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Converted by a caller that is not attached yet.
impl PrivateOrderedContinuationSlot {
    /// Hand this connection's work over and leave the store holding the place.
    ///
    /// THE LEASE IS HELD UNTIL THE DESTINATION IS. Preparation is a separate,
    /// fallible step: the bound, an unreadable store and the one allocation
    /// all happen there, while this connection still holds its lease and its
    /// work is still in its own slot. A preparation that refuses leaves both
    /// exactly as they were, because it has not touched either.
    ///
    /// THE SAME PLACE AND THE SAME CREDIT CROSS. The index is carried, not
    /// re-taken: no capacity is charged for the conversion, none is freed, and
    /// the lease is not counted abandoned on the way through. What was one
    /// connection's reservation becomes the store's own responsibility, and it
    /// was continuously somebody's.
    ///
    /// NOT ONE ATOMIC STEP, and it does not claim to be. The hand-over takes
    /// the store and then the record; the holder is written under the store
    /// again afterwards. Between them the place is retained with no holder
    /// naming it -- which is what every retained place in this tree looks like
    /// today -- and an unwind in there leaves exactly that, with the holder
    /// place released. What the interval cannot do is lose the store: the
    /// owner is pinned in a local across both.
    fn convert_to_internal(
        self,
        mut destination: PrivateHolderDestination,
        source: &mut Option<PrivateOrderedContinuation>,
    ) -> PrivateInternalConversion {
        debug_assert!(
            Arc::ptr_eq(&self.owner.inner, &destination.owner.inner),
            "a place is converted into a holder of its own store"
        );
        // PINNED ACROSS THE WHOLE CONVERSION, and taken before the lease is
        // consumed, because the lease is what is holding the store up.
        let owner = self.owner.clone();
        let index = self.index;
        let installed = self.install(source);
        if installed != PrivateContinuationInstall::Installed {
            // The destination releases its place as it drops: nothing was put
            // in it, so there is no work to account for.
            return PrivateInternalConversion::NotInstalled(installed);
        }
        {
            let mut held = owner.records_even_if_poisoned();
            // Into storage that already exists. No allocation, no callback and
            // nothing fallible between here and the credit being in place.
            held.holders[destination.index] =
                PrivateHolderPlace::Taken(PrivateStoreOwnedHolder {
                    credit: PrivateInternalCredit {
                        owner: owner.settlement_ref(),
                        index,
                        armed: true,
                    },
                });
            destination.armed = false;
        }
        PrivateInternalConversion::Held(owner)
    }
}
