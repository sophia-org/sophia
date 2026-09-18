//! Protected peer negotiation shared by standalone and component owners.
use super::*;

impl ShellComponentTransport {
    /// Reserve the complete footprint before the supervisor launches a peer.
    /// This is operator-side storage preparation, not a negotiated capability.
    /// A refused reservation changes neither this connection nor a neighbor.
    /// The caller must disconnect this owner on launch failure or abandonment.
    pub fn reserve_content(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        limits: ContentLimits,
    ) -> Result<(), ShellTransportError> {
        self.reserve_content_with_profile(epochs, limits, crate::ContentStoreProfile::Legacy)
    }

    /// The Session role selects storage before any peer is accepted. A client
    /// request cannot change this profile or borrow another role's reservation.
    pub fn reserve_content_with_profile(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        limits: ContentLimits,
        profile: crate::ContentStoreProfile,
    ) -> Result<(), ShellTransportError> {
        if self.negotiation.is_some()
            || self.stream.is_some()
            || self.content_grant.is_some()
            || self.reserved_limits.is_some()
            || epochs.resources(self.store_grant).is_some()
        {
            return Err(ShellTransportError::WrongContentGrant);
        }
        if limits.grant.connection_epoch <= self.connection_epoch {
            return Err(ShellTransportError::InvalidConnectionEpoch);
        }
        epochs.admit_with_profile(limits.clone(), profile)?;
        self.store_grant = limits.grant;
        self.reserved_limits = Some(limits);
        Ok(())
    }

    pub fn accept_and_negotiate(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.accept_and_negotiate_with_content_policy(
            epochs,
            connection_epoch,
            timeout,
            ShellContentAdmissionPolicy::Unavailable,
        )
    }

    /// Blocking compatibility driver over the same retained negotiation. The
    /// timeout now bounds the entire handshake, not each separate I/O stage.
    pub fn accept_and_negotiate_with_content_policy(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        connection_epoch: u64,
        timeout: Duration,
        content_policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.begin_negotiation(epochs, connection_epoch, timeout, content_policy)?;
        loop {
            if let Some(welcome) = self.poll_negotiation(epochs, 64 * 1024)? {
                return Ok(welcome);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
