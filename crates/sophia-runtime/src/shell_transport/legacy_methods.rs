//! Compatibility forwarding only; all policy and transitions live in the shared transport.
use super::super::*;

// The owned legacy and borrowed Session façades share forwarding, not policy.
macro_rules! transport_facade {
    ($transport:ty) => {
        impl $transport {
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
    /// Negotiate the single-shell owner over 9P2000.L. This uses the same
    /// admission, deadline and content registry as independent components.
    pub fn accept_files_with_content_policy(
        &mut self,
        connection_epoch: u64,
        timeout: Duration,
        content_policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.state.begin_file_negotiation(
            &self.content_epochs,
            connection_epoch,
            timeout,
            content_policy,
        )?;
        loop {
            if let Some(welcome) = self
                .state
                .poll_negotiation(&mut self.content_epochs, 64 * 1024)?
            {
                return Ok(welcome);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn disconnect(&mut self) -> Result<(), ShellTransportError> {
        let result = self.state.disconnect(&mut self.content_epochs);
        // A repeated disconnect cannot revoke the already-retired grant, but
        // legacy callers also use it after their last render consumer ends.
        // Collect the actual owners even when no new revocation took place.
        self.content_epochs.collect();
        result
    }
    pub fn authorize_protected_peer(
        &mut self,
        evidence: &ProtectionDomainEvidence,
    ) -> Result<(), ShellTransportError> {
        self.state.authorize_protected_peer(evidence)
    }
}
