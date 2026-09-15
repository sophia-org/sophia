// One ordered delivery the writer has taken, for as long as any of it is owed.
//
// Split by subject from the frame's own custody: that file is about a frame's
// position on the wire, this is about which delivery the writer is answering
// for and how far through its frames it has got. A delivery outlives any one
// of its frames.

/// An ordered delivery the writer has taken and not yet finished.
///
/// Holds the capsule itself rather than anything copied out of it. The capsule
/// carries custody of a delivery and its origin and does not duplicate, so
/// this is the one place answering for it while it is in flight -- and a
/// writer that had copied out the parts it wanted would be answering for
/// something it could no longer name.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct X11OrderedInFlight {
    delivery: XAuthorityOrderedDelivery,
    /// Which frame of this delivery's emission is in hand.
    ///
    /// Advanced only when the frame before it is known to have gone. The frame
    /// custody refuses to begin another while one is owed, so this index and
    /// that refusal have to agree; it is incremented where that refusal would
    /// have fired, never beside it.
    frame: usize,
    send: X11OrderedSendState<PrivateOrderedFrame>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl X11OrderedInFlight {
    fn delivery(&self) -> &XAuthorityOrderedDelivery {
        &self.delivery
    }

    fn frame_index(&self) -> usize {
        self.frame
    }

    /// Move to the next frame, if the one in hand is finished.
    ///
    /// Refuses otherwise, for the reason the frame custody refuses: bytes
    /// already accepted are an event's beginning and the recipient is waiting
    /// for the rest of it. An index advanced past an unfinished frame would
    /// ask for the next event's bytes while the wire is still mid-event.
    fn advance_frame(&mut self) -> Result<(), X11FrameSendFailure> {
        // Retire first. The index and the send state are two accounts of the
        // same thing, and an index that moved while the frame stayed would let
        // the next advance succeed against a frame that had already been
        // counted -- skipping an emission frame nobody ever began.
        self.send.retire_frame()?;
        self.frame += 1;
        Ok(())
    }

    /// How long this delivery's recipient has kept it waiting.
    fn blocked(&self) -> Duration {
        self.send.blocked()
    }
}

/// Why an ordered delivery was not taken.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11OrderedTakeRefusal {
    /// Nothing was waiting.
    Empty,
    /// The producer is gone and nothing more will arrive.
    Closed,
    /// One is already in flight.
    ///
    /// Refused rather than queued behind it. This writer answers for the
    /// delivery it holds until that delivery is finished, and taking a second
    /// would leave the first owed by nobody while its frames are still
    /// half-written.
    InFlight,
    /// It was not minted for the endpoint this writer serves.
    ///
    /// FAILS CLOSED, BEFORE ANY BYTE. A frame written for another endpoint has
    /// been read by the time anything could notice, so the comparison happens
    /// at admission and nothing is encoded or written on a mismatch.
    ForeignEndpoint,
    /// A refused capsule is already held and has not been disposed of.
    ///
    /// Taking a second would overwrite the first, which is the one thing that
    /// loses it: the queue no longer has it and nothing else has taken it.
    RefusedHeld,
}

/// Why a capsule this writer holds may not be written.
///
/// A PRIVATE CAUSE, DELIBERATELY NOT A TERMINAL OUTCOME. What a mismatch
/// establishes is only that THIS endpoint is not authorised to emit THIS
/// capsule. It is not a flush, not a recipient ending, not a timeout and not a
/// failed write -- and every terminal outcome available says one of those. A
/// pre-effect cancellation is wrong too: the effect happened, which is why the
/// capsule exists. So the capsule keeps its own finalizer and stays unanswered
/// until something establishes a fact about the admission it came from.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11OrderedAdmissionRefusal {
    /// The capsule names an endpoint this writer does not serve.
    ForeignEndpoint,
    /// One already refused is still held, so nothing further was received.
    AlreadyHolding,
}

