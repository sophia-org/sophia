// What a supervisor has been told to look at, and what it is already looking
// at, kept apart from the connections themselves.
//
// NO THREAD LIVES HERE. This is the state a supervisor would consult; nothing
// in it waits, sleeps, spawns or drives anything. What it has to get right is
// narrower than scheduling and harder than it looks: a notice that arrives
// while a pass is failing must not be erased by the failure, and a slot that
// has been reused must not act on what its previous occupant was owed.

/// Where one connection stands with the supervisor.
///
/// FOUR STATES, and the pair in the middle is the point. A slot being worked
/// on is not the same as a slot waiting to be worked on, and a slot that could
/// not be taken is not the same as one nobody has asked about -- collapsing
/// either pair is how a notice goes missing or a pass spins.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No supervisor consults this yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrivateAttentionState {
    /// Nothing to do, and nobody doing anything.
    Idle,
    /// Something to look at, and nobody looking yet.
    Ready,
    /// Being looked at now.
    ///
    /// `dirty` records that something arrived WHILE it was being looked at.
    /// Whatever the pass concludes, that arrival is newer than the conclusion,
    /// so it survives it: this is the erasure that a cleared flag suffers,
    /// written as a state rather than as a count that could overflow.
    InFlight { dirty: bool },
    /// Looked at, and could not be taken.
    ///
    /// NOT COUNTED AS READY, which is what stops a failed attempt spinning:
    /// the predicate goes false and the supervisor sleeps. What brings it back
    /// is whoever was holding it saying so, a later notice, or the sweep.
    Deferred,
}

/// One connection's place with the supervisor.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No supervisor consults this yet.
struct PrivateAttentionSlot {
    /// The occupant living here now, if any.
    ///
    /// OCCUPANCY IS NOT A STATE. A slot with nothing to do and a slot with
    /// nobody in it look the same from the state alone, and treating them as
    /// one let a second admission walk into a live connection's place --
    /// taking its readiness, its notices and its identity with it.
    ///
    /// Cleared the moment its occupant is retired, which is what makes every
    /// notice still in flight for that occupant stale immediately rather than
    /// when somebody happens to move in.
    occupant: Option<u32>,
    /// The name the next occupant would be given.
    ///
    /// Kept when the occupant goes, because the history is what stops a
    /// successor being mistaken for its predecessor. `None` once this slot can
    /// name nobody else.
    next_generation: Option<u32>,
    state: PrivateAttentionState,
}

#[cfg(unix)]
impl PrivateAttentionSlot {
    /// Whether this identity is the occupant living here now.
    ///
    /// EVERY IDENTITY OPERATION ASKS THIS. A generation that matches a slot
    /// nobody lives in is not a match: it is a notice for someone who has
    /// gone, and acting on it manufactures work for a connection that does not
    /// exist.
    fn is_occupant(&self, who: PrivateAttentionIdentity) -> bool {
        self.occupant == Some(who.generation)
    }
}

/// Who a notice or a claim is about.
///
/// CHECKED WHERE WORK IS CONSUMED, not only where it is published: the
/// retirement that makes a notice stale may not have happened when it was
/// written. Every operation below takes one of these and refuses it if the
/// slot has moved on.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No supervisor consults this yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrivateAttentionIdentity {
    slot: usize,
    generation: u32,
}

/// A pass's exclusive hold on one slot.
///
/// NEITHER COPY NOR CLONE, and consumed by whichever ending it reaches: one
/// claim, one outcome. A claim that is dropped without an outcome is an
/// abandoned pass, which is treated as a failure to take -- the conservative
/// reading, because the alternative is a slot marked done by a pass that did
/// not finish.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No supervisor consults this yet.
pub(crate) struct PrivateAttentionClaim {
    who: PrivateAttentionIdentity,
    /// Where to report the outcome. `None` once one has been reported.
    roll: Option<Arc<PrivateAttention>>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No supervisor consults this yet.
impl PrivateAttentionClaim {
    /// This pass took the record and is done with it.
    fn took_it(mut self) -> bool {
        let Some(roll) = self.roll.take() else {
            return false;
        };
        roll.conclude(self.who, true)
    }

