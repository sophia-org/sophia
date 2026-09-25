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
mod component_catalog;
mod component_lifecycle;
mod component_service;
mod cpu_visual_progress;
pub(crate) mod direct_cursor_proof;
pub(crate) mod direct_overlay_proof;
pub(super) mod input_guard;
mod metadata_broker;
pub(crate) mod metadata_shell;
mod socket_directories;
mod startup_barrier;
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
    record_discarded_presents, record_native_present_retirement,
    record_native_software_present_retirement,
};
use policy_transport_worker::{
    PolicyTransportCommand, PolicyTransportEvent, PolicyTransportWorker,
};
use process_supervision::{
    ManagedSessionChild, SessionProcessGuard, managed_child_exit_is_nonfatal, spawn_catalog_child,
    terminate_session_child,
};
use proof_artifacts::{LiveClientStdoutCapture, LiveInputProofResult};
use shutdown::{
    AuthorityIngressState, AuthorityWorkWait, drain_queued_authority_batches,
    observe_authority_ingress, stop_frontend_intake, take_authority_work,
};
use startup_readiness::{
    StartupHeadRequirement, StartupSurfacePresentationEvidence, all_startup_outputs_presented,
    independent_native_output_presented, logical_startup_output_progress,
    logical_synchronous_modeset_records, native_session_exported_pixels, rects_intersect,
    startup_native_recovery_reason, startup_output_evidence, startup_submission_requirement,
    startup_surface_visual_detail,
};
use x_frontend::LiveXAdmissionPolicy;
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
/// The device synthetic XTEST input is attributed to. Never the keyboard or
/// pointer above: evidence must be able to say which events a hand produced.
const SESSION_XTEST_DEVICE_RAW: u64 = 3;
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

include!("live_session/run.rs");
include!("live_session/startup_profiles.rs");

include!("live_session/owner_loop/pointer_evidence.rs");
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

#[path = "../tests/support/live_session.rs"]
mod tests;

include!("live_session/cpu_surface_sample.rs");
