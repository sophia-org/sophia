use std::os::fd::{AsFd, OwnedFd};

use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, ftruncate, memfd_create};

use crate::ProducerError;
use crate::io::c_name;

/// A sealed source under construction or ready to offer.
///
/// This safe builder replaces the C mapping helper: callers fill the
/// memfd with `write_pixels` (writes at exact offsets through the
/// file, no mapping), then `finish_sealed` adds the four seals.
/// Because no writable shared mapping can exist through this API,
/// `F_SEAL_WRITE` cannot be refused for a mapping this crate made;
/// descriptors mapped writable elsewhere must be unmapped by their
/// owner before sealing, which this crate cannot observe.
pub struct SealedSource {
    fd: OwnedFd,
    bytes: u64,
}

impl SealedSource {
    pub fn fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.fd.as_fd()
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Write pixels at an exact offset without moving the file
    /// position. The whole slice must land inside the declared
    /// bytes: a write that would extend the backing is refused
    /// before any byte moves, partial writes are completed by
    /// looping, `EINTR` is retried, and a zero-progress write is an
    /// error rather than a silent short fill.
    pub fn write_pixels(&self, offset: u64, pixels: &[u8]) -> Result<(), ProducerError> {
        let end = offset
            .checked_add(pixels.len() as u64)
            .ok_or(ProducerError::Argument)?;
        if end > self.bytes {
            return Err(ProducerError::Argument);
        }
        let mut written = 0usize;
        while written < pixels.len() {
            match rustix::io::pwrite(self.fd.as_fd(), &pixels[written..], offset + written as u64) {
                Ok(0) => return Err(ProducerError::Refused),
                Ok(count) => written += count,
                Err(rustix::io::Errno::INTR) => continue,
                Err(_) => return Err(ProducerError::Refused),
            }
        }
        Ok(())
    }

    /// Consume the builder and return the offerable descriptor.
    pub fn into_fd(self) -> OwnedFd {
        self.fd
    }
}

/// Create a sealable memfd of `width * height * 4` bytes.
pub fn create_sealed(width: u32, height: u32) -> Result<SealedSource, ProducerError> {
    if width == 0 || height == 0 {
        return Err(ProducerError::Argument);
    }
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ProducerError::Argument)?;
    let name = c_name("sophia-image")?;
    let fd = memfd_create(&name, MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING)
        .map_err(|_| ProducerError::Refused)?;
    ftruncate(&fd, bytes).map_err(|_| ProducerError::Refused)?;
    Ok(SealedSource { fd, bytes })
}

/// Add WRITE, GROW, SHRINK and SEAL seals. On every result the
/// caller keeps the descriptor; a refused seal means the descriptor
/// must not be offered.
pub fn finish_sealed(source: &SealedSource) -> Result<(), ProducerError> {
    let seals = SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL;
    fcntl_add_seals(source.fd.as_fd(), seals).map_err(|_| ProducerError::Refused)
}
