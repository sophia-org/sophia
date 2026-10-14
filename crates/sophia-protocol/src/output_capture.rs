//! Passive record for one immutable output image. A decoded record conveys no
//! authority; presentation identity must come from the trusted output owner.
use crate::OutputId;

pub const OUTPUT_CAPTURE_RECORD_LEN: usize = 64;
pub const OUTPUT_CAPTURE_MAX_BYTES: usize = 64 * 1024 * 1024;
pub const OUTPUT_CAPTURE_MIME: &str = "application/x-sophia-output-frame-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputCaptureFrameInfo {
    pub output: OutputId,
    /// Never reused when an output disappears or its physical owner changes.
    pub output_generation: u64,
    /// Identifies the selected completed presentation, not a render request.
    pub presentation_generation: u64,
    pub width: u32,
    pub height: u32,
}

impl OutputCaptureFrameInfo {
    /// Version 1 uses tightly packed, top-down RGBA8, with four bytes per pixel.
    pub fn byte_len(self) -> Option<usize> {
        if !self.output.is_valid()
            || self.output_generation == 0
            || self.presentation_generation == 0
            || self.width == 0
            || self.height == 0
        {
            return None;
        }
        usize::try_from(self.width)
            .ok()?
            .checked_mul(usize::try_from(self.height).ok()?)?
            .checked_mul(4)
            .filter(|len| *len <= OUTPUT_CAPTURE_MAX_BYTES)
    }

    /// Eight little-endian u64 fields: version, output, output generation,
    /// presentation generation, width, height, row stride and format (1=RGBA8).
    pub fn encode(self) -> Option<[u8; OUTPUT_CAPTURE_RECORD_LEN]> {
        self.byte_len()?;
        let words = [
            1,
            self.output.raw(),
            self.output_generation,
            self.presentation_generation,
            u64::from(self.width),
            u64::from(self.height),
            u64::from(self.width) * 4,
            1,
        ];
        let mut record = [0; OUTPUT_CAPTURE_RECORD_LEN];
        for (word, bytes) in words.into_iter().zip(record.chunks_exact_mut(8)) {
            bytes.copy_from_slice(&word.to_le_bytes());
        }
        Some(record)
    }

    pub fn decode(record: &[u8]) -> Option<Self> {
        if record.len() != OUTPUT_CAPTURE_RECORD_LEN {
            return None;
        }
        let mut words = [0u64; 8];
        for (word, bytes) in words.iter_mut().zip(record.chunks_exact(8)) {
            *word = u64::from_le_bytes(bytes.try_into().ok()?);
        }
        let info = Self {
            output: OutputId::from_raw(words[1]),
            output_generation: words[2],
            presentation_generation: words[3],
            width: u32::try_from(words[4]).ok()?,
            height: u32::try_from(words[5]).ok()?,
        };
        (info.encode()?.as_slice() == record).then_some(info)
    }
}
