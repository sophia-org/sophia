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
    /// Holds this executor began, with where each was delivered.
    ///
    /// A release answers to what its press reached, so this is what makes a
    /// later release answerable at all.
    holds: Vec<(u64, PrivateReachedResources)>,
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
    /// Storage is reserved up front, so recording an obligation never
    /// allocates on the path where something has already happened.
    fn with_capacity(capacity: usize) -> Self {
        Self {
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
        self.holds.is_empty()
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
    fn outstanding(&self) -> usize {
        self.holds
            .len()
            .saturating_add(self.settling.len())
            .saturating_add(usize::from(self.current.is_some()))
            .saturating_add(self.turn.len())
            .saturating_add(self.delivering.len())
            .saturating_add(self.undelivered.len())
    }

    /// Move everything owed out, leaving an inventory that owes nothing.
    ///
    /// The reserved buffers stay with the emptied inventory rather than going
    /// with what is taken: the instance handing over may still be running, and
    /// the next obligation it records must not allocate.
    fn hand_over(&mut self, capacity: usize) -> Self {
        let mut taken = Self::with_capacity(capacity);
        taken.holds.append(&mut self.holds);
        taken.settling.append(&mut self.settling);
        taken.current = self.current.take();
        taken.turn.append(&mut self.turn);
        taken.delivering.append(&mut self.delivering);
        taken.undelivered.append(&mut self.undelivered);
        // The phase describes the head of `delivering`, which has moved with
        // it, so it moves too rather than being left describing nothing.
        taken.emission = std::mem::replace(&mut self.emission, PrivateEmissionPhase::NotOwed);
        taken
    }
}
