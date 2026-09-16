use super::prelude::*;
mod content_mapping_evidence;
mod native_owner_retirement;
use native_owner_retirement::{NativeRetirement, RetirementMode};

use crate::desktop_output_activation::{
    NativeOutputActivationFailure, NativeOutputActivationSettlement,
    NativeOutputRollbackSettlement, UnavailableNativeOutputExecutor, run_native_output_activation,
};
use crate::desktop_output_commit::NativeOutputTopologyValidationExecutor;
use crate::desktop_output_heads::{
    LiveNativeOutputTopologyHardware, resolve_native_output_topology_heads,
};
use crate::desktop_output_topology::{
    NativeOutputActivationPlan, prepare_native_output_activation_plan,
    prepare_native_output_authority_candidate, project_native_output_topology,
};
use crate::emergency_input::{EmergencyChordAction, EmergencyChordState};
use crate::input_proof::{PhysicalTextProof, PhysicalTextProofEvent};
use crate::native_output_completion::{
    NativeOutputContentEvidence, NativeOutputContentEvidenceError,
    validate_native_output_content_evidence,
};
use crate::resize_transaction::{
    AdmissionRecoveryExtentDecision, PendingLayoutGeometryAuthority, ResizeVisualCommit,
    ResizeVisualCommitTracker, decide_admission_recovery_extent,
    merge_unrequested_layout_observation, project_authority_batch_onto_layout,
};
use crate::session_actions::{SessionLaunchIntent, SessionLaunchQueue, SessionLaunchQueueOutcome};
use crate::session_control::{SESSION_CONTROL_CAPACITY, SessionControlQueue};
use crate::session_keyboard::{
    PhysicalKeyboardCoverage, RuntimeDeadlineKeyDrain, RuntimeDeadlineKeyDrainDecision,
    SESSION_CLIENT_PRESSED_KEY_CAPACITY, SessionClientKeyState, SessionClientPressedKey,
    VirtualTerminalChordAction, VirtualTerminalChordState,
};
use crate::session_shutdown::{
    SessionLogoutDrainDecision, SessionLogoutDrainState, SessionQuiescence,
    SessionQuiescenceDecision, SessionQuiescenceSnapshot, session_logout_drain_decision,
};
use crate::session_startup::{
    SessionStartupEvent, SessionStartupReadiness, reduce_session_startup,
};
use sophia_backend_live::{
    ClassicHardwareCursorUpdate, LiveProductionAuthorityBatch, LiveProductionCpuScene,
    LiveProductionCursorPresentation, LiveProductionCycleRequest, LiveProductionDmaBufRegistration,
    LiveProductionFenceRegistration, LiveProductionNativeScanout, LiveProductionNativeSuspendError,
    LiveProductionRetiredPresent, LiveProductionVisualRuntime,
};
use sophia_engine::{
    ApplicationRouteLeaseCandidate, ApplicationRouteLeasePhase, ApplicationRouteLeaseState,
    ApplicationRouteScope, FocusedInputRoute, InputFocusDecision, InputFocusState, KeyRepeatConfig,
    KeyRepeatState, KeyRepeatTarget, KeyboardFocusHandoffState, LayoutEpochCoordinator,
    NonBlockingInputPoller, OutputFrameServiceRequest, OutputNativeFramePhase,
    PointerFocusHandoffState, WmShortcutRouter,
};
use sophia_protocol::{
    ClientAdmissionContext, DeviceId, NamespaceCapabilities, NamespaceId, NamespaceProfile, Point,
    SeatId, SessionApplicationId, WmActionId, WmSessionAction,
};
use sophia_runtime::NamespaceRegistry;
use sophia_x_authority::{
    XAuthorityClientControlAck, XAuthorityClientControlCommand, XAuthorityClientInputDelivery,
    XAuthorityClientSurfaceRoutes, XAuthorityControlCommand, XAuthorityControlKind,
    XAuthorityInputDeliveryId, XAuthorityInputDeliveryOutcome, XAuthorityRouteLeaseRelease,
    XAuthorityRouteLeaseUpdate, XAuthorityRouteLeaseUpdateKind, XAuthorityRoutedInput,
    XAuthorityRoutedInputMode, XAuthorityRoutedInputSender, XCoreKeyboardMapper,
    XPresentCompletionMode, XServerFrontendAdmissionError, XServerFrontendAdmissionPolicy,
    XServerFrontendAdmissionRequest, XServerFrontendAllocatedPixmap, XServerFrontendConfig,
    XServerFrontendControlRouter, XServerFrontendPixmapAllocation,
    XServerFrontendPixmapAllocationError, XServerFrontendPixmapAllocator,
    XServerFrontendProtocolRouter, XServerFrontendRenderDeviceError,
    XServerFrontendRenderDeviceProvider, XServerFrontendRouteBroker,
    XServerFrontendRouteCapacities, XServerFrontendServiceCommand,
    XServerFrontendSetupAuthorization, XkbKeymapSnapshot,
    run_x_server_frontend_routed_until_stopped,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Read, Write};
use std::num::NonZeroUsize;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod authority_file;
mod cpu_visual_progress;
pub(crate) mod direct_cursor_proof;
pub(crate) mod direct_overlay_proof;
pub(super) mod input_guard;
mod metadata_broker;
pub(crate) mod metadata_shell;
use cpu_visual_progress::{CpuVisualProgress, presented_logical_checksum};
use metadata_shell::live_shell_activation_surfaces;
mod native_retirement;
mod native_session_evidence;
mod visual_progress;
use native_session_evidence::{NativeEvidenceSnapshot, NativeSessionEvidence};
mod policy_transport_worker;
mod process_supervision;
mod proof_artifacts;
mod render_devices;
mod shutdown;
mod startup_readiness;
mod x_frontend;

