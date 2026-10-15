//! Typed boundary for the remaining descriptor socket compatibility path.
//! Framing and multi-frame publication stay here; presentation policy does not.
use super::super::{ShellComponentTransport, ShellTransportError};
use sophia_protocol::shell_files::ShellDescriptorRecord;
use sophia_protocol::*;

impl ShellComponentTransport {
    pub(in crate::shell_transport) fn begin_socket_descriptor_request(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellV1DescriptorSnapshot,
    ) -> Result<(), ShellTransportError> {
        let frame = encode_shell_v1_descriptor_snapshot_frame(transaction, snapshot)?;
        if self.socket().is_none() {
            return Err(ShellTransportError::NotConnected);
        }
        // Preserve the socket's refusal order: encoding precedes the obligation,
        // while a send failure leaves that obligation for disconnect to settle.
        self.descriptor_state.requested_candidate = Some((transaction, snapshot.clone()));
        self.send_async(epochs, frame)
    }

    pub(in crate::shell_transport) fn send_socket_descriptor(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        record: ShellDescriptorRecord,
    ) -> Result<(), ShellTransportError> {
        let frames = match record {
            ShellDescriptorRecord::Tabs(value) => encode_shell_tab_snapshot(transaction, &value)?,
            ShellDescriptorRecord::Shortcuts(value) => {
                encode_shell_shortcut_catalog(transaction, &value)?
            }
            ShellDescriptorRecord::DescriptorOutcome(value) => {
                vec![encode_shell_v1_candidate_outcome_frame(transaction, value)?]
            }
            ShellDescriptorRecord::DescriptorActivation(value) => {
                vec![encode_shell_v1_activation_frame(transaction, value)?]
            }
            ShellDescriptorRecord::ReferenceRequest(value) => {
                vec![encode_shell_reference_request(transaction, value)?]
            }
            ShellDescriptorRecord::ReferenceOutcome(value) => {
                vec![encode_shell_reference_outcome(transaction, value)?]
            }
            ShellDescriptorRecord::LauncherRequest(value) => {
                vec![encode_shell_launcher_request(transaction, &value)?]
            }
            ShellDescriptorRecord::LauncherOutcome(value) => {
                vec![encode_shell_launcher_outcome(transaction, value)?]
            }
            ShellDescriptorRecord::LauncherActivation(value) => {
                vec![encode_shell_launcher_activation(transaction, value)?]
            }
            ShellDescriptorRecord::LaunchOutcome(value) => {
                vec![encode_shell_launch_outcome(transaction, value)?]
            }
            _ => return Err(ShellTransportError::WrongContentRecord),
        };
        for frame in frames {
            self.send_async(epochs, frame)?;
        }
        Ok(())
    }

    pub(in crate::shell_transport) fn poll_socket_descriptor_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellV1Candidate)>, ShellTransportError> {
        let Some(frame) = self.poll_kind(epochs, IpcMessageKind::ShellV1Candidate)? else {
            return Ok(None);
        };
        self.descriptor_state.requested_candidate = None;
        Ok(Some(decode_shell_v1_candidate_frame(&frame)?))
    }

    pub(in crate::shell_transport) fn poll_socket_descriptor_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
    ) -> Result<Option<ShellV1ActivationAck>, ShellTransportError> {
        self.poll_transaction(epochs, IpcMessageKind::ShellV1ActivationAck, transaction)?
            .map(|frame| {
                decode_shell_v1_activation_ack_frame(&frame)
                    .map(|(_, ack)| ack)
                    .map_err(Into::into)
            })
            .transpose()
    }

    pub(in crate::shell_transport) fn poll_socket_tabs_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellTabCandidate)>, ShellTransportError> {
        self.poll_kind(epochs, IpcMessageKind::ShellTabsCandidate)?
            .map(|frame| decode_shell_tab_candidate(&frame).map_err(Into::into))
            .transpose()
    }

    pub(in crate::shell_transport) fn poll_socket_reference_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellReferenceCandidate)>, ShellTransportError> {
        self.poll_kind(epochs, IpcMessageKind::ShellReferenceCandidate)?
            .map(|frame| decode_shell_reference_candidate(&frame).map_err(Into::into))
            .transpose()
    }

    pub(in crate::shell_transport) fn poll_socket_launcher_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellLauncherCandidate)>, ShellTransportError> {
        self.poll_kind(epochs, IpcMessageKind::ShellLauncherCandidate)?
            .map(|frame| decode_shell_launcher_candidate(&frame).map_err(Into::into))
            .transpose()
    }

    pub(in crate::shell_transport) fn poll_socket_launcher_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<(TransactionId, ShellLauncherActivationAck)>, ShellTransportError> {
        self.poll_kind(epochs, IpcMessageKind::ShellLauncherActivationAck)?
            .map(|frame| decode_shell_launcher_activation_ack(&frame).map_err(Into::into))
            .transpose()
    }
}
