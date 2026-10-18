//! One bounded pixel canvas shared by every upload, wire and source.
//! It allocates on the first fill, so the allocation and its page faults fall
//! inside the first measured frame, and never grows afterwards.

/// Bytes of a 4-byte-per-pixel image, or None when the size overflows.
pub fn rgba_bytes(width: u32, height: u32) -> Option<u64> {
    u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(4)
}

pub struct Canvas {
    bytes: Vec<u8>,
    capacity: usize,
}

impl Canvas {
    /// Sizes the canvas for the largest configured upload without allocating.
    pub fn new(capacity: usize) -> Self {
        Self {
            bytes: Vec::new(),
            capacity,
        }
    }

    /// Overwrites the prefix every upload sends, in byte order: opaque alpha
    /// and the generation's shade in every color channel.
    pub fn fill(&mut self, length: usize, generation: u64) -> Result<&[u8], &'static str> {
        if length == 0 || length > self.capacity {
            return Err("upload exceeds the canvas");
        }
        if self.bytes.is_empty() {
            self.bytes = vec![0; self.capacity];
        }
        let prefix = &mut self.bytes[..length];
        fill_pixels(prefix, generation);
        Ok(prefix)
    }

    /// Bytes the canvas holds; zero until the first fill. Read by its tests.
    #[allow(dead_code)]
    pub fn allocated(&self) -> usize {
        self.bytes.len()
    }
}

/// Identical pixel work for the IPC canvas and directly mapped ISB backing.
pub fn fill_pixels(bytes: &mut [u8], generation: u64) {
    let shade = (generation % 128) as u8;
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = if index % 4 == 3 { 255 } else { shade };
    }
}
