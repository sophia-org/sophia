//! Who may inject, answered by the issuance Session already performs.
//!
//! The frontend asks this once, at setup, and a connection it refuses finds
//! XTEST absent rather than present and unusable. Session has exactly one
//! rule for that question -- the grant policy, and the instance-verified
//! admission that `issue` checks -- so this translates that answer rather
//! than deciding again. Two rules in two places could disagree, and the
//! disagreement would be a client that can see an extension it may not use.

use std::sync::{Arc, OnceLock, Weak};

use sophia_protocol::{ClientAdmissionContext, DeviceId, Point, SurfaceId};
use sophia_x_authority::{
    PrivateRequestBarrier, PrivateSendError, XServerFrontendInjectionError,
    XServerFrontendInjectionPolicy, XTestAccepted, XTestInjectionRefusal, XTestInjector,
};

use super::admission::PrivateInputIssueRefusal;
use super::service::PrivateInputRuntime;
use super::submission::{PrivateInputAccepted, PrivateInputSubmission, PrivateInputSubmitError};

/// The frontend's injection decision, delegated to Session's issuance.
pub(super) struct PrivateInputInjectionPolicy {
    /// WEAK, BECAUSE THE RUNTIME OWNS WHAT OWNS THIS. The frontend holds this
    /// policy and the runtime holds the frontend, so a strong reference here
    /// would be a cycle that never ends the service.
    ///
    /// Set once, and before the socket exists: see
    /// `PrivateInputRuntime::publish_injection`.
    runtime: OnceLock<Weak<PrivateInputRuntime>>,
}

impl PrivateInputInjectionPolicy {
    pub(super) fn new() -> Self {
        Self {
            runtime: OnceLock::new(),
        }
    }

    /// Give the policy the runtime it issues from. Reports false if one is
    /// already published, which is a caller's mistake rather than a state to
    /// recover from: the second runtime would answer for the first's clients.
    pub(super) fn publish(&self, runtime: &Arc<PrivateInputRuntime>) -> bool {
        self.runtime.set(Arc::downgrade(runtime)).is_ok()
    }
}

impl XServerFrontendInjectionPolicy for PrivateInputInjectionPolicy {
    fn issue(
        &self,
        context: ClientAdmissionContext,
        device: DeviceId,
    ) -> Result<Box<dyn XTestInjector>, XServerFrontendInjectionError> {
        // No runtime yet, or the service has ended. Neither is a decision
        // about this client, so neither is a denial.
        let runtime = self
            .runtime
            .get()
            .and_then(Weak::upgrade)
            .ok_or(XServerFrontendInjectionError::Unavailable)?;
        super::handle::issue_submission(&runtime, context, device)
            .map(|submission| {
                Box::new(PrivateInputInjector { submission }) as Box<dyn XTestInjector>
            })
            .map_err(injection_error)
    }
}

/// A refusal to issue, as the frontend distinguishes them: what the instance
/// does not offer at all, against what this admission may not have.
fn injection_error(refusal: PrivateInputIssueRefusal) -> XServerFrontendInjectionError {
    match refusal {
        // The service grants nothing to anyone, and a lock it could not read
        // established nothing either. Both describe the instance.
        PrivateInputIssueRefusal::GrantsDisabled | PrivateInputIssueRefusal::Unavailable => {
            XServerFrontendInjectionError::Unavailable
        }
        // Each of these is about this admission: unverified, unknown,
        // superseded by a later connection at the same number, or no longer
        // live. The client is the reason, so the client is told so.
        PrivateInputIssueRefusal::NotInstanceVerified
        | PrivateInputIssueRefusal::UnknownAdmission
        | PrivateInputIssueRefusal::AdmissionSuperseded
        | PrivateInputIssueRefusal::ConnectionGone => XServerFrontendInjectionError::Denied,
    }
}

/// One admitted connection's means of injecting, and nothing else.
struct PrivateInputInjector {
    submission: PrivateInputSubmission,
}

impl XTestInjector for PrivateInputInjector {
    fn report_completions_to(&self, barrier: PrivateRequestBarrier) -> bool {
        self.submission.report_completions_to(barrier)
    }

    fn submit_key(
        &self,
        target: SurfaceId,
        keycode: u32,
        pressed: bool,
    ) -> Result<XTestAccepted, XTestInjectionRefusal> {
        accepted(self.submission.submit_key(target, keycode, pressed))
    }

    fn submit_button(
        &self,
        target: SurfaceId,
        button: u32,
        pressed: bool,
    ) -> Result<XTestAccepted, XTestInjectionRefusal> {
        accepted(
            self.submission
                .submit_pointer_button(target, button, pressed),
        )
    }

    fn submit_motion(
        &self,
        target: SurfaceId,
        global: Point,
        local: Point,
    ) -> Result<XTestAccepted, XTestInjectionRefusal> {
        accepted(self.submission.submit_pointer_motion(target, global, local))
    }
}

/// What the order answered, in the adapter's vocabulary.
///
/// Acceptance keeps only the position: the delivery, serial and timestamp a
/// submission also reports are Session's own accounting, and an adapter that
/// could read them could report a request finished when it had only been
/// ordered. What says it finished is the completion at the barrier.
///
/// THE REFUSAL IS NARROWED, NOT DISCARDED. The order hands the work back with
/// its reason; the caller across this boundary cannot resubmit that value, so
/// only the reason crosses, and it keeps the distinction the caller acts on:
/// wait, stop, or nothing was established.
fn accepted(
    result: Result<PrivateInputAccepted, PrivateInputSubmitError>,
) -> Result<XTestAccepted, XTestInjectionRefusal> {
    match result {
        Ok(accepted) => Ok(XTestAccepted {
            sequence: Some(accepted.sequence),
        }),
        Err(PrivateInputSubmitError::Refused(error)) => Err(match error {
            PrivateSendError::Saturated(_) => XTestInjectionRefusal::Saturated,
            PrivateSendError::Disconnected(_) => XTestInjectionRefusal::Disconnected,
            PrivateSendError::Unavailable(_) => XTestInjectionRefusal::Unavailable,
            PrivateSendError::Denied(_) | PrivateSendError::ForeignServiceOwner(_) => {
                XTestInjectionRefusal::Denied
            }
            // Terminal in the same way an exhausted identity is: this
            // delivery is already live, and retrying cannot make a second.
            PrivateSendError::DeliveryAlreadyTracked(_) | PrivateSendError::Exhausted(_) => {
                XTestInjectionRefusal::Exhausted
            }
        }),
        Err(PrivateInputSubmitError::Ended) => Err(XTestInjectionRefusal::Disconnected),
        Err(PrivateInputSubmitError::Exhausted) => Err(XTestInjectionRefusal::Exhausted),
    }
}
