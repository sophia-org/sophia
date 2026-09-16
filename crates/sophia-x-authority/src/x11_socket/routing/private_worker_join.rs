// Taking custody of one connection's worker handle, joining it, and keeping
// what the join returned.
//
// Split from startup and from the body by subject: starting is a transaction
// over a handle and a permit, running is a loop, and this is the one act that
// turns a thread nobody can see into a fact somebody can read.

/// What a join returned.
///
/// THE PAYLOAD IS KEPT, not reduced to a flag. A worker that panicked was
/// carrying something, and what to do with it is whoever reads this record's
/// business; turning it into a boolean here would decide that by discarding it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
enum PrivateJoinResult {
    /// The worker's frame returned.
    Returned,
    /// It panicked, and this is what it was carrying.
    ///
    /// BEHIND A SYNCHRONISER OF ITS OWN, and that is what makes this record
    /// shareable at all. A panic payload is `Send` and nothing more -- it is
    /// whatever the panicking frame happened to be carrying -- so a record
    /// holding one bare could not be borrowed on another thread even to read
    /// which of these two it is. Readers take this when they want the payload;
    /// the writer never does, because it puts the payload in on the way past.
    Panicked(Mutex<Box<dyn std::any::Any + Send>>),
}

/// How far one reaping attempt has got.
///
/// THREE FACTS, AND THE MIDDLE ONE IS THE AWKWARD ONE. Written before a handle
/// is taken, it says an attempt claimed this record and may have consumed
/// something -- not that a join was entered, and not that the worker has left.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateReapingPhase {
    /// No attempt has consumed a handle through this record.
    NotBegun,
    /// An attempt claimed this record and intends to consume a handle.
    ///
    /// WRITE-AHEAD INTENT, AND NOT PROOF OF ANYTHING ELSE. It is written
    /// before the handle is taken, so finding it says a claim was made -- not
    /// that a handle was got, not that a join was entered, and not that the
    /// worker is gone.
    ///
    /// AND IT IS NOT RETRYABLE AS THOUGH A HANDLE WERE STILL THERE. An
    /// attempt that got this far and left no result may have consumed one;
    /// asking again on that assumption is how a handle gets joined twice or a
    /// slot made startable over a thread still running.
    InProgress,
    /// This record's handle was joined and the result is here to be read.
    Joined,
}