/// A capsule this writer took and may not write.
///
/// Held whole: the original capsule, its finalizer, its frames and its
/// identity are all still here, because the refusal is a fact about this
/// writer's entitlement and not about the capsule. Nothing is re-encoded,
/// re-addressed or given a replacement finalizer.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
struct X11OrderedRefusedDelivery {
    delivery: XAuthorityOrderedDelivery,
    /// None while the capsule has been received and not yet judged.
    ///
    /// That state is owned and reachable: the capsule lands here before
    /// anything decides about it, so an interruption between receiving and
    /// judging leaves a capsule that is owned, unwritten and unclassified --
    /// which is something an owner can find, rather than a capsule that was in
    /// a local when the frame went.
    cause: Option<X11OrderedAdmissionRefusal>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl X11OrderedRefusedDelivery {
    fn delivery(&self) -> &XAuthorityOrderedDelivery {
        &self.delivery
    }
    fn cause(&self) -> Option<X11OrderedAdmissionRefusal> {
        self.cause
    }
    /// Who these bytes were owed to, which is not this writer's endpoint.
    fn client(&self) -> XServerFrontendClientId {
        self.delivery.client()
    }
}

/// Take the next ordered delivery straight into the writer's own storage.
///
/// The capsule is placed where it will be answered for, in the same expression
/// that receives it. A caller that received it into a local and then stored it
/// would have a window in which the queue had given the delivery up and
/// nothing had taken responsibility for it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
fn take_ordered_delivery(
    queue: &Receiver<XAuthorityOrderedDelivery>,
    served: &XAuthorityServedConnection,
    slot: &mut Option<X11OrderedInFlight>,
    refused: &mut Option<X11OrderedRefusedDelivery>,
) -> Result<(), X11OrderedTakeRefusal> {
    if slot.is_some() {
        return Err(X11OrderedTakeRefusal::InFlight);
    }
    if refused.is_some() {
        return Err(X11OrderedTakeRefusal::RefusedHeld);
    }
    // RECEIVED INTO OWNED STORAGE FIRST, unclassified, in the same expression
    // that receives it. The capsule is never held in a local across the
    // decision about it.
    *refused = match queue.try_recv() {
        Ok(delivery) => Some(X11OrderedRefusedDelivery {
            delivery,
            cause: None,
        }),
        Err(std::sync::mpsc::TryRecvError::Empty) => return Err(X11OrderedTakeRefusal::Empty),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            return Err(X11OrderedTakeRefusal::Closed);
        }
    };
    // THEN JUDGED BY BORROWING THAT STORAGE. A refused capsule is classified
    // where it already lies; an admitted one moves between two places this
    // writer owns with nothing fallible in between.
    let held = refused.as_mut().expect("just received into this slot");
    if !served.admits(&held.delivery) {
        held.cause = Some(X11OrderedAdmissionRefusal::ForeignEndpoint);
        return Err(X11OrderedTakeRefusal::ForeignEndpoint);
    }
    let admitted = refused.take().expect("held just above");
    *slot = Some(X11OrderedInFlight {
        delivery: admitted.delivery,
        frame: 0,
        send: X11OrderedSendState::default(),
    });
    Ok(())
}

/// What one writing step did for the delivery in hand.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11OrderedWriteStep {
    /// Nothing is in flight.
    Idle,
    /// One frame went out whole and was retired.
    Advanced { frame: usize },
    /// Every frame this delivery owed has gone.
    ///
    /// Says the bytes went, and nothing more. Whether the recipient received
    /// them is the writer's own outcome to establish later, and whether the
    /// delivery's debt is settled is a question neither this step nor a queue
    /// can answer.
    Wrote,
}

/// Why a writing step could not be taken.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
enum X11OrderedWriteFailure {
    /// The frame could not be sent.
    Send(X11FrameSendFailure),
    /// The emission offered no frame at an index it said it had.
    ///
    /// Its own failure rather than a quiet completion: a delivery that stopped
    /// early here would report itself written with an event missing, and the
    /// count and the encoder disagreeing is a fact worth surfacing rather than
    /// rounding off.
    #[allow(dead_code)]
    MissingFrame { frame: usize, of: usize },
}

