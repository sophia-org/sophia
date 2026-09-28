//! One admitted indicator request owns a response credit until FIFO transfer.
//! Polling cannot hand the same request to the WM twice. The result is recorded
//! before fallible encoding/admission, so retry never repeats the effect.
use super::ShellComponentTransport;
use super::control_budget::{CONTROL_RECORD_BYTES, Class};
use super::outbound::OutboundRecord;
use super::{ShellSessionTransport, ShellTransportError};
use sophia_protocol::{
    ShellIndicatorActivation, ShellIndicatorActivationOutcome, ShellIndicatorActivationStatus,
    TransactionId,
};

#[derive(Clone, Copy)]
pub(super) struct PendingIndicatorResponse {
    transaction: TransactionId,
    activation: ShellIndicatorActivation,
    outcome: Option<ShellIndicatorActivationOutcome>,
}

impl ShellComponentTransport {
    pub fn poll_indicator_activation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellIndicatorActivation)>, ShellTransportError> {
        self.poll_io(epochs)?;
        self.take_indicator_request(epochs)
    }

    fn take_indicator_request(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellIndicatorActivation)>, ShellTransportError> {
        if !self.supports_indicator_activation() {
            return Err(ShellTransportError::MissingCapability);
        }
        if self.indicator_response.is_some() {
            return Ok(None);
        }
        let Some((transaction, activation)) = self.peek_indicator_activate()? else {
            return self.nothing_inbound();
        };
        let capacity = if self.content_limits.is_some() {
            self.control_capacity_available(epochs, 1)
        } else {
            self.unlimited_capacity_available(self.control_record_bytes())
        };
        if !capacity {
            return Ok(None); // The input record still owns the unadmitted request.
        }
        self.indicator_response = Some(PendingIndicatorResponse {
            transaction,
            activation,
            outcome: None,
        });
        self.take_indicator_activate();
        Ok(Some((transaction, activation)))
    }

    /// Record the exact completed decision before attempting FIFO transfer.
    /// Saturation retains it for poll_io; the request cannot be readmitted.
    pub fn finish_indicator_activation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
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
        self.flush_indicator_response(epochs)?;
        Ok(())
    }

    pub(super) fn flush_indicator_response(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<bool, ShellTransportError> {
        let Some(pending) = self.indicator_response else {
            return Ok(false);
        };
        let Some(outcome) = pending.outcome else {
            return Ok(false);
        };
        let transaction = pending.transaction;
        let admitted = self.admit_record(
            OutboundRecord::IndicatorOutcome(transaction, outcome),
            Class::Control {
                limit: CONTROL_RECORD_BYTES,
                oversize: ShellTransportError::ActivationQueueSaturated,
            },
        )?;
        let capacity = if self.content_limits.is_some() {
            self.record_capacity_available(epochs, admitted.charge, true, true)
        } else {
            self.unlimited_capacity_available(admitted.charge)
        };
        if !capacity {
            return Ok(false);
        }
        self.transfer_record(admitted);
        // Exact pending record was copied/validated above. This infallible clear
        // has no allocation, callback or socket I/O after FIFO ownership.
        self.indicator_response = None;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_indicator_responses.rs"]
mod tests;

// Legacy single-shell facade, delegating to the same shared registry path.

// The owned legacy and borrowed Session façades share forwarding, not policy.
macro_rules! transport_facade {
    ($transport:ty) => {
        impl $transport {
            pub fn poll_indicator_activation(
                &mut self,
            ) -> Result<Option<(TransactionId, ShellIndicatorActivation)>, ShellTransportError>
            {
                self.state
                    .poll_indicator_activation(&mut self.content_epochs)
            }

            pub fn finish_indicator_activation(
                &mut self,
                transaction: TransactionId,
                activation: &ShellIndicatorActivation,
                status: ShellIndicatorActivationStatus,
                reason: u16,
            ) -> Result<(), ShellTransportError> {
                self.state.finish_indicator_activation(
                    &mut self.content_epochs,
                    transaction,
                    activation,
                    status,
                    reason,
                )
            }
        }
    };
}
transport_facade!(ShellSessionTransport);
transport_facade!(crate::shell_transport::ShellTransportConnection<'_>);