/// Where a join's result is published, owned by whoever asked for one.
///
/// FIXED, AND PREPARED BEFORE ANY HANDLE IS TAKEN. The storage a join's result
/// goes into cannot be made after the join: allocating between the return and
/// the retention is exactly the interval in which the one copy of that result
/// is in a frame and nothing else.
///
/// THE CALLER KEEPS IT, outside the frame that does the joining, so a reaping
/// that fails or unwinds still leaves whatever it had established readable.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
struct PrivateJoinEvidence {
    /// WHETHER THE ONE RIGHT TO PUBLISH INTO THIS HOME IS STILL HERE.
    ///
    /// IT LIVES WHERE THE HOME LIVES, WHICH IS THE WHOLE POINT. When each
    /// attempt kept its own claim, a second view over the same source and the
    /// same custody arrived with a claim of its own, made an empty attempt,
    /// and withdrew the phase of a join that had already completed: the result
    /// stayed in storage and stopped being readable. Authority that a new view
    /// can mint is not authority.
    ///
    /// TAKEN, NOT CHECKED. One exchange is the whole acquisition, so there is
    /// no interval between finding it free and holding it. It goes back only
    /// when an attempt consumed nothing; an attempt that took a handle keeps
    /// it for good, whether it published a result or was lost before it could.
    producer: std::sync::atomic::AtomicBool,
    /// How far the attempt holding that right has got.
    phase: std::sync::atomic::AtomicU8,
    /// The result, written once by the attempt that claimed it.
    ///
    /// NOT A LOCK, AND NOT MERELY A TYPE THAT SAYS ONCE. What makes the write
    /// here uncontended is that exclusivity was established before the handle
    /// was consumed: one right, one writer, one write. Publication cannot wait
    /// on another holder, which is what a mutex taken after the join would
    /// have made it do.
    result: std::sync::OnceLock<PrivateJoinResult>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl PrivateJoinEvidence {
    /// Take the one right to publish into this home, if it is here.
    ///
    /// WHOEVER GETS IT IS THIS HOME'S PRODUCER until it is given back, and
    /// nothing else can become one in the meantime.
    fn take_publication(&self) -> bool {
        self.producer
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Give it back, which only an attempt that consumed nothing may do.
    ///
    /// THE CALLER IS THE HOLDER, so this is a release and not a race: nobody
    /// else can be holding it to lose.
    fn return_publication(&self) {
        self.producer.store(true, Ordering::Release);
    }

    fn phase(&self) -> PrivateReapingPhase {
        match self.phase.load(Ordering::Acquire) {
            0 => PrivateReapingPhase::NotBegun,
            1 => PrivateReapingPhase::InProgress,
            _ => PrivateReapingPhase::Joined,
        }
    }

    /// The result, if this join returned one.
    ///
    /// NOTHING IS VISIBLE HERE BEFORE THE PHASE SAYS SO, and the phase is not
    /// written until the result is in place, so a reader that sees `Joined`
    /// finds it.
    fn result(&self) -> Option<&PrivateJoinResult> {
        match self.phase() {
            PrivateReapingPhase::Joined => self.result.get(),
            PrivateReapingPhase::NotBegun | PrivateReapingPhase::InProgress => None,
        }
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
struct PrivateReapingRecord<'a> {
    /// The slot this record was bound to, and the only one it will ever act
    /// on.
    ///
    /// CAPTURED ONCE, AT CONSTRUCTION. A reaping that took a slot at every
    /// visit was a record with no source of its own: one that had looked at an
    /// empty slot and released its claim would go on to consume whatever
    /// handle the next caller happened to hand it, and a retry could be given
    /// a different connection's exit record and report that connection's body
    /// had left nothing. Binding it here is not a check that can be got wrong;
    /// there is no second slot to pass.
    slot: &'a Mutex<PrivateWorkerSlot>,
    /// That slot's worker's exit record.
    ///
    /// WHICH THREAD WROTE IT IS STILL THE CALLER'S OBLIGATION. Nothing here
    /// can establish that this record was written by the thread that slot
    /// holds -- the types do not carry it -- and binding the pair once is not
    /// a claim that it has been checked. What it does prevent is the pair
    /// being changed afterwards.
    exit: &'a PrivateWorkerExit,
    /// Whether an attempt has claimed THIS RECORD.
    ///
    /// RECORD-LOCAL, AND NOT THE AUTHORITY TO PUBLISH. It stops two asks on
    /// this one record reaching a handle together, and it is released again
    /// only by an attempt that consumed nothing. What decides which attempt
    /// may write into the shared home is the right that lives in that home,
    /// because a claim minted here is one a second view mints just as easily.
    claimed: AtomicBool,
    /// Where this join's evidence is published.
    ///
    /// FROM A CUSTODY THAT ALREADY OWNED IT, AND STILL DOES. This record does
    /// not make the home it publishes into: it is given one whose custodian is
    /// in a scope outside this operation, so losing this record -- by
    /// returning, by refusing, or by unwinding -- loses the record and not the
    /// result.
    ///
    /// THIS HANDLE IS AN OWNING ONE. An `Arc` clone keeps what it points at
    /// whatever it was cloned from, so what makes the custodian rather than
    /// this record the keeper is not the handle: it is that the constructor
    /// borrows the custody for as long as this record exists, so a record
    /// cannot be left holding the last one.
    ///
    /// A home allocated here would have had its only handle in this frame, and
    /// handing it back afterwards would have offered a keeper rather than
    /// making one.
    evidence: Arc<PrivateJoinEvidence>,
}

/// What a reaping attempt found.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateReaped {
    /// This attempt joined this connection's worker. The result is in the
    /// record, and was there before this was returned.
    Joined,
    /// Nothing was ever started here. NOTHING WAS CONSUMED, and this does not
    /// by itself stop a worker being started later.
    NothingStarted,
    /// The handle has already gone to somebody else. Nothing was consumed.
    HandedElsewhere,
    /// A handle was gone without this slot saying it had been handed on.
    ///
    /// Distinct from both of the above because it is neither: something took
    /// it without recording that it had, and a caller told it was never
    /// started would be wrong.
    HandleMissing,
    /// This record has already been asked. Nothing was consumed, and whatever
    /// the first attempt established is left exactly as it was.
    AlreadyAsked,
    /// This home's one publication right is somewhere else.
    ///
    /// ANOTHER VIEW HOLDS IT OR HAS USED IT, so this attempt is not the
    /// producer for this home and may not write its phase, its result, or
    /// withdraw what another attempt established. NOTHING WAS CONSUMED.
    ///
    /// NOT A STATEMENT ABOUT THE WORKER. A handle may still be sitting in the
    /// slot; what this says is that publishing about it is not this view's to
    /// do.
    NotThePublisher,
}

/// What a worker's exit record said, read after the join result was retained.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateExitReading {
    /// The body left a classification.
    Classified(PrivateWorkerOutcome),
    /// The body published its departure and left no classification.
    ///
    /// EXACTLY THAT, AND NOT A PANIC. A normal return writes its
    /// classification BEFORE publishing the departure, so this is not a body
    /// caught between two writes -- it is one that never reached the first,
    /// or one whose record was written by something other than a body's own
    /// run.
    ///
    /// What says whether a frame panicked is the join's own Returned or
    /// Panicked outcome. Not this, and not who wrote this.
    Unclassified,
    /// It had not published a departure when this was read.
    NotLeft,
    /// The record could not be read.
    Unreadable,
}

/// Everything one reaping attempt reports.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PrivateReaping {
    /// What this attempt did.
    reaped: PrivateReaped,
    /// The slot was poisoned when this attempt read it.
    ///
    /// BESIDE THE ANSWER, NOT INSTEAD OF IT. A poisoned slot is recovered far
    /// enough to get the handle out -- leaving a thread unjoinable because
    /// somebody panicked near its slot would be the worse outcome -- and the
    /// fact is reported rather than absorbed.
    slot_poisoned: bool,
    /// What the exit record said, when a join happened.
    ///
    /// `None` when no join happened: the exit record is read only after a
    /// result has been retained, and never as a condition of retaining one.
    exit: Option<PrivateExitReading>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Read by a caller no production site has yet.
impl<'a> PrivateReapingRecord<'a> {
    /// A record for a join nobody has asked for yet, over this slot, this
    /// worker's exit record, and a publication home somebody else keeps.
    ///
    /// THE CUSTODY IS TAKEN BEFORE ANY HANDLE IS. That order is the component
    /// this belongs to: a result is published into a home that already had an
    /// owner, so no frame here is the only thing standing between the evidence
    /// and nothing.
    ///
    /// AND THE BORROW IS WHAT ENFORCES IT. The custody is borrowed for as long
    /// as this record exists, so a record cannot be returned from the scope
    /// that holds its custodian and then be the last thing keeping the
    /// evidence. Holding an owning handle would not have established that: an
    /// `Arc` clone keeps what it points at no matter what it was cloned from,
    /// so a record built from a custody that then went away would have gone on
    /// working -- and losing that record would have lost the result the whole
    /// component exists to protect.
    fn bound_to(
        slot: &'a Mutex<PrivateWorkerSlot>,
        exit: &'a PrivateWorkerExit,
        custody: &'a PrivateEvidenceCustody,
    ) -> Self {
        Self {
            slot,
            exit,
            claimed: AtomicBool::new(false),
            evidence: Arc::clone(custody.join()),
        }
    }

