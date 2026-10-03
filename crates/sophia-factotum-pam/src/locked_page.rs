//! Pages for secrets: an anonymous mapping that is locked in memory, left out
//! of core dumps and zeroed before it is unmapped. The helper reads its
//! request into one, and Session keeps the secret being typed in one.
use std::ptr;

/// An anonymous mapping that is locked in memory, excluded from core dumps
/// and zeroed before it is unmapped. A page that cannot be both locked and
/// excluded is not handed out.
pub struct LockedPage {
    base: *mut u8,
    len: usize,
}

// SAFETY: the mapping is owned by exactly one value and reached only through
// it, so moving that value to another thread moves the whole page.
unsafe impl Send for LockedPage {}

impl LockedPage {
    pub fn new(len: usize) -> Option<Self> {
        // SAFETY: an anonymous private mapping takes no file or address; the
        // result is checked before use.
        let base = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return None;
        }
        // SAFETY: `base..base+len` is the mapping just created.
        let locked = unsafe {
            libc::madvise(base, len, libc::MADV_DONTDUMP) == 0 && libc::mlock(base, len) == 0
        };
        let page = Self {
            base: base.cast(),
            len,
        };
        locked.then_some(page)
    }

    /// Zeroes every byte; the stores are volatile, so none is elided.
    pub fn zero(&mut self) {
        for byte in self.as_mut_slice().iter_mut() {
            // SAFETY: a valid, exclusively borrowed byte of the mapping.
            unsafe { ptr::write_volatile(byte, 0) };
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: the mapping is `len` readable bytes, owned by this value
        // for its whole life.
        unsafe { std::slice::from_raw_parts(self.base, self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: the mapping is `len` readable and writable bytes, owned by
        // this value for its whole life.
        unsafe { std::slice::from_raw_parts_mut(self.base, self.len) }
    }
}

impl Drop for LockedPage {
    fn drop(&mut self) {
        self.zero();
        // SAFETY: unmaps exactly the mapping created in `new`.
        unsafe {
            libc::munmap(self.base.cast(), self.len);
        }
    }
}
