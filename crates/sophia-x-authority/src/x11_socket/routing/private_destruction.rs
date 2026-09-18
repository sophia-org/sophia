// What a registration's destruction decides, and where that decision is kept.
//
// Split by subject from the registration's `Drop`: the handle is what a caller
// drops, and this is the policy its dropping applies. It reaches exactly two
// things the registration was given before its row was published -- the
// custody it reserved and the cleanup record it shares with that custody's
// keeper -- and nothing that is looked up by number afterwards.
//
// THE ORDER IS THE POLICY. Destruction first closes this connection's
// registered startup admission and tells whatever start was admitted to stop,
// through the pair that start published; only then does it learn what the
// slot held. It runs the synchronous cleanup this connection always ran ONLY
// when that decision establishes that nothing was ever started. Everything
// else -- a worker running, a handle handed on, a boundary that could not be
// read, a decision that was interrupted, a custody that cannot be reached --
// is left where it already is: the cleanup duty and the client number stay
// with the custody's external keeper, and this frame records that it left
// them there.
//
// NOTHING HERE EXECUTES A DEFERRED DUTY, and nothing here joins. When the
// deferred cleanup runs is a later boundary's question.

/// Why a registration's destruction left its cleanup with the custodian.
///
/// THE DISTINCTIONS ARE KEPT, NOT COLLAPSED. A reader that later executes the
/// deferred duty needs to know whether there is a handle to collect, whether
/// somebody else already has it, or whether nothing about the worker could be
/// established at all.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateDestructionDeferral {
    /// A worker was started and its handle is still in this connection's slot.
    WorkerRunning,
    /// A worker was started and its handle has gone to whoever joins it.
    WorkerHandedOn,
    /// The slot had already been marked departing by an ask that did not go
    /// through the registered boundary. What it found is not known here.
    AlreadyDeparting,
    /// The slot could not be read, so what it held is not established.
    SlotUnreadable,
    /// An earlier registered departure had closed admission and not yet
    /// recorded a decision -- before or during its cancellation, or in its
    /// later slot wait -- or was interrupted anywhere in that span. This ask
    /// did not start a second decision; it did assert the stop.
    Deciding,
    /// The departure boundary could not be read. The bound stop was still
    /// asserted; what the slot held is not established.
    BoundaryUnreadable,
    /// The custody this registration reserved could not be reached.
    ///
    /// NOT AN ABSENCE OF A WORKER. A keeper that is gone says nothing about
    /// the threads it admitted, so this is deferred like every other case
    /// that established nothing.
    SourceUnreachable,
}

/// What one registration's destruction decided.
///
/// ONE FACT, PUBLISHED ONCE, AFTER THE REQUEST THAT LED TO IT. This is not a
/// history: a registration is destroyed once, and a second request against
/// the same record finds the first and does nothing.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateDestructionDecision {
    /// The registered decision established that nothing was ever started, so
    /// the synchronous cleanup was requested in the registration's own frame.
    ///
    /// REQUESTED, NOT FINISHED. Whether every number-keyed effect was
    /// performed is what the number's own standing says afterwards.
    Synchronous,
    /// The cleanup duty and the number claim were left with the custodian.
    Deferred(PrivateDestructionDeferral),
}

/// Where a connection's destruction stands.
///
/// THE REQUEST IS A FACT OF ITS OWN, separate from the decision it leads to.
/// Between the two the registration is inside the departure arbitration,
/// which can wait on the slot behind an admitted spawn, and a frame lost
/// there -- an unwind after the boundary is released -- leaves the request
/// standing with no decision. That is visible uncertainty, and it is what a
/// later executor must see rather than an empty cell.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateDestructionStanding {
    /// No registration has asked for this connection's destruction.
    NotRequested,
    /// A registration claimed the destruction and has not published what it
    /// decided: it is between the claim and entering the arbitration, inside
    /// the arbitration (including the slot wait), or its frame was lost
    /// anywhere in that span.
    Requested,
    /// The claimed destruction published its decision.
    Decided(PrivateDestructionDecision),
}

