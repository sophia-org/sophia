// Closing one connection's producer gate once its worker has been joined, and
// keeping exactly what the close returned.
//
// Split from the join by subject: joining is custody of a thread, and this is
// one act performed afterwards over a different capability. Nothing here
// receives, answers, ends or settles anything.

/// How far one fencing attempt has got.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateFencePhase {
    /// No attempt has entered this connection's gate through this record.
    NotAttempted,
    /// An eligible attempt claimed this record and may have entered the gate.
    ///
    /// WRITE-AHEAD INTENT. It is written before the gate is asked, so finding
    /// it says a claim was made and the gate may have been changed -- not that
    /// the gate was entered, and not that anything was closed.
    ///
    /// AN ATTEMPT THAT LEFT THIS BEHIND IS NOT RETRIED. What a close got to is
    /// exactly what this does not say, and asking again on a guess is how an
    /// Established becomes an AlreadyEstablished that nobody established.
    InProgress,
    /// This connection's gate was asked and its answer is here to be read.
    FenceRecorded,
}

/// What a fencing attempt did.
///
/// NONE OF THESE IS THE FENCE ITSELF. What the gate said is in the record; two
/// of these say why it was never asked, and naming them so that a busy or
/// repeated ask could be read as a recorded closure is exactly the confusion
/// this vocabulary exists to prevent.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateFenced {
    /// The gate was asked and what it said is in the record.
    Recorded,
    /// The bound join has published no result yet, so nothing was asked.
    ///
    /// THE GATE IS UNTOUCHED AND THIS RECORD IS STILL ELIGIBLE: an ask made
    /// too early consumes no attempt, and the same record may ask again once
    /// that same join completes.
    JoinIncomplete,
    /// This record has already been asked. The gate was not asked again and
    /// whatever the first attempt established is exactly as it was.
    AlreadyAttempted,
}

