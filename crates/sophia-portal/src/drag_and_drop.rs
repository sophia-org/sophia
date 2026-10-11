use crate::prelude::*;
use crate::types::*;

#[derive(Debug, Default)]
pub struct DragAndDropPortal {
    transfers: crate::pending::PendingTransfers,
}

impl DragAndDropPortal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_handoff(
        &mut self,
        request: DragAndDropTransferRequest,
    ) -> Result<PortalCommand, PortalError> {
        validate_drag_and_drop_request(&request)?;

        let transfer = PortalTransfer {
            transfer: request.transfer,
            source_namespace: request.source_namespace,
            target_namespace: request.target_namespace,
            kind: PortalTransferKind::DragAndDrop,
            mime_type: request.offered_types.first().cloned(),
            byte_size: request.byte_size,
            decision: PortalDecision::Pending,
            generation: request.generation,
        };

        self.transfers.insert(transfer.clone())?;
        Ok(PortalCommand::PromptDragAndDropTransfer(transfer))
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
fn validate_drag_and_drop_request(request: &DragAndDropTransferRequest) -> Result<(), PortalError> {
    if !request.transfer.is_valid() {
        return Err(PortalError::InvalidTransfer);
    }

    if !request.source_namespace.is_valid() || !request.target_namespace.is_valid() {
        return Err(PortalError::InvalidNamespace);
    }

    if request.offered_types.is_empty() || request.offered_types.iter().any(|kind| kind.is_empty())
    {
        return Err(PortalError::MissingTransferType);
    }

    if request.offered_types.len() > MAX_DRAG_AND_DROP_TYPES {
        return Err(PortalError::TooManyTransferTypes);
    }

    Ok(())
}
