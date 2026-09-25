#![cfg(test)]

// The support modules below were declared from `live_session.rs`; declared
// here they read the same namespace through this glob (t026).
#[allow(unused_imports)]
use super::*;

#[path = "live_session/focus_candidate.rs"]
mod focus_candidate;
#[path = "live_session/production_test_helpers.rs"]
mod production_test_helpers;
use production_test_helpers::{completed_pointer_gesture_geometry, observe_public_output_topology};

#[path = "mirror_gate_session_config.rs"]
mod mirror_gate_session_config;

#[path = "live_control.rs"]
mod live_control_tests;

#[path = "panel_session_config.rs"]
mod panel_session_config;

#[path = "desktop_composition.rs"]
mod desktop_composition;

#[path = "launch_origin_socket.rs"]
mod launch_origin_socket;

#[path = "policy_partial_projection_socket.rs"]
mod policy_partial_projection_socket;

#[cfg(feature = "native-session")]
#[path = "glx_pixmap_export.rs"]
mod glx_pixmap_export;

#[path = "live_session_shutdown.rs"]
mod live_session_shutdown;

#[path = "component_desktop_preparation.rs"]
mod component_desktop_preparation;
#[path = "component_execution.rs"]
mod component_execution;
#[path = "component_prepare_refusal.rs"]
mod component_prepare_refusal;
#[path = "shell_component_config.rs"]
mod shell_component_config;

use super::metadata_broker::resolve_live_broker_toplevel_action;
use super::metadata_shell::{live_shell_activation_surfaces, reservation_within_profile};
use super::startup_readiness::startup_required_submission_for_head;
use super::startup_readiness::{
    StartupHeadRequirement, StartupNativeRecoveryReason, StartupOutputEvidence,
    StartupSurfacePresentationEvidence, all_startup_outputs_presented,
    startup_native_recovery_reason, startup_surface_visual_detail, synchronous_modeset_record,
};
use super::wm_update_coordinator_batch;
use super::{
    AUTHORITY_MERGE_RUN_LIMIT, AUTHORITY_MERGE_TRANSACTION_LIMIT, BufferSource,
    CommittedSurfaceState, CpuScanoutHeadEvidence, FirefoxM8StageProof, FirefoxM10DialogProof,
    FirefoxM10KittyProof, FirefoxM10PrimaryProof, FirefoxM10SelectionKittyProof,
    FloatingPointerGestureState, FloatingPointerOutline, FloatingPointerOutlineUpdate,
    LayerSnapshot, LiveClientStdoutCapture, LiveProductionCpuScene, LiveProductionVisualRuntime,
    LiveWmSession, LiveXAuthorityFile, PRIMARY_INPUT_PROOF_SCRIPT, PersistentXtermSessionConfig,
    PhysicalInputRoutingMode, PhysicalTextProof, PolicyCheckpointIdentity, PolicySessionDirectory,
    PreparedPublicPolicyLaunch, ProductionCycleNativeOwnerPolicy, PublicPolicyFaultPoint,
    PublicPolicyRestartDecision, PublicProfilePreparationExecutor, Rect, Region,
    ResizeSyncCapability, SECONDARY_POINTER_WITNESS_SCRIPT, SESSION_APP_ADMISSION_TIMEOUT_MSEC,
    SESSION_POLICY_RESPONSE_TIMEOUT_MSEC, SESSION_PROTOCOL_ERROR_TALLY_MAX_ENTRIES,
    SHELL_SWITCHER_SHORTCUT_ACTION, SessionFatalCleanupEvidence, SessionPointerPlacement,
    SessionProcessGuard, SessionProtocolErrorTally, Size, Transform, XPresentCadence,
    authority_batch_has_engine_work, authority_batch_is_pure_content, authority_merge_run_len,
    authority_transaction_count, authority_wait_timeout, center_geometry_without_scaling,
    clamp_floating_pointer_outline, clear_client_pressed_keys_state_only, committed_output_placing,
    control_is_pending, control_priority_should_reset, current_cpu_frame_is_presented,
    flush_all_client_pressed_keys, global_runtime_deadline_ends_session,
    independent_native_output_presented, input_baseline_is_presented, is_shell_switcher_shortcut,
    live_transaction_observed_size, live_transaction_raster_size, live_transaction_visual_evidence,
    logical_startup_output_progress, logical_synchronous_modeset_records,
    managed_child_exit_is_nonfatal, native_frame_service_requires_owner_progress,
    native_frame_service_should_preempt_authority, native_session_exported_pixels,
    newest_head_composition_frame, observe_floating_pointer_gesture,
    observe_public_output_generations, paced_repaint_runnable,
    pending_wm_focus_after_engine_decision, physical_input_page_flip_correlates,
    physical_input_pixels_already_changed, physical_input_routing_mode,
    place_pointer_event_for_routing, pointer_press_starts_focus_handoff,
    policy_checkpoint_replaced, policy_profile_identity, policy_projections_place_surface,
    production_cycle_native_owner_policy, public_policy_launch_spec,
    public_policy_restart_decision, public_policy_restart_settlement_pending,
    public_session_operations, record_runtime_commits, rects_intersect, resolve_public_shortcuts,
    route_input_events, session_failure_with_refused_requests, settle_session_fatal_error,
    stable_gpu_frame_proves_post_input_pixels, startup_submission_requirement,
    successful_primary_exit_ends_session, synchronize_runtime_surface_chrome_style,
    take_settled_input_delivery_wait, terminal_exit_is_session_failure,
};
use crate::live_session::{
    PRESENT_CADENCE_CAPACITY, RoutedInputIngressSaturation, policy_cause_subject_is_live,
    resolve_public_policy_affected_outputs,
};
use crate::session_keyboard::{
    PhysicalKeyboardCoverage, SessionClientKeyState, SessionClientPressedKey,
};
use crate::session_startup::{
    SessionStartupEvent, SessionStartupReadiness, reduce_session_startup,
};
use sophia_backend_live::{
    LiveProductionMirrorGroupBegin, LiveProductionMirrorGroupLifecycle,
    LiveProductionMirrorHeadTransition, LiveProductionNativeFrameId,
};
use sophia_engine::{
    InputFocusState, KeyRepeatConfig, KeyRepeatState, KeyboardFocusHandoffState,
    OutputFrameServiceObservation, OutputFrameServiceRequest, OutputNativeFramePhase,
    WmShortcutRegistry, WmShortcutRouter, pointer_offset_for_geometry,
};
use sophia_protocol::{
    AuthorityKind, DeviceId, InputEventKind, InputEventPacket, NamespaceCapabilities,
    NamespaceProfile, Point, SeatId, SurfaceId, SurfaceTransaction, SurfaceTransactionReadiness,
    WmActionId, WmBindingRegistration, WmCapabilities, WmModifierMask, WmSessionAction,
};
use sophia_protocol::{OutputId, TransactionId};
use sophia_x_authority::{
    X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888, XAuthorityObservedTransactionBatch,
    XAuthorityRoutedInputMode,
};
use sophia_x_authority::{
    XAuthorityClientSurfaceRoutes, XCoreKeyboardMapper, XKB_DEFAULT_REPEAT_DELAY_MSEC,
    XKB_DEFAULT_REPEAT_INTERVAL_MSEC, XkbKeymapSnapshot, XkbRmlvoConfig,
};
use std::io::Write;
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

