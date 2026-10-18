//! Mapped Immutable Shared Buffer (ISB) builder.
//!
//! [`MappedIsb`] owns a fresh sealable memfd and the only writable shared
//! mapping of it. Callers fill pixels in place through a borrowed slice, so
//! no staging canvas or `pwrite` copy is needed. The descriptor is
//! unreachable until [`MappedIsb::finish`] consumes the builder, unmaps,
//! adds the WRITE, GROW, SHRINK and SEAL seals and verifies them. Dropping an
//! unfinished builder unmaps and closes it, so an abandoned buffer can never
//! be offered.
//!
//! Dimensions and the caller's resource bound are checked before anything is
//! allocated. The GROW and SHRINK seals are added at creation, so the backing
//! cannot shrink beneath the mapping. The mapping is excluded from `fork`, so
//! a child process cannot hold a writable alias that would block the WRITE
//! seal or outlive it. The backing is allocated up front with `fallocate`, so
//! ordinary memory exhaustion is reported by [`MappedIsb::create`] rather than
//! as a fault while filling. That narrows `SIGBUS` exposure; it does not rule
//! it out: hardware memory errors and kernel reclaim remain outside this
//! crate.
//!
//! # Fork
//!
//! A child created by `fork` gets no copy of the mapping, so a builder it
//! inherits points at unmapped memory. The child must not use an inherited
//! builder at all: `pixels` and `pixels_mut` would fault, and dropping it
//! would unmap whatever the child has since mapped at that address, so the
//! child must `std::mem::forget` it. The child also inherits the
//! unsealed descriptor across `fork` (`CLOEXEC` closes it only on `exec`), so
//! it must close it or `exec` promptly; a write through that descriptor
//! before the parent's `finish` would change the pixels being sealed. Calling
//! `fork` is already `unsafe` in Rust, and in a multithreaded parent the child
//! may run only async-signal-safe code until `exec`; this builder adds no
//! safe way to reach either state.
//!
//! This is the only SDK crate that permits `unsafe`, and only in its private
//! mapping module.
#![deny(unsafe_code)]

mod mapping;

use std::os::fd::{AsFd, OwnedFd};

use rustix::fs::{
    FallocateFlags, MemfdFlags, SealFlags, fallocate, fcntl_add_seals, fcntl_get_seals,
    memfd_create,
};
use rustix::io::Errno;

/// The seals a finished buffer carries.
pub const SEALS: SealFlags = SealFlags::WRITE
    .union(SealFlags::GROW)
    .union(SealFlags::SHRINK)
    .union(SealFlags::SEAL);

/// Why a buffer was not created or not finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IsbError {
    /// Zero, overflowing or over-bound dimensions. Nothing was allocated.
    Argument,
    /// The kernel refused a step. Everything created was released.
    Refused(Errno),
    /// The seals were added but did not read back complete. The descriptor
    /// was closed rather than returned.
    Unsealed(SealFlags),
}

/// A memfd under construction, writable only through this builder.
pub struct MappedIsb {
    // Declared first so it drops first: unmap, then close.
    mapping: mapping::Mapping,
    fd: OwnedFd,
    width: u32,
    height: u32,
}

impl MappedIsb {
    /// Create a zero-filled `width * height * 4` byte buffer. Refuses zero
    /// dimensions, a stride above `u32::MAX`, a size above `isize::MAX` or
    /// `i64::MAX` and any size above `max_resource_bytes`, all before
    /// allocating.
    pub fn create(width: u32, height: u32, max_resource_bytes: u64) -> Result<Self, IsbError> {
        let bytes = byte_len(width, height)
            .filter(|bytes| *bytes <= max_resource_bytes)
            .ok_or(IsbError::Argument)?;
        let len = usize::try_from(bytes).map_err(|_| IsbError::Argument)?;
        let fd = memfd_create(
            c"sophia-image",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .map_err(IsbError::Refused)?;
        // Mode 0 sets the size and allocates every page now.
        fallocate(&fd, FallocateFlags::empty(), 0, bytes).map_err(IsbError::Refused)?;
        fcntl_add_seals(&fd, SealFlags::GROW | SealFlags::SHRINK).map_err(IsbError::Refused)?;
        let mapping = mapping::Mapping::new(fd.as_fd(), len).map_err(IsbError::Refused)?;
        Ok(Self {
            mapping,
            fd,
            width,
            height,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Bytes per row: rows are tightly packed four-byte pixels.
    pub fn stride(&self) -> u32 {
        self.width * 4
    }

    /// Total bytes, `stride * height`.
    pub fn len(&self) -> usize {
        self.mapping.bytes().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn pixels(&self) -> &[u8] {
        self.mapping.bytes()
    }

    /// The pixels, writable in place. The view borrows the builder, so it
    /// cannot outlive [`finish`](Self::finish):
    ///
    /// ```
    /// let mut isb = sophia_isb::MappedIsb::create(1, 1, 4).unwrap();
    /// isb.pixels_mut().copy_from_slice(&[1, 2, 3, 255]);
    /// let fd = isb.finish().unwrap();
    /// # drop(fd);
    /// ```
    ///
    /// ```compile_fail,E0505
    /// let mut isb = sophia_isb::MappedIsb::create(1, 1, 4).unwrap();
    /// let view = isb.pixels_mut();
    /// let fd = isb.finish().unwrap();
    /// view[0] = 1;
    /// ```
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        self.mapping.bytes_mut()
    }

    /// Unmap, add [`SEALS`] and verify them, then return the offerable
    /// descriptor. On error the descriptor is closed, never returned unsealed.
    pub fn finish(self) -> Result<OwnedFd, IsbError> {
        let Self { mapping, fd, .. } = self;
        // F_SEAL_WRITE is refused while a writable shared mapping exists.
        mapping.unmap().map_err(IsbError::Refused)?;
        fcntl_add_seals(&fd, SEALS).map_err(IsbError::Refused)?;
        let seals = fcntl_get_seals(&fd).map_err(IsbError::Refused)?;
        if !seals.contains(SEALS) {
            return Err(IsbError::Unsealed(seals));
        }
        Ok(fd)
    }
}

impl std::fmt::Debug for MappedIsb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MappedIsb")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

/// The largest buffer: a file offset (`i64`) and a slice length (`isize`)
/// must both hold it. On 32-bit targets `isize::MAX` is the smaller.
const MAX_BYTES: u64 = if (isize::MAX as u64) < (i64::MAX as u64) {
    isize::MAX as u64
} else {
    i64::MAX as u64
};

/// `width * height * 4`, when nonzero, with a `u32` stride and at most
/// [`MAX_BYTES`].
fn byte_len(width: u32, height: u32) -> Option<u64> {
    if width == 0 || height == 0 {
        return None;
    }
    let stride = width.checked_mul(4)?;
    u64::from(stride)
        .checked_mul(u64::from(height))
        .filter(|bytes| *bytes <= MAX_BYTES)
}
