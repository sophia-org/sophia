//! One private writable mapping for the experimental ring producer.
use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::ptr::NonNull;

/// A bounded producer mapping with no externally writable descriptor alias.
///
/// Size seals protect every view. FUTURE_WRITE is installed after mapping and
/// before exposing the descriptor: the existing mapping stays writable, while
/// pwrite and subsequent shared writable mappings are forbidden. DONTFORK keeps
/// fork from creating a second writable mapping. Only an exclusive borrow can
/// expose bytes. This type deliberately has no Send/Sync implementation.
/// As with the SDK mapped builder, a forked child must not use or drop an
/// inherited value: DONTFORK removes its mapping. Fork is already unsafe in Rust.
pub struct ProducerRing {
    address: NonNull<libc::c_void>,
    length: usize,
    file: OwnedFd,
}

impl ProducerRing {
    pub fn create(bytes: usize) -> io::Result<Self> {
        // This is an allocation bound, not the protocol's slot geometry.
        if bytes == 0 || bytes > 32 * 1024 * 1024 {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        // SAFETY: constant NUL-terminated name and valid memfd flags.
        let raw = unsafe {
            libc::memfd_create(
                c"sophia-ring-producer".as_ptr(),
                libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: memfd_create returned a new descriptor owned by this call.
        let file = unsafe { OwnedFd::from_raw_fd(raw) };
        // SAFETY: the private descriptor is live and the bounded size fits off_t.
        if unsafe { libc::fallocate(file.as_raw_fd(), 0, 0, bytes as libc::off_t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        add_seals(&file, libc::F_SEAL_GROW | libc::F_SEAL_SHRINK)?;
        // SAFETY: this private, size-sealed file has exactly bytes initialized
        // bytes. No descriptor or mapping has escaped and the address is chosen
        // by the kernel, so the mapping aliases no existing Rust memory.
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let Some(address) = NonNull::new(raw) else {
            // SAFETY: this is the mapping just returned by mmap.
            unsafe {
                libc::munmap(raw, bytes);
            }
            return Err(io::Error::from_raw_os_error(libc::ENOMEM));
        };
        let mapping = Self {
            address,
            length: bytes,
            file,
        };
        // SAFETY: the advised range is exactly the mapping we exclusively own.
        if unsafe { libc::madvise(raw, bytes, libc::MADV_DONTFORK) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // No fd access is possible until this succeeds. SEAL prevents callers
        // adding incompatible seals later; failure drops mapping before file.
        add_seals(&mapping.file, libc::F_SEAL_FUTURE_WRITE | libc::F_SEAL_SEAL)?;
        Ok(mapping)
    }

    /// Fill only slots whose release has been consumed by the protocol owner.
    /// This memory API does not confer slot custody or authenticate the pixels.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: length initialized bytes stay mapped for self's lifetime.
        // Size seals prevent truncation; FUTURE_WRITE forbids external writers
        // through any fd we expose. DONTFORK and &mut self exclude mapping aliases.
        unsafe { std::slice::from_raw_parts_mut(self.address.as_ptr().cast(), self.length) }
    }

    /// Source descriptor for SCM_RIGHTS. Writes through it are kernel-refused.
    pub fn source_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

fn add_seals(file: &OwnedFd, seals: libc::c_int) -> io::Result<()> {
    // SAFETY: this operates on a live descriptor with valid seal bits.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

impl Drop for ProducerRing {
    fn drop(&mut self) {
        // SAFETY: the exact mapping is owned by self; no borrow outlives Drop.
        // The descriptor field closes only after this unmap.
        unsafe {
            libc::munmap(self.address.as_ptr(), self.length);
        }
    }
}
