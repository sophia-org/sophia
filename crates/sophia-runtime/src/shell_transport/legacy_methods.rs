//! Compatibility forwarding only; all policy and transitions live in the shared transport.
use super::super::*;

// The owned legacy and borrowed Session façades share forwarding, not policy.
macro_rules! transport_facade {
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

            pub fn content_reserved_bytes(&self) -> u64 {
                self.state.content_reserved_bytes(&self.content_epochs)
            }

            pub fn content_backing_reserved_bytes(&self) -> u64 {
                self.state
                    .content_backing_reserved_bytes(&self.content_epochs)
            }

            pub fn content_usage(&self) -> Option<crate::ContentMemoryUsage> {
                self.state.content_usage(&self.content_epochs)
            }

            pub fn lease_content_resource(
                &self,
                grant: ContentGrant,
                resource: sophia_protocol::ContentResourceId,
            ) -> Result<crate::ContentResourceLease, ShellTransportError> {
                self.state
                    .lease_content_resource(&self.content_epochs, grant, resource)
            }

            pub fn poll_io(&mut self) -> Result<(), ShellTransportError> {
                self.state.poll_io(&mut self.content_epochs)
            }

            pub fn poll_io_bounded(&mut self, bytes: usize) -> Result<(), ShellTransportError> {
                self.state.poll_io_bounded(&mut self.content_epochs, bytes)
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
        impl $transport {
            pub fn socket_path(&self) -> &Path {
                self.state.socket_path()
            }

            pub const fn connection_epoch(&self) -> u64 {
                self.state.connection_epoch()
            }

            pub fn content_limits(&self) -> Option<&ContentLimits> {
                self.state.content_limits()
            }

            pub const fn supports_shortcut_catalog(&self) -> bool {
                self.state.supports_shortcut_catalog()
            }

            pub const fn supports_reference(&self) -> bool {
                self.state.supports_reference()
            }

            pub const fn supports_launcher(&self) -> bool {
                self.state.supports_launcher()
            }

            pub const fn supports_tabs(&self) -> bool {
                self.state.supports_tabs()
            }

            pub const fn supports_indicators(&self) -> bool {
                self.state.supports_indicators()
            }

            pub const fn supports_indicator_activation(&self) -> bool {
                self.state.supports_indicator_activation()
            }

            pub const fn supports_content(&self) -> bool {
                self.state.supports_content()
            }

            pub const fn supports_content_discrete_input(&self) -> bool {
                self.state.supports_content_discrete_input()
            }

            pub const fn content_grant(&self) -> Option<ContentGrant> {
                self.state.content_grant()
            }
        }
    };
}
transport_facade!(ShellSessionTransport);
transport_facade!(crate::shell_transport::ShellTransportConnection<'_>);

// Admission and disconnect stay on the owning compatibility transport.
impl ShellSessionTransport {
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
    pub fn disconnect(&mut self) -> Result<(), ShellTransportError> {
        self.state.disconnect(&mut self.content_epochs)
    }
    pub fn authorize_protected_peer(
        &mut self,
        evidence: &ProtectionDomainEvidence,
    ) -> Result<(), ShellTransportError> {
        self.state.authorize_protected_peer(evidence)
    }
}
