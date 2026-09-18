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
// THE ENTRY-POINT AUDIT, so that what is covered by what is written down.
//
// Every by-number MUTATION the dispatcher performs for its own client --
// queue_present, register_surface, select_core_events,
// select_xfixes_selection_input, select_randr_input, select_present_input and
// remove_xfixes_selection_client -- is called from the one frame that owns
// that client's registration (connection/dispatch.rs, from the publication
// at the top of serve_x11_core_socket_client_with_trace_observer_and_input to
// the explicit drop near its end) and every such call precedes that drop. The
// registration's Drop is the only production trigger of the cleanup, so on
// the production path no cleanup of this number can run while one of these
// mutations is between capturing its admission and inserting. That is what
// protects them: the registration's lifetime, not this record. The methods
// themselves carry no interval protection, and a caller that held a number's
// admission across its own registration's ending would be unprotected -- no
// such caller exists in this crate, and none is admitted by this component.
//
// Every by-number REMOVAL that is decided earlier than it acts -- a send that
// found its endpoint gone (route_to_client), a client that stopped draining
// its input queue (route_input) and a watcher that stopped draining its
// protocol queue (disconnect_saturated_recipient, through the value
// route_protocol_to_watcher hands back) -- carries the identity it captured
// with the endpoint, and each act compares it under its own acquisition:
// remove_row_of for the row and InputRecovery::disconnect_exact for the
// ledger. Nothing on those paths looks the number up afresh to decide whom
// to act on.
//
// THE ONE BY-NUMBER ACT NOT ON THE DISPATCH THREAD, AND WHAT ORDERS IT. The
// input writer's recovery guard (connection/writers/input.rs) disconnects
// this client by number from the writer thread when that thread ends. It
// cannot outlive the number's release, for two reasons that are both source
// order and neither of which a type enforces: on the orderly path the
// dispatcher calls writers.shut_down() -- which stops and then JOINS every
// writer synchronously -- before it drops route_registration
// (connection/dispatch.rs, the shut_down and the explicit drop near the end
// of serve_x11_core_socket_client_with_trace_observer_and_input); and on
// every early return and unwind the writers' owner is declared after the
// registration, so it is dropped first, and its Drop performs the same
// synchronous shut_down. In both cases the guard's Drop has completed before
// the registration's Drop begins, and while the registration lives its number
// is held, so the guard's act reaches only this connection's entry. The same
// order covers the per-delivery guard on that thread. Worker attachment is
// what would change this order, and it is outside this slice.
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
    standing: PrivateNumberStanding,
}

