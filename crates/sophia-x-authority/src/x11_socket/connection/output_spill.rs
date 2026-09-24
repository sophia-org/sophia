// One connection's output to its client, and what happens when the kernel
// will not take it.
//
// EVERY BYTE LEAVES THROUGH HERE, under the one output mutex every writer of
// this socket already serialises on, and in the order the mutex admitted it.
// What the kernel's send buffer refuses is not waited for: it is kept, record
// by record, in a spill that a drain thread moves into the kernel as the
// client reads. So the thread that reads the client's requests never waits on
// that client reading its replies -- which is what a client that writes a
// burst before it reads used to deadlock (t165, XTS5's TOO_LONG purpose).
//
// TWO BOUNDS END A CLIENT THAT WILL NOT TAKE ITS OUTPUT. A byte bound on what
// may be owed, for a client that keeps writing and never reads; and a silence
// allowance, for a client that neither reads nor writes while output is owed,
// which is what a watcher that stopped draining looks like once nothing
// blocks on it. Both end the connection the way a departed peer does, drop
// the spill, and say so in one record. Neither is reached by a client that
// reads. The model is `validation/tla/X11ClientOutputSpill.tla`.
//
// The socket stays blocking: reads are the reader's, unchanged, and only the
// sends here carry `MSG_DONTWAIT`.

/// How much a connection may owe its client before it is ended: sixteen
/// mebibytes, hundreds of thousands of error records, and far past what any
/// reading client falls behind by.
#[cfg(unix)]
pub const X_AUTHORITY_CLIENT_OUTPUT_SPILL_LIMIT: usize = 16 << 20;

/// How long output may be owed with no drain progress and no request read
/// before the client is treated as gone. The same allowance the private
/// ordered writer already declares for one delivery.
#[cfg(unix)]
pub const X_AUTHORITY_CLIENT_OUTPUT_SILENCE_LIMIT: Duration = Duration::from_secs(6);

/// How long the drain waits between looks, and how long it polls for room.
#[cfg(unix)]
const X11_OUTPUT_DRAIN_SLICE: Duration = Duration::from_millis(50);

/// A record the kernel refused, kept whole with what is still to send of it
/// and, until its first byte has gone, the descriptors that travel with it.
#[cfg(unix)]
#[derive(Debug)]
struct X11SpilledRecord {
    bytes: Vec<u8>,
    fds: Vec<OwnedFd>,
    sent: usize,
}

/// Why a connection's output was ended here.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum X11OutputEnding {
    /// The byte bound was passed.
    Saturated,
    /// The silence allowance expired with output owed.
    Silent,
    /// The peer was found gone while draining.
    PeerGone,
    /// Ended on purpose by a writer of this connection.
    Terminated,
}

#[cfg(unix)]
#[derive(Debug)]
pub struct X11ClientOutput {
    stream: UnixStream,
    spill: VecDeque<X11SpilledRecord>,
    /// Bytes owed: the sum over the spill of what is still to send.
    outstanding: usize,
    limit: usize,
    ended: Option<X11OutputEnding>,
    last_progress: Instant,
    last_activity: Instant,
    /// Signalled when the spill gains its first record, so the drain can
    /// sleep on it rather than poll an empty spill.
    wake: Arc<Condvar>,
    client: u64,
}

#[cfg(unix)]
impl X11ClientOutput {
    /// This connection's output, before it is shared.
    pub fn new(stream: UnixStream, client: u64) -> Self {
        Self {
            stream,
            spill: VecDeque::new(),
            outstanding: 0,
            limit: X_AUTHORITY_CLIENT_OUTPUT_SPILL_LIMIT,
            ended: None,
            last_progress: Instant::now(),
            last_activity: Instant::now(),
            wake: Arc::new(Condvar::new()),
            client,
        }
    }

