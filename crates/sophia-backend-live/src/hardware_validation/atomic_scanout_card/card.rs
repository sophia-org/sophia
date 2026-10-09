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
    #[cfg(feature = "seat-control")]
    admitted: Option<crate::drm::seat_inventory::SeatDrmCard>,
}

impl RealAtomicScanoutCard {
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
    pub(super) fn open_admitted_with_seat(
        opener: &crate::LiveSeatDeviceOpener,
        admitted: &crate::drm::seat_inventory::SeatDrmCard,
    ) -> io::Result<Self> {
        admitted.validate_current(opener.name())?;
        if !opener.gpu_admission().admits(admitted.gpu_id.as_deref())? {
            return Err(io::Error::other("DRM card is excluded"));
        }
        let card = Self {
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
