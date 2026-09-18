// A bounded, read-only reading of what one owner's custody inventory holds.
//
// IT READS AND DOES NOTHING ELSE. No handle is taken, no thread is joined, no
// place is retired and no publication right is claimed. Every state this can
// report is one that was already true before it looked, which is what makes it
// safe to call from a controller that is deciding whether a service has
// finished rather than making it finish.
//
// UNREADABLE IS A STATE, NOT AN ABSENCE. A slot whose lock is poisoned reads as
// its own answer rather than as "nobody was ever started here": those are
// opposite facts, and reporting the second for the first is how a controller
// concludes a connection never ran from the evidence that its worker died
// badly. The same distinction is why the durable start state is read rather
// than the handle -- an empty handle means the handle went to a joiner just as
// often as it means nothing was started.

/// What a custody place's worker slot says, or that it could not be asked.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateCustodyWorkerStanding {
    /// Nobody has ever been started in this place.
    NeverStarted,
    /// A worker exists and this slot still holds its handle.
    Running,
    /// A worker exists and its handle has gone to whoever joins it. What
    /// became of it afterwards belongs to the join, not to this slot.
    HandedToJoiner,
    /// The slot could not be read. NOT the same as `NeverStarted`.
    Unreadable,
}

/// What has been published into a custody place's join home.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateCustodyJoinStanding {
    /// Nothing has been published here.
    ///
    /// SAYS NOTHING ABOUT A WORKER. A place with no published result may never
    /// have started one, may be running one, or may have handed one to a
    /// joiner that has not finished. The worker standing beside this is what
    /// separates those.
    Unpublished,
    /// A join published its result here.
    Published(PrivateJoinKind),
}

/// One custody place, as it stood when it was read.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateCustodySnapshotRow {
    /// Which place in the owner's inventory this is.
    pub place: usize,
    pub worker: PrivateCustodyWorkerStanding,
    /// Whether the slot still holds a handle, when the slot could be read.
    ///
    /// BESIDE THE START STATE, NEVER INSTEAD OF IT. Presence here is not
    /// liveness and absence is not death; the start state says which.
    pub handle_present: Option<bool>,
    /// Whether this connection has been told to depart, when readable.
    pub departing: Option<bool>,
    pub join: PrivateCustodyJoinStanding,
    /// Whether the one right to publish a join result here is still unclaimed.
    ///
    /// AN UNCLAIMED RIGHT WITH NO RESULT means no attempt has begun. A claimed
    /// right with no result means an attempt holds it and has not published --
    /// which is not the same as an attempt that will.
    pub publication_right_unclaimed: bool,
}

/// What one owner's whole custody inventory holds.
///
/// BOUNDED BY THE INVENTORY, which is itself bounded by the store's declared
/// connection bound. There is no growth here and nothing accumulates: the row
/// count is the number of places that exist, decided when the owner was
/// established.
#[cfg(unix)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateCustodySnapshot {
    /// How many places this inventory was sized for.
    pub places: usize,
    /// How many of them hold a custody.
    pub taken: usize,
    /// Whether the inventory's own lock was poisoned when this was read.
    ///
    /// REPORTED RATHER THAN HIDDEN OR FATAL. The places are still there to be
    /// read, so refusing outright would discard custody information a
    /// controller needs; reading them silently would let a reader treat a
    /// damaged inventory as an intact one.
    pub inventory_poisoned: bool,
    /// One row per occupied place, in place order.
    pub rows: Vec<PrivateCustodySnapshotRow>,
}

/// Why a custody snapshot was refused.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateCustodySnapshotRefusal {
    /// The lease offered is not this owner's.
    ///
    /// Custody is not public reading: only the keeper of an inventory may be
    /// told what is in it.
    ForeignServiceOwner,
}