fn add_test_surface_route(
    batch: &mut XAuthorityObservedTransactionBatch,
    surface: SurfaceId,
    client: sophia_x_authority::XServerFrontendClientId,
) {
    batch
        .surface_routes
        .push(sophia_x_authority::XAuthoritySurfaceRouteObservation {
            surface,
            client,
            admission: batch.admission,
        });
}

#[path = "live_session/authority_merge_tests.rs"]
mod authority_merge_tests;
#[path = "live_session/desktop_shortcut_tests.rs"]
mod desktop_shortcut_tests;
#[path = "live_session/device_identity_tests.rs"]
mod device_identity_tests;
#[path = "live_session/direct_cursor_proof_tests.rs"]
mod direct_cursor_proof_tests;
#[path = "live_session/direct_overlay_proof_tests.rs"]
mod direct_overlay_proof_tests;
#[path = "live_session/input_policy_tests.rs"]
mod input_policy_tests;
#[path = "live_session/metadata_shell_tests.rs"]
mod metadata_shell_tests;
#[path = "live_session/output_proof_tests.rs"]
mod output_proof_tests;
#[path = "live_session/output_topology_owner_tests.rs"]
mod output_topology_owner_tests;
#[path = "live_session/policy_transport_worker_tests.rs"]
mod policy_transport_worker_tests;
#[path = "live_session/present_retirement_tests.rs"]
mod present_retirement_tests;
#[path = "live_session/presentation_role_tests.rs"]
mod presentation_role_tests;
#[path = "live_session/presentation_tests.rs"]
mod presentation_tests;
#[path = "live_session/profile_preparation_tests.rs"]
mod profile_preparation_tests;
#[path = "live_session/public_policy_recovery_tests.rs"]
mod public_policy_recovery_tests;
#[path = "live_session/session_config_tests.rs"]
mod session_config_tests;
#[path = "live_session/startup_output_tests.rs"]
mod startup_output_tests;
#[path = "live_session/visual_candidate_tests.rs"]
mod visual_candidate_tests;
#[path = "live_session/wm_admission_tests.rs"]
mod wm_admission_tests;
#[path = "live_session/wm_session_tests.rs"]
mod wm_session_tests;

fn test_key_repeat_parts() -> (KeyRepeatState, XkbKeymapSnapshot) {
    (
        KeyRepeatState::new(
            KeyRepeatConfig::new(
                u64::from(XKB_DEFAULT_REPEAT_DELAY_MSEC),
                u64::from(XKB_DEFAULT_REPEAT_INTERVAL_MSEC),
            )
            .unwrap(),
        ),
        XkbKeymapSnapshot::new(&XkbRmlvoConfig::default()).unwrap(),
    )
}

include!("live_session/owner_scheduling.rs");

#[test]
fn promoted_chrome_is_synchronized_at_the_production_boundary() {
    let outputs = [sophia_engine::HeadlessOutput::deterministic()];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let promoted = sophia_engine::SurfaceChromeStyle {
        focus_ring: sophia_engine::FocusRingStyle {
            width: 6,
            ..sophia_engine::FocusRingStyle::default()
        },
        ..sophia_engine::SurfaceChromeStyle::default()
    };

    assert!(synchronize_runtime_surface_chrome_style(
        &mut runtime,
        promoted
    ));
    assert!(!synchronize_runtime_surface_chrome_style(
        &mut runtime,
        promoted
    ));
}

#[test]
fn action_launched_child_exit_is_nonfatal_in_proof_and_normal_sessions() {
    let transaction = Some(sophia_protocol::TransactionId::from_raw(9));

    assert!(managed_child_exit_is_nonfatal(false, transaction));
    assert!(managed_child_exit_is_nonfatal(true, None));
    assert!(!managed_child_exit_is_nonfatal(false, None));
}

#[test]
fn synchronous_modeset_record_requires_the_initialized_submission() {
    assert_eq!(
        synchronous_modeset_record(2, Some(1)).as_deref(),
        Some(
            "sophia_live_native_startup_output schema=1 status=presented output=2 proof=synchronous_modeset submission=1"
        )
    );
    assert_eq!(synchronous_modeset_record(2, None), None);
}

#[test]
fn mirror_startup_readiness_is_one_logical_output() {
    let mirrored = OutputId::from_raw(7);
    let independent = OutputId::from_raw(8);

    assert_eq!(
        logical_startup_output_progress([(mirrored, true), (mirrored, true), (independent, true),]),
        (2, 2)
    );
    assert_eq!(
        logical_startup_output_progress([(mirrored, true), (mirrored, false)]),
        (0, 1),
        "one ready mirror head must not publish its logical output"
    );
}

