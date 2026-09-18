//! Shared protected launch construction for legacy and independent components.
pub use super::gpu::ShellGpuLaunchEvidence as ComponentGpuLaunchEvidence;
use super::gpu::ShellGpuLaunchPolicy;
use crate::shell_component_connections::ComponentConnectionKey;
use sophia_backend_live::LiveRenderDeviceIdentitySnapshot;
use sophia_config::{ShellComponentConfig, ShellComponentRole};
use sophia_runtime::{
    ProcessLaunchSpec, ProtectionDomainRole, ProtectionDomainSpec, ProtectionPath,
};
use std::path::{Path, PathBuf};

type LaunchResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Operator-selected launch policy, never negotiated from a client request.
/// Preparation opens no device or display. Per-attempt GPU preparation still
/// performs the existing exact identity revalidation before process launch.
pub struct ShellComponentLaunch {
    selection: ShellComponentConfig,
    panel_thickness: Option<u16>,
    gpu: ShellGpuLaunchPolicy,
}

impl ShellComponentLaunch {
    pub fn new(
        mut selection: ShellComponentConfig,
        panel_thickness: Option<u16>,
        device: Option<LiveRenderDeviceIdentitySnapshot>,
    ) -> LaunchResult<Self> {
        if !selection.executable.is_absolute() {
            return Err("component executable must be absolute".into());
        }
        if selection.role == ShellComponentRole::Bar && panel_thickness.is_none_or(|v| v == 0) {
            return Err("bar component requires a positive panel allowance".into());
        }
        selection.config = selection
            .config
            .as_ref()
            .map(|p| p.canonicalize())
            .transpose()?;
        let gpu = ShellGpuLaunchPolicy::new(selection.gpu, device)?;
        Ok(Self {
            panel_thickness: (selection.role == ShellComponentRole::Bar)
                .then_some(panel_thickness)
                .flatten(),
            selection,
            gpu,
        })
    }

    pub fn selection(&self) -> &ShellComponentConfig {
        &self.selection
    }

    /// Intended for ShellComponentProcesses::start, after that owner reserves
    /// the exact attempt. Returned errors are handled by its retained supervisor.
    pub fn prepare(
        &self,
        key: ComponentConnectionKey,
        socket: &Path,
    ) -> LaunchResult<(ProcessLaunchSpec, Option<ComponentGpuLaunchEvidence>)> {
        if key.grant.connection_epoch == 0 || key.grant.content_grant_epoch == 0 {
            return Err("component launch requires a reserved nonzero grant".into());
        }
        let base = base_launch_spec(
            &self.selection.executable,
            socket,
            self.panel_thickness,
            self.selection.config.as_deref(),
        )?;
        Ok(self.gpu.prepare(&base, key.grant.connection_epoch)?)
    }
}

pub(super) fn base_launch_spec(
    executable: &Path,
    socket: &Path,
    panel_thickness: Option<u16>,
    selected_config: Option<&Path>,
) -> LaunchResult<ProcessLaunchSpec> {
    let parent = socket
        .parent()
        .filter(|p| p.is_absolute())
        .ok_or("metadata shell socket requires an absolute parent")?;
    let mut domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])?
        .path(ProtectionPath::read_only(parent))?;
    let private_config: Option<PathBuf> = selected_config.map(Path::canonicalize).transpose()?;
    if let Some(path) = private_config.as_ref() {
        domain = domain.path(ProtectionPath::read_only(path))?;
    }
    let mut spec = ProcessLaunchSpec::new(executable)
        .arg("--serve")
        .env(sophia_runtime::SOPHIA_SHELL_SOCKET_ENV, socket)
        .process_group()
        .protection_domain(domain);
    if let Some(thickness) = panel_thickness {
        spec = spec.env("SOPHIA_SHELL_BAR_THICKNESS", thickness.to_string());
    }
    if let Some(path) = private_config {
        spec = spec.env("SOPHIA_SHELL_CONFIG", path);
    }
    Ok(spec)
}
