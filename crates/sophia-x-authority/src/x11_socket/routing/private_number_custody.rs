// Who holds a client number, and for how long.
//
// A client number is an index into a namespace, not an identity. Two
// connections can hold the same number one after the other, and the second is
// not the first. Everything a connection's ending does BY NUMBER -- cancelling
// its expected writer, disconnecting its recovery, removing its row, its
// surfaces, its focus, its parents, its subscriptions, its pending
// presentations and its frozen input -- would otherwise be done to whoever
// holds that number when the effect runs.
//
// ABSENCE FROM THE CLIENT TABLE IS NOT AN ANSWER. A row is removed when a send
// finds its endpoint disconnected, and when a client stops draining its queue.
// So "no row" does not mean "that connection has finished with its number",
// and a check that looked there would be looking at the wrong thing.
//
// WHAT THIS IS. A reservation on the number itself, taken before a connection
// can establish anything under it and given back only when reuse is safe. It
// is a permission and a lifetime rule. It settles nothing: a number released
// here says nothing about retained deliveries, join or fence evidence, native
// debts or any maintenance obligation.
//
// WHERE IT IS TAKEN AND WHAT IT COVERS. One entry point takes it --
// publication -- and one interval gives it back: the cleanup a connection's
// destruction runs. Between those it covers every effect that cleanup performs
// by number: the expected-writer cancellation, the recovery disconnect (which
// closes the lifecycle gate, revokes the entry and shuts the socket down), the
// row removal, the surfaces, the focus, the parent entries, the selection and
// presentation subscriptions, the pending presentations and the frozen input
// it settles. It does NOT cover this connection's own gate, home or place:
// those are reached by identity and never by number, so they need no
// permission and lose nothing by being disposed of first.
//
// IT DOES NOT COVER DELAYED REMOVALS, AND IS NOT MEANT TO. A send that finds
// its endpoint gone, or a queue that stopped draining, removes a row long
// after the sender was captured -- possibly after a successor took the number.
// Those paths carry the identity they failed with and compare it against the
// row they would remove. That is a different rule with the same subject, and
// neither stands in for the other.
//
// THIS IS STRICTER THAN WHAT IT REPLACES, DELIBERATELY. Reuse of a number
// whose previous connection has no row was admitted before and is refused now.
// Two cases change: a connection whose ending is part-way through its
// number-keyed effects, and one whose ending could not finish them. The first
// is refused for as long as that ending takes. The second is refused until
// something establishes that the work was done -- which nothing here does, and
// which is deliberately not resolved by a timeout or by trying again. A
// namespace that cannot reissue a number is reporting a failure it really
// had.

/// Which exact connection holds a client number.
///
/// THE IDENTITY IS THE ONE REGISTRATION ALREADY MINTS. Its applied-state cell
/// is made once per registration and is what every other exactness check in
/// this registry already compares, so a number's occupant is named by the same
/// thing a row is. Held weakly: the allocation's address stays unique while
/// this names it, so a successor cannot be mistaken for its predecessor.
#[cfg(unix)]
struct PrivateNumberClaim {
    incarnation: std::sync::Weak<std::sync::OnceLock<PrivateAppliedClientState>>,
    /// Whether the connection that holds this number is running the effects
    /// that act by it.
    ///
    /// SET BEFORE THE FIRST ONE AND NOT CLEARED BY THEM. A successor refused
    /// while this is set is refused because the interval is open, which is a
    /// different fact from the row still being live.
    clearing: bool,
    /// Whether that connection ended without establishing that its number is
    /// safe to reuse.
    ///
    /// AN UNREADABLE TABLE OR AN INTERRUPTED CLEANUP LEAVES THIS SET. It is
    /// not a completed cleanup, not a settlement, and not a state anything
    /// here resolves: the number simply stays this connection's.
    unfinished: bool,
}

/// Why a client number could not be taken.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateNumberRefusal {
    /// A connection still holds it.
    ///
    /// THE LIVE-ROW CASE IS ANSWERED BEFORE THIS. Publication finds a live row
    /// under the client table and refuses as a duplicate there, so reaching
    /// this means the row is gone and the number is STILL not free: the
    /// connection that had it is running the effects keyed by it, or ended
    /// without establishing that reuse is safe.
    ///
    /// NOT A COMPLETED CLEANUP AND NOT A SETTLEMENT. It says this number is
    /// still somebody's.
    Excluded,
    /// The occupancy record could not be read.
    Unreadable,
}

/// The right to occupy one client number, held by whoever must end it.
///
/// HELD BY THE CLEANUP RECORD, which the connection's keeper owns, so losing
/// an operation view -- or the route row -- does not give the number back.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Held by a caller no production site has yet.
struct PrivateNumberRight {
    occupancy: PrivateNumberOccupancy,
    client: XServerFrontendClientId,
    incarnation: std::sync::Weak<std::sync::OnceLock<PrivateAppliedClientState>>,
}

