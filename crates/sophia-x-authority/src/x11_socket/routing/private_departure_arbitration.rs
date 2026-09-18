// Closing one connection's admission to future starts, and keeping what its
// slot said about departing.
//
// Split by subject from the control context: that context is the capability to
// drive a connection, and this is the decision that there will be no more of
// it. The decision outlives every view that might make it, because a caller
// that has to be told the answer by a return value is a caller that cannot
// find out afterwards.
//
// WHAT IT IS NOT. It runs no cleanup, joins nothing, fences nothing, commits
// nothing, changes no standing and returns no place. Nothing here makes it
// safe to destroy a registration over a live worker.

/// How far one connection's departure has got.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
struct PrivateDepartureState {
    /// Whether a registered start may still be admitted.
    ///
    /// CLOSED ONCE AND NEVER REOPENED. A view ending, unwinding or being
    /// replaced does not restore it: the whole point of the decision is that
    /// something later can rely on it.
    admitted: bool,
    /// The authoritative stop and notice an admitted start published.
    ///
    /// PUBLISHED BEFORE THE START LEAVES THIS BOUNDARY, so a departure that
    /// arrives afterwards can stop that worker without borrowing its home,
    /// entering its gate or reaching the store. `None` means no start was ever
    /// admitted -- not that no worker exists, which is what the slot decision
    /// below is for.
    published: Option<(Arc<AtomicBool>, Arc<PrivateOrderedWake>)>,
    /// Whether a departure has begun and not yet recorded what it found.
    ///
    /// LEGIBLE ON ITS OWN. An attempt interrupted here leaves this set with no
    /// observation, which says a departure may have acted and says nothing
    /// about whether a worker exists.
    deciding: bool,
    /// What the worker slot established, once one ask has established it.
    ///
    /// THE FIRST FACT WINS. A later ask reports it rather than making a second
    /// decision, because there is one departure and a second would be a second
    /// history of the same connection.
    observed: Option<PrivateDeparture>,
}

/// What an ask about this connection's departure established.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateDeparted {
    /// This ask closed admission and made the slot decision. Here is what the
    /// slot held.
    Decided(PrivateDeparture),
    /// A decision was already recorded for this connection, and this is it.
    /// Admission was already closed; nothing was decided again.
    AlreadyDecided(PrivateDeparture),
    /// Admission is closed and an earlier ask is inside its decision. Nothing
    /// here establishes what the slot holds, and nothing was decided twice.
    Deciding,
    /// The arbitration boundary could not be read.
    ///
    /// FAILING CLOSED IS THE STARTUP SIDE OF THIS, not this one: a boundary
    /// nobody can read admits no start, and this ask reports that it
    /// established nothing rather than that nothing was there.
    Unreadable,
}

/// Whether a registered start may proceed.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateStartAdmission {
    /// Admitted, and this start's stop and notice are published where a
    /// departure can reach them.
    Admitted,
    /// This connection has departed. No spawner is called.
    Departed,
    /// The boundary could not be read, so nothing is admitted.
    ///
    /// FAIL CLOSED. A poisoned admission check that let a start through would
    /// be restoring eligibility nobody established.
    Unreadable,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Asked by a caller no production site has yet.
impl PrivateEvidenceCustody {
    /// Ask this connection's admission boundary whether a start may proceed,
    /// publishing that start's stop and notice if it may.
    ///
    /// THE BOUNDARY IS HELD FOR THIS AND NOTHING ELSE. It is not held across
    /// the spawn, across waiting for the worker slot, or while anything
    /// borrows the home, enters the gate or reaches the store or inventory.
    ///
    /// LOCK ORDER. This boundary is taken alone; a caller takes the worker
    /// slot only after releasing it, and nothing takes this boundary while
    /// holding the slot, the home, the gate, the store aggregate or the
    /// custody inventory.
    fn admit_start(
        &self,
        stop: &Arc<AtomicBool>,
        notice: &Arc<PrivateOrderedWake>,
    ) -> PrivateStartAdmission {
        let Ok(mut state) = self.source.departure.lock() else {
            return PrivateStartAdmission::Unreadable;
        };
        if !state.admitted {
            return PrivateStartAdmission::Departed;
        }
        // PUBLISHED BEFORE THIS START LEAVES THE BOUNDARY. A departure that
        // arrives afterwards finds the pair and can stop this worker before it
        // waits for the slot the start is about to hold.
        state.published = Some((Arc::clone(stop), Arc::clone(notice)));
        PrivateStartAdmission::Admitted
    }

    /// Close this connection to further starts and record what its slot held.
    ///
    /// NO PREPARED CONTROL ASSOCIATION IS REQUIRED. A connection that never
    /// became serving, or whose preparation is still resolving, can still be
    /// forbidden a future start -- and forbidding one must not require
    /// entering its home, inventing a stop or making it serve.
    ///
    /// THE ORDER IS THE WHOLE PROTOCOL. Admission is closed and any published
    /// pair is taken under the boundary; the boundary is released; the worker
    /// is stopped and woken through that pair; and only then is the slot
    /// waited for. A departure that waited for the slot before stopping could
    /// be waiting on the very worker it had not told to stop.
    fn depart_registered(&self) -> PrivateDeparted {
        self.depart_registered_through(None)
    }

