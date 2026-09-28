//! Raw-frame operations and legacy descriptor facades. Descriptor exchange
//! state and transitions live in the transport-neutral descriptor owner.
use super::super::wire::Wire;
use super::super::{ShellComponentTransport, ShellContentAdmissionPolicy, ShellTransportError};
use sophia_protocol::{
    IpcMessageKind, ShellV1Activation, ShellV1ActivationAck, ShellV1Candidate,
    ShellV1CandidateOutcome, ShellV1DescriptorSnapshot, ShellV1ServerWelcome, TransactionId,
};
use std::time::Duration;

impl ShellComponentTransport {
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
