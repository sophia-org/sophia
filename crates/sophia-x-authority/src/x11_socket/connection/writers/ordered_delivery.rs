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
/// One frame per call, so a writer serving several recipients cannot be held
/// by one of them, and so the waiting a stalled recipient causes is charged to
/// the delivery it belongs to rather than to whatever came after it.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
fn write_one_ordered_frame(
    socket: &UnixStream,
    in_flight: &mut Option<X11OrderedInFlight>,
    byte_order: XByteOrder,
    sequence: u16,
) -> Result<X11OrderedWriteStep, X11OrderedWriteFailure> {
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
