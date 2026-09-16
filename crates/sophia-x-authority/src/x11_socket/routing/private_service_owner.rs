// Who owns a private service's durable store and its connections' evidence
// custodies, and how a service reaches one without becoming its keeper.
//
// Split from the custody itself by subject: a custody is one connection's
// home, and this is the inventory of them that exists before any service does
// and outlives every frame that borrows one.
//
// WHAT THIS IS NOT. No production launch, shutdown or error path holds one of
// these yet. Establishing an owner in a control establishes the contract the
// construction path enforces; it does not establish that the running server
// has acquired that lifetime, and nothing here should be read as saying so.

/// The bounded inventory of evidence custodies one owner keeps.
///
/// BOUNDED BY THE STORE'S DECLARED CONNECTION BOUND, not by a limit of its
/// own. A second number would be a second policy, and two policies over one
/// store disagree the moment an instance arrives with a different client
/// limit.
#[cfg(unix)]
struct PrivateCustodyInventory {
    /// The store every custody in here is about.
    ///
    /// HELD SO PROVENANCE IS STRUCTURAL. A custody made from whatever store a
    /// caller happened to pass would let one owner's inventory fill with
    /// another store's connections, and the name would still look well formed.
    store: PrivateSettlementOwner,
    kept: Mutex<PrivateKeptCustodies>,
}

#[cfg(unix)]
struct PrivateKeptCustodies {
    /// One place per connection the store's bound allows, made once.
    ///
    /// FIXED LENGTH. Growing this to keep admitting would be inventing
    /// capacity the store never declared, and a history that grows without
    /// limit is how an inventory becomes a leak with a tidy name.
    places: Vec<Option<Arc<PrivateEvidenceCustody>>>,
    /// How many of those places hold a custody.
    taken: usize,
}

/// What a reservation of an evidence custody found.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[must_use]
enum PrivateCustodyReserved {
    /// A custody for this connection exists and here is the way back to it.
    Reserved(PrivateRegisteredCustody),
    /// This connection already has one, and it is untouched.
    ///
    /// NOT AN ERROR ABOUT THE FIRST ONE. Preparing twice for one live
    /// reservation leaves the first home exactly as it was -- pending,
    /// published or unconfirmed alike -- because replacing it would discard
    /// evidence somebody may already be naming.
    AlreadyKept,
    /// Every place the declared bound allows holds a custody.
    ///
    /// OUTSTANDING ENTRIES COUNT. A connection whose place went back
    /// elsewhere, whose join completed, or whose number a successor now holds
    /// still has evidence here, and admitting over it would be overwriting
    /// that evidence rather than making room.
    Saturated,
    /// This name is not about this owner's store.
    Foreign,
    /// The inventory could not be read.
    Unreadable,
}

/// What reaching for a registered custody found.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
enum PrivateCustodyReach<'o> {
    /// The custody is here, pinned for the length of this act.
    Reached(PrivateCustodyPin<'o>),
    /// The owner that kept it has gone.
    ///
    /// SEPARATE FROM EVERY OTHER ANSWER, and it is not a fact about the join.
    /// It says the keeper this service was built over is no longer there --
    /// which is the outer owner's destruction, not this service's exit.
    KeeperGone,
    /// The lease offered is a different owner's.
    ///
    /// NOT A CLAIM THAT ANYTHING WAS DESTROYED. Both owners may be perfectly
    /// alive; what is wrong is the association, and a caller told the keeper
    /// had gone would go looking for a destruction that never happened.
    ForeignKeeper,
    /// The place holds something else now.
    Replaced,
}

/// One act's pin on a connection's custody.
///
/// A HANDLE, NOT A PERMISSION. Holding this says the custody is here for the
/// length of this act; it authorises nothing about the home, its standing, its
/// worker or its obligation.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Held by a caller no production site has yet.
struct PrivateCustodyPin<'o> {
    custody: Arc<PrivateEvidenceCustody>,
    /// THE OWNER THIS WAS REACHED THROUGH, borrowed for as long as this pin
    /// exists.
    ///
    /// AN OWNING HANDLE IS NOT AN ENFORCED ONE. Holding the `Arc` above keeps
    /// this custody alive wherever it came from, which is exactly how an
    /// operation could outlive the owner and become the last keeper of the
    /// evidence it published -- the opposite of what this component is for.
    /// The borrow is what makes that impossible to write.
    owner: std::marker::PhantomData<&'o PrivateServiceOwner>,
}

