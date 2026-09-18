//! Session's admission policy for the private input service.
//!
//! WHAT THIS DECIDES IS ADMISSION, AND SEPARATELY WHETHER AUTHORITY MAY EVER
//! BE GRANTED. Those are not the same question and are deliberately not
//! answered together. A connection that presents no private evidence is still
//! an ordinary recipient of this service: it may connect, it may be served,
//! and it may never be issued a grant. A connection that presents evidence for
//! another instance is not a lesser recipient, it is a different one, and it
//! is refused the grant for the same reason.
//!
//! WHAT IS NEVER EVIDENCE. The peer's user id and the socket path say who may
//! reach this service, which is a containment question the boundary already
//! answers. Neither is carried into the grant decision. A caller that could
//! turn "I am the right user" into "I hold the input authority" would make
//! every process of that user an input source.

use sophia_protocol::{ClientAdmissionContext, ClientAdmissionId};
use sophia_runtime::NamespaceRegistry;
use sophia_x_authority::{
    XServerFrontendAdmissionError, XServerFrontendAdmissionPolicy, XServerFrontendAdmissionRequest,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::PrivateInputGrantPolicy;

/// What Session recorded about one admitted connection at the moment it was
/// admitted.
///
/// KEPT BECAUSE THE TRAIT CANNOT CARRY IT. `admit` returns only a
/// `ClientAdmissionContext`, so the verified evidence that arrived with the
/// setup has nowhere to go unless this policy keeps it. Issuance later asks
/// this record rather than re-deriving a decision from something it can see
/// at the time, because by then the setup is long over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrivateInputAdmissionRecord {
    /// Whether the setup carried verified evidence bound to this exact
    /// instance. Not "carried evidence": evidence for another instance leaves
    /// this false, and so does none at all.
    pub instance_verified: bool,
}

/// The policy Session installs on the private frontend.
///
/// Nothing constructs this until the service thread lands in the next change;
/// the expectation below fails once something does, which is the point.
#[expect(
    dead_code,
    reason = "wired by the service thread; the unfulfilled expectation is what removes this"
)]
pub(super) struct PrivateInputAdmissionPolicy {
    registry: Arc<Mutex<NamespaceRegistry>>,
    namespace: sophia_protocol::NamespaceId,
    instance: sophia_input_authority::InstanceId,
    grants: PrivateInputGrantPolicy,
    admitted: Arc<Mutex<BTreeMap<ClientAdmissionId, PrivateInputAdmissionRecord>>>,
}

impl PrivateInputAdmissionPolicy {
    #[expect(
        dead_code,
        reason = "called by the service thread; the unfulfilled expectation is what removes this"
    )]
    pub(super) fn new(
        registry: Arc<Mutex<NamespaceRegistry>>,
        namespace: sophia_protocol::NamespaceId,
        instance: sophia_input_authority::InstanceId,
        grants: PrivateInputGrantPolicy,
        admitted: Arc<Mutex<BTreeMap<ClientAdmissionId, PrivateInputAdmissionRecord>>>,
    ) -> Self {
        Self {
            registry,
            namespace,
            instance,
            grants,
            admitted,
        }
    }
}

impl XServerFrontendAdmissionPolicy for PrivateInputAdmissionPolicy {
    fn admit(
        &self,
        request: XServerFrontendAdmissionRequest,
    ) -> Result<ClientAdmissionContext, XServerFrontendAdmissionError> {
        // THE EVIDENCE IS JUDGED BEFORE THE ADMISSION IS MINTED, against this
        // service's own instance. The frontend has already verified the cookie
        // itself and refused a wrong or malformed one during setup; what
        // arrives here is either nothing or a verified authorization, and the
        // only thing left to decide is whether it names this instance.
        let instance_verified = request
            .verified_private_input
            .is_some_and(|verified| verified.instance() == self.instance);
        let context = self
            .registry
            .lock()
            .map_err(|_| XServerFrontendAdmissionError::Unavailable)?
            .admit(self.namespace, request.setup_authentication)
            .map_err(|_| XServerFrontendAdmissionError::Unavailable)?;
        self.admitted
            .lock()
            .map_err(|_| XServerFrontendAdmissionError::Unavailable)?
            .insert(
                context.client_id,
                PrivateInputAdmissionRecord { instance_verified },
            );
        Ok(context)
    }

