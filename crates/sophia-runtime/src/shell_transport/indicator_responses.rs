//! One admitted indicator request owns a response credit until FIFO transfer.
//! Polling cannot hand the same request to the WM twice. The result is recorded
//! before fallible encoding/admission, so retry never repeats the effect.
use super::{ShellSessionTransport, ShellTransportError, control_budget::CONTROL_FRAME_BYTES};
use sophia_protocol::{
    IpcMessageKind, ShellIndicatorActivation, ShellIndicatorActivationOutcome,
    ShellIndicatorActivationStatus, TransactionId, decode_shell_indicator_activation,
    encode_shell_indicator_activation_outcome,
};

#[derive(Clone, Copy)]
pub(super) struct PendingIndicatorResponse {
    transaction: TransactionId,
    activation: ShellIndicatorActivation,
    outcome: Option<ShellIndicatorActivationOutcome>,
}

impl ShellSessionTransport {
    pub fn poll_indicator_activation(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellIndicatorActivation)>, ShellTransportError> {
        self.poll_io()?;
        self.take_indicator_request()
    }

    fn take_indicator_request(
        &mut self,
    ) -> Result<Option<(TransactionId, ShellIndicatorActivation)>, ShellTransportError> {
        if !self.supports_indicator_activation() {
            return Err(ShellTransportError::MissingCapability);
        }
        if self.indicator_response.is_some() {
            return Ok(None);
        }
        let at = self.inbox.iter().position(|frame| {
            u16::from_le_bytes([frame[6], frame[7]])
                == IpcMessageKind::ShellIndicatorActivate as u16
        });
        let Some(at) = at else {
            return if self.peer_closed {
                Err(ShellTransportError::NotConnected)
            } else {
                Ok(None)
            };
        };
        let (transaction, activation) = decode_shell_indicator_activation(&self.inbox[at])?;
        let capacity = if self.content_limits.is_some() {
            self.control_capacity_available(1)
        } else {
            self.output.records() < 64
                && self.output.len().saturating_add(CONTROL_FRAME_BYTES) <= 2 * 1024 * 1024
        };
        if !capacity {
            return Ok(None); // The input frame still owns the unadmitted request.
        }
        self.indicator_response = Some(PendingIndicatorResponse {
            transaction,
            activation,
            outcome: None,
        });
        self.inbox.remove(at);
        Ok(Some((transaction, activation)))
    }

    /// Record the exact completed decision before attempting FIFO transfer.
    /// Saturation retains it for poll_io; the request cannot be readmitted.
    pub fn finish_indicator_activation(
        &mut self,
        transaction: TransactionId,
        activation: &ShellIndicatorActivation,
        status: ShellIndicatorActivationStatus,
        reason: u16,
    ) -> Result<(), ShellTransportError> {
        let pending = self
            .indicator_response
            .as_mut()
            .ok_or(ShellTransportError::WrongActivation)?;
        if pending.transaction != transaction || pending.activation != *activation {
            return Err(ShellTransportError::WrongActivation);
        }
        let outcome = ShellIndicatorActivationOutcome {
            connection_epoch: self.connection_epoch,
            snapshot_generation: activation.snapshot_generation,
            event_id: activation.event_id,
            status,
            reason,
        };
        if pending.outcome.is_some_and(|recorded| recorded != outcome) {
            return Err(ShellTransportError::WrongActivation);
        }
        pending.outcome = Some(outcome);
        self.flush_indicator_response()?;
        Ok(())
    }

    pub(super) fn flush_indicator_response(&mut self) -> Result<bool, ShellTransportError> {
        let Some(pending) = self.indicator_response else {
            return Ok(false);
        };
        let Some(outcome) = pending.outcome else {
            return Ok(false);
        };
        let frame = encode_shell_indicator_activation_outcome(pending.transaction, &outcome)?;
        if frame.len() > CONTROL_FRAME_BYTES {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        let capacity = if self.content_limits.is_some() {
            self.frame_capacity_available(frame.len(), true, true)
        } else {
            self.output.records() < 64
                && self.output.len().saturating_add(frame.len()) <= 2 * 1024 * 1024
        };
        if !capacity {
            return Ok(false);
        }
        self.output.push(frame, true);
        // Exact pending record was copied/validated above. This infallible clear
        // has no allocation, callback or socket I/O after FIFO ownership.
        self.indicator_response = None;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_indicator_responses.rs"]
mod tests;