/// A pin reads as the custody it pins, because that is all it is.
///
/// IT IS STILL AN OWNING HANDLE, and that is worth saying because it is no
/// longer the thing that decides how long it lasts: a pin borrows the owner it
/// was reached through, so it cannot be held past one. What an owning handle
/// buys is that the custody itself cannot go while the pin is alive, which is
/// what makes borrowing through it sound.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Held by a caller no production site has yet.
impl std::ops::Deref for PrivateCustodyPin<'_> {
    type Target = PrivateEvidenceCustody;

    fn deref(&self) -> &Self::Target {
        &self.custody
    }
}

/// A registration's exact way back to the custody reserved for it.
///
/// A CAPABILITY, NOT A HOME. It names one custody in one owner's inventory and
/// cannot make another: a registration that could manufacture a publication
/// home later is one whose evidence keeper is decided after the connection is
/// already exposed, which is the whole thing this reservation exists to
/// prevent.
///
/// WEAK ON BOTH SIDES, and it has to be. The store keeps registries, so a
/// registration that owned its way back to the owner would put the owner on a
/// ring through its own store.
///
/// NO `Drop`. Losing a registration does not release its evidence: a service
/// that ended is not a connection whose work was disposed of, and an inventory
/// that emptied itself on teardown would make shutdown look complete by
/// discarding what it was keeping.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Held by a caller no production site has yet.
struct PrivateRegisteredCustody {
    inventory: std::sync::Weak<PrivateCustodyInventory>,
    index: usize,
    /// The exact custody, named weakly so this cannot keep it.
    custody: std::sync::Weak<PrivateEvidenceCustody>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Held by a caller no production site has yet.
impl PrivateRegisteredCustody {
    /// Pin the custody this names, for one act.
    ///
    /// IDENTITY IS CHECKED INSIDE THE ACQUISITION THAT SELECTS IT. Reading the
    /// place and then comparing afterwards would leave an interval in which
    /// the place changed hands, and the act would run against whatever arrived
    /// on the strength of a check that passed for something else.
    ///
    /// AND THE INVENTORY IS RELEASED BEFORE THE CALLER ACTS. Everything an act
    /// does with a custody -- joining, fencing, reading a payload -- would
    /// otherwise happen with every other connection's reservation behind it.
    ///
    /// THE IDENTITY CHECK IS DEFENCE IN DEPTH, AND NO CONTROL REACHES IT. An
    /// entry is only ever removed by the capability that names it, and doing
    /// so consumes that capability, so no surviving capability can be naming a
    /// place that something else has since taken. It is asked anyway because
    /// that argument is about how entries are released today, and the cost of
    /// it being wrong is one connection's operation publishing into another
    /// connection's home.
    fn pin<'o>(&self, service: &PrivateServiceLease<'o>) -> PrivateCustodyReach<'o> {
        // THE LEASE IS THE OWNER, BORROWED. Asking for it is what ties every
        // act that follows to an owner that is still there: a caller with no
        // live owner cannot produce one, and a caller holding what this
        // returns cannot let that owner go.
        if !service.keeps(&self.inventory) {
            return PrivateCustodyReach::ForeignKeeper;
        }
        let Some(inventory) = self.inventory.upgrade() else {
            return PrivateCustodyReach::KeeperGone;
        };
        let found = {
            let kept = match inventory.kept.lock() {
                Ok(kept) => kept,
                Err(poisoned) => poisoned.into_inner(),
            };
            match kept.places.get(self.index) {
                Some(Some(custody)) if std::ptr::eq(Arc::as_ptr(custody), self.custody.as_ptr()) => {
                    Some(Arc::clone(custody))
                }
                _ => None,
            }
            // The inventory goes here, before the caller has anything to act
            // with.
        };
        match found {
            Some(custody) => PrivateCustodyReach::Reached(PrivateCustodyPin {
                custody,
                owner: std::marker::PhantomData,
            }),
            None => PrivateCustodyReach::Replaced,
        }
    }

    /// Give back a reservation whose connection was never exposed.
    ///
    /// FOR THE UNPUBLISHED ATTEMPT ONLY, and it is not a disposition. A
    /// connection that never had a row never had work accepted for it, so its
    /// evidence place was set aside and never used. This removes exactly the
    /// entry this capability names, so a live sibling at another place -- or a
    /// successor at this one -- keeps its own.
    fn release_unexposed(self) {
        let Some(inventory) = self.inventory.upgrade() else {
            return;
        };
        let mut kept = match inventory.kept.lock() {
            Ok(kept) => kept,
            Err(poisoned) => poisoned.into_inner(),
        };
        let ours = matches!(
            kept.places.get(self.index),
            Some(Some(custody)) if std::ptr::eq(Arc::as_ptr(custody), self.custody.as_ptr())
        );
        if ours {
            kept.places[self.index] = None;
            kept.taken = kept.taken.saturating_sub(1);
        }
    }
}

