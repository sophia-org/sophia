// Sending a frame, and how long the sending waited.
//
// Split by subject because the subject is time, not content. What a writer
// puts on the wire is decided elsewhere; this is only about a send that does
// not complete promptly, which is the one thing a transport deadline may
// honestly be built from.

/// How long one send may spend waiting before the recipient is treated as
/// unable to take it.
///
/// A declared policy of this crate, not a fact about sockets or clients. It
/// exists so that a writer blocked on a recipient has a bound at all; a
/// recipient that cannot take a frame within it has stopped being a recipient,
/// and the alternative is a writer that waits for one forever while everything
/// behind it waits for the writer.
#[cfg(unix)]
const X_AUTHORITY_WRITER_BLOCKED_LIMIT: Duration = Duration::from_secs(2);

/// How long each wait is allowed to last before the writer looks at the clock.
///
/// Small enough that the limit above is observed with some precision, large
/// enough that an ordinary busy recipient is not woken constantly.
#[cfg(unix)]
const X_AUTHORITY_WRITER_BLOCKED_SLICE: Duration = Duration::from_millis(50);

/// Why a frame did not reach the socket.
#[cfg(unix)]
#[derive(Debug)]
enum X11FrameSendFailure {
    /// The recipient did not take the whole frame within the limit.
    ///
    /// Carries how much of the frame went out, because that decides what can
    /// be done next and nothing else can establish it. Anything other than
    /// zero means the wire holds part of an event: X11 has no way to describe
    /// a partial event and no way to retract one, so the connection is no
    /// longer usable and the only honest disposition is to end it.
    Blocked { written: usize, blocked: Duration },
    /// The send failed for a reason of its own.
    Io(std::io::Error),
}

/// Write one whole frame, adding what it spends waiting to `blocked`.
///
/// Resumable by offset rather than retried from the start. A send that placed
/// part of a frame cannot be repeated -- the bytes already gone are gone, and
/// beginning again would put an event's opening bytes after its own middle.
///
/// What is counted is time this call actually waited on the recipient. That is
/// the only measurement a transport deadline may be built from: how long a
/// delivery has existed, how long it sat in a queue and how long a lock was
/// held all describe something other than a recipient that will not take its
/// bytes.
#[cfg(unix)]
fn send_frame_accounted(
    stream: &mut UnixStream,
    frame: &[u8],
    blocked: &mut Duration,
) -> Result<(), X11FrameSendFailure> {
    let mut written = 0;
    while written < frame.len() {
        let waited = Instant::now();
        match std::io::Write::write(stream, &frame[written..]) {
            Ok(0) => {
                return Err(X11FrameSendFailure::Io(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "X11 writer made no progress on a frame",
                )));
            }
            Ok(count) => written += count,
            // A wait that ended with nothing taken. The clock is read from
            // before the call, so what is added is the waiting itself rather
            // than an assumption about how long a slice lasts.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                *blocked += waited.elapsed();
                if *blocked >= X_AUTHORITY_WRITER_BLOCKED_LIMIT {
                    return Err(X11FrameSendFailure::Blocked {
                        written,
                        blocked: *blocked,
                    });
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(X11FrameSendFailure::Io(error)),
        }
    }
    Ok(())
}

/// Give a stream the write timeout this accounting depends on.
///
/// Without it a send waits in the kernel with no way to observe that it is
/// waiting, so nothing downstream can tell a recipient that is slow from one
/// that will never take another byte. Installed by the writer on its own
/// stream, once, before anything is sent.
#[cfg(unix)]
fn install_writer_blocked_accounting(stream: &UnixStream) -> std::io::Result<()> {
    stream.set_write_timeout(Some(X_AUTHORITY_WRITER_BLOCKED_SLICE))
}

/// Read one frame's failure into the writer's own vocabulary.
///
/// A recipient that would not take its bytes is a failed recipient, not a
/// failed server. Sending it through the ordinary peer-write reading would
/// give it the fatal class, and one client that stopped reading would end the
/// service for every other.
#[cfg(unix)]
fn x11_writer_frame_error(context: &str, failure: X11FrameSendFailure) -> X11SetupSocketError {
    match failure {
        X11FrameSendFailure::Blocked { written, blocked } => {
            X11SetupSocketError::client_failure(format!(
                "{context}: recipient took {written} bytes of the frame and \
                 then nothing for {blocked:?}"
            ))
        }
        X11FrameSendFailure::Io(error) => x11_peer_write_error(context, error),
    }
}