    /// This pass could not take the record.
    fn could_not(mut self) -> bool {
        let Some(roll) = self.roll.take() else {
            return false;
        };
        roll.conclude(self.who, false)
    }

    fn who(&self) -> PrivateAttentionIdentity {
        self.who
    }
}

#[cfg(unix)]
impl Drop for PrivateAttentionClaim {
    /// AN ABANDONED PASS IS A FAILED ONE. A claim that reaches here still
    /// holding its destination reported no outcome -- it was dropped, or
    /// unwound through -- and the conservative reading is the only safe one:
    /// marking it done would leave a connection nobody looked at recorded as
    /// looked at. It is parked instead, and whatever arrived meanwhile still
    /// revives it.
    fn drop(&mut self) {
        if let Some(roll) = self.roll.take() {
            roll.conclude(self.who, false);
        }
    }
}

/// What the supervisor has been told, for every connection at once.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No supervisor consults this yet.
struct PrivateAttentionRoll {
    slots: Vec<PrivateAttentionSlot>,
    /// How many slots are Ready. The supervisor's predicate, kept rather than
    /// counted, so asking costs nothing.
    ready: usize,
    /// When the deferred sweep is next due.
    ///
    /// ABSOLUTE, AND NOT RESTARTED. A deadline recomputed on every wake is
    /// postponed for ever by unrelated traffic, which is exactly when the
    /// deferred work most needs looking at.
    sweep_due: std::time::Instant,
}

/// The supervisor's side of every connection.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No supervisor consults this yet.
pub(crate) struct PrivateAttention {
    roll: Mutex<PrivateAttentionRoll>,
    ready: std::sync::Condvar,
}

/// How long deferred work waits for the sweep when nothing else revives it.
///
/// THIS IS A POLL, and naming it one is the point: everything that reports --
/// a notice, an exit, a stop, a holder releasing a record -- revives its slot
/// immediately, and this is for the case that reports nothing, which is a
/// holder that died while something was deferred on it. Provisional, and no
/// claim about service latency follows from it.
#[cfg(unix)]
const PRIVATE_ATTENTION_SWEEP: std::time::Duration = std::time::Duration::from_millis(250);

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // No supervisor consults this yet.
impl PrivateAttention {
    /// Room for every connection this instance may admit, and no more.
    ///
    /// Allocated once. Nothing below allocates, so a notice cannot fail for
    /// want of memory at the moment something needs to be remembered.
    fn with_connections(connections: NonZeroUsize, now: std::time::Instant) -> Option<Self> {
        let mut slots = Vec::new();
        slots.try_reserve_exact(connections.get()).ok()?;
        slots.resize_with(connections.get(), || PrivateAttentionSlot {
            occupant: None,
            next_generation: Some(0),
            state: PrivateAttentionState::Idle,
        });
        Some(Self {
            roll: Mutex::new(PrivateAttentionRoll {
                slots,
                ready: 0,
                sweep_due: now + PRIVATE_ATTENTION_SWEEP,
            }),
            ready: std::sync::Condvar::new(),
        })
    }

