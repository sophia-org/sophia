//! Prepare the protected shell independently of presentation availability.
use super::*;

impl LiveMetadataShell {
    pub(in crate::live_session) fn start(
        executable: &str,
        panel_thickness: Option<u16>,
        content_requested: bool,
        content_input_requested: bool,
        gpu_mode: sophia_config::ShellGpuMode,
        gpu_device: Option<sophia_backend_live::LiveRenderDeviceIdentitySnapshot>,
        selected_config: Option<&std::path::Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut shell = Self::prepare(
            executable,
            panel_thickness,
            content_requested,
            content_input_requested,
            gpu_mode,
            gpu_device,
            selected_config,
        )?;
        shell.presentation_paused = false;
        let (peer_pid, revision, connection_epoch) = shell.launch_and_negotiate()?;
        shell.finish_negotiation(peer_pid, revision, connection_epoch, "startup")?;
        Ok(shell)
    }

    pub(in crate::live_session) fn prepare(
        executable: &str,
        panel_thickness: Option<u16>,
        content_requested: bool,
        content_input_requested: bool,
        gpu_mode: sophia_config::ShellGpuMode,
        gpu_device: Option<sophia_backend_live::LiveRenderDeviceIdentitySnapshot>,
        selected_config: Option<&std::path::Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = std::env::temp_dir().join(format!(
            "sophia-live-metadata-shell-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let transport = sophia_runtime::ShellSessionTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )?;
        let socket = transport.socket_path().to_path_buf();
        let spec = super::component_launch::base_launch_spec(
            std::path::Path::new(executable),
            &socket,
            panel_thickness,
            selected_config,
        )?;
        let gpu = gpu::ShellGpuLaunchPolicy::new(gpu_mode, gpu_device)?;
        let supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, spec.clone());
        let shell = Self {
            content: content::LiveContentSession::new(
                content_requested,
                content_input_requested,
                panel_thickness,
            ),
            tabs: LiveTabSession::default(),
            indicators: indicators::LiveIndicatorState::default(),
            reference: LiveReferenceSession::default(),
            overview: overview::LiveOverviewSession::default(),
            launcher: LiveLauncherSession::default(),
            supervisor,
            base_launch_spec: spec,
            gpu,
            transport,
            slots: BTreeMap::new(),
            next_slot: 1,
            outputs: BTreeMap::new(),
            next_connection_epoch: 1,
            next_snapshot_generation: 1,
            next_projection: 1,
            next_transaction: 1,
            requested: None,
            activating: None,
            pending: None,
            presented: None,
            presented_actions: BTreeMap::new(),
            reservations: sophia_engine::ShellWorkAreaCoordinator::new(),
            reservation_limit: panel_thickness,
            connected: false,
            reconnect_at: None,
            presentation_paused: true,
            revoked_content_grants: RevokedContentGrantLedger::default(),
        };
        Ok(shell)
    }

    pub(super) fn finish_negotiation(
        &mut self,
        peer_pid: u32,
        revision: u16,
        connection_epoch: u64,
        reason: &str,
    ) -> Result<LiveMetadataShellPoll, Box<dyn std::error::Error>> {
        if connection_epoch != self.next_connection_epoch
            || self.transport.connection_epoch() != connection_epoch
        {
            return Err("shell negotiation identity does not match its launch".into());
        }
        self.next_connection_epoch = connection_epoch
            .checked_add(1)
            .ok_or("metadata shell connection epoch exhausted")?;
        self.connected = true;
        self.reconnect_at = None;
        if connection_epoch == 1 {
            crate::session_println!(
                "sophia_live_metadata_shell schema=1 status=ready protected=true peer_pid={peer_pid} revision={revision} connection_epoch={connection_epoch}"
            );
            Ok(LiveMetadataShellPoll::Connected { connection_epoch })
        } else {
            crate::session_println!(
                "sophia_live_metadata_shell schema=1 status=reconnected protected=true peer_pid={peer_pid} revision={revision} connection_epoch={connection_epoch} reason={reason}"
            );
            Ok(LiveMetadataShellPoll::Reconnected { connection_epoch })
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/support/shell_startup.rs"]
mod tests;
