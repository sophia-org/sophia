//! Late input uses the existing typed receipt/outcome owners without admission.
use super::*;

impl ShellComponentTransport {
    /// One bounded FIFO visit after exact Closed. Unanswered requests already
    /// handed to Session remain with that owner; this cannot guess its effect.
    /// A previously recorded Admitted response likewise remains Admitted.
    pub fn service_closed_native_input(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        expected: NativeLauncherOpening,
    ) -> Result<usize, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        if self.native_launcher_closed_opening() != Some(expected)
            || expected.grant != self.store_grant
        {
            return Err(ShellTransportError::WrongActivation);
        }
        self.poll_io(epochs)?;
        let maximum = self
            .content_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?
            .max_frames_per_service_tick
            .min(32) as usize;
        let mut processed = 0;
        while processed < maximum {
            let ack_pending = self.peek_native_input_ack()?.is_some();
            let activate_pending = self.peek_native_activate()?.is_some();
            if !ack_pending && !activate_pending {
                break;
            }
            if ack_pending {
                // The production decoder validates the grant and exact receipt.
                // Closed cleared input authority; a late valid ACK is stale.
                if self.take_native_launcher_input_ack()?.is_none() {
                    break;
                }
            } else {
                let Some((transaction, activation)) = self.take_native_launcher_request(epochs)?
                else {
                    break;
                };
                // Only this newly owned late request gets Stale. There is no
                // callback into the launch queue and no replay of prior effects.
                self.finish_native_launcher_activation(
                    epochs,
                    transaction,
                    &activation,
                    control::NativeLauncherActivationDecision::Stale,
                )?;
            }
            processed += 1;
        }
        if processed == 0 && self.peer_closed {
            return Err(ShellTransportError::NotConnected);
        }
        Ok(processed)
    }
}

impl ShellTransportConnection<'_> {
    pub fn service_closed_native_input(
        &mut self,
        expected: NativeLauncherOpening,
    ) -> Result<usize, ShellTransportError> {
        self.state
            .service_closed_native_input(self.content_epochs, expected)
    }
}
