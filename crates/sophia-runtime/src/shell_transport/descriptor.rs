//! Descriptor request, presentation and activation ownership on the file export.
use super::descriptor_state::DescriptorState;
use super::{ShellComponentTransport, ShellTransportError};
use sophia_protocol::{
    SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS, ShellV1Activation, ShellV1ActivationAck,
    ShellV1Candidate, ShellV1CandidateOutcome, ShellV1CandidateOutcomeKind,
    ShellV1DescriptorSnapshot, TransactionId,
};
use std::time::Duration;
const DESCRIPTOR_TIMEOUT: Duration = Duration::from_secs(5);

impl ShellComponentTransport {
    fn descriptor(&self) -> Option<&DescriptorState> {
        self.file_descriptor().then_some(&self.descriptor_state)
    }

    fn descriptor_mut(&mut self) -> Result<&mut DescriptorState, ShellTransportError> {
        if !self.file_descriptor() {
            return Err(ShellTransportError::NotConnected);
        }
        Ok(&mut self.descriptor_state)
    }

    pub fn request_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellV1DescriptorSnapshot,
    ) -> Result<ShellV1Candidate, ShellTransportError> {
        self.begin_candidate_request(epochs, transaction, snapshot)?;
        let deadline = std::time::Instant::now() + DESCRIPTOR_TIMEOUT;
        loop {
            if let Some(candidate) = self.poll_candidate(epochs)? {
                return Ok(candidate);
            }
            if std::time::Instant::now() >= deadline {
                return Err(ShellTransportError::Io("shell candidate timed out".into()));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn begin_candidate_request(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        snapshot: &ShellV1DescriptorSnapshot,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(snapshot.connection_epoch)?;
        if self.descriptor().is_some_and(|descriptor| {
            descriptor.pending_candidate.is_some() || descriptor.requested_candidate.is_some()
        }) {
            return Err(ShellTransportError::WrongCandidate);
        }
        if !self.file_descriptor() {
            return Err(ShellTransportError::NotConnected);
        }
        self.begin_file_descriptor_request(epochs, transaction, snapshot)
    }

    pub fn poll_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellV1Candidate>, ShellTransportError> {
        if self.file_descriptor() {
            return self.poll_file_descriptor_candidate(epochs);
        }
        Ok(None)
    }

    pub fn send_candidate_outcome(
        &mut self,
        _epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        outcome: ShellV1CandidateOutcome,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(outcome.connection_epoch)?;
        let mut pending = self
            .descriptor()
            .and_then(|descriptor| descriptor.pending_candidate)
            .ok_or(ShellTransportError::WrongCandidate)?;
        if pending.transaction != transaction || pending.generation != outcome.candidate_generation
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        match outcome.kind {
            ShellV1CandidateOutcomeKind::Prepared if !pending.prepared => {
                pending.prepared = true;
            }
            ShellV1CandidateOutcomeKind::Presented if pending.prepared => {}
            ShellV1CandidateOutcomeKind::Rejected | ShellV1CandidateOutcomeKind::Superseded => {}
            _ => return Err(ShellTransportError::WrongCandidate),
        }
        self.file_descriptor_outcome(transaction, outcome)?;
        if !pending.prepared && !matches!(outcome.kind, ShellV1CandidateOutcomeKind::Prepared) {
            self.descriptor_state.response_credits -= 1;
        }
        let descriptor = self.descriptor_mut()?;
        match outcome.kind {
            ShellV1CandidateOutcomeKind::Prepared => {
                descriptor.pending_candidate = Some(pending);
            }
            ShellV1CandidateOutcomeKind::Presented => {
                descriptor.presented_candidate = Some((
                    pending.generation,
                    if pending.visible {
                        outcome.presentation_epoch
                    } else {
                        0
                    },
                ));
                descriptor.pending_candidate = None;
            }
            ShellV1CandidateOutcomeKind::Rejected | ShellV1CandidateOutcomeKind::Superseded => {
                descriptor.pending_candidate = None;
            }
        }
        Ok(())
    }

    pub fn queue_activation(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        transaction: TransactionId,
        activation: ShellV1Activation,
    ) -> Result<(), ShellTransportError> {
        self.require_epoch(activation.connection_epoch)?;
        if self
            .descriptor()
            .and_then(|descriptor| descriptor.presented_candidate)
            != Some((
                activation.candidate_generation,
                activation.presentation_epoch,
            ))
            || activation.action.recipient_epoch != self.connection_epoch
        {
            return Err(ShellTransportError::WrongActivation);
        }
        if self.descriptor().is_some_and(|descriptor| {
            descriptor.pending_activations.len() >= SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS
        }) {
            self.disconnect(epochs)?;
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        self.file_descriptor_activation(epochs, transaction, activation)?;
        self.descriptor_mut()?
            .pending_activations
            .push_back((transaction, activation.activation));
        Ok(())
    }

    pub fn receive_activation_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<ShellV1ActivationAck, ShellTransportError> {
        let deadline = std::time::Instant::now() + DESCRIPTOR_TIMEOUT;
        loop {
            if let Some(ack) = self.poll_activation_ack(epochs)? {
                return Ok(ack);
            }
            if std::time::Instant::now() >= deadline {
                return Err(ShellTransportError::Io(
                    "shell acknowledgement timed out".into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn poll_activation_ack(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellV1ActivationAck>, ShellTransportError> {
        if self.file_descriptor() {
            return self.poll_file_descriptor_ack(epochs);
        }
        Ok(None)
    }
}

// The owned legacy and borrowed Session façades share forwarding, not policy.
macro_rules! descriptor_facade {
    ($transport:ty) => {
        impl $transport {
            pub fn request_candidate(
                &mut self,
                transaction: TransactionId,
                snapshot: &ShellV1DescriptorSnapshot,
            ) -> Result<ShellV1Candidate, ShellTransportError> {
                self.state
                    .request_candidate(&mut self.content_epochs, transaction, snapshot)
            }

            pub fn begin_candidate_request(
                &mut self,
                transaction: TransactionId,
                snapshot: &ShellV1DescriptorSnapshot,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .begin_candidate_request(&mut self.content_epochs, transaction, snapshot)
            }

            pub fn poll_candidate(
                &mut self,
            ) -> Result<Option<ShellV1Candidate>, ShellTransportError> {
                self.state.poll_candidate(&mut self.content_epochs)
            }

            pub fn send_candidate_outcome(
                &mut self,
                transaction: TransactionId,
                outcome: ShellV1CandidateOutcome,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .send_candidate_outcome(&mut self.content_epochs, transaction, outcome)
            }

            pub fn queue_activation(
                &mut self,
                transaction: TransactionId,
                activation: ShellV1Activation,
            ) -> Result<(), ShellTransportError> {
                self.state
                    .queue_activation(&mut self.content_epochs, transaction, activation)
            }

            pub fn receive_activation_ack(
                &mut self,
            ) -> Result<ShellV1ActivationAck, ShellTransportError> {
                self.state.receive_activation_ack(&mut self.content_epochs)
            }

            pub fn poll_activation_ack(
                &mut self,
            ) -> Result<Option<ShellV1ActivationAck>, ShellTransportError> {
                self.state.poll_activation_ack(&mut self.content_epochs)
            }
        }
    };
}
descriptor_facade!(super::ShellSessionTransport);
descriptor_facade!(super::ShellTransportConnection<'_>);