#[cfg(unix)]
impl PrivateDestructionDecision {
    /// What a registered departure's answer means for destruction.
    fn of_departure(departed: PrivateDeparted) -> Self {
        use PrivateDestructionDeferral as Deferral;
        match departed {
            // THE ONE ESTABLISHED FACT THAT PERMITS THE SYNCHRONOUS BODY, and
            // it may be one an earlier ask established: a recorded
            // NothingStarted is still nothing started.
            PrivateDeparted::Decided(PrivateDeparture::NothingStarted)
            | PrivateDeparted::AlreadyDecided(PrivateDeparture::NothingStarted) => {
                Self::Synchronous
            }
            PrivateDeparted::Decided(PrivateDeparture::WorkerRunning)
            | PrivateDeparted::AlreadyDecided(PrivateDeparture::WorkerRunning) => {
                Self::Deferred(Deferral::WorkerRunning)
            }
            PrivateDeparted::Decided(PrivateDeparture::WorkerHandedOn)
            | PrivateDeparted::AlreadyDecided(PrivateDeparture::WorkerHandedOn) => {
                Self::Deferred(Deferral::WorkerHandedOn)
            }
            PrivateDeparted::Decided(PrivateDeparture::AlreadyDeparting)
            | PrivateDeparted::AlreadyDecided(PrivateDeparture::AlreadyDeparting) => {
                Self::Deferred(Deferral::AlreadyDeparting)
            }
            PrivateDeparted::Decided(PrivateDeparture::Unreadable)
            | PrivateDeparted::AlreadyDecided(PrivateDeparture::Unreadable) => {
                Self::Deferred(Deferral::SlotUnreadable)
            }
            PrivateDeparted::Deciding => Self::Deferred(Deferral::Deciding),
            PrivateDeparted::Unreadable => Self::Deferred(Deferral::BoundaryUnreadable),
        }
    }
}

#[cfg(unix)]
impl PrivateRegisteredCustody {
    /// Depart the exact custody this registration reserved, for its
    /// destruction.
    ///
    /// THIS IS THE WHOLE OF WHAT DESTRUCTION MAY REACH THROUGH THE CUSTODY.
    /// The custody is pinned for the departure and let go with it: no slot,
    /// gate, home or reaping record leaves this frame, so this capability is
    /// not a way to start, reap, fence or serve without a lease.
    ///
    /// BY IDENTITY, NOT THROUGH THE INVENTORY. The custody was reserved for
    /// this registration before its row was published and is named here by
    /// pointer; whether its place in the keeper has since been taken by a
    /// replacement does not change whose worker this boundary admitted.
    ///
    /// `None` MEANS THE CUSTODY IS GONE, WHICH ESTABLISHES NOTHING. The
    /// keeper that held it can be dropped without joining what it admitted,
    /// and a worker it admitted may still be running.
    ///
    /// A DEPARTURE FOUND DECIDING IS NOT LEFT TO THE OTHER ASK. `Deciding`
    /// means an earlier ask released the boundary and has not recorded its
    /// decision: it may be before its stop, inside it, or waiting on the
    /// slot afterwards -- or it was interrupted anywhere in that span and
    /// will never send one. Whichever it is, this destruction still owes the
    /// connection's stop, so it is asserted here through the bound pair --
    /// reachable outside the boundary, the same authoritative stop and
    /// notice either way; asserting it again over an ask that already sent
    /// it changes nothing. The answer stays `Deciding`: nothing is reopened,
    /// the slot is not entered and nothing is joined.
    fn depart_for_destruction(&self) -> Option<PrivateDeparted> {
        let custody = self.custody.upgrade()?;
        let departed = custody.depart_registered();
        if let (PrivateDeparted::Deciding, Some((stop, notice))) =
            (departed, custody.bound_pair())
        {
            cancel_connection_worker(&stop, &notice);
        }
        Some(departed)
    }
}

