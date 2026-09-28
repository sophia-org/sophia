//! The legacy descriptor profile (Narthex), which exists only on the socket:
//! its snapshot, candidate, outcome and activation exchange, and the raw frame
//! calls the single-process metadata shell still uses. The file contract has
//! no descriptor families; these go with the socket, or with that profile's
//! own file contract (t271).
use std::collections::VecDeque;
use std::time::Duration;

use sophia_protocol::{
    IpcMessageKind, SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS, ShellV1Activation, ShellV1ActivationAck,
    ShellV1Candidate, ShellV1CandidateOutcome, ShellV1CandidateOutcomeKind,
    ShellV1DescriptorSnapshot, ShellV1ServerWelcome, TransactionId,
    decode_shell_v1_activation_ack_frame, decode_shell_v1_candidate_frame,
    encode_shell_v1_activation_frame, encode_shell_v1_candidate_outcome_frame,
    encode_shell_v1_descriptor_snapshot_frame,
};

use super::super::wire::Wire;
use super::super::{ShellComponentTransport, ShellContentAdmissionPolicy, ShellTransportError};

const DESCRIPTOR_TIMEOUT: Duration = Duration::from_secs(5);

/// Descriptor exchange state of one socket epoch; a new epoch starts empty.
pub(in crate::shell_transport) struct DescriptorState {
    last_candidate_generation: u64,
    requested_candidate: Option<(TransactionId, ShellV1DescriptorSnapshot)>,
    pending_candidate: Option<PendingShellCandidate>,
    presented_candidate: Option<(u64, u64)>,
    pending_activations: VecDeque<(TransactionId, u64)>,
}

