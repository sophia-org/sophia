use std::{io::Write, os::fd::OwnedFd};

use sophia_linux_peer::SealedMapping;
use sophia_runtime::{ContentStoreError, ISB_PIXEL_VALIDATION_BYTES};

pub fn source(bytes: &[u8]) -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let fd = memfd_create("isb-scan", MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING).unwrap();
    let mut file = std::fs::File::from(fd);
    file.write_all(bytes).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    file.into()
}

#[allow(dead_code)] // Used by correctness controls; the benchmark reuses one source.
pub fn mapped(bytes: &[u8]) -> SealedMapping {
    SealedMapping::map(source(bytes), bytes.len()).unwrap()
}

/// Frozen byte-at-a-time predicate from the pre-optimization implementation.
/// Keep this independent of the production block scan for differential checks.
pub struct ScalarValidation<'a> {
    bytes: &'a [u8],
    checked: usize,
    failed: bool,
}

impl<'a> ScalarValidation<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            checked: 0,
            failed: false,
        }
    }

    pub fn step(&mut self, bytes: usize) -> Result<bool, ContentStoreError> {
        if bytes == 0 || self.failed {
            return Err(ContentStoreError::Malformed);
        }
        let end = self
            .checked
            .saturating_add(bytes.min(ISB_PIXEL_VALIDATION_BYTES))
            .min(self.bytes.len());
        if (self.checked..end).any(|i| self.bytes[i] > self.bytes[(i / 4) * 4 + 3]) {
            self.failed = true;
            return Err(ContentStoreError::Malformed);
        }
        self.checked = end;
        Ok(end == self.bytes.len())
    }

    pub fn finished(&self) -> bool {
        !self.failed && self.checked == self.bytes.len()
    }
}