#[test]
fn mirror_synchronous_modeset_record_is_deduplicated_and_requires_every_head() {
    let mirrored = OutputId::from_raw(7);
    assert_eq!(
        logical_synchronous_modeset_records([(mirrored, Some(1)), (mirrored, Some(1))]),
        [
            "sophia_live_native_startup_output schema=1 status=presented output=7 proof=synchronous_modeset submission=1"
        ]
    );
    assert!(
        logical_synchronous_modeset_records([(mirrored, Some(1)), (mirrored, None)]).is_empty()
    );
}

#[test]
fn startup_readiness_requires_every_output_callback_and_submission() {
    let healthy = StartupOutputEvidence {
        required_submission: 2,
        presented_submissions: 2,
        required_content_frame: 3,
        presented_content_frame: 5,
        callbacks: 1,
        synchronous_modeset: false,
    };
    assert!(all_startup_outputs_presented(&[healthy]));
    assert!(!all_startup_outputs_presented(&[
        healthy,
        StartupOutputEvidence {
            required_submission: 2,
            presented_submissions: 1,
            required_content_frame: 3,
            presented_content_frame: 5,
            callbacks: 0,
            synchronous_modeset: false,
        },
    ]));
    assert!(all_startup_outputs_presented(&[StartupOutputEvidence {
        required_submission: 1,
        presented_submissions: 1,
        required_content_frame: 0,
        presented_content_frame: 1,
        callbacks: 0,
        synchronous_modeset: true,
    }]));
}

#[test]
fn startup_native_recovery_requires_objective_transport_stall() {
    assert_eq!(
        startup_native_recovery_reason(false, Duration::from_secs(30)),
        None,
        "valid black client content must remain under the readiness deadline"
    );
    assert_eq!(
        startup_native_recovery_reason(true, Duration::from_millis(749)),
        None
    );
    assert_eq!(
        startup_native_recovery_reason(true, Duration::from_millis(750)),
        Some(StartupNativeRecoveryReason::MissingOutputCallback)
    );
}

#[test]
fn startup_surface_presentation_evidence_is_order_independent_and_surface_keyed() {
    let startup = SurfaceId::new(1, 1);
    let status_bar = SurfaceId::new(2, 1);
    let mut evidence = StartupSurfacePresentationEvidence::default();
    let mut readiness = SessionStartupReadiness::default();

    evidence.observe_stable(status_bar, 900);
    evidence.observe_stable(startup, 100);
    evidence.observe_stable(startup, 700);

    assert!(evidence.stable_presented(startup));
    assert!(evidence.stable_presented(status_bar));
    assert_eq!(evidence.nonzero_rgb_pixels(startup), 700);
    assert_eq!(evidence.nonzero_rgb_pixels(status_bar), 900);
    assert_eq!(evidence.nonzero_rgb_pixels(SurfaceId::new(3, 1)), 0);
    assert!(evidence.visual_detail(startup));
    assert!(!evidence.visual_detail(SurfaceId::new(3, 1)));

    for event in [
        SessionStartupEvent::PinSurface(startup),
        SessionStartupEvent::ClientFocusApplied(startup),
    ] {
        reduce_session_startup(&mut readiness, event);
    }
    if evidence.visual_detail(startup) {
        reduce_session_startup(&mut readiness, SessionStartupEvent::VisualDetail(startup));
    }
    if evidence.stable_presented(startup) {
        reduce_session_startup(
            &mut readiness,
            SessionStartupEvent::StablePresented(startup),
        );
    }
    reduce_session_startup(&mut readiness, SessionStartupEvent::OutputsPresented);
    assert!(readiness.ready);

    evidence.clear();
    assert!(!evidence.stable_presented(startup));
}

#[test]
fn startup_gpu_visual_detail_does_not_require_a_base_committed_surface() {
    assert!(startup_surface_visual_detail(None, 700));
    assert!(startup_surface_visual_detail(Some(false), 700));
    assert!(startup_surface_visual_detail(Some(true), 0));
    assert!(!startup_surface_visual_detail(None, 0));
    assert!(!startup_surface_visual_detail(Some(false), 0));
}

#[test]
fn retained_present_cadence_is_aggregated_without_per_frame_logging() {
    let mut cadence = XPresentCadence::new();
    cadence.observe(1_000_000);
    cadence.observe(1_016_667);
    cadence.observe(1_033_334);

    let summary = cadence.summary().expect("three advancing samples");
    assert_eq!(summary.samples, 3);
    assert_eq!(summary.advancing_intervals, 2);
    assert_eq!(summary.nonadvancing, 0);
    assert!((summary.mean_fps - 59.999).abs() < 0.001);
    assert!((summary.p95_frame_msec - 16.667).abs() < f64::EPSILON);

    cadence.observe(1_033_334);
    assert_eq!(cadence.summary().unwrap().nonadvancing, 1);
}

/// A long session used to lose its own frame pacing: the sampler latched an
/// overflow flag at capacity and never cleared it, so `summary` returned `None`
/// forever after. The window now slides instead.
#[test]
fn present_cadence_keeps_measuring_past_its_window() {
    const FRAME_USEC: u64 = 16_667;
    let mut cadence = XPresentCadence::new();
    let samples = PRESENT_CADENCE_CAPACITY * 2;
    for index in 0..=samples {
        cadence.observe(1_000_000 + FRAME_USEC * index as u64);
    }

    let summary = cadence
        .summary()
        .expect("a sliding window still summarises after it fills");
    assert_eq!(summary.advancing_intervals, PRESENT_CADENCE_CAPACITY);
    assert_eq!(cadence.evicted, samples - PRESENT_CADENCE_CAPACITY);
    // Elapsed time tracks the retained intervals rather than the whole session,
    // so the rate stays correct instead of drifting as samples age out.
    assert!(
        (summary.mean_fps - 59.999).abs() < 0.01,
        "mean_fps drifted to {}",
        summary.mean_fps
    );
    assert!((summary.p95_frame_msec - 16.667).abs() < 0.001);
}

