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
    /// THIS CONNECTION'S ONE ATTEMPT IS SPENT. The gate was not asked again
    /// and whatever that attempt established is exactly as it was.
    ///
    /// TWO WAYS TO GET IT, AND THEY ARE DIFFERENT FACTS. This view has already
    /// been asked; or another view took the source's one right, and may still
    /// be inside its attempt. A caller reads what happened from the phase and
    /// the result, not from this.
    AlreadyAttempted,
}

/// Where a fencing's answer is published, owned in its own right.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
struct PrivateFenceEvidence {
    /// WHETHER THE ONE RIGHT TO ATTEMPT THIS CONNECTION'S FENCE IS STILL HERE.
    ///
    /// IT LIVES WHERE THE RESULT LIVES. A claim kept on an operation record is
    /// one a second freshly built view mints for itself, and two views each
    /// holding their own right would each go to the gate -- so the second
    /// would replace an Established answer with AlreadyEstablished and the
    /// connection's record of its own closure would be the wrong one.
    ///
    /// TAKEN, NOT CHECKED. One exchange is the whole acquisition. It is never
    /// given back: once the gate may have been reached, the effect may have
    /// begun, and an attempt that could be retried on that assumption is one
    /// that asks a gate a second time.
    ///
    /// WHAT A SECOND ASK WOULD COST, PRECISELY. The result storage is written
    /// once, so an Established answer is not overwritten by anything. What a
    /// second attempt does is call the gate again -- which answers
    /// AlreadyEstablished -- and then try to publish it: the write is refused,
    /// the single-writer assertion is what notices, and the phase is moved by
    /// a second writer. The record of the closure is not replaced; it is
    /// contradicted by an attempt nobody asked for.
    producer: AtomicBool,
    phase: std::sync::atomic::AtomicU8,
    result: std::sync::OnceLock<PrivateHandoverFence>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl PrivateFenceEvidence {
    /// Storage for a fencing nobody has attempted.
    fn unattempted() -> Self {
        Self {
            producer: AtomicBool::new(true),
            phase: std::sync::atomic::AtomicU8::new(0),
            result: std::sync::OnceLock::new(),
        }
    }

    /// Take the one right to attempt this connection's fence.
    fn take_attempt(&self) -> bool {
        self.producer
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

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

/// One view of a connection's fencing.
///
/// EVERYTHING COMES FROM THE ONE CUSTODY: the join whose publication makes
/// this eligible, the gate this connection's queue was minted with, and the
/// home its answer is published into. There is no join argument and no gate
/// argument, so a view cannot be given one connection's completed join and
/// another's gate and close a gate on the strength of a thread that was never
/// serving through it. That pairing is no longer the caller's obligation
/// because it is no longer the caller's to get wrong.
///
/// THE REGISTRATION IS NOT KEPT ALIVE FOR IT. The gate is the custody's, and
/// reaching one through a registration would keep a connection's whole row
/// alive for the sake of a handle it published before it was exposed.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
struct PrivateFenceRecord<'a> {
    /// The connection this view is about, borrowed from its keeper.
    custody: &'a PrivateEvidenceCustody,
    /// Whether an attempt has claimed THIS VIEW.
    ///
    /// VIEW-LOCAL, AND NOT THE RIGHT TO ATTEMPT. It stops one view being asked
    /// twice at once; what decides which view may reach the gate is the right
    /// that lives in the shared evidence, because a claim minted here is one a
    /// second view mints just as easily.
    claimed: AtomicBool,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl<'a> PrivateFenceRecord<'a> {
    /// A view of this connection's fencing, over its registered source.
    ///
    /// A VIEW, NOT A FRESH ATTEMPT. Making one says nothing about what has
    /// happened: the source it reads may have an attempt in progress or a
    /// result already published, and this is how a later caller observes
    /// either. What is new here is only this view's own claim.
    fn bound_to(custody: &'a PrivateEvidenceCustody) -> Self {
        Self {
            custody,
            claimed: AtomicBool::new(false),
        }
    }

    /// This connection's fence evidence, which its keeper owns.
    fn evidence(&self) -> &'a PrivateFenceEvidence {
        self.custody.fence_evidence()
    }

    fn phase(&self) -> PrivateFencePhase {
        self.evidence().phase()
    }

    fn fence(&self) -> Option<PrivateHandoverFence> {
        self.evidence().fence()
    }

    /// The join this fencing's eligibility rests on.
    ///
    /// THE FENCE ALREADY NAMES ITS JOIN, so nothing downstream has to be given
    /// one alongside it -- and could not be given an unrelated one as though
    /// it were independent evidence.
    fn join_evidence(&self) -> Arc<PrivateJoinEvidence> {
        Arc::clone(self.custody.join())
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
        if self.custody.join().result().is_none() {
            return PrivateFenced::JoinIncomplete;
        }
        if self
            .claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return PrivateFenced::AlreadyAttempted;
        }
        // AND THE CONNECTION'S ONE RIGHT TO ATTEMPT ITS FENCE, which is not
        // this view's to assume. The claim above only stops this view being
        // asked twice; what says this attempt may reach the gate is the right
        // that lives in the shared evidence.
        //
        // IT IS NEVER GIVEN BACK. From here the effect may have begun, so a
        // view that released it would be offering a second close call over an
        // interval nobody can see the end of.
        if !self.evidence().take_attempt() {
            return PrivateFenced::AlreadyAttempted;
        }
        // WRITE-AHEAD: the intent is recorded before the gate can be changed.
        self.evidence().phase.store(1, Ordering::Release);

        let fence = self.custody.gate().close();

        // Retained before anything else, and the phase after it, so no reader
        // sees a recorded fence over storage that is still empty.
        let retained = self.evidence().result.set(fence).is_ok();
        debug_assert!(retained, "one right, one writer, one write");
        self.evidence().phase.store(2, Ordering::Release);
        PrivateFenced::Recorded
    }
}
