//! The session's side of a combined frame job, as the runtime sees it
//! (GATE3-SCOPE.md G3-1). The runtime keeps its own admission, submission
//! and reconciliation order and calls these hooks at fixed points; it never
//! learns the backend's plan type `P`.
use crate::{
    CombinedFrameAdmitted, CombinedInbox, CombinedReservation, IsbValidationFailure,
    IsbValidationWorker, ValidatedIsbPixels,
};
use sophia_protocol::shell_files::ShellFrameCommit;
use std::sync::mpsc;

/// What `collect` hands to the runtime for one job, and what a refusal
/// returns untouched.
pub type CombinedResult = Result<ValidatedIsbPixels, IsbValidationFailure>;

/// What the runtime read for a frame before its admission (GATE3-SCOPE
/// G3-5b): the description admission derives in this same call, and the
/// allocations of the current store.
pub struct CombinedFrameFacts<'a> {
    pub description: &'a sophia_protocol::ContentResourceBegin,
    pub allocations: &'a [crate::ContentAllocationSnapshot],
}

/// Service 1's answer for one proposal.
pub enum CombinedReserve<P: Send + 'static> {
    /// Not combined: today's admission and render path, accounting unchanged.
    Ordinary,
    /// A ticket and its inbox; the ticket ends in `cancel` or `submitted`
    /// within this proposal.
    Combined(u64, CombinedInbox<P>),
    /// An unclassified composition failure. Nothing was reserved and the
    /// frame is not admitted; the owner turn fails with this first error.
    Fail(String),
}

/// Every ticket `reserve` returns ends in exactly one of `cancel` or
/// `submitted`.
pub trait CombinedFrameHooks<P: Send + 'static> {
    /// Service 1, before any runtime reservation for a copy-bearing frame on
    /// the active ring of a terminal-only, lean frame stream: composition,
    /// whole-plan eligibility and the renderer reservation.
    fn reserve(
        &mut self,
        frame: &ShellFrameCommit,
        facts: &CombinedFrameFacts<'_>,
    ) -> CombinedReserve<P>;

    /// After the runtime admitted the copy, the plan for that exact
    /// admission. `None` is a plan-build failure, withdrawn as a refused send.
    fn plan(&mut self, ticket: u64, admitted: &CombinedFrameAdmitted<'_>) -> Option<P>;

    /// The admission ended before the validator owned the job.
    fn cancel(&mut self, ticket: u64);

    /// The validator owns the job. The row takes the reservation and the
    /// receiver on which a failed validation or a refused move returns the
    /// plan.
    fn submitted(
        &mut self,
        ticket: u64,
        reservation: CombinedReservation,
        returned: mpsc::Receiver<P>,
    );

    /// Service 2, first in the image owner's service: drain each row's
    /// result, settle its credit against `worker`, and pass what came back to
    /// `deliver`, which is `receive_forwarded`. A refused delivery returns the
    /// result, which the row keeps with its charge obligation.
    fn collect(
        &mut self,
        worker: &mut IsbValidationWorker<P>,
        deliver: &mut dyn FnMut(u64, CombinedResult) -> Result<(), CombinedResult>,
    );
}