/// Write one frame of the delivery in hand, resuming one already begun.
///
/// A frame is encoded only when none is in hand. A send that stopped part way
/// left bytes on the wire and an offset that describes them, so encoding again
/// would produce a second copy of the frame those bytes came from and resume
/// into the middle of it.
///
/// One frame per call bounds how many frames a call writes. It does NOT bound
/// how long the call takes: sending waits for writability and keeps waiting,
/// through as much of the six-second blocking allowance as one stalled
/// recipient needs. A single call can therefore hold its thread for that whole
/// allowance.
///
/// So this belongs to a writer serving one recipient, where blocking that long
/// is the recipient's own problem and nobody else waits behind it. It must not
/// be driven from the service runner or from any interval that schedules
/// several recipients: one recipient that stopped reading would stall every
/// other, and the fairness that a bounded frame count appears to give is not
/// fairness in time.
///
/// It belongs to a recipient-owned writer, one per connection, bounded by the
/// configured client limit -- the arrangement the ordinary input writer already
/// has. No thread per frame or per delivery, and the supervisor keeps its
/// ability to shut the socket down while this waits.
///
/// What the per-call bound does give is that the waiting a stalled recipient
/// causes is charged to the delivery it belongs to, rather than to whatever
/// came after it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
fn write_one_ordered_frame(
    socket: &UnixStream,
    in_flight: &mut Option<X11OrderedInFlight>,
    byte_order: XByteOrder,
    sequence: u16,
) -> Result<X11OrderedWriteStep, X11OrderedWriteFailure> {
    // WIRE CUSTODY IS THE CALLER'S OBLIGATION, and this call does not discharge
    // it. Several ways out leave an incomplete or unknown frame owned here --
    // the blocking allowance exhausted, a wait that could not be performed, a
    // send that failed, a send that never reported -- and none of them closes
    // the socket. Nothing in this function closes a socket at all.
    //
    // So a caller holding this socket's output serialization must either keep
    // holding it until the frame completes, or establish that the socket is
    // shut down before releasing it, on EVERY exit that leaves a frame
    // incomplete or unknown. Releasing it otherwise admits a control or
    // protocol write into the body of a half-written event, which X11 can
    // neither describe nor recover from.
    //
    // A failed wait is not the recipient's fault and must not be recorded as
    // one to make this easier: the socket is equally unusable either way, and
    // the blame is a separate fact from the custody.

    let Some(held) = in_flight.as_mut() else {
        return Ok(X11OrderedWriteStep::Idle);
    };
    let frames = held.delivery().emission().frame_count();
    if held.frame_index() >= frames {
        return Ok(X11OrderedWriteStep::Wrote);
    }
    if held.send.frame.is_none() {
        let frame = held
            .delivery()
            .emission()
            .encode_frame(held.frame_index(), byte_order, sequence)
            .ok_or(X11OrderedWriteFailure::MissingFrame {
                frame: held.frame_index(),
                of: frames,
            })?;
        held.send
            .begin_frame(frame)
            .map_err(X11OrderedWriteFailure::Send)?;
    }
    send_pending_frame(socket, &mut held.send).map_err(X11OrderedWriteFailure::Send)?;
    let frame = held.frame_index();
    held.advance_frame().map_err(X11OrderedWriteFailure::Send)?;
    Ok(X11OrderedWriteStep::Advanced { frame })
}

