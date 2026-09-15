// One turn of the shared order, and what becomes of what it decided.
//
// Split from the guarded execution by subject: that file is what happens to
// one admitted input under the guards, and this is what the order hands to a
// turn and what the turn still owes afterwards.

/// One ordered turn's result for a single queued operation.
///
/// No `Debug`: the variants own custody and stamped work, and formatting one
/// would put a request's identity and a client's input into any log that
/// prints a turn.
///
/// Not public, because one variant carries an operation and that carries the
/// stamped envelope shape. Publishing this to hand an owner a report would
/// export the wire form for the sake of describing a turn, which is the trade
/// the private operation type was kept out of the public surface to avoid. It
/// becomes public when there is an owner outside this crate to give it to, and
/// what that owner needs decides what it says rather than what is convenient
/// What one terminal step did.
///
/// A sequence and at most a report, never an entry. Advancing a refusal into
/// retained inventory produces no report and is still a step that happened,
/// so a caller that read "no report" as "no work" would charge nothing for
/// work it did.
/// How many stalled press handovers pass before the visit goes to cleanup
/// instead.
///
/// A press that cannot progress must not hold the only visit this service
/// takes. Independent work that can progress -- recording a proof that owes
/// nothing to this handover -- still runs while the dependent wire output
/// stays blocked.
#[cfg(unix)]
const PRIVATE_PRESS_STALL_ALLOWANCE: u8 = 2;

/// How many recording visits pass before dispatch is given a turn, when both
/// are owed.
///
/// Small, because delivery debt that is already native is waiting on nothing
/// but a turn.
#[cfg(unix)]
const PRIVATE_NATIVE_CLASS_INTERVAL: u8 = 2;

/// How many delivery steps native work waits before it is given a turn of its
/// own, while deliveries are still ready.
///
/// A bound on waiting, not a share of the service: deliveries keep the step
/// they would have had, and native work simply stops being postponed for
/// ever by traffic that never stops arriving.
#[cfg(unix)]
const PRIVATE_NATIVE_TURN_INTERVAL: u8 = 4;

/// What answering one receipt established.
///
/// Three different facts, counted apart. Reporting a refused ledger answer as
/// a return counted something that had not happened.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateReceiptStep {
    /// The recipient's half is settled, on an established flush or a
    /// terminated connection.
    ///
    /// `debt_settled` is the ledger's own answer about the WHOLE debt, not a
    /// second opinion formed here. Discarding it lost a fact that had
    /// actually happened: a turn that closed a debt reported closing none.
    Settled {
        #[cfg_attr(not(test), allow(dead_code))]
        debt_settled: bool,
    },
    /// The attempt was returned with neither bit and the event will not be
    /// sent again. The debt stays owed.
    ReturnedUnsettled,
    /// The ledger did not answer. Nothing changed and nothing is counted.
    Unanswered,
}

#[cfg(unix)]
enum PrivateDeliveryStep {
    /// Nothing was waiting, and nothing owed a recording either.
    Idle,
    /// One delivery attempt was spent on one chosen release.
    ///
    /// `enqueued` says whether the capsule reached the recipient's queue.
    /// False is an attempt that was given straight back -- the debt is exactly
    /// as owed as before and the capsule is still held here. It is not a
    /// recording, and it is not a receipt.
    Dispatched {
        #[cfg_attr(not(test), allow(dead_code))]
        enqueued: bool,
        /// True when the visit went to giving an attempt back rather than
        /// making one. Counted apart: relinquishing is not delivering.
        #[cfg_attr(not(test), allow(dead_code))]
        relinquished: bool,
    },
    /// One receipt was answered against the debt it belongs to.
    Receipt {
        #[cfg_attr(not(test), allow(dead_code))]
        step: PrivateReceiptStep,
    },
    /// One proof-recording visit was spent on one chosen release.
    ///
    /// `recorded` says whether that release's native bit went in. False is an
    /// attempt that refused, which keeps its cause and its identity and stays
    /// owed; it is not the visit failing to happen.
    Recorded {
        #[cfg_attr(not(test), allow(dead_code))]
        recorded: bool,
    },
    /// The entry at the head cannot be described, so nothing may be done with
    /// it. Not the same as nothing waiting.
    ///
    /// The sequence names the entry that is stuck, for a runner that has to
    /// say which one. The wrapper in this file only needs to stop.
    #[allow(dead_code)]
    Blocked(crate::ReadySequence),
    /// Exactly one entry was disposed of, transferred or observed.
    Advanced {
        #[cfg_attr(not(test), allow(dead_code))]
        sequence: crate::ReadySequence,
        report: Option<PrivateDelivered>,
    },
}

