//! The SDK's only unsafe code: one shared writable mapping of a private
//! memfd, owned exclusively by its [`MappedIsb`](crate::MappedIsb).
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::os::fd::BorrowedFd;
use std::ptr::NonNull;

use rustix::io::Errno;
use rustix::mm::{Advice, MapFlags, ProtFlags, madvise, mmap, munmap};

pub(crate) struct Mapping {
    ptr: NonNull<u8>,
    len: usize,
}

// SAFETY: the mapping is plain memory with no thread affinity. It is owned
// exclusively, and shared access yields only `&[u8]`.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    /// Map all `len` bytes of `fd` shared and writable, then exclude the
    /// range from `fork`. `fd` must be a private, size-sealed memfd of
    /// exactly `len` nonzero bytes that no other code can reach.
    pub(crate) fn new(fd: BorrowedFd<'_>, len: usize) -> Result<Self, Errno> {
        // SAFETY: a new mapping at a kernel-chosen address aliases no
        // existing Rust memory.
        let raw = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                ProtFlags::READ | ProtFlags::WRITE,
                MapFlags::SHARED,
                fd,
                0,
            )?
        };
        let Some(ptr) = NonNull::new(raw.cast::<u8>()) else {
            // SAFETY: the range is exactly the mapping just created.
            let _ = unsafe { munmap(raw, len) };
            return Err(Errno::NOMEM);
        };
        let mapping = Self { ptr, len };
        // A forked child would otherwise inherit a writable alias. On error
        // `mapping` drops and unmaps.
        // SAFETY: the range is exactly the mapping just created.
        unsafe { madvise(mapping.raw(), len, Advice::LinuxDontFork)? };
        Ok(mapping)
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        // SAFETY: the range stays mapped and readable for `self`'s lifetime.
        // The size seals keep the backing whole, and the private descriptor
        // admits no other writer, so no write races this shared borrow.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }

    pub(crate) fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as `bytes`, and `&mut self` makes this the only view.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    /// Unmap now, reporting failure. Consuming `self` ends every view.
    pub(crate) fn unmap(self) -> Result<(), Errno> {
        let this = std::mem::ManuallyDrop::new(self);
        // SAFETY: the range is this mapping, and no view outlives `self`.
        unsafe { munmap(this.raw(), this.len) }
    }

    fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr().cast()
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: as `unmap`.
        let _ = unsafe { munmap(self.raw(), self.len) };
    }
}
