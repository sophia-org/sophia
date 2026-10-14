//! Kind-specific payload contracts, separate from grant and journal custody.
use sophia_9p::Errno;
use sophia_protocol::{PortalRequest, PortalTransferKind};

pub struct OutputCapturePayloadHandler;
impl PortalPayloadHandler for OutputCapturePayloadHandler {
    fn supports(&self, kind: PortalTransferKind) -> bool {
        kind == PortalTransferKind::ScreenCapture
    }
    fn validate(&self, request: &PortalRequest, payload: &[u8]) -> Result<(), Errno> {
        use sophia_protocol::output_capture::{OUTPUT_CAPTURE_MIME, OutputCaptureFrameInfo};
        if !self.supports(request.transfer.kind)
            || request.transfer.mime_type.as_deref() != Some(OUTPUT_CAPTURE_MIME)
        {
            return Err(Errno::EOPNOTSUPP);
        }
        let info = OutputCaptureFrameInfo::decode(payload).ok_or(Errno::EINVAL)?;
        if info.output_generation != request.transfer.generation {
            return Err(Errno::ESTALE);
        }
        Ok(())
    }
}

/// A bounded upload handler. Other execution representations require their
/// own admitted handoff; registering a kind does not invent such a handoff.
pub trait PortalPayloadHandler: Send + Sync {
    fn supports(&self, kind: PortalTransferKind) -> bool;
    fn validate(&self, request: &PortalRequest, payload: &[u8]) -> Result<(), Errno>;
}

#[derive(Default)]
pub struct ClipboardPayloadHandler;
impl PortalPayloadHandler for ClipboardPayloadHandler {
    fn supports(&self, kind: PortalTransferKind) -> bool {
        kind == PortalTransferKind::Clipboard
    }
    fn validate(&self, request: &PortalRequest, payload: &[u8]) -> Result<(), Errno> {
        if !self.supports(request.transfer.kind) {
            return Err(Errno::EOPNOTSUPP);
        }
        match request.transfer.mime_type.as_deref() {
            Some("TARGETS") if payload.is_empty() => Ok(()),
            Some("UTF8_STRING" | "text/plain" | "text/plain;charset=utf-8") => {
                std::str::from_utf8(payload)
                    .map(|_| ())
                    .map_err(|_| Errno::EINVAL)
            }
            _ => Err(Errno::EOPNOTSUPP),
        }
    }
}
