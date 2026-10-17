//! Passive portal records. Decoding establishes neither authority nor presentation.
mod capture;
pub use capture::*;
mod discovery;
pub use discovery::*;
pub const FRAME_INFO_BYTES: usize = 64;
pub const IMAGE_MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordError;

/// RGBA8, tightly packed, top-down. No pixel storage is copied or retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameInfo {
    pub output: u64,
    pub output_generation: u64,
    pub presentation_generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u64,
    pub image_bytes: u64,
}

/// Requires one complete record, without trailing bytes. The caller must
/// establish EOF and keep frame reads bound to the same recipient object.
pub fn decode_frame_info(bytes: &[u8]) -> Result<FrameInfo, RecordError> {
    if bytes.len() != FRAME_INFO_BYTES {
        return Err(RecordError);
    }
    let mut fields = [0u64; 8];
    for (field, chunk) in fields.iter_mut().zip(bytes.chunks_exact(8)) {
        *field = u64::from_le_bytes(chunk.try_into().map_err(|_| RecordError)?);
    }
    let [
        version,
        output,
        output_generation,
        presentation_generation,
        w,
        h,
        stride,
        format,
    ] = fields;
    let width = u32::try_from(w).map_err(|_| RecordError)?;
    let height = u32::try_from(h).map_err(|_| RecordError)?;
    if version != 1
        || format != 1
        || output == 0
        || output_generation == 0
        || presentation_generation == 0
        || width == 0
        || height == 0
        || stride != w * 4
    {
        return Err(RecordError);
    }
    let image_bytes = stride.checked_mul(h).ok_or(RecordError)?;
    if image_bytes > IMAGE_MAX_BYTES {
        return Err(RecordError);
    }
    Ok(FrameInfo {
        output,
        output_generation,
        presentation_generation,
        width,
        height,
        stride,
        image_bytes,
    })
}

pub mod requester;