use authority_file::{LiveXAuthorityFile, fill_session_random};
use metadata_broker::LiveMetadataBroker;
use metadata_shell::{LiveMetadataShell, LiveMetadataShellPoll};
use native_retirement::{
    NativePresentRetirementObservation, correlate_physical_input_page_flip,
    record_native_present_retirement, record_native_software_present_retirement,
};
use policy_transport_worker::{
    PolicyTransportCommand, PolicyTransportEvent, PolicyTransportWorker,
};
use process_supervision::{
    ManagedSessionChild, SessionProcessGuard, managed_child_exit_is_nonfatal,
    terminate_session_child,
};
use proof_artifacts::{LiveClientStdoutCapture, LiveInputProofResult};
use shutdown::{
    AuthorityIngressState, AuthorityWorkWait, disconnect_frontend_for_drain,
    drain_queued_authority_batches, observe_authority_ingress, stop_frontend_intake,
    take_authority_work,
};
use startup_readiness::{
    StartupHeadRequirement, StartupSurfacePresentationEvidence, all_startup_outputs_presented,
    independent_native_output_presented, logical_startup_output_progress,
    logical_synchronous_modeset_records, native_session_exported_pixels, rects_intersect,
    startup_native_recovery_reason, startup_output_evidence, startup_submission_requirement,
    startup_surface_visual_detail,
};
use x_frontend::LiveXAdmissionPolicy;
#[cfg(test)]
use x_frontend::{LiveXPixmapAllocator, LiveXRenderDeviceProvider};

include!("live_session/config.rs");
include!("live_session/input.rs");
include!("live_session/input_capacity.rs");
include!("live_session/client_keys.rs");
include!("live_session/policy.rs");
mod window_allocation;
include!("live_session/presentation.rs");
include!("live_session/startup.rs");
include!("live_session/wm.rs");
include!("live_session/control.rs");

const SESSION_AUTHORITY_CAPACITY: usize = 256;
const SESSION_KEY_CAPACITY: usize = 64;
// One accepted Present can emit independent Complete and Idle records. Size
// protocol transport from authority work, not from the smaller input queue.
const SESSION_PRESENT_PROTOCOL_CAPACITY: usize = SESSION_AUTHORITY_CAPACITY * 2;
const SESSION_INPUT_QUIET_MSEC: u64 = 500;
const SESSION_PHYSICAL_SEQUENCE_TIMEOUT_MSEC: u64 = 15_000;
const SESSION_PHYSICAL_PIXEL_TIMEOUT_MSEC: u64 = 5_000;
const SESSION_COMPLETION_TIMEOUT_MSEC: u64 = 5_000;
const SESSION_POLICY_RESPONSE_TIMEOUT_MSEC: u64 = 4_000;
const SESSION_APP_ADMISSION_TIMEOUT_MSEC: u64 = 12_000;
const SESSION_INPUT_DELIVERY_TIMEOUT_MSEC: u64 = 1_000;
const SESSION_QUIESCENCE_TIMEOUT_MSEC: u64 = 2_000;
const SESSION_SEAT_RAW: u64 = 1;
const SESSION_KEYBOARD_DEVICE_RAW: u64 = 1;
const SESSION_POINTER_DEVICE_RAW: u64 = 2;
const PRIMARY_INPUT_PROOF_SCRIPT: &str = r#"printf 'type %s then Return: ' "$1"; IFS= read -r line; umask 077; printf '%s' "$line" > "$2"; printf '\nreceived:%s\n' "$line"; sleep 300"#;
const SECONDARY_POINTER_WITNESS_SCRIPT: &str = r#"saved=$(stty -g); stty raw -echo; printf '\033[?1000h\033[?1006hPointer witness: click here\r\n'; dd bs=1 count=1 >/dev/null 2>&1; printf '\033[?1000l\033[?1006l'; stty "$saved"; printf 'Pointer input received\n'; sleep 300"#;
static NEXT_SESSION_GENERATION: AtomicU64 = AtomicU64::new(1);
static NEXT_POLICY_OPERATION_ISSUER: AtomicU64 = AtomicU64::new(1);

enum SessionPhysicalInput {
    Threaded(sophia_backend_live::ThreadedNativeLibinputEventPoller),
}

impl NonBlockingInputPoller for SessionPhysicalInput {
    fn poll_ready(&mut self) -> std::io::Result<Vec<sophia_protocol::InputEventPacket>> {
        match self {
            Self::Threaded(poller) => poller.poll_ready(),
        }
    }
}

impl SessionPhysicalInput {
    fn stats(&self) -> sophia_backend_live::ThreadedNativeInputStats {
        match self {
            Self::Threaded(poller) => poller.stats(),
        }
    }

    fn policy_report(&self) -> sophia_backend_live::NativeLibinputPolicyReport {
        match self {
            Self::Threaded(poller) => poller.policy_report(),
        }
    }

    fn drain_event_timings(&mut self) -> Vec<sophia_backend_live::ThreadedNativeInputEventTiming> {
        match self {
            Self::Threaded(poller) => poller.drain_event_timings(),
        }
    }

    fn take_acquisition_saturation(&mut self) -> Option<sophia_protocol::CapacitySaturationReport> {
        match self {
            Self::Threaded(poller) => poller.take_acquisition_saturation(),
        }
    }
}

/// The one DRM device every enabled output in a plan is driven by, if there is one.
///
/// An atomic request reaches exactly one device, so a topology spanning two cards
/// cannot be validated as a unit. Returning `None` for that case keeps startup from
/// validating a fragment and reporting the answer as if it covered the desktop.
pub(super) fn plan_validation_device<'a>(
    scanout: &'a LiveProductionNativeScanout,
    plan: &NativeOutputActivationPlan,
) -> Option<&'a sophia_backend_live::RealAtomicScanoutCard> {
    let mut device: Option<&sophia_backend_live::RealAtomicScanoutCard> = None;
    for target in plan.targets() {
        if !target.requested().enabled {
            continue;
        }
        let card = scanout.card(scanout.primary_head_index(target.output())?);
        match device {
            Some(existing) if !std::ptr::eq(existing, card) => return None,
            Some(_) => {}
            None => device = Some(card),
        }
    }
    device
}

