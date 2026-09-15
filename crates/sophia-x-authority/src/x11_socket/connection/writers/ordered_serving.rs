// One connection's ordered output: binding it, owning it, and ending it.
//
// Split by subject from the delivery mechanics beside it. That file answers
// how one capsule is taken and one frame written; this answers whose output
// this is, who may write it, and what happens to what it still holds when the
// connection ends.

/// Why a connection's ordered output could not be bound.
///
/// Carried out beside the resources it was given, never instead of them.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11OrderedServingRefusal {
    /// The receiver was minted by a different registration.
    ForeignReceiver,
    /// The connection's own output could not supply a second handle, so this
    /// binding could be made but never ended.
    TransportUnavailable,
    /// Room for what a close would have to retain could not be reserved, so
    /// this binding could be made but never closed without losing work.
    RetentionUnavailable,
    /// The registration names no endpoint: it is not admitted, or it is no
    /// longer the row this client currently has.
    Unadmitted(PrivateAdmissionRefusal),
}

/// One connection's ordered output transport, bound where both halves of it
/// are owned.
///
/// The receiver half is established: it carries the registration cell it was
/// minted with, so its provenance is asked here rather than asserted, and a
/// later holder cannot substitute it.
///
/// THE SOCKET HALF IS NOT ESTABLISHED YET. A file descriptor carries no
/// witness of which connection negotiated it, and inventing one would be a
/// claim rather than a check. What would make the pairing sound is binding
/// where the accepted stream and its registration are both in hand and neither
/// has been anywhere else -- and today every caller of `bind` is a test, while
/// connection setup still drops its ordered receiver. Until that caller
/// exists, retaining whatever socket is passed here proves nothing about its
/// origin, and this says so rather than describing a property it does not have.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The per-connection loop is not attached yet.
struct XAuthorityOrderedTransport {
    ordered: XAuthorityOrderedReceiver,
    /// This connection's actual serialized output.
    ///
    /// The one every other writer for this connection already goes through. A
    /// private descriptor of our own would write beside them rather than among
    /// them, which is what serialization is for.
    output: Arc<Mutex<UnixStream>>,
    /// A handle on the same connection that does not go through that lock.
    ///
    /// Ending a connection must not require the mutex a stalled write is
    /// holding -- that is exactly the case where ending it is what is needed.
    shutdown: UnixStream,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The per-connection loop is not attached yet.
impl XAuthorityOrderedTransport {
    /// Bind this connection's queue to this connection's own output.
    ///
    /// NO SOCKET IS ACCEPTED HERE. Taking one would be taking a caller's word
    /// for which connection it belongs to, which is the thing that cannot be
    /// checked. Both handles are derived from the connection's own serialized
    /// output instead, so what this binds is the queue a registration minted
    /// to the stream that registration was made for.
    ///
    /// Refusing hands the receiver back: it may already hold accepted events,
    /// and a binding that failed is not a licence to destroy them.
    #[allow(clippy::result_large_err)] // The receiver travels out rather than being dropped.
    fn bind(
        registration: &XServerFrontendClientRouteRegistration,
        ordered: XAuthorityOrderedReceiver,
        output: &Arc<Mutex<UnixStream>>,
    ) -> Result<Self, (X11OrderedServingRefusal, XAuthorityOrderedReceiver)> {
        if !ordered.minted_by(registration) {
            return Err((X11OrderedServingRefusal::ForeignReceiver, ordered));
        }
        // Taken before anything is owned, so a descriptor that cannot be had
        // refuses the binding rather than producing one that could never be
        // ended.
        let shutdown = match output.lock() {
            Ok(guard) => match guard.try_clone() {
                Ok(handle) => handle,
                Err(_) => {
                    drop(guard);
                    return Err((X11OrderedServingRefusal::TransportUnavailable, ordered));
                }
            },
            Err(_) => return Err((X11OrderedServingRefusal::TransportUnavailable, ordered)),
        };
        Ok(Self {
            ordered,
            output: output.clone(),
            shutdown,
        })
    }

}

/// Why a connection's ordered output is being closed.
///
/// DIAGNOSTIC ONLY, AND DELIBERATELY NOT AN OUTCOME. It never reaches the
/// authority. What a close can establish about a recipient is that the
/// connection to it ended; it cannot establish that anything was flushed, or
/// that a measured wait ran out, and a caller that could name the terminal
/// outcome could assert either of those by writing an enum.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The per-connection loop is not attached yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11OrderedCloseCause {
    /// The connection itself ended.
    ConnectionEnded,
    /// Preparation for this connection failed and what it had must be given up.
    PreparationFailed,
    /// The supervisor stopped this connection's service.
    SupervisorStopped,
}