/// A registry's way to reserve custodies from the owner that built it.
///
/// INSTALLED ONCE, LIKE THE STORE'S. A registry that could be given a second
/// keeper would be able to put this connection's evidence in one inventory and
/// that connection's in another, and no reader could tell which owner was
/// responsible for what.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Held by a caller no production site has yet.
#[derive(Clone)]
pub(crate) struct PrivateCustodyKeeper {
    inventory: std::sync::Weak<PrivateCustodyInventory>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a caller no production site has yet.
impl PrivateCustodyKeeper {
    /// Reserve this connection's evidence custody.
    ///
    /// BEFORE THE ROW IS PUBLISHED, which is the only time it can be done
    /// honestly. Once a row is in, work can be accepted for that connection,
    /// and a connection that discovers afterwards that its keeper has nowhere
    /// to put its evidence has already been told it was admitted.
    ///
    /// ALLOCATES OR REFUSES WITH EVERYTHING STILL WHERE IT WAS. The name is
    /// copied, nothing is consumed, and a refusal leaves the caller holding
    /// exactly what it held before.
    fn reserve_for(&self, identity: &PrivateMaintenanceIdentity) -> PrivateCustodyReserved {
        let Some(inventory) = self.inventory.upgrade() else {
            return PrivateCustodyReserved::Unreadable;
        };
        // PROVENANCE FIRST. A name from another store may be perfectly well
        // formed and still be nothing to do with this owner.
        if !identity
            .owner
            .names_same_store(&inventory.store.settlement_ref())
        {
            return PrivateCustodyReserved::Foreign;
        }
        let mut kept = match inventory.kept.lock() {
            Ok(kept) => kept,
            Err(poisoned) => poisoned.into_inner(),
        };
        if kept
            .places
            .iter()
            .flatten()
            .any(|custody| custody.identity().same_as(identity))
        {
            return PrivateCustodyReserved::AlreadyKept;
        }
        let Some(index) = kept.places.iter().position(Option::is_none) else {
            return PrivateCustodyReserved::Saturated;
        };
        // The home is made here, before this connection is exposed, and goes
        // into the owner's storage on the next line.
        //
        // THERE IS AN INTERVAL, AND IT IS NOT THE ONE THAT MATTERS. Between
        // this line and the assignment below, the only strong handle to this
        // custody is the local -- so an unwind in between would take it. What
        // it would take is an empty home belonging to a connection that has
        // not been published, whose worker does not exist and whose join has
        // not happened: there is no result to lose, and the caller is refused.
        // What the component rules out is losing a home somebody has already
        // published INTO, and by the time anything can, this home has an
        // owner that is not a frame.
        let custody = Arc::new(PrivateEvidenceCustody::prepared_for(
            &inventory.store,
            identity.clone(),
        ));
        let capability = PrivateRegisteredCustody {
            inventory: Arc::downgrade(&inventory),
            index,
            custody: Arc::downgrade(&custody),
        };
        kept.places[index] = Some(custody);
        kept.taken = kept.taken.saturating_add(1);
        PrivateCustodyReserved::Reserved(capability)
    }

