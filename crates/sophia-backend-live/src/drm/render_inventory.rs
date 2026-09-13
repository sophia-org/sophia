use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rustix::fs::{OFlags, fstat, major, minor, stat};

mod selection;
use selection::{RenderCandidate, admit_candidate, is_node_name, seat_matches, validate_identity};

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
    use LiveRenderDeviceInventoryError as E;
    if seat.is_empty()
        || seat.len() > 64
        || !seat.is_ascii()
        || seat.bytes().any(|byte| byte <= b' ')
    {
        return Err(E::InvalidSeat);
    }
    let mut enumerator = udev::Enumerator::new().map_err(|_| E::DiscoveryUnavailable)?;
    enumerator
        .match_subsystem("drm")
        .and_then(|()| enumerator.match_sysname("card[0-9]*"))
        .map_err(|_| E::DiscoveryUnavailable)?;
    let mut selected = Vec::new();
    for card in enumerator
        .scan_devices()
        .map_err(|_| E::DiscoveryUnavailable)?
    {
        if !is_node_name(card.sysname(), "card")
            || !seat_matches(seat, card.is_initialized(), card.property_value("ID_SEAT"))
        {
            continue;
        }
        let Ok(physical) = fs::canonicalize(card.syspath().join("device")) else {
            continue;
        };
        if let Some(candidate) = selection::render_sibling(Path::new("/sys/class/drm"), &physical)?
        {
            admit_candidate(&mut selected, candidate)?;
        }
    }
    selected.sort_by(|left, right| left.sysfs_node.cmp(&right.sysfs_node));
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
    if seat.is_empty()
        || seat.len() > 64
        || !seat.is_ascii()
        || seat.bytes().any(|byte| byte <= b' ')
    {
        return Err(E::InvalidSeat);
    }
    let mut enumerator = udev::Enumerator::new().map_err(|_| E::DiscoveryUnavailable)?;
    enumerator
        .match_subsystem("drm")
        .and_then(|()| enumerator.match_sysname("card[0-9]*"))
        .map_err(|_| E::DiscoveryUnavailable)?;
    let mut selected = Vec::new();
    for card in enumerator
        .scan_devices()
        .map_err(|_| E::DiscoveryUnavailable)?
    {
        if !is_node_name(card.sysname(), "card")
            || !seat_matches(seat, card.is_initialized(), card.property_value("ID_SEAT"))
        {
            continue;
        }
        let Ok(physical) = fs::canonicalize(card.syspath().join("device")) else {
            continue;
        };
        if let Some(candidate) = selection::render_sibling(Path::new("/sys/class/drm"), &physical)?
        {
            admit_candidate(&mut selected, candidate)?;
        }
    }
    selected.sort_by(|left, right| left.sysfs_node.cmp(&right.sysfs_node));
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
