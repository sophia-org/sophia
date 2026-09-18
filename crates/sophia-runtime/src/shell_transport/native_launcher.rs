//! Native launcher role over the same protected connection and content owners.
//! Explicit reservation is required; legacy live startup never selects this role.
use super::*;
use sophia_protocol::*;

mod closed_content;
mod closed_input;
mod content;
pub(crate) mod control;

const CAPABILITIES: u64 = SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER;

impl ShellComponentTransport {
    pub(super) fn select_native_launcher_negotiation(
        &self,
        connection_epoch: u64,
        policy: ShellContentAdmissionPolicy,
        hello: ShellV1ClientHello,
    ) -> Result<(ShellV1ServerWelcome, Option<ContentLimits>), ShellTransportError> {
        if hello.minimum_revision == 0
            || hello.minimum_revision > SOPHIA_SHELL_NATIVE_LAUNCHER_REVISION
            || hello.maximum_revision < SOPHIA_SHELL_NATIVE_LAUNCHER_REVISION
            || hello.minimum_revision > hello.maximum_revision
        {
            return Err(ShellTransportError::UnsupportedRevision);
        }
        // Native launch is neither descriptor launch nor indicator activation.
        // Additional bits are not silently admitted for this protection domain.
        if hello.required_capabilities != CAPABILITIES {
            return Err(ShellTransportError::MissingCapability);
        }
        let reason = match policy {
            ShellContentAdmissionPolicy::Unavailable => Some(content_admission::UNAVAILABLE),
            ShellContentAdmissionPolicy::Denied
            | ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            } => Some(content_admission::PERMISSION_DENIED),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            } => None,
        };
        if let Some(reason) = reason {
            return Err(ShellTransportError::ContentAdmissionRefused(
                ContentAdmissionRefused {
                    reason,
                    denied_capabilities: CAPABILITIES,
                },
            ));
        }
        let limits = self
            .reserved_limits
            .as_ref()
            .ok_or(ShellTransportError::MissingCapability)?;
        if limits.grant.connection_epoch != connection_epoch {
            return Err(ShellTransportError::WrongContentGrant);
        }
        Ok((
            ShellV1ServerWelcome {
                selected_revision: SOPHIA_SHELL_NATIVE_LAUNCHER_REVISION,
                connection_epoch,
                capabilities: CAPABILITIES,
                max_descriptors: SOPHIA_SHELL_MAX_DESCRIPTORS as u16,
                max_label_bytes: MAX_CHROME_LABEL_LEN as u16,
                max_pending_activations: SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS as u16,
            },
            Some(limits.clone()),
        ))
    }

    pub const fn supports_native_launcher(&self) -> bool {
        self.capabilities & SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER != 0
    }

    fn require_native_launcher(
        &self,
        epochs: &crate::ContentEpochRegistry,
    ) -> Result<(), ShellTransportError> {
        if !self.supports_native_launcher()
            || self.content_grant != Some(self.store_grant)
            || epochs.profile(self.store_grant) != Some(crate::ContentStoreProfile::NativeLauncher)
        {
            return Err(ShellTransportError::MissingCapability);
        }
        Ok(())
    }

    /// Session publishes its owned opening on the existing FIFO. This only
    /// queues a notification; it grants no focus or application launch authority.
    pub fn publish_native_launcher_opening(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        transaction: TransactionId,
        opening: NativeLauncherOpening,
    ) -> Result<(), ShellTransportError> {
        self.require_native_launcher(epochs)?;
        if Some(opening.grant) != self.content_grant {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if self.native_control.opening.is_some()
            || opening.opening <= self.native_control.last_opening
        {
            return Err(ShellTransportError::WrongActivation);
        }
        let frame = encode_shell_native_launcher_frame(
            transaction,
            &ShellNativeLauncherRecord::Opening(opening),
        )?;
        if frame.len() > super::control_budget::CONTROL_FRAME_BYTES
            || !self.control_capacity_available(epochs, 2)
        {
            return Err(ShellTransportError::ContentQueueSaturated);
        }
        self.output.push(frame, true);
        self.native_control.opening = Some(opening);
        self.native_control.last_opening = opening.opening;
        self.native_control.revision = opening.state_revision;
        Ok(())
    }
}

impl super::ShellTransportConnection<'_> {
    pub const fn supports_native_launcher(&self) -> bool {
        self.state.supports_native_launcher()
    }

    pub fn publish_native_launcher_opening(
        &mut self,
        transaction: TransactionId,
        opening: NativeLauncherOpening,
    ) -> Result<(), ShellTransportError> {
        self.state
            .publish_native_launcher_opening(self.content_epochs, transaction, opening)
    }

    pub fn service_native_launcher_content(
        &mut self,
        context: crate::ContentCandidateContext<'_>,
        current: crate::NativeLauncherCandidateContext<'_>,
        now_msec: u64,
    ) -> Result<usize, ShellTransportError> {
        self.state
            .service_native_launcher_content(self.content_epochs, context, current, now_msec)
    }

    pub fn begin_native_launcher_submission(
        &mut self,
        generation: u64,
        context: crate::ContentCandidateContext<'_>,
        current: crate::NativeLauncherCandidateContext<'_>,
        now_msec: u64,
    ) -> Result<crate::ContentRenderBundle, ShellTransportError> {
        self.state.begin_native_launcher_submission(
            self.content_epochs,
            generation,
            context,
            current,
            now_msec,
        )
    }
}

impl super::ShellTransportConnection<'_> {
    pub fn native_launcher_state(&self) -> Option<(NativeLauncherOpening, u64)> {
        self.state.native_launcher_state()
    }
    pub fn native_launcher_closed_opening(&self) -> Option<NativeLauncherOpening> {
        self.state.native_launcher_closed_opening()
    }
    pub fn native_launcher_focus(&self) -> Option<NativeLauncherBinding> {
        self.state.native_launcher_focus()
    }
    pub fn install_native_launcher_focus(
        &mut self,
        transaction: TransactionId,
    ) -> Result<NativeLauncherBinding, ShellTransportError> {
        self.state
            .install_native_launcher_focus(self.content_epochs, transaction)
    }
    pub fn issue_native_launcher_input(
        &mut self,
        expected: NativeLauncherBinding,
        transaction: TransactionId,
        kind: NativeLauncherInputKind,
        text: &str,
        issued_mono_usec: u64,
    ) -> Result<Option<NativeLauncherEvent>, ShellTransportError> {
        self.state.issue_native_launcher_input(
            self.content_epochs,
            expected,
            transaction,
            kind,
            text,
            issued_mono_usec,
        )
    }
    pub fn service_native_launcher_deadlines(
        &mut self,
        expected: NativeLauncherOpening,
        transaction: TransactionId,
        now_mono_usec: u64,
    ) -> Result<bool, ShellTransportError> {
        self.state.service_native_launcher_deadlines(
            self.content_epochs,
            expected,
            transaction,
            now_mono_usec,
        )
    }
    pub fn poll_native_launcher_input_ack(
        &mut self,
    ) -> Result<Option<(TransactionId, NativeLauncherInputAck, bool)>, ShellTransportError> {
        self.state
            .poll_native_launcher_input_ack(self.content_epochs)
    }
    pub fn close_native_launcher(
        &mut self,
        expected: NativeLauncherOpening,
        transaction: TransactionId,
        reason: ContentReason,
    ) -> Result<(), ShellTransportError> {
        self.state
            .close_native_launcher(self.content_epochs, expected, transaction, reason)
    }
}
