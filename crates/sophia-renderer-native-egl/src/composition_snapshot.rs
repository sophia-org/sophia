use std::sync::Arc;

/// One tightly packed RGBA8 image, in top-to-bottom row order. This owns no
/// native handles and grants no permission to disclose its pixels.
#[derive(Clone)]
pub struct NativeCompositionSnapshot {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
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
        (Self::byte_len(width, height)? == rgba.len()).then(|| Self {
            width,
            height,
            rgba: rgba.into(),
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
