use crate::prelude::*;
use crate::types::*;

#[derive(Debug, Default)]
pub struct FileHandoffPortal {
    transfers: crate::pending::PendingTransfers,
}

impl FileHandoffPortal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_handoff(
        &mut self,
        request: FileHandoffRequest,
    ) -> Result<PortalCommand, PortalError> {
        validate_file_handoff_request(&request)?;

        let transfer = PortalTransfer {
            transfer: request.transfer,
            source_namespace: request.source_namespace,
            target_namespace: request.target_namespace,
            kind: PortalTransferKind::FileHandoff,
            mime_type: Some(file_handoff_type_hint(&request)),
            byte_size: request.byte_size,
            decision: PortalDecision::Pending,
            generation: request.generation,
        };

        self.transfers.insert(transfer.clone())?;
        Ok(PortalCommand::PromptFileHandoff(transfer))
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
fn validate_file_handoff_request(request: &FileHandoffRequest) -> Result<(), PortalError> {
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

    if request.offered_types.len() > MAX_FILE_HANDOFF_TYPES {
        return Err(PortalError::TooManyTransferTypes);
    }

    if let Some(name) = &request.suggested_name
        && !valid_suggested_file_name(name)
    {
        return Err(PortalError::InvalidSuggestedName);
    }

    Ok(())
}

fn valid_suggested_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SUGGESTED_FILE_NAME_LEN
        && !name.contains('/')
        && !name.contains('\\')
        && name != "."
        && name != ".."
}

fn file_handoff_type_hint(request: &FileHandoffRequest) -> String {
    let mode = match request.mode {
        FileHandoffMode::Open => "open",
        FileHandoffMode::Save => "save",
    };

    format!("{mode}:{}", request.offered_types[0])
}
