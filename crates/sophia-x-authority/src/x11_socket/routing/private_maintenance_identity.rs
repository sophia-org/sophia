// What names one connection's obligation, and how it is resolved.
//
// Split from the reservation by subject: reserving is capacity and storage,
// and this is the name that outlives the connection which was given it.

/// What an identity found when it went to its place.
///
/// THREE ANSWERS, AND NONE OF THEM IS "SETTLED". A store that has gone and a
/// place that has moved on are different facts about different things, and
/// neither says that what this connection owed was finished. Whether the home
/// holds anything is asked of the home and is not a refusal at all -- a
/// connection that never bound still had a place, and still has a name.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Resolved by a commitment not attached yet.
enum PrivateMaintenanceReach<R> {
    /// The place still holds this identity's own home, and here is what the
    /// act returned.
    Reached(R),
    /// The store has gone, and the place went with it.
    StoreGone,
    /// This is not the current occupant of that place.
    ///
    /// The place may be free, or it may have been handed to a successor. A
    /// caller told either of those had a name that was still current would go
    /// on to act on somebody else's connection.
    Stale,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Resolved by a commitment not attached yet.
impl<R> PrivateMaintenanceReach<R> {
    fn reached(self) -> Option<R> {
        match self {
            Self::Reached(value) => Some(value),
            Self::StoreGone | Self::Stale => None,
        }
    }
}

/// The name one connection's obligation has.
///
/// MINTED WITH THE RESERVATION, BEFORE THE CONNECTION IS PUBLISHED, and handed
/// out from it rather than assembled: a name built from an index, a home and a
/// store that a caller happened to have could name a place those three never
/// agreed about.
///
/// IT NAMES AN OCCUPANT, NOT A NUMBER. A place is returned when what was in it
/// is finished and the next connection to reserve one takes the same index, so
/// a name that carried only the number would go on naming whatever arrived
/// next. The home this reservation made is what it carries, weakly: the weak
/// reference keeps that allocation's address from being handed to anything
/// else, so the comparison stays exact for as long as the name exists, and a
/// successor -- which gets a home of its own -- is a different occupant.
///
/// INERT. It says which obligation this is. It commits nothing, authorises
/// nobody to drive anything, holds no capacity of its own, and has no disposal
/// to perform: it is not a lease, and dropping one returns and abandons
/// nothing.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Resolved by a commitment not attached yet.
#[derive(Clone)]
struct PrivateMaintenanceIdentity {
    /// The store this place is in, held the only way a name may.
    owner: PrivateSettlementRef,
    /// Which place, which is half of the answer.
    index: usize,
    /// Whose place, which is the other half.
    home: std::sync::Weak<PrivateOrderedHome>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Resolved by a commitment not attached yet.
impl PrivateMaintenanceIdentity {
    /// Which place this names.
    fn place(&self) -> usize {
        self.index
    }

    /// Whether this names the same connection's obligation as another.
    fn same_as(&self, other: &Self) -> bool {
        self.index == other.index
            && std::ptr::eq(self.home.as_ptr(), other.home.as_ptr())
            && self.owner.names_same_store(&other.owner)
    }

    /// Act on the home this identity names, if it is still the one there.
    ///
    /// THE OCCUPANT IS CHECKED AND THE HOME IS PINNED IN ONE ACQUISITION of
    /// the store. Checking and then looking the number up again would leave an
    /// interval in which the place is returned and taken by a successor, and
    /// the act would run against that connection's home on the strength of a
    /// check that passed for a different one.
    ///
    /// TWO DIFFERENT THINGS, and they go opposite ways. The upgraded store
    /// OWNER is held for the whole act: a name holds its store weakly, so an
    /// operation that let that go would be acting on a home whose store could
    /// disappear underneath it. The store's AGGREGATE GUARD is released before
    /// the act runs: keeping it would put every other connection behind
    /// whatever this does with the home.
    ///
    /// AND IT ESTABLISHES REACHABILITY, NOTHING MORE. That this place was this
    /// connection's at the moment it was looked at is not a licence to visit
    /// the payload, and not a promise that it still will be afterwards.
    /// Anything that goes on to change something will need to ask again.
    fn with_home<R>(
        &self,
        act: impl FnOnce(&Arc<PrivateOrderedHome>) -> R,
    ) -> PrivateMaintenanceReach<R> {
        let Some(owner) = self.owner.owner() else {
            return PrivateMaintenanceReach::StoreGone;
        };
        let found = {
            let held = owner.records_even_if_poisoned();
            match held.continuations.get(self.index) {
                Some(PrivateOrderedContinuationPlace::Taken(home))
                    if std::ptr::eq(Arc::as_ptr(home), self.home.as_ptr()) =>
                {
                    Some(home.clone())
                }
                _ => None,
            }
            // The aggregate goes here, before anything is done with the home.
        };
        let Some(home) = found else {
            return PrivateMaintenanceReach::Stale;
        };
        PrivateMaintenanceReach::Reached(act(&home))
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a commitment not attached yet.
impl PrivateSettlementRef {
    /// Whether two handles name the same store, without keeping it alive.
    fn names_same_store(&self, other: &Self) -> bool {
        std::ptr::eq(self.inner.as_ptr(), other.inner.as_ptr())
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a caller not attached yet.
impl PrivateOrderedContinuationSlot {
    /// The name of this connection's obligation.
    ///
    /// THE SAME ONE EVERY TIME, because it is made of what the reservation
    /// already fixed: this store, this place and the home reserved with it.
    /// Nothing here reserves anything; the destination was set aside when the
    /// place was.
    fn maintenance_identity(&self) -> PrivateMaintenanceIdentity {
        PrivateMaintenanceIdentity {
            owner: self.owner.settlement_ref(),
            index: self.index,
            home: self.record.clone(),
        }
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a caller not attached yet.
impl PrivateInternalCredit {
    /// The same name, after this connection's place became the store's own
    /// responsibility.
    ///
    /// A CONVERSION DOES NOT RENAME AN OBLIGATION. It changes who is
    /// responsible for the place, not which connection's work is in it, and
    /// the identity is built from the same store, the same place and the same
    /// home either side of it.
    fn maintenance_identity(&self) -> PrivateMaintenanceIdentity {
        PrivateMaintenanceIdentity {
            owner: self.owner.clone(),
            index: self.index,
            home: self.record.clone(),
        }
    }
}
