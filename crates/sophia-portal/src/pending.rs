//! Shared compatibility request ledger. These reducers translate requests;
//! they cannot mint grants or execute effects. Live grants belong exclusively
//! to PortalRequestGrantLifecycle in the files owner.
use crate::{PortalCommand, PortalError};
use sophia_protocol::{
    NamespaceId, PortalDecision, PortalTransfer, PortalTransferId, PortalTransferKind,
};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub(crate) struct PendingTransfers(BTreeMap<PortalTransferId, PortalTransfer>);
impl PendingTransfers {
    pub(crate) fn insert(&mut self, transfer: PortalTransfer) -> Result<(), PortalError> {
        if self.0.contains_key(&transfer.transfer) {
            return Err(PortalError::DuplicateTransfer);
        }
        if self.0.len() >= 4096 {
            return Err(PortalError::CapacityExceeded);
        }
        self.0.insert(transfer.transfer, transfer);
        Ok(())
    }
    pub(crate) fn get(&self, id: &PortalTransferId) -> Option<&PortalTransfer> {
        self.0.get(id)
    }
    pub(crate) fn deny(&mut self, id: PortalTransferId) -> Result<PortalCommand, PortalError> {
        let transfer = self.pending(id)?;
        transfer.decision = PortalDecision::Denied;
        Ok(command(transfer, false))
    }
    pub(crate) fn approve_generation(
        &mut self,
        id: PortalTransferId,
        generation: u64,
    ) -> Result<PortalCommand, PortalError> {
        let transfer = self.pending(id)?;
        let current = transfer.generation == generation;
        transfer.decision = if current {
            PortalDecision::Allowed
        } else {
            PortalDecision::Revoked
        };
        Ok(command(transfer, current))
    }
    pub(crate) fn source_owner_changed(
        &mut self,
        namespace: NamespaceId,
        generation: u64,
    ) -> Vec<PortalCommand> {
        self.0
            .values_mut()
            .filter_map(|transfer| {
                if transfer.source_namespace != namespace
                    || transfer.generation == generation
                    || transfer.decision != PortalDecision::Pending
                {
                    return None;
                }
                transfer.decision = PortalDecision::Revoked;
                Some(command(transfer, false))
            })
            .collect()
    }
    fn pending(&mut self, id: PortalTransferId) -> Result<&mut PortalTransfer, PortalError> {
        let transfer = self.0.get_mut(&id).ok_or(PortalError::UnknownTransfer)?;
        if transfer.decision != PortalDecision::Pending {
            return Err(PortalError::NotPending);
        }
        Ok(transfer)
    }
}
fn command(value: &PortalTransfer, allowed: bool) -> PortalCommand {
    use PortalCommand::*;
    let transfer = value.transfer;
    match (value.kind, allowed) {
        (PortalTransferKind::Clipboard, true) => HandoffClipboard { transfer },
        (PortalTransferKind::Clipboard, false) => FailSelection { transfer },
        (PortalTransferKind::DragAndDrop, true) => HandoffDragAndDrop { transfer },
        (PortalTransferKind::DragAndDrop, false) => CancelDragAndDrop { transfer },
        (PortalTransferKind::FileHandoff, true) => HandoffFile { transfer },
        (PortalTransferKind::FileHandoff, false) => CancelFileHandoff { transfer },
        (PortalTransferKind::ScreenCapture | PortalTransferKind::ScreenRecording, true) => {
            HandoffScreenCapture { transfer }
        }
        (PortalTransferKind::ScreenCapture | PortalTransferKind::ScreenRecording, false) => {
            CancelScreenCapture { transfer }
        }
        (PortalTransferKind::UriOpen, true) => HandoffUriOpen { transfer },
        (PortalTransferKind::UriOpen, false) => CancelUriOpen { transfer },
        (PortalTransferKind::Notification, true) => DeliverNotification { transfer },
        (PortalTransferKind::Notification, false) => DropNotification { transfer },
    }
}
