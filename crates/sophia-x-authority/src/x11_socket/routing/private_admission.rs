// Where a private instance's producers hand work over.
//
// Split from the control-transition surface by subject rather than by size:
// this file is what a producer meets -- one place to be accepted, the typed
// reasons it can be refused, and the handles that offer nothing else -- while
// how a transition is applied and how an instance settles stay together next
// door.

/// The one place a private instance's runnable work is accepted.
///
/// Producers admit here directly rather than into their own channels for a
/// consumer to collect later. Position is assigned and the entry published
/// inside one hold on this lock, so two producers cannot interleave between
/// the two, and a send that has returned cannot be overtaken by one that
/// started afterwards.
#[cfg(unix)]
pub struct SharedAdmission {
    /// Credits for the storage that would hold this work if it were ever
    /// abandoned, taken before acceptance so that transfer cannot be refused.
    durable: PrivateSettlementOwner,
    ready: Arc<Mutex<SharedQueue>>,
    /// Set once the stream can no longer name an entry.
    ///
    /// Terminal, unlike a full queue. Retrying cannot produce an identity that
    /// does not exist, so further acceptance stops rather than looping. What
    /// was already accepted keeps its completion and its debt; this refuses
    /// new work instead of pretending the instance is healthy.
    exhausted: AtomicBool,
}

#[cfg(unix)]
impl SharedAdmission {
    fn new(ready: crate::ReadyStream<PrivateOperation>, durable: PrivateSettlementOwner) -> Self {
        Self {
            durable,
            ready: Arc::new(Mutex::new(SharedQueue {
                ready,
                closed: false,
            })),
            exhausted: AtomicBool::new(false),
        }
    }

    /// Stop accepting, and take what was accepted and never run.
    ///
    /// Closing happens under the same lock acceptance takes, so a producer is
    /// either accepted before the close or refused after it, never accepted
    /// into a queue nobody will drain. What was already accepted comes back
    /// here: those entries were promised a consumer and are owed an outcome,
    /// so they are handed to whoever closes rather than dropped with the
    /// queue.
    fn close(&self) -> Result<Vec<PrivateOperation>, ()> {
        // A queue that cannot be opened cannot be closed or drained either.
        // Returning an empty list here would say there was nothing owed.
        let mut queue = self.ready.lock().map_err(|_| ())?;
        queue.closed = true;
        let mut stranded = Vec::new();
        while let Some((_, _, operation)) = queue.ready.take_next() {
            stranded.push(operation);
        }
        Ok(stranded)
    }

