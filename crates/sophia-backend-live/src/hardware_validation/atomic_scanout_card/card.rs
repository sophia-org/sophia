use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::prelude::*;

#[derive(Debug)]
enum RealAtomicScanoutCardFd {
    Direct(std::fs::File),
    #[cfg(feature = "seat-control")]
    Seat(crate::LiveSeatDevice),
}

#[derive(Debug)]
pub struct RealAtomicScanoutCard {
    fd: RealAtomicScanoutCardFd,
    #[cfg(feature = "gbm-probe")]
    property_identity: u64,
    #[cfg(feature = "seat-control")]
    admitted: Option<crate::drm::seat_inventory::SeatDrmCard>,
}

impl RealAtomicScanoutCard {
    /// A process-local identity for this opened owner, not its reusable fd
    /// number or kernel object IDs. Clones conservatively get a fresh identity.
    #[cfg(feature = "gbm-probe")]
    pub(crate) fn property_identity(&self) -> u64 {
        self.property_identity
    }

    pub fn gpu_identity(&self) -> Option<&str> {
        #[cfg(feature = "seat-control")]
        if let Some(admitted) = &self.admitted {
            return admitted.gpu_id.as_deref().and_then(std::ffi::OsStr::to_str);
        }
        None
    }

    pub fn connector_key(&self, connector: &str) -> String {
        self.gpu_identity()
            .map_or_else(|| connector.to_owned(), |gpu| format!("{gpu}/{connector}"))
    }

    #[cfg(feature = "gbm-probe")]
    pub(crate) fn sysfs_node(&self) -> io::Result<std::path::PathBuf> {
        let metadata = rustix::fs::fstat(self)?;
        std::fs::canonicalize(format!(
            "/sys/dev/char/{}:{}",
            rustix::fs::major(metadata.st_rdev),
            rustix::fs::minor(metadata.st_rdev)
        ))
    }

    pub(super) fn open_nonblocking(path: &Path) -> io::Result<Self> {
        Ok(Self {
            #[cfg(feature = "gbm-probe")]
            property_identity: next_property_identity()?,
            fd: RealAtomicScanoutCardFd::Direct(
                std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
                    .open(path)?,
            ),
            #[cfg(feature = "seat-control")]
            admitted: None,
        })
    }

    #[cfg(feature = "seat-control")]
    pub(crate) fn open_admitted_with_seat(
        opener: &crate::LiveSeatDeviceOpener,
        admitted: &crate::drm::seat_inventory::SeatDrmCard,
    ) -> io::Result<Self> {
        admitted.validate_current(opener.name())?;
        if !opener.gpu_admission().admits(admitted.gpu_id.as_deref())? {
            return Err(io::Error::other("DRM card is excluded"));
        }
        let card = Self {
            #[cfg(feature = "gbm-probe")]
            property_identity: next_property_identity()?,
            fd: RealAtomicScanoutCardFd::Seat(
                opener.open(&admitted.node).map_err(io::Error::other)?,
            ),
            admitted: Some(admitted.clone()),
        };
        admitted.validate_opened(&rustix::fs::fstat(&card)?)?;
        admitted.validate_current(opener.name())?;
        Ok(card)
    }

    #[cfg(all(feature = "seat-control", feature = "drm-hotplug"))]
    pub fn admitted_render_device_opener(
        &self,
    ) -> io::Result<crate::LiveAdmittedRenderDeviceOpener> {
        let admitted = self
            .admitted
            .as_ref()
            .ok_or_else(|| io::Error::other("render instance requires an admitted seat card"))?;
        crate::LiveAdmittedRenderDeviceOpener::from_primary(admitted).map_err(io::Error::other)
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        let fd = match &self.fd {
            RealAtomicScanoutCardFd::Direct(file) => {
                RealAtomicScanoutCardFd::Direct(file.try_clone()?)
            }
            #[cfg(feature = "seat-control")]
            RealAtomicScanoutCardFd::Seat(device) => {
                RealAtomicScanoutCardFd::Seat(device.try_clone()?)
            }
        };
        Ok(Self {
            fd,
            #[cfg(feature = "gbm-probe")]
            property_identity: next_property_identity()?,
            #[cfg(feature = "seat-control")]
            admitted: self.admitted.clone(),
        })
    }

    pub fn try_clone_file(&self) -> io::Result<std::fs::File> {
        match &self.fd {
            RealAtomicScanoutCardFd::Direct(file) => file.try_clone(),
            #[cfg(feature = "seat-control")]
            RealAtomicScanoutCardFd::Seat(device) => device.try_clone_file(),
        }
    }
}

impl AsFd for RealAtomicScanoutCard {
    fn as_fd(&self) -> BorrowedFd<'_> {
        match &self.fd {
            RealAtomicScanoutCardFd::Direct(file) => file.as_fd(),
            #[cfg(feature = "seat-control")]
            RealAtomicScanoutCardFd::Seat(device) => device.as_fd(),
        }
    }
}

impl drm::Device for RealAtomicScanoutCard {}
impl drm::control::Device for RealAtomicScanoutCard {}

#[cfg(feature = "gbm-probe")]
fn next_property_identity() -> io::Result<u64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
        .map_err(|_| io::Error::other("DRM property owner identities exhausted"))
}

#[cfg(all(test, feature = "gbm-probe"))]
#[path = "../../../tests/support/native_property_device_identity.rs"]
mod property_identity_tests;
