#[cfg(feature = "native-session")]
mod backend;
mod client_launch;
mod config;
pub(crate) mod diagnostics;
mod help;
mod msg;
mod runtime;
mod session_prepare;
mod x_authority;

#[allow(unused_imports)]
mod prelude {
    pub(crate) use sophia_session::support::*;

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

pub(crate) fn run(args: &[String], verbose: bool) -> Result<(), Box<dyn std::error::Error>> {
    if session_prepare::try_run(args)? {
        return Ok(());
    }
    if client_launch::try_run(args)? {
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "msg") {
        std::process::exit(msg::run(&args[1..]));
    }
    if diagnostics::try_run(args)? {
        return Ok(());
    }
    if config::try_run(args)? {
        return Ok(());
    }
    #[cfg(feature = "native-session")]
    if backend::try_run(args)? {
        return Ok(());
    }
    if runtime::try_run(args)? {
        return Ok(());
    }
    if x_authority::try_run(args)? {
        return Ok(());
    }
    help::print(verbose);
    Ok(())
}
