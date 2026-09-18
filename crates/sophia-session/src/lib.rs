mod output;
pub use output::{SessionOutput, install as install_session_output};

macro_rules! session_println {
    ($($argument:tt)*) => {{
        crate::output::stdout(format_args!($($argument)*));
    }};
}
pub(crate) use session_println;

macro_rules! session_eprintln {
    ($($argument:tt)*) => {{
        crate::output::stderr(format_args!($($argument)*));
    }};
}
pub(crate) use session_eprintln;

pub mod application_catalog;
pub mod backend_args;
pub mod backend_evidence;
#[cfg(feature = "native-session")]
pub mod desktop_output_activation;
#[cfg(feature = "native-session")]
pub mod desktop_output_commit;
#[cfg(feature = "native-session")]
pub mod desktop_output_frames;
#[cfg(feature = "native-session")]
pub mod desktop_output_heads;
#[cfg(feature = "native-session")]
pub mod desktop_output_publication;
#[cfg(feature = "native-session")]
pub mod desktop_output_topology;
pub mod desktop_profile_activation;
pub mod diagnostics;
pub mod emergency_input;
pub mod input_delivery;
pub mod input_latency_samples;
pub mod input_proof;
#[cfg(feature = "native-session")]
pub mod live_output_authority;
pub mod native_output_completion;
pub mod resize_transaction;
pub mod resource_sampling;
pub mod session_actions;
pub mod session_control;
pub mod session_keyboard;
pub mod session_shutdown;
pub mod session_startup;
pub mod shell_component_connections;
pub mod shell_component_processes;

#[cfg(feature = "native-session")]
pub mod shell_panel_service {
    pub use crate::live_session::metadata_shell::PanelComponentService;
}

#[cfg(feature = "native-session")]
pub mod shell_native_launcher {
    pub use crate::live_session::metadata_shell::{
        NativeLauncherActionService, NativeLauncherContentService,
    };
}
pub mod support;

#[cfg(feature = "native-session")]
mod live_session;

/// The one published seam a conformance host needs: the projection from a
/// policy indicator publication onto the `sophia_shell_v1` wire snapshot.
///
/// Exported as a single named item rather than by opening `metadata_shell`, so
/// a host shares the production mapping without gaining the transport.
pub mod shell_indicator_publication {
    pub use crate::live_session::metadata_shell::indicators::indicator_snapshot;
}
/// Cadence of the bounded native-session resource evidence population.
#[cfg(feature = "native-session")]
pub const LIVE_RESOURCE_SAMPLE_INTERVAL: std::time::Duration =
    live_session::RESOURCE_SAMPLE_INTERVAL;

/// Maximum native-session resource samples retained before saturation.
#[cfg(feature = "native-session")]
pub const LIVE_RESOURCE_SAMPLE_CAPACITY: u64 = live_session::RESOURCE_SAMPLE_CAPACITY;

#[cfg(feature = "native-session")]
pub fn run_from_args(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    live_session::run_persistent_xterm_session(args)
}

#[cfg(feature = "native-session")]
pub fn run_input_guard(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    live_session::input_guard::run(args)
}

/// Exercise Lom's real protected GPU/content path without acquiring DRM master
/// or claiming native presentation.
#[cfg(feature = "native-session")]
pub fn run_shell_gpu_content_hardware_proof(
    client: &std::path::Path,
    config: &std::path::Path,
    seat: &str,
    render_node: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    live_session::metadata_shell::gpu_content_proof::run(client, config, seat, render_node)
}

#[cfg(feature = "native-session")]
pub fn plan_validation_device<'a>(
    scanout: &'a sophia_backend_live::LiveProductionNativeScanout,
    plan: &desktop_output_topology::NativeOutputActivationPlan,
) -> Option<&'a sophia_backend_live::RealAtomicScanoutCard> {
    live_session::plan_validation_device(scanout, plan)
}

#[allow(unused_imports)]
mod prelude {
    pub(crate) use crate::support::*;

    pub(crate) use sophia_engine::{
        AuthorityTransactionInbox, AuthorityTransactionIntake, CompositorBackendTickInput,
        FrameClockTick, FrameScheduleDecision, HeadlessCompositorBackendAssembly, HeadlessEngine,
        HeadlessSessionDriver, HeadlessSessionDriverTick, LayoutEpochState,
        LiveRuntimeDriverAdapter, LiveRuntimeDriverIntake, WmTransactionUpdate,
        schedule_frame_from_damage,
    };
    pub(crate) use sophia_portal::PortalCommand;
    pub(crate) use sophia_protocol::{
        BrokerHealthPacket, BrokerHealthState, BrokerKind, BufferSource, CommittedSurfaceState,
        DamageFrame, LayerSnapshot, LayoutNodeCapabilities, LayoutNodeKind, LayoutNodeSnapshot,
        LayoutNodeState, NamespaceId, PortalTransferId, Rect, Region, ResizeSyncCapability, Size,
        SurfaceConstraints, SurfaceId, SurfaceTransaction, TransactionCommit, TransactionId,
        TransactionOutcome, Transform, WorkspaceId, decode_broker_health_frame,
        encode_broker_health_frame,
    };
    pub(crate) use sophia_runtime::{
        ProcessLaunchSpec, ProcessSupervisor, RestartPolicy, RuntimeBrokerSupervisors,
        SessionRuntimeCommand, SessionRuntimeLoop, SessionRuntimeObservation,
        SupervisedProcessKind, SupervisorEvent, update_supervisor,
    };
    pub(crate) use sophia_x_authority::{
        X_SOPHIA_PRESENT_EXTENSION_NAME, X_SOPHIA_PRESENT_MAJOR_OPCODE,
        X_SOPHIA_PRESENT_PIXMAP_MINOR_OPCODE, XAuthorityCpuBufferSnapshot,
        XAuthorityCpuBufferUpdate, XAuthorityKeyEvent, XAuthorityObservedTransactionBatch,
        XAuthorityRequestKind, XAuthorityRequestPacket, XByteOrder, XClientOutput, XResourceId,
        XSelectionChangeKind as XAuthoritySelectionChangeKind, read_x_authority_response,
        run_x_authority_socket_server_once, run_x11_core_socket_server_once,
        run_x11_core_socket_server_once_channel, run_x11_core_socket_server_once_channels,
        run_x11_core_socket_server_once_observed,
        run_x11_core_socket_server_once_traced_with_idle_timeout, write_x_authority_request,
        x_fixed_glyph_rows,
    };
    pub(crate) use std::os::unix::net::UnixStream;
    pub(crate) use std::sync::mpsc::{channel, sync_channel};
    pub(crate) use std::time::{Duration, SystemTime, UNIX_EPOCH};
    pub(crate) use x11rb::protocol::Event;
}

pub mod launch_origin;

/// Operator launch policy sharing the live protected-shell construction path.
#[cfg(feature = "native-session")]
pub mod shell_component_launch {
    pub use crate::live_session::metadata_shell::component_launch::{
        ComponentGpuLaunchEvidence, ShellComponentLaunch,
    };
}
