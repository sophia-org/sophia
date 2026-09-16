// What one connection's worker does while it is running.
//
// Split from startup by subject: starting a worker is a transaction over a
// handle and a permit, and this is the loop that runs afterwards. Nothing here
// creates a thread, owns a registration, or decides when one should exist.

/// Why a worker body never began ordinary serving.
///
/// EACH IS A DIFFERENT FACT, and none of them is "there was no work". A body
/// that reported idleness for any of these would say a connection had nothing
/// owed when what happened was that this worker could not serve it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateWorkerRefusal {
    /// A holder panicked inside the home. Not recovered into serving.
    HomeUnreadable,
    /// The home is live and nothing has been bound into it.
    HomeEmpty,
    /// The connection has ended. Its home belongs to whoever finishes it.
    HomeRetained,
    /// Bound, but no serving owner was built: there is nothing to serve with.
    NotServing,
    /// The owner carries no stop handle.
    ///
    /// A WORKER WITHOUT ONE CANNOT BE TOLD TO STOP, and the production binding
    /// passes none today. Accepting that would be running a thread nothing can
    /// end, so it is refused here rather than started and hoped about.
    NoStop,
    /// The owner's notice is not the one this worker was given.
    ForeignNotice,
    /// The owner's stop is not the one this worker was given.
    ForeignStop,
}

/// What made a worker body stop asking its owner for steps.
///
/// KEPT APART FROM WHAT THE OWNER SAID. A trigger is this body's reason for
/// leaving; the result beside it is the owner's own answer, and synthesising
/// either from the other is how a flag becomes a wire outcome nobody
/// established.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateWorkerTrigger {
    /// The owner's own step ended ordinary serving.
    OwnerStep,
    /// The authoritative stop was set. The owner was asked once for its
    /// departure and what it said is beside this.
    Stopped,
    /// The notice could not be read. The same authoritative stop was set and
    /// the notice woken, and then the owner was asked once.
    NoticeUnreadable,
    /// The senders disappeared before a permit ever arrived.
    ///
    /// NOT A CHANNEL ENDING. Nothing received here, so nothing established
    /// that the queue is finished; what this says is that startup stopped
    /// waiting.
    NeverPermitted,
    /// The step budget ran out with the connection still servable.
    Exhausted,
    /// The home or its owner could not be used at all.
    Ineligible(PrivateWorkerRefusal),
}

/// What came of asking this connection's owner for a step.
///
/// A REFUSAL IS NOT AN ABSENCE. Reporting "could not ask" as "did not ask"
/// loses the one fact a caller needs to act on -- that the home or its owner
/// was in a state nothing may serve through -- and leaves a body looking as
/// though it left for its own reasons.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateWorkerAsk {
    /// What the owner said.
    Said(X11OrderedServeStep),
    /// Why it could not be asked.
    Refused(PrivateWorkerRefusal),
}

/// What a worker body did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PrivateWorkerOutcome {
    /// This body's reason for leaving.
    trigger: PrivateWorkerTrigger,
    /// WHAT CAME OF THIS BODY'S LAST ASK, whichever ask that was.
    ///
    /// For a trigger the owner's own step produced, it is that step. For a
    /// departure it is the departure ask, which is an extra visit made after
    /// the loop and NOT counted against the step budget. For `Exhausted` it is
    /// the last ordinary step taken, which is not nothing: a body that used
    /// its whole budget has been serving.
    ///
    /// `None` MEANS NO ASK WAS EVER MADE OR ATTEMPTED, which is narrower than
    /// it sounds. An ineligible body carries its refusal here, so that is not
    /// one of these. What is left is startup ending before serving began, and
    /// a budget of no steps at all -- which exhausts without asking anything.
    /// It is never a way of saying the owner had nothing to say.
    last: Option<PrivateWorkerAsk>,
}

