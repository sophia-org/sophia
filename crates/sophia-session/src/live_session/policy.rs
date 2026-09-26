/// Distinct (major, minor, code) triples retained before the tally resets.
///
/// Reset losses are counted separately so the retained rows and discarded
/// observations reconcile with the session total.
const SESSION_PROTOCOL_ERROR_TALLY_MAX_ENTRIES: usize = 64;

/// Every X protocol error a session saw, grouped by the request that produced it.
///
/// The session keeps only the first error otherwise, so a run reporting two dozen
/// failures named one opcode and discarded the rest. Each cause then cost its own
/// physical run to find. This carries opcodes and counts only: the resource id
/// stays inside the frontend, because a raw XID may not reach a default-level log.
#[derive(Debug, Default)]
struct SessionProtocolErrorTally {
    counts: BTreeMap<(u8, u16, u8), u64>,
    discarded: u64,
}

impl SessionProtocolErrorTally {
    fn observe(&mut self, error: &sophia_x_authority::XAuthorityProtocolErrorObservation) {
        let key = (error.major_code, error.minor_code, error.code);
        if self.counts.len() >= SESSION_PROTOCOL_ERROR_TALLY_MAX_ENTRIES
            && !self.counts.contains_key(&key)
        {
            // Keep the newest opcodes rather than the oldest, and say how many
            // observations the reset dropped so the total still reconciles.
            self.discarded = self
                .discarded
                .saturating_add(self.counts.values().copied().fold(0, u64::saturating_add));
            self.counts.clear();
        }
        let count = self.counts.entry(key).or_insert(0);
        *count = count.saturating_add(1);
    }

    fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Every observation the tally holds, reset losses included.
    ///
    /// Reported alongside the per-opcode lines so a reader can reconcile them:
    /// the lines sum to this only when `discarded` is zero.
    fn total(&self) -> u64 {
        self.counts
            .values()
            .copied()
            .fold(self.discarded, u64::saturating_add)
    }

    /// One line per opcode, never a merged bucket, so a run that hit two causes
    /// cannot disguise itself as one.
    ///
    /// A tally that saw nothing still produces a line. An absent line is
    /// indistinguishable from a line nobody emitted, and the difference between
    /// "refused nothing" and "never counted" is the whole point of the tally.
    ///
    /// Built rather than printed so the shape can be asserted; `report` prints
    /// what this returns.
    fn report_lines(&self) -> Vec<String> {
        let distinct = self.counts.len();
        if self.counts.is_empty() {
            return vec![format!(
                "sophia_live_session_protocol_error_tally schema=3 status=clean major=0 minor=0 code=0 count=0 distinct=0 discarded={} total={}",
                self.discarded, self.total()
            )];
        }
        self.counts
            .iter()
            .map(|((major, minor, code), count)| {
                format!(
                    "sophia_live_session_protocol_error_tally schema=3 status=compatibility_refusals major={major} minor={minor} code={code} count={count} distinct={distinct} discarded={} total={}",
                    self.discarded, self.total()
                )
            })
            .collect()
    }

    fn report(&self) {
        for line in self.report_lines() {
            crate::session_println!("{line}");
        }
    }