fn open_session_physical_input(
    config: &PersistentXtermSessionConfig,
    device_map: sophia_backend_live::NativeLibinputDeviceMap,
    seat_opener: Option<sophia_backend_live::LiveSeatDeviceOpener>,
) -> Result<Option<SessionPhysicalInput>, Box<dyn std::error::Error>> {
    if !config.input_devices.is_empty() {
        return Ok(Some(SessionPhysicalInput::Threaded(
            sophia_backend_live::open_threaded_native_libinput_path_poller_with_pointer_policy(
                &config.input_devices,
                device_map,
                64,
                256,
                config.native_pointer_policy(),
            )?,
        )));
    }
    config
        .input_seat
        .as_deref()
        .map(|seat_name| {
            if let Some(opener) = seat_opener {
                sophia_backend_live::open_threaded_native_libinput_udev_poller_with_seat_and_pointer_policy(
                    seat_name,
                    device_map,
                    64,
                    256,
                    opener,
                    config.native_pointer_policy(),
                )
            } else {
                sophia_backend_live::open_threaded_native_libinput_udev_poller_with_pointer_policy(
                    seat_name,
                    device_map,
                    64,
                    256,
                    config.native_pointer_policy(),
                )
            }
            .map(SessionPhysicalInput::Threaded)
            .map_err(|error| error.into())
        })
        .transpose()
}

