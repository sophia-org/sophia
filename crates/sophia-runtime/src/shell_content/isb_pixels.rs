//! Bounded validation on immutable CPU storage. No owner-loop scan.
use super::ContentStoreError;
use sophia_linux_peer::SealedMapping;

/// Default validation unit; an owner may publish a smaller byte budget.
pub const ISB_PIXEL_VALIDATION_BYTES: usize = 65_536;

pub struct CpuPixelValidation {
    backing: ImmutableCpuPixels,
    checked: usize,
    failed: bool,
}

/// Only the complete premultiplied-pixel scan can construct this value.
pub struct ValidatedCpuPixels(pub(super) ImmutableCpuPixels);

/// Existing sealed-ISB entry point; owned-copy admission uses its own source type.
pub type IsbPixelValidation = CpuPixelValidation;
/// Compatibility name for the existing sealed-ISB completion API.
pub type ValidatedIsbPixels = ValidatedCpuPixels;

pub(super) enum ImmutableCpuPixels {
    Sealed(SealedMapping),
    Owned(Box<[u8]>),
}

impl ImmutableCpuPixels {
    pub(super) fn bytes(&self) -> &[u8] {
        match self {
            Self::Sealed(mapping) => mapping.bytes(),
            Self::Owned(bytes) => bytes,
        }
    }
}

impl ValidatedCpuPixels {
    pub fn bytes(&self) -> &[u8] {
        self.0.bytes()
    }

    /// Transfer validated backing to an immutable consumer lease without a
    /// pixel copy. This does not grant presentation or release accounting.
    pub fn into_shared(self) -> crate::SharedPixels {
        match self.0 {
            ImmutableCpuPixels::Sealed(mapping) => crate::SharedPixels::isb(mapping),
            ImmutableCpuPixels::Owned(bytes) => crate::SharedPixels::owned(bytes),
        }
    }
}

impl CpuPixelValidation {
    pub fn new(mapping: SealedMapping) -> Result<Self, ContentStoreError> {
        Self::from_backing(ImmutableCpuPixels::Sealed(mapping))
    }

    /// Consume a Session-owned copy. No producer mapping or mutable alias may
    /// enter here. This supplies pixel validation, not ring admission or budget
    /// authorization: the caller must reserve resource/worker credit first.
    pub fn from_owned(bytes: Box<[u8]>) -> Result<Self, ContentStoreError> {
        Self::from_backing(ImmutableCpuPixels::Owned(bytes))
    }

    fn from_backing(backing: ImmutableCpuPixels) -> Result<Self, ContentStoreError> {
        if !backing.bytes().len().is_multiple_of(4) {
            return Err(ContentStoreError::Malformed);
        }
        Ok(Self {
            backing,
            checked: 0,
            failed: false,
        })
    }

    /// Runs at most one bounded unit. The worker checks cancellation and the
    /// wall-clock deadline between calls; progress never renews that deadline.
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
            .min(self.backing.bytes().len());
        let pixels = self.backing.bytes();
        if !valid_unit(pixels, self.checked, end) {
            self.failed = true;
            return Err(ContentStoreError::Malformed);
        }
        self.checked = end;
        Ok(end == self.backing.bytes().len())
    }

    pub fn finish(self) -> Result<ValidatedCpuPixels, ContentStoreError> {
        if self.failed || self.checked != self.backing.bytes().len() {
            return Err(ContentStoreError::Incomplete);
        }
        Ok(ValidatedCpuPixels(self.backing))
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
