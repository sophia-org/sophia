// Everything one instance still owes for the work it accepted.
//
// Split by subject because the subject is ownership: these are obligations,
// and an obligation that lives in several places is one that can be answered
// in several places -- or, when an instance goes, in none.

/// What an instance still owes, in one place that can be handed on.
///
/// Reachable through the live owner, through the settlement handle it returns,
/// and through the durable owner behind both. That chain is the point: each
/// entry carries what answering it requires, so whoever holds the inventory
/// can answer without asking anything that has gone.
///
/// Nothing here goes back into the order as a command. Work that has already
/// applied, or that may already be on a client's queue, is not replayable --
/// requeueing it would apply an effect twice and no reader downstream could
/// tell. What is carried is the right to finish answering for it.
#[cfg(unix)]
struct PrivateTerminalInventory {
    /// The registry that can answer for everything here.
    ///
    /// Inseparable from the obligations rather than held alongside them. A
    /// hold's plan names a client, a window and a seat's projection, and all
    /// of those are the registry's to reach -- so an inventory that outlived
    /// its registry would describe obligations nothing could act on. Custody
    /// sometimes keeping a capability alive is not a guarantee for the
    /// inventory as a whole: the last custody can be observed and dropped
    /// while holds remain.
    origin: XServerFrontendRouteRegistry,
    /// The authority these obligations answer to.
    ///
    /// Carried for the same reason. A retained hold still names a ledger
    /// incarnation and a credit, and neither can be reached once the
    /// controller has gone.
    controller: PrivateAuthorityController,
    lifecycle: PrivateLifecycleOwner,
    /// Holds this executor began, with where each was delivered.
    ///
    /// A release answers to what its press reached, so this is what makes a
    /// later release answerable at all.
    holds: Vec<PrivateHoldRecord>,
    /// Releases whose delivery was decided and whose debt is still open.
    settling: Vec<PrivateSettlingRelease>,
    /// The item currently being executed.
    ///
    /// Owned before the execution that could fail, so an interruption leaves
    /// the obligation here rather than in a frame that is going.
    current: Option<PrivateOrderedItem>,
    /// The items of the turn in progress.
    turn: Vec<PrivateOrderedItem>,
    /// Decided work being handed on right now.
    delivering: Vec<PrivateOrderedItem>,
    /// How far the entry at the head of `delivering` got toward its client.
    ///
    /// Beside the entry rather than inside a local, because the case it
    /// describes is an unwind inside the send.
    emission: PrivateEmissionPhase,
    /// Decided work that has not been handed on, with how far it got.
    undelivered: Vec<PrivateUndelivered>,
}

#[cfg(unix)]
impl PrivateTerminalInventory {
    /// Storage for the fixed-size records is reserved up front. The turn and
    /// delivery lists are reserved to the service budget and can still grow
    /// past it, which is open preallocation work rather than a guarantee.
    fn with_capacity(
        origin: XServerFrontendRouteRegistry,
        controller: PrivateAuthorityController,
        lifecycle: PrivateLifecycleOwner,
        capacity: usize,
    ) -> Self {
        Self {
            origin,
            controller,
            lifecycle,
            holds: Vec::with_capacity(PRIVATE_HOLD_RECORDS),
            settling: Vec::with_capacity(PRIVATE_HOLD_RECORDS),
            current: None,
            turn: Vec::with_capacity(capacity),
            delivering: Vec::with_capacity(capacity),
            emission: PrivateEmissionPhase::NotOwed,
            undelivered: Vec::with_capacity(capacity),
        }
    }

    /// Whether anything is still owed.
    ///
    /// An empty inventory is one nobody needs to carry; a non-empty one is an
    /// obligation, whoever happens to be holding it.
    fn is_empty(&self) -> bool {
        self.lifecycle.inventory().is_ok_and(|inventory| inventory.open == 0 && inventory.closed == 0)
            && self.holds.is_empty()
            && self.settling.is_empty()
            && self.current.is_none()
            && self.turn.is_empty()
            && self.delivering.is_empty()
            && self.undelivered.is_empty()
    }

    /// How many separate obligations are here.
    ///
    /// Counted rather than summarised as a boolean, because "some" and "one"
    /// are different things to whoever has to finish them.
    #[cfg_attr(not(test), allow(dead_code))]
    fn outstanding(&self) -> Option<usize> {
        let lifecycle = self.lifecycle.inventory().ok()?;
        Some(self.holds
            .len()
            .saturating_add(self.settling.len())
            .saturating_add(usize::from(self.current.is_some()))
            .saturating_add(self.turn.len())
            .saturating_add(self.delivering.len())
            .saturating_add(self.undelivered.len())
            .saturating_add(lifecycle.open).saturating_add(lifecycle.closed))
    }

    /// Move everything owed out, storage and capabilities together.
    ///
    /// A move, not a copy into fresh buffers. This runs while an instance is
    /// closing, and building new destinations there allocates during cleanup
    /// -- the opposite of what reserving them was for. The emptied inventory
    /// is left with no capacity because nothing records into a closing
    /// instance; an inventory that had to keep recording would need its
    /// destination reserved before the work was accepted, which is a different
    /// arrangement from this one.
    fn hand_over(&mut self) -> Self {
        std::mem::replace(
            self,
            Self {
                origin: self.origin.clone(),
                controller: self.controller.clone(),
                lifecycle: self.lifecycle.clone(),
                holds: Vec::new(),
                settling: Vec::new(),
                current: None,
                turn: Vec::new(),
                delivering: Vec::new(),
                emission: PrivateEmissionPhase::NotOwed,
                undelivered: Vec::new(),
            },
        )
    }

    /// The registry that can answer for what is here.
    #[cfg_attr(not(test), allow(dead_code))]
    fn origin(&self) -> &XServerFrontendRouteRegistry {
        &self.origin
    }

    /// The authority these obligations answer to.
    #[cfg_attr(not(test), allow(dead_code))]
    fn controller(&self) -> &PrivateAuthorityController {
        &self.controller
    }
}
