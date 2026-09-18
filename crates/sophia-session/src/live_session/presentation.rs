/// Frame intervals retained for the cadence summary.
///
/// A sliding window rather than a quota: the oldest interval is dropped to make
/// room, and `first_ust` advances with it so elapsed time keeps matching the
/// intervals actually retained. An earlier revision latched an `overflowed`
/// flag at this bound and never cleared it, so one long session permanently
/// lost its own frame-pacing measurement. Recent cadence is also the more
/// useful answer, which is why the window is smaller than the quota was.
const PRESENT_CADENCE_CAPACITY: usize = 1_024;

#[derive(Debug)]
struct XPresentCadence {
    first_ust: Option<u64>,
    previous_ust: Option<u64>,
    intervals_usec: std::collections::VecDeque<u64>,
    nonadvancing: usize,
    /// Intervals aged out of the window. Never a reason to stop reporting.
    evicted: usize,
}

impl XPresentCadence {
    fn new() -> Self {
        Self {
            first_ust: None,
            previous_ust: None,
            intervals_usec: std::collections::VecDeque::with_capacity(PRESENT_CADENCE_CAPACITY),
            nonadvancing: 0,
            evicted: 0,
        }
    }

    fn observe(&mut self, ust: u64) {
        let Some(previous) = self.previous_ust else {
            self.first_ust = Some(ust);
            self.previous_ust = Some(ust);
            return;
        };
        if ust <= previous {
            self.nonadvancing = self.nonadvancing.saturating_add(1);
            return;
        }
        if self.intervals_usec.len() == PRESENT_CADENCE_CAPACITY
            && let Some(oldest) = self.intervals_usec.pop_front()
        {
            // The window start moves by exactly the interval that left it, so
            // elapsed time and retained intervals stay describing each other.
            self.first_ust = self.first_ust.map(|first| first.saturating_add(oldest));
            self.evicted = self.evicted.saturating_add(1);
        }
        self.intervals_usec.push_back(ust - previous);
        self.previous_ust = Some(ust);
    }