    /// Accept runnable work, assigning its position as it is published.
    ///
    /// A refusal returns the operation itself rather than some part of it.
    /// Control and cleanup are not routes, so a refusal that handed back only
    /// a route would destroy exactly the work that has no other owner.
    /// Accept runnable work, handing over a producer's own state in the same
    /// transaction.
    ///
    /// `prepare` is asked for that handover inside the admitting hold and
    /// before anything is published, and its result decides the outcome: no
    /// handover means no acceptance, and a publication that fails rolls the
    /// handover back. Promoting beside this hold, or promoting and not
    /// checking whether it worked, leaves the instance owning queued work
    /// whose producer still owns the record.
    ///
    /// Lock rank: the admission queue is taken before anything `prepare`
    /// takes. Nothing holds a completion registry and then admits.
    fn accept_with<'handoff>(
        &self,
        class: crate::ReadyClass,
        operation: PrivateOperation,
        prepare: impl FnOnce() -> Option<ControlAcceptance<'handoff>>,
    ) -> Result<crate::ReadySequence, (AdmissionRefusal, PrivateOperation)> {
        // Checked before acceptance, so an exhausted stream never takes work
        // it cannot name.
        if self.exhausted.load(Ordering::Acquire) {
            return Err((AdmissionRefusal::Exhausted, operation));
        }
        // Reserved before acceptance. A producer refused here keeps work it
        // was never told had been taken; a bound applied later would have to
        // refuse work already accepted, with nowhere to put it.
        if let Err(refusal) = self.durable.reserve() {
            return Err((refusal, operation));
        }
        let Ok(mut queue) = self.ready.lock() else {
            self.durable.release();
            return Err((AdmissionRefusal::Unavailable, operation));
        };
        // Checked inside the same hold as admission, so a close cannot land
        // between deciding this is acceptable and accepting it.
        if queue.closed {
            self.durable.release();
            return Err((AdmissionRefusal::ConsumerGone, operation));
        }
        // Before anything is published, so a refusal here leaves the queue as
        // it was and the payload and its credit with their owner.
        let Some(handoff) = prepare() else {
            self.durable.release();
            return Err((AdmissionRefusal::Unavailable, operation));
        };
        match queue.ready.admit(class, operation) {
            Ok(sequence) => {
                handoff.commit();
                Ok(sequence)
            }
            Err(refused) => match refused.refusal {
                crate::ReadyRefusal::AtCapacity => {
                    self.durable.release();
                    Err((AdmissionRefusal::Saturated, refused.payload))
                }
                crate::ReadyRefusal::SequencesExhausted => {
                    self.durable.release();
                    // Latched here rather than rediscovered on every later
                    // send, and never reset: reusing a position would answer
                    // one request with another's identity.
                    self.exhausted.store(true, Ordering::Release);
                    Err((AdmissionRefusal::Exhausted, refused.payload))
                }
            },
        }
    }

    /// Accept runnable work with nothing to hand over alongside it.
    fn accept(
        &self,
        class: crate::ReadyClass,
        operation: PrivateOperation,
    ) -> Result<crate::ReadySequence, (AdmissionRefusal, PrivateOperation)> {
        self.accept_with(class, operation, || Some(ControlAcceptance::ungoverned()))
    }

    /// Take the next entry, distinguishing an empty queue from an unusable one.
    ///
    /// Mapping a poisoned lock to `None` made an unreachable queue look drained
    /// and a run of it look like progress, while accepted work sat in it
    /// unanswered.
    fn take_next(
        &self,
    ) -> Result<Option<(crate::ReadySequence, crate::ReadyClass, PrivateOperation)>, ()> {
        let mut queue = self.ready.lock().map_err(|_| ())?;
        Ok(queue.ready.take_next())
    }
}

/// The shared queue and whether it is still being drained.
#[cfg(unix)]
struct SharedQueue {
    ready: crate::ReadyStream<PrivateOperation>,
    closed: bool,
}

/// Why the shared admission would not accept work.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionRefusal {
    /// No room now. Retrying later is sensible.
    Saturated,
    /// Positions are exhausted. Terminal: retrying cannot create an identity
    /// that does not exist.
    Exhausted,
    /// The shared queue cannot be reached.
    Unavailable,
    /// The consumer is gone. Nothing accepted now could ever run.
    ConsumerGone,
}

/// Why a private producer's work was not accepted.
///
/// Denial and saturation are different answers and a caller acts on them
/// differently: saturation says try again, denial says this will not be
/// accepted until something changes. The ordinary backend reports a stamp
/// refusal as `TrySendError::Full`, which tells a caller to retry work that is
/// being refused on policy. A private producer is told which it is.
#[cfg(unix)]
#[derive(Debug)]
pub enum PrivateSendError {
    /// No stamp: a transition is in flight, or routing is otherwise closed.
    /// The work is handed back, unaccepted.
    Denied(XAuthorityRoutedInput),
    /// The ingress is full. The work is handed back, and retrying is sensible.
    Saturated(XAuthorityRoutedInput),
    /// The consumer is gone.
    Disconnected(XAuthorityRoutedInput),
    /// This delivery id is already live. Retrying cannot help, and cancelling
    /// the live one would answer a different request.
    DeliveryAlreadyTracked(XAuthorityRoutedInput),
    /// The recovery ledger or the shared queue cannot be reached.
    Unavailable(XAuthorityRoutedInput),
    /// Positions are exhausted. Terminal for this instance: what was already
    /// accepted keeps its completion, and nothing further is taken.
    Exhausted(XAuthorityRoutedInput),
}

/// A producer's handle to a private frontend.
///
/// Offers one way in, and answers with a typed refusal. The ordinary sender is
/// deliberately not reachable through this: its `try_send` calls a policy
/// denial `Full`, which tells a caller to retry something that is being
/// refused.
#[cfg(unix)]
pub struct PrivateIngress {
    sender: XAuthorityRoutedInputSender,
    admission: Arc<SharedAdmission>,
}