/// What is known about a held number, and nothing more than is known.
///
/// THE TWO NON-HELD STATES ARE BOTH UNCERTAINTY, not progress. Neither says an
/// ending is running, neither says one finished, and nothing here turns one
/// into the other. They are kept apart because they are reached differently
/// and a report that confused them would be a report about a different
/// failure.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateNumberStanding {
    /// Claimed, and no ending has begun the effects keyed by it.
    Held,
    /// An ending began those effects, and nothing has established an outcome.
    ///
    /// A BODY STILL RUNNING AND A BODY THAT NEVER RETURNED LEAVE THIS SAME
    /// STANDING, because nothing here can tell them apart: this is set before
    /// the first effect and changed only by that same visit returning. It is
    /// therefore not evidence that anything is still running. An unwound
    /// cleanup, a thread that died mid-body and a cleanup in progress are one
    /// state, and the number stays excluded in all three.
    Visiting,
    /// A visit returned without establishing that its effects were performed.
    ///
    /// AN UNREADABLE TABLE, A REFUSED DISCONNECT OR A REMOVAL THAT DID NOT
    /// HAPPEN LEAVES THIS. It is not a completed cleanup, not a settlement,
    /// and not a state anything here resolves -- no timeout, no retry and no
    /// second visit. The number simply stays this connection's, and a
    /// namespace that cannot reissue it is reporting a failure it really had.
    Unestablished,
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
                standing: PrivateNumberStanding::Held,
            },
        );
        Ok(PrivateNumberRight {
            occupancy: self.clone(),
            client,
            incarnation: Arc::downgrade(incarnation),
        })
    }

    /// What is known about this number, if anything holds it.
    fn state_of(&self, client: XServerFrontendClientId) -> Option<PrivateNumberStanding> {
        let held = self.held.lock().ok()?;
        held.get(&client).map(|claim| claim.standing)
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
    /// ONE RIGHT DOES NOT MEAN ONE VISIT. The record this right lives on
    /// outlives the registration -- its keeper holds it -- so the same right
    /// can be asked twice, and two bodies admitted through it would both run
    /// number-keyed effects while each believed itself alone. The first to
    /// return would release the number for both, and the other would then be
    /// acting on whoever took it next. Only a claim that no visit has opened
    /// admits one.
    ///
    /// A REFUSAL IS NOT A FAILURE REPORT. It says this right is not the
    /// occupant, or that a visit has already been opened on it. Neither of
    /// those is something a second visit could put right, which is why there
    /// is no waiting here and no retry.
    ///
    /// The lock is released before any effect runs. What keeps a successor out
    /// is the standing, not the lock.
    fn begin_clearing(&self) -> bool {
        let Ok(mut held) = self.occupancy.held.lock() else {
            return false;
        };
        let Some(claim) = held.get_mut(&self.client) else {
            return false;
        };
        // Not this connection's number any more. A stale or repeated request
        // must not reacquire a relinquished incarnation's right and run by
        // number against whoever holds it now.
        if !std::ptr::eq(claim.incarnation.as_ptr(), self.incarnation.as_ptr()) {
            return false;
        }
        if claim.standing != PrivateNumberStanding::Held {
            return false;
        }
        claim.standing = PrivateNumberStanding::Visiting;
        true
    }

    /// Give the number back, if this connection established that it is safe.
    ///
    /// RETURNING FROM A BEST-EFFORT BODY IS NOT THAT PROOF. Where an effect
    /// could not be performed -- a table nobody could read, a removal that did
    /// not happen -- this leaves the number held and says so. Nothing here
    /// resolves that state; it is a fact about this connection that the
    /// namespace has to keep.
    ///
    /// THE OCCUPANT CHECK HERE IS EXERCISED AT ITS OWN SEAM, WITH THAT SCOPE.
    /// On the production call graph nothing reaches this with a foreign
    /// occupant: a visit opens only from `Held` under its own identity, and
    /// the only removal during a visit is that visit's own return, so no
    /// second body can be inside when a successor's claim goes in. The two
    /// acquisitions being separate is therefore not, by itself, a release
    /// route. What the check is for is a right that reports again after its
    /// visit has returned and the number has been reissued -- which the
    /// control that retains an old right and makes it report late arranges
    /// directly. Releasing then would free a claim this right never made.
    fn finish(&self, established: bool) {
        let Ok(mut held) = self.occupancy.held.lock() else {
            return;
        };
        let Some(claim) = held.get_mut(&self.client) else {
            return;
        };
        if !std::ptr::eq(claim.incarnation.as_ptr(), self.incarnation.as_ptr()) {
            return;
        }
        match (established, claim.standing) {
            // The visit this right opened, returning with every effect
            // performed. That is the only thing that frees a number.
            (true, PrivateNumberStanding::Visiting) => {
                held.remove(&self.client);
            }
            (false, PrivateNumberStanding::Visiting) => {
                claim.standing = PrivateNumberStanding::Unestablished;
            }
            // A report from no visit of this right establishes nothing, and an
            // already unestablished claim is not made good by a later report
            // saying otherwise.
            _ => {}
        }
    }

    /// Give back a number nothing was ever established under.
    ///
    /// FOR A PUBLICATION THAT FAILED, and only that: nothing ran under this
    /// number, so there is nothing whose end has to be established first.
    ///
    /// ONLY FROM UNVISITED, which is what makes that claim checkable rather
    /// than asserted. A number some ending has already been inside is not an
    /// unpublished one, and this is not a way to release it.
    fn relinquish_unpublished(&self) {
        let Ok(mut held) = self.occupancy.held.lock() else {
            return;
        };
        if held.get(&self.client).is_some_and(|claim| {
            std::ptr::eq(claim.incarnation.as_ptr(), self.incarnation.as_ptr())
                && claim.standing == PrivateNumberStanding::Held
        }) {
            held.remove(&self.client);
        }
    }
}
