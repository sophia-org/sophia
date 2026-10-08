use std::fs;
use std::path::{Path, PathBuf};

use rustix::fs::{FileType, Stat, makedev};

use super::LiveRenderDeviceInventoryError as E;
use super::seat_policy::is_node_name;

const CAPACITY: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RenderCandidate {
    pub sysfs_node: PathBuf,
    pub physical_device: PathBuf,
    pub device_number: u64,
}

pub(super) fn node_device_number(node: &Path) -> Option<u64> {
    let number = fs::read_to_string(node.join("dev")).ok()?;
    let (major, minor) = number.trim().split_once(':')?;
    Some(makedev(major.parse().ok()?, minor.parse().ok()?))
}

pub(super) fn render_sibling(root: &Path, physical: &Path) -> Result<Option<RenderCandidate>, E> {
    let mut selected = None;
    for entry in fs::read_dir(root).map_err(|_| E::DiscoveryUnavailable)? {
        let Ok(entry) = entry else { continue };
        if !is_node_name(&entry.file_name(), "renderD") {
            continue;
        }
        let Ok(parent) = fs::canonicalize(entry.path().join("device")) else {
            continue;
        };
        if parent != physical {
            continue;
        }
        let Some(device_number) = node_device_number(&entry.path()) else {
            continue;
        };
        if selected.is_some() {
            return Err(E::AmbiguousRenderNode);
        }
        selected = Some(RenderCandidate {
            sysfs_node: entry.path(),
            physical_device: parent,
            device_number,
        });
    }
    Ok(selected)
}

pub(super) fn admit_candidate(
    selected: &mut Vec<RenderCandidate>,
    candidate: RenderCandidate,
) -> Result<(), E> {
    for existing in selected.iter() {
        if existing.physical_device == candidate.physical_device
            || existing.device_number == candidate.device_number
        {
            return if existing == &candidate {
                Ok(())
            } else {
                Err(E::AmbiguousRenderNode)
            };
        }
    }
    if selected.len() == CAPACITY {
        return Err(E::CapacityExceeded);
    }
    selected.push(candidate);
    Ok(())
}

pub(super) fn validate_identity(
    candidate: &RenderCandidate,
    before: &Stat,
    opened: &Stat,
    after: &Stat,
    physical: &Path,
) -> Result<(), E> {
    if [before, opened, after]
        .iter()
        .any(|stat| FileType::from_raw_mode(stat.st_mode) != FileType::CharacterDevice)
    {
        return Err(E::InvalidDevice);
    }
    if [before, opened, after]
        .iter()
        .any(|stat| stat.st_rdev != candidate.device_number)
        || before.st_dev != opened.st_dev
        || before.st_ino != opened.st_ino
        || after.st_dev != opened.st_dev
        || after.st_ino != opened.st_ino
        || physical != candidate.physical_device
    {
        return Err(E::IdentityChanged);
    }
    Ok(())
}