/// What one bounded step of the order did.
///
/// Carries a sequence and never an item. What was taken is stored in this
/// instance before the step returns, so a caller doing fallible accounting
/// afterwards is never the only holder of accepted work: a failure there ends
/// the call, not the obligation. Two of these are facts about the order rather
/// than about an item -- nothing waiting, or nothing may run yet -- and a
/// caller told only "no item" could not tell them apart.
#[cfg(unix)]
enum PrivateOrderedStep {
    /// The order had nothing waiting.
    Idle,
    /// The order is blocked behind an earlier operation whose disposition is
    /// not established. Nothing was taken.
    Blocked(crate::ReadySequence),
    /// One item was taken, decided, and stored in the turn.
    ///
    /// The sequence is what a runner correlates its accounting against; the
    /// unaccounted caller in this file has nothing to correlate and ignores
    /// it.
    #[cfg_attr(not(test), allow(dead_code))]
    Decided(crate::ReadySequence),
    /// Taken, decided and stored, but the supervisor would not take the
    /// finish. The work happened; that anything was still watching when it
    /// returned did not.
    ///
    /// The sequence is here for the runner that reads it. No control in this
    /// crate produces a latched finish: doing so needs the supervisor to fail
    /// during an execution, which is its own timing rather than something a
    /// caller can ask for. Allowed unconditionally for that reason rather
    /// than left to look consumed.
    #[allow(dead_code)]
    DecidedUnwatched(crate::ReadySequence),
    /// Taken but not run, because nothing would watch it. The work is owned
    /// and un-attempted and the order is blocked on it.
    #[cfg_attr(not(test), allow(dead_code))]
    Unwatched(crate::ReadySequence),
    /// One item was taken that this path does not execute, and is held as the
    /// parked operation. The order is blocked behind it now.
    Parked(crate::ReadySequence),
}

/// to expose now.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
enum PrivateOrderedItem {
    /// The work ran and decided.
    Ran {
        sequence: crate::ReadySequence,
        run: PrivateOrderedRun,
        /// The request it ran against, still owed its terminal observation.
        ///
        /// Returned rather than dropped here: execution produced an outcome
        /// and the cell holding it is freed by observing, which the terminal
        /// owner does. Dropping it here would end the only right to take that
        /// outcome.
        custody: PrivateOutstandingRequest,
        /// The work as it was accepted.
        ///
        /// Kept on success for the same reason a refusal keeps it. The
        /// delivery identity, the route lease and the rest are what the
        /// terminal owner answers with, and a completion token does not
        /// contain them -- a request identity and a delivery identity are
        /// different things, and one cannot be reconstructed from the other.
        route: XAuthorityRoutedInput,
    },
    /// The consumer refused, and the work it was accepted for comes back.
    ///
    /// Custody travels with the refusal. An accepted request that the consumer
    /// declines is still a request the order took, and dropping it here would
    /// erase it on the strength of a decision not to run it.
    Refused {
        sequence: crate::ReadySequence,
        refusal: PrivateExecutionRefusal,
        custody: PrivateOutstandingRequest,
        route: XAuthorityRoutedInput,
    },
    /// An earlier operation this path does not execute, parked where it sits.
    ///
    /// Not "unreserved": a control carries its own accepted completion
    /// registration, and calling it unreserved describes it by what this path
    /// happens to lack rather than by what it is.
    ///
    /// It keeps its place in the order and nothing after it runs until its
    /// disposition is established. Handing it out and carrying on would apply
    /// later input past an earlier operation that has neither executed nor
    /// been cancelled -- the report would be in order while the effects were
    /// not, which is the ordering this path exists to hold.
    ///
    /// The report names it; it does not hand it over. Seeing that the order is
    /// parked is not accepting responsibility for what it is parked on, and
    /// the operation stays owned here until something takes it.
    Parked { sequence: crate::ReadySequence },
}

