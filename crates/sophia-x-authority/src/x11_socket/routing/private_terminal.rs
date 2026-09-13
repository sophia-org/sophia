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
    /// Run one turn of the shared order through the ordered path.
    ///
    /// Work is taken from the order and the queue guard is released before
    /// common is acquired, so no path from the queue to common exists. Each
    /// item carries the reservation made for it before it was published, and
    /// that reservation becomes the custody the transaction runs against --
    /// which is what ties the thing executed to the thing the order accepted,
    /// rather than to a payload supplied alongside it.
    ///
    /// Bounded by what the queue can hold rather than by when producers stop,
    /// for the same reason the ordinary turn is: draining until empty lets a
    /// producer that keeps replenishing hold the turn open.
    ///
    /// Nothing is emitted here and nothing is settled here. What comes back is
    /// owned: a decision with its custody, a refusal with the custody and the
    /// work it was for, or an operation this path does not run.
    #[cfg_attr(not(test), allow(dead_code))]
    fn route_pending_ordered(
        &mut self,
        keyboards: &mut PrivateKeyboards,
    ) -> Result<Vec<PrivateOrderedItem>, XServerFrontendRouteError> {
        // An item left owned by an interrupted turn blocks the order. Storing
        // it before one call protects it from that call and from nothing else:
        // a later turn that dequeued into the same slot would overwrite the
        // only record of work already taken, whose application is unknown.
        // Nothing resolves such an item yet, so the order stays blocked, which
        // is the same honest state as the park.
        if self.current.is_some() {
            return Err(XServerFrontendRouteError::OrderedItemUnresolved);
        }
        // Anything an earlier turn parked still holds its place. Nothing after
        // it may run until its disposition is established, and that outlives
        // the turn that met it -- a turn that merely stopped would let the
        // next one overtake exactly the operation it stopped for.
        if let Some(sequence) = self.parked_barrier {
            // Still blocked. Holding the operation and having established what
            // becomes of it are different things, so this survives the operation
            // being handed to an owner: an owner that took it and then dropped
            // it established nothing, and running later input at that point is
            // exactly the overtaking the park prevents.
            return Ok(vec![PrivateOrderedItem::Parked { sequence }]);
        }
        let budget = self.service_budget;
        while self.turn.len() < budget {
            let next = match self.admission.take_next() {
                Ok(next) => next,
                Err(()) => {
                    // Everything already taken out of the order stays owned
                    // here rather than going with the frame. Returning results
                    // only on success would drop the work of every earlier
                    // iteration on the failure of a later one.
                    return Err(XServerFrontendRouteError::RegistryPoisoned);
                }
            };
            let Some((sequence, _class, operation)) = next else {
                break;
            };
            let PrivateOperation::RoutedInput(mut envelope) = operation else {
                // An operation this path does not execute. Parked in place,
                // and the turn ends: later input must not apply past an
                // earlier operation that has neither run nor been cancelled.
                self.parked = Some((sequence, operation));
                self.parked_barrier = Some(sequence);
                self.turn.push(PrivateOrderedItem::Parked { sequence });
                break;
            };
            let Some(reservation) = envelope.reservation.take() else {
                self.parked = Some((sequence, PrivateOperation::RoutedInput(envelope)));
                self.parked_barrier = Some(sequence);
                self.turn.push(PrivateOrderedItem::Parked { sequence });
                break;
            };
            // The reservation made for this exact work before it was published
            // becomes the custody it runs against.
            let custody = reservation.accepted();
            let route = envelope.route;
            // Owned before the execution, not after it. The item has left the
            // order and nothing else holds it, so a failure or an unwind
            // inside execution would otherwise take the custody and the work
            // with the frame. Its phase is the custody's own: never entered,
            // entered and unknown, or settled.
            self.current = Some(PrivateOrderedItem::Refused {
                sequence,
                refusal: PrivateExecutionRefusal::NotAttempted,
                custody,
                route,
            });
            let outcome = self.run_current(keyboards);
            let Some(PrivateOrderedItem::Refused {
                sequence,
                custody,
                route,
                ..
            }) = self.current.take()
            else {
                break;
            };
            match outcome {
                Ok(run) => self.turn.push(PrivateOrderedItem::Ran {
                    sequence,
                    run,
                    custody,
                    route,
                }),
                Err(refusal) => self.turn.push(PrivateOrderedItem::Refused {
                    sequence,
                    refusal,
                    custody,
                    route,
                }),
            }
        }
        Ok(std::mem::take(&mut self.turn))
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
        std::mem::take(&mut self.turn)
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
        self.delivering.extend(items);
        let mut delivered = Vec::with_capacity(self.delivering.len());
        // Removed only once its outcome has been decided, so the item being
        // worked on is owned throughout rather than held in a local for the
        // length of the attempt.
        while !self.delivering.is_empty() {
            // An entry an earlier call left part-way through keeps whatever it
            // reached. Starting a new call is not a disposition, and resetting
            // its phase here would turn an event that may already be queued
            // back into one that looks never attempted.
            if self.emission == PrivateEmissionPhase::Indeterminate {
                break;
            }
            self.emission = PrivateEmissionPhase::NotOwed;
            // Read from the entry rather than taken out of it. Removing it
            // first put the obligation in a local, so the phase on this
            // instance survived an unwind while the work it described did not.
            let PrivateOrderedItem::Ran { sequence, run, .. } = &self.delivering[0] else {
                let item = self.delivering.remove(0);
                // A refusal attempted no emission, so nothing about a client's
                // queue is owed or unknown for it.
                self.undelivered.push(PrivateUndelivered {
                    item,
                    emission: PrivateEmissionPhase::NotOwed,
                });
                continue;
            };
            let (sequence, run) = (*sequence, *run);
            let delivery = match &self.delivering[0] {
                PrivateOrderedItem::Ran { route, .. } => route.delivery,
                _ => None,
            };
            let enqueued = match (run.event, run.reached) {
                (Some(event), Some(reached)) => {
                    // Written before the send, because a phase set after it
                    // says nothing about a send that did not return.
                    self.emission = PrivateEmissionPhase::Indeterminate;
                    let sent = self.emit(reached, event, delivery).is_ok();
                    self.emission = if sent {
                        PrivateEmissionPhase::Enqueued
                    } else {
                        PrivateEmissionPhase::NotEnqueued
                    };
                    sent
                }
                _ => false,
            };
            if run.owes_event && !enqueued {
                // An event was owed and has not reached a queue. The entry is
                // moved with the phase it reached, not before it was known.
                let item = self.delivering.remove(0);
                self.undelivered.push(PrivateUndelivered {
                    item,
                    emission: self.emission,
                });
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
                let Self {
                    delivering,
                    ..
                } = self;
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
                    let item = self.delivering.remove(0);
                    self.undelivered.push(PrivateUndelivered {
                        item,
                        emission: self.emission,
                    });
                    continue;
                }
            };
            // Disposed, so the entry goes.
            let _resolved = self.delivering.remove(0);
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
