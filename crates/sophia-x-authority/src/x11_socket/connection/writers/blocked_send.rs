// How long one recipient kept one delivery waiting.
//
// Split by subject because the subject is time, not content. What goes on the
// wire is decided elsewhere; this is only about a send that does not complete,
// which is the one thing a transport deadline may honestly be built from.

// Exercised by controls and not yet by a writer: this is the measurement, and
// the ordered path that will consume it is still being built. Marked rather
// than wired early, because turning accumulated waiting into a delivery's
// outcome is a separate decision from being able to measure it.
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
    /// This recipient did not take the rest of the frame within the limit.
    ///
    /// Carries how much of the frame went out, because that decides what can
    /// be done next and nothing else establishes it. Anything other than zero
    /// means the wire holds part of an event, which X11 can neither describe
    /// nor retract, so the connection is no longer usable.
    Blocked { written: usize, blocked: Duration },
    /// The send failed for a reason of its own.
    Io(std::io::Error),
}

/// What one delivery has already put on the wire, and how long it has waited.
///
/// Owner-bound on purpose. The offset is what the socket has accepted of the
/// frame in hand, and a local holding it is lost to an unwind while the bytes
/// it describes are already gone -- leaving the wire in a state nothing can
/// name. Whoever owns the delivery owns this, across every fallible send and
/// every wait.
///
/// One of these belongs to exactly one delivery. Waiting is a fact about a
/// recipient and a delivery together, so an accumulator shared between them
/// would let an earlier stall be spent against a later deadline, and a
/// delivery could be declared blocked on time it never waited.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Default)]
struct X11OrderedSendState {
    offset: usize,
    blocked: Duration,
}

#[cfg(unix)]
impl X11OrderedSendState {
    /// Begin another frame of the SAME delivery.
    ///
    /// The offset starts again because a new frame has had nothing accepted;
    /// the waiting does not, because it is this delivery's and it has already
    /// happened.
    #[cfg_attr(not(test), allow(dead_code))]
    fn begin_frame(&mut self) {
        self.offset = 0;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn blocked(&self) -> Duration {
        self.blocked
    }
}

/// Send what is left of one frame without blocking the shared socket.
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
fn send_frame_bounded(
    socket: &UnixStream,
    frame: &[u8],
    state: &mut X11OrderedSendState,
) -> Result<(), X11FrameSendFailure> {
    while state.offset < frame.len() {
        match rustix::net::send(
            socket,
            &frame[state.offset..],
            rustix::net::SendFlags::DONTWAIT | rustix::net::SendFlags::NOSIGNAL,
        ) {
            Ok(0) => {
                return Err(X11FrameSendFailure::Io(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "X11 ordered writer made no progress on a frame",
                )));
            }
            // Recorded in the owned state before anything else can fail. These
            // bytes are on the wire whatever happens next.
            Ok(count) => state.offset += count,
            Err(rustix::io::Errno::AGAIN) => {
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
                let _ = rustix::event::poll(&mut watched, Some(&slice));
                state.blocked += waited.elapsed();
                if state.blocked >= X_AUTHORITY_ORDERED_BLOCKED_LIMIT {
                    return Err(X11FrameSendFailure::Blocked {
                        written: state.offset,
                        blocked: state.blocked,
                    });
                }
            }
            Err(rustix::io::Errno::INTR) => {}
            Err(error) => {
                return Err(X11FrameSendFailure::Io(std::io::Error::from(error)));
            }
        }
    }
    Ok(())
}

/// Read one frame's failure into the writer's own vocabulary.
///
/// Waiting on the ordered writer with the rest of this file.
///
/// A recipient that would not take its bytes is a failed recipient, not a
/// failed server. Sending it through the ordinary peer-write reading would
/// give it the fatal class, and one client that stopped reading would end the
/// service for every other.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
fn x11_ordered_frame_error(context: &str, failure: X11FrameSendFailure) -> X11SetupSocketError {
    match failure {
        X11FrameSendFailure::Blocked { written, blocked } => {
            X11SetupSocketError::client_failure(format!(
                "{context}: recipient took {written} bytes of the frame and then nothing for \
                 {blocked:?}"
            ))
        }
        X11FrameSendFailure::Io(error) => x11_peer_write_error(context, error),
    }
}