/// A close in progress, retained by the owner that is closing.
///
/// EVERYTHING THE CLOSE HAS LEARNED LIVES HERE, beside the queue, the endpoint
/// and the in-flight slot the owner still holds. A close that drained into a
/// local and returned would lose whatever it had not finished the moment
/// anything went wrong, and would read an empty queue as a producer that had
/// stopped -- which it is not, while senders are still held elsewhere.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct X11OrderedClosing {
    /// Why, for whoever reads this afterwards. Never published.
    cause: X11OrderedCloseCause,
    /// Whether this close actually ended the wire, and what happened if not.
    ///
    /// A close that could not end the wire is still a close: ordinary serving
    /// is excluded from the moment it begins, because the alternative is a
    /// later write manufacturing a failure the completion records ahead of the
    /// ending this was trying to establish. But it is NOT a termination, and
    /// nothing derived from termination may be offered until it is one.
    termination: X11OrderedTermination,
    /// How many times ending the wire has actually been attempted.
    ///
    /// Counted so a retry is a retry: a repeated request that returned success
    /// because a close already existed acknowledged an ending nobody had tried
    /// again.
    attempts: u8,
    /// Answers this close established.
    answered: usize,
    /// Admissions the authority had already answered.
    already: usize,
    /// Offers the authority took reporting responsibility for under a claim it
    /// holds. Counted apart: a deferral is not a published answer, and a
    /// caller that treated it as one would stop looking.
    deferred: usize,
}

/// Whether a close has actually ended its connection.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11OrderedTermination {
    /// The wire is ended. Only this authorises an outcome derived from it.
    Established,
    /// Ending it was attempted and refused. The cause is kept; the connection
    /// may well still be carrying bytes.
    Refused(std::io::ErrorKind),
}

/// How many times one close may attempt to end its wire HERE.
///
/// THIS BOUNDS LOCAL EFFORT AND NOTHING ELSE. Reaching it does not finish the
/// close, does not establish termination, and does not stop anyone waiting on
/// this connection: it stops this owner from retrying, leaving an unconfirmed
/// close retained and undriven. Who drives it after that, and for how long, is
/// scheduling that does not exist yet.
#[cfg(unix)]
const X11_ORDERED_CLOSE_ATTEMPTS: u8 = 3;

/// What one bounded step of a close did.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11OrderedCloseStep {
    /// This close has not begun, so there is nothing to advance.
    NotClosing,
    /// One admission was offered an outcome and the authority took it.
    Adjudicated(PrivateAdjudication),
    /// One capsule was owed to another endpoint and is retained unanswered.
    Foreign,
    /// This close has not ended its connection, so nothing may be offered.
    ///
    /// Custody is untouched and ordinary serving stays excluded. Someone has to
    /// establish termination -- by a retry that succeeds, or by a continuation
    /// that takes the unconfirmed close on -- before this can go further.
    TerminationUnconfirmed(std::io::ErrorKind),
    /// Nothing can be received: what is retained is already at its bound.
    ///
    /// The queue and the slots are kept as they are, and this says so, because
    /// taking one more with nowhere to put it is how accepted work is lost.
    /// Someone has to take the retention on before this can go further.
    Backpressure,
    /// Nothing more is waiting RIGHT NOW.
    ///
    /// Not "the producer has stopped": senders for this queue may still be
    /// held elsewhere, so the receiver is kept rather than treated as closed.
    /// A caller that needs a real stop has to establish it, not infer it here.
    Quiet,
    /// The queue's producers are all gone, so nothing further can arrive.
    Drained,
}