/// Where one connection's fence evidence is kept, owned by whoever asked.
///
/// BOUND ONCE, TO ONE JOIN AND ONE GATE. A fencing that took a join record and
/// a gate at every visit would be a record with no connection of its own: it
/// could be given one connection's completed join and another's gate, and
/// close a gate on the strength of a thread that was never serving through it.
/// There is no substitute to pass here.
///
/// THAT THE TWO BELONG TOGETHER IS THE CALLER'S OBLIGATION. Nothing in these
/// types establishes that this gate is the gate of the connection whose worker
/// that record joined; binding them once prevents the pair being changed
/// afterwards, which is a different and smaller thing.
///
/// THE GATE IS HELD, THE REGISTRATION IS NOT. What this needs is the
/// capability, and keeping a registration alive to reach one would keep a
/// connection's whole row alive for the sake of a handle it already published.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
struct PrivateFenceEvidence {
    phase: std::sync::atomic::AtomicU8,
    result: std::sync::OnceLock<PrivateHandoverFence>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl PrivateFenceEvidence {
    fn phase(&self) -> PrivateFencePhase {
        match self.phase.load(Ordering::Acquire) {
            0 => PrivateFencePhase::NotAttempted,
            1 => PrivateFencePhase::InProgress,
            _ => PrivateFencePhase::FenceRecorded,
        }
    }

    /// What the gate said, once it has been asked.
    fn fence(&self) -> Option<PrivateHandoverFence> {
        match self.phase() {
            PrivateFencePhase::FenceRecorded => self.result.get().copied(),
            PrivateFencePhase::NotAttempted | PrivateFencePhase::InProgress => None,
        }
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
struct PrivateFenceRecord<'a> {
    /// The join whose publication makes this record eligible.
    join: &'a PrivateReapingRecord<'a>,
    /// This connection's own gate.
    gate: Arc<PrivateHandoverGate>,
    /// Whether an attempt has claimed this record.
    claimed: AtomicBool,
    /// Where this fencing's evidence is published.
    ///
    /// OWNED IN ITS OWN RIGHT, AND ALLOCATED HERE -- before the gate is ever
    /// asked. Publication cannot wait on another holder: the claim
    /// established exclusivity before the gate was asked, so this is one
    /// write, and a reader consulting the phase finds it. Something that goes
    /// on to keep this clones the handle rather than the record.
    evidence: Arc<PrivateFenceEvidence>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl<'a> PrivateFenceRecord<'a> {
    /// A record for a fencing nobody has asked for yet, over this join and
    /// this connection's gate.
    fn bound_to(join: &'a PrivateReapingRecord<'a>, gate: Arc<PrivateHandoverGate>) -> Self {
        Self {
            join,
            gate,
            claimed: AtomicBool::new(false),
            evidence: Arc::new(PrivateFenceEvidence {
                phase: std::sync::atomic::AtomicU8::new(0),
                result: std::sync::OnceLock::new(),
            }),
        }
    }

    fn phase(&self) -> PrivateFencePhase {
        self.evidence.phase()
    }

    fn fence(&self) -> Option<PrivateHandoverFence> {
        self.evidence.fence()
    }

    /// The join this fencing's eligibility rests on.
    ///
    /// THE FENCE ALREADY NAMES ITS JOIN, so nothing downstream has to be given
    /// one alongside it -- and could not be given an unrelated one as though
    /// it were independent evidence.
    fn join_evidence(&self) -> Arc<PrivateJoinEvidence> {
        self.join.join_evidence()
    }

    /// Close this connection's gate to further handovers, and keep the answer.
    ///
    /// ELIGIBILITY IS THE PUBLISHED JOIN RESULT AND NOTHING ELSE, and that is
    /// a sequencing rule rather than a claim about who the gate holds back.
    ///
    /// TWO SEPARATE FACTS, which an earlier account of this ran together. The
    /// gate serializes closure against PRODUCERS -- whatever is handing
    /// capsules into this connection's queue -- and producers can exist after
    /// a worker has been joined, which is precisely why closing is an act and
    /// not a consequence. The worker is the CONSUMER: joining it says nothing
    /// about who may still hand something over, and closing the gate is what
    /// says that.
    ///
    /// What the join buys is that this connection's own serving has finished
    /// before its endpoint is closed, so nothing here is fencing a wire its
    /// own worker is still writing to. Every weaker sign leaves that open: a
    /// departure notice says a frame went, an empty slot says nobody is
    /// holding a handle, and an unconfirmed attempt says somebody may have
    /// taken one -- none of them says the thread has finished. A join that
    /// reported a panic says it as surely as one that returned, so both are
    /// eligible and the payload is neither inspected nor locked to decide it.
    ///
    /// READ FROM THE PUBLISHED EVIDENCE DIRECTLY. Whether the reaping that
    /// produced it has finished reading its optional exit diagnostics is
    /// nothing to do with this, and waiting on that would make a fence hostage
    /// to a record somebody else is holding.
    ///
    /// NOTHING IS HELD WHILE THE GATE IS ASKED. The gate's own mutex is the
    /// serialization, and a handover already inside it finishes first; holding
    /// this record, the join's, the slot's or anything of the connection's
    /// across that wait would make all of it unreadable behind a producer that
    /// is still going. A caller can read the join result and this attempt's
    /// standing throughout.
    ///
    /// AND THE ANSWER IS RETAINED BEFORE ANYTHING ELSE. Established,
    /// AlreadyEstablished and Unreadable are three different facts and stay
    /// three: a closure somebody else had already made is not one this made,
    /// and a gate whose lock carried a panic out of somebody's handover is not
    /// a fence at all -- it is a recorded outcome saying closure could not be
    /// established over custody nobody stands behind.
    ///
    /// IT STOPS HERE. Closing a gate says no further handover will be
    /// admitted. It receives nothing, answers no completion, settles no
    /// delivery, ends no socket, moves no payload, changes no standing and
    /// authorises no maintenance.
    fn record_fence(&self) -> PrivateFenced {
        // ASKED BEFORE ANY CLAIM IS TAKEN, so an ask made too early costs this
        // record nothing and the same record may ask again later.
        if self.join.result().is_none() {
            return PrivateFenced::JoinIncomplete;
        }
        if self
            .claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return PrivateFenced::AlreadyAttempted;
        }
        // WRITE-AHEAD: the intent is recorded before the gate can be changed.
        self.evidence.phase.store(1, Ordering::Release);

        let fence = self.gate.close();

        // Retained before anything else, and the phase after it, so no reader
        // sees a recorded fence over storage that is still empty.
        let retained = self.evidence.result.set(fence).is_ok();
        debug_assert!(retained, "one claim, one writer, one write");
        self.evidence.phase.store(2, Ordering::Release);
        PrivateFenced::Recorded
    }
}