    fn phase(&self) -> PrivateReapingPhase {
        self.evidence.phase()
    }

    fn result(&self) -> Option<&PrivateJoinResult> {
        self.evidence.result()
    }

    /// The publication home this record was given.
    ///
    /// ASKED FOR IN ORDER TO COMPARE. Whoever needs to know that this record
    /// publishes into a particular custody's home asks for it and checks.
    ///
    /// WHAT IT HANDS BACK IS AN OWNING HANDLE, and saying otherwise would be
    /// wrong: an `Arc` clone keeps the home alive for as long as the clone
    /// lasts, whoever made it. What the borrow above establishes is about THIS
    /// RECORD -- it cannot outlive the custody it was built from -- and not
    /// about what it hands out. A caller may keep this clone after both are
    /// gone and still read the result through it. That is a reader holding
    /// evidence, which is allowed; what the component rules out is an
    /// OPERATION being the only thing that kept it.
    fn join_evidence(&self) -> Arc<PrivateJoinEvidence> {
        Arc::clone(&self.evidence)
    }

    /// Take this connection's worker handle, join it, and keep what came back.
    ///
    /// ONE ATTEMPT OWNS THE HANDLE. The record is claimed and the intent written
    /// before anything is taken, so two asks cannot both reach a handle, an
    /// interrupted attempt cannot be retried as though one were still there, and a
    /// second ask cannot overwrite what the first established.
    ///
    /// NO LOCK IS HELD ACROSS THE JOIN. Not the slot, not this record, not the
    /// exit record, and nothing of the connection's at all: joining waits for
    /// another thread to finish, and anything held across it is held for as long
    /// as that thread takes. Everything fallible happens before the handle is
    /// consumed, and what is between the consumption and the join is a guard being
    /// released and one atomic write.
    ///
    /// AND THE RESULT IS RETAINED BEFORE ANYTHING ELSE HAPPENS. Between a join
    /// returning and this being kept, the only copy of what that worker did is in
    /// this frame; an allocation, an acquisition, a callback or a formatting call
    /// in there is a place it can be lost.
    ///
    /// NOTHING HERE IS A WIRE OUTCOME. A join says a thread has finished and what
    /// it finished with. It is not an ending, a receipt, a fence, a settlement, or
    /// permission to drive this connection's home.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // Nothing reaps a worker yet.
    fn reap(&self) -> PrivateReaping {

        // CLAIMED FIRST. Everything below this point is one attempt's, and a
        // second ask leaves without touching the slot.
        if self.claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return PrivateReaping {
                reaped: PrivateReaped::AlreadyAsked,
                slot_poisoned: false,
                exit: None,
            };
        }
        // AND THE HOME'S ONE RIGHT TO PUBLISH, which is not this record's to
        // assume. The claim above only stops this record being asked twice at
        // once; what says this attempt may write into that home is holding the
        // right that lives in it.
        if !self.evidence.take_publication() {
            // Nothing was consumed and nothing was written, so this record is
            // exactly as it was found and may be asked again -- by which time
            // the right may have come back.
            self.claimed.store(false, Ordering::Release);
            return PrivateReaping {
                reaped: PrivateReaped::NotThePublisher,
                slot_poisoned: false,
                exit: None,
            };
        }
        // WRITE-AHEAD: the intent is recorded before a handle can be consumed, so
        // an attempt interrupted anywhere below leaves something that says a
        // handle may have been taken rather than nothing at all.
        self.evidence.phase.store(1, Ordering::Release);