#[cfg(unix)]
impl PrivateServiceOwner {
    /// Read every occupied custody place, without touching any of them.
    ///
    /// NOT A COLLECTION, AND DELIBERATELY NOT NEAR ONE. `PrivateWorkerCollection`
    /// is what a reaping produced: it says whether *that* collection joined the
    /// thread, and it exists only because something took a handle and joined
    /// it. This is a reading of state that was already there. Keeping the two
    /// apart is the point -- a snapshot that could reap would make every
    /// observation a change, and a controller could no longer look at a service
    /// without altering what it was looking at.
    pub fn custody_snapshot(
        &self,
        service: &PrivateServiceLease<'_>,
    ) -> Result<PrivateCustodySnapshot, PrivateCustodySnapshotRefusal> {
        if !std::ptr::eq(
            Arc::as_ptr(&self.inventory),
            Arc::as_ptr(&service.owner.inventory),
        ) {
            return Err(PrivateCustodySnapshotRefusal::ForeignServiceOwner);
        }
        let (kept, inventory_poisoned) = match self.inventory.kept.lock() {
            Ok(kept) => (kept, false),
            // The places survive a poisoned inventory; the fact that it was
            // poisoned travels with the reading rather than replacing it.
            Err(poisoned) => (poisoned.into_inner(), true),
        };
        let places = kept.places.len();
        let taken = kept.taken;
        // THE INVENTORY LOCK IS RELEASED BEFORE ANY SLOT IS READ. Holding it
        // while reaching into each custody's own slot would nest two locks that
        // nothing else nests, and would block every admission and revocation on
        // whichever slot happened to be held. The occupied custodies are
        // cloned out -- a bounded list, since the inventory is sized to the
        // store's declared connection bound -- and read afterwards.
        //
        // Each row is therefore a snapshot of its own place rather than one
        // instant across all of them, which is what this is for: a controller
        // asking what each place holds, not a claim that they were all like
        // that at once.
        let found: Vec<(usize, Arc<PrivateEvidenceCustody>)> = kept
            .places
            .iter()
            .enumerate()
            .filter_map(|(place, held)| held.as_ref().map(|custody| (place, Arc::clone(custody))))
            .collect();
        drop(kept);
        let rows = found
            .into_iter()
            .map(|(place, custody)| custody.snapshot_row(place))
            .collect();
        Ok(PrivateCustodySnapshot {
            places,
            taken,
            inventory_poisoned,
            rows,
        })
    }
}

#[cfg(unix)]
impl PrivateEvidenceCustody {
    /// This place's standing, read and not disturbed.
    fn snapshot_row(&self, place: usize) -> PrivateCustodySnapshotRow {
        let (worker, handle_present, departing) = match self.source.slot.lock() {
            Ok(slot) => (
                match slot.life {
                    PrivateWorkerLife::NeverStarted => PrivateCustodyWorkerStanding::NeverStarted,
                    PrivateWorkerLife::Running => PrivateCustodyWorkerStanding::Running,
                    PrivateWorkerLife::HandedToJoiner => {
                        PrivateCustodyWorkerStanding::HandedToJoiner
                    }
                },
                Some(slot.handle.is_some()),
                Some(slot.departing),
            ),
            // NOT READ THROUGH. A poisoned worker slot is exactly the case this
            // distinction exists for: the durable start state inside it may be
            // mid-write, and reporting whatever is there as fact would make a
            // damaged slot indistinguishable from an untouched one.
            Err(_) => (PrivateCustodyWorkerStanding::Unreadable, None, None),
        };
        let join = match self.join.result.get() {
            None => PrivateCustodyJoinStanding::Unpublished,
            // The payload's own synchroniser is never taken: which of the two
            // it is can be read without it, and taking it would be a reader
            // reaching into evidence it does not own.
            Some(PrivateJoinResult::Returned) => {
                PrivateCustodyJoinStanding::Published(PrivateJoinKind::Returned)
            }
            Some(PrivateJoinResult::Panicked(_)) => {
                PrivateCustodyJoinStanding::Published(PrivateJoinKind::Panicked)
            }
        };
        PrivateCustodySnapshotRow {
            place,
            worker,
            handle_present,
            departing,
            join,
            publication_right_unclaimed: !self
                .join
                .producer
                .load(std::sync::atomic::Ordering::Acquire),
        }
    }
}
