use crate::prelude::*;
use crate::types::*;

#[derive(Debug, Default)]
pub struct UriOpenPortal {
    transfers: crate::pending::PendingTransfers,
}

impl UriOpenPortal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_open(&mut self, request: UriOpenRequest) -> Result<PortalCommand, PortalError> {
        validate_uri_open_request(&request)?;

        let transfer = PortalTransfer {
            transfer: request.transfer,
            source_namespace: request.source_namespace,
            target_namespace: request.target_namespace,
            kind: PortalTransferKind::UriOpen,
            mime_type: Some(uri_open_type_hint(&request.uri)),
            byte_size: request.uri.len() as u64,
            decision: PortalDecision::Pending,
            generation: request.generation,
        };

        self.transfers.insert(transfer.clone())?;
        Ok(PortalCommand::PromptUriOpen(transfer))
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
fn validate_uri_open_request(request: &UriOpenRequest) -> Result<(), PortalError> {
    if !request.transfer.is_valid() {
        return Err(PortalError::InvalidTransfer);
    }

    if !request.source_namespace.is_valid() || !request.target_namespace.is_valid() {
        return Err(PortalError::InvalidNamespace);
    }

    if request.uri.is_empty()
        || request.uri.len() > MAX_URI_LEN
        || request
            .uri
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(PortalError::InvalidUri);
    }

    let Some(scheme) = uri_scheme(&request.uri) else {
        return Err(PortalError::InvalidUri);
    };

    if !supported_uri_scheme(scheme) {
        return Err(PortalError::UnsupportedUriScheme);
    }

    Ok(())
}

fn uri_scheme(uri: &str) -> Option<&str> {
    let (scheme, _rest) = uri.split_once(':')?;
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    {
        return None;
    }

    Some(scheme)
}

fn supported_uri_scheme(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "mailto" | "tel")
}

fn uri_open_type_hint(uri: &str) -> String {
    let scheme = uri_scheme(uri).unwrap_or("unknown");
    format!("uri-open:{scheme}")
}
