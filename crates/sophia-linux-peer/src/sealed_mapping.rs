//! Read-only views of immutable, size-checked kernel backing objects.
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::ptr::NonNull;

/// Owns one sealed backing and its read-only mapping. No writable view escapes.
/// Retaining this value keeps the backing alive after the producer closes it.
pub struct SealedMapping {
    address: NonNull<libc::c_void>,
    length: usize,
    _file: OwnedFd,
}

// SAFETY: the mapping cannot change: WRITE/GROW/SHRINK/SEAL are all installed
// before mmap. Only shared byte slices escape; Drop requires unique ownership.
unsafe impl Send for SealedMapping {}
unsafe impl Sync for SealedMapping {}

impl SealedMapping {
    /// Takes ownership on both success and failure. Checks seals before size
    /// so no producer can race truncation against the subsequent mapping.
    /// A sealed regular file with the wrong length returns EMSGSIZE, separately
    /// from type/seal/mapping failures so role owners preserve refusal reasons.
    pub fn map(file: OwnedFd, expected_bytes: usize) -> io::Result<Self> {
        if expected_bytes == 0 || expected_bytes > isize::MAX as usize {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        let required =
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        // SAFETY: fcntl only inspects this live owned descriptor.
        let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
        if seals < 0 {
            return Err(io::Error::last_os_error());
        }
        if seals & required != required {
            return Err(io::Error::from_raw_os_error(libc::EPERM));
        }
        let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstat initializes the full stat on success.
        if unsafe { libc::fstat(file.as_raw_fd(), status.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fstat succeeded.
        let status = unsafe { status.assume_init() };
        if status.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        }
        if u64::try_from(status.st_size).ok() != Some(expected_bytes as u64) {
            return Err(io::Error::from_raw_os_error(libc::EMSGSIZE));
        }
        // SAFETY: a sealed regular file of exactly this length cannot shrink.
        // The returned mapping is read-only and remains owned until Drop.
        let address = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                expected_bytes,
                libc::PROT_READ,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if address == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let Some(address) = NonNull::new(address) else {
            // A null address cannot back a Rust slice, even if mmap allowed it.
            // SAFETY: mmap succeeded for this exact address and length.
            unsafe {
                libc::munmap(address, expected_bytes);
            }
            return Err(io::Error::from_raw_os_error(libc::ENOMEM));
        };
        Ok(Self {
            address,
            length: expected_bytes,
            _file: file,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: the sealed mapping owns length initialized bytes and cannot
        // be mutated through any descriptor or mapping. Lifetime is tied to us.
        unsafe { std::slice::from_raw_parts(self.address.as_ptr().cast(), self.length) }
    }
}

impl Drop for SealedMapping {
    fn drop(&mut self) {
        // SAFETY: this exact mapping is uniquely owned and no slice outlives us.
        unsafe {
            libc::munmap(self.address.as_ptr(), self.length);
        }
    }
}
