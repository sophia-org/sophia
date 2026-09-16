// Where one connection's ordered output lives, from before it is exposed
// until whoever is responsible for it is finished.
//
// Split from the place by subject: a place is capacity in the store, and this
// is the thing that sits in one.

/// Whether a home's connection is still there.
///
/// NOT THE SAME QUESTION AS WHETHER IT HOLDS ANYTHING. A live home may be
/// empty because its connection has not bound yet, and a retained one may be
/// empty because it never did. What this says is who may act on it.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateHomeStanding {
    /// A registration still holds this. It may still bind into it, and a
    /// producer may still be accepted onto the queue inside it.
    Live,
    /// The connection has ended, and what is here is owed to whoever drives
    /// it. Nothing will be bound into it again.
    Retained,
}

/// The one place a connection's ordered output lives.
///
/// SHARED, AND IT HAS TO BE. This used to live in the registration, and
/// teardown moved it into the place the reservation had set aside. That made
/// the registration the only thing able to reach it while the connection ran,
/// and made the move the moment anything else could begin to -- so anything
/// meaning to borrow it later had to be handed the payload rather than a way
/// to reach it, and the hand-over invalidated whatever it was holding.
///
/// Here the home is made when the place is reserved, before the connection is
/// exposed, and the registration, the place it sits in and any later borrower
/// all hold a handle to the same one. NOTHING MOVES when the connection ends.
/// What changes is its standing.
///
/// ONE OWNER, MANY HANDLES. The payload is owned here and borrowed under this
/// lock. A handle is a way to ask, not a copy and not custody, so no number of
/// them can put the same work in two places.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Borrowed by a driver that is not attached yet.
struct PrivateOrderedHome {
    /// Both facts under one lock, because nothing reads one without the other:
    /// what may be done with this depends on its standing, and its standing is
    /// only interesting while there is something here.
    state: Mutex<PrivateOrderedHomeState>,
}

#[cfg(unix)]
struct PrivateOrderedHomeState {
    payload: Option<PrivateOrderedContinuation>,
    standing: PrivateHomeStanding,
}

/// What a home said when something was offered to it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller that is not attached yet.
#[must_use]
enum PrivateHomeBinding {
    /// It is in the home now.
    Bound,
    /// Something is already here. The offer comes back untouched: a home holds
    /// one connection's output, and a second would have to displace the first.
    Occupied(PrivateOrderedContinuation),
    /// The connection this home belongs to has ended. The offer comes back:
    /// binding into a retained home would give work to a connection nobody
    /// will serve, behind whoever is already responsible for finishing it.
    Ended(PrivateOrderedContinuation),
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Borrowed by a driver that is not attached yet.
impl PrivateOrderedHome {
    /// A home for a place that has just been reserved.
    ///
    /// EMPTY AND LIVE. The connection it belongs to has not been exposed yet,
    /// let alone bound, so there is nothing here and nothing has ended.
    fn empty() -> Self {
        Self {
            state: Mutex::new(PrivateOrderedHomeState {
                payload: None,
                standing: PrivateHomeStanding::Live,
            }),
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, PrivateOrderedHomeState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Put this connection's ordered output in its home.
    ///
    /// THE OFFER COMES BACK IF IT IS REFUSED. What is being bound is a queue
    /// with work already accepted onto it; dropping it to report a refusal
    /// would answer a question by destroying the thing the question was about.
    fn bind(&self, payload: PrivateOrderedContinuation) -> PrivateHomeBinding {
        let mut held = self.held();
        match held.standing {
            PrivateHomeStanding::Retained => PrivateHomeBinding::Ended(payload),
            PrivateHomeStanding::Live if held.payload.is_some() => {
                PrivateHomeBinding::Occupied(payload)
            }
            PrivateHomeStanding::Live => {
                held.payload = Some(payload);
                PrivateHomeBinding::Bound
            }
        }
    }

    /// Act on what is here, if anything is.
    ///
    /// BORROWED IN PLACE. Acting on a continuation means calling into its
    /// close, which takes this connection's output and its finalizers; doing
    /// that with the payload in a local would put it in a stack frame across
    /// exactly the calls that can unwind. It never leaves the home.
    fn borrow<R>(&self, act: impl FnOnce(&mut PrivateOrderedContinuation) -> R) -> Option<R> {
        self.held().payload.as_mut().map(act)
    }

    /// Read what is here, saying separately that it could not be read.
    ///
    /// `None` means a holder panicked inside this home: there may well be a
    /// connection in it, and what there is not is a reading of it. `borrow`
    /// deliberately does not make that distinction -- an act that has to go
    /// ahead regardless uses the poisoned guard, as everything else in this
    /// tree does -- and this exists for the caller whose whole subject is
    /// what can be read.
    fn peek<R>(&self, act: impl FnOnce(&PrivateOrderedContinuation) -> R) -> Option<Option<R>> {
        let held = self.state.lock().ok()?;
        Some(held.payload.as_ref().map(act))
    }

    /// Act on this home's own storage.
    ///
    /// FOR THE ONE CALLER THAT REPLACES WHAT IS HERE rather than acting on
    /// what is here: promotion builds a serving owner out of a setup's
    /// transport and puts it back, which is a take and an assignment into this
    /// slot, not a borrow of its contents.
    ///
    /// `None` means a holder panicked inside this home. That is what promotion
    /// has always reported rather than recovering, because what it would go on
    /// to do is build an owner out of a payload nobody can vouch for.
    fn occupy<R>(&self, act: impl FnOnce(&mut Option<PrivateOrderedContinuation>) -> R) -> Option<R> {
        let mut held = self.state.lock().ok()?;
        Some(act(&mut held.payload))
    }

    /// Whether a holder panicked inside this home.
    ///
    /// Asked before an act that will go ahead through the poisoned guard
    /// anyway, so what the act records is what it actually found rather than
    /// what a recovered guard makes it look like.
    fn unreadable(&self) -> bool {
        self.state.is_poisoned()
    }

    /// Whether anything is here.
    fn occupied(&self) -> bool {
        self.held().payload.is_some()
    }

    fn standing(&self) -> PrivateHomeStanding {
        self.held().standing
    }

    /// Whether this home is a retained one holding work.
    ///
    /// The two together, under one acquisition, because that pair is what
    /// retention means and asking separately would let them disagree.
    ///
    /// `None` IF IT CANNOT BE READ, and not `false`. Counting a home a holder
    /// panicked inside as holding nothing publishes a zero for an obligation
    /// that is still owned and still unanswered, which is the one answer a
    /// caller must not be given.
    fn retaining(&self) -> Option<bool> {
        let held = self.state.lock().ok()?;
        Some(held.standing == PrivateHomeStanding::Retained && held.payload.is_some())
    }

    /// Say that the connection this belongs to has ended.
    ///
    /// IDEMPOTENT, and it has to be: teardown is the only caller today, but
    /// what this records is a fact about the connection rather than an act, so
    /// saying it twice must not be different from saying it once.
    ///
    /// Returns whether anything is here to be owed, which is what its caller
    /// has to account for.
    fn retain(&self) -> bool {
        let mut held = self.held();
        held.standing = PrivateHomeStanding::Retained;
        held.payload.is_some()
    }
}
