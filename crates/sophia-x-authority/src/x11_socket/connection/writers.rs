#[cfg(unix)]
struct X11InputEventWriter {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<Result<(), X11SetupSocketError>>,
}
struct X11ControlWriter {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<Result<(), X11SetupSocketError>>,
}

#[cfg(unix)]
struct X11ProtocolEventWriter {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<Result<(), X11SetupSocketError>>,
}

/// Every writer one client connection started, owned together.
///
/// Stopping is separated from joining. A writer waiting on something another
/// writer still holds cannot be waited on first, and joining one at a time
/// meant the first failure returned before the rest were even told to stop:
/// they kept running against a closing stream, and the control writer kept
/// the registrations its client's work is answered through.
///
/// Owning them together also means a setup failure after any spawn owns their
/// shutdown, because dropping this is shutting them down.
#[cfg(unix)]
struct X11ClientWriters {
    input: Option<X11InputEventWriter>,
    control: Option<X11ControlWriter>,
    protocol: Option<X11ProtocolEventWriter>,
    /// The drain that moves this connection's owed output into the kernel as
    /// the client reads. Joined last, so no writer admits into a spill that
    /// nothing will drain.
    drain: Option<X11OutputDrain>,
    /// An independent handle on the same socket the writers share.
    ///
    /// A writer blocked in a write observes no flag, and whoever joins it then
    /// waits on a peer that may never read again. Shutting the socket down is
    /// what ends that wait, and it is done through a handle of this shutdown's
    /// own: the blocked writer is holding the output mutex, so anything that
    /// had to take that mutex first could not reach it.
    ///
    /// Required, not optional. Taking this handle needs a descriptor, and the
    /// moment one cannot be had is exactly the moment a connection is most
    /// likely to stall -- so a cohort that started workers without it would
    /// lose the guarantee silently, precisely when it is needed. It is
    /// acquired before any worker exists, and failing to acquire it refuses
    /// the connection instead.
    transport: UnixStream,
}

#[cfg(unix)]
impl X11ClientWriters {
    /// Take the shutdown's own handle on the output socket.
    ///
    /// Separate from owning the writers so that it can happen before this
    /// client is registered as anything, while the cohort itself is declared
    /// after those registrations and so gives its workers up before them.
    fn take_transport(stream: &Arc<Mutex<X11ClientOutput>>) -> Result<UnixStream, X11SetupSocketError> {
        let transport = stream
            .lock()
            .map_err(|_| X11SetupSocketError::new("X11 output socket lock poisoned"))?
            .try_clone_stream()
            .map_err(|error| {
                X11SetupSocketError::new(format!(
                    "failed to clone X11 output socket for writer shutdown: {error}"
                ))
            })?;
        Ok(transport)
    }

    /// Own the writers a connection starts, with the handle that can end a
    /// write no flag reaches.
    fn owning(transport: UnixStream, drain: X11OutputDrain) -> Self {
        Self {
            input: None,
            control: None,
            protocol: None,
            drain: Some(drain),
            transport,
        }
    }
}

#[cfg(unix)]
impl X11ClientWriters {
    /// Stop every writer, join every writer, and report the first failure.
    ///
    /// Every one is stopped before any is joined, and every one is joined even
    /// when an earlier join failed. A failure is reported, not acted on by
    /// abandoning what is left: a writer nobody joins is a thread still
    /// holding a stream and a queue.
    ///
    /// Reports how many it actually waited for as well as what went wrong,
    /// because "one of them failed" and "the rest were left running" are
    /// different facts and only the count tells them apart.
    ///
    /// Idempotent, so the ordinary path can call it and the drop that follows
    /// finds nothing left to do.
    fn shut_down(&mut self) -> X11WriterShutdown {
        self.stop_all();
        // Almost always already true by now: a writer between events notices
        // its flag immediately, and this returns without waiting. It is a
        // deadline rather than a delay.
        if !self.settled_within(X11_WRITER_STOP_GRACE) {
            // Something is inside a write that no flag reaches. The connection
            // is ending either way, so the socket goes and the write fails.
            let _ = self.transport.shutdown(Shutdown::Both);
        }
        self.join_all()
    }

    fn stop_all(&self) {
        for stop in [
            self.input.as_ref().map(|writer| &writer.stop),
            self.control.as_ref().map(|writer| &writer.stop),
            self.protocol.as_ref().map(|writer| &writer.stop),
        ]
        .into_iter()
        .flatten()
        {
            stop.store(true, Ordering::Release);
        }
        if let Some(drain) = self.drain.as_ref() {
            drain.stop();
        }
    }