    /// This connection's output, shared by every writer of it.
    pub fn shared(stream: UnixStream, client: u64) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::new(stream, client)))
    }

    /// The socket itself, for a writer with its own send discipline.
    pub(crate) fn stream(&self) -> &UnixStream {
        &self.stream
    }

    /// Another handle on the same connection, for whoever must be able to
    /// end it without this mutex.
    pub(crate) fn try_clone_stream(&self) -> std::io::Result<UnixStream> {
        self.stream.try_clone()
    }

    /// End this connection on purpose.
    pub(crate) fn shutdown(&mut self, how: Shutdown) -> std::io::Result<()> {
        let result = self.stream.shutdown(how);
        if how != Shutdown::Read {
            self.close(X11OutputEnding::Terminated);
        }
        result
    }

    /// The client did something -- a request was read from it -- which is the
    /// activity the silence allowance measures the absence of.
    pub(crate) fn note_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    fn silence(&self) -> Duration {
        self.last_progress
            .max(self.last_activity)
            .elapsed()
    }

    fn ended_error(ending: X11OutputEnding) -> std::io::Error {
        std::io::Error::new(
            ErrorKind::BrokenPipe,
            match ending {
                X11OutputEnding::Saturated => "X11 client output ended: the client stopped reading past the byte bound",
                X11OutputEnding::Silent => "X11 client output ended: the client went silent with output owed",
                X11OutputEnding::PeerGone => "X11 client output ended: the peer is gone",
                X11OutputEnding::Terminated => "X11 client output ended: the connection was terminated",
            },
        )
    }

    /// Send one record, or as much of it as the kernel takes now.
    ///
    /// The descriptors travel with the first byte that goes and never again,
    /// which is why they are given only while nothing of the record has been
    /// sent. `WouldBlock` means the kernel took nothing.
    fn try_send(&self, bytes: &[u8], fds: &[OwnedFd], from: usize) -> std::io::Result<usize> {
        let flags = rustix::net::SendFlags::DONTWAIT | rustix::net::SendFlags::NOSIGNAL;
        let remaining = &bytes[from..];
        loop {
            let attempt = if from == 0 && !fds.is_empty() {
                let borrowed = fds.iter().map(AsFd::as_fd).collect::<Vec<_>>();
                let mut ancillary_space = [MaybeUninit::uninit();
                    rustix::cmsg_space!(ScmRights(sophia_protocol::DMA_BUF_MAX_PLANES))];
                let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut ancillary_space);
                if !ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&borrowed)) {
                    return Err(std::io::Error::other(
                        "failed to encode X11 output file descriptors",
                    ));
                }
                rustix::net::sendmsg(&self.stream, &[IoSlice::new(remaining)], &mut ancillary, flags)
            } else {
                rustix::net::send(&self.stream, remaining, flags)
            };
            match attempt {
                Ok(0) if !remaining.is_empty() => {
                    return Err(std::io::Error::new(
                        ErrorKind::WriteZero,
                        "failed to write X11 output record",
                    ));
                }
                Ok(sent) => return Ok(sent),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(std::io::Error::from(error)),
            }
        }
    }

    /// Move what the kernel will take now from the head of the spill, in
    /// order. Returns whether anything went.
    pub(crate) fn drain_once(&mut self) -> std::io::Result<bool> {
        if let Some(ending) = self.ended {
            return Err(Self::ended_error(ending));
        }
        let mut progressed = false;
        while let Some(head) = self.spill.front_mut() {
            let sent = match Self::send_from(&self.stream, head) {
                Ok(sent) => sent,
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            };
            head.sent += sent;
            self.outstanding -= sent;
            if sent > 0 {
                // Gone with the first bytes; never again.
                head.fds.clear();
                progressed = true;
            }
            if head.sent >= head.bytes.len() {
                self.spill.pop_front();
            } else {
                break;
            }
        }
        if progressed {
            self.last_progress = Instant::now();
        }
        Ok(progressed)
    }

    fn send_from(stream: &UnixStream, record: &X11SpilledRecord) -> std::io::Result<usize> {
        let flags = rustix::net::SendFlags::DONTWAIT | rustix::net::SendFlags::NOSIGNAL;
        let remaining = &record.bytes[record.sent..];
        loop {
            let attempt = if record.sent == 0 && !record.fds.is_empty() {
                let borrowed = record.fds.iter().map(AsFd::as_fd).collect::<Vec<_>>();
                let mut ancillary_space = [MaybeUninit::uninit();
                    rustix::cmsg_space!(ScmRights(sophia_protocol::DMA_BUF_MAX_PLANES))];
                let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut ancillary_space);
                if !ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&borrowed)) {
                    return Err(std::io::Error::other(
                        "failed to encode X11 output file descriptors",
                    ));
                }
                rustix::net::sendmsg(stream, &[IoSlice::new(remaining)], &mut ancillary, flags)
            } else {
                rustix::net::send(stream, remaining, flags)
            };
            match attempt {
                Ok(0) if !remaining.is_empty() => {
                    return Err(std::io::Error::new(
                        ErrorKind::WriteZero,
                        "failed to write X11 output record",
                    ));
                }
                Ok(sent) => return Ok(sent),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(std::io::Error::from(error)),
            }
        }
    }

    /// Admit one record to the wire: after everything already owed, into the
    /// kernel as far as it goes now, and the rest onto the spill.
    pub(crate) fn admit(&mut self, bytes: Vec<u8>, fds: Vec<OwnedFd>) -> std::io::Result<()> {
        if let Some(ending) = self.ended {
            return Err(Self::ended_error(ending));
        }
        self.drain_once()?;
        let mut sent = 0;
        if self.spill.is_empty() {
            match self.try_send(&bytes, &fds, 0) {
                Ok(count) if count >= bytes.len() => return Ok(()),
                Ok(count) => sent = count,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
        }
        let owed = bytes.len() - sent;
        if self.outstanding + owed > self.limit {
            let outstanding = self.outstanding + owed;
            tracing::warn!(
                "sophia_x11_client_output schema=1 status=ended cause=saturated client={} outstanding_bytes={outstanding} limit_bytes={}",
                self.client,
                self.limit,
            );
            self.close(X11OutputEnding::Saturated);
            return Err(Self::ended_error(X11OutputEnding::Saturated));
        }
        let first = self.spill.is_empty();
        self.spill.push_back(X11SpilledRecord {
            bytes,
            fds: if sent > 0 { Vec::new() } else { fds },
            sent,
        });
        self.outstanding += owed;
        if first {
            self.wake.notify_all();
        }
        Ok(())
    }

    /// The silence allowance expired with output owed: the drain's verdict.
    fn end_silent(&mut self, silence: Duration) {
        tracing::warn!(
            "sophia_x11_client_output schema=1 status=ended cause=silent client={} outstanding_bytes={} silence_msec={} allowance_msec={}",
            self.client,
            self.outstanding,
            silence.as_millis(),
            X_AUTHORITY_CLIENT_OUTPUT_SILENCE_LIMIT.as_millis(),
        );
        let _ = self.stream.shutdown(Shutdown::Both);
        self.close(X11OutputEnding::Silent);
    }

    /// Ended, however it came about: nothing more is owed, and nothing more
    /// is admitted.
    fn close(&mut self, ending: X11OutputEnding) {
        if self.ended.is_some() {
            return;
        }
        if matches!(ending, X11OutputEnding::Saturated | X11OutputEnding::PeerGone) {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
        self.spill.clear();
        self.outstanding = 0;
        self.ended = Some(ending);
        self.wake.notify_all();
    }
}

/// `write_all` and `flush` for every writer that used to hold the socket:
/// one write call is one record, accepted whole into the kernel or the spill,
/// and a flush is a drain attempt that never waits.
#[cfg(unix)]
impl std::io::Write for X11ClientOutput {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.admit(buf.to_vec(), Vec::new())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.drain_once().map(|_| ())
    }
}