/// One connection's ordered output, owned together.
///
/// THE ENDPOINT, THE QUEUE AND THE SOCKET ARE BOUND HERE. Passing them
/// separately to a serving call let a caller supply any three: the writer's
/// expectation could be refreshed between calls, or a queue and socket from
/// one connection served against another's identity. Bound together at
/// construction, from the registration that made them, there is nothing a
/// per-call caller can substitute.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The per-connection loop is not attached yet.
struct X11OrderedServingOwner {
    served: XAuthorityServedConnection,
    queue: Receiver<XAuthorityOrderedDelivery>,
    output: Arc<Mutex<UnixStream>>,
    shutdown: UnixStream,
    in_flight: Option<X11OrderedInFlight>,
    refused: Option<X11OrderedRefusedDelivery>,
    /// Set once this connection's wire could not be ended after a frame was
    /// left part-written. Nothing may be written through it again: the bytes
    /// on the wire are the beginning of an event nobody can finish.
    unterminated: bool,
    closing: Option<X11OrderedClosing>,
    /// How many capsules this owner may retain beyond the ones in its slots.
    ///
    /// The queue's own capacity, because that is the most it can be holding at
    /// any moment, and a retention policy has to come from the thing being
    /// retained rather than a number someone liked. Anything past it is
    /// backpressure for whoever takes this continuation on, not a bigger Vec.
    retention: usize,
    /// Admissions the authority would not take an offer for.
    ///
    /// Still owed, still owned, still carrying their own finalizers -- AND
    /// still carrying how far their bytes got. Keeping only the capsule threw
    /// away the frame index and send offset, which is part of what is unknown
    /// about a delivery whose report is still owed.
    ///
    /// Reserved when this owner is built, while construction can still refuse
    /// and hand its inputs back -- not when a close begins, by which point the
    /// wire may already be ended and there is nowhere to give anything back to.
    unanswered: Vec<X11OrderedInFlight>,
    /// Capsules that reached this queue owed to another endpoint. This socket
    /// ending is not evidence about their recipients, so they are neither
    /// offered an outcome nor discarded.
    foreign: Vec<X11OrderedRefusedDelivery>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))] // The per-connection loop is not attached yet.
impl X11OrderedServingOwner {
    /// Bind this connection's output to the registration that created it.
    ///
    /// ONE VALUE, ALREADY BOUND. This takes a bound transport rather than a
    /// queue and a socket a caller pairs up here. The receiver's provenance
    /// was asked when it was bound, against the registration; both output
    /// handles were derived there from the connection's own serialized output
    /// rather than supplied. Nothing about the association is asserted at this
    /// boundary -- though until connection setup is the caller that binds,
    /// which it is not yet, the connection whose output that was is whichever
    /// one the binder held.
    ///
    /// The identity is captured from the registration held here, not looked up
    /// by the client id it happens to carry, so a writer started for one
    /// registration cannot be handed the identity of the one that replaced it.
    ///
    /// EVERY REFUSAL HANDS BACK WHAT IT WAS GIVEN. The receiver may already
    /// hold accepted events and the socket is a live connection; dropping them
    /// on the way out of a failed preparation would destroy work that was
    /// admitted and answer for none of it. Nothing is moved into this owner
    /// until the fallible part has succeeded.
    #[allow(clippy::result_large_err)] // The transport travels out whole rather than being dropped.
    fn for_registration(
        frontend: &crate::x11_socket::PrivateXServerFrontend,
        registration: &XServerFrontendClientRouteRegistration,
        transport: XAuthorityOrderedTransport,
    ) -> Result<Self, (X11OrderedServingRefusal, XAuthorityOrderedTransport)> {
        // Asked again here, because a transport bound for one registration
        // must not prepare a writer for another even though both are opaque.
        if !transport.ordered.minted_by(registration) {
            return Err((X11OrderedServingRefusal::ForeignReceiver, transport));
        }
        let endpoint = match frontend.endpoint_for(registration) {
            Ok(endpoint) => endpoint,
            Err(refusal) => {
                return Err((X11OrderedServingRefusal::Unadmitted(refusal), transport));
            }
        };
        // Taken before the receiver is consumed, and the retention it implies
        // reserved before this owner exists: a close must never be the moment
        // room is first asked for.
        //
        // RESERVED FALLIBLY, WHILE THE TRANSPORT IS STILL WHOLE. Room that
        // cannot be had refuses the binding and hands everything back, rather
        // than producing an owner that would discover it at the one moment it
        // is holding custody with nowhere to put it.
        let retention = transport.ordered.capacity();
        let mut unanswered = Vec::new();
        let mut foreign = Vec::new();
        if unanswered.try_reserve_exact(retention).is_err()
            || foreign.try_reserve_exact(retention).is_err()
        {
            return Err((X11OrderedServingRefusal::RetentionUnavailable, transport));
        }
        Ok(Self {
            served: XAuthorityServedConnection::retained(endpoint),
            queue: transport.ordered.into_receiver(),
            output: transport.output,
            shutdown: transport.shutdown,
            in_flight: None,
            refused: None,
            unterminated: false,
            closing: None,
            retention,
            unanswered,
            foreign,
        })
    }