#[cfg(unix)]
impl PrivateOrderedItem {
    /// Where this item stood in the order.
    ///
    /// Every kind has one: what an item is does not change which position the
    /// order gave it, and that position is what anything accounting for the
    /// item names it by.
    fn sequence(&self) -> crate::ReadySequence {
        match self {
            Self::Ran { sequence, .. }
            | Self::Refused { sequence, .. }
            | Self::Parked { sequence } => *sequence,
        }
    }
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Take one item from the order, mark it, and execute it.
    ///
    /// Bounded to a single dequeue because that is what can be accounted for.
    /// A call that drained until the order was empty could be charged one
    /// start for an unbounded amount of work, and a watchdog mark placed after
    /// it would describe work that had already finished rather than work in
    /// progress.
    ///
    /// `mark` runs immediately after the item is taken and before common is
    /// acquired: the item is in hand, so the mark names the work actually
    /// being attempted, and it precedes the guard rather than following the
    /// attempt. It must not take common itself -- it runs above that guard in
    /// the rank, and reaching for it here would invert the order this
    /// placement exists to respect.
    ///
    /// The item is taken and the queue guard released before common is
    /// acquired, so no path from the queue to common exists. The reservation
    /// made for this work before it was published becomes the custody the
    /// transaction runs against, which is what ties the thing executed to the
    /// thing the order accepted rather than to a payload supplied alongside
    /// it.
    ///
    /// Nothing is emitted here and nothing is settled here. What comes back is
    /// owned: a decision with its custody, a refusal with the custody and the
    /// work it was for, or an operation this path does not run.
    fn step_once(
        &mut self,
        keyboards: &mut PrivateKeyboards,
        start: &mut dyn FnMut(
            crate::ReadySequence,
            std::time::Instant,
        ) -> Result<(), XServerFrontendRouteError>,
        watch: &private_watchdog::PrivateWatchdogOwner,
    ) -> Result<PrivateOrderedStep, XServerFrontendRouteError> {
        // An item left owned by an interrupted turn blocks the order. Storing
        // it before one call protects it from that call and from nothing else:
        // a later turn that dequeued into the same slot would overwrite the
        // only record of work already taken, whose application is unknown.
        if self.terminal.current.is_some() {
            return Err(XServerFrontendRouteError::OrderedItemUnresolved);
        }
        // Anything an earlier turn parked still holds its place. Nothing after
        // it may run until its disposition is established, and that outlives
        // the turn that met it -- a turn that merely stopped would let the next
        // one overtake exactly the operation it stopped for.
        if let Some(sequence) = self.parked_barrier {
            return Ok(PrivateOrderedStep::Blocked(sequence));
        }
        let next = match self.admission.take_next() {
            Ok(next) => next,
            Err(()) => return Err(XServerFrontendRouteError::RegistryPoisoned),
        };
        // Taken at the return, not at the mark. The two are separated by the
        // storing below, and an instant read afterwards would quietly exclude
        // that from whatever the mark is accounting for.
        let taken_at = std::time::Instant::now();
        let Some((sequence, _class, operation)) = next else {
            return Ok(PrivateOrderedStep::Idle);
        };
        // Stored before anything else may run. Until this, the work is only in
        // a local: anything that unwinds between the queue and here takes the
        // accepted custody and its payload with the frame, and nothing would
        // be left to say the order had given it out. Marking, accounting and
        // the transaction all come after it is this instance's.
        let taken = match operation {
            PrivateOperation::RoutedInput(mut envelope) => match envelope.reservation.take() {
                // The reservation made for this exact work before it was
                // published becomes the custody it runs against, which is what
                // ties the thing executed to the thing the order accepted.
                Some(reservation) => {
                    self.terminal.current = Some(PrivateOrderedItem::Refused {
                        sequence,
                        refusal: PrivateExecutionRefusal::NotAttempted,
                        custody: reservation.accepted(),
                        route: envelope.route,
                    });
                    None
                }
                None => {
                    self.parked = Some((sequence, PrivateOperation::RoutedInput(envelope)));
                    self.parked_barrier = Some(sequence);
                    Some(PrivateOrderedStep::Parked(sequence))
                }
            },
            // An operation this path does not execute. Parked in place, and
            // the order is blocked: later input must not apply past an earlier
            // operation that has neither run nor been cancelled.
            other => {
                self.parked = Some((sequence, other));
                self.parked_barrier = Some(sequence);
                Some(PrivateOrderedStep::Parked(sequence))
            }
        };
        // Offered every dequeue, including one that will only be parked:
        // whether that counts as a start is the hook's policy and not this
        // step's to assume. It can refuse, and a refusal here stops before the
        // execution with the work still owned -- which is why it returns a
        // result rather than being told after the fact.
        start(sequence, taken_at)?;
        // A parked operation is a dequeue with no execution after it, so
        // there is nothing for a supervisor to watch and nothing that could
        // fail to come back.
        if let Some(parked) = taken {
            return Ok(parked);
        }
        // In this instance's hands, and common not yet taken. The instant is
        // the one read at the dequeue, so what the supervisor measures starts
        // where the work left the order rather than where this call reached.
        let Ok(mut watched) = watch.begin_dequeued(taken_at) else {
            // Not run. The work stays owned and un-attempted, which is the
            // same honest state an interrupted step leaves, and the order is
            // blocked on it until something answers for it.
            return Ok(PrivateOrderedStep::Unwatched(sequence));
        };
        let outcome = self.run_current(keyboards, &mut watched);
        let Some(PrivateOrderedItem::Refused {
            sequence,
            custody,
            route,
            ..
        }) = self.terminal.current.take()
        else {
            // Execution reads the current item and never replaces it, so what
            // comes back is what was placed. Anything else means the slot was
            // written by something that does not own it, and continuing would
            // decide an outcome for work this step cannot name.
            return Err(XServerFrontendRouteError::OrderedItemUnresolved);
        };
        // Into the turn here rather than handed back. The decided item carries
        // the custody still owed an observation, and a caller that had to hold
        // it while it charged a budget or finished a watchdog would be the
        // only holder of it across a call that can fail.
        self.terminal.turn.push(match outcome {
            Ok(run) => PrivateOrderedItem::Ran {
                sequence,
                run,
                custody,
                route,
            },
            Err(refusal) => PrivateOrderedItem::Refused {
                sequence,
                refusal,
                custody,
                route,
            },
        });
        // Finished after the item is stored, never between taking it out and
        // putting it back: a caller whose accounting failed in that gap would
        // be the only holder of work the order had already given up.
        match watched.finish() {
            Ok(()) => Ok(PrivateOrderedStep::Decided(sequence)),
            // The work ran and is recorded. What is not established is that
            // anything was still watching when it returned, so the step says
            // so rather than reporting an ordinary decision.
            Err(_) => Ok(PrivateOrderedStep::DecidedUnwatched(sequence)),
        }
    }

