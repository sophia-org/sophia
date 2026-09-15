// What an execution reached, and what it still owes for having reached it.
//
// Split by subject: these are the records a transaction produces and a
// settlement later answers against, and they outlive the call that made them.
// Keeping them beside the transaction made one file responsible for both
// deciding and remembering.

/// What one ordered input reached.
///
/// Decided once, under the guards that decide it, and never asked again. The
/// fields are private and there is no way to build one outside the resolver,
/// so a later step cannot revise where an event went after the ledger has
/// recorded it going there.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateReachedResources {
    client: XServerFrontendClientId,
    window: XResourceId,
    surface: SurfaceId,
    namespace: NamespaceId,
    /// The seat whose pointer state this hold moved.
    ///
    /// Recorded with the plan so the release moves the same mapper the press
    /// moved. Finding it from the current route or from whatever seat a
    /// release happens to name would clear a different seat's buttons and
    /// leave this one's held forever.
    seat: SeatId,
    /// The grant that authorised the press.
    ///
    /// Settling a debt names the participant that owes it, and the capability
    /// does not expose its grant outside the authority. Recorded with the plan
    /// so the release that ends this hold can name the same participant its
    /// press was made by.
    grant: sophia_input_authority::GrantId,
}

#[cfg(unix)]
impl PrivateReachedResources {
    pub fn client(self) -> XServerFrontendClientId {
        self.client
    }
    pub fn window(self) -> XResourceId {
        self.window
    }
    pub fn surface(self) -> SurfaceId {
        self.surface
    }
    pub fn namespace(self) -> NamespaceId {
        self.namespace
    }
    pub fn seat(self) -> SeatId {
        self.seat
    }
}

/// How many refused recording attempts a release absorbs before visits stop
/// choosing it.
///
/// THIS IS NOT A PROOF THAT ANOTHER ATTEMPT WOULD FAIL. Common can become
/// usable again after any number of refusals, and nothing here can see that
/// happen. What the bound buys is that one unrecordable release cannot absorb
/// every visit and starve the others; what it costs is that such a release
/// ends up RETAINED BUT NOT DRIVEN -- its proof, its identity and its cause
/// are all still held, and nothing is currently scheduled to try again.
///
/// Scheduling that retry is open work, not a decision made here.
#[cfg(unix)]
const PRIVATE_NATIVE_RECORDING_ATTEMPTS: u8 = 3;

/// What this release's delivery is currently doing.
///
/// FORWARD CUSTODY. Once an emission leaves its hold it never goes back; it
/// travels on in this record instead, and this says where it has got to. The
/// phase is what makes a retry safe or unsafe to take, and it is deliberately
/// NOT derivable from whether the pending slot happens to be full.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateDispatchPhase {
    /// Nothing has been taken from the hold yet.
    Untaken,
    /// Held here and KNOWN NOT TO BE on the recipient's queue.
    ///
    /// A queue that refused because it was full said exactly that, so the
    /// same capsule is offered again later. Nothing is re-encoded, no target
    /// is reselected and no new event is built: the bytes and the identity
    /// are the ones the release decided.
    Pending,
    /// Handed over, and the handover never reported.
    ///
    /// NOT RETRIED. Nobody can say whether the recipient has it, and the
    /// pending slot being empty is not evidence that it does not -- that
    /// emptiness is exactly what an interrupted handoff looks like from here.
    Indeterminate,
    /// On the recipient's queue. What is owed from here is the receipt, and
    /// no replayable copy of the event is kept.
    Enqueued,
    /// The emission could not be wrapped, and is retained with its cause.
    Unwrappable,
    /// Handed over, answered, and NEVER TO BE SENT AGAIN.
    ///
    /// A write that failed or timed out may have put part of an event on the
    /// wire. Rebuilding it would produce a second copy of something the
    /// recipient may hold half of, and no later fact can establish how much
    /// arrived. The debt may stay unsettled; that is the honest outcome, and
    /// it is preferable to a duplicate nobody can detect.
    Unrepeatable,
}

/// What a release is holding on its way to a writer.
#[cfg(unix)]
enum PrivatePendingDelivery {
    /// The capsule, exactly as it will be sent.
    Capsule(XAuthorityOrderedDelivery),
    /// An emission that could not be wrapped, kept with the refusal that
    /// described it rather than dropped for being unusable at this moment.
    Unwrapped {
        #[allow(dead_code)]
        emission: PrivateOrderedEmission,
        #[allow(dead_code)]
        cause: crate::XAuthorityOrderedAssemblyRefusal,
    },
}