impl Default for DescriptorState {
    fn default() -> Self {
        Self {
            last_candidate_generation: 0,
            requested_candidate: None,
            pending_candidate: None,
            presented_candidate: None,
            pending_activations: VecDeque::with_capacity(SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingShellCandidate {
    transaction: TransactionId,
    generation: u64,
    visible: bool,
    prepared: bool,
}

impl ShellComponentTransport {
    fn descriptor(&self) -> Option<&DescriptorState> {
        self.socket().map(|socket| &socket.descriptor)
    }

    fn descriptor_mut(&mut self) -> Result<&mut DescriptorState, ShellTransportError> {
        self.socket_mut()
            .map(|socket| &mut socket.descriptor)
            .ok_or(ShellTransportError::NotConnected)
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
        let frame = encode_shell_v1_descriptor_snapshot_frame(transaction, snapshot)?;
        self.descriptor_mut()?.requested_candidate = Some((transaction, snapshot.clone()));
        self.send_async(epochs, frame)
    }

    pub fn poll_candidate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> Result<Option<ShellV1Candidate>, ShellTransportError> {
        let Some((transaction, snapshot)) = self
            .descriptor()
            .and_then(|descriptor| descriptor.requested_candidate.clone())
        else {
            return Ok(None);
        };
        let Some(frame) = self.poll_kind(epochs, IpcMessageKind::ShellV1Candidate)? else {
            return Ok(None);
        };
        self.descriptor_mut()?.requested_candidate = None;
        let (response_transaction, candidate) = decode_shell_v1_candidate_frame(&frame)?;
        if response_transaction != transaction {
            return Err(ShellTransportError::WrongTransaction);
        }
        self.require_epoch(candidate.connection_epoch)?;
        let descriptor = self.descriptor_mut()?;
        if candidate.snapshot_generation != snapshot.snapshot_generation
            || candidate.output != snapshot.output
            || candidate.candidate_generation <= descriptor.last_candidate_generation
            || candidate.entries.iter().any(|entry| {
                !snapshot.descriptors.iter().any(|descriptor| {
                    descriptor.slot == entry.slot && descriptor.generation == entry.generation
                })
            })
        {
            return Err(ShellTransportError::WrongCandidate);
        }
        descriptor.last_candidate_generation = candidate.candidate_generation;
        descriptor.pending_candidate = Some(PendingShellCandidate {
            transaction,
            generation: candidate.candidate_generation,
            visible: candidate.visible,
            prepared: false,
        });
        descriptor.requested_candidate = None;
        Ok(Some(candidate))
    }

    pub fn send_candidate_outcome(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
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
        let frame = encode_shell_v1_candidate_outcome_frame(transaction, outcome)?;
        self.send_async(epochs, frame)?;
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
        let frame = encode_shell_v1_activation_frame(transaction, activation)?;
        self.send_async(epochs, frame)?;
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
        let Some((expected_transaction, expected_activation)) = self
            .descriptor()
            .and_then(|descriptor| descriptor.pending_activations.front().copied())
        else {
            return Ok(None);
        };
        let Some(frame) = self.poll_transaction(
            epochs,
            IpcMessageKind::ShellV1ActivationAck,
            expected_transaction,
        )?
        else {
            return Ok(None);
        };
        let (_, ack) = decode_shell_v1_activation_ack_frame(&frame)?;
        self.require_epoch(ack.connection_epoch)?;
        if ack.activation != expected_activation {
            return Err(ShellTransportError::WrongActivation);
        }
        self.descriptor_mut()?.pending_activations.pop_front();
        Ok(Some(ack))
    }

    pub fn send_async(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        frame: Vec<u8>,
    ) -> Result<(), ShellTransportError> {
        self.enqueue_async(epochs, frame)?;
        self.poll_io(epochs)
    }

    /// Transfer one bulk frame into the socket's lane of the shared bounded
    /// output, with no I/O after transfer. Returned refusal always precedes
    /// ownership transfer. Producers may then remove their prevalidated exact
    /// front without an I/O ambiguity. The file wire carries no raw frame.
    pub fn enqueue_async(
        &mut self,
        epochs: &crate::ContentEpochRegistry,
        frame: Vec<u8>,
    ) -> Result<(), ShellTransportError> {
        if self.socket().is_none() {
            return Err(ShellTransportError::NotConnected);
        }
        if !self.bulk_capacity_available(epochs, frame.len()) {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        let Some(Wire::Socket(socket)) = self.wire.as_mut() else {
            unreachable!("checked above");
        };
        socket.push_lane(&mut self.output, frame, false);
        Ok(())
    }

    pub fn poll_kind(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        kind: IpcMessageKind,
    ) -> Result<Option<Vec<u8>>, ShellTransportError> {
        self.poll_io(epochs)?;
        let result = self
            .socket_mut()
            .and_then(|socket| socket.take_frame(|frame| super::kind(frame) == kind as u16));
        if result.is_none() && self.peer_closed {
            return Err(ShellTransportError::NotConnected);
        }
        Ok(result)
    }

    pub fn poll_transaction(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        kind: IpcMessageKind,
        tx: TransactionId,
    ) -> Result<Option<Vec<u8>>, ShellTransportError> {
        self.poll_io(epochs)?;
        let frame = self.socket_mut().and_then(|socket| {
            socket.take_frame(|frame| {
                super::kind(frame) == kind as u16
                    && u64::from_le_bytes(frame[8..16].try_into().unwrap()) == tx.raw()
            })
        });
        if frame.is_none() && self.peer_closed {
            return Err(ShellTransportError::NotConnected);
        }
        Ok(frame)
    }
}

impl super::SocketWire {
    /// Removes the oldest frame the predicate selects, preserving the rest.
    fn take_frame(&mut self, select: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
        let at = self.inbox.iter().position(|frame| select(frame))?;
        self.inbox.remove(at)
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

            pub fn enqueue_async(&mut self, frame: Vec<u8>) -> Result<(), ShellTransportError> {
                self.state.enqueue_async(&self.content_epochs, frame)
            }

            pub fn send_async(&mut self, frame: Vec<u8>) -> Result<(), ShellTransportError> {
                self.state.send_async(&mut self.content_epochs, frame)
            }

            pub fn poll_kind(
                &mut self,
                kind: sophia_protocol::IpcMessageKind,
            ) -> Result<Option<Vec<u8>>, ShellTransportError> {
                self.state.poll_kind(&mut self.content_epochs, kind)
            }

            pub fn poll_transaction(
                &mut self,
                kind: sophia_protocol::IpcMessageKind,
                tx: TransactionId,
            ) -> Result<Option<Vec<u8>>, ShellTransportError> {
                self.state
                    .poll_transaction(&mut self.content_epochs, kind, tx)
            }
        }
    };
}
descriptor_facade!(super::super::ShellSessionTransport);
descriptor_facade!(super::super::ShellTransportConnection<'_>);

impl super::super::ShellSessionTransport {
    pub fn accept_and_negotiate(
        &mut self,
        connection_epoch: u64,
        timeout: Duration,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.state
            .accept_and_negotiate(&mut self.content_epochs, connection_epoch, timeout)
    }
    pub fn accept_and_negotiate_with_content_policy(
        &mut self,
        connection_epoch: u64,
        timeout: Duration,
        content_policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.state.accept_and_negotiate_with_content_policy(
            &mut self.content_epochs,
            connection_epoch,
            timeout,
            content_policy,
        )
    }
}
