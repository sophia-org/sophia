// One frame, its position on the wire, and how long its recipient kept it
// waiting.
//
// Split by subject because the subject is custody of a frame in flight. What
// goes on the wire is decided elsewhere; this owns the bytes from the moment
// they may leave until the moment the whole frame is known to have gone.
//
// Exercised by controls and not yet by a writer: this is the measurement and
// the custody, and the ordered path that will consume them is still being
// built. Marked rather than wired early, because turning accumulated waiting
// into a delivery's outcome is a separate decision from being able to measure
// it.

/// How long one delivery may spend waiting on its recipient before that
/// recipient is treated as unable to take it.
///
/// A declared policy, not a fact about sockets or clients. A recipient that
/// cannot take a frame within it has stopped being a recipient, and the
/// alternative is a writer that waits for one forever while everything behind
/// it waits for the writer.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
const X_AUTHORITY_ORDERED_BLOCKED_LIMIT: Duration = Duration::from_secs(6);

/// How long one wait may last before the accumulated total is looked at again.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
const X_AUTHORITY_ORDERED_BLOCKED_SLICE: Duration = Duration::from_millis(50);

/// Why a frame did not reach the socket.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
enum X11FrameSendFailure {
    /// Nothing is in hand to send.
    NoFrame,
    /// A finished frame is still in hand and has not been retired.
    ///
    /// Refused so that giving up a frame is something that happens once and
    /// on purpose. Overwriting one here would let a frame that went out be
    /// forgotten without anything having decided it was done with, and the
    /// count of frames that went would stop matching the frames that did.
    FrameHeld,
    /// A send was begun and never reported, so how much of the frame reached
    /// the wire is unknown. Not resumable and not retryable: the only honest
    /// disposition is to end the connection, because nothing can establish
    /// where the event stopped.
    Interrupted,
    /// A frame is still owed bytes, and something asked for a different one.
    ///
    /// Refused rather than accepted, because the wire already holds the
    /// beginning of an event: writing another frame now would put a second
    /// event's opening bytes inside the first one's body, and an X11 client
    /// has no way to notice that or recover from it.
    Incomplete { sent: usize, len: usize },
    /// This recipient did not take the rest of the frame within the limit.
    ///
    /// Carries how much of the frame went out, because that decides what can
    /// be done next and nothing else establishes it. Anything other than zero
    /// means the wire holds part of an event.
    Blocked { written: usize, blocked: Duration },
    /// Waiting for the recipient to become writable could not be performed.
    ///
    /// Nothing is established about the recipient by this: it was never asked
    /// and it never declined. Kept apart from blocking so that a deadline is
    /// never built out of a failed wait, and it is no more a settlement fact
    /// than a deadline is.
    WaitFailed(std::io::Error),
    /// The send failed for a reason of its own.
    Io(std::io::Error),
}

/// How much of the frame in hand is on the wire.
///
/// Three states rather than a count, because between handing bytes to the
/// kernel and recording that they went there is an interval in which an
/// interruption leaves the count behind and the bytes gone. A number alone
/// cannot say that happened, and a resume that trusted it would send the same
/// bytes twice.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11OrderedSendProgress {
    /// This many bytes of the frame have been accepted, and that is known.
    Sent(usize),
    /// A send was begun from this offset and never reported.
    Unknown { from: usize },
}

/// The frame itself, held for as long as any of it is still owed.
///
/// The bytes are owned here rather than borrowed from a caller. An offset into
/// somebody else's slice says how far through *something* the wire is, and the
/// next call can arrive with different bytes behind the same number -- which
/// would send the tail of one event as though it were the tail of another.
/// Owning them is what makes the offset mean anything.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct X11OrderedFrame<B> {
    bytes: B,
    progress: X11OrderedSendProgress,
}

/// What one delivery has in flight, and how long it has waited.
///
/// Owner-bound on purpose, and it owns the frame rather than a position in
/// one. Bytes handed to the kernel are gone whatever happens next, so the
/// thing that records them has to outlive every fallible send and every wait,
/// and it has to be the same bytes each time.
///
/// One of these belongs to exactly one delivery. Waiting is a fact about a
/// recipient and a delivery together, so an accumulator shared between them
/// would let an earlier stall be spent against a later deadline.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct X11OrderedSendState<B = Vec<u8>> {
    frame: Option<X11OrderedFrame<B>>,
    blocked: Duration,
}

