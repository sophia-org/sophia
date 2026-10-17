//! Bounded validation on a worker-owned immutable mapping. No owner-loop scan.
use super::ContentStoreError;
use sophia_linux_peer::SealedMapping;

/// Default validation unit; an owner may publish a smaller byte budget.
pub const SEALED_PIXEL_VALIDATION_BYTES: usize = 65_536;

pub struct SealedPixelValidation {
    mapping: SealedMapping,
    checked: usize,
    failed: bool,
}

/// Only the complete premultiplied-pixel scan can construct this value.
pub struct ValidatedSealedPixels(pub(super) SealedMapping);

impl ValidatedSealedPixels {
    pub fn bytes(&self) -> &[u8] {
        self.0.bytes()
    }
}

impl SealedPixelValidation {
    pub fn new(mapping: SealedMapping) -> Result<Self, ContentStoreError> {
        if !mapping.bytes().len().is_multiple_of(4) {
            return Err(ContentStoreError::Malformed);
        }
        Ok(Self {
            mapping,
            checked: 0,
            failed: false,
        })
    }

    /// Runs at most one bounded unit. The worker checks cancellation and the
    /// wall-clock deadline between calls; progress never renews that deadline.
    pub fn step(&mut self) -> Result<bool, ContentStoreError> {
        self.step_with_budget(SEALED_PIXEL_VALIDATION_BYTES)
    }

    /// Honor even a budget smaller than one pixel. The immutable alpha byte
    /// may be inspected before its colour components have all been visited.
    pub fn step_with_budget(&mut self, bytes: usize) -> Result<bool, ContentStoreError> {
        if bytes == 0 {
            return Err(ContentStoreError::Malformed);
        }
        if self.failed {
            return Err(ContentStoreError::Malformed);
        }
        let end = self
            .checked
            .saturating_add(bytes.min(SEALED_PIXEL_VALIDATION_BYTES))
            .min(self.mapping.bytes().len());
        let pixels = self.mapping.bytes();
        if (self.checked..end).any(|i| pixels[i] > pixels[(i / 4) * 4 + 3]) {
            self.failed = true;
            return Err(ContentStoreError::Malformed);
        }
        self.checked = end;
        Ok(end == self.mapping.bytes().len())
    }

    pub fn finish(self) -> Result<ValidatedSealedPixels, ContentStoreError> {
        if self.failed || self.checked != self.mapping.bytes().len() {
            return Err(ContentStoreError::Incomplete);
        }
        Ok(ValidatedSealedPixels(self.mapping))
    }
}