#[cfg(unix)]
impl PrivateCleanupRecord {
    /// Claim this connection's destruction, before anything is decided.
    ///
    /// IN STORAGE RESERVED WITH THE RECORD, before the row was published, and
    /// shared with the custody's keeper: the fact survives the frame that
    /// wrote it. Closing startup admission is a separate fact kept by the
    /// departure boundary; this is the record that destruction was
    /// requested, made before the request enters that boundary.
    ///
    /// A REPEATED REQUEST IS INERT. The first claim stands and `false` says
    /// so; nothing enters the arbitration or runs a body twice on the
    /// strength of asking twice. A poisoned cell is read through: whether a
    /// claim was made is a fact somebody's panic does not change.
    fn claim_destruction(&self) -> bool {
        let mut standing = match self.destruction.lock() {
            Ok(standing) => standing,
            Err(poisoned) => poisoned.into_inner(),
        };
        if *standing != PrivateDestructionStanding::NotRequested {
            return false;
        }
        *standing = PrivateDestructionStanding::Requested;
        true
    }

    /// Publish what the claimed destruction decided.
    ///
    /// ONLY OVER A STANDING REQUEST. A decision published over no claim, or
    /// over one already decided, is somebody else's and is refused.
    fn publish_destruction(&self, decision: PrivateDestructionDecision) -> bool {
        let mut standing = match self.destruction.lock() {
            Ok(standing) => standing,
            Err(poisoned) => poisoned.into_inner(),
        };
        if *standing != PrivateDestructionStanding::Requested {
            return false;
        }
        *standing = PrivateDestructionStanding::Decided(decision);
        true
    }

    /// Where this connection's destruction stands.
    ///
    /// `NotRequested` IS NOT A CLAIM THAT NOTHING RAN: a record acted on
    /// directly, outside a registration, records nothing here.
    #[cfg_attr(not(test), allow(dead_code))] // Read by the executor a later boundary attaches.
    fn destruction_standing(&self) -> PrivateDestructionStanding {
        match self.destruction.lock() {
            Ok(standing) => *standing,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    /// What this connection's destruction decided, if a decision was
    /// published.
    #[cfg_attr(not(test), allow(dead_code))] // Read by the executor a later boundary attaches.
    fn destruction_decision(&self) -> Option<PrivateDestructionDecision> {
        match self.destruction_standing() {
            PrivateDestructionStanding::Decided(decision) => Some(decision),
            PrivateDestructionStanding::NotRequested | PrivateDestructionStanding::Requested => {
                None
            }
        }
    }
}

#[cfg(unix)]
impl XServerFrontendClientRouteRegistration {
    /// Apply the destruction policy for a registration with a private source.
    ///
    /// STOP FIRST, WAIT FOR NOTHING BUT THE SLOT. The registered departure
    /// closes admission, tells an admitted start to stop through the pair it
    /// published, and only then takes the slot; no gate, home, store or
    /// cleanup table is touched before that stop, and nothing here joins.
    ///
    /// THE REQUEST IS CLAIMED BEFORE THE ARBITRATION IS ENTERED, AND THE
    /// DECISION IS PUBLISHED BEFORE THE SYNCHRONOUS BODY RUNS. The arbitration
    /// can wait on the slot, and a frame lost anywhere after the claim leaves
    /// a standing request with no decision -- uncertainty, visibly. A body
    /// interrupted part-way leaves its decision published. The record is the
    /// custody's as much as this registration's, which is what keeps it
    /// after this frame returns.
    ///
    /// THE DEFERRED BRANCH DOES NOTHING ELSE. No standing change, no fence, no
    /// lease transfer, no number-keyed removal, no place return: every one of
    /// those is the synchronous body's, and the synchronous body is not run.
    fn destroy_registered(&self, registered: &PrivateRegisteredCustody) {
        if !self.cleanup.claim_destruction() {
            // Already requested. Whatever the first request decided, or has
            // yet to decide, stands; this one enters nothing.
            return;
        }
        let decision = match registered.depart_for_destruction() {
            Some(departed) => PrivateDestructionDecision::of_departure(departed),
            None => PrivateDestructionDecision::Deferred(
                PrivateDestructionDeferral::SourceUnreachable,
            ),
        };
        if !self.cleanup.publish_destruction(decision) {
            // The claim above is this frame's, so this cannot be refused
            // unless the record was decided out from under it. It is not
            // this frame's decision then, and no body runs on it.
            return;
        }
        if decision == PrivateDestructionDecision::Synchronous {
            self.cleanup.run_synchronous_cleanup();
        }
    }
}
