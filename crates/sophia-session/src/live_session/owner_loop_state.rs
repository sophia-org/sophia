use crate::input_delivery::{InputDeliveryError, InputDeliveryState, settle_input_delivery};

#[derive(Clone, Copy, Debug, Default)]
struct SessionLoopMetrics {
    batches: usize,
    transactions: usize,
    cpu_buffer_updates: usize,
    cpu_buffer_replacements: usize,
    cpu_buffer_patch_updates: usize,
    cpu_buffer_patch_rects: usize,
    cpu_buffer_payload_bytes: usize,
    dma_buf_registrations_observed: usize,
    fence_registrations_observed: usize,
    present_submissions_observed: usize,
    software_present_submissions_observed: usize,
    cpu_compositions: usize,
    coalesced_batches: usize,
    cadence_deferred_batches: usize,
    cadence_repaints: usize,
    /// Batches committed alongside another in one production cycle, so a burst
    /// of client draws costs one cycle rather than one frame each.
    merged_batches: usize,
    max_merge_run: usize,
    backend_ticks: usize,
    runtime_committed: u64,
    runtime_surfaces: u64,
    runtime_max_surfaces: u64,
    physical_events: usize,
    physical_keys_routed: usize,
    key_repeats_routed: usize,
    physical_pointer_events: usize,
    physical_pointer_routed: usize,
    physical_pointer_buttons_routed: usize,
    session_ticks: usize,
    max_compose: Duration,
    max_child_reap: Duration,
    max_input_phase: Duration,
    protocol_error_count: usize,
    expected_protocol_error_count: usize,
    cursor_moves_coalesced: u64,
    cursor_max_motion_to_submit: Duration,
}

impl SessionLoopMetrics {
    fn new(initialize_empty_runtime: bool) -> Self {
        Self {
            cpu_compositions: usize::from(initialize_empty_runtime),
            ..Self::default()
        }
    }

    /// Records how many surfaces are committed now, and the most there have
    /// ever been at once.
    ///
    /// The live figure is sampled, and a session that has drained its clients
    /// correctly reports none -- so it cannot distinguish a session that ran
    /// two terminals from one that never admitted a client. The peak is the
    /// figure that says surfaces existed, and how many stood together, which
    /// is the question a two-terminal scenario is actually asking.
    fn record_runtime_surfaces(&mut self, committed: usize) {
        self.runtime_surfaces = u64::try_from(committed).unwrap_or(u64::MAX);
        self.runtime_max_surfaces = self.runtime_max_surfaces.max(self.runtime_surfaces);
    }
}

#[derive(Default)]
struct InputObservationState {
    key_observed: bool,
    key_routed: bool,
    key_suppressed_no_focus: bool,
    pointer_motion_observed: bool,
    pointer_motion_routed: bool,
    pointer_button_observed: bool,
    pointer_buttons_suppressed_no_target: usize,
    pointer_button_routed: bool,
    pointer_focus_target: Option<SurfaceId>,
    pointer_focus_key_routed: bool,
    client_positioned_pointer_button_routed: bool,
    pointer_axis_observed: bool,
    pointer_axis_routed: bool,
    client_positioned_pointer_axis_routed: bool,
    return_suppressed: bool,
    /// Devices a key has been observed from, so each is announced once.
    devices_keyed: std::collections::BTreeSet<DeviceId>,
}

struct CursorUpdateState {
    dirty: bool,
    dirty_since: Option<Instant>,
}

impl CursorUpdateState {
    fn new(dirty: bool) -> Self {
        Self {
            dirty,
            dirty_since: dirty.then(Instant::now),
        }
    }
}

struct InputDeliveryPhase<'a> {
    sender: Option<&'a XAuthorityRoutedInputSender>,
    receiver: &'a Receiver<XAuthorityClientInputDelivery>,
    state: &'a mut InputDeliveryState,
    client_key_release_barrier: &'a mut BTreeSet<XAuthorityInputDeliveryId>,
    proof_started_at: &'a mut Option<Instant>,
    post_input_deadline: &'a mut Option<Instant>,
}

