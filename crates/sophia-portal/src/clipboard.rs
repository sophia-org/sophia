use crate::prelude::*;
use crate::types::*;

#[derive(Debug, Default)]
pub struct ClipboardPortal {
    transfers: crate::pending::PendingTransfers,
}

impl ClipboardPortal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_import(
        &mut self,
        request: ClipboardTransferRequest,
    ) -> Result<PortalCommand, PortalError> {
        let transfer = Self::prepare_import(&request)?;
        self.transfers.insert(transfer.clone())?;
        Ok(PortalCommand::PromptClipboardTransfer(transfer))
    }

    /// Family validation only. The portal/files owner retains live custody;
    /// callers using it must not also retain a compatibility decision ledger.
    pub fn prepare_import(
        request: &ClipboardTransferRequest,
    ) -> Result<PortalTransfer, PortalError> {
        validate_request(request)?;

        let transfer = PortalTransfer {
            transfer: request.transfer,
            source_namespace: request.source_namespace,
            target_namespace: request.target_namespace,
            kind: PortalTransferKind::Clipboard,
            mime_type: Some(request.target.as_str().to_owned()),
            byte_size: request.byte_size,
            decision: PortalDecision::Pending,
            generation: request.generation,
        };

        Ok(transfer)
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

    pub fn apply_owner_changed(
        &mut self,
        event: ClipboardSourceOwnerChanged,
    ) -> Vec<PortalCommand> {
        self.source_owner_changed(event.source_namespace, event.generation)
    }

    pub fn transfer(&self, transfer: PortalTransferId) -> Option<&PortalTransfer> {
        self.transfers.get(&transfer)
    }
}
fn validate_request(request: &ClipboardTransferRequest) -> Result<(), PortalError> {
    if !request.transfer.is_valid() {
        return Err(PortalError::InvalidTransfer);
    }

    if !request.source_namespace.is_valid() || !request.target_namespace.is_valid() {
        return Err(PortalError::InvalidNamespace);
    }

    if request.target.as_str() != "TARGETS" && !request.target.is_text() {
        return Err(PortalError::UnsupportedTarget);
    }

    Ok(())
}