/// A producer of control work, bound to one instance's shared admission.
#[cfg(unix)]
pub struct PrivateControlProducer {
    admission: Arc<SharedAdmission>,
    completion: ControlCompletionRegistry,
    /// Where this client's control writer is known from.
    ///
    /// The question is whether a writer exists that could execute this, and
    /// only the client's own route state answers it. A registration outlives
    /// its writer, and a not-known-revoked connection is weaker still.
    routing: XServerFrontendRouteRegistry,
}

#[cfg(unix)]
impl PrivateControlProducer {
    /// Accept control into the shared order.
    pub fn submit(
        &self,
        control: XAuthorityClientControlCommand,
    ) -> Result<crate::ReadySequence, (AdmissionRefusal, XAuthorityClientControlCommand)> {
        // Nothing is left to execute work for a client whose control writer
        // has stopped, so it is refused before anything is reserved for it.
        // Positive evidence, not the absence of a revocation.
        if !self.routing.control_writer_present(control.client) {
            return Err((AdmissionRefusal::ConsumerGone, control));
        }
        // Reserved before acceptance, and at the producer rather than at
        // either routing site: focus commands bypass one of those, and this is
        // the only point bound to the admission that accepted the work.
        let token = match self.completion.register(control) {
            Ok(token) => token,
            // Each refusal keeps its own meaning. Saturation says retry, a
            // sealed client says nothing is left to execute this, exhaustion
            // says no identity remains, and an unreadable registry says
            // nothing could be established -- and a caller acts on each of
            // those differently.
            Err((refusal, returned)) => {
                let refusal = match refusal {
                    ControlCompletionRefusal::AtCapacity => AdmissionRefusal::Saturated,
                    ControlCompletionRefusal::Exhausted => AdmissionRefusal::Exhausted,
                    ControlCompletionRefusal::Unavailable => AdmissionRefusal::Unavailable,
                };
                return Err((refusal, returned));
            }
        };
        self.admission
            .accept_with(
                crate::ReadyClass::Control,
                PrivateOperation::Control(control, Some(token)),
                // The handover and the queue entry are one transaction. The
                // instance owns the record exactly when it owns the
                // operation, and if the entry cannot be published the record
                // goes back into reserve with the command.
                || self.completion.begin_acceptance(token),
            )
            .map_err(|(refusal, returned)| match returned {
                PrivateOperation::Control(control, _) => {
                    // Refused, so the command goes back to the producer that
                    // still owns it, and the reservation goes with it. Nothing
                    // was ever handed over, so there is no second owner and
                    // nothing that could answer for it.
                    self.completion.release_reservation(token);
                    (refusal, control)
                }
                _ => unreachable!("control is returned as control"),
            })
    }
}

#[cfg(unix)]
impl PrivateIngress {
    /// Accept work into the shared order, stamping it first.
    ///
    /// Position is assigned as the entry is published, inside the shared
    /// admission's own hold, so a send that has returned cannot be overtaken
    /// by one that started afterwards.
    pub fn submit(&self, route: XAuthorityRoutedInput) -> Result<crate::ReadySequence, PrivateSendError> {
        let envelope = self.sender.stamp_and_reserve(route)?;
        self.admission
            .accept(
                crate::ReadyClass::RoutedInput,
                PrivateOperation::RoutedInput(envelope),
            )
            .map_err(|(refusal, returned)| {
                let route = match returned {
                    PrivateOperation::RoutedInput(envelope) => envelope.route,
                    _ => unreachable!("routed input is returned as routed input"),
                };
                // The reservation this send made is rolled back, and only
                // this one: another request's live delivery is untouched.
                self.sender.abort_reservation(route.delivery);
                match refusal {
                    AdmissionRefusal::Saturated => PrivateSendError::Saturated(route),
                    AdmissionRefusal::Exhausted => PrivateSendError::Exhausted(route),
                    AdmissionRefusal::Unavailable => PrivateSendError::Unavailable(route),
                    AdmissionRefusal::ConsumerGone => PrivateSendError::Disconnected(route),
                }
            })
    }
}