/// The thread that moves a connection's spill into the kernel as its client
/// reads, and ends a client that goes silent with output owed.
#[cfg(unix)]
pub(crate) struct X11OutputDrain {
    stop: Arc<AtomicBool>,
    /// The output's own wake, so a stop reaches a drain asleep on an empty
    /// spill at once rather than at its next slice: teardown waits for this
    /// thread, and a connection's teardown is measured in microseconds.
    wake: Arc<Condvar>,
    /// Taken by whoever joins it; a drain dropped with its thread still here
    /// stops and joins it itself.
    thread: Option<std::thread::JoinHandle<Result<(), X11SetupSocketError>>>,
}

#[cfg(unix)]
impl X11OutputDrain {
    /// Tell the drain to stop, and wake it so it hears.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.wake.notify_all();
    }
}

#[cfg(unix)]
impl Drop for X11OutputDrain {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(unix)]
fn spawn_x11_output_drain(
    output: Arc<Mutex<X11ClientOutput>>,
    client: u64,
) -> Result<X11OutputDrain, X11SetupSocketError> {
    let (handle, wake) = {
        let guard = output
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 output socket lock poisoned"))?;
        let handle = guard.try_clone_stream().map_err(|error| {
            X11SetupSocketError::new(format!(
                "failed to clone X11 output socket for the drain: {error}"
            ))
        })?;
        (handle, guard.wake.clone())
    };
    let stop = Arc::new(AtomicBool::new(false));
    let drain_stop = stop.clone();
    let thread = std::thread::Builder::new()
        .name(format!("sophia-x11-drain-{client}"))
        .spawn(move || -> Result<(), X11SetupSocketError> {
            loop {
                let mut guard = output
                    .lock()
                    .map_err(|_| X11SetupSocketError::new("X11 output socket lock poisoned"))?;
                if drain_stop.load(Ordering::Acquire) {
                    // Best effort on the way out; the connection is ending.
                    let _ = guard.drain_once();
                    return Ok(());
                }
                if guard.ended.is_some() {
                    return Ok(());
                }
                if guard.spill.is_empty() {
                    let wake = guard.wake.clone();
                    let (guard, _) = wake
                        .wait_timeout(guard, X11_OUTPUT_DRAIN_SLICE)
                        .map_err(|_| X11SetupSocketError::new("X11 output socket lock poisoned"))?;
                    drop(guard);
                    continue;
                }
                let silence = guard.silence();
                if silence >= X_AUTHORITY_CLIENT_OUTPUT_SILENCE_LIMIT {
                    guard.end_silent(silence);
                    return Ok(());
                }
                // Wait for room without the mutex: a writer admitting a record
                // must not queue behind a drain that is only waiting.
                drop(guard);
                let mut watched = [rustix::event::PollFd::new(
                    &handle,
                    rustix::event::PollFlags::OUT,
                )];
                let slice = rustix::fs::Timespec {
                    tv_sec: 0,
                    tv_nsec: i64::from(X11_OUTPUT_DRAIN_SLICE.subsec_nanos()),
                };
                match rustix::event::poll(&mut watched, Some(&slice)) {
                    Ok(_) | Err(rustix::io::Errno::INTR) => {}
                    Err(error) => {
                        return Err(X11SetupSocketError::new(format!(
                            "X11 output drain could not wait for the socket: {error}"
                        )));
                    }
                }
                let mut guard = output
                    .lock()
                    .map_err(|_| X11SetupSocketError::new("X11 output socket lock poisoned"))?;
                match guard.drain_once() {
                    Ok(_) => {}
                    Err(error) if is_x11_client_disconnect(&error) => {
                        guard.close(X11OutputEnding::PeerGone);
                        return Ok(());
                    }
                    Err(error) => {
                        return Err(X11SetupSocketError::new(format!(
                            "failed to drain X11 client output: {error}"
                        )));
                    }
                }
            }
        })
        .map_err(|error| {
            X11SetupSocketError::new(format!("failed to start the X11 output drain: {error}"))
        })?;
    Ok(X11OutputDrain {
        stop,
        wake,
        thread: Some(thread),
    })
}