    /// Serve one step, writing through this connection's own serialization.
    ///
    /// The output lock is taken for the write. When the step below could not
    /// end the wire itself, this ends it through the handle that needs no
    /// lock, before the guard goes.
    ///
    /// PENDING, AND NOT YET TRUE OF THE CONNECTION. If that ending also fails,
    /// the guard is released -- it cannot be held across a return -- and the
    /// only thing stopping a later write is a latch private to THIS owner.
    /// Every other writer of this socket shares the same Arc and does not read
    /// it: the input, protocol and reply writers reach it through the
    /// non-control helper, and control writes take the stream directly. So in
    /// that one case serialization IS released while the wire holds the
    /// beginning of an event nobody can finish. Closing it needs a permission
    /// check under the shared serialization boundary that every post-exposure
    /// writer observes, which does not exist yet.
    fn serve_one(&mut self, byte_order: XByteOrder, sequence: u16) -> X11OrderedServeStep {
        if self.unterminated {
            return X11OrderedServeStep::Unterminated;
        }
        if self.closing.is_some() {
            // ORDINARY SERVING IS OVER. Once a close has begun -- whether or
            // not it managed to end the wire -- receiving, encoding or sending
            // here would drive this admission from two places at once, and a
            // write attempted on a socket the close shut down records a
            // failure in the completion cell ahead of the ending the close was
            // establishing. Two owners of one admission is the whole problem.
            return X11OrderedServeStep::Closing;
        }
        let Ok(socket) = self.output.lock() else {
            // Nothing was taken and nothing was written. An unusable transport
            // is its own answer, not an empty queue: reporting Idle would tell
            // a caller there was nothing to do while output was still owed.
            return X11OrderedServeStep::TransportUnavailable;
        };
        let step = serve_one_ordered_delivery(
            &socket,
            &self.served,
            &mut self.in_flight,
            &mut self.refused,
            &self.queue,
            byte_order,
            sequence,
        );
        let X11OrderedServeStep::Ended {
            outcome,
            shutdown: false,
        } = step
        else {
            return step;
        };
        // Still holding serialization, which is the point.
        let ended = match self.shutdown.shutdown(Shutdown::Both) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotConnected,
        };
        drop(socket);
        if ended {
            return X11OrderedServeStep::Ended {
                outcome,
                shutdown: true,
            };
        }
        self.unterminated = true;
        X11OrderedServeStep::Unterminated
    }

    /// Begin ending this connection's ordered output.
    ///
    /// THE OUTCOME IS NOT THE CALLER'S TO NAME. What a close establishes about
    /// a recipient is that the connection to it ended, and that is the only
    /// thing offered. A caller that could pass a terminal outcome could assert
    /// a flush for bytes never written, or a measured timeout that was never
    /// measured, through a real finalizer -- so it passes only a cause, which
    /// is diagnostic and never reaches the authority.
    ///
    /// ADMISSION STOPS AND THE WIRE ENDS FIRST. Nothing is offered until
    /// termination is established, because termination is the fact the outcome
    /// rests on. The socket is ended through the handle that does not need the
    /// output lock: a write stalled under that lock is exactly when ending has
    /// to work.
    ///
    /// Storage for what the close will hold is already reserved, so accepting
    /// custody never waits on an allocation.
    fn begin_close(&mut self, cause: X11OrderedCloseCause) -> Result<(), std::io::ErrorKind> {
        // RECORDED BEFORE THE FALLIBLE PART, and never unrecorded. Serving is
        // excluded from this instant, so a shutdown that refuses cannot leave
        // ordinary writing eligible on a connection somebody has decided to
        // end.
        //
        // A REPEATED REQUEST IS A RETRY, NOT AN ACKNOWLEDGEMENT. Returning
        // success because a close already existed reported an ending nobody
        // had attempted again, and left the connection carrying bytes while
        // its recipient was about to be told it had gone. The original cause
        // and identity are kept; only the attempt is repeated, and only while
        // there are attempts left.
        let closing = self.closing.get_or_insert(X11OrderedClosing {
            cause,
            termination: X11OrderedTermination::Refused(std::io::ErrorKind::Other),
            attempts: 0,
            answered: 0,
            already: 0,
            deferred: 0,
        });
        if closing.termination == X11OrderedTermination::Established {
            return Ok(());
        }
        if closing.attempts >= X11_ORDERED_CLOSE_ATTEMPTS {
            let X11OrderedTermination::Refused(kind) = closing.termination else {
                unreachable!("established returns above")
            };
            return Err(kind);
        }
        closing.attempts = closing.attempts.saturating_add(1);
        match self.shutdown.shutdown(Shutdown::Both) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotConnected => {}
            Err(error) => {
                closing.termination = X11OrderedTermination::Refused(error.kind());
                return Err(error.kind());
            }
        }
        closing.termination = X11OrderedTermination::Established;
        // A capsule already classified as another endpoint's moves into this
        // owner's keeping, still unanswered. Room for it was reserved when the
        // owner was built.
        if let Some(refused) = self.refused.take() {
            self.foreign.push(refused);
        }
        Ok(())
    }

    /// Advance a close by one bounded step.
    ///
    /// ONE CAPSULE PER VISIT, and every capsule is classified against the
    /// retained endpoint before anything is offered for it -- the same
    /// question the serving path asks, because closing this connection proves
    /// nothing about another endpoint's recipient. What is received is owned
    /// before it is judged and consumed only once the authority has taken an
    /// answer for it.
    ///
    /// AN EMPTY QUEUE IS NOT A STOPPED PRODUCER. Senders for it may still be
    /// held elsewhere, so a quiet visit keeps the receiver; only the producers
    /// actually being gone reports Drained.
    fn advance_close(&mut self, byte_order: XByteOrder, sequence: u16) -> X11OrderedCloseStep {
        let _ = (byte_order, sequence);
        let Some(closing) = self.closing.as_ref() else {
            return X11OrderedCloseStep::NotClosing;
        };
        // NOTHING DERIVED FROM AN ENDING THAT DID NOT HAPPEN. A close whose
        // shutdown was refused has excluded serving and may hold custody, but
        // the wire may still be carrying bytes and its recipient may still be
        // there -- so the one outcome this close would offer is not a fact yet.
        if let X11OrderedTermination::Refused(kind) = closing.termination {
            return X11OrderedCloseStep::TerminationUnconfirmed(kind);
        }
        // The one this owner was already serving comes first: it is this
        // endpoint's by construction, and it is owed an answer before anything
        // still waiting behind it. It is adjudicated WHERE IT LIES.
        if self.in_flight.is_some() {
            return self.adjudicate_in_flight();
        }
        // ROOM BEFORE CUSTODY. Nothing is received while what is already
        // retained is at its bound: a capsule taken with nowhere to put it is
        // one that gets lost at the next refusal.
        if self.unanswered.len() + self.foreign.len() >= self.retention {
            return X11OrderedCloseStep::Backpressure;
        }
        // Received into this owner's own slots and classified there, by the
        // same admission the serving path uses.
        match take_ordered_delivery(&self.queue, &self.served, &mut self.in_flight, &mut self.refused)
        {
            Ok(()) => self.adjudicate_in_flight(),
            Err(X11OrderedTakeRefusal::ForeignEndpoint)
            | Err(X11OrderedTakeRefusal::RefusedHeld) => {
                let refused = self.refused.take().expect("a refusal leaves it owned");
                self.foreign.push(refused);
                X11OrderedCloseStep::Foreign
            }
            Err(X11OrderedTakeRefusal::InFlight) => X11OrderedCloseStep::Quiet,
            Err(X11OrderedTakeRefusal::Empty) => X11OrderedCloseStep::Quiet,
            Err(X11OrderedTakeRefusal::Closed) => X11OrderedCloseStep::Drained,
        }
    }

    /// Offer the in-flight admission the only outcome a close establishes.
    ///
    /// ADJUDICATED BY BORROWING THE SLOT IT LIES IN. Taking it out first put
    /// the capsule in a local across the publication, and an interruption
    /// there left the admission owned by nobody -- its finalizer gone with the
    /// frame, its cell unanswered, and no record that it had ever existed. It
    /// is consumed only once the authority has returned a disposition, and
    /// then moved straight into storage reserved before it arrived.
    ///
    /// The in-flight frame and offset travel with it while the report is owed,
    /// because how far its bytes got is part of what is still unknown about it.
    fn adjudicate_in_flight(&mut self) -> X11OrderedCloseStep {
        // GUARDED AT THE SOURCE OF THE OFFER, not only where it is driven. A
        // later caller reaching this directly must not be able to publish an
        // ending that was never established.
        match self.closing.as_ref().map(|closing| closing.termination) {
            Some(X11OrderedTermination::Established) => {}
            Some(X11OrderedTermination::Refused(kind)) => {
                return X11OrderedCloseStep::TerminationUnconfirmed(kind);
            }
            None => return X11OrderedCloseStep::NotClosing,
        }
        let Some(held) = self.in_flight.as_ref() else {
            return X11OrderedCloseStep::Quiet;
        };
        let Some(finalizer) = held.delivery().finalizer().cloned() else {
            // Nothing carries its answer, so nothing here can give it one. It
            // moves between two places this owner already owns.
            let held = self.in_flight.take().expect("borrowed just above");
            self.unanswered.push(held);
            return X11OrderedCloseStep::Adjudicated(PrivateAdjudication::Refused);
        };
        // The connection to this recipient ended. That is the whole of what a
        // close knows about it. The capsule stays in its slot across this.
        let adjudication = finalizer.finalize(XAuthorityInputDeliveryOutcome::ClientDisconnected);
        let closing = self.closing.as_mut().expect("closing");
        match adjudication {
            PrivateAdjudication::Answered => closing.answered += 1,
            PrivateAdjudication::AlreadyAnswered => closing.already += 1,
            PrivateAdjudication::Deferred => closing.deferred += 1,
            PrivateAdjudication::Refused => {
                // Nothing was taken, so nothing is consumed. It moves from one
                // owned place to another with nothing fallible in between.
                let held = self.in_flight.take().expect("still in its slot");
                self.unanswered.push(held);
                return X11OrderedCloseStep::Adjudicated(adjudication);
            }
        }
        // Answered, already answered or deferred: the authority has it, so the
        // slot is given up.
        let _consumed = self.in_flight.take().expect("still in its slot");
        X11OrderedCloseStep::Adjudicated(adjudication)
    }

    fn retained_unanswered(&self) -> &[X11OrderedInFlight] {
        &self.unanswered
    }

    fn retained_foreign(&self) -> &[X11OrderedRefusedDelivery] {
        &self.foreign
    }

    /// How much room the retention stores actually hold.
    ///
    /// Read so a control can require that reservation was reservation: a store
    /// that grew while holding custody never had the room it claimed.
    fn retention_capacity(&self) -> (usize, usize) {
        (self.unanswered.capacity(), self.foreign.capacity())
    }

    fn closing(&self) -> Option<&X11OrderedClosing> {
        self.closing.as_ref()
    }

    fn in_flight(&self) -> Option<&X11OrderedInFlight> {
        self.in_flight.as_ref()
    }

    fn refused(&self) -> Option<&X11OrderedRefusedDelivery> {
        self.refused.as_ref()
    }
}
