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
impl PrivateHoldRecord {
    /// Whether this press's own event is still owed to its recipient.
    ///
    /// A press has no debt in the ledger -- one appears when the last holder
    /// goes -- so no attempt is claimed for it. What it has is an event that
    /// was decided and a handle to answer it through.
    fn owes_press_handover(&self) -> bool {
        self.custody.owes_handover()
    }
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

/// What one decided event carries on its way to a writer, and how far it has
/// got.
///
/// ONE THING, held by whatever owes the event. A press and a release owe their
/// recipients the same kind of obligation -- an event that was decided, a
/// handle to answer it through, an attempt it is being made under, and a
/// phase saying where it has reached -- and keeping two copies of that shape
/// invited them to drift apart exactly where they must not.
#[cfg(unix)]
struct PrivateDeliveryCustody {
    /// Where this event sits in the order its recipient must see.
    ///
    /// STAMPED WHEN THE DEBT IS RECORDED, and compared across every place a
    /// custody can live. Preferring one storage location over another is not
    /// an order: a press whose hold has ended travels into the settling
    /// record, so choosing held work first hands a later press over before an
    /// earlier one that merely moved.
    order: u64,
    /// The slot is prepared before anything is taken from the hold, so there
    /// is never a moment in which an emission has left its obligation and has
    /// nowhere to be.
    pending: Option<PrivatePendingDelivery>,
    dispatch: PrivateDispatchPhase,
    /// The attempt this delivery is being made under.
    ///
    /// Persisted BEFORE the handover, because a receipt arrives naming a
    /// delivery while the debt is named by an incarnation, and the record that
    /// holds both is the only thing that can join them.
    attempt: Option<sophia_input_authority::AttemptToken>,
    /// Custody of this delivery's completion, taken on the operation that
    /// created the debt.
    ///
    /// THE CELL IS THE IDENTITY. An id, a timestamp and an epoch are what a
    /// caller supplied; this is what the ledger minted, so holding it is
    /// holding the completion of that exact admission and of no other.
    completion: Option<Arc<PrivateDeliveryCompletion>>,
    /// The writer's own answer, once there is one, kept apart from what it
    /// settled.
    outcome_seen: Option<XAuthorityInputDeliveryOutcome>,
}

#[cfg(unix)]
impl PrivateDeliveryCustody {
    /// Whether this debt's event is still owed a handover.
    ///
    /// READ FROM THE PHASE, never from the slot. An empty slot means either
    /// nothing taken yet or a handover that never reported, and only the phase
    /// tells them apart.
    fn owes_handover(&self) -> bool {
        self.completion.is_some()
            && matches!(
                self.dispatch,
                PrivateDispatchPhase::Untaken | PrivateDispatchPhase::Pending
            )
    }

    /// Whether this custody's phase permits a handover to be attempted.
    ///
    /// ORDERING AND PERMISSION ARE DIFFERENT QUESTIONS. An unresolved handover
    /// must stay in the ordering comparison, because it is what blocks the
    /// events behind it -- but being the head does not make it sendable. A
    /// phase that says the bytes may already be on the wire authorizes
    /// nothing: offering that capsule again is a replay, and its slot holding
    /// bytes is not permission.
    fn handover_permitted(&self) -> bool {
        matches!(
            self.dispatch,
            PrivateDispatchPhase::Untaken | PrivateDispatchPhase::Pending
        )
    }

    /// Whether anything about this event's handover is still unresolved.
    ///
    /// An unfinished handover -- owed, or begun and unreported -- is what a
    /// later event of the same hold must not overtake.
    fn handover_unfinished(&self) -> bool {
        self.completion.is_some()
            && !matches!(self.dispatch, PrivateDispatchPhase::Enqueued)
    }

