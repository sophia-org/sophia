struct SessionLoopChannels<'a> {
    authority: &'a Receiver<XAuthorityObservedTransactionBatch>,
    input: &'a XAuthorityRoutedInputSender,
    control: &'a XServerFrontendControlRouter,
    raster: &'a sophia_x_authority::XServerFrontendRasterRouter,
    control_acknowledgements: &'a Receiver<XAuthorityClientControlAck>,
    input_deliveries: &'a Receiver<XAuthorityClientInputDelivery>,
    route_lease_updates: &'a Receiver<XAuthorityRouteLeaseUpdate>,
    route_lease_releases: &'a SyncSender<XAuthorityRouteLeaseRelease>,
    explicit_pointer_grabs: &'a sophia_x_authority::XAuthorityExplicitPointerGrabOwner,
    frontend_service: &'a SyncSender<XServerFrontendServiceCommand>,
    metadata_candidates: &'a Receiver<sophia_x_authority::XAuthorityClientMetadataCandidate>,
}

struct SessionLoopResources<'a> {
    launch_origins: &'a Arc<Mutex<crate::launch_origin::LaunchOriginRegistry>>,
    child: Option<&'a mut Child>,
    secondary_children: &'a mut Vec<ManagedSessionChild>,
    physical_input: &'a mut Option<SessionPhysicalInput>,
    native_scanout: &'a mut Option<LiveProductionNativeScanout>,
    native_retirement: &'a mut NativeRetirement,
    render_owners: &'a mut LoopRenderOwners,
    suspended_renderer_images: &'a mut Option<sophia_backend_live::LiveProductionRendererImageHandoff>,
    seat_controller: &'a mut Option<sophia_backend_live::LiveSeatController>,
    wm_session: &'a mut Option<LiveWmSession>,
    scripting: &'a mut LiveControlState,
    metadata_broker: &'a mut Option<LiveMetadataBroker>,
    metadata_shell: &'a mut Option<LiveMetadataShell>,
    /// Which connectors share one logical output, from the profile loaded at
    /// startup. Fixed for the session's life: a rescan that regrouped differently
    /// would change the desktop's identity behind policy's back.
    mirror_grouping: &'a sophia_backend_live::NativeMirrorGrouping,
    /// Neutral initial policy for heads reconstructed after VT/hotplug loss.
    /// Output-authority commits may replace it independently on live heads.
    initial_head_mapping: sophia_protocol::OutputHeadMapping,
}

#[derive(Default)]
struct LoopRenderOwners {
    runtime: Option<LiveProductionVisualRuntime>,
    scene: Option<LiveProductionCpuScene>,
    seat_active: bool,
}

struct SessionLoopStartup<'a> {
    client_render_devices: Option<render_devices::LiveRenderDeviceCoordinator>,
    output_topology_monitor: Option<sophia_backend_live::LiveDrmTopologyMonitor>,
    xauthority: &'a std::path::Path,
    protocol_router: XServerFrontendProtocolRouter,
    input_proof_result: Option<&'a LiveInputProofResult>,
    client_stdout_capture: Option<&'a LiveClientStdoutCapture>,
    require_startup_focus: bool,
    initial_authority_batch: Option<XAuthorityObservedTransactionBatch>,
    output_notifications: usize,
}

fn authority_wait_timeout(
    physical_input_active: bool,
    cursor_update_pending: bool,
    control_pending: bool,
) -> Duration {
    Duration::from_millis(
        if physical_input_active || cursor_update_pending || control_pending {
            1
        } else {
            25
        },
    )
}