#[cfg(unix)]
impl<B> Default for X11OrderedSendState<B> {
    fn default() -> Self {
        Self {
            frame: None,
            blocked: Duration::ZERO,
        }
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl<B: AsRef<[u8]>> X11OrderedSendState<B> {
    /// Take the next frame of this delivery, if the last one is finished.
    ///
    /// Refuses while anything is still owed. A frame that has had bytes
    /// accepted cannot be abandoned -- those bytes are an event's beginning
    /// and the recipient is waiting for the rest of it -- and a frame whose
    /// send never reported cannot be left behind either, because what the wire
    /// holds is unknown and a following frame would be appended to something
    /// nobody can describe.
    ///
    /// There is no way to declare a frame finished. Finished means every byte
    /// of the frame this state owns was accepted, which only sending can
    /// establish.
    fn begin_frame(&mut self, bytes: B) -> Result<(), X11FrameSendFailure> {
        match self
            .frame
            .as_ref()
            .map(|frame| (frame.progress, frame.bytes.as_ref().len()))
        {
            Some((X11OrderedSendProgress::Unknown { .. }, _)) => {
                return Err(X11FrameSendFailure::Interrupted);
            }
            Some((X11OrderedSendProgress::Sent(sent), len)) if sent < len => {
                return Err(X11FrameSendFailure::Incomplete { sent, len });
            }
            // A frame that finished is still in hand until it is retired.
            // Replacing it here would let a completed frame be forgotten
            // without anything having decided it was finished with.
            Some(_) => return Err(X11FrameSendFailure::FrameHeld),
            None => {}
        }
        self.frame = Some(X11OrderedFrame {
            bytes,
            progress: X11OrderedSendProgress::Sent(0),
        });
        Ok(())
    }

    /// Give up the frame in hand, once it is known to have gone whole.
    ///
    /// Consumptive: the frame leaves the send state, so nothing can retire the
    /// same frame twice and nothing can mistake a finished frame for a fresh
    /// one. A caller that retired twice would believe two frames had gone when
    /// one had, and the second of them would never have been begun at all.
    ///
    /// The waiting is not given up with it. That belongs to the delivery, not
    /// to any one of its frames, and a recipient that stalled on the first
    /// frame has kept this delivery waiting whatever the next one does.
    fn retire_frame(&mut self) -> Result<(), X11FrameSendFailure> {
        match self.frame.as_ref().map(|frame| frame.progress) {
            None => Err(X11FrameSendFailure::NoFrame),
            Some(X11OrderedSendProgress::Unknown { .. }) => {
                Err(X11FrameSendFailure::Interrupted)
            }
            Some(X11OrderedSendProgress::Sent(sent)) => {
                let len = self
                    .frame
                    .as_ref()
                    .expect("frame in hand")
                    .bytes
                    .as_ref()
                    .len();
                if sent < len {
                    return Err(X11FrameSendFailure::Incomplete { sent, len });
                }
                // The frame does not survive being retired. Its bytes go with
                // it: the next frame brings its own, already encoded, and
                // holding this one's storage back would keep a copy of an
                // event that has already gone.
                self.frame = None;
                Ok(())
            }
        }
    }

    fn blocked(&self) -> Duration {
        self.blocked
    }

    /// Whether everything the frame in hand owes has been accepted.
    ///
    /// Derived from what was sent, never set. A flag a caller could raise
    /// would be a claim about the wire made by something that cannot see it.
    fn frame_complete(&self) -> bool {
        self.frame.as_ref().is_some_and(|frame| {
            frame.progress == X11OrderedSendProgress::Sent(frame.bytes.as_ref().len())
        })
    }
}

/// Send what is left of the frame in hand without blocking the shared socket.
///
/// Every send is non-blocking for this call only: the flags are per-call, so
/// nothing about the socket changes and no other writer sharing it is
/// affected. A send that cannot proceed opens a waiting interval, and the
/// waiting is measured rather than assumed -- an accepted count says the
/// socket took bytes, never that taking them was instant.
///
/// What is counted is time spent waiting on this recipient for this delivery.
/// That is the only measurement a transport deadline may be built from: how
/// long a delivery has existed, how long it sat in a queue and how long a lock
/// was held all describe something other than a recipient that will not take
/// its bytes.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
fn send_pending_frame<B: AsRef<[u8]>>(
    socket: &UnixStream,
    state: &mut X11OrderedSendState<B>,
) -> Result<(), X11FrameSendFailure> {
    loop {
        let blocked_so_far = state.blocked;
        let Some(frame) = state.frame.as_mut() else {
            return Err(X11FrameSendFailure::NoFrame);
        };
        let X11OrderedSendProgress::Sent(offset) = frame.progress else {
            // A previous send never reported. Resuming would send from an
            // offset that may already be behind what the wire took, putting an
            // event's middle after its own middle.
            return Err(X11FrameSendFailure::Interrupted);
        };
        let frame_bytes = frame.bytes.as_ref();
        if offset >= frame_bytes.len() {
            return Ok(());
        }
        // Marked before the bytes can leave, not after they are counted. A
        // marker written afterwards says nothing about a call that did not
        // return, and this is the one interval where the wire can be ahead of
        // everything that describes it.
        frame.progress = X11OrderedSendProgress::Unknown { from: offset };
        let attempt = rustix::net::send(
            socket,
            &frame_bytes[offset..],
            rustix::net::SendFlags::DONTWAIT | rustix::net::SendFlags::NOSIGNAL,
        );
        match attempt {
            Ok(0) => {
                frame.progress = X11OrderedSendProgress::Sent(offset);
                return Err(X11FrameSendFailure::Io(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "X11 ordered writer made no progress on a frame",
                )));
            }
            // The send reported, so what it took is known again.
            Ok(count) => frame.progress = X11OrderedSendProgress::Sent(offset + count),
            Err(rustix::io::Errno::AGAIN) => {
                // Nothing left, so the offset is what it was.
                frame.progress = X11OrderedSendProgress::Sent(offset);
                // The socket would have blocked, which is what opens a waiting
                // interval. What is added is the interval that actually
                // elapsed, not the slice that was asked for: a wait can end
                // early on readiness and a wait can overrun.
                let waited = Instant::now();
                let mut watched = [rustix::event::PollFd::new(
                    socket,
                    rustix::event::PollFlags::OUT,
                )];
                let slice = rustix::fs::Timespec {
                    tv_sec: 0,
                    tv_nsec: i64::from(X_AUTHORITY_ORDERED_BLOCKED_SLICE.subsec_nanos()),
                };
                match rustix::event::poll(&mut watched, Some(&slice)) {
                    // Readiness or the slice expiring. Either way this call
                    // waited on the recipient, and that is what is counted.
                    Ok(_) => state.blocked = blocked_so_far + waited.elapsed(),
                    // A signal ended the wait early. Still waiting on the
                    // recipient, just less of it than was asked for.
                    Err(rustix::io::Errno::INTR) => {
                        state.blocked = blocked_so_far + waited.elapsed();
                    }
                    // The wait itself failed. Time passed, but none of it is
                    // evidence about this recipient: nothing was asked of it
                    // and nothing declined. Counting it would build a deadline
                    // out of a broken syscall, and the deadline is the one
                    // thing here a settlement is not allowed to be inferred
                    // from. The frame and its offset stay exactly as they are.
                    Err(error) => {
                        return Err(X11FrameSendFailure::WaitFailed(std::io::Error::from(
                            error,
                        )));
                    }
                }
                if state.blocked >= X_AUTHORITY_ORDERED_BLOCKED_LIMIT {
                    return Err(X11FrameSendFailure::Blocked {
                        written: offset,
                        blocked: state.blocked,
                    });
                }
            }
            Err(rustix::io::Errno::INTR) => {
                frame.progress = X11OrderedSendProgress::Sent(offset);
            }
            Err(error) => {
                frame.progress = X11OrderedSendProgress::Sent(offset);
                return Err(X11FrameSendFailure::Io(std::io::Error::from(error)));
            }
        }
    }
}

/// Read one frame's failure into the writer's own vocabulary.
///
/// A recipient that would not take its bytes is a failed recipient, not a
/// failed server. Sending it through the ordinary peer-write reading would
/// give it the fatal class, and one client that stopped reading would end the
/// service for every other.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
fn x11_ordered_frame_error(context: &str, failure: X11FrameSendFailure) -> X11SetupSocketError {
    match failure {
        X11FrameSendFailure::NoFrame => {
            X11SetupSocketError::new(format!("{context}: nothing was in hand to send"))
        }
        X11FrameSendFailure::FrameHeld => X11SetupSocketError::new(format!(
            "{context}: the frame in hand was never retired"
        )),
        X11FrameSendFailure::Interrupted => X11SetupSocketError::client_failure(format!(
            "{context}: a send never reported, so what reached this recipient is unknown"
        )),
        X11FrameSendFailure::Incomplete { sent, len } => {
            X11SetupSocketError::client_failure(format!(
                "{context}: {sent} of {len} bytes of the previous frame are still owed"
            ))
        }
        X11FrameSendFailure::Blocked { written, blocked } => {
            X11SetupSocketError::client_failure(format!(
                "{context}: recipient took {written} bytes of the frame and then nothing for \
                 {blocked:?}"
            ))
        }
        X11FrameSendFailure::WaitFailed(error) => X11SetupSocketError::new(format!(
            "{context}: waiting for this recipient could not be performed: {error}"
        )),
        X11FrameSendFailure::Io(error) => x11_peer_write_error(context, error),
    }
}