    /// Begin custody for a debt, holding the completion it was created with.
    fn new(order: u64, completion: Option<Arc<PrivateDeliveryCompletion>>) -> Self {
        Self {
            order,
            pending: None,
            dispatch: PrivateDispatchPhase::Untaken,
            attempt: None,
            completion,
            outcome_seen: None,
        }
    }
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
    custody: PrivateDeliveryCustody,
    /// The custody of the PRESS this release ended, carried on.
    ///
    /// A DISTINCT INSTANCE, not a replacement. Ending the physical hold does
    /// not answer the press event or transfer its delivery: the press and the
    /// release are two events owed to the same recipient, each with its own
    /// admission and its own handle. Dropping the press's with the hold record
    /// left its delivery owed by nobody, with the answer reachable only
    /// through owners outside this instance.
    #[allow(dead_code)]
    press_custody: Option<PrivateDeliveryCustody>,
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
    native: Option<PrivateNativeHold>,
    /// Why this release's event could not be built, when it could not.
    ///
    /// Kept as its own cause rather than folded into an absent event. A
    /// release that owes an event nobody could build is not a release that
    /// owes none, and the two are told apart here rather than by whoever
    /// later finds an empty slot.
    #[cfg_attr(not(test), allow(dead_code))]
    unbuilt: Option<PrivateAppliedRefusal>,
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
    fn native(&self) -> Option<&PrivateNativeHold> {
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
        self.custody.outcome_seen
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn dispatch(&self) -> PrivateDispatchPhase {
        self.custody.dispatch
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn attempt(&self) -> Option<sophia_input_authority::AttemptToken> {
        self.custody.attempt
    }

    fn custody_order(&self) -> u64 {
        self.custody.order
    }

    fn custody_handover_permitted(&self) -> bool {
        self.custody.handover_permitted()
    }

    fn custody_handover_unfinished(&self) -> bool {
        self.custody.handover_unfinished()
    }

    /// Whether this release's own press is still waiting to be handed over.
    ///
    /// A release must not reach the recipient's queue before the press it
    /// ends. The press's custody travels here when the hold record goes, so
    /// this is where that ordering is decided.
    fn press_handover_unfinished(&self) -> bool {
        self.press_custody
            .as_ref()
            .is_some_and(PrivateDeliveryCustody::handover_unfinished)
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
        // ORDER FIRST. A release cannot be handed over while the press it
        // ends is still owed one, or has begun one that never reported: the
        // recipient would see the button come up before it went down.
        !self.press_handover_unfinished()
            && self.native_recorded
            && self.custody.attempt.is_none()
            && matches!(
                self.custody.dispatch,
                PrivateDispatchPhase::Untaken | PrivateDispatchPhase::Pending
            )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn completion(&self) -> Option<&Arc<PrivateDeliveryCompletion>> {
        self.custody.completion.as_ref()
    }

    /// This delivery's answer, if its completion has one.
    ///
    /// One read of one cell. There is no ticket to validate and no second
    /// lookup to pair with it, so there is no interval in which the delivery
    /// could be pruned and re-admitted between establishing identity and
    /// reading the outcome.
    fn completion_answer(&self) -> Option<XAuthorityClientInputDelivery> {
        self.custody.completion.as_ref()?.answer()
    }

    /// Keep the writer's own answer, whatever it settled.
    fn record_outcome(&mut self, outcome: XAuthorityInputDeliveryOutcome) {
        self.custody.outcome_seen = Some(outcome);
    }

    /// Mark this release's event as one that must never be sent again.
    fn mark_unrepeatable(&mut self) {
        self.custody.pending = None;
        self.custody.dispatch = PrivateDispatchPhase::Unrepeatable;
    }

    /// Stop naming an attempt, once the ledger has confirmed it back.
    fn clear_attempt(&mut self) {
        self.custody.attempt = None;
    }

    fn native_mut(&mut self) -> Option<&mut PrivateNativeHold> {
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
        let Some(proof) = self.native.as_ref().and_then(PrivateNativeHold::proof) else {
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
            && self.native.as_ref().and_then(PrivateNativeHold::proof).is_some()
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
    /// What this press owes its recipient, and how far it has got.
    ///
    /// The same custody a release carries. A press owes an event just as a
    /// release does, and giving it a different shape is how the two paths
    /// drifted apart in the first place.
    ///
    /// NOT YET DRIVEN. The acquisition is live -- a press whose answer could
    /// never be recognised refuses because of it -- and what has not landed is
    /// the dispatch that reads the rest. It is taken now rather than when that
    /// arrives, because acquiring late is the defect this shape exists to
    /// prevent.
    #[allow(dead_code)]
    custody: PrivateDeliveryCustody,
    /// The native obligation this press began.
    ///
    /// Carried whole rather than copied out of, because it is the only thing
    /// that can answer for this hold natively and it does not duplicate. It
    /// travels with the record through terminal transfers: an inventory handed
    /// on without it would describe a hold whose native side nobody could
    /// complete, and a second copy would let two holders each believe they
    /// were the one completing it.
    ///
    /// `None` while the destination record has been installed but the source
    /// obligation has not yet moved from the inventory's pending slot. Either
    /// native kind uses the same transfer; no obligation lives only in a local
    /// across its source operation.
    #[allow(dead_code)]
    native: Option<PrivateNativeHold>,
}
