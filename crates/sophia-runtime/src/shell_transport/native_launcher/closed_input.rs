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
        self.poll_io_bounded(epochs, 64 * 1024)?;
        let maximum = self
            .content_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?
            .max_frames_per_service_tick
            .min(32) as usize;
        let mut processed = 0;
        while processed < maximum {
            let Some(kind) = self.inbox.iter().find_map(|frame| {
                let kind = u16::from_le_bytes([frame[6], frame[7]]);
                matches!(kind, 194 | 195).then_some(kind)
            }) else {
                break;
            };
            if kind == 194 {
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