    /// Whether every writer has finished within the deadline.
    fn settled_within(&self, deadline: Duration) -> bool {
        let limit = std::time::Instant::now() + deadline;
        loop {
            let running = [
                self.input.as_ref().map(|writer| &writer.thread),
                self.control.as_ref().map(|writer| &writer.thread),
                self.protocol.as_ref().map(|writer| &writer.thread),
                self.drain.as_ref().and_then(|writer| writer.thread.as_ref()),
            ]
            .into_iter()
            .flatten()
            .any(|thread| !thread.is_finished());
            if !running {
                return true;
            }
            if std::time::Instant::now() >= limit {
                return false;
            }
            std::thread::yield_now();
        }
    }

    fn join_all(&mut self) -> X11WriterShutdown {
        let joins = [
            self.input.take().map(|writer| (writer.thread, "input event")),
            self.control.take().map(|writer| (writer.thread, "control")),
            self.protocol
                .take()
                .map(|writer| (writer.thread, "protocol event")),
            self.drain
                .take()
                .and_then(|mut writer| writer.thread.take().map(|thread| (thread, "output drain"))),
        ];
        let mut shutdown = X11WriterShutdown {
            joined: 0,
            outcome: Ok(()),
        };
        for (thread, what) in joins.into_iter().flatten() {
            #[cfg(all(test, unix))]
            let identity = thread.thread().id();
            let joined = match thread.join() {
                Ok(result) => result,
                Err(_) => Err(X11SetupSocketError::new(format!(
                    "X11 {what} writer thread panicked"
                ))),
            };
            #[cfg(all(test, unix))]
            routing_tests::m3_acceptance::actor_joined(identity);
            shutdown.joined = shutdown.joined.saturating_add(1);
            if shutdown.outcome.is_ok() {
                shutdown.outcome = joined;
            }
        }
        shutdown
    }
}

/// How long a writer is given to notice its stop flag before the socket it may
/// be blocked on is taken away.
///
/// Long enough that an ordinary teardown never reaches it, short enough that a
/// blocked one does not hold a connection's teardown open.
///
/// A grace before the socket goes, and nothing more. It does not bound the
/// join that follows, and it does not bound a writer waiting on the runtime
/// lock or any other condition a closed socket does not touch. Those are
/// separate waits and this says nothing about them.
#[cfg(unix)]
const X11_WRITER_STOP_GRACE: Duration = Duration::from_millis(250);

/// What shutting a client's writers down achieved.
#[cfg(unix)]
struct X11WriterShutdown {
    /// How many writers were waited for. Every one that was running, or the
    /// ones after a failure were left detached.
    joined: usize,
    /// The first failure any of them reported.
    outcome: Result<(), X11SetupSocketError>,
}

#[cfg(unix)]
impl Drop for X11ClientWriters {
    fn drop(&mut self) {
        // The fallback for every path that leaves without shutting them down
        // itself, including a setup failure between two spawns.
        let _ = self.shut_down();
    }
}

#[cfg(unix)]
struct X11ControlOutputPriority {
    pending: Arc<AtomicUsize>,
}

#[cfg(unix)]
impl X11ControlOutputPriority {
    fn new(pending: Arc<AtomicUsize>) -> Self {
        pending.fetch_add(1, Ordering::AcqRel);
        Self { pending }
    }
}

#[cfg(unix)]
impl Drop for X11ControlOutputPriority {
    fn drop(&mut self) {
        let previous = self.pending.fetch_sub(1, Ordering::AcqRel);
        debug_assert_ne!(previous, 0, "control-output priority underflow");
    }
}

/// Whether this connection's wire may still be written at all.
///
/// ONE PER CONNECTION, SHARED BY EVERY POST-EXPOSURE WRITER. A wire that holds
/// the beginning of an event nobody can finish must not receive anything more:
/// what follows would be read as the rest of that event, and X11 has no way to
/// describe or recover from it. Whoever discovers that bars the wire here, and
/// every writer of this socket sees it -- because a latch private to one
/// writer fences only that writer, which is no fence at all.
///
/// Read under the output mutex, so the answer cannot change between being
/// asked and the write it authorises.
#[cfg(unix)]
struct X11WirePermission {
    barred: AtomicBool,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl X11WirePermission {
    fn open() -> Self {
        Self {
            barred: AtomicBool::new(false),
        }
    }

    /// Bar this wire for good. Nothing unbars it: the bytes on it stay there.
    fn bar(&self) {
        self.barred.store(true, Ordering::Release);
    }

    fn barred(&self) -> bool {
        self.barred.load(Ordering::Acquire)
    }
}

/// Take this connection's output for a write.
///
/// THE ONE BOUNDARY. Every post-exposure writer of this socket comes through
/// here -- the input, protocol and reply writers by way of the non-control
/// helper, and control writes directly -- so the permission is asked in the
/// same place the serialization is taken, while holding it. Asking anywhere
/// else would leave a window between the answer and the write.
#[cfg(unix)]
fn enter_x11_wire<'a>(
    stream: &'a Arc<Mutex<X11ClientOutput>>,
    wire: &X11WirePermission,
) -> Result<std::sync::MutexGuard<'a, X11ClientOutput>, X11SetupSocketError> {
    let guard = stream
        .lock()
        .map_err(|_| X11SetupSocketError::new("X11 output socket lock poisoned"))?;
    if wire.barred() {
        return Err(X11SetupSocketError::client_failure(
            "X11 output wire holds an unfinished event and was barred",
        ));
    }
    Ok(guard)
}