/// Where a worker body's departure is recorded, owned by whoever started it.
///
/// FIXED, AND NOT IN THE BODY'S FRAME. Whatever a departing worker leaves has
/// to outlive the frame that leaves it, including a frame that unwinds.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
struct PrivateWorkerExit {
    /// The classification, written before `left` on any normal return.
    outcome: Mutex<Option<PrivateWorkerOutcome>>,
    /// That the body's frame is gone, however it went.
    ///
    /// ARMED BEFORE THE FIRST WAIT OR VISIT and published by a guard's drop,
    /// so an unwind publishes it too. IT IS NOT A JOIN: it says the frame
    /// left, not that the thread has been collected, and a caller that treated
    /// it as one would be reading a departure as a reaping.
    ///
    /// AND ITS ABSENCE OF A CLASSIFICATION IS NOT PROOF OF A PANIC. A body
    /// that has published this without an outcome may have unwound, or may be
    /// between the two writes. What establishes a panic is the join.
    left: AtomicBool,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl PrivateWorkerExit {
    fn unstarted() -> Self {
        Self {
            outcome: Mutex::new(None),
            left: AtomicBool::new(false),
        }
    }

    fn left(&self) -> bool {
        self.left.load(Ordering::SeqCst)
    }

    fn outcome(&self) -> Option<PrivateWorkerOutcome> {
        *self
            .outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Publishes a body's departure as its frame goes.
#[cfg(unix)]
struct PrivateWorkerLeaving<'a>(&'a PrivateWorkerExit);

#[cfg(unix)]
impl Drop for PrivateWorkerLeaving<'_> {
    fn drop(&mut self) {
        self.0.left.store(true, Ordering::SeqCst);
    }
}

/// What a worker body runs over, supplied whole by whoever starts it.
///
/// NOTHING HERE IS LOOKED UP. A body that found its own home, notice or stop
/// would be deciding which connection it belongs to, and that decision is the
/// startup transaction's.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Built by a caller no production site has yet.
struct PrivateWorkerBody<'a> {
    /// This connection's home. BORROWED, NEVER TAKEN: the payload stays where
    /// it lives, and this worker is one of the things the relocation exists to
    /// let borrow it.
    home: &'a Arc<PrivateOrderedHome>,
    /// The notice this worker waits on, to be validated against the owner's.
    wake: &'a Arc<PrivateOrderedWake>,
    /// The authoritative stop, to be validated against the owner's.
    stop: &'a Arc<AtomicBool>,
    /// This connection's byte order.
    byte_order: XByteOrder,
    /// This connection's own event sequence, shared with its other writers.
    ///
    /// THE ONE THE DISPATCH ALREADY MAKES, not a mirror of it. A counter of
    /// this body's own would number this connection's ordered events
    /// independently of everything else written to the same wire.
    sequence: &'a Arc<AtomicU16>,
    /// Where this body's departure goes.
    exit: &'a PrivateWorkerExit,
    /// The most ordinary serving steps this body will take.
    ///
    /// A COUNT OF STEPS, NOT A DURATION. Nothing here bounds how long a step
    /// takes: a visit writes to a socket and a wait sleeps until it is woken.
    ///
    /// AND NOT EVERY ASK, EITHER. A departure asks the owner once more after
    /// the loop, and that ask is outside this count: a body told to stop on
    /// its last permitted step must still be able to find out what stopping
    /// means for this connection.
    steps: usize,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Run by a caller no production site has yet.
impl PrivateWorkerBody<'_> {
    /// Serve this connection until something ends it.
    fn run(&self) -> PrivateWorkerOutcome {
        // ARMED FIRST, so nothing between here and the return can leave
        // without saying so.
        let _leaving = PrivateWorkerLeaving(self.exit);
        let outcome = self.serve();
        // THE CLASSIFICATION BEFORE THE DEPARTURE. `_leaving` publishes as it
        // drops, after this; a reader that saw the departure first could find
        // no outcome and conclude an unwind that did not happen.
        *self
            .exit
            .outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(outcome);
        outcome
    }

