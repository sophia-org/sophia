fn present_monotonic_usec() -> u64 {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    (now.tv_sec as u64).saturating_mul(1_000_000).saturating_add(now.tv_nsec as u64 / 1_000)
}

fn service_timed_presents(
    state: &X11CoreSocketServerState,
    routing: &XServerFrontendRouteRegistry,
    generated: &mut XGeneratedEgress,
    now_usec: u64,
) -> Result<bool, X11SetupSocketError> {
    if !state.present_service_demand.load(Ordering::Acquire) { return Ok(false); }
    let mut settled = false;
    let ready = {
        let mut runtime = state.runtime.lock()
            .map_err(|_| X11SetupSocketError::new("timed Present runtime lock poisoned"))?;
        runtime.record_present_service_lock(false);
        runtime.advance_prepared_present_fake_clocks(now_usec).map_err(X11SetupSocketError::new)?;
        routing.observe_completion_clock(crate::XPresentClockSource::Fake,
            Some(crate::XPresentClockSample::background(now_usec)), Some(&mut runtime))
            .map_err(|_| X11SetupSocketError::new("timed Present completion clock routing failed"))?;
        runtime.settle_prepared_present_fake_terminals(now_usec);
        // Scrapping never waits for the target or an acquire fence. Keep the
        // runtime lock through delivery so destruction cannot fall between
        // checking the obligation and routing its Idle. Direct admission
        // may already have delivered it; the feedback phase is idempotent.
        for preparation in runtime.prepared_present_idle_deliveries() {
            routing.route_present_idle(preparation)
                .map_err(|_| X11SetupSocketError::new("timed Present Idle delivery failed"))?;
            runtime.prepared_present_idle_delivered(preparation);
            settled = true;
        }
        for (preparation, ust, msc) in runtime.ready_prepared_skips() {
            routing.route_present_complete(preparation, ust, msc, XPresentCompletionMode::Skip)
                .map_err(|_| X11SetupSocketError::new("timed Present Skip delivery failed"))?;
            runtime.cancel_prepared_standard_pixmap(preparation);
            settled = true;
        }
        for notify in runtime.ready_prepared_msc_notifies() {
            routing.route_present_msc_notify(notify.window, notify.serial, notify.ust, notify.msc)
                .map_err(|_| X11SetupSocketError::new("timed NotifyMSC delivery failed"))?;
            runtime.cancel_prepared_msc_notify(notify.request);
            settled = true;
        }
        let ready = runtime.ready_prepared_presents();
        for &preparation in &ready { runtime.prepared_present_msc_serviced(preparation); }
        if !runtime.prepared_present_service_pending() {
            let pending = routing.pending_presentations.entries.lock()
                .map_err(|_| X11SetupSocketError::new("timed Present feedback registry poisoned"))?;
            let fake_pending = pending.values().any(|p| !p.phases.completed()
                && p.clock.as_ref().is_some_and(|c| c.binding.source == crate::XPresentClockSource::Fake));
            if !fake_pending {
                // Same lock order as preparation -> executed feedback transfer.
                // No producer can publish between this proof and the clear.
                state.present_service_demand.store(false, Ordering::Release);
            }
        }
        ready
    };
    // At most one execution per pass and one outstanding generated Present
    // envelope. A fence-blocked request does not starve other windows.
    for preparation in ready {
        if execute_timed_present(state, routing, preparation, generated,
            |runtime, _| timed_present_ready(runtime, preparation, now_usec))?
        {
            return Ok(true);
        }
    }
    Ok(settled)
}

fn timed_present_ready(runtime: &mut XAuthorityRuntime, preparation: TransactionId,
    now_usec: u64) -> Result<bool, X11SetupSocketError>
{
    // Cancellation or source loss can turn a considered Pixmap into Skip
    // between the service's peek and this lock hold. Recheck at execution.
    if !runtime.prepared_present_is_ready(preparation) { return Ok(false); }
    runtime.poll_prepared_present_fence(preparation, now_usec).map_err(X11SetupSocketError::new)
}