    /// A compact summary for the failure string, which is all a failed run prints.
    fn summary(&self) -> String {
        self.counts
            .iter()
            .map(|((major, minor, code), count)| {
                format!("{major}/{minor}/{code}x{count}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A session failure, carrying the requests the session refused alongside it.
///
/// The two used to be reported apart, and only when the run completed cleanly:
/// a session that refused seven requests and then timed out waiting for input
/// named the timeout alone. Which of the two explains the run is exactly the
/// question an operator is trying to answer, so neither is reported without the
/// other.
fn session_failure_with_refused_requests(
    failure: &str,
    tally: &SessionProtocolErrorTally,
) -> String {
    if tally.is_empty() {
        return failure.to_owned();
    }
    format!(
        "{failure}; the session also refused {} X requests by_opcode=[{}]",
        tally.total(),
        tally.summary()
    )
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SessionFatalCleanupEvidence {
    frontend_intake_stopped: bool,
    native_heads_in_flight_before: usize,
    native_cleanup_required: bool,
    native_suspend_attempted: bool,
    native_suspend_reported: bool,
    native_drained: bool,
    abandoned_scanouts: usize,
    renderer_images_cleared: bool,
    presentations_shutdown: bool,
}

impl SessionFatalCleanupEvidence {
    const fn clean(self) -> bool {
        self.frontend_intake_stopped
            && (!self.native_cleanup_required
            || (self.native_suspend_attempted
                && self.native_suspend_reported
                && self.native_drained
                && self.abandoned_scanouts == 0
                && self.renderer_images_cleared))
            && self.presentations_shutdown
    }
}

fn settle_session_fatal_error(
    original: &str,
    evidence: SessionFatalCleanupEvidence,
    cleanup_failures: &[String],
) -> String {
    if evidence.clean() && cleanup_failures.is_empty() {
        return original.to_owned();
    }
    let details = if cleanup_failures.is_empty() {
        "bounded cleanup did not reach a clean terminal state".to_owned()
    } else {
        cleanup_failures.join("; ")
    };
    format!("{original}; bounded session cleanup failed: {details}")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PhysicalInputRoutingMode {
    Suppressed,
    CursorOnly,
    ShortcutsOnly,
    ControlPlaneOnly,
    Full,
}

fn physical_input_routing_mode_label(mode: PhysicalInputRoutingMode) -> &'static str {
    match mode {
        PhysicalInputRoutingMode::Suppressed => "suppressed",
        PhysicalInputRoutingMode::CursorOnly => "cursor_only",
        PhysicalInputRoutingMode::ShortcutsOnly => "shortcuts_only",
        PhysicalInputRoutingMode::ControlPlaneOnly => "control_plane_only",
        PhysicalInputRoutingMode::Full => "full",
    }
}

fn physical_input_routing_mode(
    primary_child_exited: bool,
    focused_surface: Option<SurfaceId>,
    proof_surface: Option<SurfaceId>,
    global_shortcuts_available: bool,
) -> PhysicalInputRoutingMode {
    if focused_surface.is_none()
        || !primary_child_exited
        || focused_surface != proof_surface
    {
        PhysicalInputRoutingMode::Full
    } else if global_shortcuts_available {
        PhysicalInputRoutingMode::ShortcutsOnly
    } else {
        PhysicalInputRoutingMode::Suppressed
    }
}

fn pending_wm_focus_after_engine_decision(
    request: (TransactionId, SurfaceId),
    decision: InputFocusDecision,
) -> Option<(TransactionId, SurfaceId)> {
    // An unchanged focus satisfies the request as fully as a moved one. Treating
    // it as unsatisfied would keep the request pending and re-arm it every turn,
    // which is the loop this idempotency exists to end.
    (!matches!(
        decision,
        InputFocusDecision::Focused | InputFocusDecision::AlreadyFocused
    ))
    .then_some(request)
}

struct InitialSessionFocusContext<'a> {
    runtime: &'a LiveProductionVisualRuntime,
    focus: &'a mut InputFocusState,
    seat: SeatId,
    wm_session_present: bool,
    layout: &'a PersistentLiveLayout,
    session_controls: &'a mut SessionControlQueue,
    next_focus_control_transaction: &'a mut u64,
}

fn initial_session_focus_candidate(
    wm_session_present: bool,
    focused_surface: Option<SurfaceId>,
    committed_surfaces: &[CommittedSurfaceState],
    layout: &PersistentLiveLayout,
) -> Option<SurfaceId> {
    if wm_session_present || focused_surface.is_some() {
        return None;
    }
    // Committed is not visible, in either direction. A surface that has gone
    // hidden stays in the committed set, so without this the first surface to
    // be cleared for going hidden is immediately proposed again: the session
    // has just decided it must not hold the focus, and then asks for it back.
    // The authority refuses a focus on a window that is not viewable, which
    // it reports as a rejected control, which is fatal.
    //
    // `input_eligible` is the same predicate the hidden path uses to decide a
    // surface can no longer answer input, so the two agree by construction.
    committed_surfaces
        .iter()
        .map(|committed| committed.surface)
        .find(|surface| layout.input_eligible(*surface))
}

fn reconcile_initial_session_focus(
    context: InitialSessionFocusContext<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let InitialSessionFocusContext {
        runtime,
        focus,
        seat,
        wm_session_present,
        layout,
        session_controls,
        next_focus_control_transaction,
    } = context;
    let Some(surface) = initial_session_focus_candidate(
        wm_session_present,
        focus.focused_surface(seat),
        runtime.committed_surfaces(),
        layout,
    ) else {
        return Ok(());
    };
    // `AlreadyFocused` is unreachable here: the candidate is only produced when
    // the seat holds no focus at all, so a decision other than `Focused` means
    // the surface could not take it.
    if focus.focus_surface(seat, surface, runtime.committed_surfaces())
        != InputFocusDecision::Focused
    {
        return Ok(());
    }
    let client = layout
        .client_routes
        .client_for_surface(surface)
        .ok_or("initial X11 focus has no client route")?;
    let transaction = TransactionId::from_raw(*next_focus_control_transaction);
    *next_focus_control_transaction = next_focus_control_transaction
        .checked_add(1)
        .ok_or("initial X11 focus transaction exhausted")?;
    session_controls
        .enqueue(XAuthorityClientControlCommand {
            client,
            command: XAuthorityControlCommand::FocusSurface {
                transaction,
                surface,
            },
        }, Instant::now())
        .map_err(|error| format!("failed to queue initial X11 focus: {error:?}"))?;
    Ok(())
}

fn authority_transaction_count(transactions: &[SurfaceTransaction]) -> usize {
    transactions.len()
}

fn take_settled_input_delivery_wait(
    wait_started: &mut Option<Instant>,
    pending_deliveries_empty: bool,
) -> Option<Instant> {
    if pending_deliveries_empty {
        wait_started.take()
    } else {
        None
    }
}

fn record_runtime_commits(committed: u64, accepted_transactions: usize) -> u64 {
    committed.saturating_add(u64::try_from(accepted_transactions).unwrap_or(u64::MAX))
}

fn physical_input_pixels_already_changed(
    baseline_checksum: Option<u64>,
    current_checksum: Option<u64>,
    input_surface_changed: bool,
) -> bool {
    input_surface_changed
        && baseline_checksum
            .zip(current_checksum)
            .is_some_and(|(baseline, current)| baseline != current)
}

fn input_baseline_is_presented(
    focused_gpu_presented: bool,
    cpu_baseline_presented: bool,
) -> bool {
    focused_gpu_presented || cpu_baseline_presented
}

/// One head's scanout progress, as the CPU input baseline reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CpuScanoutHeadEvidence {
    submissions: usize,
    presented_submissions: usize,
}

/// Answers whether what the CPU scene composed is what a camera would see.
///
/// Arming input while a frame is still in flight would let the post-input
/// correlation latch onto a page flip that was carrying pre-input content, so
/// this asks two things: the focused surface's content already crossed a flip
/// on every head it occupies, which is the startup barrier the caller passes
/// in, and that no submission is outstanding behind it on any head.
///
/// It compares submission counts, never checksums. A head records the logical
/// scene it presented; the CPU report carries a composition evidence hash over
/// output size and composited elements. Those are different quantities that
/// never compare equal, and requiring them to match wedged every CPU-backed
/// input proof from the day per-head composition replaced the CPU queue path.
fn current_cpu_frame_is_presented(
    scene_nonzero_pixel_bytes: Option<usize>,
    focused_content_reached_scanout: bool,
    native_heads: Option<impl IntoIterator<Item = CpuScanoutHeadEvidence>>,
) -> bool {
    scene_nonzero_pixel_bytes.is_some_and(|nonzero_pixel_bytes| nonzero_pixel_bytes > 0)
        && focused_content_reached_scanout
        && native_heads.is_none_or(|heads| {
            let mut heads = heads.into_iter().peekable();
            heads.peek().is_some()
                && heads.all(|head| head.presented_submissions >= head.submissions)
        })
}

fn stable_gpu_frame_proves_post_input_pixels(
    input_delivery_complete: bool,
    input_surface: Option<SurfaceId>,
    retired_surface: SurfaceId,
    stable: bool,
) -> bool {
    input_delivery_complete && stable && input_surface == Some(retired_surface)
}

/// Whether this page flip is the one that showed the physical input.
///
/// The submission counter alone cannot answer that. A render already under
/// way when the input arrives carries a composition built before it, and
/// completes into a submission that outranks the baseline while showing none
/// of the input -- which is what every session of the first full physical run
/// actually measured. The presented composition must therefore be newer than
/// the newest one the head held when the input was routed.
fn physical_input_page_flip_correlates(
    input_delivery_complete: bool,
    input_pixel_change: bool,
    ingress_ust_usec: u64,
    baseline_submission: usize,
    presented_submission: usize,
    baseline_frame: u64,
    presented_frame: u64,
    submission_ust_usec: u64,
    page_flip_ust_usec: u64,
) -> bool {
    input_delivery_complete
        && input_pixel_change
        && presented_submission > baseline_submission
        && presented_frame > baseline_frame
        && submission_ust_usec >= ingress_ust_usec
        && page_flip_ust_usec >= submission_ust_usec
}

/// The newest composition a head holds anywhere in its pipeline.
///
/// A head may be rendering one frame, holding another submitted, and
/// displaying a third. Input must be shown by something newer than all of
/// them, so the baseline is the maximum rather than any single stage.
pub(super) fn newest_head_composition_frame(
    contents: impl IntoIterator<Item = Option<u64>>,
) -> u64 {
    contents.into_iter().flatten().max().unwrap_or(0)
}

fn software_batch_may_coalesce(batch: &XAuthorityObservedTransactionBatch) -> bool {
    batch.removed_surfaces.is_empty()
        && batch.dma_buf_registrations.is_empty()
        && batch.fence_registrations.is_empty()
        && batch.present_submissions.is_empty()
        && batch.software_present_submissions.is_empty()
        && batch.released_dma_bufs.is_empty()
        && batch.released_fences.is_empty()
        && (!batch.transactions.is_empty() || !batch.cpu_buffer_updates.is_empty())
}

/// Queued batches one authority drain may buffer.
const AUTHORITY_DRAIN_CAPACITY: usize = 64;
/// Batches one production cycle may commit together, counting the head. The
/// bound keeps a single owner turn short enough that native frame service is
/// not starved; the producer channel holds 256, so a burst still drains in a
/// handful of cycles rather than one per frame.
const AUTHORITY_MERGE_RUN_LIMIT: usize = 64;

/// Committed transactions a merged run may add beyond its head.
///
/// Every commit contributes one runtime observation, and the session runtime
/// refuses a tick whose batch exceeds `MAX_SESSION_RUNTIME_OBSERVATION_BATCH`.
/// The real budget is therefore transactions, not batches: one batch can carry
/// several. The reserve leaves room for the fixed per-tick observations — tick
/// start and end, event polling, frame scheduling, scanout state — that share
/// the same batch.
const AUTHORITY_MERGE_OBSERVATION_RESERVE: usize = 16;
const AUTHORITY_MERGE_TRANSACTION_LIMIT: usize =
    sophia_runtime::MAX_SESSION_RUNTIME_OBSERVATION_BATCH - AUTHORITY_MERGE_OBSERVATION_RESERVE;

/// Whether a batch carries only client content: pixels and geometry for
/// surfaces the layout already knows, with no lifecycle, reservation, or
/// resource edge that a later batch in the same cycle could reorder.
fn authority_batch_is_pure_content(batch: &XAuthorityObservedTransactionBatch) -> bool {
    software_batch_may_coalesce(batch)
        && batch.surface_presentations.is_empty()
        && batch.presentation_intents.is_empty()
        && batch.surface_output_reservations.is_empty()
}

/// Whether a batch may follow another inside one merged commit run.
///
/// A raster response is evaluated against the requirement state of its own
/// cycle, so it may open a run but never join one: merging accepts every
/// response before any commit, and a response admitted against a state two
/// batches stale is exactly the confusion this boundary exists to prevent.
fn authority_batch_may_follow_merge(batch: &XAuthorityObservedTransactionBatch) -> bool {
    authority_batch_is_pure_content(batch) && batch.raster_responses.is_empty()
}

/// Whether a batch carries work a production cycle must observe.
fn authority_batch_has_engine_work(batch: &XAuthorityObservedTransactionBatch) -> bool {
    !batch.transactions.is_empty()
        || !batch.removed_surfaces.is_empty()
        || !batch.cpu_buffer_updates.is_empty()
        || !batch.dma_buf_registrations.is_empty()
        || !batch.fence_registrations.is_empty()
        || !batch.present_submissions.is_empty()
        || !batch.software_present_submissions.is_empty()
        || !batch.released_dma_bufs.is_empty()
        || !batch.released_fences.is_empty()
        || !batch.surface_presentations.is_empty()
        || !batch.presentation_intents.is_empty()
        || !batch.surface_output_reservations.is_empty()
}

/// How many queued batches this production cycle commits together, counting
/// `head`.
///
/// One reproduces the historical cadence exactly. A longer run is admitted
/// only while the admission pipeline is quiescent — projection drains the
/// released-group queue, so a release landing between two projections in one
/// cycle would emit a quarantined group twice — and only across batches that
/// carry nothing but client content. A repeated transaction identity also ends
/// the run: production groups are bucketed per projection call, so a repeat
/// would split one atomic group across the concatenation.
fn authority_merge_run_len<'a>(
    head: &XAuthorityObservedTransactionBatch,
    queued: impl IntoIterator<Item = &'a XAuthorityObservedTransactionBatch>,
    admission_quiescent: bool,
    limit: usize,
) -> usize {
    if limit <= 1 || !admission_quiescent || !authority_batch_is_pure_content(head) {
        return 1;
    }
    let mut identities = BTreeSet::new();
    authority_batch_transaction_identities(head, &mut identities);
    let mut len = 1;
    // The head commits regardless of its size, exactly as it did before
    // merging; only the batches joining it are held to the observation budget.
    let mut transactions = head.transactions.len();
    for batch in queued {
        if len >= limit || !authority_batch_may_follow_merge(batch) {
            break;
        }
        let joined = transactions.saturating_add(batch.transactions.len());
        if joined > AUTHORITY_MERGE_TRANSACTION_LIMIT {
            break;
        }
        let mut candidate = identities.clone();
        let before = candidate.len();
        authority_batch_transaction_identities(batch, &mut candidate);
        if candidate.len() != before.saturating_add(batch_transaction_identity_count(batch)) {
            break;
        }
        identities = candidate;
        transactions = joined;
        len = len.saturating_add(1);
    }
    len
}

fn authority_batch_transaction_identities(
    batch: &XAuthorityObservedTransactionBatch,
    identities: &mut BTreeSet<TransactionId>,
) {
    identities.insert(batch.transaction);
    for transaction in &batch.transactions {
        identities.insert(transaction.transaction);
    }
}

fn batch_transaction_identity_count(batch: &XAuthorityObservedTransactionBatch) -> usize {
    let mut identities = BTreeSet::new();
    authority_batch_transaction_identities(batch, &mut identities);
    identities.len()
}

struct SessionActionExecutionContext<'a> {
    config: &'a PersistentXtermSessionConfig,
    xauthority: &'a std::path::Path,
    children: &'a mut Vec<ManagedSessionChild>,
    launches: &'a mut SessionLaunchQueue,
    launch_admission_started_at: &'a mut Option<Instant>,
    admission_pipeline_idle: bool,
    stable_admission_surface: Option<SurfaceId>,
    withdrawn_admissions: &'a [SurfaceId],
    layout: &'a PersistentLiveLayout,
    focus: &'a InputFocusState,
    seat: SeatId,
    session_controls: &'a mut SessionControlQueue,
}

/// What committed session actions asked the session to do next.
///
/// Three requests that outlive the action loop. Logout ends the session,
/// reload puts a changed profile into effect, and restart replaces the policy
/// client; each is carried out by the owner loop, which owns the machinery.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct CommittedSessionActionRequests {
    pub logout: bool,
    pub reload_profile: bool,
    pub restart_wm: bool,
    pub open_launcher: bool,
}

fn execute_committed_session_actions(
    context: SessionActionExecutionContext<'_>,
    actions: &mut VecDeque<(TransactionId, WmSessionAction, Option<SurfaceId>)>,
) -> Result<CommittedSessionActionRequests, Box<dyn std::error::Error>> {
    let SessionActionExecutionContext {
        config,
        xauthority,
        children,
        launches,
        launch_admission_started_at,
        admission_pipeline_idle,
        stable_admission_surface,
        withdrawn_admissions,
        layout,
        focus,
        seat,
        session_controls,
    } = context;
    if let Some(admission) =
        launches.complete_if_stable(admission_pipeline_idle, stable_admission_surface)
    {
        *launch_admission_started_at = None;
        crate::session_println!(
            "sophia_session_app schema=2 status=admitted source=action transaction={} surface={}",
            admission.intent.transaction.raw(),
            stable_admission_surface
                .expect("settled launch admission requires a stable surface")
                .index(),
        );
    } else if let Some(admission) = launches.withdraw_current(withdrawn_admissions) {
        // The surface this launch was waiting on is gone, so the remaining
        // budget would be spent waiting for something that cannot arrive --
        // and every later press queues behind it in silence.
        *launch_admission_started_at = None;
        crate::session_eprintln!(
            "sophia_session_app schema=2 status=failed source=action transaction={} application={} reason=surface_withdrawn",
            admission.intent.transaction.raw(),
            admission.intent.application.raw(),
        );
    } else if launch_admission_started_at
        .is_some_and(|started| {
            started.elapsed() >= Duration::from_millis(SESSION_APP_ADMISSION_TIMEOUT_MSEC)
        })
        && let Some(admission) = launches.timeout_current()
    {
        *launch_admission_started_at = None;
        crate::session_eprintln!(
            "sophia_session_app schema=2 status=failed source=action transaction={} application={} reason=admission_timeout",
            admission.intent.transaction.raw(),
            admission.intent.application.raw(),
        );
    }

    let mut requests = CommittedSessionActionRequests::default();
    while let Some((transaction, action, target)) = actions.pop_front() {
        if let WmSessionAction::LaunchApplication { application } = action {
            if application == LAUNCHER_APPLICATION_ID && config.application_catalog.is_some() {
                requests.open_launcher = true;
                continue;
            }
            let command = if config.normal_session {
                config.application_for_action(action).cloned()
            } else {
                let program = match application {
                    TERMINAL_APPLICATION_ID => Some(config.terminal.as_str()),
                    LAUNCHER_APPLICATION_ID => config.session_launcher.as_deref(),
                    BROWSER_APPLICATION_ID => config.session_browser.as_deref(),
                    _ => None,
                };
                program.map(|program| crate::session_actions::SessionLaunchCommand {
                    id: format!("role-{}", application.raw()),
                    executable: program.into(),
                    arguments: Vec::new(),
                    placement_classification: None,
                })
            };
            let Some(command) = command else {
                crate::session_eprintln!(
                    "sophia_session_app schema=2 status=rejected source=action transaction={} reason=unavailable_command",
                    transaction.raw(),
                );
                continue;
            };
            match launches.enqueue_command(
                SessionLaunchIntent {
                    transaction,
                    application,
                    placement_classification: command.placement_classification,
                },
                std::sync::Arc::new(command),
                children.len(),
            ) {
                SessionLaunchQueueOutcome::Queued { depth } => crate::session_println!(
                    "sophia_session_app schema=2 status=queued source=action transaction={} application={} depth={depth}",
                    transaction.raw(),
                    application.raw(),
                ),
                SessionLaunchQueueOutcome::RejectedCapacity => crate::session_eprintln!(
                    "sophia_session_app schema=2 status=rejected source=action transaction={} reason=capacity",
                    transaction.raw(),
                ),
            }
            crate::session_println!(
                "sophia_live_wm schema=1 status=session_action_committed transaction={} action={}",
                transaction.raw(),
                session_action_evidence_name(action)
            );
            continue;
        }
        match action {
            WmSessionAction::LaunchApplication { .. } => unreachable!(),
            WmSessionAction::CloseFocused => {
                let surface = target
                    .or_else(|| focus.focused_surface(seat))
                    .ok_or("WM close action has no focused surface")?;
                let client = layout
                    .client_routes
                    .client_for_surface(surface)
                    .ok_or("WM close action has no X11 client route")?;
                crate::session_println!(
                    "sophia_live_wm schema=1 status=close_routed transaction={} target=surface surface={surface:?} client={client:?}",
                    transaction.raw()
                );
                session_controls.enqueue(XAuthorityClientControlCommand {
                    client,
                    command: XAuthorityControlCommand::CloseSurface {
                        transaction,
                        surface,
                    },
                }, Instant::now()).map_err(|error| {
                    format!("failed to queue polite close control: {error:?}")
                })?;
            }
            WmSessionAction::ReloadProfile => {
                // Re-reading the profile is the owner loop's to do: it holds
                // the staging directory, the authority slots and the policy
                // launch. Recording the request here keeps this loop what it
                // is, which is the place committed actions are acknowledged.
                requests.reload_profile = true;
            }
            WmSessionAction::RestartWm => {
                requests.restart_wm = true;
            }
            WmSessionAction::Logout => {
                requests.logout = true;
                let cancelled = launches.cancel_pending();
                let admission_cancelled = usize::from(launches.fail_current().is_some());
                *launch_admission_started_at = None;
                if cancelled != 0 {
                    crate::session_println!(
                        "sophia_session_app schema=2 status=cancelled source=logout pending={cancelled}"
                    );
                }
                if admission_cancelled != 0 {
                    crate::session_println!(
                        "sophia_session_app schema=2 status=cancelled source=logout admission={admission_cancelled}"
                    );
                }
            }
        }
        crate::session_println!(
            "sophia_live_wm schema=1 status=session_action_committed transaction={} action={}",
            transaction.raw(),
            session_action_evidence_name(action)
        );
    }

    if requests.logout {
        return Ok(requests);
    }
    let Some(intent) = launches.begin_next(admission_pipeline_idle) else {
        return Ok(requests);
    };
    if children.len() >= crate::session_actions::SESSION_ACTION_APPLICATION_CAPACITY {
        let _ = launches.fail_current();
        crate::session_eprintln!(
            "sophia_session_app schema=2 status=rejected source=action transaction={} reason=capacity",
            intent.transaction.raw(),
        );
        return Ok(requests);
    }
    if launches.dispatch_catalog(intent.transaction) {
        *launch_admission_started_at=Some(Instant::now());
        return Ok(requests);
    }
    let Some(command) = launches.take_admitted_command(intent.transaction) else {
        let _ = launches.fail_current();
        *launch_admission_started_at = None;
        crate::session_eprintln!(
            "sophia_session_app schema=2 status=rejected source=action transaction={} reason=missing_retained_command",
            intent.transaction.raw(),
        );
        return Ok(requests);
    };
    let spawned = if config.normal_session {
        PersistentXtermSessionConfig::spawn_session_application(
            &command,
            &config.display,
            xauthority,
            config.control_socket.as_deref(),
            config.inspection_socket.as_deref(),
            crate::diagnostics::application::LaunchContext {
                source: crate::diagnostics::application::LaunchSource::Shortcut,
                transaction: Some(intent.transaction.raw()),
            },
        )
        .map(|child| (Some(command.id.clone()), child))
    } else {
        spawn_approved_application(
            &command.executable.to_string_lossy(),
            &config.display,
            xauthority,
            config.control_socket.as_deref(),
            config.inspection_socket.as_deref(),
        )
        .map(|child| (None, child))
    };
    match spawned {
        Ok((id, child)) => {
            let evidence_id = id.as_deref().unwrap_or("untracked");
            crate::session_println!(
                "sophia_session_app schema=2 status=started id={evidence_id} source=action transaction={}",
                intent.transaction.raw(),
            );
            // Retain the schema-1 compatibility record consumed by existing
            // physical-session verifiers.
            crate::session_println!(
                "sophia_session_app schema=1 status=started id={evidence_id} source=action"
            );
            children.push(ManagedSessionChild::for_launch(
                id,
                intent.transaction,
                child,
            ));
            let observes_proof_surface = config.launch_surface_proof_requested();
            *launch_admission_started_at = if !observes_proof_surface
                && launches.complete_spawn(intent.transaction).is_some()
            {
                None
            } else {
                Some(Instant::now())
            };
        }
        Err(error) => {
            let _ = launches.fail_current();
            crate::session_eprintln!(
                "sophia_session_app schema=2 status=failed source=action transaction={} reason=spawn error={error}",
                intent.transaction.raw(),
            );
        }
    }
    Ok(requests)
}