/// One registry's record of who holds which number.
#[cfg(unix)]
#[derive(Clone)]
pub(crate) struct PrivateNumberOccupancy {
    held: Arc<Mutex<BTreeMap<XServerFrontendClientId, PrivateNumberClaim>>>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a caller no production site has yet.
impl PrivateNumberOccupancy {
    fn new() -> Self {
        Self {
            held: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Take this number for this exact connection.
    ///
    /// BEFORE ANYTHING IS ESTABLISHED UNDER IT. This runs before the recovery
    /// ledger registers the client and before a writer is expected for it, not
    /// merely before the row goes in: those reset state keyed by the number,
    /// and a predecessor whose cleanup has not finished would have them reset
    /// under it.
    ///
    /// THE LOCK IS HELD FOR THIS AND NOTHING ELSE. It is never held across a
    /// cleanup body, a send, a gate wait, a home borrow or a callback, so one
    /// connection's slow ending does not queue unrelated connections behind
    /// it.
    fn claim(
        &self,
        client: XServerFrontendClientId,
        incarnation: &Arc<std::sync::OnceLock<PrivateAppliedClientState>>,
    ) -> Result<PrivateNumberRight, PrivateNumberRefusal> {
        let Ok(mut held) = self.held.lock() else {
            return Err(PrivateNumberRefusal::Unreadable);
        };
        if held.contains_key(&client) {
            return Err(PrivateNumberRefusal::Excluded);
        }
        held.insert(
            client,
            PrivateNumberClaim {
                incarnation: Arc::downgrade(incarnation),
                clearing: false,
                unfinished: false,
            },
        );
        Ok(PrivateNumberRight {
            occupancy: self.clone(),
            client,
            incarnation: Arc::downgrade(incarnation),
        })
    }

    /// Whether this number is held, and by an ended connection.
    fn state_of(&self, client: XServerFrontendClientId) -> Option<(bool, bool)> {
        let held = self.held.lock().ok()?;
        held.get(&client).map(|claim| (claim.clearing, claim.unfinished))
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a caller no production site has yet.
impl PrivateNumberRight {
    /// Open the interval in which this connection's number-keyed effects run.
    ///
    /// VALIDATED AND MARKED IN ONE ACQUISITION. Checking that this right is
    /// still the occupant and then marking it afterwards would leave the
    /// interval between them, which is the interval the whole component is
    /// about.
    ///
    /// The lock is released before any effect runs. What keeps a successor out
    /// is the mark, not the lock.
    fn begin_clearing(&self) -> bool {
        let Ok(mut held) = self.occupancy.held.lock() else {
            return false;
        };
        match held.get_mut(&self.client) {
            Some(claim)
                if std::ptr::eq(claim.incarnation.as_ptr(), self.incarnation.as_ptr()) =>
            {
                claim.clearing = true;
                true
            }
            // Not this connection's number any more. A stale or repeated
            // request must not reacquire an incarnation's right and run by
            // number against whoever holds it now.
            _ => false,
        }
    }

    /// Give the number back, if this connection established that it is safe.
    ///
    /// RETURNING FROM A BEST-EFFORT BODY IS NOT THAT PROOF. Where an effect
    /// could not be performed -- a table nobody could read, a removal that did
    /// not happen -- this leaves the number held and says so. Nothing here
    /// resolves that state; it is a fact about this connection that the
    /// namespace has to keep.
    ///
    /// THE OCCUPANT CHECK HERE IS DEFENSIVE, and no caller can currently fail
    /// it. The cleanup reaches this only after `begin_clearing` found this
    /// right to be the occupant, and while a claim is held no second right for
    /// that number exists: `claim` refuses an occupied number, and the only
    /// removal is this method. It is kept because the two acquisitions are
    /// separate and the rule -- act on the connection, not on the number --
    /// should hold at each one on its own terms.
    fn finish(&self, established: bool) {
        let Ok(mut held) = self.occupancy.held.lock() else {
            return;
        };
        let ours = held
            .get(&self.client)
            .is_some_and(|claim| {
                std::ptr::eq(claim.incarnation.as_ptr(), self.incarnation.as_ptr())
            });
        if !ours {
            return;
        }
        if established {
            held.remove(&self.client);
        } else if let Some(claim) = held.get_mut(&self.client) {
            claim.clearing = false;
            claim.unfinished = true;
        }
    }

    /// Give back a number nothing was ever established under.
    ///
    /// FOR A PUBLICATION THAT FAILED, and only that: nothing ran under this
    /// number, so there is nothing whose end has to be established first.
    fn relinquish_unpublished(&self) {
        self.finish(true);
    }
}