/// What one serving step did for the recipient it answers.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
enum X11OrderedServeStep {
    /// Nothing was waiting and nothing is in flight.
    Idle,
    /// Every frame went, and the answer could not be adjudicated.
    ///
    /// Custody is retained: this writer still owes this delivery an answer,
    /// and giving the slot up would leave bytes the recipient has with nothing
    /// owning the report of them.
    Unanswered,
    /// One frame of the delivery in hand went out; more are owed.
    Advanced,
    /// Every frame of one delivery has gone.
    ///
    /// This is what an established flush means, and it is the only step that
    /// may be reported as one. Whether the recipient has read the bytes is a
    /// question nobody here can answer and this does not claim to.
    Flushed,
    /// The connection is finished, and this step finished it.
    ///
    /// `outcome` is what the recipient's debt should be answered with.
    /// `shutdown` says the socket was closed before this returned -- which is
    /// the whole point of ending here rather than reporting upward: the caller
    /// must not be able to release output serialization while the wire holds
    /// the beginning of an event nobody can finish.
    Ended {
        outcome: XAuthorityInputDeliveryOutcome,
        shutdown: bool,
    },
    /// A capsule on this queue was not minted for the endpoint served.
    ///
    /// REPORTED AS ITSELF, not as Idle, Flushed or Ended. Nothing was encoded
    /// or written, so nothing is owed on the wire and there is no
    /// half-finished event to shut down around -- and the socket of the
    /// endpoint this writer does serve is left alone, because it did nothing
    /// wrong. The capsule stays owned in this writer's own slot with its
    /// cause, and unanswered: a writer that was never entitled to these bytes
    /// is not the thing that decides any recipient's outcome. Nothing yet
    /// counts it as outstanding work or hands it to a durable owner -- that
    /// reporting does not exist, and the slot is the whole of where it lives.
    AdmissionRefused(X11OrderedAdmissionRefusal),
    /// This connection's own output could not be taken.
    ///
    /// Nothing was received, written or disposed of. Told apart from Idle
    /// because an empty queue and an unusable transport are opposite facts:
    /// one says there is nothing to do, the other says something is owed and
    /// cannot be done.
    TransportUnavailable,
    /// The wire holds the beginning of an event nobody can finish, and could
    /// not be ended.
    ///
    /// The connection is latched here: nothing more is written through it,
    /// because anything written would follow a half-finished frame. Custody of
    /// whatever is held stays with this owner.
    Unterminated,
}

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
    /// Answers this close established.
    answered: usize,
    /// Admissions the authority had already answered.
    already: usize,
    /// Offers the authority took reporting responsibility for under a claim it
    /// holds. Counted apart: a deferral is not a published answer, and a
    /// caller that treated it as one would stop looking.
    deferred: usize,
    /// Admissions the authority would not take an offer for. Still owed, still
    /// owned, still carrying their own finalizers.
    unanswered: Vec<XAuthorityOrderedDelivery>,
    /// Capsules that reached this queue owed to another endpoint. This socket
    /// ending is not evidence about their recipients, so they are neither
    /// offered an outcome nor discarded.
    foreign: Vec<X11OrderedRefusedDelivery>,
}

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
    /// Nothing more is waiting RIGHT NOW.
    ///
    /// Not "the producer has stopped": senders for this queue may still be
    /// held elsewhere, so the receiver is kept rather than treated as closed.
    /// A caller that needs a real stop has to establish it, not infer it here.
    Quiet,
    /// The queue's producers are all gone, so nothing further can arrive.
    Drained,
}

/// One connection's ordered output, owned together./// One connection's ordered output, owned together.
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
}