/// A release whose delivery has been decided and not yet handed on.
///
/// Everything the delivery owes, kept together and bound to the hold it ends.
/// The plan alone is not enough: the event carries the coordinates and the
/// state from the moment it was decided, and rebuilding either from later
/// facts would describe a different moment.
// Not Copy and not Clone. It owns a source obligation, and an obligation
// that can be duplicated is one that two places can both believe they are
// answering for.
#[cfg(unix)]
pub struct PrivateSettlingRelease {
    /// The identity a settlement is named against.
    incarnation: sophia_input_authority::HoldIncarnation,
    reached: PrivateReachedResources,
    /// What this release is carrying towards a writer, and how far it has got.
    ///
    /// The slot is prepared before anything is taken from the hold, so there
    /// is never a moment in which an emission has left its obligation and has
    /// nowhere to be.
    pending: Option<PrivatePendingDelivery>,
    dispatch: PrivateDispatchPhase,
    /// The attempt this release's delivery is being made under.
    ///
    /// Persisted BEFORE the handover, because a receipt arrives naming a
    /// delivery while the debt is named by an incarnation, and this record is
    /// the only thing holding both. An attempt reserved and not written down
    /// is one nothing could finish.
    attempt: Option<sophia_input_authority::AttemptToken>,
    /// The source obligation this release is still answering for.
    ///
    /// Carried rather than dropped with the record it came from. The hold
    /// owns the implicit activation, the query scope and the selection the
    /// press raised, and it is the only thing holding the exact connection
    /// they belong to. A release that removed the record and left this behind
    /// would retire none of them and leave nothing able to.
    ///
    /// Noncopy, and it stays owned here until a terminal continuation takes
    /// it: a release is not finished when its event is built.
    ///
    /// Retiring it is the sibling retirement work and is still open; carrying
    /// it is what makes that work possible at all, because the alternative is
    /// not "unread" but "gone". Donor holds stay here until those visits are
    /// complete.
    native: Option<private_native::Hold>,
    /// Why this release's event could not be built, when it could not.
    ///
    /// Kept as its own cause rather than folded into an absent event. A
    /// release that owes an event nobody could build is not a release that
    /// owes none, and the two are told apart here rather than by whoever
    /// later finds an empty slot.
    #[cfg_attr(not(test), allow(dead_code))]
    unbuilt: Option<PrivateAppliedRefusal>,
    /// Custody of this delivery's completion, taken before the handover.
    ///
    /// THE CELL IS THE IDENTITY. A delivery id can be pruned and handed out
    /// again, and the same client can then publish an outcome under that
    /// number for a different incarnation; an id, a timestamp and an epoch are
    /// what a caller supplied, not what an origin minted. This cell is minted
    /// by the ledger at admission, so holding it is holding the completion of
    /// that exact admission and of no other.
    ///
    /// Held rather than looked up. The ordinary observer prunes the ticket the
    /// moment it consumes the outcome, and a reader that went back for its
    /// answer would find it gone.
    completion: Option<Arc<PrivateDeliveryCompletion>>,
    /// The writer's own answer for this release's delivery, once it has one.
    ///
    /// Preserved separately from what it settled. "Nothing was settled" and
    /// "settled because the recipient was gone" are different facts, and a
    /// reader with only the settlement bits cannot tell them apart.
    #[cfg_attr(not(test), allow(dead_code))]
    outcome_seen: Option<XAuthorityInputDeliveryOutcome>,
    /// Whether the source's own native bit has been recorded for this release.
    native_recorded: bool,
    /// What the last recording attempt refused with, if one did.
    #[cfg_attr(not(test), allow(dead_code))]
    native_failure: Option<PrivateAuthorityRefusal>,
    /// How many recording attempts have been spent against the bound below.
    native_attempts: u8,
    outcome: sophia_input_authority::ReleaseOutcome,
    event: Option<XAuthorityInputEvent>,
    /// The delivery that carries this release's event.
    ///
    /// Recorded because a receipt arrives naming a delivery and settles a
    /// debt named by an incarnation, and nothing else holds both. Without it
    /// a writer's result can be observed and still not be attributable: the
    /// debt it settles would have to be guessed from whatever else was in
    /// flight, and a guess that settles the wrong incarnation lets a later
    /// press through a barrier that was still owed.
    ///
    /// `None` where the release carried no delivery identity, which is not
    /// the same as a receipt that has not arrived.
    delivery: Option<XAuthorityInputDeliveryId>,
    /// What the ledger will do with this release's event.
    ///
    /// The debt is recorded whichever it is: the hold ended, and something was
    /// owed for it. What differs is what may be concluded from it. `Ended`
    /// establishes that nothing will be carried, so a settlement waiting for a
    /// receipt would wait for one that cannot come. `Unknown` establishes
    /// nothing at all -- and neither may ever be read as the recipient having
    /// settled, which is a fact only a writer's own outcome can supply.
    binding: PrivateReleaseBinding,
}

