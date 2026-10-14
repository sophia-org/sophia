//! Immutable pixels retained outside the bounded portal upload transport.
use sophia_protocol::output_capture::OutputCaptureFrameInfo;
use std::sync::Arc;

pub struct PortalCaptureFrame {
    info: OutputCaptureFrameInfo,
    rgba: Arc<[u8]>,
}
impl PortalCaptureFrame {
    /// Called by the trusted renderer adapter after selecting a presentation.
    /// This validates storage, not the caller's claim to presentation identity.
    pub fn new(info: OutputCaptureFrameInfo, rgba: Arc<[u8]>) -> Option<Self> {
        (info.byte_len()? == rgba.len()).then_some(Self { info, rgba })
    }
    pub fn info(&self) -> OutputCaptureFrameInfo {
        self.info
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}