#[cfg(unix)]
/// Wait for control output to finish, or for this writer to be told to stop.
///
/// Cancellable, because the alternative is a writer that has been told to stop
/// and cannot act on it. Whoever joins it then waits for a condition only
/// another thread can clear, and a stop flag nothing observes makes a join
/// unbounded however carefully the flags were set first.
///
/// Returns false when the wait was cancelled.
#[cfg(unix)]
fn wait_for_x11_control_output(control_pending: &AtomicUsize, stop: Option<&AtomicBool>) -> bool {
    while control_pending.load(Ordering::Acquire) != 0 {
        if stop.is_some_and(|stop| stop.load(Ordering::Acquire)) {
            return false;
        }
        std::thread::yield_now();
    }
    true
}

#[cfg(unix)]
/// Take the output socket for a non-control write, or give up because this
/// writer was told to stop.
///
/// `Ok(None)` is the second of those. It is not a failure: nothing was written
/// and nothing is owed, and the caller's business is to leave.
#[cfg(unix)]
fn lock_x11_non_control_output<'a>(
    stream: &'a Arc<Mutex<X11ClientOutput>>,
    wire: &X11WirePermission,
    control_pending: &AtomicUsize,
    stop: Option<&AtomicBool>,
) -> Result<Option<std::sync::MutexGuard<'a, X11ClientOutput>>, X11SetupSocketError> {
    loop {
        if !wait_for_x11_control_output(control_pending, stop) {
            return Ok(None);
        }
        let stream = enter_x11_wire(stream, wire)?;
        // Recheck after acquisition: a control may have registered while this
        // writer was waiting on a request, input, or protocol-event write.
        if control_pending.load(Ordering::Acquire) == 0 {
            return Ok(Some(stream));
        }
        drop(stream);
        // Back to the wait, which is where stop is observed. A second check
        // here would save one spin and would be one more thing a reader has to
        // reason about to see that this terminates.
        std::thread::yield_now();
    }
}

#[cfg(unix)]
fn spawn_x11_protocol_event_writer(
    stream: Arc<Mutex<X11ClientOutput>>,
    output_control_pending: Arc<AtomicUsize>,
    output_wire: Arc<X11WirePermission>,
    byte_order: XByteOrder,
    sequence: Arc<AtomicU16>,
    client: XServerFrontendClientId,
    receiver: impl Into<X11ProtocolReceiver>,
) -> Result<X11ProtocolEventWriter, X11SetupSocketError> {
    let receiver = receiver.into();
    let stop = Arc::new(AtomicBool::new(false));
    let writer_stop = stop.clone();
    let thread = std::thread::spawn(move || {
        while !writer_stop.load(Ordering::Acquire) {
            let envelope = match receiver.receive(Duration::from_millis(10)) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            };
            receiver.validate(&envelope)?;
            let mut event = envelope.event;
            let Some(mut stream) =
                lock_x11_non_control_output(
                    &stream,
                    &output_wire,
                    &output_control_pending,
                    Some(&writer_stop),
                )?
            else {
                // The writer stopped before emission. A governed envelope's
                // original journal retains its unresolved payload and receipt.
                return Ok(());
            };
            set_x11_protocol_event_sequence(&mut event, sequence.load(Ordering::Acquire));
            let record = encode_x_client_event(byte_order, event);
            receiver.retain_wire_record(&envelope, &record)?;
            crate::evidence::present_event(client, None, "write_started", event);
            if std::env::var_os("SOPHIA_X11_AUTHORITY_TRACE").is_some() {
                tracing::trace!(
                    "sophia_x11_socket_write schema=1 writer=protocol bytes={} payload_redacted=true",
                    record.len(),
                );
            }
            if let Err(error) = stream.write_all(&record) {
                if is_x11_client_disconnect(&error) {
                    return Ok(());
                }
                return Err(X11SetupSocketError::new(format!(
                    "failed to write X11 protocol event: {error}"
                )));
            }
            stream.flush().map_err(|error| {
                x11_peer_write_error("failed to flush X11 protocol event", error)
            })?;
            crate::evidence::present_event(client, None, "written", event);
            receiver.record_flushed(&envelope)?;
            // Giving up the dependency may enter the completion registry.
            // Release wire first: source retirement takes completion before
            // operation, while an originating control holds operation/wire.
            drop(stream);
            drop(envelope);
            trace_written_selection_event(client, event);
        }
        Ok(())
    });
    Ok(X11ProtocolEventWriter { stop, thread })
}

