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
    /// Whether a grab chose this rather than the route.
    grabbed: bool,
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
    pub fn grabbed(self) -> bool {
        self.grabbed
    }
}

/// facts would describe a different moment.
#[cfg(unix)]
#[derive(Debug, Clone, Copy)]
pub struct PrivateSettlingRelease {
    /// The identity a settlement is named against.
    incarnation: sophia_input_authority::HoldIncarnation,
    reached: PrivateReachedResources,
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
    pub fn delivery(self) -> Option<XAuthorityInputDeliveryId> {
        self.delivery
    }
    /// The identity, not the number inside it.
    ///
    /// A caller settling this debt names the incarnation; one that could only
    /// ask for the number could not settle anything with the answer.
    pub fn incarnation(self) -> sophia_input_authority::HoldIncarnation {
        self.incarnation
    }
    pub fn reached(self) -> PrivateReachedResources {
        self.reached
    }
    pub fn outcome(self) -> sophia_input_authority::ReleaseOutcome {
        self.outcome
    }
    pub fn event(self) -> Option<XAuthorityInputEvent> {
        self.event
    }
    /// Not `pub`, because what it answers with is not. The reason a release
    /// was or was not carried is this module's vocabulary, and widening the
    /// type to match an accessor would export a decision nobody outside has
    /// asked to make.
    #[cfg_attr(not(test), allow(dead_code))]
    fn binding(self) -> PrivateReleaseBinding {
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
#[derive(Debug, Clone, Copy)]
struct PrivateHoldRecord {
    /// The whole minted identity, not the number inside it.
    ///
    /// A settlement names an incarnation -- authority, recipient, connection
    /// generation and input together -- and an attempt claim is matched
    /// against one. The number alone cannot be compared with either, so a
    /// debt recorded as a number is a debt nothing can later answer for.
    incarnation: sophia_input_authority::HoldIncarnation,
    reached: PrivateReachedResources,
}