#[test]
fn independent_output_accepts_exact_synchronous_or_asynchronous_lifecycle() {
    assert!(independent_native_output_presented(1, 0, 0, true));
    assert!(independent_native_output_presented(8, 7, 7, true));
    assert!(!independent_native_output_presented(1, 0, 0, false));
    assert!(!independent_native_output_presented(2, 0, 0, true));
    assert!(!independent_native_output_presented(8, 7, 6, true));
}

#[test]
fn a_head_showing_an_empty_desktop_still_completes() {
    // An output holding no windows composes black, and black is the right
    // picture for it. The pixels the session did render are what must exist,
    // and they can come from any head.
    assert!(independent_native_output_presented(8, 7, 7, true));
    assert!(native_session_exported_pixels([17, 0]));
    assert!(native_session_exported_pixels([0, 17]));
    // Nothing anywhere is a session that rendered nothing.
    assert!(!native_session_exported_pixels([0, 0]));
    assert!(!native_session_exported_pixels([]));
}

#[test]
fn blank_normal_session_process_guard_has_no_primary_child() {
    let mut guard = SessionProcessGuard {
        child: None,
        diagnostic: None,
        secondary_children: Vec::new(),
        socket_path: None,
        grouped: true,
    };
    let (primary, secondary) = guard.children_mut();
    assert!(primary.is_none());
    assert!(secondary.is_empty());
    guard.terminate().unwrap();
}

#[test]
fn client_stdout_capture_reads_without_waiting_for_inherited_writer_close() {
    let (capture, mut writer) = LiveClientStdoutCapture::create(181).unwrap();
    writer.write_all(b"sophia\n").unwrap();
    writer.flush().unwrap();

    assert_eq!(capture.read_bounded().unwrap(), b"sophia\n");

    writer.write_all(b"still-open").unwrap();
}

#[test]
fn settled_input_delivery_wait_is_consumed_once() {
    let started = Instant::now();
    let mut wait = Some(started);

    assert_eq!(take_settled_input_delivery_wait(&mut wait, false), None);
    assert_eq!(wait, Some(started));
    assert_eq!(
        take_settled_input_delivery_wait(&mut wait, true),
        Some(started)
    );
    assert_eq!(wait, None);
}

#[test]
fn successful_primary_exit_keeps_requested_input_proof_alive() {
    assert!(successful_primary_exit_ends_session(false));
    assert!(!successful_primary_exit_ends_session(true));
}

#[test]
fn fatal_client_cleanup_preserves_error_after_pending_mirror_callback_drains() {
    let original = "session client exited during live session with status exit status: 83";
    let frame = LiveProductionNativeFrameId::from_raw(10);
    let mut group = LiveProductionMirrorGroupLifecycle::new(
        OutputId::from_raw(7),
        [
            sophia_engine::RenderHeadId::from_raw(94),
            sophia_engine::RenderHeadId::from_raw(104),
        ],
    )
    .unwrap();
    assert_eq!(group.begin(frame), LiveProductionMirrorGroupBegin::Started);
    assert_eq!(
        group.mark_submitted(sophia_engine::RenderHeadId::from_raw(94), frame),
        LiveProductionMirrorHeadTransition::GroupReady
    );
    assert_eq!(
        group.mark_flipped(sophia_engine::RenderHeadId::from_raw(94), frame),
        LiveProductionMirrorHeadTransition::GroupReady
    );
    assert_eq!(
        group.mark_submitted(sophia_engine::RenderHeadId::from_raw(104), frame),
        LiveProductionMirrorHeadTransition::Accepted
    );
    assert!(group.awaiting_flips());

    // Fatal intake stops here. Logical presentation already belongs to the
    // primary, but the bounded drain must retain the sibling's physical owner
    // until its callback releases that head's KMS submission.
    assert_eq!(
        group.mark_flipped(sophia_engine::RenderHeadId::from_raw(104), frame),
        LiveProductionMirrorHeadTransition::Accepted
    );
    assert_eq!(group.take_completed_frame(), Some(frame));
    let evidence = SessionFatalCleanupEvidence {
        frontend_intake_stopped: true,
        // The sibling head still owns frame 10 when the client fails. The
        // bounded completion drain consumes that callback before detaching.
        native_heads_in_flight_before: 1,
        native_cleanup_required: true,
        native_suspend_attempted: true,
        native_suspend_reported: true,
        native_drained: true,
        abandoned_scanouts: 0,
        renderer_images_cleared: true,
        presentations_shutdown: true,
    };

    assert!(evidence.clean());
    assert_eq!(
        settle_session_fatal_error(original, evidence, &[]),
        original
    );
}

#[test]
fn fatal_client_cleanup_aggregates_owner_abandonment_without_masking_client_error() {
    let original = "session client exited during live session with status exit status: 83";
    let evidence = SessionFatalCleanupEvidence {
        frontend_intake_stopped: true,
        native_heads_in_flight_before: 1,
        native_cleanup_required: true,
        native_suspend_attempted: true,
        native_suspend_reported: true,
        native_drained: false,
        abandoned_scanouts: 1,
        renderer_images_cleared: false,
        presentations_shutdown: true,
    };
    let failures = vec!["native completion forced detach with 1 abandoned scanouts".to_owned()];

    let error = settle_session_fatal_error(original, evidence, &failures);
    assert!(error.starts_with(original));
    assert!(error.contains("bounded session cleanup failed"));
    assert!(error.contains("1 abandoned scanouts"));
}