#[cfg(unix)]
fn trace_written_selection_event(client: XServerFrontendClientId, event: XClientEvent) {
    if std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_none() {
        return;
    }
    match event {
        XClientEvent::SelectionClear {
            sequence,
            time,
            owner,
            selection,
        } => tracing::info!(
            "sophia_x11_selection_delivery schema=1 stage=socket_flushed kind=clear client={} sequence={} time={} owner={} selection={} content=redacted",
            client.raw(),
            sequence,
            time,
            owner.local.raw(),
            selection,
        ),
        XClientEvent::SelectionRequest {
            sequence,
            time,
            owner,
            requestor,
            selection,
            target,
            property,
        } => tracing::info!(
            "sophia_x11_selection_delivery schema=1 stage=socket_flushed kind=request client={} sequence={} time={} owner={} requestor={} selection={} target={} property={} content=redacted",
            client.raw(),
            sequence,
            time,
            owner.local.raw(),
            requestor.local.raw(),
            selection,
            target,
            property,
        ),
        XClientEvent::SelectionNotify {
            sequence,
            synthetic,
            time,
            requestor,
            selection,
            target,
            property,
        } => tracing::info!(
            "sophia_x11_selection_delivery schema=1 stage=socket_flushed kind=notify client={} sequence={} synthetic={} time={} requestor={} selection={} target={} property={} property_present={} content=redacted",
            client.raw(),
            sequence,
            synthetic,
            time,
            requestor.local.raw(),
            selection,
            target,
            property,
            property != crate::X_ATOM_NONE,
        ),
        _ => {}
    }
}

#[cfg(unix)]
fn set_x11_protocol_event_sequence(event: &mut XClientEvent, value: u16) {
    match event {
        XClientEvent::SelectionClear { sequence, .. }
        | XClientEvent::SelectionRequest { sequence, .. }
        | XClientEvent::SelectionNotify { sequence, .. }
        | XClientEvent::PropertyNotify { sequence, .. }
        | XClientEvent::CreateNotify { sequence, .. }
        | XClientEvent::MapNotify { sequence, .. }
        | XClientEvent::MapRequest { sequence, .. }
        | XClientEvent::Focus { sequence, .. }
        | XClientEvent::DestroyNotify { sequence, .. }
        | XClientEvent::UnmapNotify { sequence, .. }
        | XClientEvent::ConfigureNotify { sequence, .. }
        | XClientEvent::MappingNotify { sequence, .. }
        | XClientEvent::ReparentNotify { sequence, .. }
        | XClientEvent::ClientMessage { sequence, .. }
        | XClientEvent::CirculateNotify { sequence, .. }
        | XClientEvent::CirculateRequest { sequence, .. }
        | XClientEvent::ConfigureRequest { sequence, .. }
        | XClientEvent::ResizeRequest { sequence, .. }
        | XClientEvent::VisibilityNotify { sequence, .. }
        | XClientEvent::GravityNotify { sequence, .. }
        | XClientEvent::ColormapNotify { sequence, .. }
        | XClientEvent::Expose { sequence, .. }
        | XClientEvent::RandrScreenChange { sequence, .. }
        | XClientEvent::RandrCrtcChange { sequence, .. }
        | XClientEvent::RandrOutputChange { sequence, .. }
        | XClientEvent::RandrResourceChange { sequence, .. }
        | XClientEvent::PresentConfigureNotify { sequence, .. }
        | XClientEvent::PresentCompleteNotify { sequence, .. }
        | XClientEvent::PresentIdleNotify { sequence, .. }
        | XClientEvent::XfixesSelectionNotify { sequence, .. } => *sequence = value,
        // KeymapNotify carries no sequence number: its bytes 1 to 31 are the
        // key bitmap.
        XClientEvent::KeymapNotify { .. } => {}
        _ => unreachable!("protocol routing received a non-routable event"),
    }
}