#[cfg(unix)]
impl PrivateSettlingRelease {
    /// The delivery whose receipt settles this debt, if it has one.
    pub fn delivery(&self) -> Option<XAuthorityInputDeliveryId> {
        self.delivery
    }
    /// The identity, not the number inside it.
    ///
    /// A caller settling this debt names the incarnation; one that could only
    /// ask for the number could not settle anything with the answer.
    pub fn incarnation(&self) -> sophia_input_authority::HoldIncarnation {
        self.incarnation
    }
    pub fn reached(&self) -> PrivateReachedResources {
        self.reached
    }
    /// The source obligation this release still carries.
    ///
    /// Borrowed, never taken: whoever retires it has to be the terminal
    /// continuation that owns this release, not a reader passing through.
    #[cfg_attr(not(test), allow(dead_code))]
    fn native(&self) -> Option<&private_native::Hold> {
        self.native.as_ref()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn unbuilt(&self) -> Option<PrivateAppliedRefusal> {
        self.unbuilt
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn native_recorded(&self) -> bool {
        self.native_recorded
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn outcome_seen(&self) -> Option<XAuthorityInputDeliveryOutcome> {
        self.outcome_seen
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn dispatch(&self) -> PrivateDispatchPhase {
        self.dispatch
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn attempt(&self) -> Option<sophia_input_authority::AttemptToken> {
        self.attempt
    }

    /// Whether a delivery attempt may be made for this release now.
    ///
    /// READ FROM THE PHASE, never from the slot. An empty slot means one of
    /// two opposite things -- nothing taken yet, or a handover that never
    /// reported -- and only the phase tells them apart. Deciding from the slot
    /// would retry exactly the deliveries nobody can say were not already
    /// received.
    ///
    /// The native half has to be in first, because the ledger refuses to
    /// claim an attempt otherwise, and one attempt at a time, because a
    /// second would be a second writer answering for the same event.
    fn owes_delivery_attempt(&self) -> bool {
        self.native_recorded
            && self.attempt.is_none()
            && matches!(
                self.dispatch,
                PrivateDispatchPhase::Untaken | PrivateDispatchPhase::Pending
            )
    }

    /// Take custody of this delivery's completion.
    fn hold_completion(&mut self, cell: Arc<PrivateDeliveryCompletion>) {
        self.completion = Some(cell);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn completion(&self) -> Option<&Arc<PrivateDeliveryCompletion>> {
        self.completion.as_ref()
    }

    /// This delivery's answer, if its completion has one.
    ///
    /// One read of one cell. There is no ticket to validate and no second
    /// lookup to pair with it, so there is no interval in which the delivery
    /// could be pruned and re-admitted between establishing identity and
    /// reading the outcome.
    fn completion_answer(&self) -> Option<XAuthorityClientInputDelivery> {
        self.completion.as_ref()?.answer()
    }

    /// Keep the writer's own answer, whatever it settled.
    fn record_outcome(&mut self, outcome: XAuthorityInputDeliveryOutcome) {
        self.outcome_seen = Some(outcome);
    }

    /// Mark this release's event as one that must never be sent again.
    fn mark_unrepeatable(&mut self) {
        self.pending = None;
        self.dispatch = PrivateDispatchPhase::Unrepeatable;
    }

    /// Stop naming an attempt, once the ledger has confirmed it back.
    fn clear_attempt(&mut self) {
        self.attempt = None;
    }

    fn native_mut(&mut self) -> Option<&mut private_native::Hold> {
        self.native.as_mut()
    }

    /// Record the source's native bit for this release, if it is still owed.
    ///
    /// CALLED WITH NO ADAPTER GUARD AND NO COMMON TRANSACTION HELD. The proof
    /// enters common as its own origin, so calling this from inside the
    /// transaction that produced it is common re-entering itself.
    ///
    /// Bounded. A recording that refuses is retried on later turns and then
    /// left owed with its cause: retrying without end is a release that never
    /// finishes, and giving up quietly is a debt nobody knows is open. THE
    /// RECORDING IS RETRIED, NEVER THE RELEASE -- the effect happened once.
    fn record_native_once(&mut self) -> bool {
        if !self.owes_native_recording() {
            return false;
        }
        let Some(proof) = self.native.as_ref().and_then(private_native::Hold::proof) else {
            return false;
        };
        self.native_attempts = self.native_attempts.saturating_add(1);
        match proof.record_native() {
            // `false` is not a failure. It says the native bit is recorded
            // while the recipient's own obligation is still outstanding, and
            // those are two different debts; this call answers one of them.
            Ok(_) => {
                self.native_recorded = true;
                self.native_failure = None;
            }
            Err(cause) => self.native_failure = Some(cause),
        }
        // WHETHER THE BIT WENT IN, not whether a visit happened. The visit is
        // the caller's own fact -- it spent one either way -- and reporting a
        // refusal as a recording would let a counter rise while the debt it
        // counts stays exactly as owed as before.
        self.native_recorded
    }

    /// Whether a visit should choose this release.
    ///
    /// False once recorded, and false once the attempt bound is reached --
    /// which says this release is no longer being driven, not that it is
    /// finished. Its proof and cause are still here to be driven by whatever
    /// schedules that later.
    fn owes_native_recording(&self) -> bool {
        !self.native_recorded
            && self.native_attempts < PRIVATE_NATIVE_RECORDING_ATTEMPTS
            && self.native.as_ref().and_then(private_native::Hold::proof).is_some()
    }


    pub fn outcome(&self) -> sophia_input_authority::ReleaseOutcome {
        self.outcome
    }
    pub fn event(&self) -> Option<XAuthorityInputEvent> {
        self.event
    }
    /// Not `pub`, because what it answers with is not. The reason a release
    /// was or was not carried is this module's vocabulary, and widening the
    /// type to match an accessor would export a decision nobody outside has
    /// asked to make.
    #[cfg_attr(not(test), allow(dead_code))]
    fn binding(&self) -> PrivateReleaseBinding {
        self.binding
    }
}

/// What the ledger will do with a release's event.
///
/// Three answers rather than a flag, because the flag collapsed two facts a
/// settlement has to keep apart: a recipient that is established to be gone,
/// and a ledger nobody could read.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateReleaseBinding {
    /// Bound to the recipient its press reached. An event is owed.
    Reached,
    /// The ledger will not carry it: a terminal outcome is already recorded
    /// for this delivery, or its recipient's connection is revoked.
    Ended,
    /// The ledger could not be read. Whether anything can still be carried is
    /// unknown, which is not the same as nothing being carried.
    Unknown,
}

/// One hold this executor began, and what answering it needs.
///
/// The input is part of the record because a press has to know whether it is
/// starting a hold or joining one *before* the ledger moves. A join reaches
/// nobody new -- it adopts the recipient the hold already has -- so binding
/// its delivery to whatever the route resolves to now would name a client the
/// event never reached.
#[cfg(unix)]
struct PrivateHoldRecord {
    /// The whole minted identity, not the number inside it.
    ///
    /// A settlement names an incarnation -- authority, recipient, connection
    /// generation and input together -- and an attempt claim is matched
    /// against one. The number alone cannot be compared with either, so a
    /// debt recorded as a number is a debt nothing can later answer for.
    incarnation: sophia_input_authority::HoldIncarnation,
    reached: PrivateReachedResources,
    /// The native obligation this press began.
    ///
    /// Carried whole rather than copied out of, because it is the only thing
    /// that can answer for this hold natively and it does not duplicate. It
    /// travels with the record through terminal transfers: an inventory handed
    /// on without it would describe a hold whose native side nobody could
    /// complete, and a second copy would let two holders each believe they
    /// were the one completing it.
    ///
    /// `None` where no native operation produced one, which is every record
    /// today: the press that installs one is the next step, and the slot is
    /// here first so that the record it travels in is not reshaped twice.
    #[allow(dead_code)]
    native: Option<private_native::Hold>,
}
