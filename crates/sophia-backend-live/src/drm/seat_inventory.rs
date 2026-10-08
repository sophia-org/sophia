//! Seat membership is established before any primary or render node is opened.
//! This inventory is an observation, not a replacement for libseat authority.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(all(feature = "seat-control", feature = "libdrm-events"))]
use rustix::fs::Stat;
use rustix::fs::{FileType, stat};

pub(crate) mod policy;
use policy::{is_node_name, seat_matches, valid_seat};

const CAPACITY: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SeatDrmCard {
    pub node: PathBuf,
    pub sysfs_node: PathBuf,
    pub physical_device: PathBuf,
    pub device_number: u64,
    filesystem: u64,
    inode: u64,
}

/// Does not open a DRM node. Errors on an admitted card refuse the inventory;
/// an inaccessible card is never silently reclassified as another seat's.
pub(crate) fn discover_seat_cards(seat: &str) -> io::Result<Vec<SeatDrmCard>> {
    if !valid_seat(seat) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid DRM seat",
        ));
    }
    let mut enumerator = udev::Enumerator::new()?;
    enumerator.match_subsystem("drm")?;
    enumerator.match_sysname("card[0-9]*")?;
    let mut cards = Vec::new();
    for card in enumerator.scan_devices()? {
        admit_seat_card(
            &mut cards,
            seat,
            card.sysname(),
            card.is_initialized(),
            card.property_value("ID_SEAT"),
            || inspect_card(&card),
        )?;
    }
    cards.sort_by(|left, right| left.node.cmp(&right.node));
    Ok(cards)
}

fn admit_seat_card(
    cards: &mut Vec<SeatDrmCard>,
    seat: &str,
    name: &std::ffi::OsStr,
    initialized: bool,
    assigned: Option<&std::ffi::OsStr>,
    inspect: impl FnOnce() -> io::Result<SeatDrmCard>,
) -> io::Result<()> {
    if !is_node_name(name, "card") {
        return Ok(());
    }
    // Until udev has initialized the record, even an explicit assignment is
    // provisional. Refuse the inventory before touching the node; omitting
    // this card would also hide it from the scoped completeness check.
    if !initialized {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "DRM card seat assignment is not initialized",
        ));
    }
    if seat_matches(seat, initialized, assigned) {
        admit_card(cards, inspect()?)?;
    }
    Ok(())
}

fn inspect_card(card: &udev::Device) -> io::Result<SeatDrmCard> {
    let node = card
        .devnode()
        .ok_or_else(|| io::Error::other("DRM card has no node"))?;
    if node != Path::new("/dev/dri").join(card.sysname()) {
        return Err(io::Error::other(
            "DRM card node does not match its sysfs name",
        ));
    }
    let device_number = card
        .devnum()
        .ok_or_else(|| io::Error::other("DRM card has no devnum"))?;
    let metadata = stat(node)?;
    if FileType::from_raw_mode(metadata.st_mode) != FileType::CharacterDevice
        || metadata.st_rdev != device_number
    {
        return Err(io::Error::other("DRM card node identity changed"));
    }
    Ok(SeatDrmCard {
        node: node.to_owned(),
        sysfs_node: fs::canonicalize(card.syspath())?,
        physical_device: fs::canonicalize(card.syspath().join("device"))?,
        device_number,
        filesystem: metadata.st_dev,
        inode: metadata.st_ino,
    })
}

fn admit_card(cards: &mut Vec<SeatDrmCard>, candidate: SeatDrmCard) -> io::Result<()> {
    for existing in cards.iter() {
        if existing.node == candidate.node
            || existing.device_number == candidate.device_number
            || existing.physical_device == candidate.physical_device
        {
            return if existing == &candidate {
                Ok(())
            } else {
                Err(io::Error::other("ambiguous seat DRM card"))
            };
        }
    }
    if cards.len() == CAPACITY {
        return Err(io::Error::other("seat DRM card capacity exceeded"));
    }
    cards.push(candidate);
    Ok(())
}

impl SeatDrmCard {
    /// Recheck both seat and physical identity before/after a seat-broker open.
    /// No KMS ioctl may be issued on the returned descriptor before this check.
    #[cfg(all(feature = "seat-control", feature = "libdrm-events"))]
    pub(crate) fn validate_current(&self, seat: &str) -> io::Result<()> {
        let card = udev::Device::from_syspath(&self.sysfs_node)?;
        if !seat_matches(seat, card.is_initialized(), card.property_value("ID_SEAT"))
            || inspect_card(&card)? != *self
        {
            return Err(io::Error::other("seat DRM membership or identity changed"));
        }
        Ok(())
    }

    #[cfg(all(feature = "seat-control", feature = "libdrm-events"))]
    pub(crate) fn validate_opened(&self, opened: &Stat) -> io::Result<()> {
        if FileType::from_raw_mode(opened.st_mode) != FileType::CharacterDevice
            || opened.st_rdev != self.device_number
            || opened.st_dev != self.filesystem
            || opened.st_ino != self.inode
        {
            return Err(io::Error::other(
                "opened DRM card differs from admitted identity",
            ));
        }
        Ok(())
    }
}

#[path = "../../tests/support/seat_drm_inventory.rs"]
mod tests;
