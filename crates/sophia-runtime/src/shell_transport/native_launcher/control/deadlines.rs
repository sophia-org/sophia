use super::*;

impl ShellComponentTransport {
    /// Visit before servicing input/ACKs at the Session monotonic boundary.
    /// True means the exact opening was disarmed for timeout, not that its
    /// pixels were withdrawn or its resource consumers have retired. New input
    /// also calls this boundary; idle expiration requires the owner's visits.
    pub fn service_native_launcher_deadlines(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        expected: NativeLauncherOpening,
        transaction: TransactionId,
        now_mono_usec: u64,
    ) -> Result<bool, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        if self.native_control.opening != Some(expected)
            || !transaction.is_valid()
            || now_mono_usec < self.native_control.last_issued
            || now_mono_usec < self.native_control.last_service
        {
            return Err(ShellTransportError::WrongActivation);
        }
        let limits = self
            .content_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?;
        let expired_input = self.native_control.inputs.iter().flatten().any(|receipt| {
            receipt.ack.is_none()
                && now_mono_usec.saturating_sub(receipt.issued)
                    >= u64::from(limits.action_ack_timeout_ms) * 1000
        });
        let expired_accept = self.native_control.accept.is_some_and(|intent| {
            now_mono_usec.saturating_sub(intent.issued)
                >= u64::from(limits.presentation_timeout_ms) * 1000
        });
        self.native_control.last_service = now_mono_usec;
        if self.native_control.closing.is_some() {
            self.flush_native_close(epochs)?;
            return Ok(true);
        }
        if expired_input || expired_accept {
            self.close_native_launcher(epochs, expected, transaction, ContentReason::Timeout)?;
            return Ok(true);
        }
        Ok(false)
    }
}