    /// Step until the service budget is spent or the order stops offering work.
    ///
    /// The unaccounted caller: it drives the same step a runner does, without
    /// a budget to charge or a watchdog to mark. Bounded by what the queue can
    /// hold rather than by when producers stop, for the same reason the
    /// ordinary turn is: draining until empty lets a producer that keeps
    /// replenishing hold the turn open. A runner that must account for each
    /// decision calls `step_once` itself rather than this.
    #[cfg_attr(not(test), allow(dead_code))]
    fn route_pending_ordered(
        &mut self,
        keyboards: &mut PrivateKeyboards,
        watch: &private_watchdog::PrivateWatchdogOwner,
    ) -> Result<Vec<PrivateOrderedItem>, XServerFrontendRouteError> {
        let budget = self.service_budget;
        while self.terminal.turn.len() < budget {
            match self.step_once(keyboards, &mut |_, _| Ok(()), watch)? {
                PrivateOrderedStep::Idle => break,
                PrivateOrderedStep::Blocked(sequence) | PrivateOrderedStep::Parked(sequence) => {
                    self.terminal.turn.push(PrivateOrderedItem::Parked { sequence });
                    break;
                }
                // Nothing here supervises, so a step that could not be
                // watched ends the turn rather than being retried blind.
                PrivateOrderedStep::Unwatched(sequence) => {
                    self.terminal.turn.push(PrivateOrderedItem::Parked { sequence });
                    break;
                }
                PrivateOrderedStep::Decided(_) | PrivateOrderedStep::DecidedUnwatched(_) => {}
            }
        }
        Ok(std::mem::take(&mut self.terminal.turn))
    }

    /// What an earlier turn parked, if anything.
    ///
    /// Read rather than taken: seeing it is not disposing of it.
    #[cfg_attr(not(test), allow(dead_code))]
    fn parked(&self) -> Option<crate::ReadySequence> {
        self.parked.as_ref().map(|(sequence, _)| *sequence)
    }