fn native_frame_service_requires_owner_progress(request: &OutputFrameServiceRequest) -> bool {
    // A waiting software present is owed work too. It stopped being visible in
    // the per-output flags once those became native-only, and without it here
    // the owner would drop to its idle pacing while a present still needed
    // lowering.
    request.presentation_queued
        || request.software_frame_waiting
        || request.outputs.iter().any(|output| {
            output.pending_frame || output.native_phase != OutputNativeFramePhase::Idle
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProductionCycleNativeOwnerPolicy {
    Unavailable,
    Available,
}

const fn production_cycle_native_owner_policy(
    native_enabled: bool,
    cpu_frame_deferred: bool,
) -> ProductionCycleNativeOwnerPolicy {
    // Composition coalescing is advisory. The visual runtime still needs the
    // native owner to preserve retained GPU content or repaint changed chrome.
    match (native_enabled, cpu_frame_deferred) {
        (false, false) | (false, true) => ProductionCycleNativeOwnerPolicy::Unavailable,
        (true, false) | (true, true) => ProductionCycleNativeOwnerPolicy::Available,
    }
}

fn native_frame_service_should_preempt_authority(
    request: &OutputFrameServiceRequest,
    preempted_previous_cycle: bool,
    control_pending: bool,
    control_priority_cycles: u8,
    service_due: bool,
) -> bool {
    // Controls get several owner turns first, but no control class may block
    // renderer polling indefinitely. An armed service deadline also covers
    // work whose instantaneous output hint has gone idle, so worker watchdogs
    // keep making progress through resize-recovery traffic. The following
    // owner turn returns to authority traffic.
    const CONTROL_PRIORITY_CYCLES: u8 = 4;

    (!control_pending || control_priority_cycles >= CONTROL_PRIORITY_CYCLES)
        && !preempted_previous_cycle
        && (service_due || native_frame_service_requires_owner_progress(request))
}

fn synchronize_runtime_surface_chrome_style(
    runtime: &mut LiveProductionVisualRuntime,
    style: sophia_engine::SurfaceChromeStyle,
) -> bool {
    runtime.set_surface_chrome_style(style)
}

fn capture_renderer_image_handoff(
    runtime: &LiveProductionVisualRuntime,
    native_scanout: &mut LiveProductionNativeScanout,
) -> Result<sophia_backend_live::LiveProductionRendererImageHandoff, Box<dyn std::error::Error>> {
    let retained = runtime.retained_renderer_image_ids();
    native_scanout.export_renderer_image_handoff(&retained).inspect_err(|error| {
        crate::session_eprintln!(
            "sophia_live_renderer_handoff schema=1 status=failed phase=export_images failure_code={} retained_count={}",
            crate::diagnostics::failure_code(error.as_ref()), retained.len(),
        );
    })
}

fn resume_native_scanout_from_scene(
    runtime: &mut LiveProductionVisualRuntime,
    native: &mut LiveProductionNativeScanout,
    outputs: &[sophia_engine::HeadlessOutput],
    scene: &mut LiveProductionCpuScene,
    handoff: &mut Option<sophia_backend_live::LiveProductionRendererImageHandoff>,
) -> Result<usize, Box<dyn std::error::Error>> {
    native_owner_retirement::restore_retained_handoff(handoff, |handoff| {
        runtime.resume_native_scanout(native, outputs, scene, handoff)
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiveOutputTopologyExecutionPhase {
    WaitingForQuiescence,
    Preparing,
    Applying,
    AwaitingFirstPresentation,
    Reconciling,
    RollingBack,
}

fn owner_loop_shell_presentation_available(
    seat_active: bool,
    native_attached: bool,
    topology_phase: Option<LiveOutputTopologyExecutionPhase>,
    startup_pending: bool,
) -> bool {
    metadata_shell::shell_presentation_available(
        seat_active,
        native_attached,
        topology_phase.is_none() && !startup_pending,
    )
}

#[derive(Clone, Debug)]
struct LiveOutputTopologyExecution {
    effect: crate::live_output_authority::LiveOutputAuthorityEffect,
    phase: LiveOutputTopologyExecutionPhase,
    preparation_deadline: Instant,
    first_frames:
        BTreeMap<sophia_protocol::OutputId, sophia_backend_live::LiveProductionNativeFrameId>,
    frontend_candidate_published: bool,
    /// Whether this candidate has already forced presentation quiescence. The
    /// escalation is one-shot: a second expiry rejects rather than skipping
    /// more client work.
    quiescence_escalated: bool,
    last_preparation_progress:
        Option<sophia_backend_live::LiveProductionNativeTopologyPreparationReport>,
}

const OUTPUT_TOPOLOGY_QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a committed topology waits for its layout to reach a screen before
/// input is restored anyway.
///
/// Generous relative to a client redraw and short relative to a desktop that
/// feels dead: pointer motion is dropped and every non-shortcut key discarded
/// for the whole wait, so an unbounded one is worse than presenting a frame
/// late.
const OUTPUT_TOPOLOGY_PRESENTATION_TIMEOUT: Duration = Duration::from_secs(2);

fn begin_output_topology_first_presentation_rollback<NativeRollback, PolicyRollback>(
    phase: &mut LiveOutputTopologyExecutionPhase,
    transaction: sophia_protocol::TransactionId,
    failure: &str,
    request_native_rollback: NativeRollback,
    reject_policy_candidate: PolicyRollback,
) -> Result<bool, Box<dyn std::error::Error>>
where
    NativeRollback: FnOnce(String) -> Result<(), Box<dyn std::error::Error>>,
    PolicyRollback:
        FnOnce(sophia_protocol::TransactionId) -> Result<(), Box<dyn std::error::Error>>,
{
    if *phase != LiveOutputTopologyExecutionPhase::AwaitingFirstPresentation {
        return Ok(false);
    }
    request_native_rollback(format!("first topology presentation failed: {failure}"))?;
    // Once the physical owner accepted reverse apply, retain that fact even if
    // policy notification fails. Session completion can then finish rollback
    // without mistaking the candidate for a merely queued transaction.
    *phase = LiveOutputTopologyExecutionPhase::RollingBack;
    reject_policy_candidate(transaction)?;
    Ok(true)
}

const fn hardware_output_snapshot_is_stale(snapshot_epoch: u64, current_epoch: u64) -> bool {
    snapshot_epoch <= current_epoch
}

/// The session loop, with the protocol-error tally reported whatever the outcome.
///
/// The tally used to be a local of the loop body, and that body has several dozen
/// early exits between here and its close in `completion.rs`. Every one of them
/// dropped the count on the floor: a run that refused seven `BuffersFromPixmap`
/// requests and then failed its input sequence reported the timeout and said
/// nothing about the seven. Refused opcodes matter most on precisely the runs
/// that never reach a clean completion, so the tally is owned out here, where no
/// early return can skip it.
///
/// Deliberately not a `Drop` impl: emitting from a destructor would hide the
/// control flow this exists to make visible.
fn run_session_loop(
    config: &mut PersistentXtermSessionConfig,
    channels: SessionLoopChannels<'_>,
    resources: SessionLoopResources<'_>,
    startup: SessionLoopStartup<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut protocol_error_tally = SessionProtocolErrorTally::default();
    let mut failure_phase = crate::diagnostics::SessionFailurePhase::Startup;
    let result = run_session_loop_inner(
        config,
        channels,
        resources,
        startup,
        &mut protocol_error_tally,
        &mut failure_phase,
    );
    protocol_error_tally.report();
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            crate::session_eprintln!("{}", crate::diagnostics::session_failure_record(
                failure_phase, error.as_ref(),
            ));
            Err(session_failure_with_refused_requests(
                &error.to_string(), &protocol_error_tally,
            ).into())
        }
    }
}

fn run_session_loop_inner(
    config: &mut PersistentXtermSessionConfig,
    channels: SessionLoopChannels<'_>,
    resources: SessionLoopResources<'_>,
    startup: SessionLoopStartup<'_>,
    protocol_error_tally: &mut SessionProtocolErrorTally,
    failure_phase: &mut crate::diagnostics::SessionFailurePhase,
) -> Result<(), Box<dyn std::error::Error>> {
    let SessionLoopChannels {
        authority: authority_receiver,
        input: input_sender,
        control: control_sender,
        raster: raster_sender,
        control_acknowledgements: control_ack_receiver,
        input_deliveries: input_delivery_receiver,
        route_lease_updates: route_lease_update_receiver,
        route_lease_releases: route_lease_release_sender,
        explicit_pointer_grabs,
        frontend_service: frontend_service_sender,
        metadata_candidates: metadata_candidate_receiver,
    } = channels;
    let SessionLoopResources {
        launch_origins,
        mut child,
        secondary_children,
        physical_input,
        native_scanout,
        native_retirement,
        render_owners,
        suspended_renderer_images,
        seat_controller,
        wm_session,
        scripting,
        metadata_broker,
        metadata_shell,
        mirror_grouping,
        initial_head_mapping,
    } = resources;
    let SessionLoopStartup {
        mut output_topology_monitor,
        mut client_render_devices,
        xauthority,
        protocol_router,
        input_proof_result,
        client_stdout_capture,
        require_startup_focus,
        mut initial_authority_batch,
        output_notifications,
    } = startup;
    let started = Instant::now();
    let mut render_inventory_service_at = started;
    let config_watcher = config
        .core_config_source
        .path
        .as_deref()
        .map(sophia_config::ConfigWatcher::spawn)
        .transpose()?;
    let mut config_reload_pending = false;
    let deadline = config.max_runtime.map(|duration| started + duration);
    let initialize_empty_runtime =
        config.normal_session;
    let mut outputs = native_scanout
        .as_ref()
        .map(LiveProductionNativeScanout::outputs)
        .unwrap_or_else(|| vec![sophia_engine::HeadlessOutput::deterministic()]);
    let mut output = outputs[0];
    // Headless has no connectors, so every output is its own single head. That is
    // the same shape hardware reports for an unmirrored desktop.
    let heads = native_scanout
        .as_ref()
        .map(LiveProductionNativeScanout::head_fingerprint)
        .unwrap_or_else(|| outputs.iter().map(|output| (output.id, 1)).collect());
    let initial_output_publication_generation =
        1u64.saturating_add(u64::from(config.inject_output_size.is_some()));
    let mut output_topology_owner = LiveOutputTopologyOwner::new_at_generation(
        outputs.clone(),
        heads,
        initial_output_publication_generation,
    )?;
    let mut physical_output_topology_replaced = false;
    let mut output_topology_retry_at: Option<Instant> = None;
    let mut deferred_output_topology_notice: Option<
        sophia_backend_live::LiveDrmTopologyRescanNotice,
    > = None;
    let mut output_topology_policy_commit_baseline = 0u64;
    let scene = render_owners.scene.insert(LiveProductionCpuScene::new(output.size));
    scene.set_cursor_asset(config.cursor_resolution.asset.clone());
    let cursor_fallback = config
        .cursor_resolution
        .fallback_reason
        .as_deref()
        .unwrap_or("none");
    crate::session_println!(
        "sophia_live_cursor_asset schema=1 requested_theme={} effective_theme={} requested_size={} effective_size={} requested_shape={} effective_shape={} width={} height={} hotspot_x={} hotspot_y={} digest={} animation_frames_ignored={} fallback={:?}",
        config.cursor_resolution.requested_theme,
        config.cursor_resolution.effective_theme,
        config.cursor_resolution.requested_size,
        config.cursor_resolution.effective_nominal_size,
        config.core_config_state.active().cursor.shape,
        config.cursor_resolution.shape.name(),
        config.cursor_resolution.asset.width(),
        config.cursor_resolution.asset.height(),
        config.cursor_resolution.asset.hotspot().0,
        config.cursor_resolution.asset.hotspot().1,
        config.cursor_resolution.asset.digest(),
        config.cursor_resolution.ignored_animation_frames,
        cursor_fallback,
    );
    if initialize_empty_runtime {
        scene.compose(&[], None, None)?;
    }
    let mut layout = PersistentLiveLayout::new(
        LivePolicyMapMode::from_external_wm(wm_session.is_some()),
        output.size,
    );
    let mut pending_wm_update = None;
    let mut active_output_topology_preparation: Option<LiveOutputTopologyExecution> = None;
    let mut pending_hardware_output_publication: Option<(
        sophia_protocol::OutputAuthoritySnapshot,
        Vec<sophia_backend_live::LibdrmNativeOutputCapability>,
    )> = None;
    // Whether the parked hardware snapshot's topology has presented, which is
    // the first of the two conditions its publication waits on.
    let mut hardware_output_publication_presented = false;
    // When the post-commit presentation wait began. Nothing forces the flip it
    // waits for, so the wait is bounded rather than open-ended; input is held
    // at shortcuts-only for its whole duration.
    let mut topology_presentation_deadline: Option<Instant> = None;
    let mut floating_pointer_gesture = FloatingPointerGestureState::default();
    let mut staged_cpu_buffer_handles = Vec::with_capacity(16);
    let mut layout_progress_deferred_reported = false;
    let mut committed_session_actions = VecDeque::new();
    let mut session_launches = SessionLaunchQueue::default();
    let mut launch_admission_started_at: Option<Instant> = None;
    let mut present_observer = XPresentSessionObserver::new(protocol_router);
    let mut present_feedback = Vec::new();
    let mut visual_progress = visual_progress::VisualProgress::new();
    let initial_border_style = wm_session
        .as_ref()
        .and_then(|wm| wm.surface_chrome_style())
        .unwrap_or(config.surface_chrome_style);
    let window_transitions_enabled = !std::env::var("SOPHIA_ENABLE_WINDOW_TRANSITIONS")
        .is_ok_and(|value| value == "0");
    let runtime = &mut render_owners.runtime;
    if initialize_empty_runtime {
        let mut initialized = LiveProductionVisualRuntime::new(&outputs, native_scanout.as_mut())?
        .with_m4_proof_controls(
            config.m4_first_acquire_delay,
            config.m4_reject_first_present,
            config.m4_diagnose_first_mixed_export,
        )
        .with_surface_chrome_style(initial_border_style);
        initialized.set_transitions_enabled(window_transitions_enabled);
        initialized.set_indicator_publication(
            wm_session
                .as_ref()
                .and_then(LiveWmSession::indicator_publication),
        );
        *runtime = Some(initialized);
        if let Some(native) = native_scanout.as_mut() {
            let initialized = runtime.as_mut().expect("runtime just retained");
            let _ = initialized.run_cpu_repaint(
                scene,
                None,
                None,
                LiveProductionCursorPresentation::HardwarePlane,
                &outputs,
                native,
            )?;
        }
    }
    let mut window_allocation_publisher = window_allocation::LiveWindowAllocationPublisher::default();
    let mut last_authority_update = started;
    let mut injection_checksum = None;
    let mut physical_input_ready_at: Option<Instant> = None;
    let mut physical_text_proof = config
        .expect_physical_text
        .as_deref()
        .map(|text| {
            if config.application_proof_requested() {
                PhysicalTextProof::new_without_submit(text)
            } else {
                PhysicalTextProof::new(text)
            }
        })
        .transpose()?;
    let mut physical_sequence_completed_at: Option<Instant> = None;
    let mut physical_input_completion_reported = false;
    let mut input_pixel_change = false;
    let mut input_surface = None;
    let mut input_surface_generation = None;
    let mut input_surface_pixel_change = false;
    let mut input_proof_started_at = None;
    let mut input_change_submission_baseline = None;
    let mut input_change_frame_baseline = None;
    let mut input_presented_latency = None;
    let mut input_raw_ingress_msec: Option<u64> = None;
    // Repeatable sampling beside the one-shot proof: the proof answers whether
    // input reached a page flip, this answers how long that took, many times.
    let mut input_latency_samples = crate::input_latency_samples::InputLatencySamples::new();
    let mut deferred_physical_key_timings = BTreeMap::new();
    let mut routed_input_saturation = RoutedInputIngressSaturation::default();
    let mut routed_input_saturation_ledger = sophia_protocol::CapacityReportLedger::default();
    let mut input_queue_dwell: Option<Duration> = None;
    let mut input_presented_ust_usec: Option<u64> = None;
    let mut input_submit_to_page_flip: Option<Duration> = None;
    let mut pointer_checksum = None;
    let mut pointer_cursor_checksum = None;
    let mut pointer_phase_started_at = None;
    let mut cursor_visible_reported = false;
    let mut pointer_pixel_change = false;
    let mut metrics = SessionLoopMetrics::new(initialize_empty_runtime);
    let mut input_batch_baseline = None;
    let mut input_cpu_update_baseline = None;
    let mut focus = InputFocusState::new();
    let mut modifiers = config.keyboard_mapper();
    let key_repeat_map = XkbKeymapSnapshot::new(&config.xkb_config)?;
    let key_repeat_config = KeyRepeatConfig::new(
        config.key_repeat_config.delay_msec,
        config.key_repeat_config.interval_msec,
    )
    .ok_or("X11 key repeat controls must be nonzero")?;
    let mut key_repeat = KeyRepeatState::new(key_repeat_config);
    let mut client_keys = SessionClientKeyState::default();
    let mut client_key_scratch = Vec::with_capacity(SESSION_CLIENT_PRESSED_KEY_CAPACITY);
    let mut client_key_deliveries = Vec::with_capacity(SESSION_CLIENT_PRESSED_KEY_CAPACITY);
    let mut client_key_release_barrier = BTreeSet::new();
    let mut runtime_deadline_key_drain = RuntimeDeadlineKeyDrain::default();
    let mut emergency_chord = EmergencyChordState::armed();
    let mut virtual_terminal_chord = VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    let mut keyboard_focus_handoff = KeyboardFocusHandoffState::default();
    let mut pointer_focus_handoff = PointerFocusHandoffState::default();
    let mut physical_policy_inputs = PhysicalPolicyInputQueue::default();
    let mut application_route_leases = ApplicationRouteLeaseState::default();
    let mut explicit_grab_queue = ExplicitPointerGrabQueue::default();
    let mut pending_lease_input = PendingLeaseInput::default();
    let mut shell_proof_visible_presentations = 0_u32;
    let mut shell_proof_restart_triggered = false;
    // Carries the shell's committed work-area claim from the shell phase to
    // the WM phase within one tick.
    let mut shell_work_area_bands: Option<Vec<sophia_protocol::OutputReservation>> = None;
    let mut shell_proof_waiting_for_inert_click = false;
    let mut chrome_captures = sophia_engine::ChromeCaptureState::default();
    let mut reference_capture = sophia_engine::ReferenceSheetCapture::default();
    let mut launcher_capture=sophia_engine::LauncherCapture::default();
    let k=&config.xkb_config;
    let mut launcher_keyboard=sophia_engine::LauncherKeyboard::new(&k.rules,&k.model,&k.layout,&k.variant,&k.options,&std::env::var_os("LC_ALL").or_else(||std::env::var_os("LC_CTYPE")).or_else(||std::env::var_os("LANG")).unwrap_or_else(||"C".into()))?;
    let mut descriptor_captures = sophia_engine::PresentedChromeCaptureState::default();
    let mut content_captures = sophia_engine::ContentCaptureState::default();
    if native_scanout.is_some() {
        pointer.set_output_bounds(
            wm_output_bounds(&outputs)
                .into_iter()
                .map(|(_, bounds)| bounds)
                .collect(),
        );
        pointer.center_on_primary_output(output.size);
    }
    let seat = SeatId::from_raw(SESSION_SEAT_RAW);
    let mut focus_deadline_started_at = None;
    let mut focus_ready_reported = false;
    let mut applied_client_focus: Option<SurfaceId> = None;
    let mut session_controls = SessionControlQueue::default();
    let mut session_control_completions = Vec::with_capacity(SESSION_CONTROL_CAPACITY);
    let mut next_focus_control_transaction = 1_000_000u64;
    let resize_proof_targets = config.surface_resize_targets();
    let mut resize_proof: Option<(TransactionId, SurfaceId, Size)> = None;
    let mut resize_proof_completed = 0usize;
    let mut resize_proof_complete = false;
    let mut input_observations = InputObservationState::default();
    let mut input_delivery = InputDeliveryState {
        fail_on_client_error: !config.normal_session || config.input_proof_requested(),
        ..InputDeliveryState::default()
    };
    let mut logout_requested = false;
    // Set by a committed session action and serviced at the top of the next
    // pass, where the policy launch and the authority slots are reachable.
    let mut profile_reload_requested = false;
    let mut wm_restart_requested = false;
    let mut post_input_deadline: Option<Instant> = None;
    let mut application_surface_gone_at: Option<Instant> = None;
    let mut input_content_surface: Option<SurfaceId> = None;
    let mut startup_readiness = SessionStartupReadiness::default();
    let startup_proof_requested = config.startup_proof_requested();
    let mut startup_content_ready = false;
    let mut startup_ready_msec = None;
    let mut native_presentation_admitted = false;
    let mut input_text_match = false;
    let mut primary_child_exited = child.is_none();
    let mut primary_exit_status = None;
    let mut terminal_client_error: Option<(&'static str, String)> = None;
    let mut terminal_runtime_error: Option<String> = None;
    let mut terminal_client_intake_stopped = false;
    let mut terminal_client_cleanup_failures = Vec::new();
    let mut post_startup_exit_pointer_reported = false;
    let mut application_surface_missing_since: Option<Instant> = None;
    let mut client_stdout = Vec::new();
    let mut firefox_m8_proof = if config.firefox_m10_proof {
        FirefoxM8StageProof::promotion()
    } else {
        FirefoxM8StageProof::default()
    };
    let mut firefox_m10_kitty_proof = FirefoxM10KittyProof::default();
    let mut firefox_m10_selection_kitty_proof = FirefoxM10SelectionKittyProof::default();
    let mut firefox_m10_dialog_proof = FirefoxM10DialogProof::default();
    let mut firefox_m10_primary_proof = FirefoxM10PrimaryProof::default();
    let mut firefox_m10_rendering_page_ready = false;
    let mut firefox_m8_page_ready_reported = false;
    let mut firefox_m8_navigation_ready_reported = false;
    let mut firefox_m8_dialog_ready_reported = false;
    let mut selection_owner_changes = 0usize;
    let mut selection_conversions = 0usize;
    let mut first_protocol_error = None;
    let mut emergency_exit_requested = false;
    let mut cursor_updates = CursorUpdateState::new(pointer.position().is_some());
    // Shake-to-find. The detector holds no clock of its own, so the loop hands
    // it milliseconds from a fixed origin; only differences matter to it.
    let mut cursor_shake = sophia_engine::CursorShakeDetector::default();
    // Set when the hardware refuses the enlarged raster, which is a property
    // of the card rather than of the moment: a driver whose cursor plane is
    // 64x64 will refuse a 96 px cursor every time it is asked, and saying so
    // once is evidence where saying it per gesture is noise.
    let mut cursor_shake_refused = false;
    // The position the detector last saw. Motion is decided here rather than
    // from the cursor-update flag, because `observe_motion` treats every call
    // as movement and pushes the restore deadline out: fed a position that has
    // not changed, an enlarged cursor would never shrink again.
    let mut cursor_shake_seen: Option<(i32, i32)> = None;
    let cursor_shake_epoch = Instant::now();
    let startup_ready_deadline = config
        .startup_ready_timeout
        .map(|timeout| started + timeout);
    let mut startup_required_submissions: Option<
        BTreeMap<sophia_engine::RenderHeadId, StartupHeadRequirement>,
    > = None;
    let mut retired_present_surfaces = BTreeMap::new();
    let mut startup_surface_presentations = StartupSurfacePresentationEvidence::default();
    let mut startup_ready_reported = false;
    let mut cpu_visual_progress = CpuVisualProgress::default();
    if !startup_proof_requested {
        // Normal sessions account for CPU work from owner-loop admission.
        // Waiting for an application proof would also disable drain tracking.
        let (submissions, checksum, refresh) = native_scanout
            .as_ref()
            .and_then(|native| native.heads.first())
            .map_or((0, None, 0), |head| (
                head.presented_submissions,
                presented_logical_checksum(head.presented_content),
                head.refresh_millihz,
            ));
        cpu_visual_progress.observe_ready(started, submissions, checksum, refresh);
    }
    let mut session_quiescence = None::<SessionQuiescence>;
    let mut native_evidence = NativeSessionEvidence::default();
    let mut content_mapping_evidence = content_mapping_evidence::ContentMappingEvidence::default();
    if native_scanout.is_some() {
        native_evidence.open("startup");
    }
    macro_rules! native_recovery_allowed {
        () => {
            crate::live_session::shutdown::native_recovery_allowed(deadline, Instant::now(),
                session_quiescence.is_some(), runtime_deadline_key_drain.is_draining())
        };
    }
    macro_rules! close_native_owner {
        ($reason:literal, $mode:expr) => {{
            if let Some(native) = native_scanout.as_ref() {
                cpu_visual_progress.observe_native_scanout(&native, Instant::now());
                cpu_visual_progress.close_native_owner();
                input_latency_samples.close_native_owner();
                input_change_submission_baseline = None;
                input_change_frame_baseline = None;
                startup_required_submissions = None;
                native_evidence.close(&NativeEvidenceSnapshot::capture(&native), $reason);
            }
            native_retirement.begin(native_scanout, $mode, $reason)?;
        }};
        ($reason:literal) => { close_native_owner!($reason, RetirementMode::Drained) };
    }


    // Inert unless the session asked for it; see `direct_overlay_proof`.
    let mut direct_overlay_proof =
        crate::live_session::direct_overlay_proof::DirectOverlayProof::new(
            config.direct_overlay_proof,
            config.direct_overlay_hold_ticks,
        );
    let mut direct_cursor_proof = crate::live_session::direct_cursor_proof::DirectCursorProof::new(
        config.direct_cursor_proof,
    );
    let direct_overlay_generation = 1u64;
    let mut startup_native_recovery_attempted = false;
    let mut startup_topology_recovery_pending = false;
    let mut startup_outputs_ready_reported = false;
    let mut pending_authority_batches = VecDeque::new();
    // Advisory raster demand is latest-wins per protocol-neutral SurfaceId.
    // A busy client must not turn an authority-route stall into an unbounded
    // generation queue.
    let mut pending_surface_raster_requirements = BTreeMap::new();
    let mut seat_state = sophia_backend_live::LiveSeatState::Active;
    let mut pending_virtual_terminal: Option<(u8, Instant)> = None;
    let mut requested_virtual_terminal = None;
    let mut seat_release_prepared = false;
    let mut observed_wm_restart_count = wm_session.as_ref().map_or(0, |wm| wm.restarts);
    let mut output_proof_rollback_after_apply =
        OutputProofRollbackAfterApply::new(config.output_proof_rollback_after_apply);

    macro_rules! revoke_floating_pointer_interaction {
        ($reason:literal) => {{
            if let Some(interaction) = floating_pointer_gesture.cancel() {
                if let Some(wm) = wm_session.as_mut()
                    && matches!(
                        wm.enqueue_pointer_interaction(interaction, &layout)?,
                        LiveWmRequestAdmission::RejectedCapacity
                    )
                {
                    return Err(format!(
                        "security cancellation exceeded WM owner capacity: {}",
                        $reason
                    )
                    .into());
                }
                if let Some(runtime) = runtime.as_mut() {
                    let _ = runtime.set_floating_outline(
                        None,
                        &scene,
                        native_scanout.as_mut(),
                    )?;
                }
                crate::session_println!(
                    "sophia_live_wm_pointer schema=2 status=interaction_cancelled reason={} surface={}",
                    $reason,
                    interaction.surface.index(),
                );
            }
        }};
    }

    macro_rules! revoke_chrome_captures {
        ($reason:literal) => {{
            reference_capture.present(None);
            launcher_capture.present(None,0,&[],true);
            if let Some(shell)=metadata_shell.as_mut() && shell.launcher_busy(){
                shell.cancel_launcher()?;
                if let Some(runtime)=runtime.as_mut(){runtime.set_descriptor_overlay(None,&scene,native_scanout.as_mut())?;}
            }
            if let Some(shell)=metadata_shell.as_mut() && shell.reference_busy() {
                if let Err(error)=shell.cancel_reference() {
                    crate::session_eprintln!("sophia_reference status=cancel_failed reason={} error={error}",$reason);
                    shell.recover_transport("reference_cancel_failure")?;
                }
                if let Some(runtime)=runtime.as_mut(){runtime.set_descriptor_overlay(None,&scene,native_scanout.as_mut())?;}
            }
            let revoked = chrome_captures
                .cancel_all()
                .len()
                .saturating_add(descriptor_captures.cancel_all().len());
            content_captures.revoke_targets();
            if revoked != 0 {
                crate::session_println!(
                    "sophia_live_chrome_input schema=1 status=captures_cancelled reason={} count={revoked}",
                    $reason,
                );
            }
        }};
    }

    macro_rules! synchronize_wm_pointer_epoch {
        () => {{
            let restart_count = wm_session.as_ref().map_or(0, |wm| wm.restarts);
            if restart_count != observed_wm_restart_count {
                observed_wm_restart_count = restart_count;
                revoke_floating_pointer_interaction!("policy_restart");
                revoke_chrome_captures!("policy_restart");
            }
        }};
    }

    macro_rules! settle_revoked_shell_content_claims {
        ($shell:expr, $reason:expr) => {{
            let settlement = $shell.settle_revoked_content_grants(runtime.as_mut());
            if settlement.grants != 0 || settlement.retained != 0 {
                crate::session_println!(
                    "sophia_live_shell_content schema=3 status=claims_revoked reason={} grants={} claims={} retained={}",
                    $reason,
                    settlement.grants,
                    settlement.claims,
                    settlement.retained,
                );
            }
        }};
    }

    macro_rules! pause_metadata_shell_presentation {
        ($reason:literal) => {{
            if let Some(shell) = metadata_shell.as_mut() {
                let _ = shell.set_presentation_available(false, $reason)?;
                settle_revoked_shell_content_claims!(shell, $reason);
            }
        }};
    }

    macro_rules! begin_session_quiescence {
        ($reason:literal) => {{
            if session_quiescence.is_none() {
                let now = Instant::now();
                if let Err(error) = disconnect_frontend_for_drain(
                    frontend_service_sender,
                    &mut terminal_client_intake_stopped,
                ) {
                    return Err(format!(
                        "failed to stop frontend admission for session quiescence: {error}"
                    )
                    .into());
                }
                session_quiescence = Some(SessionQuiescence::new(
                    $reason,
                    now,
                    Duration::from_millis(SESSION_QUIESCENCE_TIMEOUT_MSEC),
                ));
                crate::session_println!(
                    "sophia_live_session_quiescence schema=3 status=started reason={} timeout_msec={}",
                    $reason,
                    SESSION_QUIESCENCE_TIMEOUT_MSEC,
                );
            }
        }};
    }

include!("owner_loop/session_control.rs")
}