#[test]
fn runtime_fatal_cleanup_preserves_the_engine_error_after_clean_drain() {
    let original = "engine backend tick failed: invalid surface ID";
    let evidence = SessionFatalCleanupEvidence {
        frontend_intake_stopped: true,
        native_heads_in_flight_before: 1,
        native_cleanup_required: true,
        native_suspend_attempted: true,
        native_suspend_reported: true,
        native_drained: true,
        abandoned_scanouts: 0,
        renderer_images_cleared: true,
        presentations_shutdown: true,
    };

    assert!(evidence.clean());
    assert_eq!(
        settle_session_fatal_error(original, evidence, &[]),
        original
    );
}

#[test]
fn global_runtime_deadline_does_not_strand_an_active_input_proof() {
    assert!(global_runtime_deadline_ends_session(false));
    assert!(!global_runtime_deadline_ends_session(true));
}

/// A failed run prints one string, so that string has to carry every opcode.
///
/// Keeping only the first error meant a session reporting two dozen failures named
/// one request and discarded the rest, and each remaining cause then cost its own
/// physical run to find.
#[test]
fn the_protocol_error_tally_names_every_opcode_it_saw() {
    fn error(
        major_code: u8,
        minor_code: u16,
        code: u8,
    ) -> sophia_x_authority::XAuthorityProtocolErrorObservation {
        sophia_x_authority::XAuthorityProtocolErrorObservation {
            code,
            sequence: 1,
            minor_code,
            major_code,
        }
    }

    let mut tally = SessionProtocolErrorTally::default();
    assert!(tally.is_empty());
    assert_eq!(tally.summary(), "");

    // The shape the physical run produced: many of one opcode, a few of another.
    for _ in 0..17 {
        tally.observe(&error(139, 2, 3));
    }
    for _ in 0..7 {
        tally.observe(&error(135, 2, 1));
    }
    assert!(!tally.is_empty());
    // Ordered by opcode, one bucket each, never merged.
    assert_eq!(tally.summary(), "135/2/1x7 139/2/3x17");

    // The same request failing a different way is a different bucket.
    tally.observe(&error(139, 2, 8));
    assert_eq!(tally.summary(), "135/2/1x7 139/2/3x17 139/2/8x1");
}

/// The tally is bounded, and says how much a reset dropped rather than capping
/// silently.
#[test]
fn the_protocol_error_tally_reports_what_a_reset_discarded() {
    let mut tally = SessionProtocolErrorTally::default();
    for index in 0..SESSION_PROTOCOL_ERROR_TALLY_MAX_ENTRIES {
        tally.observe(&sophia_x_authority::XAuthorityProtocolErrorObservation {
            code: 1,
            sequence: 1,
            minor_code: u16::try_from(index).unwrap(),
            major_code: 139,
        });
    }
    assert_eq!(tally.discarded, 0, "a full table has lost nothing yet");

    // One opcode past the bound resets, and the count of what went with it
    // survives so the total still reconciles.
    tally.observe(&sophia_x_authority::XAuthorityProtocolErrorObservation {
        code: 1,
        sequence: 1,
        minor_code: u16::try_from(SESSION_PROTOCOL_ERROR_TALLY_MAX_ENTRIES).unwrap(),
        major_code: 139,
    });
    assert_eq!(
        tally.discarded,
        u64::try_from(SESSION_PROTOCOL_ERROR_TALLY_MAX_ENTRIES).unwrap()
    );
    assert_eq!(
        tally.summary(),
        format!("139/{SESSION_PROTOCOL_ERROR_TALLY_MAX_ENTRIES}/1x1")
    );
}

/// A run that refused nothing still says so.
///
/// The tally used to print one line per opcode and nothing at all when there
/// were none, which made "this session refused nothing" and "nobody kept the
/// count" the same observation.
#[test]
fn a_clean_protocol_error_tally_still_reports_a_line() {
    let lines = SessionProtocolErrorTally::default().report_lines();
    assert_eq!(lines.len(), 1, "a clean tally reports exactly once");
    assert_eq!(
        lines[0],
        "sophia_live_session_protocol_error_tally schema=3 status=clean major=0 minor=0 code=0 count=0 distinct=0 discarded=0 total=0"
    );
}

/// A failure that is not about protocol errors still carries the ones it saw.
///
/// This is the regression the whole wrapper exists for: the physical run that
/// refused seven `BuffersFromPixmap` requests died on its input-sequence timeout
/// and reported only the timeout, because the tally was read on the clean-exit
/// path alone.
#[test]
fn a_session_failure_carries_the_requests_the_session_refused() {
    let mut tally = SessionProtocolErrorTally::default();
    for _ in 0..7 {
        tally.observe(&sophia_x_authority::XAuthorityProtocolErrorObservation {
            code: 3,
            sequence: 1,
            minor_code: 8,
            major_code: 139,
        });
    }
    assert_eq!(
        session_failure_with_refused_requests("physical input sequence timed out", &tally),
        "physical input sequence timed out; the session also refused 7 X requests by_opcode=[139/8/3x7]"
    );

    // The refusals reported and the refusals counted reconcile: `total` covers
    // what a reset dropped, which `summary` no longer holds.
    assert_eq!(tally.total(), 7);

    // A failure with nothing to attach is left exactly as it was.
    let clean = SessionProtocolErrorTally::default();
    assert_eq!(
        session_failure_with_refused_requests("native page flip stalled", &clean),
        "native page flip stalled"
    );
}

#[test]
fn physical_input_preserves_shortcuts_without_an_application_surface() {
    let proof = SurfaceId::new(1, 1);
    let survivor = SurfaceId::new(2, 1);
    assert_eq!(
        physical_input_routing_mode(false, Some(proof), Some(proof), false),
        PhysicalInputRoutingMode::Full
    );
    assert_eq!(
        physical_input_routing_mode(true, Some(proof), Some(proof), false),
        PhysicalInputRoutingMode::Suppressed
    );
    assert_eq!(
        physical_input_routing_mode(true, Some(survivor), Some(proof), false),
        PhysicalInputRoutingMode::Full
    );
    assert_eq!(
        physical_input_routing_mode(true, None, None, true),
        PhysicalInputRoutingMode::Full
    );
    assert_eq!(
        physical_input_routing_mode(true, Some(proof), Some(proof), true),
        PhysicalInputRoutingMode::ShortcutsOnly
    );
}