    /// Whether this keeper is that owner's.
    fn kept_by(&self, owner: &PrivateServiceOwner) -> bool {
        std::ptr::eq(self.inventory.as_ptr(), Arc::as_ptr(&owner.inventory))
    }
}

/// One live borrow of a service's owner.
///
/// WHAT IT IS FOR. Every act that continues a service -- taking an ingress,
/// serving a turn, reaching a connection's custody -- asks for one of these,
/// and the only way to have one is to borrow an owner that is still there.
/// That is the difference between a service whose keeper is alive and a
/// service that merely remembers having had one.
///
/// AND IT IS THIS OWNER, NOT AN OWNER. Two owners over one store are two
/// inventories; a lease on the wrong one proves the wrong thing, so every act
/// that takes one compares it with the inventory it is about to use. That
/// comparison failing says the association is wrong, NOT that anything was
/// destroyed: both owners may be perfectly alive.
///
/// WHAT IT BOUNDS IS THE ACT, NOT WHAT THE ACT FOUND. A pin taken through a
/// lease cannot outlive the owner, because pinning is reaching into that
/// owner's inventory. An owning handle on the EVIDENCE is a different thing
/// and deliberately outlives all of it: a reader that keeps one goes on
/// reading the result after the service, the operation and the keeper have
/// all gone.
///
/// NOTHING IS OWNED HERE, and holding one authorises nothing by itself: it
/// says the keeper is there, not that anything may be started, joined, driven
/// or disposed of.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Held by a caller no production site has yet.
#[derive(Clone, Copy)]
pub struct PrivateServiceLease<'o> {
    owner: &'o PrivateServiceOwner,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a caller no production site has yet.
impl PrivateServiceLease<'_> {
    /// Whether this lease's owner is the one keeping that inventory.
    fn keeps(&self, inventory: &std::sync::Weak<PrivateCustodyInventory>) -> bool {
        std::ptr::eq(inventory.as_ptr(), Arc::as_ptr(&self.owner.inventory))
    }

    /// Whether this lease's owner is the one that registry reserves through.
    fn keeps_for(&self, keeper: &PrivateCustodyKeeper) -> bool {
        self.keeps(&keeper.inventory)
    }
}

/// The owner of a private service's store and its connections' evidence.
///
/// ESTABLISHED BEFORE THE SERVICE, AND OUTSIDE IT. It holds the store and the
/// custody inventory in its own fields; a frontend, a runner and every
/// operation frame borrow from it and none of them can become the keeper by
/// returning, failing or unwinding.
///
/// IT IS NOT REACHABLE FROM THE STORE. The store keeps registries and terminal
/// inventories, so everything pointing back this way is weak: a registry's
/// keeper, a registration's capability and a committed obligation's name
/// alike. What this owns strongly runs one way only -- owner, inventory,
/// custody, store -- and a panic payload holding a store handle makes a chain
/// along it rather than a ring through it.
///
/// WHAT IT DOES NOT DO. Constructing one starts nothing, drives nothing, joins
/// nothing and authorises no turn or shutdown work. Destroying one is a
/// separate event from a service exiting, and is the only thing that releases
/// what is kept here.
#[cfg(unix)]
pub struct PrivateServiceOwner {
    store: PrivateSettlementOwner,
    inventory: Arc<PrivateCustodyInventory>,
}

