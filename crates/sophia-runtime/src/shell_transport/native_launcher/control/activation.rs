use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeLauncherActivationDecision {
    Admitted,
    Stale,
    Unknown,
    Unauthorized,
    Capacity,
}
impl NativeLauncherActivationDecision {
    fn wire(self) -> (u16, u16) {
        match self {
            Self::Admitted => (1, 0),
            Self::Stale => (2, ContentReason::Stale as u16),
            Self::Unknown => (3, ContentReason::Malformed as u16),
            Self::Unauthorized => (4, ContentReason::Unauthorized as u16),
            Self::Capacity => (5, ContentReason::Budget as u16),
        }
    }
}

/// Neither eligible variant is launch admission. Pointer additionally requires
/// the Session's actual ContentActionLedger; transport cancellation credits do
/// not substitute for that authority. Catalog/queue policy remains in Session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeLauncherActivationEligibility {
    Keyboard,
    Pointer,
    Rejected(NativeLauncherActivationDecision),
}

#[derive(Clone, Copy)]
pub(super) struct PendingNativeActivation {
    transaction: TransactionId,
    activation: NativeLauncherActivation,
    outcome: Option<NativeLauncherActivationOutcome>,
}

impl ShellComponentTransport {
    pub fn native_launcher_has_row(&self, binding: NativeLauncherBinding, slot: u16) -> bool {
        self.native_launcher_focus() == Some(binding)
            && self.native_control.revision == binding.state_revision
            && self
                .native_control
                .presented
                .is_some_and(|v| v.content.rows().contains(&slot))
    }
    pub fn poll_native_launcher_activation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, NativeLauncherActivation)>, ShellTransportError> {
        self.require_native_launcher(epochs)?;
        self.poll_io_bounded(epochs, 64 * 1024)?;
        self.take_native_launcher_request(epochs)
    }

    pub(in crate::shell_transport::native_launcher) fn take_native_launcher_request(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, NativeLauncherActivation)>, ShellTransportError> {
        if self.native_control.activation_response.is_some() {
            return Ok(None);
        }
        let Some(index) = self
            .inbox
            .iter()
            .position(|frame| u16::from_le_bytes([frame[6], frame[7]]) == 195)
        else {
            return if self.peer_closed {
                Err(ShellTransportError::NotConnected)
            } else {
                Ok(None)
            };
        };
        let (transaction, ShellNativeLauncherRecord::Activate(activation)) =
            decode_shell_native_launcher_frame(&self.inbox[index])?
        else {
            return Err(ShellTransportError::WrongContentRecord);
        };
        if Some(activation.event.binding.grant) != self.content_grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if !self.control_capacity_available(epochs, 1) {
            return Ok(None);
        }
        self.native_control.activation_response = Some(PendingNativeActivation {
            transaction,
            activation,
            outcome: None,
        });
        self.inbox.remove(index);
        Ok(Some((transaction, activation)))
    }

    pub fn native_launcher_activation_eligibility(
        &self,
        transaction: TransactionId,
        activation: &NativeLauncherActivation,
        catalog: &ShellApplicationCatalog,
        now_mono_usec: u64,
    ) -> Result<NativeLauncherActivationEligibility, ShellTransportError> {
        use NativeLauncherActivationDecision as D;
        use NativeLauncherActivationEligibility as E;
        let pending = self
            .native_control
            .activation_response
            .ok_or(ShellTransportError::WrongActivation)?;
        if pending.transaction != transaction
            || pending.activation != *activation
            || pending.outcome.is_some()
        {
            return Err(ShellTransportError::WrongActivation);
        }
        let binding = activation.event.binding;
        if self.native_launcher_focus() != Some(binding)
            || activation.event.state_revision != self.native_control.revision
            || catalog.connection_epoch != binding.grant.connection_epoch
            || catalog.generation != binding.catalog_generation
            || now_mono_usec < self.native_control.last_service
            || now_mono_usec < self.native_control.last_issued
        {
            return Ok(E::Rejected(D::Stale));
        }
        if catalog.entries.len() > SOPHIA_SHELL_MAX_APPLICATIONS {
            return Ok(E::Rejected(D::Unauthorized));
        }
        let mut entries = catalog
            .entries
            .iter()
            .filter(|entry| entry.slot == activation.slot);
        let Some(entry) = entries.next() else {
            return Ok(E::Rejected(D::Unknown));
        };
        if !entry.available || entries.next().is_some() {
            return Ok(E::Rejected(D::Unauthorized));
        }
        let shown = self
            .native_control
            .presented
            .expect("validated focus has presentation");
        if !shown.content.rows().contains(&activation.slot) {
            return Ok(E::Rejected(D::Unauthorized));
        }
        if activation.cause == 2 {
            return Ok(E::Pointer);
        }
        let timeout = u64::from(
            self.content_limits
                .as_ref()
                .ok_or(ShellTransportError::MissingCapability)?
                .action_ack_timeout_ms,
        ) * 1000;
        let issued = self.native_control.inputs.iter().flatten().any(|receipt| {
            receipt.kind == NativeLauncherInputKind::Accept
                && receipt.event == activation.event
                && !receipt.activation_attempted
                && now_mono_usec >= receipt.ack_started
                && now_mono_usec - receipt.ack_started < timeout
        });
        if activation.cause != 1 || !issued || activation.slot != shown.content.selected {
            return Ok(E::Rejected(D::Stale));
        }
        Ok(E::Keyboard)
    }

    /// Caller has made the queue decision. Preserve its first exact outcome
    /// before fallible FIFO transfer. This never invokes the launch owner.
    pub fn finish_native_launcher_activation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        activation: &NativeLauncherActivation,
        decision: NativeLauncherActivationDecision,
    ) -> Result<(), ShellTransportError> {
        let pending = self
            .native_control
            .activation_response
            .as_mut()
            .ok_or(ShellTransportError::WrongActivation)?;
        if pending.transaction != transaction || pending.activation != *activation {
            return Err(ShellTransportError::WrongActivation);
        }
        let (status, reason) = decision.wire();
        let outcome = NativeLauncherActivationOutcome {
            activation: *activation,
            status,
            reason,
        };
        if pending.outcome.is_some_and(|old| old != outcome) {
            return Err(ShellTransportError::WrongActivation);
        }
        pending.outcome = Some(outcome);
        if activation.cause == 1 {
            for receipt in self.native_control.inputs.iter_mut().flatten() {
                if receipt.event == activation.event {
                    receipt.activation_attempted = true;
                }
            }
        }
        if decision == NativeLauncherActivationDecision::Admitted
            && self.native_control.opening.is_some_and(|opening| {
                opening.grant == activation.event.binding.grant
                    && opening.opening == activation.event.binding.opening
            })
        {
            self.native_control.launch_admitted = true;
        }
        self.flush_native_activation(epochs)?;
        Ok(())
    }

    pub(in crate::shell_transport) fn flush_native_activation(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Result<bool, ShellTransportError> {
        let Some(pending) = self.native_control.activation_response else {
            return Ok(false);
        };
        let Some(outcome) = pending.outcome else {
            return Ok(false);
        };
        let frame = encode_shell_native_launcher_frame(
            pending.transaction,
            &ShellNativeLauncherRecord::ActivationOutcome(outcome),
        )?;
        if frame.len() > self.control_frame_bytes()
            || !self.frame_capacity_available(epochs, frame.len(), true, true)
        {
            return Ok(false);
        }
        self.output.push(frame, true);
        // Copy pending record: no allocation, callback, or I/O after transfer.
        self.native_control.activation_response = None;
        Ok(true)
    }
}