    /// Whether the order is still blocked on an earlier operation.
    ///
    /// True while its disposition is unestablished, whether or not the
    /// operation itself has been handed to an owner.
    #[cfg_attr(not(test), allow(dead_code))]
    fn blocked(&self) -> Option<crate::ReadySequence> {
        self.parked_barrier
    }

    /// Take the parked operation into an owner's hands.
    ///
    /// This moves the operation; it does **not** establish what becomes of it,
    /// and the order stays blocked. Executing or cancelling it is what would
    /// lift the barrier, and no path does that yet -- so until one exists, the
    /// honest state after a handover is still blocked rather than running the
    /// input behind an operation nothing has answered for.
    #[cfg_attr(not(test), allow(dead_code))]
    fn take_parked(&mut self) -> Option<(crate::ReadySequence, PrivateOperation)> {
        self.parked.take()
    }

    /// Recover the items of a turn that ended in an error.
    ///
    /// Everything taken out of the order before the failure is here. It is not
    /// re-run: what applied has applied, and these carry their custody so the
    /// terminal owner can still answer for them.
    #[cfg_attr(not(test), allow(dead_code))]
    fn take_interrupted_turn(&mut self) -> Vec<PrivateOrderedItem> {
        std::mem::take(&mut self.terminal.turn)
    }
}

/// Decided work whose request could not be observed.
///
/// NOT A RECORD OF A SEND. Nothing on this path sends, so nothing here says
/// how far an event got; what an entry lands here for is that its own
/// completion could not be read. The item is retained because it is the only
/// handle able to take that observation later.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateUndelivered {
    item: PrivateOrderedItem,
}

