// Who keeps one connection's join evidence, established before anything can
// produce a result that needs keeping.
//
// WHAT THIS IS AND IS NOT. It makes every operation BORROW an evidence home
// somebody else already owns, instead of handing back the only handle and
// hoping the caller stores it. That is a real property and it is enforced
// here. It is NOT a service lifetime: no production constructor makes one of
// these, so nothing here establishes that the running server has such an owner
// above its shutdown and error paths. Naming a type durable would not
// establish it either.

/// One connection's evidence custody, owned outside every operation.
///
/// ESTABLISHED FIRST, IN THE SCOPE THAT OUTLIVES WHAT IT PROTECTS. Its own
/// fields keep the store and the publication home; a reaping, a fencing and a
/// commitment borrow them. None of those is the final owner and none of them
/// can empty this by finishing, returning or unwinding.
///
/// WHY THAT ORDER. A join result is published into this home, and a home
/// created inside the operation that fills it is one whose only handle is in
/// that operation's frame. Returning it afterwards offers a keeper; it does
/// not make one, and a caller that dropped it or unwound would take the
/// evidence with it while the obligation stayed outstanding. Owning it first
/// is the difference between offering and having.
///
/// ONE CONNECTION, ONE HOME. This is not a pool, an admission limit, a sharing
/// scheme or a collection of past results, and nothing accumulates in it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No production constructor makes one yet.
struct PrivateEvidenceCustody {
    /// The store this connection's place and obligation live in.
    ///
    /// OWNED, because evidence about a connection is no use without the store
    /// that names it, and the whole point of this type is that both outlive
    /// the frames that use them.
    store: PrivateSettlementOwner,
    /// Which connection's evidence this keeps.
    identity: PrivateMaintenanceIdentity,
    /// The publication home a join will write into.
    ///
    /// ALLOCATED HERE, BEFORE ANY HANDLE IS CONSUMED. What is published into
    /// it later goes into a home that already had an owner, so losing the
    /// operation that published it loses the operation and not the result.
    join: Arc<PrivateJoinEvidence>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No production constructor makes one yet.
impl PrivateEvidenceCustody {
    /// Take custody of one connection's evidence, before anything produces
    /// any.
    ///
    /// PREPARATION REFUSES NOTHING AND CONSUMES NOTHING. The lease, the worker
    /// handle and whatever evidence already exists stay with their own owners;
    /// this allocates a home and takes handles, which is the order that
    /// matters -- a custody that consumed first and allocated afterwards would
    /// have the same hole it exists to close.
    fn prepared_for(
        store: &PrivateSettlementOwner,
        identity: PrivateMaintenanceIdentity,
    ) -> Self {
        Self {
            store: store.clone(),
            identity,
            join: Arc::new(PrivateJoinEvidence {
                phase: std::sync::atomic::AtomicU8::new(0),
                result: std::sync::OnceLock::new(),
            }),
        }
    }

    /// The store this custody keeps.
    fn store(&self) -> &PrivateSettlementOwner {
        &self.store
    }

    /// Which connection this custody is for.
    fn identity(&self) -> &PrivateMaintenanceIdentity {
        &self.identity
    }

    /// The publication home this custody owns.
    ///
    /// BORROWED, NOT HANDED OVER. A caller reads what is in it; what keeps it
    /// alive is this.
    fn join(&self) -> &Arc<PrivateJoinEvidence> {
        &self.join
    }

    /// A view of this custody for the length of one operation.
    fn view(&self) -> PrivateCustodyView<'_> {
        PrivateCustodyView(self)
    }
}

/// One operation's view of a connection's custody.
///
/// A BORROW, AND ENDING IT ENDS THE BORROW. There is no `Drop` here and there
/// must not be one that does anything: a scope going away is not a keeper
/// letting go, not a place being returned, not an identity being retired and
/// not a join or a fence becoming established. Losing a view loses a view.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Borrowed by callers no production site has yet.
struct PrivateCustodyView<'a>(&'a PrivateEvidenceCustody);

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Borrowed by callers no production site has yet.
impl PrivateCustodyView<'_> {
    fn store(&self) -> &PrivateSettlementOwner {
        self.0.store()
    }

    fn identity(&self) -> &PrivateMaintenanceIdentity {
        self.0.identity()
    }

    fn join(&self) -> &Arc<PrivateJoinEvidence> {
        self.0.join()
    }
}