    /// The same, for a caller that already holds this connection's own pair.
    ///
    /// THE SOURCE'S PAIR WINS. What an admitted start published is this
    /// connection's; a caller's is only used when no start ever published one,
    /// and it is the same authoritative stop and notice either way. Asserting
    /// the same stop again is not a second departure and records no second
    /// history.
    fn depart_registered_through(
        &self,
        supplied: Option<(&Arc<AtomicBool>, &Arc<PrivateOrderedWake>)>,
    ) -> PrivateDeparted {
        let supplied = supplied.map(|(stop, notice)| (Arc::clone(stop), Arc::clone(notice)));
        let pair = {
            let state = match self.source.departure.lock() {
                Ok(state) => state,
                Err(poisoned) => {
                    // THE BOUNDARY IS UNREADABLE, AND A WORKER MAY ALREADY
                    // EXIST. Nothing about admission or about what was
                    // published here can be established -- but this
                    // connection's bound control association is immutable and
                    // does not live behind this lock, so the authoritative
                    // stop it published is still reachable and the worker
                    // admitted through this boundary can still be told to
                    // stop. Failing closed to a NEW start is not a reason to
                    // leave a running one unstopped.
                    //
                    // THE GUARD GOES FIRST. Nothing is read from it, nothing
                    // is written back, and the wake below is published with
                    // this lock released.
                    drop(poisoned);
                    if let Some((stop, notice)) =
                        supplied.as_ref().or(self.bound_pair().as_ref())
                    {
                        cancel_connection_worker(stop, notice);
                    }
                    // STILL UNREADABLE. Cancelling is not a decision: this ask
                    // established nothing about the slot, and says so.
                    return PrivateDeparted::Unreadable;
                }
            };
            let mut state = state;
            if let Some(recorded) = state.observed {
                return PrivateDeparted::AlreadyDecided(recorded);
            }
            if state.deciding {
                // An earlier ask is inside its decision, or was interrupted
                // there. Either way this one does not start a second.
                return PrivateDeparted::Deciding;
            }
            // CLOSED HERE, BEFORE ANYTHING ELSE. From this point no start is
            // admitted, including one whose context was obtained long ago.
            state.admitted = false;
            state.deciding = true;
            state.published.clone()
            // The boundary is released here.
        };
        // STAGE-ONLY SCHEDULING HOOK: the interval after this ask released
        // the boundary and before it sends its stop. A control that needs
        // another ask, or a destruction, to land exactly here arms it on this
        // thread; nothing in production ever does, and an unarmed hook is a
        // thread-local read and nothing else.
        stage_after_departure_boundary();
        // A KNOWN BOUND WORKER IS TOLD TO STOP whatever the slot says
        // afterwards. This reaches nothing but the pair itself.
        if let Some((stop, notice)) = pair.as_ref().or(supplied.as_ref()) {
            cancel_connection_worker(stop, notice);
        }
        // AND ONLY NOW IS THE SLOT WAITED FOR. This can block: an admitted
        // start holds the destination for its whole transaction, and this is
        // the boundary that orders the two.
        let found = decide_departure(self.worker_slot());
        let Ok(mut state) = self.source.departure.lock() else {
            // The decision was made and cannot be recorded. Saying it was not
            // made would be worse than saying nothing: the slot really was
            // told, and this connection really is departing.
            return PrivateDeparted::Unreadable;
        };
        state.deciding = false;
        match state.observed {
            // Somebody recorded first. One departure, one history.
            Some(recorded) => PrivateDeparted::AlreadyDecided(recorded),
            None => {
                state.observed = Some(found);
                PrivateDeparted::Decided(found)
            }
        }
    }

    /// This connection's authoritative pair, from its bound control
    /// association.
    ///
    /// NOT BEHIND THE ARBITRATION BOUNDARY. The association is published once
    /// and never rebound, so it is reachable when that boundary is not -- which
    /// is exactly when a worker that was admitted still needs telling.
    fn bound_pair(&self) -> Option<(Arc<AtomicBool>, Arc<PrivateOrderedWake>)> {
        self.source
            .control
            .get()
            .map(|bound| (Arc::clone(&bound.stop), Arc::clone(&bound.notice)))
    }

    /// Whether this connection still admits a registered start.
    fn startup_admitted(&self) -> Option<bool> {
        match self.source.departure.lock() {
            Ok(state) => Some(state.admitted),
            Err(_) => None,
        }
    }

    /// What this connection's departure established, if a decision was
    /// recorded.
    ///
    /// `None` COVERS TWO DIFFERENT THINGS and says so: no ask has been made,
    /// or one is inside its decision. Neither means no worker exists.
    fn departure_observation(&self) -> Option<PrivateDeparture> {
        match self.source.departure.lock() {
            Ok(state) => state.observed,
            Err(poisoned) => poisoned.into_inner().observed,
        }
    }

    /// The stop and notice an admitted start published here, if one did.
    fn published_stop(&self) -> Option<(Arc<AtomicBool>, Arc<PrivateOrderedWake>)> {
        match self.source.departure.lock() {
            Ok(state) => state.published.clone(),
            Err(poisoned) => poisoned.into_inner().published.clone(),
        }
    }
}

thread_local! {
    /// STAGE-ONLY SCHEDULING HOOK. What `depart_registered_through` runs on
    /// this thread once, after it has released its boundary and before it
    /// sends its stop. Armed only by controls, on the thread that will ask;
    /// production never arms it, so in production this is an empty cell that
    /// is read and left empty.
    static STAGE_AFTER_DEPARTURE_BOUNDARY: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Fire the staged hook, if a control armed one on this thread.
#[cfg(unix)]
fn stage_after_departure_boundary() {
    let hook = STAGE_AFTER_DEPARTURE_BOUNDARY.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}