    fn revoke(&self, context: ClientAdmissionContext) -> Result<(), XServerFrontendAdmissionError> {
        if context.namespace.id != self.namespace {
            return Err(XServerFrontendAdmissionError::Unavailable);
        }
        // The record goes first. A revocation that failed at the registry must
        // not leave this policy still willing to vouch for the admission at
        // issuance, and a record removed for an admission that was never here
        // costs nothing.
        if let Ok(mut admitted) = self.admitted.lock() {
            admitted.remove(&context.client_id);
        }
        self.registry
            .lock()
            .map_err(|_| XServerFrontendAdmissionError::Unavailable)?
            .revoke_admission(context.client_id)
            .map(|_| ())
            .map_err(|_| XServerFrontendAdmissionError::Unavailable)
    }
}

/// Why an issuance was refused.
///
/// SEPARATE CAUSES, NOT ONE DENIAL. Which of these it is decides what a caller
/// should do next, and collapsing them would make a disabled service and a
/// stale connection look alike.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrivateInputIssueRefusal {
    /// Grants are not enabled for this service at all.
    GrantsDisabled,
    /// This admission never presented evidence bound to this instance.
    NotInstanceVerified,
    /// Session has no record of this admission.
    UnknownAdmission,
    /// The registry no longer considers this admission current. A number that
    /// was reused by a later connection lands here.
    AdmissionSuperseded,
    /// The boundary has no live connection for this admission, or the one it
    /// has is closed.
    ConnectionGone,
    /// A lock this decision needs could not be read.
    Unavailable,
}

impl core::fmt::Display for PrivateInputIssueRefusal {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            Self::GrantsDisabled => "private input grants are not enabled",
            Self::NotInstanceVerified => {
                "the admission presented no verified evidence for this instance"
            }
            Self::UnknownAdmission => "no record of that admission",
            Self::AdmissionSuperseded => "that admission is no longer the current one",
            Self::ConnectionGone => "no live connection holds that admission",
            Self::Unavailable => "the admission record could not be read",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for PrivateInputIssueRefusal {}

/// Whether authority may be issued for this admission, right now.
///
/// EVERY CONDITION IS CHECKED AGAINST CURRENT STATE, not against what was true
/// when the connection was admitted. The record says what evidence arrived;
/// the registry says whether this admission is still the current one; the
/// boundary says whether a live connection still holds it. A caller that
/// satisfied all three a moment ago and none of them now is refused.
#[expect(
    dead_code,
    reason = "called by the service thread; the unfulfilled expectation is what removes this"
)]
pub(super) fn may_issue(
    grants: PrivateInputGrantPolicy,
    admitted: &Mutex<BTreeMap<ClientAdmissionId, PrivateInputAdmissionRecord>>,
    registry: &Mutex<NamespaceRegistry>,
    context: ClientAdmissionContext,
    live: &[sophia_x_authority::PrivateAdmittedConnection],
) -> Result<sophia_x_authority::XServerFrontendClientId, PrivateInputIssueRefusal> {
    if grants == PrivateInputGrantPolicy::Disabled {
        return Err(PrivateInputIssueRefusal::GrantsDisabled);
    }
    let record = admitted
        .lock()
        .map_err(|_| PrivateInputIssueRefusal::Unavailable)?
        .get(&context.client_id)
        .copied()
        .ok_or(PrivateInputIssueRefusal::UnknownAdmission)?;
    if !record.instance_verified {
        return Err(PrivateInputIssueRefusal::NotInstanceVerified);
    }
    if !registry
        .lock()
        .map_err(|_| PrivateInputIssueRefusal::Unavailable)?
        .is_current_admission(context)
    {
        return Err(PrivateInputIssueRefusal::AdmissionSuperseded);
    }
    // THE BOUNDARY'S OWN VIEW DECIDES WHICH CLIENT THIS IS. Session must not
    // guess a client number: the exact admission id and the connection
    // generation together name one connection, and a later connection that
    // reused the number disagrees on at least one of them.
    live.iter()
        .find(|seen| {
            seen.admission == context.client_id
                && seen.namespace == context.namespace.id
                && seen.connection_generation == context.auth_provenance.session_generation
                && !seen.closed
                && seen.lifecycle_open
        })
        .map(|seen| seen.client)
        .ok_or(PrivateInputIssueRefusal::ConnectionGone)
}