        let handoff = hand_worker_to_joiner(self.slot);
        let slot_poisoned = handoff.source_poisoned;
        let Some(handle) = handoff.handle else {
            // NOTHING WAS CONSUMED, so this attempt is not one: the intent is
            // withdrawn, the right goes back to the home and the claim is
            // released, and the record is exactly as it was found. A later ask
            // may still find a handle here, because nothing here started or
            // stopped anything.
            //
            // THE PHASE THIS WITHDRAWS IS ITS OWN. Holding the right is what
            // established that: no other attempt could have written one while
            // this one held it, and an attempt that never held it never got
            // here.
            self.evidence.phase.store(0, Ordering::Release);
            self.evidence.return_publication();
            self.claimed.store(false, Ordering::Release);
            return PrivateReaping {
                reaped: match handoff.found {
                    PrivateWorkerLife::NeverStarted => PrivateReaped::NothingStarted,
                    PrivateWorkerLife::HandedToJoiner => PrivateReaped::HandedElsewhere,
                    PrivateWorkerLife::Running => PrivateReaped::HandleMissing,
                },
                slot_poisoned,
                exit: None,
            };
        };

        // THE JOIN, WITH NOTHING HELD. The slot's guard went inside the hand-over
        // above; this record is not locked and never was; the exit record is not
        // touched until afterwards.
        let joined = handle.join();

        // RETAINED BEFORE ANYTHING ELSE. No allocation, no acquisition, no
        // formatting, and the panic payload is moved rather than dropped.
        let result = match joined {
            Ok(()) => PrivateJoinResult::Returned,
            // WRAPPED, NOT COPIED, AND NOTHING IS ALLOCATED. The payload is
            // moved into a synchroniser so that readers on other threads can
            // reach it safely; forming it is infallible and takes no lock,
            // which is what keeps this on the right side of the retention.
            Err(payload) => PrivateJoinResult::Panicked(Mutex::new(payload)),
        };
        let retained = self.evidence.result.set(result).is_ok();
        debug_assert!(retained, "one claim, one writer, one write");
        // AND ONLY THEN IS IT READABLE. The phase is what a reader consults, so
        // writing it after the result is what stops anyone seeing `Joined` over
        // storage that is still empty.
        self.evidence.phase.store(2, Ordering::Release);

        // THE EXIT RECORD IS READ LAST, and never as a condition of any of the
        // above. What a body left is its own evidence: a join that returned does
        // not manufacture a classification, and one that reports a panic does not
        // overwrite a classification that is there.
        let exit = Some(self.exit.reading());
        PrivateReaping {
            reaped: PrivateReaped::Joined,
            slot_poisoned,
            exit,
        }
    }
}
