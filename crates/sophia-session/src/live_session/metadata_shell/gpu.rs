use sophia_backend_live::LiveRenderDeviceIdentitySnapshot;
use sophia_config::ShellGpuMode;
use sophia_runtime::{ProcessLaunchSpec, ProtectionDevice, ProtectionDeviceIdentity};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};

mod sysfs;
use sysfs::RevalidatedGpuDevice;

pub(super) const GPU_MODE_ENV: &str = "SOPHIA_SHELL_GPU_MODE";
pub(super) const GPU_GRANT_EPOCH_ENV: &str = "SOPHIA_SHELL_GPU_GRANT_EPOCH";
pub(super) const GPU_RENDER_NODE_ENV: &str = "SOPHIA_SHELL_GPU_RENDER_NODE";
pub(super) const GPU_DEVICE_MAJOR_ENV: &str = "SOPHIA_SHELL_GPU_DEVICE_MAJOR";
pub(super) const GPU_DEVICE_MINOR_ENV: &str = "SOPHIA_SHELL_GPU_DEVICE_MINOR";
pub(super) const GPU_PCI_BUS_ID_ENV: &str = "SOPHIA_SHELL_GPU_PCI_BUS_ID";
pub(super) const GPU_PCI_VENDOR_ID_ENV: &str = "SOPHIA_SHELL_GPU_PCI_VENDOR_ID";
pub(super) const GPU_PCI_DEVICE_ID_ENV: &str = "SOPHIA_SHELL_GPU_PCI_DEVICE_ID";
pub(super) const PRIVATE_DRI_DIRECTORY: &str = "/dev/dri";

#[derive(Clone, Debug)]
pub(super) struct ShellGpuLaunchPolicy {
    mode: ShellGpuMode,
    device: Option<LiveRenderDeviceIdentitySnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellGpuLaunchEvidence {
    pub epoch: u64,
    pub major: u32,
    pub minor: u32,
    pub render_node: PathBuf,
    pub pci_bus_id: Option<String>,
    pub pci_vendor_id: Option<u32>,
    pub pci_device_id: Option<u32>,
}

impl ShellGpuLaunchPolicy {
    pub fn new(
        mode: ShellGpuMode,
        device: Option<LiveRenderDeviceIdentitySnapshot>,
    ) -> Result<Self, String> {
        match (mode, device.is_some()) {
            (ShellGpuMode::Denied, false) | (ShellGpuMode::Direct, true) => {
                Ok(Self { mode, device })
            }
            (ShellGpuMode::Denied, true) => {
                Err("a denied shell GPU policy carried a device".into())
            }
            (ShellGpuMode::Direct, false) => {
                Err("direct shell GPU access has no admitted render device".into())
            }
        }
    }

    pub fn prepare(
        &self,
        base: &ProcessLaunchSpec,
        epoch: u64,
    ) -> Result<(ProcessLaunchSpec, Option<ShellGpuLaunchEvidence>), String> {
        self.prepare_with_revalidation(base, epoch, revalidate_identity)
    }

    fn prepare_with_revalidation(
        &self,
        base: &ProcessLaunchSpec,
        epoch: u64,
        revalidate: impl FnOnce(
            &LiveRenderDeviceIdentitySnapshot,
        ) -> Result<RevalidatedGpuDevice, String>,
    ) -> Result<(ProcessLaunchSpec, Option<ShellGpuLaunchEvidence>), String> {
        if epoch == 0 {
            return Err("shell GPU grant epoch must be nonzero".into());
        }
        if self.mode == ShellGpuMode::Denied {
            return Ok((base.clone(), None));
        }
        let identity = self
            .device
            .as_ref()
            .ok_or("direct shell GPU access has no admitted render device")?;
        let revalidated = revalidate(identity)?;
        let major = revalidated.major;
        let minor = revalidated.minor;
        let pci_bus_id = revalidated.pci_bus_id.clone();
        let pci_ids = revalidated.pci_ids;
        let private_render_node = Path::new(PRIVATE_DRI_DIRECTORY).join(&revalidated.render_name);

        let mut spec = base
            .clone()
            .env(GPU_MODE_ENV, "direct")
            .env(GPU_GRANT_EPOCH_ENV, epoch.to_string())
            .env(GPU_RENDER_NODE_ENV, &private_render_node)
            .env(GPU_DEVICE_MAJOR_ENV, major.to_string())
            .env(GPU_DEVICE_MINOR_ENV, minor.to_string());
        if let Some(pci_bus_id) = pci_bus_id.as_ref() {
            spec = spec.env(GPU_PCI_BUS_ID_ENV, pci_bus_id);
        }
        if let Some((vendor, device)) = pci_ids {
            spec = spec
                .env(GPU_PCI_VENDOR_ID_ENV, format!("{vendor:04x}"))
                .env(GPU_PCI_DEVICE_ID_ENV, format!("{device:04x}"));
        }
        let domain = spec
            .protection_domain
            .take()
            .ok_or("shell GPU access requires a protection domain")?
            .read_only_filesystem("/sys", revalidated.sysfs)
            .map_err(|error| error.to_string())?
            .device(ProtectionDevice::required_at_exact(
                &identity.node,
                &private_render_node,
                ProtectionDeviceIdentity::new(
                    identity.device,
                    identity.inode,
                    identity.device_number,
                ),
            ))
            .map_err(|error| error.to_string())?;
        spec.protection_domain = Some(domain);
        Ok((
            spec,
            Some(ShellGpuLaunchEvidence {
                epoch,
                major,
                minor,
                render_node: private_render_node,
                pci_bus_id,
                pci_vendor_id: pci_ids.map(|ids| ids.0),
                pci_device_id: pci_ids.map(|ids| ids.1),
            }),
        ))
    }

    pub fn replace_device(&mut self, device: Option<LiveRenderDeviceIdentitySnapshot>) -> bool {
        if self.mode == ShellGpuMode::Denied || self.device == device {
            return false;
        }
        self.device = device;
        true
    }
}

fn revalidate_identity(
    identity: &LiveRenderDeviceIdentitySnapshot,
) -> Result<RevalidatedGpuDevice, String> {
    if !render_node_identity_matches(identity) {
        return Err("shell render node identity changed before launch".into());
    }
    let major = rustix::fs::major(identity.device_number);
    let minor = rustix::fs::minor(identity.device_number);
    let physical = std::fs::canonicalize(format!("/sys/dev/char/{major}:{minor}/device"))
        .map_err(|error| format!("shell render node physical identity: {error}"))?;
    if physical != identity.physical_device {
        return Err("shell render node physical device changed before launch".into());
    }
    let planned = sysfs::plan(identity, major, minor, &physical)?;
    if !render_node_identity_matches(identity)
        || std::fs::canonicalize(format!("/sys/dev/char/{major}:{minor}/device")).ok()
            != Some(physical)
    {
        return Err("shell render node identity changed during discovery".into());
    }
    Ok(planned)
}

fn render_node_identity_matches(identity: &LiveRenderDeviceIdentitySnapshot) -> bool {
    std::fs::symlink_metadata(&identity.node).is_ok_and(|metadata| {
        metadata.file_type().is_char_device()
            && metadata.dev() == identity.device
            && metadata.ino() == identity.inode
            && metadata.rdev() == identity.device_number
    })
}

#[path = "../../../tests/support/metadata_shell_gpu.rs"]
mod tests;