#[test]
fn shortcut_only_input_activates_super_enter_without_routing_unfocused_keys() {
    let action = WmActionId::from_raw(7);
    let registry = WmShortcutRegistry::new(
        &[WmBindingRegistration {
            action,
            keycode: 28,
            modifiers: WmModifierMask {
                bits: WmModifierMask::SUPER,
            },
        }],
        WmCapabilities::all_supported(),
        1,
        sophia_protocol::WmChromePolicy::default(),
    )
    .unwrap();
    let mut shortcuts = WmShortcutRouter::new(registry);
    let events = [125, 28]
        .into_iter()
        .enumerate()
        .map(|(index, keycode)| InputEventPacket {
            serial: u64::try_from(index + 1).unwrap(),
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(1),
            time_msec: u64::try_from(index + 1).unwrap(),
            kind: InputEventKind::Key {
                keycode,
                pressed: true,
            },
            global_position: None,
            target_surface: None,
            local_position: None,
        })
        .collect();
    let (input_sender, input_receiver) = sync_channel(4);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    let mut next_delivery = 1;

    let report = route_input_events(
        events,
        &InputFocusState::new(),
        &[],
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        Some(&mut shortcuts),
        &mut pointer,
        false,
        false,
        false,
        PhysicalInputRoutingMode::ShortcutsOnly,
        &mut next_delivery,
        0,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.wm_actions, [action]);
    assert_eq!(report.keys_observed, 2);
    assert_eq!(report.keys_routed, 0);
    assert!(input_receiver.try_recv().is_err());
}

#[test]
fn pending_physical_proof_moves_cursor_without_routing_application_input() {
    let events = vec![
        InputEventPacket {
            serial: 1,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(2),
            time_msec: 1,
            kind: InputEventKind::PointerMotion,
            global_position: Some(Point { x: 12.0, y: -8.0 }),
            target_surface: None,
            local_position: None,
        },
        InputEventPacket {
            serial: 2,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(1),
            time_msec: 2,
            kind: InputEventKind::Key {
                keycode: 31,
                pressed: true,
            },
            global_position: None,
            target_surface: None,
            local_position: None,
        },
    ];
    let (input_sender, input_receiver) = sync_channel(2);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    pointer.center_on_primary_output(Size {
        width: 2560,
        height: 1440,
    });
    let mut proof = PhysicalTextProof::new("sophia").unwrap();
    let mut next_delivery = 1;

    let report = route_input_events(
        events,
        &InputFocusState::new(),
        &[],
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        None,
        &mut pointer,
        true,
        false,
        false,
        PhysicalInputRoutingMode::CursorOnly,
        &mut next_delivery,
        0,
        Some(&mut proof),
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.pointer_events, 1);
    assert_eq!(report.pointer_routed, 0);
    assert_eq!(report.keys_observed, 1);
    assert_eq!(report.keys_routed, 0);
    assert_eq!(proof.matched_events(), 0);
    assert!(input_receiver.try_recv().is_err());
    assert_ne!(
        pointer.position(),
        Some(Point {
            x: 1280.0,
            y: 720.0
        })
    );
}

#[test]
fn authority_transaction_accounting_excludes_surface_removals() {
    assert_eq!(authority_transaction_count(&[]), 0);
}

#[test]
fn runtime_commit_accounting_records_only_accepted_batches() {
    assert_eq!(record_runtime_commits(166, 1), 167);
    assert_eq!(record_runtime_commits(167, 0), 167);
}

#[test]
fn completed_physical_input_reconciles_pixels_that_arrived_before_return() {
    assert!(physical_input_pixels_already_changed(
        Some(10),
        Some(20),
        true
    ));
    assert!(!physical_input_pixels_already_changed(
        Some(10),
        Some(20),
        false
    ));
    assert!(!physical_input_pixels_already_changed(
        Some(10),
        Some(10),
        true
    ));
}

#[test]
fn stable_focused_gpu_content_arms_input_without_cpu_scene_pixels() {
    assert!(input_baseline_is_presented(true, false));
    assert!(input_baseline_is_presented(false, true));
    assert!(!input_baseline_is_presented(false, false));
}

#[test]
fn cpu_input_waits_for_the_current_scene_frame_to_reach_scanout() {
    // The counts a settled two-head session actually reports: every
    // submission retired, nothing queued behind it.
    let caught_up = [
        CpuScanoutHeadEvidence {
            submissions: 4,
            presented_submissions: 4,
        },
        CpuScanoutHeadEvidence {
            submissions: 3,
            presented_submissions: 3,
        },
    ];
    assert!(current_cpu_frame_is_presented(
        Some(1),
        true,
        Some(caught_up)
    ));
    // Without native scanout there is no head to wait for.
    assert!(current_cpu_frame_is_presented(
        Some(1),
        true,
        None::<[CpuScanoutHeadEvidence; 0]>
    ));
    // A frame still in flight on any head would let the post-input
    // correlation latch onto a flip that was carrying pre-input pixels.
    assert!(!current_cpu_frame_is_presented(
        Some(1),
        true,
        Some([
            CpuScanoutHeadEvidence {
                submissions: 4,
                presented_submissions: 4,
            },
            CpuScanoutHeadEvidence {
                submissions: 4,
                presented_submissions: 3,
            },
        ])
    ));
    // The focused surface's content has not crossed a flip yet.
    assert!(!current_cpu_frame_is_presented(
        Some(1),
        false,
        Some(caught_up)
    ));
    // A blank scene is not a baseline, and neither is no scene at all.
    assert!(!current_cpu_frame_is_presented(
        Some(0),
        true,
        Some(caught_up)
    ));
    assert!(!current_cpu_frame_is_presented(None, true, Some(caught_up)));
    // Native scanout that owns no head proves nothing.
    assert!(!current_cpu_frame_is_presented(
        Some(1),
        true,
        Some([] as [CpuScanoutHeadEvidence; 0])
    ));
}