    fn serve(&self) -> PrivateWorkerOutcome {
        if let Err(refusal) = self.credentials() {
            return PrivateWorkerOutcome {
                trigger: PrivateWorkerTrigger::Ineligible(refusal),
                last: Some(PrivateWorkerAsk::Refused(refusal)),
            };
        }
        match self.await_permit() {
            PrivateWorkerStart::Permitted => {}
            PrivateWorkerStart::Stopped => return self.depart(PrivateWorkerTrigger::Stopped),
            PrivateWorkerStart::NoticeUnreadable => {
                return self.depart(PrivateWorkerTrigger::NoticeUnreadable);
            }
            PrivateWorkerStart::Disappeared => {
                return PrivateWorkerOutcome {
                    trigger: PrivateWorkerTrigger::NeverPermitted,
                    // Never asked: startup ended before a single visit.
                    last: None,
                };
            }
        }
        // KEPT ACROSS THE LOOP, so a body that leaves on its own budget can
        // still say what it was doing. Reporting nothing there would read as a
        // body that never served.
        let mut last = None;
        for _ in 0..self.steps {
            // STOP IS ASKED BEFORE EVERY VISIT. It outranks a permit, and a
            // worker told to stop must not take this connection's output for
            // one more event first.
            if self.stop.load(Ordering::SeqCst) {
                return self.depart(PrivateWorkerTrigger::Stopped);
            }
            let step = match self.visit() {
                Ok(step) => {
                    last = Some(PrivateWorkerAsk::Said(step));
                    step
                }
                Err(refusal) => {
                    return PrivateWorkerOutcome {
                        trigger: PrivateWorkerTrigger::Ineligible(refusal),
                        last: Some(PrivateWorkerAsk::Refused(refusal)),
                    };
                }
            };
            match step {
                // Progress. Nothing to wait for: ask again.
                X11OrderedServeStep::Advanced | X11OrderedServeStep::Flushed => continue,
                // Nothing to do right now, which is the only step that waits.
                X11OrderedServeStep::Idle => match self.wait() {
                    PrivateWorkerWait::LookAgain => continue,
                    PrivateWorkerWait::Stopped => {
                        return self.depart(PrivateWorkerTrigger::Stopped);
                    }
                    PrivateWorkerWait::NoticeUnreadable => {
                        return self.depart(PrivateWorkerTrigger::NoticeUnreadable);
                    }
                },
                // EVERY OTHER STEP ENDS ORDINARY SERVING, with its own
                // distinction kept. Nothing here restarts, retries a
                // publication, replays, unbars a wire, resets a cap or decides
                // a close: what the owner said is what is reported.
                X11OrderedServeStep::Unanswered
                | X11OrderedServeStep::AdmissionRefused(_)
                | X11OrderedServeStep::TransportUnavailable
                | X11OrderedServeStep::Unterminated
                | X11OrderedServeStep::WireBarred
                | X11OrderedServeStep::Closing
                | X11OrderedServeStep::Stopped
                | X11OrderedServeStep::Ended { .. } => {
                    return PrivateWorkerOutcome {
                        trigger: PrivateWorkerTrigger::OwnerStep,
                        last: Some(PrivateWorkerAsk::Said(step)),
                    };
                }
            }
        }
        PrivateWorkerOutcome {
            trigger: PrivateWorkerTrigger::Exhausted,
            last,
        }
    }

    /// Check that this body may serve this connection, and that the notice and
    /// stop it was given are the owner's own.
    ///
    /// BEFORE ANYTHING IS CONSUMED. A refusal here has received nothing,
    /// written nothing and taken nothing out of any queue.
    fn credentials(&self) -> Result<(), PrivateWorkerRefusal> {
        let found = self.home.borrow_live(|payload| {
            let PrivateOrderedContinuation::Serving { owner, .. } = payload else {
                return Err(PrivateWorkerRefusal::NotServing);
            };
            let Some(stop) = owner.stop.as_ref() else {
                return Err(PrivateWorkerRefusal::NoStop);
            };
            if !Arc::ptr_eq(&owner.wake, self.wake) {
                return Err(PrivateWorkerRefusal::ForeignNotice);
            }
            if !Arc::ptr_eq(stop, self.stop) {
                return Err(PrivateWorkerRefusal::ForeignStop);
            }
            Ok(())
        });
        match found {
            PrivateHomeBorrow::Acted(result) => result,
            PrivateHomeBorrow::Retained => Err(PrivateWorkerRefusal::HomeRetained),
            PrivateHomeBorrow::Empty => Err(PrivateWorkerRefusal::HomeEmpty),
            PrivateHomeBorrow::Unreadable => Err(PrivateWorkerRefusal::HomeUnreadable),
        }
    }

    /// One step of this connection's own serving, through its own owner.
    ///
    /// ELIGIBILITY IS CHECKED IN THE ACQUISITION THE VISIT USES. Asking first
    /// and acting afterwards would leave an interval in which the connection
    /// ends, and this would then serve through a home that has been handed to
    /// whoever finishes it.
    fn visit(&self) -> Result<X11OrderedServeStep, PrivateWorkerRefusal> {
        let order = self.byte_order;
        // Read for this visit. How a connection's counter becomes the wire's
        // sequence is the encoding's business, not this body's; what this does
        // is pass the connection's own rather than a constant.
        let sequence = self.sequence.load(Ordering::SeqCst);
        let found = self.home.borrow_live(|payload| {
            let PrivateOrderedContinuation::Serving { owner, .. } = payload else {
                return Err(PrivateWorkerRefusal::NotServing);
            };
            Ok(owner.serve_one(order, sequence))
        });
        match found {
            PrivateHomeBorrow::Acted(result) => result,
            PrivateHomeBorrow::Retained => Err(PrivateWorkerRefusal::HomeRetained),
            PrivateHomeBorrow::Empty => Err(PrivateWorkerRefusal::HomeEmpty),
            PrivateHomeBorrow::Unreadable => Err(PrivateWorkerRefusal::HomeUnreadable),
        }
    }

