//! One exact persistent activation outcome owns one aggregate response credit.
//! Session decides authorization; this boundary only retains intake and output.
use super::control_budget::CONTROL_FRAME_BYTES;
use super::{ShellComponentTransport, ShellTransportConnection, ShellTransportError};
use sophia_protocol::*;

pub(super) struct PendingCatalogResponse {
    transaction: TransactionId,
    activation: CatalogActivation,
    status: Option<u16>,
}

impl ShellComponentTransport {
    fn take_catalog_request(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, CatalogActivation)>, ShellTransportError> {
        if !self.supports_persistent_catalog(epochs) {
            return Err(ShellTransportError::MissingCapability);
        }
        if self.catalog_response.is_some() {
            return Ok(None);
        }
        let Some(at) = self
            .inbox
            .iter()
            .position(|frame| u16::from_le_bytes([frame[6], frame[7]]) == 200)
        else {
            return if self.peer_closed {
                Err(ShellTransportError::NotConnected)
            } else {
                Ok(None)
            };
        };
        let (transaction, record) = decode_shell_catalog_action_frame(&self.inbox[at])?;
        let ShellCatalogActionRecord::Activate(activation) = record else {
            return Err(ShellTransportError::WrongContentRecord);
        };
        if Some(activation.action.grant) != self.content_grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if !self.control_capacity_available(epochs, 1) {
            return Ok(None);
        }
        self.catalog_response = Some(PendingCatalogResponse {
            transaction,
            activation: activation.clone(),
            status: None,
        });
        self.inbox.remove(at);
        Ok(Some((transaction, activation)))
    }

    fn finish_catalog_activation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        activation: &CatalogActivation,
        status: u16,
    ) -> Result<(), ShellTransportError> {
        if !(1..=5).contains(&status) {
            return Err(ShellTransportError::WrongActivation);
        }
        let pending = self
            .catalog_response
            .as_mut()
            .ok_or(ShellTransportError::WrongActivation)?;
        if pending.transaction != transaction
            || pending.activation != *activation
            || pending.status.is_some_and(|prior| prior != status)
        {
            return Err(ShellTransportError::WrongActivation);
        }
        pending.status = Some(status);
        self.flush_catalog_response(epochs)?;
        Ok(())
    }

    pub(super) fn flush_catalog_response(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Result<bool, ShellTransportError> {
        let Some(pending) = &self.catalog_response else {
            return Ok(false);
        };
        let Some(status) = pending.status else {
            return Ok(false);
        };
        let frame = encode_shell_catalog_action_frame(
            pending.transaction,
            &ShellCatalogActionRecord::ActivationOutcome(CatalogActivationOutcome {
                activation: pending.activation.clone(),
                status,
                reason: 0,
            }),
        )?;
        if frame.len() > CONTROL_FRAME_BYTES {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        if !self.frame_capacity_available(epochs, frame.len(), true, true) {
            return Ok(false);
        }
        self.output.push(frame, true);
        // This exact fixed-field request was validated before enqueue. Clearing
        // it cannot allocate, call user code or perform I/O after FIFO transfer.
        self.catalog_response = None;
        Ok(true)
    }
}

impl ShellTransportConnection<'_> {
    pub fn poll_catalog_activation(
        &mut self,
    ) -> Result<Option<(TransactionId, CatalogActivation)>, ShellTransportError> {
        self.state.poll_io_bounded(self.content_epochs, 64 * 1024)?;
        self.state.take_catalog_request(self.content_epochs)
    }
    pub fn finish_catalog_activation(
        &mut self,
        transaction: TransactionId,
        activation: &CatalogActivation,
        status: u16,
    ) -> Result<(), ShellTransportError> {
        self.state
            .finish_catalog_activation(self.content_epochs, transaction, activation, status)
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_catalog_responses.rs"]
mod tests;
