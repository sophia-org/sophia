//! Local owner settlement is not an acknowledgement that the peer is quiet.
use super::*;

impl ShellComponentTransport {
    /// Inspect actual retained stores and FIFO, including partial writes.
    /// High-water identities remain retained. This does not prove peer receipt
    /// or exclude additional old records still in the socket or peer outbox.
    pub fn closed_native_owners_settled(
        &self,
        epochs: &crate::ContentEpochRegistry,
        expected: NativeLauncherOpening,
    ) -> Result<bool, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        if self.native_launcher_closed_opening() != Some(expected)
            || expected.grant != self.store_grant
        {
            return Err(ShellTransportError::WrongActivation);
        }
        let resources = epochs
            .resources(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?;
        let candidates = epochs
            .active_candidates(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?;
        let allocations = epochs
            .allocations(self.store_grant)
            .ok_or(ShellTransportError::MissingCapability)?;
        Ok(resources.quiescent()
            && resources.control_occupancy() == 0
            && candidates.quiescent()
            && candidates.control_occupancy() == 0
            && allocations.quiescent()
            && allocations.control_occupancy() == 0
            && self.native_control.credits() == 0
            && self.native_control.input_occupancy() == 0
            && !self.peer_closed
            && self.input.is_empty()
            && self.action_cancellations.is_empty()
            && self.indicator_response.is_none()
            && self.output.is_empty()
            && self.inbox.is_empty())
    }
}

impl ShellTransportConnection<'_> {
    pub fn closed_native_owners_settled(
        &self,
        expected: NativeLauncherOpening,
    ) -> Result<bool, ShellTransportError> {
        self.state
            .closed_native_owners_settled(self.content_epochs, expected)
    }
}