pub(crate) fn run_persistent_xterm_session(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    // Answer whether these arguments would be accepted, and stop.
    //
    // Every check `from_args` performs runs, and nothing else does: no DRM, no
    // input seat, no display manager stopped. Three physical runs died in this
    // function's first line with the display manager already down, because
    // nothing asked the question while it was still cheap to ask.
    let validate_only = args.iter().any(|arg| arg == "--validate-session-args");
    let args = if validate_only {
        args.iter()
            .filter(|arg| *arg != "--validate-session-args")
            .cloned()
            .collect::<Vec<_>>()
    } else {
        args.to_vec()
    };
    let args = args.as_slice();
    let mut config = PersistentXtermSessionConfig::from_args(args)?;
    if validate_only {
        crate::session_println!(
            "sophia_live_session_args schema=1 status=accepted arguments={}",
            args.len(),
        );
        return Ok(());
    }
    if let Ok(profile_mode) = std::env::var("SOPHIA_HAGIA_PROFILE_MODE") {
        if !matches!(
            profile_mode.as_str(),
            "user" | "system" | "explicit" | "packaged-fallback" | "packaged-promotion"
        ) {
            return Err("SOPHIA_HAGIA_PROFILE_MODE has an invalid value".into());
        }
        let profile_sha256 = std::env::var("SOPHIA_DESKTOP_PROFILE_SHA256")?;
        if profile_sha256.len() != 64
            || !profile_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("SOPHIA_DESKTOP_PROFILE_SHA256 must be lowercase SHA-256".into());
        }
        crate::session_println!(
            "sophia_live_desktop_profile schema=1 status=loaded mode={} generation={} digest={} root_sha256={} sources={}",
            profile_mode,
            config.desktop_profile.generation.raw(),
            config.desktop_profile.digest,
            profile_sha256,
            config.desktop_profile.sources.len()
        );
    }
    crate::session_println!(
        "sophia_session_profile schema=1 status=loaded role=desktop generation={} digest={}",
        config.desktop_profile.generation.raw(),
        config.desktop_profile.digest
    );
    crate::session_println!(
        "sophia_session_profile schema=1 status=loaded role=core generation={} digest={}",
        config.core_config_state.active().generation.raw(),
        config.core_config_state.active().digest
    );
    let prepared_public_launch = LiveWmSession::prepare_public_launch(&mut config)?;
    let public_policy_launch =
        LiveWmSession::activate_public_launch(&mut config, prepared_public_launch)?;
    let terminal = if config.client.is_none() {
        Some(crate::support::resolve_external_probe_binary(
            "xterm",
            &config.terminal,
        )?)
    } else {
        None
    };
    prepare_display_socket(&config.socket_path)?;
    let display_number = parse_display_number(&config.display)?;
    let (mut xauthority, xauthority_cookie) = LiveXAuthorityFile::create(display_number)?;
    let mut seat_controller = config
        .native_scanout
        .then(sophia_backend_live::LiveSeatController::open)
        .transpose()?;
    if let Some(controller) = seat_controller.as_mut() {
        let _ = controller.dispatch()?;
        crate::session_println!(
            "sophia_live_seat schema=1 status=active seat={}",
            controller.name()
        );
    }
    // The grouping is what makes mirroring happen at all: connectors named by a
    // `mirror` directive share one logical output, and without it every connector
    // is its own. It is fixed for the session's life because it comes from the
    // profile loaded at startup, so it is built once and reused by every rebuild
    // below -- a rescan that regrouped differently would change the desktop's
    // identity behind policy's back.
    let mirror_grouping = sophia_backend_live::NativeMirrorGrouping::new(
        config.output_profile.current().mirror_groups(),
    )
    .map_err(|error| format!("configured mirror grouping is invalid: {error:?}"))?;
    let initial_head_mapping = match config.output_profile.current().mirror_fit() {
        Some(sophia_config::DesktopMirrorFit::Cover) => sophia_protocol::OutputHeadMapping::Cover,
        Some(sophia_config::DesktopMirrorFit::Exact) => sophia_protocol::OutputHeadMapping::Exact,
        Some(sophia_config::DesktopMirrorFit::Fit) | None => {
            sophia_protocol::OutputHeadMapping::Fit
        }
    };
    // Subscribe before inventory discovery so a device change during startup
    // remains queued for authoritative reconstruction in the owner loop.
    let mut output_topology_monitor = config
        .native_scanout
        .then(sophia_backend_live::LiveDrmTopologyMonitor::open)
        .transpose()?;
    if let (Some(monitor), Some(controller)) =
        (output_topology_monitor.as_mut(), seat_controller.as_ref())
    {
        monitor.initialize_render_inventory(controller.device_opener().name())?;
    }
    let mut native_scanout = seat_controller
        .as_ref()
        .map(|controller| {
            LiveProductionNativeScanout::new_with_seat_mirroring_mapping_and_cursor(
                &controller.device_opener(),
                &mirror_grouping,
                initial_head_mapping,
                config.cursor_resolution.asset.clone(),
            )
        })
        .transpose()?;
    let mut output_authority_capabilities = None;
    let mut startup_output_activation = None;
    if let Some(native) = native_scanout.as_ref() {
        let capabilities = native.output_capabilities()?;
        for capability in &capabilities {
            let mode = capability.selected_mode();
            // The one place the opaque head id is printed beside its connector
            // name: later per-head evidence carries only `head=`, and physical
            // verifiers correlate through this mapping line.
            let head = native
                .head_index_for_native_connector(capability.connector_id())
                .map(|index| native.heads[index].head.raw())
                .ok_or_else(|| {
                    format!(
                        "native readiness found no head for connector {}",
                        capability.connector_name()
                    )
                })?;
            crate::session_println!(
                "sophia_live_native_head schema=2 status=ready output={} head={} connector={} connector_id={} mode={}x{} refresh_millihz={} mirrored={}",
                capability.output().raw(),
                head,
                capability.connector_name(),
                capability.connector_id(),
                mode.width,
                mode.height,
                mode.refresh_millihz,
                mirror_grouping.is_mirrored(capability.connector_name()),
            );
        }
        let topology = project_native_output_topology(&capabilities, &native.outputs())?;
        let reconciled = sophia_config::reconcile_desktop_output_candidate(
            config.output_profile.current(),
            &topology,
        )?;
        let activation =
            prepare_native_output_activation_plan(&capabilities, &topology, &reconciled)?;
        let generation = activation.generation().raw();
        let targets = activation.targets().len();
        let focused = activation.focused_output().is_some();
        // The prepared plan drives the real activation phase machine, and the test
        // phase now reaches hardware: the candidate is resolved into topology heads
        // and submitted as one TEST_ONLY request, so the kernel judges the whole
        // desktop. Startup still performs no KMS mutation, because a validation
        // executor has no apply. What it settles as is now evidence about the
        // topology rather than evidence that nothing was attempted.
        let hardware = LiveNativeOutputTopologyHardware::new(native, &capabilities);
        let resolved = resolve_native_output_topology_heads(&activation, &capabilities, &hardware);
        let (report, executor, validation) = match &resolved {
            Ok(heads) => match plan_validation_device(native, &activation) {
                Some(card) => {
                    let mut executor =
                        NativeOutputTopologyValidationExecutor::new(card, heads.heads());
                    let report = run_native_output_activation(activation.clone(), &mut executor)?;
                    (report, "topology_validation", executor.validation())
                }
                // One atomic request cannot span two DRM devices, so a topology
                // that does is not validatable as a unit and must not be reported
                // as refused.
                None => (
                    run_native_output_activation(
                        activation.clone(),
                        &mut UnavailableNativeOutputExecutor,
                    )?,
                    "multi_device_unvalidatable",
                    "not_attempted",
                ),
            },
            Err(error) => {
                tracing::warn!(
                    schema = 1,
                    %error,
                    "native desktop output candidate could not be resolved into heads"
                );
                (
                    run_native_output_activation(
                        activation.clone(),
                        &mut UnavailableNativeOutputExecutor,
                    )?,
                    "unresolved",
                    "not_attempted",
                )
            }
        };
        let (status, phase, cause) = match report.settlement {
            NativeOutputActivationSettlement::Activated { .. } => ("applied", "activated", "none"),
            NativeOutputActivationSettlement::Rejected {
                cause, rollback, ..
            } => (
                "prepared_not_applied",
                match rollback {
                    NativeOutputRollbackSettlement::Failed(_) => "recovery_failed",
                    _ => "rejected",
                },
                match cause {
                    NativeOutputActivationFailure::Invalidated => "invalidated",
                    NativeOutputActivationFailure::Rejected => "rejected",
                    NativeOutputActivationFailure::WouldBlock => "would_block",
                    NativeOutputActivationFailure::TimedOut => "timed_out",
                    NativeOutputActivationFailure::Disconnected => "disconnected",
                },
            ),
        };
        tracing::info!(
            schema = 1,
            status,
            phase,
            cause,
            executor,
            validation,
            generation,
            outputs = targets,
            rollback_targets = targets,
            focused,
            "native desktop output candidate admitted"
        );
        if validation == "accepted" {
            startup_output_activation = Some(activation);
        }
        output_authority_capabilities = Some(capabilities.clone());
    }
    validate_prepared_output_proof_candidate(
        config.output_proof_rollback_after_apply,
        startup_output_activation.is_some(),
    )?;
    let device_map =
        sophia_backend_live::NativeLibinputDeviceMap::new(SeatId::from_raw(SESSION_SEAT_RAW))
            .with_keyboard_device(DeviceId::from_raw(SESSION_KEYBOARD_DEVICE_RAW))
            .with_pointer_device(DeviceId::from_raw(SESSION_POINTER_DEVICE_RAW));
    let mut physical_input = open_session_physical_input(
        &config,
        device_map,
        seat_controller
            .as_ref()
            .map(sophia_backend_live::LiveSeatController::device_opener),
    )?;
    if let Some(physical_input) = physical_input.as_ref() {
        let policy = physical_input.policy_report();
        crate::session_println!(
            "sophia_live_session_input_pipeline schema=4 status=poller_ready source={} seat={} devices={} active={} keyboards={} pointers={} touch={} tap_capable={} tap_enabled={} pointer_configured={} settings_unsupported={}",
            if policy.udev_managed { "udev" } else { "paths" },
            config.input_seat.as_deref().unwrap_or("explicit"),
            policy.devices_added,
            policy.active_devices,
            policy.keyboards,
            policy.pointers,
            policy.touch_devices,
            policy.tap_capable,
            policy.tap_enabled,
            policy.pointer_configured,
            // A preference a device did not have is skipped, not refused. It is
            // counted here so the skip is visible rather than silent.
            policy.pointer_settings_unsupported
        );
        std::io::stdout().flush()?;
    }
    let initial_outputs = native_scanout
        .as_ref()
        .map(LiveProductionNativeScanout::outputs)
        .unwrap_or_else(|| vec![sophia_engine::HeadlessOutput::deterministic()]);
    let output_authority_bootstrap = if public_policy_launch.is_some() {
        match (
            output_authority_capabilities.take(),
            native_scanout.as_ref(),
        ) {
            (Some(capabilities), Some(native)) => {
                let snapshot = native.output_authority_snapshot(1)?;
                let startup_candidate = startup_output_activation
                    .as_ref()
                    .map(|plan| {
                        prepare_native_output_authority_candidate(
                            plan,
                            &capabilities,
                            &snapshot,
                            initial_head_mapping,
                        )
                    })
                    .transpose()?;
                Some(LiveOutputAuthorityBootstrap {
                    snapshot,
                    capabilities,
                    startup_candidate,
                })
            }
            (None, _) => None,
            (Some(_), None) => {
                return Err("output authority capabilities lost their native owner".into());
            }
        }
    } else {
        None
    };
    let mut scripting = LiveControlState::start(&mut config);
    let mut wm_session = LiveWmSession::from_config(
        &config,
        &initial_outputs,
        public_policy_launch,
        output_authority_bootstrap,
    )?;
    let policy_map_mode = LivePolicyMapMode::from_external_wm(wm_session.is_some());
    let output_topology = output_topology_from_engine_outputs(&initial_outputs)?;

    let server_path = config.socket_path.clone();
    let session_generation = NEXT_SESSION_GENERATION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |generation| {
            generation.checked_add(1)
        })
        .map_err(|_| "Sophia session generation exhausted")?;
    let namespace_registry = Arc::new(Mutex::new(NamespaceRegistry::new(session_generation)?));
    let x_namespace = namespace_registry
        .lock()
        .map_err(|_| "Sophia namespace registry lock was poisoned")?
        .create_namespace(config.namespace_profile, config.namespace_capabilities);
    let session_user_id = rustix::process::geteuid().as_raw();
    let launch_origins = wm_session
        .as_ref()
        .and_then(|wm| wm.public.as_ref())
        .map(|p| p.launch_origins.clone())
        .unwrap_or_default();
    let admission_policy = Arc::new(LiveXAdmissionPolicy {
        launch_origins: launch_origins.clone(),
        registry: namespace_registry.clone(),
        namespace: x_namespace.id,
        session_user_id,
    });
    let mut frontend_config =
        XServerFrontendConfig::new_with_namespace_context(&server_path, x_namespace)?
            .with_output_topology(output_topology.clone())?
            .with_xkb_config(config.xkb_config.clone())?
            .with_setup_authorization(XServerFrontendSetupAuthorization::MitMagicCookie(
                xauthority_cookie,
            ))
            // XLibre maps immediately unless a redirecting policy owner is
            // present. Deferring without a WM strands the client's toplevel
            // before MapNotify, VisibilityNotify, and Expose.
            .with_policy_map_deferred(policy_map_mode.frontend_deferred())
            .with_admission_policy(admission_policy);
    let mut client_render_devices = None;
    if !config.software_client_rendering
        && let Some(native_scanout) = native_scanout.as_ref()
    {
        let seat = seat_controller
            .as_ref()
            .ok_or("native client device lost its seat")?
            .device_opener()
            .name()
            .to_owned();
        let (bundle, coordinator) = render_devices::initial(native_scanout, &seat)?;
        frontend_config = frontend_config.with_device_bundle(bundle);
        client_render_devices = Some(coordinator);
    }
    let (authority_sender, authority_receiver) = sync_channel(SESSION_AUTHORITY_CAPACITY);
    let (control_ack_sender, control_ack_receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    // Completion notifications must never kill an X11 writer merely because
    // another client filled a shared acknowledgement queue while the owner was
    // committing a WM transaction. Routed input itself remains bounded.
    let (input_delivery_sender, input_delivery_receiver) = channel();
    let (route_lease_update_sender, route_lease_update_receiver) =
        sync_channel(SESSION_CONTROL_CAPACITY);
    let (explicit_pointer_grab_client, explicit_pointer_grab_owner) =
        sophia_x_authority::x_authority_explicit_pointer_grab_bridge(
            NonZeroUsize::new(SESSION_CONTROL_CAPACITY)
                .expect("session explicit pointer-grab capacity is nonzero"),
        );
    let mut broker = XServerFrontendRouteBroker::with_route_capacities_xkb_and_lease_updates(
        XServerFrontendRouteCapacities::new(
            NonZeroUsize::new(SESSION_KEY_CAPACITY)
                .expect("session input route capacity is nonzero"),
            NonZeroUsize::new(SESSION_CONTROL_CAPACITY)
                .expect("session control route capacity is nonzero"),
            NonZeroUsize::new(SESSION_PRESENT_PROTOCOL_CAPACITY)
                .expect("session protocol route capacity is nonzero"),
            NonZeroUsize::new(SESSION_KEY_CAPACITY)
                .expect("session presentation route capacity is nonzero"),
        ),
        control_ack_sender,
        input_delivery_sender,
        route_lease_update_sender,
        config.xkb_config.clone(),
    )?
    .with_explicit_pointer_grab_client(explicit_pointer_grab_client);
    let metadata_candidate_receiver = broker
        .take_metadata_candidate_receiver()
        .ok_or("X frontend omitted its reduced metadata route")?;
    crate::session_println!(
        "sophia_live_x11_route_capacity schema=1 input={} control={} protocol={} presentations={}",
        SESSION_KEY_CAPACITY,
        SESSION_CONTROL_CAPACITY,
        SESSION_PRESENT_PROTOCOL_CAPACITY,
        SESSION_KEY_CAPACITY,
    );
    let input_sender = broker.routed_input_sender();
    let route_lease_release_sender = broker.route_lease_release_sender();
    let control_sender = broker.control_router();
    let raster_sender = broker.raster_router();
    let protocol_router = broker.protocol_router();
    let (service_command_sender, service_command_receiver) = sync_channel(1);
    let mut server = Some(std::thread::spawn(move || {
        run_x_server_frontend_routed_until_stopped(
            frontend_config,
            authority_sender,
            broker,
            service_command_receiver,
        )
    }));
    wait_for_x_server_socket(&config.socket_path, &mut server)?;
    let mut metadata_broker = config
        .wm_process
        .is_some()
        .then(LiveMetadataBroker::start)
        .transpose()?;
    let mut metadata_shell = config
        .shell_process
        .as_deref()
        .map(|process| {
            let gpu_device = match config.shell_gpu_mode {
                sophia_config::ShellGpuMode::Denied => None,
                sophia_config::ShellGpuMode::Direct => Some(
                    client_render_devices
                        .as_ref()
                        .ok_or("direct shell GPU access requires native client rendering")?
                        .shell_gpu_device()?,
                ),
            };
            let construct = if native_scanout.is_some() {
                LiveMetadataShell::prepare
            } else {
                LiveMetadataShell::start
            };
            construct(
                process,
                config.shell_panel_thickness,
                config.shell_content_enabled,
                config.shell_content_input_enabled,
                config.shell_gpu_mode,
                gpu_device,
                config.shell_config.as_deref(),
            )
        })
        .transpose()?;

    let input_proof_result = (config.input_proof_requested() && config.client.is_none())
        .then(|| LiveInputProofResult::create(display_number))
        .transpose()?;
    let normal_primary = config
        .normal_session
        .then(|| {
            config.applications.startup.first().map(|id| {
                config
                    .applications
                    .applications
                    .get(id)
                    .expect("normal session startup application was validated")
            })
        })
        .flatten();
    let mut terminal_command = match (normal_primary, config.client.as_deref()) {
        (Some(app), _) => Some(std::process::Command::new(&app.executable)),
        (None, _) if config.normal_session => None,
        (None, Some(client)) => Some(application_client_command(client)),
        (None, None) => Some(std::process::Command::new(
            terminal.as_deref().expect("xterm executable is resolved"),
        )),
    };
    let (client_stdout_capture, client_stdout_file) = if config.client.is_some() {
        let (capture, file) = LiveClientStdoutCapture::create(display_number)?;
        (Some(capture), Some(file))
    } else {
        (None, None)
    };
    if let Some(terminal_command) = terminal_command.as_mut() {
        configure_control_environment(terminal_command, config.control_socket.as_deref());
        terminal_command
            .env("DISPLAY", &config.display)
            .env("XAUTHORITY", xauthority.path())
            .env_remove("ENV")
            .env_remove("BASH_ENV")
            .stdin(Stdio::null())
            .stderr(Stdio::inherit());
        if let Some(app) = normal_primary {
            terminal_command
                .args(&app.arguments)
                .process_group(0)
                .stdout(Stdio::inherit());
        } else if config.client.is_some() {
            terminal_command
                .env("GDK_BACKEND", "x11")
                .env("GTK_USE_PORTAL", "0")
                .env_remove("WAYLAND_DISPLAY")
                .args(&config.client_args)
                .stdout(Stdio::from(
                    client_stdout_file.expect("application stdout file was created"),
                ));
        } else {
            terminal_command
                .args([
                    "-cm",
                    "-dc",
                    "-geometry",
                    "120x36+80+60",
                    "-title",
                    "Sophia Terminal",
                ])
                .stdout(Stdio::inherit());
        }
        if let Some(result) = input_proof_result.as_ref() {
            terminal_command.env("SOPHIA_INPUT_PROOF_RESULT", result.path());
        }
        if config.client.is_none()
            && let Some(proof_text) = config
                .inject_text
                .as_deref()
                .or(config.expect_physical_text.as_deref())
        {
            terminal_command
                .args([
                    "-e",
                    "sh",
                    "-c",
                    PRIMARY_INPUT_PROOF_SCRIPT,
                    "sophia-input-proof",
                ])
                .arg(proof_text)
                .arg(
                    input_proof_result
                        .as_ref()
                        .expect("input proof result exists with proof text")
                        .path(),
                );
        } else if let Some(program) = config.terminal_exec.as_deref() {
            terminal_command
                .env_remove("ENV")
                .env_remove("BASH_ENV")
                .arg("-e")
                .arg(program)
                .args(&config.terminal_exec_args);
        }
    }
    let child = match terminal_command
        .map(|mut command| command.spawn())
        .transpose()
    {
        Ok(child) => child,
        Err(error) if !config.startup_proof_requested() => {
            crate::session_eprintln!(
                "sophia_session_app schema=2 status=failed source=startup reason=spawn error={error}"
            );
            None
        }
        Err(error) => return Err(error.into()),
    };
    if child.is_some()
        && let Some(app) = normal_primary
    {
        crate::session_println!(
            "sophia_session_app schema=1 status=started id={} source=startup",
            app.id
        );
    }
    let mut process = SessionProcessGuard::new(
        child,
        Vec::new(),
        config.socket_path.clone(),
        config.normal_session,
    );
    // Admit one primary-client transaction before launching the secondary
    // proof client. Otherwise optimized startup lets both xterms race for the
    // first committed surface, making initial focus nondeterministic.
    let initial_authority_batch = if config.startup_proof_requested()
        && (config.secondary_terminal || config.applications.startup.len() > 1)
    {
        Some(
            authority_receiver
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| {
                    format!("primary xterm did not publish a startup frame: {error}")
                })?,
        )
    } else {
        None
    };
    if config.secondary_terminal {
        process.add_secondary_child(
            None,
            spawn_secondary_xterm(
                terminal
                    .as_deref()
                    .expect("secondary terminal requires xterm"),
                &config.display,
                xauthority.path(),
                config
                    .inject_text
                    .as_deref()
                    .or(config.expect_physical_text.as_deref()),
            )?,
        );
    }
    for id in config.applications.startup.iter().skip(1) {
        let app = config
            .applications
            .applications
            .get(id)
            .expect("normal session startup application was validated");
        match PersistentXtermSessionConfig::spawn_session_application(
            app,
            &config.display,
            xauthority.path(),
            config.control_socket.as_deref(),
        ) {
            Ok(child) => {
                process.add_secondary_child(Some(app.id.clone()), child);
                crate::session_println!(
                    "sophia_session_app schema=1 status=started id={} source=startup",
                    app.id
                );
            }
            Err(error) if !config.startup_proof_requested() => {
                crate::session_eprintln!(
                    "sophia_session_app schema=2 status=failed id={} source=startup reason=spawn error={error}",
                    app.id
                );
            }
            Err(error) => return Err(error),
        }
    }

    let mut randr_witness = config
        .inject_output_size
        .map(|_| open_randr_update_witness(&config.socket_path, xauthority_cookie))
        .transpose()?;
    let mut output_notifications = 0usize;
    if let Some(size) = config.inject_output_size {
        let mut snapshot = output_topology.clone();
        snapshot.generation = snapshot.generation.saturating_add(1);
        let primary_id = snapshot.primary;
        let primary = snapshot
            .outputs
            .iter_mut()
            .find(|entry| entry.output == primary_id)
            .ok_or("live output injection lost the primary output")?;
        primary.logical.width = size.width;
        primary.logical.height = size.height;
        primary.pixel_size = size;
        snapshot
            .validate()
            .map_err(|error| format!("invalid --inject-output-size topology: {error:?}"))?;
        let (ack_sender, ack_receiver) = sync_channel(1);
        service_command_sender.send(XServerFrontendServiceCommand::UpdateOutputTopology {
            snapshot,
            acknowledgement: ack_sender,
        })?;
        let outcome = ack_receiver.recv_timeout(Duration::from_secs(1))?;
        let notifications = match outcome {
            sophia_x_authority::XAuthorityOutputUpdateOutcome::Applied {
                notifications, ..
            } => notifications,
            outcome => {
                return Err(format!("live output injection was rejected: {outcome:?}").into());
            }
        };
        output_notifications = notifications;
        let witness = randr_witness
            .as_mut()
            .ok_or("live output injection lost its RandR witness")?;
        confirm_randr_update_witness(witness, size)?;
        crate::session_println!(
            "sophia_live_output_update schema=3 status=applied width={} height={} notifications={} witness=true",
            size.width,
            size.height,
            notifications
        );
    }

    crate::session_println!(
        "sophia_live_session_mode schema=1 mode={} configured_apps={} startup_apps={}",
        if config.normal_session {
            "normal"
        } else {
            "proof"
        },
        config.applications.applications.len(),
        config.applications.startup.len(),
    );

    crate::session_println!(
        "sophia_live_session schema=7 status=running display={} terminal=xterm runtime=persistent authority_capacity={} input_capacity={} control_capacity={} native_presentation={} physical_input={} pointer_proof={} secondary_terminal={} wm_policy={} namespace_profile={} namespace_request_capabilities={} namespace_publish_capabilities={}",
        config.display,
        SESSION_AUTHORITY_CAPACITY,
        SESSION_KEY_CAPACITY,
        SESSION_CONTROL_CAPACITY,
        if native_scanout.is_some() {
            "enabled"
        } else {
            "disabled"
        },
        if physical_input.is_some() {
            "enabled"
        } else {
            "disabled"
        },
        if config.expect_physical_pointer {
            "enabled"
        } else {
            "disabled"
        },
        if config.secondary_terminal {
            "enabled"
        } else {
            "disabled"
        },
        if wm_session.is_some() {
            "external"
        } else {
            "disabled"
        },
        match config.namespace_profile {
            NamespaceProfile::ClassicShared => "classic_shared",
            NamespaceProfile::Confined => "confined",
        },
        config.namespace_capabilities.request_bits(),
        config.namespace_capabilities.publish_bits(),
    );
    // Says whether the post-input pixel and text proofs were asked for at all.
    // Without it their results are two facts in one field: the completion line
    // reports `input_pixel_change=false input_text_match=false` both when a
    // configured proof failed and when no proof was ever configured, and a
    // reader of a session that drives neither -- the mixed-output gate, say --
    // cannot tell those apart. `pointer_proof` already draws this line for the
    // pointer side.
    crate::session_println!(
        "sophia_live_session_input_proof schema=1 status={}",
        if config.expect_physical_text.is_some() {
            "enabled"
        } else {
            "disabled"
        },
    );
    if !config.startup_proof_requested() {
        crate::session_println!(
            "sophia_live_session schema=1 status=desktop_ready startup_apps={}",
            config.applications.startup.len()
        );
        crate::session_println!("sophia_live_session_startup_proof schema=1 status=not_requested");
    }
    if let Some(native_scanout) = native_scanout.as_ref() {
        crate::session_println!(
            "sophia_live_outputs schema=2 status=ready discovered={} presentation={} native_owned={} multi_output_scanout=enabled layout=extended_horizontal",
            native_scanout.discovered_outputs,
            native_scanout.presentation_outputs,
            native_scanout.heads.len(),
        );
    }

    // These owners outlive both successful and failing owner-loop returns.
    let mut native_retirement = NativeRetirement::default();
    if let Some(native) = native_scanout.as_ref() {
        native_retirement.admit(native)?;
    }
    let mut suspended_renderer_images = None;
    let mut render_owners = LoopRenderOwners {
        seat_active: true,
        ..Default::default()
    };
    let (primary_child, secondary_children) = process.children_mut();
    let result = run_session_loop(
        &mut config,
        SessionLoopChannels {
            authority: &authority_receiver,
            input: &input_sender,
            control: &control_sender,
            raster: &raster_sender,
            control_acknowledgements: &control_ack_receiver,
            input_deliveries: &input_delivery_receiver,
            route_lease_updates: &route_lease_update_receiver,
            route_lease_releases: &route_lease_release_sender,
            explicit_pointer_grabs: &explicit_pointer_grab_owner,
            frontend_service: &service_command_sender,
            metadata_candidates: &metadata_candidate_receiver,
        },
        SessionLoopResources {
            launch_origins: &launch_origins,
            child: primary_child,
            secondary_children,
            physical_input: &mut physical_input,
            native_scanout: &mut native_scanout,
            native_retirement: &mut native_retirement,
            render_owners: &mut render_owners,
            suspended_renderer_images: &mut suspended_renderer_images,
            seat_controller: &mut seat_controller,
            wm_session: &mut wm_session,
            scripting: &mut scripting,
            metadata_broker: &mut metadata_broker,
            metadata_shell: &mut metadata_shell,
            mirror_grouping: &mirror_grouping,
            initial_head_mapping,
        },
        SessionLoopStartup {
            output_topology_monitor,
            client_render_devices,
            xauthority: xauthority.path(),
            protocol_router,
            input_proof_result: input_proof_result.as_ref(),
            client_stdout_capture: client_stdout_capture.as_ref(),
            require_startup_focus: false,
            initial_authority_batch,
            output_notifications,
        },
    );
    let session_error = result.err();
    let mut outer_cleanup_failures = Vec::new();
    drop(randr_witness);
    crate::session_println!("sophia_live_session_lifecycle schema=1 status=stopping_frontend");
    // Stop frontend routing before terminating its clients. Pointer motion can
    // leave a bounded burst in the Engine ingress queue; killing xterm first
    // turns that normal shutdown backlog into a client-queue disconnect.
    let intake_status =
        match service_command_sender.send(XServerFrontendServiceCommand::StopAccepting) {
            Ok(()) => "requested",
            Err(_) => "already_stopped",
        };
    crate::session_println!(
        "sophia_live_session_lifecycle schema=1 status=frontend_intake_stop command={intake_status}"
    );
    drop(input_sender);
    drop(control_sender);
    crate::session_println!("sophia_live_session_lifecycle schema=1 status=stopping_clients");
    if let Err(error) = process.terminate() {
        outer_cleanup_failures.push(format!("session client cleanup failed: {error}"));
    }
    let cancellation_status =
        match service_command_sender.send(XServerFrontendServiceCommand::StopAndDisconnect) {
            Ok(()) => "requested",
            Err(_) => "already_stopped",
        };
    crate::session_println!(
        "sophia_live_session_lifecycle schema=1 status=frontend_cancellation command={cancellation_status}"
    );
    crate::session_println!("sophia_live_session_lifecycle schema=1 status=joining_frontend");
    match server
        .take()
        .expect("X Server Frontend handle is retained after startup")
        .join()
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            outer_cleanup_failures.push(format!("persistent X authority server failed: {error}"))
        }
        Err(_) => {
            outer_cleanup_failures.push("persistent X authority server thread panicked".to_owned())
        }
    }
    crate::session_println!("sophia_live_session_lifecycle schema=1 status=frontend_joined");
    match namespace_registry.lock() {
        Ok(mut registry) => {
            if let Err(error) = registry.revoke_namespace(x_namespace.id) {
                outer_cleanup_failures.push(format!("namespace revocation failed: {error}"));
            }
        }
        Err(_) => {
            outer_cleanup_failures.push("Sophia namespace registry lock was poisoned".to_owned())
        }
    }
    if let Err(error) = xauthority.remove() {
        outer_cleanup_failures.push(format!("X authority cleanup failed: {error}"));
    }
    // All rendering owners survived the loop, including singleton custody in
    // the visual runtime. Earlier errors do not waive their disposition.
    if let Some(shell) = metadata_shell.as_mut()
        && let Err(error) = shell.stop_for_session_shutdown()
    {
        outer_cleanup_failures.push(format!("shell admission shutdown failed: {error}"));
    }
    let native_finish = native_owner_retirement::finish_render_owners(
        &mut render_owners.runtime,
        &mut render_owners.scene,
        &mut native_scanout,
        &mut native_retirement,
        render_owners.seat_active,
        config.native_scanout,
    );
    let native_error = native_finish.err();
    if let Some(error) = native_error.as_ref() {
        outer_cleanup_failures.push(format!("native retirement unresolved: {error}"));
    } else {
        // A failed retirement keeps its handoff in the terminal error carrier.
        // Only established native disposition permits releasing it and then
        // collecting the shell's actual remaining resource consumers.
        drop(suspended_renderer_images.take());
        if let Some(shell) = metadata_shell.as_mut()
            && let Err(error) = shell.finish_content_shutdown(&native_retirement)
        {
            outer_cleanup_failures.push(format!("shell content final cleanup failed: {error}"));
        }
    }
    if outer_cleanup_failures.is_empty() {
        crate::session_println!(
            "sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed"
        );
    }
    if session_error.is_some() || !outer_cleanup_failures.is_empty() {
        let message = format!(
            "{}{}{}",
            session_error
                .as_ref()
                .map_or(String::new(), ToString::to_string),
            if session_error.is_some() && !outer_cleanup_failures.is_empty() {
                "; "
            } else {
                ""
            },
            outer_cleanup_failures.join("; ")
        );
        return Err(Box::new(native_owner_retirement::RetirementFailure::new(
            message,
            native_retirement,
            (
                native_scanout,
                metadata_shell,
                suspended_renderer_images,
                render_owners,
                session_error,
                native_error,
            ),
        )));
    }
    Ok(())
}