    /// Give this slot to a new occupant.
    ///
    /// The generation moves, which is what makes everything the previous
    /// occupant was owed stale. A slot whose generations are exhausted is
    /// RETIRED RATHER THAN WRAPPED: a wrapped generation is precisely a stale
    /// notice that passes the check, and one connection's worth of capacity is
    /// a smaller cost than an identity that lies.
    fn admit(&self, slot: usize) -> Option<PrivateAttentionIdentity> {
        let mut roll = self.roll.lock().ok()?;
        let held = roll.slots.get_mut(slot)?;
        // SOMEBODY LIVES HERE. A second admission is refused rather than
        // served: moving in over a live connection takes its identity, its
        // readiness and everything it had been told, and the connection it
        // took them from is still there.
        if held.occupant.is_some() {
            return None;
        }
        let generation = held.next_generation?;
        held.occupant = Some(generation);
        // Spent when it is handed out, so the next one is refused at the
        // handing-out rather than wrapping into a name already used.
        held.next_generation = generation.checked_add(1);
        held.state = PrivateAttentionState::Idle;
        Some(PrivateAttentionIdentity { slot, generation })
    }

    /// Say there is something to look at here.
    ///
    /// Coalescing: a slot already waiting stays one slot waiting. What is NOT
    /// coalesced away is a notice arriving while a pass is in flight -- that
    /// one is newer than whatever the pass is about to conclude, so it is
    /// remembered as `dirty` and outlives the conclusion.
    fn flag(&self, who: PrivateAttentionIdentity) -> bool {
        let Ok(mut roll) = self.roll.lock() else {
            return false;
        };
        let Some(held) = roll.slots.get_mut(who.slot) else {
            return false;
        };
        if !held.is_occupant(who) {
            return false;
        }
        match held.state {
            PrivateAttentionState::Idle | PrivateAttentionState::Deferred => {
                held.state = PrivateAttentionState::Ready;
                roll.ready = roll.ready.saturating_add(1);
            }
            PrivateAttentionState::InFlight { .. } => {
                held.state = PrivateAttentionState::InFlight { dirty: true };
            }
            PrivateAttentionState::Ready => {}
        }
        self.ready.notify_all();
        true
    }

    /// Say that a record somebody may be waiting on has been released.
    ///
    /// CALLED BY WHOEVER WAS HOLDING IT, after actually unlocking it. The
    /// interest that makes this matter was armed by the claim itself, before
    /// the attempt to take the record: the slot is InFlight from that moment,
    /// so a release that lands during a failing attempt is remembered as
    /// dirty and the defer that follows does not park it. A release that lands
    /// after the attempt gave up finds it Deferred and makes it Ready.
    ///
    /// THE SUPERVISOR DOES NOT CALL THIS. It took the record because it was
    /// told to; finishing with the record is not news, and a reader that
    /// re-announced itself would keep its own predicate true for ever.
    ///
    /// A KNOWN ROUGH EDGE: this makes a slot ready even when nobody was
    /// waiting on the record -- an actor releasing one that no pass had
    /// deferred on still says so. That costs a pass that finds nothing, which
    /// is harmless here and would not be once an actor schedules on it: a
    /// visit that finds nothing and releases, announcing itself as it goes, is
    /// a loop with no progress in it. Whoever wires that scheduling has to
    /// narrow this to releases somebody is actually waiting on.
    fn released(&self, who: PrivateAttentionIdentity) -> bool {
        self.flag(who)
    }

    /// Take the next slot waiting to be looked at.
    ///
    /// ARMS THE RELEASE INTEREST as it goes: from here until an outcome, this
    /// slot is InFlight, which is what lets a release or a notice arriving
    /// during the attempt be remembered rather than lost.
    /// NO FAIRNESS HERE, and none claimed. It takes the first waiting slot it
    /// finds, so a connection whose slot sits above a steadily busy one is
    /// reached only when that one falls quiet. That is a scheduling property
    /// and belongs with whoever schedules; this says only which slots are
    /// waiting.
    fn claim_next(self: &Arc<Self>) -> Option<PrivateAttentionClaim> {
        let mut roll = self.roll.lock().ok()?;
        let slot = roll
            .slots
            .iter()
            .position(|held| held.state == PrivateAttentionState::Ready)?;
        // A slot nobody lives in cannot be waiting for anything, and if it
        // somehow is, no pass may be made for it.
        let generation = roll.slots[slot].occupant?;
        roll.slots[slot].state = PrivateAttentionState::InFlight { dirty: false };
        roll.ready = roll.ready.saturating_sub(1);
        Some(PrivateAttentionClaim {
            who: PrivateAttentionIdentity { slot, generation },
            roll: Some(self.clone()),
        })
    }

