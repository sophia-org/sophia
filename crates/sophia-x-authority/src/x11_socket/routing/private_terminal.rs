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
        mark: &mut dyn FnMut(crate::ReadySequence, std::time::Instant),
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
        // In this instance's hands, and common not yet taken.
        mark(sequence, taken_at);
        if let Some(parked) = taken {
            return Ok(parked);
        }
        let outcome = self.run_current(keyboards);
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
        Ok(PrivateOrderedStep::Decided(sequence))
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
    ) -> Result<Vec<PrivateOrderedItem>, XServerFrontendRouteError> {
        let budget = self.service_budget;
        while self.terminal.turn.len() < budget {
            match self.step_once(keyboards, &mut |_, _| {})? {
                PrivateOrderedStep::Idle => break,
                PrivateOrderedStep::Blocked(sequence) | PrivateOrderedStep::Parked(sequence) => {
                    self.terminal.turn.push(PrivateOrderedItem::Parked { sequence });
                    break;
                }
                PrivateOrderedStep::Decided(_) => {}
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

/// How far an event got toward its client.
///
/// Recorded before the send it describes, so an interruption inside the send
/// leaves the phase saying the outcome is unknown rather than leaving it to be
/// inferred afterwards from which list an item is in. What a recovery owner
/// may do depends entirely on this: an event that never reached a queue may be
/// sent, one that reached a queue owes only its observation and must never be
/// sent again, and one whose send did not return may be neither.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrivateEmissionPhase {
    /// Nothing was owed a client, so nothing was attempted.
    NotOwed,
    /// The send was entered and did not return. Whether the event reached the
    /// queue is exactly what was lost, so it is neither resent nor assumed
    /// delivered.
    Indeterminate,
    /// The send returned and the queue did not take it.
    NotEnqueued,
    /// The queue took it. Only the observation is still owed; resending would
    /// deliver the same transition twice.
    Enqueued,
}

/// Decided work that has not been handed on, with how far it got.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateUndelivered {
    item: PrivateOrderedItem,
    emission: PrivateEmissionPhase,
}

/// What delivering one decided item established.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateDelivered {
    sequence: crate::ReadySequence,
    /// Whether the event was accepted onto the client's queue.
    ///
    /// Acceptance, not arrival. A queue that is full and a client that has
    /// gone are both failures to enqueue, but succeeding only means the event
    /// is queued: the recipient half of a debt is established by the writer's
    /// own outcome -- a flush that reached the client, or a disconnect that
    /// established it never will -- and this says nothing about either.
    enqueued: bool,
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
    /// Deliver what a turn decided.
    ///
    /// Settlement is not here, and there is no settlement path at all: closing
    /// a release debt needs the recipient half, the recipient half is the
    /// writer's outcome rather than the queue's acceptance, and nothing on
    /// this path can yet observe one. Keeping a settlement step that could
    /// only ever be called with a receipt nobody has would be machinery
    /// describing a decision nothing makes.
    ///
    /// The native half is not established here either. What the guarded code
    /// demonstrates is that the aggregate transition and the projection it
    /// moves happen in one interval; what else native reconciliation requires
    /// is not shown by that, and was previously asserted rather than proved.
    ///
    /// Emission happens here, with no guard held: the decision was made under
    /// the guards and is immutable, and sending on a client's queue is exactly
    /// the kind of work that must not happen beneath them.
    ///
    /// Settlement follows delivery rather than accompanying it. The native
    /// half was reconciled under the guard when the aggregate and the
    /// projection moved together; the recipient half is only established by
    /// the event actually reaching the client. A full queue, a disconnected
    /// client, a cleared mapper or an observed completion are none of them
    /// receipts, and a debt closed on any of those would be closed on
    /// something that did not happen.
    #[cfg_attr(not(test), allow(dead_code))]
    fn deliver_turn(&mut self, items: Vec<PrivateOrderedItem>) -> Vec<PrivateDelivered> {
        // Taken into storage this instance owns before anything is delivered.
        // Iterating a parameter leaves every item not yet reached in a local,
        // and those have already left the order -- an interruption part-way
        // would destroy the ones behind the current one along with the custody
        // they carry. Appended rather than assigned, so anything a previous
        // interruption left here is still first in line.
        self.terminal.delivering.extend(items);
        let mut delivered = Vec::with_capacity(self.terminal.delivering.len());
        // Removed only once its outcome has been decided, so the item being
        // worked on is owned throughout rather than held in a local for the
        // length of the attempt.
        while !self.terminal.delivering.is_empty() {
            // What may happen to the entry at the head depends on how far it
            // already got. Starting a new call is not a disposition, and
            // deciding that from scratch turns an event that may already be
            // queued back into one that looks never attempted.
            let resuming = self.terminal.emission;
            match resuming {
                // Nobody can say whether its event reached the queue. It may
                // not be sent again and its receipt may not be inferred, and
                // nothing here can establish either, so the order stops.
                PrivateEmissionPhase::Indeterminate => break,
                // Already on the client's queue. Only the observation is
                // owed; sending again would deliver the same transition twice.
                PrivateEmissionPhase::Enqueued => {}
                // A fresh entry, or one whose send returned without the queue
                // taking it.
                PrivateEmissionPhase::NotOwed | PrivateEmissionPhase::NotEnqueued => {
                    self.terminal.emission = PrivateEmissionPhase::NotOwed;
                }
            }
            // Read from the entry rather than taken out of it. Removing it
            // first put the obligation in a local, so the phase on this
            // instance survived an unwind while the work it described did not.
            let PrivateOrderedItem::Ran { sequence, run, .. } = &self.terminal.delivering[0] else {
                let item = self.terminal.delivering.remove(0);
                // A refusal attempted no emission, so nothing about a client's
                // queue is owed or unknown for it.
                self.terminal.undelivered.push(PrivateUndelivered {
                    item,
                    emission: PrivateEmissionPhase::NotOwed,
                });
                continue;
            };
            let (sequence, run) = (*sequence, *run);
            let delivery = match &self.terminal.delivering[0] {
                PrivateOrderedItem::Ran { route, .. } => route.delivery,
                _ => None,
            };
            let enqueued = if resuming == PrivateEmissionPhase::Enqueued {
                // Resumed after its send. Not sent again.
                true
            } else {
                match (run.event, run.reached) {
                    (Some(event), Some(reached)) => {
                        // Written before the send, because a phase set after it
                        // says nothing about a send that did not return.
                        self.terminal.emission = PrivateEmissionPhase::Indeterminate;
                        let sent = self.emit(reached, event, delivery).is_ok();
                        self.terminal.emission = if sent {
                            PrivateEmissionPhase::Enqueued
                        } else {
                            PrivateEmissionPhase::NotEnqueued
                        };
                        sent
                    }
                    _ => false,
                }
            };
            if run.owes_event && !enqueued {
                // An event was owed and has not reached a queue. The entry is
                // moved with the phase it reached, not before it was known.
                let item = self.terminal.delivering.remove(0);
                self.terminal.undelivered.push(PrivateUndelivered {
                    item,
                    emission: self.terminal.emission,
                });
                // The phase described that entry. With it gone the next one
                // has not started, and carrying the phase forward would let it
                // resume a send it never made.
                self.terminal.emission = PrivateEmissionPhase::NotOwed;
                continue;
            }
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
                    // The emission phase travels with it: this event may
                    // already be on the client's queue, and a recovery owner
                    // that resent it would deliver the same transition twice.
                    let item = self.terminal.delivering.remove(0);
                    self.terminal.undelivered.push(PrivateUndelivered {
                        item,
                        emission: self.terminal.emission,
                    });
                    self.terminal.emission = PrivateEmissionPhase::NotOwed;
                    continue;
                }
            };
            // Disposed, so the entry goes and the phase that described it goes
            // with it.
            let _resolved = self.terminal.delivering.remove(0);
            self.terminal.emission = PrivateEmissionPhase::NotOwed;
            delivered.push(PrivateDelivered {
                sequence,
                enqueued,
                completion,
                debt_settled: false,
            });
            continue;
        }
        delivered
    }

    /// Send one decided event to the client it was decided for.
    fn emit(
        &self,
        reached: PrivateReachedResources,
        event: XAuthorityInputEvent,
        delivery: Option<XAuthorityInputDeliveryId>,
    ) -> Result<(), XServerFrontendRouteError> {
        let senders = self.broker.registry.client_senders(reached.client())?;
        self.broker.registry.route_to_client(
            reached.client(),
            senders.input,
            XAuthorityClientInputEvent {
                client: reached.client(),
                event,
                target_window: Some(reached.window()),
                xi_event_type: None,
                xi_event_window: None,
                xi_emulated_button_type: None,
                xi_emulated_button_window: None,
                xi_pointer_crossing_mask: 0,
                delivery,
            },
        )
    }

}