#[test]
fn physical_pointer_starts_at_focused_surface_center() {
    let raw = Point { x: -4.0, y: 6.0 };
    let offset = pointer_offset_for_geometry(
        raw,
        Rect {
            x: 80,
            y: 60,
            width: 960,
            height: 640,
        },
    );
    assert_eq!(raw.x + offset.x, 560.0);
    assert_eq!(raw.y + offset.y, 380.0);
}

#[test]
fn physical_pointer_can_move_before_an_application_surface_exists() {
    let mut pointer = SessionPointerPlacement::default();
    assert_eq!(
        pointer.center_on_primary_output(Size {
            width: 2560,
            height: 1440,
        }),
        Point {
            x: 1280.0,
            y: 720.0,
        }
    );
    let events = vec![InputEventPacket {
        serial: 1,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        time_msec: 1,
        kind: InputEventKind::PointerMotion,
        global_position: Some(Point { x: 12.0, y: -8.0 }),
        target_surface: None,
        local_position: None,
    }];
    let (input_sender, input_receiver) = sync_channel(1);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut next_delivery = 1;
    let report = route_input_events(
        events,
        &InputFocusState::new(),
        &[],
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        None,
        &mut pointer,
        true,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut next_delivery,
        0,
        None,
        None,
        None,
    )
    .unwrap();

    assert_eq!(report.pointer_events, 1);
    assert_eq!(report.pointer_routed, 0);
    assert_eq!(
        pointer.position(),
        Some(Point {
            x: 1292.0,
            y: 712.0,
        })
    );
    assert!(input_receiver.try_recv().is_err());
}

#[test]
fn closing_surface_clears_pressed_keys_without_client_delivery_barrier() {
    let surface = SurfaceId::new(7, 1);
    let key = SessionClientPressedKey {
        surface,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(1),
        keycode: 125,
    };
    let mut client_keys = SessionClientKeyState::default();
    client_keys.record_routed(key, true);
    let mut scratch = Vec::new();
    let mut modifiers = XCoreKeyboardMapper::new();
    let (input_sender, input_receiver) = sync_channel(1);
    let mut next_delivery = 9;

    let cleared = clear_client_pressed_keys_state_only(
        surface,
        &mut client_keys,
        &mut scratch,
        &mut modifiers,
        &input_sender,
        &mut RoutedInputIngressSaturation::default(),
        &mut next_delivery,
        10,
    )
    .unwrap();
    let routed = input_receiver.try_recv().unwrap();

    assert_eq!(cleared, 1);
    assert_eq!(client_keys.pending_len(), 0);
    assert_eq!(next_delivery, 10);
    assert_eq!(routed.delivery, None);
    assert_eq!(routed.mode, XAuthorityRoutedInputMode::StateOnly);
    assert_eq!(
        routed.request.kind,
        InputEventKind::Key {
            keycode: 125,
            pressed: false,
        }
    );
}

#[test]
fn vt_chord_releases_application_modifiers_before_suspension() {
    let seat = SeatId::from_raw(1);
    let surface = SurfaceId::new(1, 1);
    let committed = [CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            sophia_protocol::Size {
                width: 640,
                height: 480,
            },
        ),
        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }),
    }];
    let mut focus = InputFocusState::new();
    assert_eq!(
        focus.focus_surface(seat, surface, &committed),
        sophia_engine::InputFocusDecision::Focused
    );
    let events = [29, 56, 60]
        .into_iter()
        .enumerate()
        .map(|(index, keycode)| InputEventPacket {
            serial: u64::try_from(index + 1).unwrap(),
            seat,
            device: DeviceId::from_raw(1),
            time_msec: u64::try_from(index + 1).unwrap(),
            kind: InputEventKind::Key {
                keycode,
                pressed: true,
            },
            global_position: None,
            target_surface: None,
            local_position: None,
        })
        .collect();
    let (input_sender, input_receiver) = sync_channel(8);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    let mut next_delivery = 1;

    let report = route_input_events(
        events,
        &focus,
        &committed,
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        None,
        &mut pointer,
        false,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut next_delivery,
        0,
        None,
        None,
        None,
    )
    .unwrap();
    let routed = input_receiver
        .try_iter()
        .map(|input| input.request.kind)
        .collect::<Vec<_>>();

    assert_eq!(report.virtual_terminal, Some(2));
    assert_eq!(report.virtual_terminal_trigger_keycode, Some(60));
    assert_eq!(
        report.virtual_terminal_modifier_keycodes,
        [Some(29), None, Some(56), None]
    );
    assert_eq!(report.virtual_terminal_modifier_releases, 2);
    assert_eq!(
        routed,
        [
            InputEventKind::Key {
                keycode: 29,
                pressed: true,
            },
            InputEventKind::Key {
                keycode: 56,
                pressed: true,
            },
            InputEventKind::Key {
                keycode: 29,
                pressed: false,
            },
            InputEventKind::Key {
                keycode: 56,
                pressed: false,
            },
        ]
    );
    assert_eq!(modifiers.modifier_mask(), 0);
}

#[test]
fn interactive_pointer_proof_routes_motion_after_placement() {
    let mut pointer =
        SessionPointerPlacement::with_raw_to_logical_offset(Point { x: 10.0, y: 20.0 });
    let mut motion = InputEventPacket {
        serial: 1,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        time_msec: 1,
        kind: InputEventKind::PointerMotion,
        global_position: Some(Point { x: 30.0, y: 40.0 }),
        target_surface: None,
        local_position: None,
    };

    assert!(place_pointer_event_for_routing(&mut motion, None, &[], &mut pointer, false).0);
    assert_eq!(motion.global_position, Some(Point { x: 40.0, y: 60.0 }));
}