fn timed_present_service_deadline(
    state: &X11CoreSocketServerState,
    now_usec: u64,
    now: Instant,
    egress_or_revocations_pending: bool,
    present_slot_vacant: bool,
) -> Result<Option<Instant>, X11SetupSocketError> {
    let retry = egress_or_revocations_pending.then_some(Duration::from_millis(1));
    if !state.present_service_demand.load(Ordering::Acquire) {
        return Ok(retry.and_then(|delay| now.checked_add(delay)));
    }
    let mut runtime = state.runtime.lock()
        .map_err(|_| X11SetupSocketError::new("timed Present runtime lock poisoned"))?;
    runtime.record_present_service_lock(true);
    // Transport obligations retain their bounded retry. Fence checks have
    // per-request backoff; a stuck fence cannot cause a 1 kHz idle loop.
    // Future fake targets keep exact deadlines, including behind a fence-
    // blocked ready request. Hardware progress is never extrapolated here.
    let clock = runtime.prepared_present_service_deadline_usec(now_usec, present_slot_vacant)
        .map(|deadline| Duration::from_micros(deadline.saturating_sub(now_usec)));
    let delay = match (retry, clock) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    Ok(delay.and_then(|delay| now.checked_add(delay)))
}

/// Complete one timing/fence-admitted request. The runtime lock covers
/// preparation removal, execution and feedback-reservation transfer. Window
/// destruction and client cleanup can occur before or after, never between.
/// No ticket is allocated for a preparation cancelled before this visit.
/// Nested lock order is runtime -> clients -> pending. This path only
/// transfers an existing reservation; it must never call queue_present,
/// whose capacity wait happens outside the runtime lock at wire admission.
fn execute_timed_present(
    state: &X11CoreSocketServerState,
    routing: &XServerFrontendRouteRegistry,
    preparation: TransactionId,
    generated: &mut XGeneratedEgress,
    fence_ready: impl FnOnce(&mut XAuthorityRuntime, Option<sophia_protocol::FenceHandle>) -> Result<bool, X11SetupSocketError>,
) -> Result<bool, X11SetupSocketError> {
    if !generated.vacant(XGeneratedEgressKind::Present) { return Ok(false); }
    let mut runtime = state.runtime.lock()
        .map_err(|_| X11SetupSocketError::new("timed Present runtime lock poisoned"))?;
    let Some((client, namespace, window)) = runtime.prepared_present_origin(preparation) else {
        return Ok(false);
    };
    let clients = routing.clients.lock()
        .map_err(|_| X11SetupSocketError::new("timed Present client registry poisoned"))?;
    let mut pending = routing.pending_presentations.entries.lock()
        .map_err(|_| X11SetupSocketError::new("timed Present feedback registry poisoned"))?;
    let Some(reservation) = pending.get(&preparation).copied() else {
        runtime.cancel_prepared_standard_pixmap(preparation);
        return Ok(false);
    };
    if !clients.contains_key(&reservation.client) {
        runtime.cancel_prepared_standard_pixmap(preparation);
        pending.remove(&preparation);
        routing.pending_presentations.capacity_changed.notify_all();
        return Ok(false);
    }
    if reservation.client.raw() != client || reservation.window != window {
        return Err(X11SetupSocketError::new("timed Present preparation origin mismatch"));
    }
    let fences = runtime.prepared_present_fences(preparation)
        .expect("preparation is held under runtime lock");
    if !fence_ready(&mut runtime, fences.wait)? { return Ok(false); }
    let root = runtime.window_presentation_root_and_offset(namespace, window).ok();
    let route = root.map(|(_, surface, _, _)| routing.surface_route_observation(surface))
        .transpose().map_err(|_| X11SetupSocketError::new("timed Present surface registry poisoned"))?
        .flatten();
    if route.is_none() {
        runtime.skip_unrouted_prepared_present(preparation).map_err(X11SetupSocketError::new)?;
        return Ok(true);
    }
    // Reserve the fresh ticket in caller-owned egress before execution. Even
    // a rejected response must advance it; the guard keeps it on every exit.
    let transaction = state.allocate_transaction()?;
    generated.insert(XGeneratedEgressKind::Present,
        XAuthorityBoundedEgressEnvelope::new(transaction, None));
    runtime.begin_dispatch();
    let execution = runtime.execute_prepared_standard_pixmap(preparation, transaction)
        .map_err(|_| X11SetupSocketError::new("timed Present execution ticket invalid"))?
        .expect("preparation cannot disappear while runtime is locked");
    let accepted = execution.response.outcome == crate::XAuthorityResponseOutcome::Accepted;
    let mut batch = XAuthorityObservedTransactionBatch::from_authority_response(&execution.response);
    batch.client = Some(reservation.client);
    batch.admission = reservation.admission;
    batch.cpu_buffer_updates = runtime.take_cpu_buffer_updates();
    batch.released_dma_bufs = runtime.take_retired_pixmap_registrations(namespace);
    if accepted && let Some(update) = execution.response.transactions.first() {
        let (_, surface, child_x, child_y) = root.expect("accepted execution has a root");
        assert_eq!(update.surface, surface, "timed Present changed a different surface");
        match update.target_buffer() {
            sophia_protocol::BufferSource::DmaBuf { handle } => {
                let present = XAuthorityPresentSubmission {
                    transaction, surface, buffer: sophia_protocol::BufferHandle::from_raw(handle),
                    x_offset: child_x.saturating_add(i32::from(execution.offset.0)),
                    y_offset: child_y.saturating_add(i32::from(execution.offset.1)),
                    acquire_fence: execution.fences.wait, idle_fence: execution.fences.idle,
                };
                batch.present_submissions.push(present);
            }
            sophia_protocol::BufferSource::CpuBuffer { .. } => {
                batch.software_present_submissions.push(crate::XAuthoritySoftwarePresentSubmission {
                    transaction, surface, acquire_fence: execution.fences.wait,
                    idle_fence: execution.fences.idle,
                });
            }
            _ => unreachable!("prepared Present executes a pixmap backing"),
        }
        batch.surface_routes.extend(route);
    }
    if accepted && (!batch.present_submissions.is_empty() || !batch.software_present_submissions.is_empty()) {
        let mut reservation = pending.remove(&preparation).expect("held feedback reservation");
        reservation.clock = execution.schedule.map(|request| XPresentCompletionState::new(
            request.binding,
            execution.clock_sample.expect("a scheduled execution retains its observation"),
        ));
        reservation.allocation_subject = batch.present_submissions.first().and_then(|present|
            runtime.present_allocation_subject(namespace, client, window, reservation.pixmap, present));
        assert!(pending.insert(transaction, reservation).is_none(), "fresh execution ticket already reserved");
    } else if runtime.prepared_present_origin(preparation).is_none() {
        return Err(X11SetupSocketError::new("timed Present rejection lost its completion target"));
    }
    let has_effects = !batch.transactions.is_empty() || !batch.surface_presentations.is_empty()
        || !batch.cpu_buffer_updates.is_empty() || !batch.released_dma_bufs.is_empty();
    if has_effects {
        let envelope = generated.slots[XGeneratedEgressKind::Present.index()].as_mut().expect("owned execution envelope");
        envelope.client = batch.client;
        envelope.observed_batch = true;
        envelope.batch = Some(batch);
    }
    tracing::debug!("sophia_x_present_execution schema=1 request_transaction={} execution_transaction={} client={} accepted={}",
        preparation.raw(), transaction.raw(), client, u8::from(accepted));
    Ok(true)
}
