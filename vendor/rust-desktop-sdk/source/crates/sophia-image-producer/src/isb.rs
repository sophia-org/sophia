use std::os::fd::{AsFd, OwnedFd};

use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, ftruncate, memfd_create};

use sophia_isb::{IsbError, MappedIsb};

use crate::ProducerError;
use crate::io::c_name;

/// An Immutable Shared Buffer (ISB) under construction or ready to offer.
/// Linux implements its immutable backing with a sealed memfd.
///
/// This builder fills the memfd with `write_pixels` (writes at exact
/// offsets through the file, no mapping), then `finish_isb` adds the
/// four seals. It copies every byte through `pwrite`; for in-place
/// writes use [`create_mapped_isb`].
/// Because no writable shared mapping can exist through this API,
/// `F_SEAL_WRITE` cannot be refused for a mapping this crate made;
/// descriptors mapped writable elsewhere must be unmapped by their
/// owner before sealing, which this crate cannot observe.
pub struct IsbSource {
    fd: OwnedFd,
    bytes: u64,
}

impl IsbSource {
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
pub fn create_isb(width: u32, height: u32) -> Result<IsbSource, ProducerError> {
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
    Ok(IsbSource { fd, bytes })
}

/// Create a mapped builder that is filled in place, matching C's
/// `sophia_ip_isb_create_bounded`. Dimensions and `max_resource_bytes`
/// (normally the role's granted `max_resource_bytes`) are checked before
/// anything is allocated. The descriptor is reachable only through
/// [`MappedIsb::finish`], which unmaps, seals and verifies; dropping the
/// builder unmaps and closes it. See [`sophia_isb`] for the fork and
/// `SIGBUS` guarantees and their limits.
pub fn create_mapped_isb(
    width: u32,
    height: u32,
    max_resource_bytes: u64,
) -> Result<MappedIsb, ProducerError> {
    MappedIsb::create(width, height, max_resource_bytes).map_err(|error| match error {
        IsbError::Argument => ProducerError::Argument,
        IsbError::Refused(_) | IsbError::Unsealed(_) => ProducerError::Refused,
    })
}

/// Add WRITE, GROW, SHRINK and SEAL seals. On every result the
/// caller keeps the descriptor; a refused seal means the descriptor
/// must not be offered.
pub fn finish_isb(source: &IsbSource) -> Result<(), ProducerError> {
    let seals = SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL;
    fcntl_add_seals(source.fd.as_fd(), seals).map_err(|_| ProducerError::Refused)
}