impl InputDeliveryPhase<'_> {
    fn drain(self) -> Result<(), Box<dyn std::error::Error>> { self.drain_at(Instant::now()) }

    fn drain_at(self, now: Instant) -> Result<(), Box<dyn std::error::Error>> {
        // Receipts win before expiry; recovery then emits failure receipts for
        // revoked work. The second drain removes only those exact obligations.
        for pass in 0..2 {
        while let Ok(delivery) = self.receiver.try_recv() {
            if let Some(sender) = self.sender {
                let ticket = sender.delivery_ticket(delivery.delivery);
                if !sender.observe_delivery(delivery) { continue; }
                if let Some(ticket) = ticket
                    && let Some(pending) = self.state.pending.get_mut(&delivery.delivery)
                { pending.ticket = ticket; }
            }
            let pending = self.state.pending.get(&delivery.delivery).copied();
            let outcome = settle_input_delivery(self.state, self.client_key_release_barrier, delivery)
                .map_err(InputDeliveryError::ClientFailure)?;
            let Some(outcome) = outcome else {
                continue;
            };
            if let Some(pending) = pending
                && (pending.release_barrier || outcome != XAuthorityInputDeliveryOutcome::Flushed) {
                crate::session_println!(
                    "sophia_live_session_input_delivery schema=3 status=settled delivery={} client={} surface={} generation={} age_msec={} release_barrier={} outcome={:?} content=redacted",
                    delivery.delivery.raw(), delivery.client.raw(), pending.ticket.surface.index(),
                    pending.ticket.surface.generation(), now.saturating_duration_since(pending.ticket.admitted_at).as_millis(),
                    pending.release_barrier, outcome,
                );
            }
            match outcome {
                XAuthorityInputDeliveryOutcome::Flushed => {}
                XAuthorityInputDeliveryOutcome::TargetGone => {
                    crate::session_println!(
                        "sophia_live_session_input_delivery schema=1 status=retired reason=target_gone client={}",
                        delivery.client.raw(),
                    );
                }
                // The session revoked this event itself when it closed the
                // input epoch for a topology, policy, or seat boundary. Ending
                // the session over it would make every pointer motion that
                // overlaps an output change fatal, which is what it did.
                XAuthorityInputDeliveryOutcome::EpochRevoked => {
                    crate::session_println!(
                        "sophia_live_session_input_delivery schema=1 status=retired reason=epoch_revoked client={}",
                        delivery.client.raw(),
                    );
                }
                XAuthorityInputDeliveryOutcome::RouteRejected
                | XAuthorityInputDeliveryOutcome::WriteFailed
                | XAuthorityInputDeliveryOutcome::ClientDisconnected
                | XAuthorityInputDeliveryOutcome::TimedOut => {
                    tracing::warn!(
                        "sophia_live_session_input_delivery schema=2 status=retired reason=client_failure outcome={:?} client={} failed={} session=continuing content=redacted",
                        outcome, delivery.client.raw(), self.state.events_failed,
                    );
                }
            }
        }
        if pass == 0 && let Some(sender) = self.sender {
            for ticket in sender.recover_input_deliveries(now, false)? {
                crate::session_println!(
                    "sophia_live_session_input_recovery schema=1 status=revoked delivery={} client={} surface={} generation={} seat={} control_epoch={} age_msec={} reason=delivery_deadline release_barrier={} content=redacted",
                    ticket.delivery.raw(), ticket.client.map_or(0, |client| client.raw()),
                    ticket.surface.index(), ticket.surface.generation(), ticket.seat.raw(),
                    ticket.control_epoch, ticket.admitted_at.elapsed().as_millis(),
                    self.client_key_release_barrier.contains(&ticket.delivery),
                );
            }
        }
        }
        if let Some(wait_started) = self.state.wait_started_at
            && !self.state.pending.is_empty()
            && wait_started.elapsed()
                >= Duration::from_millis(SESSION_INPUT_DELIVERY_TIMEOUT_MSEC)
        {
            return Err(InputDeliveryError::ProofTimeout.into());
        }
        if let Some(wait_started) = take_settled_input_delivery_wait(
            &mut self.state.wait_started_at,
            self.state.pending.is_empty(),
        )
            && self.proof_started_at.is_none()
        {
            let flushed_at = Instant::now();
            self.state.flush_latency = Some(flushed_at.saturating_duration_since(wait_started));
            *self.proof_started_at = Some(flushed_at);
            *self.post_input_deadline = Some(
                flushed_at + Duration::from_millis(SESSION_PHYSICAL_PIXEL_TIMEOUT_MSEC),
            );
            crate::session_println!(
                "sophia_live_session_input_pipeline schema=2 status=key_flushed source={} expected={} flushed={}",
                self.state.source.unwrap_or("unknown"),
                self.state.events_expected,
                self.state.events_flushed,
            );
            std::io::stdout().flush()?;
        }
        Ok(())
    }
}