#[test]
fn secondary_terminal_is_a_pointer_witness_without_a_text_prompt() {
    assert!(SECONDARY_POINTER_WITNESS_SCRIPT.contains("?1000h"));
    assert!(SECONDARY_POINTER_WITNESS_SCRIPT.contains("stty raw -echo"));
    assert!(SECONDARY_POINTER_WITNESS_SCRIPT.contains("Pointer input received"));
    assert!(!SECONDARY_POINTER_WITNESS_SCRIPT.contains("read -r line"));
    assert!(!SECONDARY_POINTER_WITNESS_SCRIPT.contains('\0'));
}

#[test]
fn primary_input_proof_remains_visible_until_session_completion() {
    assert!(PRIMARY_INPUT_PROOF_SCRIPT.contains("sleep 300"));
    assert!(!PRIMARY_INPUT_PROOF_SCRIPT.contains("sleep 5"));
}

/// A cause queued across a topology change must not be refused for naming
/// outputs that change replaced.
///
/// Ordinary policy cycles are held for the whole of a topology candidate, so a
/// cause raised before it is submitted after it, against a scene whose outputs
/// may be entirely different. Passing its original outputs through produced
/// `UnknownAffectedOutput` and failed the session on a request whose only fault
/// was that it waited.
#[test]
fn a_queued_cause_resolves_its_outputs_against_the_current_scene() {
    let one = OutputId::from_raw(1);
    let two = OutputId::from_raw(2);
    let three = OutputId::from_raw(3);

    // Outputs that survived are kept, in the order the cause named them.
    assert_eq!(
        resolve_public_policy_affected_outputs(vec![two, one], [one, two]),
        vec![two, one]
    );

    // Outputs the topology removed are dropped.
    assert_eq!(
        resolve_public_policy_affected_outputs(vec![one, three], [one, two]),
        vec![one]
    );

    // Nothing it named survived: the topology moved under it, which is itself
    // a reason to lay out, so every live output is affected.
    assert_eq!(
        resolve_public_policy_affected_outputs(vec![three], [two, one]),
        vec![one, two]
    );

    // A cause raised before any output existed still resolves to the live set
    // rather than an empty request, which the projection reducer rejects.
    assert_eq!(
        resolve_public_policy_affected_outputs(Vec::new(), [one]),
        vec![one]
    );
}

/// A cause outliving the surface it was raised about must be dropped, not
/// submitted.
///
/// The projection reducer refuses a cause naming a withdrawn surface with
/// `InvalidRequestCause`, and that refusal ends the session. Causes are queued
/// long enough for this to matter because ordinary cycles are held for the
/// whole of a topology candidate, which is exactly when a surface can vanish.
#[test]
fn a_cause_naming_a_withdrawn_surface_is_not_submitted() {
    let output = OutputId::from_raw(1);
    let live = SurfaceId::new(41, 1);
    let gone = SurfaceId::new(42, 1);
    let bounds = sophia_protocol::Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    };
    let scene = sophia_protocol::PolicySceneSnapshot {
        generation: 1,
        active_output: output,
        outputs: vec![sophia_protocol::PolicyOutputSnapshot {
            policy_key: None,
            output,
            generation: 1,
            focus: Some(live),
            bounds,
            work_area: bounds,
        }],
        surfaces: vec![sophia_protocol::PolicySurfaceSnapshot {
            surface: live,
            generation: 1,
            current_output: Some(output),
            kind: sophia_protocol::PolicySurfaceKind::Toplevel,
            capabilities: sophia_protocol::LayoutNodeCapabilities::STANDARD_TOPLEVEL,
            constraints: sophia_protocol::SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            exact_size: None,
            requested_state: sophia_protocol::PolicyPresentationState::default(),
            current_state: sophia_protocol::PolicyPresentationState::default(),
            transient_owner: None,
            geometry: bounds,
        }],
        session_operations: Vec::new(),
    };

    // A scene-wide cause names no subject, so it always survives.
    assert!(policy_cause_subject_is_live(
        sophia_protocol::PolicyRequestCause::SceneChanged,
        &scene
    ));

    assert!(policy_cause_subject_is_live(
        sophia_protocol::PolicyRequestCause::Focus { target: live },
        &scene
    ));
    assert!(!policy_cause_subject_is_live(
        sophia_protocol::PolicyRequestCause::Focus { target: gone },
        &scene
    ));
}

#[path = "application_lease_routing.rs"]
mod application_lease_routing;

#[path = "launcher_input_routing.rs"]
mod launcher_input_routing;

#[path = "window_allocation.rs"]
mod window_allocation;

#[path = "pointer_focus.rs"]
mod pointer_focus_tests;

#[path = "pointer_focus_handoff_routing.rs"]
mod pointer_focus_handoff_routing;

#[path = "live_session/input_recovery_tests.rs"]
mod input_recovery_tests;

#[path = "live_session/socket_directory_tests.rs"]
mod socket_directory_tests;

#[test]
fn a_terminal_that_dies_while_the_session_quiesces_is_not_the_session_s_failure() {
    // The QEMU session gate stayed red from 2026-08-28 on exactly this.
    // Quiescence closes the X server its terminals are connected to, so xterm
    // loses the connection and exits 84 with `fatal IO error 11` -- the
    // shutdown working, reported as a client fatal, which turned every
    // deliberate tick-limit exit into session_failure failure_code=
    // unclassified with cleanup_errors=0 on the next line.
    assert!(
        !terminal_exit_is_session_failure(false, false, true),
        "a client the session is shutting down may exit non-zero"
    );

    // AND IT IS STILL A FAULT WHILE THE SESSION IS LIVE, which is the whole
    // of the difference. Nothing else about the decision moves.
    assert!(
        terminal_exit_is_session_failure(false, false, false),
        "a client that dies during a live session is still fatal"
    );
    assert!(
        !terminal_exit_is_session_failure(true, false, false),
        "a clean exit is never a failure"
    );
    assert!(
        !terminal_exit_is_session_failure(false, true, false),
        "a normal session owns its clients' exits"
    );
}
