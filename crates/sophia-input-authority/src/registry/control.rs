//! Privileged control remains available after input grants have been revoked.
use super::{AuthorityInstance, IssuerHandle, RegistrationError};
use crate::identity::AuthorityUid;

/// An authority lifetime, distinct even when two instances have identical
/// public seat bindings. Comparing this identity grants no mutation rights.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorityIdentity(pub(super) AuthorityUid);

/// Exclusive access for a privileged adapter-side control transaction.
///
/// The integration caller acquires its common mutex before obtaining this
/// permit, retains both through later-ranked adapter state changes, and drops
/// them before sending receipts. The permit names that exact authority and
/// keeps its exclusive borrow alive; it does not acquire a mutex itself.
/// It exposes no input execution, grant issuance, or authority mutation API.
///
/// A control transaction cannot overlap another mutation of its authority:
/// ```compile_fail
/// use sophia_input_authority::*;
/// use sophia_protocol::SeatId;
/// let (mut authority, issuer, _) = AuthorityInstance::new(
///     SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1)),
///     Capacity::PLANNED, 9).unwrap();
/// let permit = authority.control_permit(&issuer).unwrap();
/// authority.begin_transition(&issuer, 1, 1).unwrap();
/// let _ = permit.identity();
/// ```
pub struct ControlPermit<'a> {
    authority: &'a mut AuthorityInstance,
}

impl ControlPermit<'_> {
    pub fn identity(&self) -> AuthorityIdentity {
        AuthorityIdentity(self.authority.uid)
    }
}

impl AuthorityInstance {
    /// Bind a coordinator to this authority lifetime. This remains readable
    /// during a transition; it says nothing about routing being available.
    pub fn authority_identity(
        &self,
        issuer: &IssuerHandle,
    ) -> Result<AuthorityIdentity, RegistrationError> {
        self.check_issuer(issuer)?;
        Ok(AuthorityIdentity(self.uid))
    }

    /// Reserve exclusive access for privileged control, independent of grants,
    /// publication availability, request cells, and release-attempt capacity.
    pub fn control_permit(
        &mut self,
        issuer: &IssuerHandle,
    ) -> Result<ControlPermit<'_>, RegistrationError> {
        self.check_issuer(issuer)?;
        Ok(ControlPermit { authority: self })
    }
}