impl super::super::super::ShellTransportConnection<'_> {
    pub fn native_launcher_has_row(&self, binding: NativeLauncherBinding, slot: u16) -> bool {
        self.state.native_launcher_has_row(binding, slot)
    }
    pub fn poll_native_launcher_activation(
        &mut self,
    ) -> Result<Option<(TransactionId, NativeLauncherActivation)>, ShellTransportError> {
        self.state
            .poll_native_launcher_activation(self.content_epochs)
    }
    pub fn native_launcher_activation_eligibility(
        &self,
        transaction: TransactionId,
        activation: &NativeLauncherActivation,
        catalog: &ShellApplicationCatalog,
        now_mono_usec: u64,
    ) -> Result<NativeLauncherActivationEligibility, ShellTransportError> {
        self.state.native_launcher_activation_eligibility(
            transaction,
            activation,
            catalog,
            now_mono_usec,
        )
    }
    pub fn finish_native_launcher_activation(
        &mut self,
        transaction: TransactionId,
        activation: &NativeLauncherActivation,
        decision: NativeLauncherActivationDecision,
    ) -> Result<(), ShellTransportError> {
        self.state.finish_native_launcher_activation(
            self.content_epochs,
            transaction,
            activation,
            decision,
        )
    }
}

#[cfg(test)]
#[path = "../../../../tests/support/native_launcher_activation_owner.rs"]
mod tests;
