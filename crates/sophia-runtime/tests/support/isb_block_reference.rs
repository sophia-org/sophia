//! Frozen fixed-block implementation from babc4e78, used only as a timing reference.
//! Bounded validation on a worker-owned immutable mapping. No owner-loop scan.
use sophia_linux_peer::SealedMapping;
use sophia_runtime::ContentStoreError;

/// Default validation unit; an owner may publish a smaller byte budget.
pub const ISB_PIXEL_VALIDATION_BYTES: usize = 65_536;

pub struct IsbPixelValidation {
    mapping: SealedMapping,
    checked: usize,
    failed: bool,
}

/// Only the complete premultiplied-pixel scan can construct this value.
#[allow(dead_code)]
pub struct ValidatedIsbPixels(SealedMapping);

impl IsbPixelValidation {
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
    #[allow(dead_code)]
    pub fn step(&mut self) -> Result<bool, ContentStoreError> {
        self.step_with_budget(ISB_PIXEL_VALIDATION_BYTES)
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
            .saturating_add(bytes.min(ISB_PIXEL_VALIDATION_BYTES))
            .min(self.mapping.bytes().len());
        let pixels = self.mapping.bytes();
        if !valid_unit(pixels, self.checked, end) {
            self.failed = true;
            return Err(ContentStoreError::Malformed);
        }
        self.checked = end;
        Ok(end == self.mapping.bytes().len())
    }

    pub fn finish(self) -> Result<ValidatedIsbPixels, ContentStoreError> {
        if self.failed || self.checked != self.mapping.bytes().len() {
            return Err(ContentStoreError::Incomplete);
        }
        Ok(ValidatedIsbPixels(self.mapping))
    }
}

fn valid_unit(pixels: &[u8], start: usize, end: usize) -> bool {
    // A budget may split a pixel. Only its alpha can be read beyond the unit;
    // colour components outside the unit must wait for a later step.
    let head_end = (start + (4 - start % 4) % 4).min(end);
    if (start..head_end).any(|i| pixels[i] > pixels[i | 3]) {
        return false;
    }
    let (whole, tail) = pixels[head_end..end].as_chunks::<4>();
    // Fixed-size arrays expose a constant trip count to the vectorizer. Test
    // accumulated validity per block; failure leaves the unit uncommitted.
    let (blocks, remainder) = whole.as_chunks::<16>();
    for block in blocks {
        let mut valid = true;
        for &[b, g, r, a] in block {
            valid &= (b <= a) & (g <= a) & (r <= a);
        }
        if !valid {
            return false;
        }
    }
    if remainder
        .iter()
        .any(|&[b, g, r, a]| b > a || g > a || r > a)
    {
        return false;
    }
    let tail_start = end - tail.len();
    (tail_start..end).all(|i| pixels[i] <= pixels[i | 3])
}
