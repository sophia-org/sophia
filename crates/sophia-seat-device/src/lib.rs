//! Narrow safe adapter for the descriptor libseat hands out with a device.
//!
//! `libseat_open_device` returns a descriptor that belongs to the caller, and
//! `libseat_close_device` releases the device without closing it: no libseat
//! backend closes it, and smithay and wlroots both close it themselves. The
//! `libseat` crate's `Device` keeps it as a bare integer with no `Drop`, so a
//! caller that only closes the device leaks one open file per device. On a
//! card that file can keep DRM master, and a later open of the same card is
//! refused its modeset (t306).

use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::Path;

/// One open seat device and the descriptor that comes with it.
#[derive(Debug)]
pub struct SeatDevice {
    device: libseat::Device,
    descriptor: OwnedFd,
}

impl SeatDevice {
    /// Opens `path` on `seat` and takes ownership of its descriptor.
    pub fn open(seat: &mut libseat::Seat, path: &Path) -> io::Result<Self> {
        let device = seat
            .open_device(&path)
            .map_err(|error| io::Error::from_raw_os_error(i32::from(error)))?;
        // SAFETY: a successful libseat_open_device transfers this descriptor to
        // the caller, and libseat never closes it (see the crate comment). It is
        // owned here exactly once; `device` keeps only the number, which the
        // logind backend reads again in `close` before the descriptor closes.
        let descriptor = unsafe { OwnedFd::from_raw_fd(device.as_fd().as_raw_fd()) };
        Ok(Self { device, descriptor })
    }

    /// Releases the device on its seat, then closes its descriptor. The order
    /// matters: the logind backend identifies the device by the descriptor.
    pub fn close(self, seat: &mut libseat::Seat) -> io::Result<()> {
        let Self { device, descriptor } = self;
        let released = seat
            .close_device(device)
            .map_err(|error| io::Error::from_raw_os_error(i32::from(error)));
        drop(descriptor);
        released
    }
}

impl AsFd for SeatDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.descriptor.as_fd()
    }
}