    /// How many slots are waiting to be looked at.
    fn waiting(&self) -> Option<usize> {
        self.roll.lock().ok().map(|roll| roll.ready)
    }

    /// What this slot is, for a control or a report.
    fn state_of(&self, who: PrivateAttentionIdentity) -> Option<PrivateAttentionState> {
        let roll = self.roll.lock().ok()?;
        let held = roll.slots.get(who.slot)?;
        held.is_occupant(who).then_some(held.state)
    }

    /// Finish a claim, one way or the other.
    ///
    /// STALE CLAIMS CHANGE NOTHING. The generation is checked here as well as
    /// where the claim was made, because a slot can be retired and given to
    /// someone else while a pass is in flight -- and a pass that then wrote
    /// its outcome would be writing it about a connection it never saw.
    fn conclude(&self, who: PrivateAttentionIdentity, took_it: bool) -> bool {
        let Ok(mut roll) = self.roll.lock() else {
            return false;
        };
        let Some(held) = roll.slots.get_mut(who.slot) else {
            return false;
        };
        if !held.is_occupant(who) {
            return false;
        }
        let PrivateAttentionState::InFlight { dirty } = held.state else {
            return false;
        };
        // SOMETHING ARRIVED WHILE THIS PASS RAN, so the pass's conclusion is
        // already out of date -- whether the pass succeeded or not.
        held.state = if dirty {
            PrivateAttentionState::Ready
        } else if took_it {
            PrivateAttentionState::Idle
        } else {
            // Could not be taken, and nothing new: park it. Not Ready, which
            // is what stops a failed attempt spinning.
            PrivateAttentionState::Deferred
        };
        if held.state == PrivateAttentionState::Ready {
            roll.ready = roll.ready.saturating_add(1);
            self.ready.notify_all();
        }
        true
    }

    /// Bring deferred work back, if the deadline has come.
    ///
    /// THE DEADLINE IS ABSOLUTE and is not pushed back by anything else
    /// happening. A sweep rescheduled on every wake is postponed indefinitely
    /// by unrelated traffic, and busy is exactly when the work nobody revived
    /// has been waiting longest.
    fn sweep_due(&self, now: std::time::Instant) -> bool {
        let Ok(mut roll) = self.roll.lock() else {
            return false;
        };
        if now < roll.sweep_due {
            return false;
        }
        roll.sweep_due = now + PRIVATE_ATTENTION_SWEEP;
        let mut revived = 0;
        for held in &mut roll.slots {
            if held.state == PrivateAttentionState::Deferred {
                held.state = PrivateAttentionState::Ready;
                revived += 1;
            }
        }
        roll.ready = roll.ready.saturating_add(revived);
        if revived > 0 {
            self.ready.notify_all();
        }
        true
    }

    /// Retire this slot for good, or give it up for reuse.
    fn retire(&self, who: PrivateAttentionIdentity, forever: bool) -> bool {
        let Ok(mut roll) = self.roll.lock() else {
            return false;
        };
        let Some(held) = roll.slots.get_mut(who.slot) else {
            return false;
        };
        if !held.is_occupant(who) {
            return false;
        }
        let was_ready = held.state == PrivateAttentionState::Ready;
        // GONE AT ONCE, not when a successor arrives. Leaving the name valid
        // in between let a late notice make an empty slot ready, and a pass
        // then took it -- work invented for a connection that had left.
        held.occupant = None;
        held.state = PrivateAttentionState::Idle;
        if forever {
            held.next_generation = None;
        }
        if was_ready {
            roll.ready = roll.ready.saturating_sub(1);
        }
        true
    }
}