/// What delivering one decided item established.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateDelivered {
    sequence: crate::ReadySequence,
    // WHETHER THE EVENT WAS QUEUED IS NOT REPORTED HERE ANY MORE. This step
    // disposes of an entry; the handover happens later, from the custody the
    // debt holds, so a report written now could only have guessed. Enqueueing
    // is the dispatch's fact and is reported by the dispatch.
    /// The outcome the request recorded, taken exactly once.
    completion: Option<sophia_input_authority::RequestCompletion>,
    /// Whether a release debt was closed by this delivery.
    ///
    /// Always false today. Closing one needs a receipt this path cannot yet
    /// obtain, and reporting it from anything weaker would close a debt on
    /// something that did not happen.
    debt_settled: bool,
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// The charge and watch hook takes an optional sequence because not every
    /// terminal step is an ordered entry. A proof-recording visit is real
    /// work and must be admitted and watched like any other, and it has no
    /// sequence to name.
    fn deliver_one(
        &mut self,
        start: &mut dyn FnMut(
            Option<crate::ReadySequence>,
            std::time::Instant,
        ) -> Result<(), XServerFrontendRouteError>,
    ) -> Result<PrivateDeliveryStep, XServerFrontendRouteError> {
        // A WAITING RECEIPT COMES FIRST, ahead of deliveries and ahead of the
        // arbitration below. This is not a preference between kinds of work:
        // an answered receipt is the only thing that releases an attempt whose
        // handover already happened, and while one is held every other claim
        // on this frontend is refused. Leaving it behind ordinary traffic
        // would let the whole instance wait on a fact already in hand.
        if self.owes_receipt_settlement() {
            start(None, std::time::Instant::now())?;
            if let Some(step) = self.settle_one_receipt() {
                return Ok(PrivateDeliveryStep::Receipt { step });
            }
        }

        // THE CHOICE BETWEEN TWO KINDS OF WORK, made before either is taken.
        //
        // Deliveries keep their order and their head; nothing here reorders
        // them, replays one, or steps past the one in front. What this decides
        // is only whether THIS step goes to a delivery or to a native proof,
        // and it is decided from retained state because a chooser that looked
        // only at the moment would give native work a turn exactly when the
        // queues fell empty -- which, for a pointer anyone is using, is never.
        let native_owed = self.owes_native_recording();
        let nothing_to_deliver = self.terminal.delivering.is_empty() && self.terminal.turn.is_empty();
        let native_turn = native_owed
            && (nothing_to_deliver
                || self.terminal.native_turn_debt >= PRIVATE_NATIVE_TURN_INTERVAL);
        if native_turn {
            // CHARGED AND WATCHED BEFORE THE VISIT, like every other terminal
            // step, and before anything takes common.
            start(None, std::time::Instant::now())?;
            self.terminal.native_turn_debt = 0;
            // An attempt whose give-back never landed is answered next. It
            // is the ledger's slot, not this executor's, and holding one
            // while claiming another is how a bounded pool runs out.
            if let Some(relinquished) = self.relinquish_one_attempt() {
                return Ok(PrivateDeliveryStep::Dispatched {
                    enqueued: false,
                    relinquished,
                });
            }
            // ARBITRATED, NOT RANKED. Recording has to come first for any ONE
            // release, because the ledger refuses an attempt until that
            // release's native half is in. Preferring it across ALL releases
            // is a different thing: a stream of new proofs would then starve
            // delivery debt that is already native and waiting. The debt
            // counter gives dispatch its turn.
            let dispatch_due =
                self.terminal.native_class_debt >= PRIVATE_NATIVE_CLASS_INTERVAL;
            if dispatch_due
                && let Some(enqueued) = self.attempt_one_delivery()
            {
                self.terminal.native_class_debt = 0;
                return Ok(PrivateDeliveryStep::Dispatched {
                    enqueued,
                    relinquished: false,
                });
            }
            // A press's own event. It claims no ledger attempt, and it is
            // offered before the release work so a release can never overtake
            // the press it ends.
            //
            // BOUNDED, THOUGH. A press that cannot progress -- a full queue, a
            // wrapper that cannot be built -- would otherwise take every visit
            // and starve the proof work behind it, which is work that CAN
            // progress and does not depend on this handover. After a run of
            // stalled attempts the visit goes to cleanup instead, and the
            // press is offered again after it.
            if self.terminal.press_stall < PRIVATE_PRESS_STALL_ALLOWANCE
                && let Some(enqueued) = self.dispatch_one_press()
            {
                self.terminal.press_stall = if enqueued {
                    0
                } else {
                    self.terminal.press_stall.saturating_add(1)
                };
                return Ok(PrivateDeliveryStep::Dispatched {
                    enqueued,
                    relinquished: false,
                });
            }
            // Its turn comes back once something else has had one.
            self.terminal.press_stall = 0;
            if let Some(recorded) = self.record_one_native() {
                self.terminal.native_class_debt =
                    self.terminal.native_class_debt.saturating_add(1);
                return Ok(PrivateDeliveryStep::Recorded { recorded });
            }
            return Ok(match self.attempt_one_delivery() {
                Some(enqueued) => {
                    self.terminal.native_class_debt = 0;
                    PrivateDeliveryStep::Dispatched {
                        enqueued,
                        relinquished: false,
                    }
                }
                None => PrivateDeliveryStep::Idle,
            });
        }
        {
            // Between two places this inventory owns, with nothing that can
            // fail in between.
            let PrivateTerminalInventory {
                turn, delivering, ..
            } = &mut self.terminal;
            if delivering.is_empty() {
                if turn.is_empty() {
                    return Ok(PrivateDeliveryStep::Idle);
                }
                delivering.push(turn.remove(0));
            }
        }
        // Read from the entry rather than taken out of it: choosing is not
        // disposing, and a chosen entry held in a local is one an interruption
        // would take with the frame.
        let sequence = self.terminal.delivering[0].sequence();
        // Charged for the step about to happen, before any guard is taken and
        // before anything is observed. A refusal here leaves the entry owned
        // and untouched.
        start(Some(sequence), std::time::Instant::now())?;
        // AFTER ADMISSION, not before it. A refused charge is not a delivery
        // step, and counting it would move native work closer to its turn for
        // work that never happened -- or, the other way round, spend the debt
        // that was about to give it one.
        self.terminal.native_turn_debt = self.terminal.native_turn_debt.saturating_add(1);
        // Read from the entry rather than taken out of it. Removing it
        // first put the obligation in a local, so the phase on this
        // instance survived an unwind while the work it described did not.
        let PrivateOrderedItem::Ran { sequence, .. } = &self.terminal.delivering[0] else {
            let item = self.terminal.delivering.remove(0);
            // A refusal has no completion to read, so nothing about it is
            // left unobserved.
            self.terminal.undelivered.push(PrivateUndelivered { item });
            return Ok(PrivateDeliveryStep::Advanced { sequence, report: None });
        };
        let sequence = *sequence;
        // NOTHING IS SENT HERE. A private event reaches its recipient through
        // the ordered handover the debt's own custody performs, which is the
        // one path it has; sending here as well put the same event on two
        // queues. What this step still owns is the entry: choosing it,
        // observing the outcome it recorded, and disposing of it.
        //
        // An event that is still owed is owed by the record that holds its
        // custody, and that record outlives this entry. The entry no longer
        // waits on it, and no longer reports whether it was queued -- it could
        // not know, because the handover has not happened yet.

        // Owing nobody an event is an outcome, not a failure to emit one.
        // A press that joined a hold, and a release that found nothing
        // held, both finished: their completion is taken here so the grant
        // can reserve again.
        //
        // Taken exactly once, which is what frees the grant's cell. An
        // observation that could not be made is kept apart from one that
        // found nothing waiting: the first leaves the custody owed and the
        // second does not.
        let observed = {
            let Self { terminal, .. } = self;
            let PrivateTerminalInventory { delivering, .. } = terminal;
            let PrivateOrderedItem::Ran { custody, .. } = &delivering[0] else {
                unreachable!("checked above")
            };
            custody.observe()
        };
        let completion = match observed {
            Ok(completion) => completion,
            Err(_unreadable) => {
                // Nothing was established about the outcome, so the only
                // handle able to take it is retained rather than dropped.
                // WHAT IS UNKNOWN HERE IS THE OBSERVATION, not a send: this
                // entry sends nothing, and its event's own custody carries
                // whatever is known about the handover. Retaining the item is
                // what keeps the unreadable request answerable.
                let item = self.terminal.delivering.remove(0);
                self.terminal.undelivered.push(PrivateUndelivered { item });
                return Ok(PrivateDeliveryStep::Advanced { sequence, report: None });
            }
        };
        // Disposed, so the entry goes.
        let _resolved = self.terminal.delivering.remove(0);
        Ok(PrivateDeliveryStep::Advanced {
            sequence,
            report: Some(PrivateDelivered {
                sequence,
                completion,
                debt_settled: false,
            }),
        })
    }

    /// Step until the order stops offering terminal work.
    ///
    /// The unaccounted caller, kept for what already reads a whole turn. A
    /// runner that must charge each step calls `deliver_one` itself.
    ///
    /// Settlement is not here, and there is no settlement path at all: closing
    /// a release debt needs the recipient half, the recipient half is the
    /// writer's outcome rather than the queue's acceptance, and nothing on
    /// this path can yet observe one. Keeping a settlement step that could
    /// only ever be called with a receipt nobody has would be machinery
    /// describing a decision nothing makes.
    ///
    /// THE NATIVE HALF IS NOW ESTABLISHED HERE, AND ONLY THE NATIVE HALF. This
    /// path reaches deliver_one, which may spend a proof-recording visit and
    /// return Recorded, so a source proof can be consumed on the way through.
    /// That is what changed: an earlier paragraph here said no proof is
    /// consumed, and it is withdrawn.
    ///
    /// What a consumed proof establishes is exactly the native bit for the
    /// incarnation the proof itself names. The producer that owns the
    /// operation sealed it over the aggregate, the exact retained projection,
    /// the passive and implicit grab lifecycle and the route-lease and query
    /// projections together; nothing here reconstructs that reasoning or
    /// stands in for it.
    ///
    /// It establishes NOTHING about the recipient, and nothing about the debt
    /// as a whole. A release whose event was queued, or built and unsent, or
    /// never built at all, is in the same position on that half as one that
    /// was never delivered.
    ///
    /// Emission happens here, with no guard held: the decision was made under
    /// the guards and is immutable, and sending on a client's queue is exactly
    /// the kind of work that must not happen beneath them.
    ///
    /// A full queue, a disconnected client, a cleared mapper or an observed
    /// completion are none of them receipts, and a debt closed on any of those
    /// would be closed on something that did not happen.
    #[cfg_attr(not(test), allow(dead_code))]
    fn deliver_turn(&mut self, items: Vec<PrivateOrderedItem>) -> Vec<PrivateDelivered> {
        // Taken into storage this instance owns before anything is delivered.
        // Appended rather than assigned, so anything a previous interruption
        // left here is still first in line.
        self.terminal.delivering.extend(items);
        let mut delivered = Vec::with_capacity(self.terminal.delivering.len());
        while let Ok(PrivateDeliveryStep::Advanced { report, .. }) =
            self.deliver_one(&mut |_, _| Ok(()))
        {
            delivered.extend(report);
        }
        delivered
    }

}