#[cfg(unix)]
impl PrivateServiceOwner {
    /// Establish an owner over this store, for this many connections.
    ///
    /// THE BOUND IS THE STORE'S, NOT THIS CALLER'S. The number asked for is
    /// declared to the store, and what comes back is the bound in force --
    /// which for a store that already carries places from an earlier instance
    /// is the number those were taken against. The inventory is sized to THAT,
    /// so there is one limit here and not two that can disagree.
    ///
    /// TWO REFUSALS, AND BOTH ARE ABOUT ESTABLISHING, NOT ABOUT ANY
    /// CONNECTION. A store whose bound cannot be read has nothing to size an
    /// inventory to; and the storage for that many places is asked for
    /// up front, so an allocation that will not be made is refused here rather
    /// than found later by a connection that was already admitted.
    pub fn established_over(
        store: &PrivateSettlementOwner,
        connections: NonZeroUsize,
    ) -> Option<Self> {
        let bound = store.declare_connection_bound(connections)?;
        let mut places = Vec::new();
        places.try_reserve_exact(bound).ok()?;
        places.resize_with(bound, || None);
        Some(Self {
            store: store.clone(),
            inventory: Arc::new(PrivateCustodyInventory {
                store: store.clone(),
                kept: Mutex::new(PrivateKeptCustodies { places, taken: 0 }),
            }),
        })
    }

    /// The store this owner keeps.
    pub(crate) fn store(&self) -> &PrivateSettlementOwner {
        &self.store
    }

    /// A live borrow of this owner, for the acts that require one.
    ///
    /// THE OWNER IS THE SCOPE. A caller holding one of these is holding this
    /// owner borrowed, so the ACT it is for, and any pin taken through it,
    /// cannot outlive the keeper they came from.
    ///
    /// AN OWNING EVIDENCE HANDLE IS NOT ONE OF THOSE, and deliberately so. A
    /// reader that keeps an `Arc` on a join's evidence goes on reading that
    /// result after the act, the service and this owner have all gone. What a
    /// lease bounds is reaching, not what reaching found.
    pub fn lease(&self) -> PrivateServiceLease<'_> {
        PrivateServiceLease { owner: self }
    }

    /// A registry's way back to this owner's inventory, held weakly.
    pub(crate) fn keeper(&self) -> PrivateCustodyKeeper {
        PrivateCustodyKeeper {
            inventory: Arc::downgrade(&self.inventory),
        }
    }

    /// How many custodies this inventory was sized for.
    ///
    /// THE STORE'S BOUND, reported so a service can check that the owner it
    /// was handed is sized to the store it is about to be built over.
    pub(crate) fn custody_bound(&self) -> usize {
        match self.inventory.kept.lock() {
            Ok(kept) => kept.places.len(),
            Err(poisoned) => poisoned.into_inner().places.len(),
        }
    }

    /// How many custodies this owner is keeping.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn custodies_kept(&self) -> usize {
        match self.inventory.kept.lock() {
            Ok(kept) => kept.taken,
            Err(poisoned) => poisoned.into_inner().taken,
        }
    }

    /// How many more it can keep before the declared bound is reached.
    ///
    /// REPORTED, BECAUSE A REFUSAL AT THE BOUND IS A LIMITATION AND NOT A
    /// DISPOSITION. Nothing here retires an entry to make room.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn custody_capacity_remaining(&self) -> usize {
        match self.inventory.kept.lock() {
            Ok(kept) => kept.places.len().saturating_sub(kept.taken),
            Err(poisoned) => {
                let kept = poisoned.into_inner();
                kept.places.len().saturating_sub(kept.taken)
            }
        }
    }

    /// Pin the custody kept for this name, if this owner keeps one.
    ///
    /// BY NAME, NOT BY NUMBER. A place that went back and was taken by a
    /// successor has a different home, so a successor's name does not find its
    /// predecessor's evidence and cannot be used to reach it.
    #[cfg_attr(not(test), allow(dead_code))]
    fn custody_named(&self, identity: &PrivateMaintenanceIdentity) -> Option<PrivateCustodyPin<'_>> {
        let kept = match self.inventory.kept.lock() {
            Ok(kept) => kept,
            Err(poisoned) => poisoned.into_inner(),
        };
        let found = kept
            .places
            .iter()
            .flatten()
            .find(|custody| custody.identity().same_as(identity))
            .map(Arc::clone);
        drop(kept);
        found.map(|custody| PrivateCustodyPin {
            custody,
            owner: std::marker::PhantomData,
        })
    }
}
