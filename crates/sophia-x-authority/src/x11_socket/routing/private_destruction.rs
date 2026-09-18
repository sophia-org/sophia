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
    /// An earlier registered departure is inside its decision, or was
    /// interrupted there. This ask did not start a second.
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
/// ONE FACT, RECORDED ONCE. This is not a history: a registration is
/// destroyed once, and a second request against the same record finds the
/// first decision and does nothing.
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
    fn depart_for_destruction(&self) -> Option<PrivateDeparted> {
        let custody = self.custody.upgrade()?;
        Some(custody.depart_registered())
    }
}

#[cfg(unix)]
impl PrivateCleanupRecord {
    /// Record what this connection's destruction decided.
    ///
    /// IN STORAGE RESERVED WITH THE RECORD, before the row was published, and
    /// shared with the custody's keeper: the fact survives the frame that
    /// wrote it. Closing startup admission is a separate fact kept by the
    /// departure boundary; this is the record that destruction was requested
    /// and what it left behind.
    ///
    /// A REPEATED REQUEST IS INERT. The first decision stands and `false`
    /// says so; nothing runs twice on the strength of asking twice.
    fn request_destruction(&self, decision: PrivateDestructionDecision) -> bool {
        self.destruction.set(decision).is_ok()
    }

    /// What this connection's destruction decided, if it has been requested.
    ///
    /// `None` MEANS NO REQUEST WAS RECORDED. It is not a claim that the
    /// registration still exists, and it is not a claim that nothing ran:
    /// a record acted on directly, outside a registration, records nothing
    /// here.
    #[cfg_attr(not(test), allow(dead_code))] // Read by the executor a later boundary attaches.
    fn destruction_decision(&self) -> Option<PrivateDestructionDecision> {
        self.destruction.get().copied()
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
    /// THE DECISION IS RECORDED BEFORE THE SYNCHRONOUS BODY RUNS, so a body
    /// interrupted part-way leaves a record that cleanup was requested, and a
    /// deferred duty is recorded whether or not anything ever comes back for
    /// it. The record is the custody's as much as this registration's, which
    /// is what keeps it after this frame returns.
    ///
    /// THE DEFERRED BRANCH DOES NOTHING ELSE. No standing change, no fence, no
    /// lease transfer, no number-keyed removal, no place return: every one of
    /// those is the synchronous body's, and the synchronous body is not run.
    fn destroy_registered(&self, registered: &PrivateRegisteredCustody) {
        let decision = match registered.depart_for_destruction() {
            Some(departed) => PrivateDestructionDecision::of_departure(departed),
            None => PrivateDestructionDecision::Deferred(
                PrivateDestructionDeferral::SourceUnreachable,
            ),
        };
        if !self.cleanup.request_destruction(decision) {
            // Already requested. The first decision stands, whatever it was.
            return;
        }
        if decision == PrivateDestructionDecision::Synchronous {
            self.cleanup.run_synchronous_cleanup();
        }
    }
}