    /// Ask the owner once for its departure, and report what it said.
    ///
    /// THE OWNER'S OWN RESULT, NOT A FLAG READ BACK. Synthesising `Stopped`
    /// from the stop handle would report a wire outcome nobody established --
    /// and the owner may well have something else to say, a begun frame or a
    /// close already under way among them.
    ///
    /// STOP IS ALREADY VISIBLE TO IT before this is called, including to its
    /// control-priority wait, so this is not a licence to write normally.
    fn depart(&self, trigger: PrivateWorkerTrigger) -> PrivateWorkerOutcome {
        if trigger == PrivateWorkerTrigger::NoticeUnreadable {
            // THE SAME AUTHORITATIVE STOP, SET BEFORE THE OWNER IS ASKED. A
            // notice nobody stands behind leaves this connection with no way
            // to be woken, so what this worker does about it is tell the
            // connection to stop -- its own stop, not a fresh flag nothing
            // else reads -- and wake whatever else may be waiting on the
            // notice. The wake is a signal only: the level is in the state
            // nobody can read, and this does not pretend otherwise.
            //
            // AND IT IS SET BEFORE THE ASK, so what the owner reports is a
            // connection already stopped rather than one this call is about to
            // stop. Asking first would get an ordinary answer to a question
            // that is not ordinary.
            self.stop.store(true, Ordering::SeqCst);
            self.wake.ready.notify_all();
        }
        PrivateWorkerOutcome {
            trigger,
            // AN OWNER THAT CANNOT BE USED SAYS WHY, and that is kept. Turning
            // the refusal into an absence here erased the one fact a caller
            // has to act on -- that the home was in a state nothing may serve
            // through -- and left a departure looking ordinary.
            last: Some(match self.visit() {
                Ok(step) => PrivateWorkerAsk::Said(step),
                Err(refusal) => PrivateWorkerAsk::Refused(refusal),
            }),
        }
    }

    /// Wait for this connection's startup permit.
    ///
    /// ASKED ONCE, AND SEPARATELY FROM IDLENESS. Work already on the queue is
    /// not a permit -- a worker that served on the strength of it would be
    /// serving before the transaction that owns its handle had finished -- and
    /// it is not a reason to give up either. The pending level is left exactly
    /// as it was found, so whatever put it there still gets its wake.
    fn await_permit(&self) -> PrivateWorkerStart {
        let mut state = match self.wake.state.lock() {
            Ok(state) => state,
            Err(_) => return PrivateWorkerStart::NoticeUnreadable,
        };
        loop {
            // STOP WINS OVER A PERMIT, and is asked first for that reason.
            if self.stop.load(Ordering::SeqCst) {
                return PrivateWorkerStart::Stopped;
            }
            if state.started {
                return PrivateWorkerStart::Permitted;
            }
            if state.gone {
                // Nothing will arrive and nothing permitted this. Startup
                // stops waiting; what is on the queue is still on the queue.
                return PrivateWorkerStart::Disappeared;
            }
            state = match self.wake.ready.wait(state) {
                Ok(state) => state,
                Err(_) => return PrivateWorkerStart::NoticeUnreadable,
            };
        }
    }

    /// Wait until there is a reason to look again.
    ///
    /// ONLY IDLENESS COMES HERE. A step that made progress has a queue to go
    /// back to, and a step that ended serving has nothing to wait for.
    fn wait(&self) -> PrivateWorkerWait {
        let mut state = match self.wake.state.lock() {
            Ok(state) => state,
            Err(_) => return PrivateWorkerWait::NoticeUnreadable,
        };
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return PrivateWorkerWait::Stopped;
            }
            if state.pending {
                // CONSUMED UNDER THIS MUTEX, before it is released. A level
                // left set is a reason to look again that nothing cleared, so
                // the next idle step would find it and return immediately, and
                // the one after that, for as long as the budget lasted.
                state.pending = false;
                return PrivateWorkerWait::LookAgain;
            }
            if state.gone {
                // A HINT, NOT A FINDING. That every sender has gone is a
                // reason to look; what establishes that the queue is finished
                // is the owner's own receive, and it is about to make one.
                return PrivateWorkerWait::LookAgain;
            }
            // THE STARTED LEVEL IS STICKY AND IS NOT A REASON. It stays set
            // for this connection's whole life, so waking on it would make
            // every idle step return at once.
            state = match self.wake.ready.wait(state) {
                Ok(state) => state,
                // POISON ON REACQUISITION IS THE SAME FACT AS POISON ON THE
                // WAY IN, and is classified the same way. A wait that treated
                // it as a spurious wake would go round again on a guard
                // recovered from a panic.
                Err(_) => return PrivateWorkerWait::NoticeUnreadable,
            };
        }
    }
}

/// How waiting for a permit ended.
#[cfg(unix)]
enum PrivateWorkerStart {
    Permitted,
    Stopped,
    NoticeUnreadable,
    Disappeared,
}

/// How waiting for something to do ended.
#[cfg(unix)]
enum PrivateWorkerWait {
    LookAgain,
    Stopped,
    NoticeUnreadable,
}
