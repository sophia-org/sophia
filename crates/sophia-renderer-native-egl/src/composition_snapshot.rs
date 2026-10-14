use std::sync::{Arc, Mutex};
#[path = "../tests/support/snapshot_retention.rs"]
mod retention_tests;

static RETAINED: Mutex<(usize, usize)> = Mutex::new((0, 0));
pub const NATIVE_SNAPSHOT_RETAINED_BYTES: usize = 128 * 1024 * 1024;
pub const NATIVE_SNAPSHOT_RETAINED_IMAGES: usize = 8;

pub struct SnapshotReservation {
    bytes: usize,
}
impl SnapshotReservation {
    pub(crate) fn byte_len(&self) -> usize {
        self.bytes
    }
    pub fn for_image(width: u32, height: u32) -> Option<Self> {
        Self::reserve(NativeCompositionSnapshot::byte_len(width, height)?)
    }
    pub(crate) fn reserve(bytes: usize) -> Option<Self> {
        let mut used = RETAINED.lock().ok()?;
        let next = used.0.checked_add(bytes)?;
        if next > NATIVE_SNAPSHOT_RETAINED_BYTES || used.1 >= NATIVE_SNAPSHOT_RETAINED_IMAGES {
            return None;
        }
        *used = (next, used.1 + 1);
        Some(Self { bytes })
    }
}
impl Drop for SnapshotReservation {
    fn drop(&mut self) {
        let mut used = RETAINED.lock().unwrap_or_else(|error| error.into_inner());
        used.0 -= self.bytes;
        used.1 -= 1;
    }
}

/// One tightly packed RGBA8 image, in top-to-bottom row order. This owns no
/// native handles and grants no permission to disclose its pixels.
#[derive(Clone)]
pub struct NativeCompositionSnapshot {
    width: u32,
    height: u32,
    rgba: Arc<Vec<u8>>,
    // Shared across snapshot clones, including displayed scanout custody.
    _reservation: Arc<SnapshotReservation>,
}

/// Bounds each readback before allocation, including driver-facing dimensions.
pub const NATIVE_COMPOSITION_SNAPSHOT_MAX_BYTES: usize = 64 * 1024 * 1024;

impl NativeCompositionSnapshot {
    pub fn byte_len(width: u32, height: u32) -> Option<usize> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return None;
        }
        usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?
            .checked_mul(4)
            .filter(|len| *len <= NATIVE_COMPOSITION_SNAPSHOT_MAX_BYTES)
    }

    /// Takes private storage; later renders cannot overwrite an earlier image.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        let reservation = SnapshotReservation::reserve(Self::byte_len(width, height)?)?;
        Self::from_reserved(width, height, rgba, reservation)
    }

    pub(crate) fn from_reserved(
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        reservation: SnapshotReservation,
    ) -> Option<Self> {
        if reservation.bytes != rgba.len() {
            return None;
        }
        (Self::byte_len(width, height)? == rgba.len()).then(|| Self {
            width,
            height,
            rgba: Arc::new(rgba),
            _reservation: Arc::new(reservation),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}