/// How much a close keeps room for before it accepts anything.
///
/// Reserved when the owner is built, so a close never allocates while it is
/// holding custody it has nowhere to put.
#[cfg(unix)]
const X11_ORDERED_CLOSE_RESERVE: usize = 8;

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
        Ok(Self {
            served: XAuthorityServedConnection::retained(endpoint),
            queue: transport.ordered.into_receiver(),
            output: transport.output,
            shutdown: transport.shutdown,
            in_flight: None,
            refused: None,
            unterminated: false,
            closing: None,
        })
    }

    /// Serve one step, writing through this connection's own serialization.
    ///
    /// The output lock is taken for the write. It is NOT released while this
    /// connection's wire holds the beginning of an event nobody can finish:
    /// when the step below could not end the wire itself, this ends it through
    /// the handle that needs no lock, before the guard goes. If even that
    /// fails, the connection is latched unusable rather than handed back to a
    /// caller who would write into a half-finished frame.
    fn serve_one(&mut self, byte_order: XByteOrder, sequence: u16) -> X11OrderedServeStep {
        if self.unterminated {
            return X11OrderedServeStep::Unterminated;
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
        if self.closing.is_some() {
            return Ok(());
        }
        match self.shutdown.shutdown(Shutdown::Both) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotConnected => {}
            Err(error) => return Err(error.kind()),
        }
        let mut closing = X11OrderedClosing {
            cause,
            answered: 0,
            already: 0,
            deferred: 0,
            unanswered: Vec::with_capacity(X11_ORDERED_CLOSE_RESERVE),
            foreign: Vec::with_capacity(X11_ORDERED_CLOSE_RESERVE),
        };
        // A capsule already classified as another endpoint's moves into the
        // close's own keeping, still unanswered.
        if let Some(refused) = self.refused.take() {
            closing.foreign.push(refused);
        }
        self.closing = Some(closing);
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
        if self.closing.is_none() {
            return X11OrderedCloseStep::NotClosing;
        }
        // The one this owner was already serving comes first: it is this
        // endpoint's by construction, and it is owed an answer before anything
        // still waiting behind it.
        if let Some(held) = self.in_flight.take() {
            return self.adjudicate_closing(held.delivery);
        }
        // Received into this owner's own slots and classified there, by the
        // same admission the serving path uses.
        match take_ordered_delivery(&self.queue, &self.served, &mut self.in_flight, &mut self.refused)
        {
            Ok(()) => {
                let held = self.in_flight.take().expect("admitted into the slot");
                self.adjudicate_closing(held.delivery)
            }
            Err(X11OrderedTakeRefusal::ForeignEndpoint)
            | Err(X11OrderedTakeRefusal::RefusedHeld) => {
                let refused = self.refused.take().expect("a refusal leaves it owned");
                self.closing
                    .as_mut()
                    .expect("closing")
                    .foreign
                    .push(refused);
                X11OrderedCloseStep::Foreign
            }
            Err(X11OrderedTakeRefusal::InFlight) => X11OrderedCloseStep::Quiet,
            Err(X11OrderedTakeRefusal::Empty) => X11OrderedCloseStep::Quiet,
            Err(X11OrderedTakeRefusal::Closed) => X11OrderedCloseStep::Drained,
        }
    }

    /// Offer one admitted capsule the only outcome a close establishes.
    fn adjudicate_closing(&mut self, capsule: XAuthorityOrderedDelivery) -> X11OrderedCloseStep {
        let closing = self.closing.as_mut().expect("closing");
        let Some(finalizer) = capsule.finalizer().cloned() else {
            // Nothing carries its answer, so nothing here can give it one.
            closing.unanswered.push(capsule);
            return X11OrderedCloseStep::Adjudicated(PrivateAdjudication::Refused);
        };
        // The connection to this recipient ended. That is the whole of what a
        // close knows about it.
        let adjudication = finalizer.finalize(XAuthorityInputDeliveryOutcome::ClientDisconnected);
        match adjudication {
            PrivateAdjudication::Answered => closing.answered += 1,
            PrivateAdjudication::AlreadyAnswered => closing.already += 1,
            PrivateAdjudication::Deferred => closing.deferred += 1,
            PrivateAdjudication::Refused => closing.unanswered.push(capsule),
        }
        X11OrderedCloseStep::Adjudicated(adjudication)
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

/// Serve one step of one recipient's ordered queue.
///
/// THE WIRE CUSTODY OBLIGATION IS DISCHARGED HERE. write_one_ordered_frame
/// says plainly that it closes nothing and leaves an incomplete or unknown
/// frame owned by its caller; this is that caller. Every failure below leaves
/// a frame owed or its extent unknown, so every one of them shuts the socket
/// down BEFORE returning. A caller that released serialization after one of
/// these without the socket being closed would admit a control or protocol
/// write into the body of a half-written event, which X11 can neither
/// describe nor recover from.
///
/// NOTHING IS EVER RESENT. A frame whose extent is unknown is not retried,
/// and no delivery is re-encoded: the only dispositions are finishing it or
/// ending the connection.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
fn serve_one_ordered_delivery(
    socket: &UnixStream,
    served: &XAuthorityServedConnection,
    in_flight: &mut Option<X11OrderedInFlight>,
    refused: &mut Option<X11OrderedRefusedDelivery>,
    queue: &Receiver<XAuthorityOrderedDelivery>,
    byte_order: XByteOrder,
    sequence: u16,
) -> X11OrderedServeStep {
    if in_flight.is_none() {
        match take_ordered_delivery(queue, served, in_flight, refused) {
            Ok(()) => {}
            Err(X11OrderedTakeRefusal::Empty) => return X11OrderedServeStep::Idle,
            Err(X11OrderedTakeRefusal::ForeignEndpoint) => {
                return X11OrderedServeStep::AdmissionRefused(
                    X11OrderedAdmissionRefusal::ForeignEndpoint,
                );
            }
            Err(X11OrderedTakeRefusal::RefusedHeld) => {
                return X11OrderedServeStep::AdmissionRefused(
                    X11OrderedAdmissionRefusal::AlreadyHolding,
                );
            }
            Err(X11OrderedTakeRefusal::Closed) => {
                // The producer is gone and nothing more will arrive. Nothing
                // is owed on the wire, so this is an ending without a
                // shutdown to perform.
                return X11OrderedServeStep::Ended {
                    outcome: XAuthorityInputDeliveryOutcome::ClientDisconnected,
                    shutdown: false,
                };
            }
            Err(X11OrderedTakeRefusal::InFlight) => return X11OrderedServeStep::Advanced,
        }
    }
    match write_one_ordered_frame(socket, in_flight, byte_order, sequence) {
        Ok(X11OrderedWriteStep::Advanced { .. }) => X11OrderedServeStep::Advanced,
        Ok(X11OrderedWriteStep::Wrote) => {
            // ANSWERED FIRST, RETIRED AFTER. Every frame has gone, and the
            // delivery stays in custody across the answer: taking it out first
            // put the finished capsule in a local across the publication, and
            // an interruption there left the bytes read by the recipient with
            // nothing owning the report of them.
            //
            // The answer goes through the finalizer this capsule carried from
            // the debt that owns it, which adjudicates it in the one place
            // that owns terminal outcomes. Nothing looks a delivery id up
            // here: by now the id may name a different admission.
            let adjudication = in_flight
                .as_ref()
                .and_then(|held| held.delivery().finalizer().cloned())
                .map_or(PrivateAdjudication::Refused, |finalizer| {
                    finalizer.finalize(XAuthorityInputDeliveryOutcome::Flushed)
                });
            // Taken by the authority, in any of the ways it can take one. Only
            // a refusal leaves this writer still owing an answer.
            let answered = !matches!(adjudication, PrivateAdjudication::Refused);
            if !answered {
                // Nothing was adjudicated, so this delivery is still owed an
                // answer and is still owed BY THIS WRITER. Custody stays.
                return X11OrderedServeStep::Unanswered;
            }
            // Confirmed, so the slot is given up. Once: the take is what makes
            // a second report impossible.
            let _finished = in_flight.take().expect("a delivery was in flight");
            X11OrderedServeStep::Flushed
        }
        Ok(X11OrderedWriteStep::Idle) => X11OrderedServeStep::Idle,
        Err(failure) => {
            // A recipient that did not take its bytes within the allowance is
            // a failed recipient, and is told apart from a writer that could
            // not write. Neither settles anything by itself; both end the
            // connection, because both leave a frame owed.
            let outcome = match &failure {
                X11OrderedWriteFailure::Send(X11FrameSendFailure::Blocked { .. }) => {
                    XAuthorityInputDeliveryOutcome::TimedOut
                }
                _ => XAuthorityInputDeliveryOutcome::WriteFailed,
            };
            let shutdown = socket.shutdown(std::net::Shutdown::Both).is_ok();
            // Answered through the carried finalizer as well, and while the
            // delivery is still in custody. A delivery that ended badly is
            // still answered, and answered to the admission it belonged to
            // through the authority that owns the answer.
            if let Some(finalizer) = in_flight
                .as_ref()
                .and_then(|held| held.delivery().finalizer().cloned())
            {
                finalizer.finalize(outcome);
            }
            X11OrderedServeStep::Ended { outcome, shutdown }
        }
    }
}
