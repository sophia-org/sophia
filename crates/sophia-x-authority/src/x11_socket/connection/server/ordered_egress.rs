// The bounded, ordered egress of authority transactions to the Engine: the
// envelope, the report and the state machine that keeps them in order.
// Included into `connection/server.rs`; split out to keep that file within
// the layout ledger's bound (t026).

/// Runs the routed frontend with an additional value-free backpressure observer.
///
/// Production uses this seam for stable tracing. Tests may wait for an exact
/// transition before exercising shutdown without inferring worker state from
/// socket timing.
#[cfg(unix)]
struct XAuthorityBoundedEgressEnvelope {
    transaction: TransactionId,
    batch: Option<XAuthorityObservedTransactionBatch>,
    client: Option<XServerFrontendClientId>,
    observed_batch: bool,
    waiting_since: Option<Instant>,
    /// Whether this envelope's wait was cancelled. A cancelled envelope is
    /// still unsent work while it holds its batch; it is not resubmitted and
    /// not reported twice.
    cancelled: bool,
}

#[cfg(unix)]
impl XAuthorityBoundedEgressEnvelope {
    fn new(transaction: TransactionId, batch: Option<XAuthorityObservedTransactionBatch>) -> Self {
        let client = batch.as_ref().and_then(|batch| batch.client);
        let observed_batch = batch.is_some();
        Self {
            transaction,
            batch,
            client,
            observed_batch,
            waiting_since: None,
            cancelled: false,
        }
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct XAuthorityOrderedEgressReport {
    tickets_advanced: u64,
    batches_delivered: u64,
    peak_waiting_producers: usize,
    wait_episodes: u64,
    resumed: u64,
    cancelled: u64,
}

#[cfg(unix)]
struct XAuthorityOrderedEgressState {
    next_ticket: u64,
    waiting_producers: usize,
    report: XAuthorityOrderedEgressReport,
}

#[cfg(unix)]
impl Default for XAuthorityOrderedEgressState {
    fn default() -> Self {
        Self {
            next_ticket: 1,
            waiting_producers: 0,
            report: XAuthorityOrderedEgressReport {
                tickets_advanced: 0,
                batches_delivered: 0,
                peak_waiting_producers: 0,
                wait_episodes: 0,
                resumed: 0,
                cancelled: 0,
            },
        }
    }
}

#[cfg(unix)]
struct XAuthorityOrderedEgress {
    sender: SyncSender<XAuthorityObservedTransactionBatch>,
    cancellation: Arc<AtomicBool>,
    transport_disconnected: AtomicBool,
    state: Mutex<XAuthorityOrderedEgressState>,
    turn: Condvar,
    telemetry: Arc<XAuthorityBackpressureObserver>,
}

#[cfg(unix)]
impl XAuthorityOrderedEgress {
    fn new(
        sender: SyncSender<XAuthorityObservedTransactionBatch>,
        cancellation: Arc<AtomicBool>,
        telemetry: Arc<XAuthorityBackpressureObserver>,
    ) -> Self {
        Self {
            sender,
            cancellation,
            transport_disconnected: AtomicBool::new(false),
            state: Mutex::new(XAuthorityOrderedEgressState::default()),
            turn: Condvar::new(),
            telemetry,
        }
    }

    /// Cancel every submission, present and future.
    ///
    /// UNDER THE ORDER LOCK, OR THE WAITER SLEEPS PAST IT. A submitter reads
    /// the cancellation flag under that lock and then waits on `turn`; a
    /// store and a notification made outside the lock can land between its
    /// read and its wait, and the only notification it will ever get has
    /// gone by. Holding the lock while storing and notifying leaves the
    /// waiter exactly two places to be: before its read, where it sees the
    /// flag, or inside the wait, where it is woken. A poisoned lock still
    /// notifies -- cancellation is what a poisoned service needs most -- and
    /// nothing here is called with the lock already held.
    fn cancel(&self) {
        let order = match self.state.lock() {
            Ok(order) => order,
            Err(poisoned) => poisoned.into_inner(),
        };
        self.cancellation.store(true, Ordering::Release);
        self.turn.notify_all();
        drop(order);
    }

    fn cancelled(&self) -> bool {
        self.cancellation.load(Ordering::Acquire)
    }

    fn transport_disconnected(&self) -> bool {
        self.transport_disconnected.load(Ordering::Acquire)
    }

    fn state(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, XAuthorityOrderedEgressState>, X11SetupSocketError> {
        self.state
            .lock()
            .map_err(|_| X11SetupSocketError::new("authority egress order lock poisoned"))
    }

    fn begin_wait(
        &self,
        envelope: &mut XAuthorityBoundedEgressEnvelope,
    ) -> Result<(), X11SetupSocketError> {
        if envelope.waiting_since.is_some() {
            return Ok(());
        }
        envelope.waiting_since = Some(Instant::now());
        let mut state = self.state()?;
        state.waiting_producers = state.waiting_producers.saturating_add(1);
        state.report.wait_episodes = state.report.wait_episodes.saturating_add(1);
        state.report.peak_waiting_producers = state
            .report
            .peak_waiting_producers
            .max(state.waiting_producers);
        drop(state);
        if envelope.observed_batch {
            (self.telemetry)(XAuthorityBackpressureTelemetry {
                kind: XAuthorityBackpressureTelemetryKind::Wait,
                client: envelope.client,
                transaction: envelope.transaction,
                waited: Duration::ZERO,
                failure: None,
            });
        }
        Ok(())
    }

    fn finish_wait(
        &self,
        envelope: &mut XAuthorityBoundedEgressEnvelope,
        kind: XAuthorityBackpressureTelemetryKind,
        failure: Option<XAuthorityBackpressureFailure>,
    ) -> Result<(), X11SetupSocketError> {
        let waiting_since = envelope.waiting_since.take();
        let waited = waiting_since.map_or(Duration::ZERO, |started| started.elapsed());
        if waiting_since.is_some() || matches!(kind, XAuthorityBackpressureTelemetryKind::Shutdown)
        {
            let mut state = self.state()?;
            if waiting_since.is_some() {
                state.waiting_producers = state.waiting_producers.saturating_sub(1);
            }
            match kind {
                XAuthorityBackpressureTelemetryKind::Resume if waiting_since.is_some() => {
                    state.report.resumed = state.report.resumed.saturating_add(1);
                }
                XAuthorityBackpressureTelemetryKind::Shutdown => {
                    state.report.cancelled = state.report.cancelled.saturating_add(1);
                }
                XAuthorityBackpressureTelemetryKind::Resume
                | XAuthorityBackpressureTelemetryKind::Wait
                | XAuthorityBackpressureTelemetryKind::TransportFailure => {}
            }
        }
        if envelope.observed_batch && (waiting_since.is_some() || failure.is_some()) {
            (self.telemetry)(XAuthorityBackpressureTelemetry {
                kind,
                client: envelope.client,
                transaction: envelope.transaction,
                waited,
                failure,
            });
        }
        Ok(())
    }

    /// Cancel an envelope's wait, in place and once.
    ///
    /// CANCELLING A WAIT IS NOT DELIVERING THE BATCH. The envelope keeps its
    /// batch and stays in its owner's slot; what this reports is that the
    /// wait ended in shutdown. A second call reports nothing.
    fn cancel_envelope(
        &self,
        envelope: &mut XAuthorityBoundedEgressEnvelope,
    ) -> Result<(), X11SetupSocketError> {
        if envelope.cancelled {
            return Ok(());
        }
        envelope.cancelled = true;
        self.finish_wait(
            envelope,
            XAuthorityBackpressureTelemetryKind::Shutdown,
            Some(XAuthorityBackpressureFailure::Cancelled),
        )
    }

    fn advance(
        &self,
        envelope: &mut XAuthorityBoundedEgressEnvelope,
        delivered_batch: bool,
    ) -> Result<(), X11SetupSocketError> {
        let ticket = envelope.transaction.raw();
        let mut state = self.state()?;
        if ticket != state.next_ticket {
            return Err(X11SetupSocketError::new(
                "authority egress advanced a stale or out-of-order ticket",
            ));
        }
        state.next_ticket = state.next_ticket.checked_add(1).ok_or_else(|| {
            X11SetupSocketError::new("authority egress transaction ticket exhausted")
        })?;
        state.report.tickets_advanced = state.report.tickets_advanced.saturating_add(1);
        if delivered_batch {
            state.report.batches_delivered = state.report.batches_delivered.saturating_add(1);
        }
        drop(state);
        self.finish_wait(envelope, XAuthorityBackpressureTelemetryKind::Resume, None)?;
        self.turn.notify_all();
        Ok(())
    }

    fn submit_blocking(
        &self,
        mut envelope: XAuthorityBoundedEgressEnvelope,
    ) -> Result<(), X11SetupSocketError> {
        loop {
            if self.cancelled() {
                self.cancel_envelope(&mut envelope)?;
                return Err(X11SetupSocketError::service_shutdown(
                    "authority egress submission cancelled",
                ));
            }
            let state = self.state()?;
            let ticket = envelope.transaction.raw();
            if ticket < state.next_ticket {
                return Err(X11SetupSocketError::new(
                    "authority egress received a duplicate or stale ticket",
                ));
            }
            if ticket > state.next_ticket {
                drop(state);
                self.begin_wait(&mut envelope)?;
                let state = self.state()?;
                if self.cancelled() || ticket <= state.next_ticket {
                    drop(state);
                    continue;
                }
                let state = self.turn.wait(state).map_err(|_| {
                    X11SetupSocketError::new("authority egress order lock poisoned")
                })?;
                drop(state);
                continue;
            }
            drop(state);
            let Some(batch) = envelope.batch.take() else {
                return self.advance(&mut envelope, false);
            };
            match self.sender.try_send(batch) {
                Ok(()) => return self.advance(&mut envelope, true),
                Err(TrySendError::Full(batch)) => {
                    envelope.batch = Some(batch);
                    self.begin_wait(&mut envelope)?;
                }
                Err(TrySendError::Disconnected(batch)) => {
                    envelope.batch = Some(batch);
                    self.transport_disconnected.store(true, Ordering::Release);
                    self.cancel();
                    self.finish_wait(
                        &mut envelope,
                        XAuthorityBackpressureTelemetryKind::TransportFailure,
                        Some(XAuthorityBackpressureFailure::Disconnected),
                    )?;
                    return Err(X11SetupSocketError::new(
                        "X authority observed transaction channel is disconnected",
                    ));
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Submit the envelope in `slot` without blocking, leaving it there while
    /// it waits.
    ///
    /// IN PLACE, BECAUSE THE OBSERVER IS CALLED FROM HERE. `begin_wait`
    /// reports to a caller-supplied observer while the envelope is still
    /// unsent; an envelope moved into a local for that call was destroyed by
    /// a panic in the observer before any owner could retain it. The envelope
    /// leaves the slot only once it has been advanced -- sent, or carrying no
    /// batch -- or cancelled. A slot that still holds it after this returns,
    /// or after this unwinds, holds exactly the unresolved work.
    fn try_submit(
        &self,
        slot: &mut Option<XAuthorityBoundedEgressEnvelope>,
    ) -> Result<(), X11SetupSocketError> {
        let Some(envelope) = slot.as_mut() else {
            return Ok(());
        };
        if self.cancelled() {
            // Cancelled between the caller's check and this call: the wait
            // ends, the envelope STAYS in the slot with its batch. Clearing
            // the slot here was losing an unsent batch.
            self.cancel_envelope(envelope)?;
            return Ok(());
        }
        let state = self.state()?;
        let ticket = envelope.transaction.raw();
        if ticket < state.next_ticket {
            return Err(X11SetupSocketError::new(
                "authority egress received a duplicate or stale ticket",
            ));
        }
        if ticket > state.next_ticket {
            drop(state);
            self.begin_wait(envelope)?;
            return Ok(());
        }
        drop(state);
        let Some(batch) = envelope.batch.take() else {
            self.advance(envelope, false)?;
            *slot = None;
            return Ok(());
        };
        match self.sender.try_send(batch) {
            Ok(()) => {
                self.advance(envelope, true)?;
                *slot = None;
                Ok(())
            }
            Err(TrySendError::Full(batch)) => {
                envelope.batch = Some(batch);
                self.begin_wait(envelope)?;
                Ok(())
            }
            Err(TrySendError::Disconnected(batch)) => {
                envelope.batch = Some(batch);
                self.transport_disconnected.store(true, Ordering::Release);
                self.cancel();
                self.finish_wait(
                    envelope,
                    XAuthorityBackpressureTelemetryKind::TransportFailure,
                    Some(XAuthorityBackpressureFailure::Disconnected),
                )?;
                Err(X11SetupSocketError::new(
                    "X authority observed transaction channel is disconnected",
                ))
            }
        }
    }

    fn report(&self) -> Result<XAuthorityOrderedEgressReport, X11SetupSocketError> {
        Ok(self.state()?.report)
    }
}
