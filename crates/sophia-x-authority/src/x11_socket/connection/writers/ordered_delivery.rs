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
    slot: &mut Option<X11OrderedInFlight>,
) -> Result<(), X11OrderedTakeRefusal> {
    if slot.is_some() {
        return Err(X11OrderedTakeRefusal::InFlight);
    }
    match queue.try_recv() {
        Ok(delivery) => {
            *slot = Some(X11OrderedInFlight {
                delivery,
                frame: 0,
                send: X11OrderedSendState::default(),
            });
            Ok(())
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => Err(X11OrderedTakeRefusal::Empty),
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(X11OrderedTakeRefusal::Closed),
    }
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
    in_flight: &mut Option<X11OrderedInFlight>,
    queue: &Receiver<XAuthorityOrderedDelivery>,
    byte_order: XByteOrder,
    sequence: u16,
) -> X11OrderedServeStep {
    if in_flight.is_none() {
        match take_ordered_delivery(queue, in_flight) {
            Ok(()) => {}
            Err(X11OrderedTakeRefusal::Empty) => return X11OrderedServeStep::Idle,
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
        Ok(X11OrderedWriteStep::Wrote) => X11OrderedServeStep::Flushed,
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
            X11OrderedServeStep::Ended { outcome, shutdown }
        }
    }
}