include!("live_session/owner_loop/resource_samples.rs");
include!("live_session/owner_loop_state.rs");
include!("live_session/output_topology_owner.rs");
include!("live_session/owner_loop.rs");

/// Builds the topology candidate a reloaded profile asks for.
///
/// The same four steps startup takes, run again against the hardware as it is
/// now: capabilities, the topology they project, the profile reconciled onto
/// it, and the activation plan that becomes a candidate. Running them again
/// rather than reusing startup's plan is deliberate -- a display may have been
/// unplugged since, and a plan built against absent hardware is exactly the
/// kind of thing the candidate preparation is there to refuse.
fn build_reloaded_output_topology_candidate(
    native: &LiveProductionNativeScanout,
    config: &PersistentXtermSessionConfig,
    snapshot: &sophia_protocol::OutputAuthoritySnapshot,
    mapping: sophia_protocol::OutputHeadMapping,
) -> Result<sophia_protocol::OutputTopologyCandidate, Box<dyn std::error::Error>> {
    let capabilities = native.output_capabilities()?;
    let topology = project_native_output_topology(&capabilities, &native.outputs())?;
    let reconciled = sophia_config::reconcile_desktop_output_candidate(
        config.output_profile.current(),
        &topology,
    )?;
    let plan = prepare_native_output_activation_plan(&capabilities, &topology, &reconciled)?;
    Ok(prepare_native_output_authority_candidate(
        &plan,
        &capabilities,
        snapshot,
        mapping,
    )?)
}

mod tests;

#[cfg(test)]
#[path = "../tests/support/mirror_gate_session_config.rs"]
mod mirror_gate_session_config;

#[cfg(test)]
#[path = "../tests/support/live_control.rs"]
mod live_control_tests;

#[cfg(test)]
#[path = "../tests/support/panel_session_config.rs"]
mod panel_session_config;

#[cfg(test)]
#[path = "../tests/support/desktop_composition.rs"]
mod desktop_composition;

include!("live_session/cpu_surface_sample.rs");
