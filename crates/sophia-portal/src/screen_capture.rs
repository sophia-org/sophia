use crate::prelude::*;
use crate::types::*;

#[derive(Debug, Default)]
pub struct ScreenCapturePortal {
    transfers: crate::pending::PendingTransfers,
}

impl ScreenCapturePortal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_capture(
        &mut self,
        request: ScreenCaptureRequest,
    ) -> Result<PortalCommand, PortalError> {
        validate_screen_capture_request(&request)?;

        let transfer = PortalTransfer {
            transfer: request.transfer,
            source_namespace: request.source_namespace,
            target_namespace: request.target_namespace,
            kind: match request.mode {
                ScreenCaptureMode::Screenshot => PortalTransferKind::ScreenCapture,
                ScreenCaptureMode::ScreenRecording => PortalTransferKind::ScreenRecording,
            },
            mime_type: Some(screen_capture_type_hint(&request)),
            byte_size: request.byte_size,
            decision: PortalDecision::Pending,
            generation: request.generation,
        };

        self.transfers.insert(transfer.clone())?;
        Ok(PortalCommand::PromptScreenCapture(transfer))
    }

    pub fn deny(&mut self, transfer: PortalTransferId) -> Result<PortalCommand, PortalError> {
        self.transfers.deny(transfer)
    }

    pub fn approve_generation(
        &mut self,
        transfer: PortalTransferId,
        generation: u64,
    ) -> Result<PortalCommand, PortalError> {
        self.transfers.approve_generation(transfer, generation)
    }

    pub fn source_owner_changed(
        &mut self,
        source_namespace: NamespaceId,
        generation: u64,
    ) -> Vec<PortalCommand> {
        self.transfers
            .source_owner_changed(source_namespace, generation)
    }

    pub fn transfer(&self, transfer: PortalTransferId) -> Option<&PortalTransfer> {
        self.transfers.get(&transfer)
    }
}
fn validate_screen_capture_request(request: &ScreenCaptureRequest) -> Result<(), PortalError> {
    if !request.transfer.is_valid() {
        return Err(PortalError::InvalidTransfer);
    }

    if !request.source_namespace.is_valid() || !request.target_namespace.is_valid() {
        return Err(PortalError::InvalidNamespace);
    }

    if !supported_screen_capture_mime(request.mode, &request.mime_type) {
        return Err(PortalError::UnsupportedCaptureMimeType);
    }

    Ok(())
}

fn supported_screen_capture_mime(mode: ScreenCaptureMode, mime_type: &str) -> bool {
    match mode {
        ScreenCaptureMode::Screenshot => matches!(mime_type, "image/png" | "image/jpeg"),
        ScreenCaptureMode::ScreenRecording => matches!(mime_type, "video/webm" | "video/mp4"),
    }
}

fn screen_capture_type_hint(request: &ScreenCaptureRequest) -> String {
    let mode = match request.mode {
        ScreenCaptureMode::Screenshot => "screenshot",
        ScreenCaptureMode::ScreenRecording => "screen-recording",
    };
    let scope = match request.scope {
        ScreenCaptureScope::Desktop => "desktop",
        ScreenCaptureScope::Output => "output",
        ScreenCaptureScope::Surface => "surface",
    };

    format!("{mode}:{scope}:{}", request.mime_type)
}
