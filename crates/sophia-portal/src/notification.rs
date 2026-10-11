use crate::prelude::*;
use crate::types::*;

#[derive(Debug, Default)]
pub struct NotificationPortal {
    transfers: crate::pending::PendingTransfers,
}

impl NotificationPortal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_display(
        &mut self,
        request: NotificationRequest,
    ) -> Result<PortalCommand, PortalError> {
        validate_notification_request(&request)?;

        let transfer = PortalTransfer {
            transfer: request.transfer,
            source_namespace: request.source_namespace,
            target_namespace: request.target_namespace,
            kind: PortalTransferKind::Notification,
            mime_type: Some(notification_type_hint(request.urgency)),
            byte_size: notification_byte_size(&request),
            decision: PortalDecision::Pending,
            generation: request.generation,
        };

        self.transfers.insert(transfer.clone())?;
        Ok(PortalCommand::PromptNotification(transfer))
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
fn validate_notification_request(request: &NotificationRequest) -> Result<(), PortalError> {
    if !request.transfer.is_valid() {
        return Err(PortalError::InvalidTransfer);
    }

    if !request.source_namespace.is_valid() || !request.target_namespace.is_valid() {
        return Err(PortalError::InvalidNamespace);
    }

    if !valid_notification_text(&request.summary, MAX_NOTIFICATION_SUMMARY_LEN)
        || request
            .body
            .as_ref()
            .is_some_and(|body| !valid_notification_text(body, MAX_NOTIFICATION_BODY_LEN))
        || request
            .actions
            .iter()
            .any(|action| !valid_notification_text(action, MAX_NOTIFICATION_ACTION_LEN))
    {
        return Err(PortalError::InvalidNotificationText);
    }

    if request.actions.len() > MAX_NOTIFICATION_ACTIONS {
        return Err(PortalError::TooManyNotificationActions);
    }

    Ok(())
}

fn valid_notification_text(text: &str, max_len: usize) -> bool {
    !text.is_empty() && text.len() <= max_len && !text.chars().any(char::is_control)
}

fn notification_byte_size(request: &NotificationRequest) -> u64 {
    let body_len = request.body.as_ref().map_or(0, String::len);
    let actions_len: usize = request.actions.iter().map(String::len).sum();

    (request.summary.len() + body_len + actions_len) as u64
}

fn notification_type_hint(urgency: NotificationUrgency) -> String {
    let urgency = match urgency {
        NotificationUrgency::Low => "low",
        NotificationUrgency::Normal => "normal",
        NotificationUrgency::Critical => "critical",
    };

    format!("notification:{urgency}")
}
