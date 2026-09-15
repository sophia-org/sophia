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
/// Where a claimed attempt has got to, while this executor holds it.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateAttemptPhase {
    /// Claimed and not handed to anyone. An unused reservation, safe to give
    /// back with neither bit.
    Unplaced,
    /// The handover to a recipient has begun and did not report.
    ///
    /// NOT SAFE TO GIVE BACK. The delivery may be on the queue, and returning
    /// the attempt as unused would say a delivery that may have happened did
    /// not. It is resolved by the receipt, not by this phase being tidied.
    Dispatching,
}

/// One attempt held by this executor, with where it has got to.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
struct PrivateAttemptCustody {
    token: sophia_input_authority::AttemptToken,
    phase: PrivateAttemptPhase,
}

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
    /// Where the source installs a native obligation before its effect.
    ///
    /// It has to exist before the operation is called, because the source
    /// refuses to begin one while this is occupied, and it has to be owned
    /// here rather than by the call: an interruption between the effect and
    /// the record would otherwise take with it the only thing that can answer
    /// for the hold the ledger has already begun.
    ///
    /// Empty between operations. What lands here moves into the record for its
    /// hold as soon as that record exists, and nothing else reads it.
    native_pending: Option<private_native::Hold>,
    /// Where a debt's custody is prepared before the effect that creates it.
    ///
    /// INSTANCE-OWNED BEFORE THE SOURCE IS ENTERED, for the same reason
    /// native_pending is: an interruption between the effect and the record
    /// would otherwise take with it the only handle that can answer the event
    /// that effect just owed. It travels beside native_pending through a
    /// refusal or an unwind, and moves into the record for its debt as soon as
    /// that record exists.
    ///
    /// Empty between operations that completed. A refusal which left the
    /// source holding context leaves this held too, attached to that same
    /// continuation, and the next operation is refused rather than allowed to
    /// replace it.
    pending_custody: Option<PrivateDeliveryCustody>,
    /// Releases whose delivery was decided and whose debt is still open.
    settling: Vec<PrivateSettlingRelease>,
    /// How many terminal steps have gone to deliveries since native work last
    /// had a turn.
    ///
    /// Retained, because fairness between two kinds of work cannot be decided
    /// from a single step: choosing native work only when the queues happen to
    /// be empty lets a delivery that is always ready starve a proof forever,
    /// and that is not a rare interleaving -- it is what a busy pointer looks
    /// like.
    native_turn_debt: u8,
    /// The one attempt this executor may hold unplaced at a time.
    ///
    /// A PRE-EXISTING SLOT, not storage acquired after the grant. Claiming
    /// first and finding somewhere to put the token afterwards is a
    /// reservation whose custody is not yet reserved; this driver places one
    /// claim at a time, so one slot is the exact storage and it is empty
    /// before the ledger is asked.
    ///
    /// Carries its phase, because an unplaced claim and a claim whose handover
    /// has begun are opposite things: the first is an unused reservation and
    /// safe to give back, the second may already be on a recipient's queue.
    attempt_custody: Option<PrivateAttemptCustody>,
    /// How many recording visits have passed since dispatch last had a turn.
    ///
    /// Recording must come first for any ONE release, because the ledger
    /// refuses an attempt until that release's native half is in. Preferring
    /// it across ALL releases is a different thing and starves delivery debt
    /// that is already native: these are two classes of terminal work, and
    /// they are arbitrated rather than ranked.
    native_class_debt: u8,
    /// The ledger's own fair cursor for claiming delivery attempts.
    ///
    /// Retained for the same reason as the recording cursor, and kept apart
    /// from it: the ledger advances this one itself, over debts rather than
    /// over this executor's records.
    attempt_cursor: usize,
    /// Where the next proof-recording visit starts looking.
    ///
    /// Retained rather than restarted, so visits move through the releases
    /// that owe a recording instead of returning to the same one. One entry
    /// is chosen per charged visit; sweeping the whole vector would make the
    /// work unbounded and, worse, make it incidental to whatever else was
    /// happening rather than something the service can be asked for.
    native_recording_cursor: usize,
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
            native_pending: None,
            pending_custody: None,
            native_recording_cursor: 0,
            attempt_cursor: 0,
            attempt_custody: None,
            native_class_debt: 0,
            native_turn_debt: 0,
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
            // An attempt this executor holds is the ledger's slot, and an
            // instance reporting itself empty while holding one is reporting
            // the absence of its own records rather than of the obligation.
            && self.attempt_custody.is_none()
            // A retained source obligation is an obligation. It is normally
            // empty between operations, but a disagreement leaves one here
            // deliberately, and an instance reporting itself empty while
            // holding an activation, a query scope and a selection would be
            // reporting the absence of the record rather than of the debt.
            && self.native_pending.is_none()
            // A retained custody is an obligation on its own. It outlives a
            // refusal that left the source holding context, and an instance
            // reporting itself empty while holding one would be reporting the
            // absence of a record rather than of the debt.
            && self.pending_custody.is_none()
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
            .saturating_add(usize::from(self.native_pending.is_some()))
            .saturating_add(usize::from(self.pending_custody.is_some()))
            .saturating_add(usize::from(self.attempt_custody.is_some()))
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
                native_pending: None,
                pending_custody: None,
                native_recording_cursor: 0,
                attempt_cursor: 0,
                attempt_custody: None,
                native_class_debt: 0,
                native_turn_debt: 0,
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
