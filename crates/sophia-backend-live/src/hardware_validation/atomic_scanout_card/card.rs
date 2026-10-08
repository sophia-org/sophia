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
pub struct RealAtomicScanoutCard(RealAtomicScanoutCardFd);

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
        Ok(Self(RealAtomicScanoutCardFd::Direct(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
                .open(path)?,
        )))
    }

    #[cfg(feature = "seat-control")]
    pub(super) fn open_with_seat(
        opener: &crate::LiveSeatDeviceOpener,
        path: &Path,
    ) -> io::Result<Self> {
        opener
            .open(path)
            .map(RealAtomicScanoutCardFd::Seat)
            .map(Self)
            .map_err(io::Error::other)
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        match &self.0 {
            RealAtomicScanoutCardFd::Direct(file) => {
                Ok(Self(RealAtomicScanoutCardFd::Direct(file.try_clone()?)))
            }
            #[cfg(feature = "seat-control")]
            RealAtomicScanoutCardFd::Seat(device) => {
                Ok(Self(RealAtomicScanoutCardFd::Seat(device.try_clone()?)))
            }
        }
    }

    pub fn try_clone_file(&self) -> io::Result<std::fs::File> {
        match &self.0 {
            RealAtomicScanoutCardFd::Direct(file) => file.try_clone(),
            #[cfg(feature = "seat-control")]
            RealAtomicScanoutCardFd::Seat(device) => device.try_clone_file(),
        }
    }
}

impl AsFd for RealAtomicScanoutCard {
    fn as_fd(&self) -> BorrowedFd<'_> {
        match &self.0 {
            RealAtomicScanoutCardFd::Direct(file) => file.as_fd(),
            #[cfg(feature = "seat-control")]
            RealAtomicScanoutCardFd::Seat(device) => device.as_fd(),
        }
    }
}

impl drm::Device for RealAtomicScanoutCard {}
impl drm::control::Device for RealAtomicScanoutCard {}
