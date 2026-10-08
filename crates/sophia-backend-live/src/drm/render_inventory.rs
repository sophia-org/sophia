use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rustix::fs::{OFlags, fstat, major, minor, stat};

mod selection;
use super::seat_inventory::{SeatDrmCard, discover_seat_cards, policy as seat_policy};
use selection::{RenderCandidate, admit_candidate, validate_identity};

#[derive(Debug)]
pub struct LiveRenderDevice {
    pub file: File,
    pub identity: LiveRenderDeviceIdentitySnapshot,
}

/// Identity observed when the render node was opened, not a liveness guarantee.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveRenderDeviceIdentitySnapshot {
    pub node: PathBuf,
    pub device: u64,
    pub inode: u64,
    pub device_number: u64,
    pub physical_device: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRenderDeviceInventoryError {
    InvalidSeat,
    DiscoveryUnavailable,
    CapacityExceeded,
    AmbiguousRenderNode,
    OpenFailed,
    IdentityChanged,
    InvalidDevice,
}

impl std::fmt::Display for LiveRenderDeviceInventoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LiveRenderDeviceInventoryError {}

/// Opens at most sixteen render devices assigned to the explicit seat.
/// Connected outputs and KMS capabilities do not determine membership.
pub fn discover_seat_render_devices(
    seat: &str,
) -> Result<Vec<LiveRenderDevice>, LiveRenderDeviceInventoryError> {
    discover_render_devices_on_cards(&render_cards(seat)?)
}

pub(crate) fn discover_render_devices_on_cards(
    cards: &[SeatDrmCard],
) -> Result<Vec<LiveRenderDevice>, LiveRenderDeviceInventoryError> {
    let selected = select_render_candidates(cards)?;
    // The complete selection is bounded before the first descriptor is opened.
    selected.into_iter().map(open_candidate).collect()
}

/// Captures render-device membership and identity without opening the devices.
/// This is used for hotplug comparison; persistent device files are opened only
/// by `discover_seat_render_devices` after the comparison has settled.
pub fn snapshot_seat_render_inventory(
    seat: &str,
) -> Result<Vec<LiveRenderDeviceIdentitySnapshot>, LiveRenderDeviceInventoryError> {
    use LiveRenderDeviceInventoryError as E;
    let selected = select_render_candidates(&render_cards(seat)?)?;
    selected
        .into_iter()
        .map(|candidate| {
            let name = candidate.sysfs_node.file_name().ok_or(E::InvalidDevice)?;
            let path = Path::new("/dev/dri").join(name);
            let metadata = fs::metadata(&path).map_err(|_| E::OpenFailed)?;
            let physical = fs::canonicalize(candidate.sysfs_node.join("device"))
                .map_err(|_| E::IdentityChanged)?;
            let device_number = metadata.rdev();
            if device_number != candidate.device_number {
                return Err(E::IdentityChanged);
            }
            Ok(LiveRenderDeviceIdentitySnapshot {
                node: path,
                device: metadata.dev(),
                inode: metadata.ino(),
                device_number,
                physical_device: physical,
            })
        })
        .collect()
}

fn open_candidate(
    candidate: RenderCandidate,
) -> Result<LiveRenderDevice, LiveRenderDeviceInventoryError> {
    use LiveRenderDeviceInventoryError as E;
    let name = candidate.sysfs_node.file_name().ok_or(E::InvalidDevice)?;
    let path = Path::new("/dev/dri").join(name);
    let before = stat(&path).map_err(|_| E::OpenFailed)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags((OFlags::CLOEXEC | OFlags::NOFOLLOW).bits() as i32)
        .open(&path)
        .map_err(|_| E::OpenFailed)?;
    let opened = fstat(&file).map_err(|_| E::InvalidDevice)?;
    let after = stat(&path).map_err(|_| E::IdentityChanged)?;
    let physical = fs::canonicalize(format!(
        "/sys/dev/char/{}:{}/device",
        major(opened.st_rdev),
        minor(opened.st_rdev),
    ))
    .map_err(|_| E::IdentityChanged)?;
    let current =
        fs::canonicalize(candidate.sysfs_node.join("device")).map_err(|_| E::IdentityChanged)?;
    if current != physical
        || selection::node_device_number(&candidate.sysfs_node) != Some(candidate.device_number)
    {
        return Err(E::IdentityChanged);
    }
    validate_identity(&candidate, &before, &opened, &after, &physical)?;
    Ok(LiveRenderDevice {
        file,
        identity: LiveRenderDeviceIdentitySnapshot {
            node: path,
            device: opened.st_dev,
            inode: opened.st_ino,
            device_number: opened.st_rdev,
            physical_device: physical,
        },
    })
}

fn render_cards(seat: &str) -> Result<Vec<SeatDrmCard>, LiveRenderDeviceInventoryError> {
    use LiveRenderDeviceInventoryError as E;
    if !seat_policy::valid_seat(seat) {
        return Err(E::InvalidSeat);
    }
    discover_seat_cards(seat).map_err(|_| E::DiscoveryUnavailable)
}

fn select_render_candidates(
    cards: &[SeatDrmCard],
) -> Result<Vec<RenderCandidate>, LiveRenderDeviceInventoryError> {
    let mut selected = Vec::new();
    for card in cards {
        if let Some(candidate) =
            selection::render_sibling(Path::new("/sys/class/drm"), &card.physical_device)?
        {
            admit_candidate(&mut selected, candidate)?;
        }
    }
    selected.sort_by(|left, right| left.sysfs_node.cmp(&right.sysfs_node));
    Ok(selected)
}
