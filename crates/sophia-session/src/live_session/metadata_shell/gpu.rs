use sophia_backend_live::LiveRenderDeviceIdentitySnapshot;
use sophia_config::ShellGpuMode;
use sophia_runtime::{ProcessLaunchSpec, ProtectionDevice};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::Path;

pub(super) const GPU_MODE_ENV: &str = "SOPHIA_SHELL_GPU_MODE";
pub(super) const GPU_GRANT_EPOCH_ENV: &str = "SOPHIA_SHELL_GPU_GRANT_EPOCH";
pub(super) const GPU_RENDER_NODE_ENV: &str = "SOPHIA_SHELL_GPU_RENDER_NODE";
pub(super) const GPU_DEVICE_MAJOR_ENV: &str = "SOPHIA_SHELL_GPU_DEVICE_MAJOR";
pub(super) const GPU_DEVICE_MINOR_ENV: &str = "SOPHIA_SHELL_GPU_DEVICE_MINOR";
pub(super) const GPU_PCI_BUS_ID_ENV: &str = "SOPHIA_SHELL_GPU_PCI_BUS_ID";
pub(super) const GPU_PCI_VENDOR_ID_ENV: &str = "SOPHIA_SHELL_GPU_PCI_VENDOR_ID";
pub(super) const GPU_PCI_DEVICE_ID_ENV: &str = "SOPHIA_SHELL_GPU_PCI_DEVICE_ID";
pub(super) const PRIVATE_RENDER_NODE: &str = "/dev/dri/renderD128";

#[derive(Clone, Debug)]
pub(super) struct ShellGpuLaunchPolicy {
    mode: ShellGpuMode,
    device: Option<LiveRenderDeviceIdentitySnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ShellGpuLaunchEvidence {
    pub epoch: u64,
    pub major: u32,
    pub minor: u32,
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
        ) -> Result<(u32, u32, std::path::PathBuf), String>,
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
        let (major, minor, physical) = revalidate(identity)?;
        let pci_bus_id = pci_bus_id(&physical);
        let pci_ids = pci_bus_id
            .as_ref()
            .map(|_| pci_ids(&physical))
            .transpose()?;

        let mut spec = base
            .clone()
            .env(GPU_MODE_ENV, "direct")
            .env(GPU_GRANT_EPOCH_ENV, epoch.to_string())
            .env(GPU_RENDER_NODE_ENV, PRIVATE_RENDER_NODE)
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
            .device(ProtectionDevice::required_at(
                &identity.node,
                PRIVATE_RENDER_NODE,
            ))
            .map_err(|error| error.to_string())?;
        spec.protection_domain = Some(domain);
        Ok((
            spec,
            Some(ShellGpuLaunchEvidence {
                epoch,
                major,
                minor,
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
) -> Result<(u32, u32, std::path::PathBuf), String> {
    let metadata = std::fs::symlink_metadata(&identity.node)
        .map_err(|error| format!("shell render node metadata: {error}"))?;
    if !metadata.file_type().is_char_device()
        || metadata.dev() != identity.device
        || metadata.ino() != identity.inode
        || metadata.rdev() != identity.device_number
    {
        return Err("shell render node identity changed before launch".into());
    }
    let major = rustix::fs::major(identity.device_number);
    let minor = rustix::fs::minor(identity.device_number);
    let physical = std::fs::canonicalize(format!("/sys/dev/char/{major}:{minor}/device"))
        .map_err(|error| format!("shell render node physical identity: {error}"))?;
    if physical != identity.physical_device {
        return Err("shell render node physical device changed before launch".into());
    }
    Ok((major, minor, physical))
}

fn pci_bus_id(physical: &Path) -> Option<String> {
    let value = physical.file_name()?.to_str()?;
    let bytes = value.as_bytes();
    (bytes.len() == 12
        && bytes[4] == b':'
        && bytes[7] == b':'
        && bytes[10] == b'.'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7 | 10) || byte.is_ascii_hexdigit()))
    .then(|| value.to_ascii_lowercase())
}

fn pci_ids(physical: &Path) -> Result<(u32, u32), String> {
    let read = |name| {
        let value = std::fs::read_to_string(physical.join(name))
            .map_err(|error| format!("shell render node PCI {name}: {error}"))?;
        let value = value.trim().strip_prefix("0x").unwrap_or(value.trim());
        let value = u32::from_str_radix(value, 16)
            .map_err(|_| format!("shell render node PCI {name} is malformed"))?;
        (value <= u16::MAX.into())
            .then_some(value)
            .ok_or_else(|| format!("shell render node PCI {name} exceeds sixteen bits"))
    };
    Ok((read("vendor")?, read("device")?))
}

#[path = "gpu/tests.rs"]
mod tests;
