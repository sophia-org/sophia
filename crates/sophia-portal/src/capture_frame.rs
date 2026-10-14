//! Immutable pixels retained outside the bounded portal upload transport.
use sophia_protocol::output_capture::OutputCaptureFrameInfo;
use std::sync::{Arc, Mutex};

/// Maximum portal retention within one shared Session budget.
pub const MAX_CAPTURE_RETAINED_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_CAPTURE_RETAINED_FRAMES: usize = 8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureRetentionUsage {
    pub bytes: usize,
    pub frames: usize,
}

struct Budget {
    limit: CaptureRetentionUsage,
    used: Mutex<CaptureRetentionUsage>,
}

#[derive(Clone)]
pub struct CaptureRetentionBudget(Arc<Budget>);

impl Default for CaptureRetentionBudget {
    fn default() -> Self {
        Self::new(MAX_CAPTURE_RETAINED_BYTES, MAX_CAPTURE_RETAINED_FRAMES)
            .expect("valid capture retention limits")
    }
}

impl CaptureRetentionBudget {
    /// Smaller limits support constrained owners; callers cannot raise the cap.
    pub fn new(bytes: usize, frames: usize) -> Option<Self> {
        if bytes == 0
            || bytes > MAX_CAPTURE_RETAINED_BYTES
            || frames == 0
            || frames > MAX_CAPTURE_RETAINED_FRAMES
        {
            return None;
        }
        Some(Self(Arc::new(Budget {
            limit: CaptureRetentionUsage { bytes, frames },
            used: Mutex::new(CaptureRetentionUsage::default()),
        })))
    }

    /// Reserve before scheduling readback. Failure leaves existing images live.
    /// This accounts for retention; it grants no authority to capture an output.
    pub fn reserve(&self, info: OutputCaptureFrameInfo) -> Option<CaptureReservation> {
        let bytes = info.byte_len()?;
        let mut used = self.0.used.lock().ok()?;
        let next = CaptureRetentionUsage {
            bytes: used.bytes.checked_add(bytes)?,
            frames: used.frames.checked_add(1)?,
        };
        if next.bytes > self.0.limit.bytes || next.frames > self.0.limit.frames {
            return None;
        }
        *used = next;
        Some(CaptureReservation {
            budget: self.clone(),
            info,
            bytes,
        })
    }

    pub fn usage(&self) -> Option<CaptureRetentionUsage> {
        self.0.used.lock().ok().map(|used| *used)
    }
}

/// A pending reservation releases its charge on cancellation. Once completed,
/// the charge follows the frame through source selection and recipient custody.
pub struct CaptureReservation {
    budget: CaptureRetentionBudget,
    info: OutputCaptureFrameInfo,
    bytes: usize,
}

impl CaptureReservation {
    /// The trusted adapter must separately verify completed presentation identity.
    pub fn complete(self, rgba: Arc<[u8]>) -> Option<PortalCaptureFrame> {
        if rgba.len() != self.bytes {
            return None;
        }
        Some(PortalCaptureFrame {
            info: self.info,
            rgba,
            _reservation: self,
        })
    }
}

impl Drop for CaptureReservation {
    fn drop(&mut self) {
        let mut used = self
            .budget
            .0
            .used
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        used.bytes -= self.bytes;
        used.frames -= 1;
    }
}

pub struct PortalCaptureFrame {
    info: OutputCaptureFrameInfo,
    rgba: Arc<[u8]>,
    // Release this pixel reference before its charge. Native owners may retain
    // their own references; their custody needs separate scheduling bounds.
    _reservation: CaptureReservation,
}
impl PortalCaptureFrame {
    pub fn info(&self) -> OutputCaptureFrameInfo {
        self.info
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}