    fn summary(&self) -> Option<XPresentCadenceSummary> {
        if self.intervals_usec.len() < 2 {
            return None;
        }
        let first_ust = self.first_ust?;
        let last_ust = self.previous_ust?;
        let elapsed_usec = last_ust.checked_sub(first_ust)?;
        if elapsed_usec == 0 {
            return None;
        }
        let mut sorted_intervals = self.intervals_usec.iter().copied().collect::<Vec<_>>();
        sorted_intervals.sort_unstable();
        let p95_index = sorted_intervals
            .len()
            .saturating_mul(95)
            .div_ceil(100)
            .saturating_sub(1);
        Some(XPresentCadenceSummary {
            samples: self.intervals_usec.len().saturating_add(1),
            advancing_intervals: self.intervals_usec.len(),
            nonadvancing: self.nonadvancing,
            mean_fps: self.intervals_usec.len() as f64 * 1_000_000.0
                / elapsed_usec as f64,
            p95_frame_msec: sorted_intervals[p95_index] as f64 / 1_000.0,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct XPresentCadenceSummary {
    samples: usize,
    advancing_intervals: usize,
    nonadvancing: usize,
    mean_fps: f64,
    p95_frame_msec: f64,
}

struct XPresentSessionObserver {
    router: XServerFrontendProtocolRouter,
    displayed_cadence: XPresentCadence,
    diagnostic_events: bool,
    visual_progress_events: bool,
    aggregate_progress: bool,
    progress_last_emit: Option<Instant>,
    complete_copy: usize,
    /// X `Flip` completions whose predecessor stayed on glass -- the
    /// `Retained` disposition. Never includes direct scanout; the session
    /// line, which reports X completion modes rather than dispositions, adds
    /// the two together.
    complete_flip: usize,
    /// X `Flip` completions where the client's own buffer reached the plane
    /// uncomposed. Counted apart so that a session with no direct scanout
    /// cannot report the number that proves this row.
    complete_direct: usize,
    complete_skip: usize,
    idle: usize,
    complete_routed: usize,
    idle_routed: usize,
    route_failures: usize,
    idle_fence_triggers: usize,
    disconnect_sources: usize,
    disconnect_fences: usize,
    disconnect_failures: usize,
}

impl XPresentSessionObserver {
    /// Completions the X client saw as `Flip`, whatever put them there.
    ///
    /// The session record reports X completion modes, and both `Retained` and
    /// a direct flip are `Flip` to a client. Reporting their sum keeps
    /// `present_idle == copy + flip + skip` true, which is the identity the
    /// persistent-evidence verifier checks, without conflating the two
    /// anywhere the distinction is what is being proven.
    pub const fn complete_flip_modes(&self) -> usize {
        self.complete_flip.saturating_add(self.complete_direct)
    }

    pub const fn complete_direct(&self) -> usize {
        self.complete_direct
    }

    fn new(router: XServerFrontendProtocolRouter) -> Self {
        let aggregate_progress =
            std::env::var_os("SOPHIA_LIVE_SESSION_PRESENT_AGGREGATE").is_some();
        Self {
            router,
            displayed_cadence: XPresentCadence::new(),
            diagnostic_events: std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_some()
                && !aggregate_progress,
            visual_progress_events: std::env::var("SOPHIA_LIVE_VISUAL_PROGRESS")
                .is_ok_and(|value| matches!(value.as_str(), "1" | "true")),
            aggregate_progress,
            progress_last_emit: None,
            complete_copy: 0,
            complete_flip: 0,
            complete_direct: 0,
            complete_skip: 0,
            idle: 0,
            complete_routed: 0,
            idle_routed: 0,
            route_failures: 0,
            idle_fence_triggers: 0,
            disconnect_sources: 0,
            disconnect_fences: 0,
            disconnect_failures: 0,
        }
    }

    fn drain_pending_feedback_with_layout(
        &mut self,
        runtime: &mut LiveProductionVisualRuntime,
        pending: &mut Vec<sophia_backend_live::LivePresentFeedbackOutcome>,
        layout: &mut PersistentLiveLayout,
        mut comparison: impl FnMut(
            &PersistentLiveLayout,
            &LiveProductionVisualRuntime,
            &sophia_backend_live::LivePresentFeedbackOutcome,
        )
            -> Option<(TransactionId, sophia_x_authority::XPresentLayoutComparison)>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.drain_pending_feedback_observed(runtime, pending, |runtime, outcome| {
            layout.observe_terminal_present_feedback(outcome);
            comparison(layout, runtime, outcome)
        })
    }

    fn drain_pending_feedback_observed(
        &mut self,
        runtime: &mut LiveProductionVisualRuntime,
        pending: &mut Vec<sophia_backend_live::LivePresentFeedbackOutcome>,
        mut observe: impl FnMut(
            &LiveProductionVisualRuntime,
            &sophia_backend_live::LivePresentFeedbackOutcome,
        )
            -> Option<(TransactionId, sophia_x_authority::XPresentLayoutComparison)>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        pending.clear();
        runtime.drain_present_feedback_into(pending)?;
        for outcome in pending.drain(..) {
            if self.visual_progress_events {
                for feedback in &outcome.feedback {
                    let (transaction, kind) = match feedback {
                        sophia_backend_live::LivePresentProtocolFeedback::Complete { transaction, .. } => (*transaction, "complete"),
                        sophia_backend_live::LivePresentProtocolFeedback::Idle { transaction } => (*transaction, "idle"),
                    };
                    crate::session_println!(
                        "sophia_live_visual_progress schema=1 status=feedback_ready transaction={} kind={kind}",
                        transaction.raw(),
                    );
                }
            }
            let comparison = observe(runtime, &outcome);
            self.observe_feedback(outcome, comparison);
        }
        Ok(())
    }

    fn observe_feedback(
        &mut self,
        outcome: sophia_backend_live::LivePresentFeedbackOutcome,
        comparison: Option<(
            sophia_protocol::TransactionId,
            sophia_x_authority::XPresentLayoutComparison,
        )>,
    ) {
        if outcome.idle_fence_triggered {
            self.idle_fence_triggers = self.idle_fence_triggers.saturating_add(1);
        }
        for feedback in outcome.feedback {
            match feedback {
                sophia_backend_live::LivePresentProtocolFeedback::Complete {
                    transaction,
                    ust,
                    msc,
                    disposition,
                } => {
                    let mode = match disposition {
                        sophia_backend_live::LivePresentBufferDisposition::Copied => {
                            self.complete_copy = self.complete_copy.saturating_add(1);
                            XPresentCompletionMode::Copy
                        }
                        sophia_backend_live::LivePresentBufferDisposition::Retained => {
                            self.complete_flip = self.complete_flip.saturating_add(1);
                            XPresentCompletionMode::Flip
                        }
                        // Counted apart from `Retained`. Both report X `Flip`,
                        // but only this one means a client's own buffer went to
                        // the plane uncomposed, and conflating them would let a
                        // session with no direct scanout at all report the
                        // number that proves this row.
                        sophia_backend_live::LivePresentBufferDisposition::Flipped => {
                            self.complete_direct = self.complete_direct.saturating_add(1);
                            XPresentCompletionMode::Flip
                        }
                        sophia_backend_live::LivePresentBufferDisposition::Skipped => {
                            self.complete_skip = self.complete_skip.saturating_add(1);
                            XPresentCompletionMode::Skip
                        }
                    };
                    let comparison = comparison
                        .filter(|(candidate, _)| {
                            *candidate == transaction && mode == XPresentCompletionMode::Copy
                        })
                        .map(|(_, comparison)| comparison);
                    match self
                        .router
                        .route_present_complete_with_layout(
                            transaction, ust, msc, mode, comparison,
                        )
                    {
                        Ok(route) => {
                            let routed = route.routed;
                            let mode = route.mode;
                            if matches!(
                                route.layout_comparison,
                                Some(sophia_x_authority::XPresentLayoutComparisonResult::Matched)
                            )
                                && let Some(comparison) = comparison
                            {
                                crate::session_println!(
                                    "sophia_live_layout_probe schema=2 status=PreferenceMatched transaction={} output={} native_generation={} preference_generation={} format={} original_modifier={} alternative_modifier={}",
                                    transaction.raw(),
                                    comparison.native_context.output.raw(),
                                    comparison.native_context.generation,
                                    comparison.preference_generation,
                                    comparison.format,
                                    comparison.original_modifier,
                                    comparison.alternative_modifier,
                                );
                            }
                            self.complete_routed =
                                self.complete_routed.saturating_add(usize::from(routed));
                            if routed
                                && matches!(
                                    mode,
                                    XPresentCompletionMode::Copy
                                        | XPresentCompletionMode::Flip
                                        | XPresentCompletionMode::SuboptimalCopy
                                )
                            {
                                self.displayed_cadence.observe(ust);
                            }
                            if self.diagnostic_events || self.visual_progress_events {
                                crate::session_eprintln!(
                                    "sophia_live_session_present_feedback schema=1 kind=complete transaction={} routed={routed} mode={mode:?} ust={ust} msc={msc}",
                                    transaction.raw(),
                                );
                            }
                        }
                        Err(error) => {
                            self.route_failures = self.route_failures.saturating_add(1);
                            crate::session_eprintln!(
                                "sophia_live_session_present_feedback schema=1 kind=complete transaction={} routed=false error={error}",
                                transaction.raw(),
                            );
                        }
                    }
                }
                sophia_backend_live::LivePresentProtocolFeedback::Idle { transaction } => {
                    self.idle = self.idle.saturating_add(1);
                    match self.router.route_present_idle(transaction) {
                        Ok(routed) => {
                            self.idle_routed = self.idle_routed.saturating_add(usize::from(routed));
                            if self.diagnostic_events || self.visual_progress_events {
                                crate::session_eprintln!(
                                    "sophia_live_session_present_feedback schema=1 kind=idle transaction={} routed={routed}",
                                    transaction.raw(),
                                );
                            }
                        }
                        Err(error) => {
                            self.route_failures = self.route_failures.saturating_add(1);
                            crate::session_eprintln!(
                                "sophia_live_session_present_feedback schema=1 kind=idle transaction={} routed=false error={error}",
                                transaction.raw(),
                            );
                        }
                    }
                }
            }
        }
        self.emit_progress(false);
    }

    fn emit_progress(&mut self, force: bool) {
        if !self.aggregate_progress {
            return;
        }
        let now = Instant::now();
        if !force
            && self
                .progress_last_emit
                .is_some_and(|last| now.duration_since(last) < Duration::from_millis(250))
        {
            return;
        }
        self.progress_last_emit = Some(now);
        // Cumulative samples keep production evidence observable without making
        // synchronous serial output part of every Present feedback transition.
        crate::session_eprintln!(
            "sophia_live_present_progress schema=2 complete_copy={} complete_flip={} complete_direct={} complete_skip={} idle={}",
            self.complete_copy,
            self.complete_flip,
            self.complete_direct,
            self.complete_skip,
            self.idle,
        );
    }

    fn observe_disconnect(
        &mut self,
        report: sophia_backend_live::LivePresentationDisconnectReport,
    ) {
        self.idle_fence_triggers = self
            .idle_fence_triggers
            .saturating_add(report.triggered_idle_fences);
        self.disconnect_sources = self
            .disconnect_sources
            .saturating_add(report.released_sources.len());
        self.disconnect_fences = self
            .disconnect_fences
            .saturating_add(report.released_fences.len());
        self.disconnect_failures = self
            .disconnect_failures
            .saturating_add(report.failed_idle_fences);
    }
}

fn production_authority_batch(
    batch: &XAuthorityObservedTransactionBatch,
    released_admission_groups: &[LiveAdmissionAuthorityGroup],
    layout: &PersistentLiveLayout,
) -> Result<LiveProductionAuthorityBatch, &'static str> {
    let mut groups = Vec::<sophia_backend_live::LiveProductionAuthorityGroup>::new();
    // Admission groups were quarantined before the current authority batch.
    // Release them first so same-surface generations remain FIFO. Appending
    // them after current work lets a newer generation reach Engine first and
    // makes the entire retained chain stale, including an exact recovery
    // target Present that arrived while its fallback was retiring.
    for released in released_admission_groups {
        let index = production_authority_group_index(&mut groups, released.transaction);
        groups[index]
            .transactions
            .extend(released.transactions.iter().cloned());
        let cpu_buffer_updates = released
            .cpu_buffer_updates
            .iter()
            .map(|update| production_cpu_buffer_update(&released.transactions, update))
            .collect::<Result<Vec<_>, _>>()?;
        groups[index]
            .cpu_buffer_updates
            .extend(cpu_buffer_updates);
        groups[index].present_submissions.extend(
            released
                .present_submissions
                .iter()
                .map(|submission| {
                    let layout_disposition = if released.superseded {
                        sophia_backend_live::LiveProductionPresentDisposition::RejectSuperseded
                    } else {
                        layout.present_layout_disposition(
                            submission.transaction,
                            submission.surface,
                            submission.buffer,
                        )
                    };
                    sophia_backend_live::LiveProductionPresentSubmission {
                        transaction: submission.transaction,
                        surface: submission.surface,
                        buffer: submission.buffer,
                        x_offset: submission.x_offset,
                        y_offset: submission.y_offset,
                        acquire_fence: submission.acquire_fence,
                        idle_fence: submission.idle_fence,
                        layout_disposition,
                    }
                }),
        );
        let software_presents = released
            .software_present_submissions
            .iter()
            .map(|submission| {
                production_software_present_submission(
                    &released.transactions,
                    *submission,
                    layout,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        groups[index]
            .software_present_submissions
            .extend(software_presents);
    }
    for transaction in &batch.transactions {
        let index = production_authority_group_index(&mut groups, transaction.transaction);
        groups[index].transactions.push(transaction.clone());
    }
    for update in &batch.cpu_buffer_updates {
        let update = production_cpu_buffer_update(&batch.transactions, update)?;
        let index = production_authority_group_index(&mut groups, update.identity.transaction);
        groups[index].cpu_buffer_updates.push(update);
    }
    if !batch.removed_surfaces.is_empty() {
        let index = production_authority_group_index(&mut groups, batch.transaction);
        groups[index]
            .removed_surfaces
            .extend(batch.removed_surfaces.iter().copied());
    }
    for submission in &batch.present_submissions {
        let index = production_authority_group_index(&mut groups, submission.transaction);
        groups[index].present_submissions.push(
            sophia_backend_live::LiveProductionPresentSubmission {
                transaction: submission.transaction,
                surface: submission.surface,
                buffer: submission.buffer,
                x_offset: submission.x_offset,
                y_offset: submission.y_offset,
                acquire_fence: submission.acquire_fence,
                idle_fence: submission.idle_fence,
                layout_disposition: layout
                    .present_layout_disposition(
                        submission.transaction,
                        submission.surface,
                        submission.buffer,
                    ),
            },
        );
    }
    for submission in &batch.software_present_submissions {
        let index = production_authority_group_index(&mut groups, submission.transaction);
        groups[index]
            .software_present_submissions
            .push(production_software_present_submission(
                &batch.transactions,
                *submission,
                layout,
            )?);
    }
    Ok(LiveProductionAuthorityBatch {
        groups,
        dma_buf_registrations: batch
            .dma_buf_registrations
            .iter()
            .map(|registration| LiveProductionDmaBufRegistration {
                descriptor: registration.descriptor,
                plane_fds: registration.plane_fds.clone(),
            })
            .collect(),
        fence_registrations: batch
            .fence_registrations
            .iter()
            .map(|registration| LiveProductionFenceRegistration {
                handle: registration.handle,
                initially_triggered: registration.initially_triggered,
                fd: Arc::clone(&registration.fd),
            })
            .collect(),
        released_dma_bufs: batch.released_dma_bufs.clone(),
        released_fences: batch.released_fences.clone(),
    })
}

fn production_software_present_submission(
    transactions: &[sophia_protocol::SurfaceTransaction],
    submission: sophia_x_authority::XAuthoritySoftwarePresentSubmission,
    layout: &PersistentLiveLayout,
) -> Result<sophia_backend_live::LiveProductionSoftwarePresentSubmission, &'static str> {
    let mut candidates = transactions.iter().filter(|transaction| {
        transaction.transaction == submission.transaction
            && transaction.surface == submission.surface
            && matches!(transaction.target_buffer(), BufferSource::CpuBuffer { .. })
    });
    let candidate = candidates
        .next()
        .ok_or("software Present has no exact CPU transaction")?;
    if candidates.next().is_some() {
        return Err("software Present has multiple CPU transactions");
    }
    let source_size = live_transaction_pixel_size(
        candidate.target_buffer(),
        &layout.dma_buf_sizes,
        &layout.cpu_buffer_sizes,
    )
    .ok_or("software Present CPU buffer has no known extent")?;
    Ok(sophia_backend_live::LiveProductionSoftwarePresentSubmission {
        candidate: candidate.key(),
        source_size,
        transaction: submission.transaction,
        surface: submission.surface,
        acquire_fence: submission.acquire_fence,
        idle_fence: submission.idle_fence,
    })
}

fn production_cpu_buffer_update(
    transactions: &[sophia_protocol::SurfaceTransaction],
    update: &sophia_x_authority::XAuthorityCpuBufferUpdate,
) -> Result<sophia_backend_live::LiveProductionCpuBufferUpdate, &'static str> {
    let mut owners = transactions.iter().filter(|transaction| {
        transaction.content.variants().iter().any(|variant| {
            matches!(
                variant.source,
                BufferSource::CpuBuffer { handle } if handle == update.handle()
            )
        })
    });
    let owner = owners
        .next()
        .ok_or("CPU update has no surface transaction")?;
    if owners.next().is_some() {
        return Err("CPU update has multiple surface transactions");
    }
    Ok(sophia_backend_live::LiveProductionCpuBufferUpdate::new(
        owner.transaction,
        owner.surface,
        renderer_cpu_buffer_update(update),
    ))
}

fn production_authority_group_index(
    groups: &mut Vec<sophia_backend_live::LiveProductionAuthorityGroup>,
    transaction: TransactionId,
) -> usize {
    if let Some(index) = groups
        .iter()
        .position(|group| group.transaction == transaction)
    {
        return index;
    }
    groups.push(sophia_backend_live::LiveProductionAuthorityGroup {
        transaction,
        transactions: Vec::new(),
        cpu_buffer_updates: Vec::new(),
        removed_surfaces: Vec::new(),
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
    });
    groups.len() - 1
}

fn renderer_cpu_buffer_update(
    update: &sophia_x_authority::XAuthorityCpuBufferUpdate,
) -> sophia_backend_live::LiveCpuBufferUpdate {
    match update {
        sophia_x_authority::XAuthorityCpuBufferUpdate::Replace(buffer) => {
            sophia_backend_live::LiveCpuBufferUpdate::Replace(
                sophia_backend_live::LiveCpuBufferSource {
                    handle: buffer.handle,
                    size: buffer.size,
                    stride: buffer.stride,
                    format: buffer.format,
                    generation: buffer.generation,
                    // A refcount bump. The authority's snapshot and the registry's copy
                    // share one allocation until one of them writes.
                    bytes: std::sync::Arc::clone(&buffer.bytes),
                },
            )
        }
        sophia_x_authority::XAuthorityCpuBufferUpdate::Patch(patch) => {
            sophia_backend_live::LiveCpuBufferUpdate::Patch(
                sophia_backend_live::LiveCpuBufferPatch {
                    handle: patch.handle,
                    size: patch.size,
                    stride: patch.stride,
                    format: patch.format,
                    generation: patch.generation,
                    rect: patch.rect,
                    bytes: patch.bytes.clone(),
                },
            )
        }
        sophia_x_authority::XAuthorityCpuBufferUpdate::PatchBatch(batch) => {
            sophia_backend_live::LiveCpuBufferUpdate::PatchBatch(
                sophia_backend_live::LiveCpuBufferPatchBatch {
                    handle: batch.handle,
                    size: batch.size,
                    stride: batch.stride,
                    format: batch.format,
                    generation: batch.generation,
                    patches: batch
                        .patches
                        .iter()
                        .map(|patch| sophia_backend_live::LiveCpuBufferPatchRegion {
                            rect: patch.rect,
                            bytes: patch.bytes.clone(),
                        })
                        .collect(),
                },
            )
        }
    }
}

fn synthetic_text_input_events(
    text: &str,
) -> Result<Vec<sophia_protocol::InputEventPacket>, Box<dyn std::error::Error>> {
    let mut serial = 1u64;
    let mut events = Vec::with_capacity((text.len() + 1).saturating_mul(2));
    for x_keycode in text
        .bytes()
        .map(crate::support::x11_keycode_for_ascii)
        .chain(std::iter::once(Some(36)))
    {
        let x_keycode = x_keycode.ok_or("test input has no core X keycode")?;
        let keycode = u32::from(
            x_keycode
                .checked_sub(8)
                .ok_or("test input has no evdev keycode")?,
        );
        for pressed in [true, false] {
            events.push(sophia_protocol::InputEventPacket {
                serial,
                seat: SeatId::from_raw(SESSION_SEAT_RAW),
                device: DeviceId::from_raw(SESSION_KEYBOARD_DEVICE_RAW),
                time_msec: serial,
                kind: sophia_protocol::InputEventKind::Key { keycode, pressed },
                global_position: None,
                target_surface: None,
                local_position: None,
            });
            serial = serial.saturating_add(1);
        }
    }
    Ok(events)
}
