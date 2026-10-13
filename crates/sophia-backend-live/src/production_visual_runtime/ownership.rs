//! Who owns a frame retirement.
//!
//! The kernel retires what it scanned out, not what the scheduler currently
//! names, so a frame can retire after a later present has displaced it. That
//! is ordinary; a retirement naming a frame this session never submitted is
//! not, and telling the two apart is the whole job here.
//!
//! `PresentMixedOwnership` in `validation/tla` states the rule, and its
//! negative control is the version that judges by the current frame alone.

use super::*;

/// Everything that disagreed when a retirement failed the ownership check.
///
/// The check used to end the session with a sentence naming nothing, so the
/// only way to tell which transition produced it was to read the source and
/// infer from surrounding log lines. These are the facts that separate the
/// candidates, carried to the message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeOwnershipMismatch {
    pub retired_frame: LiveProductionNativeFrameId,
    pub content_frame: LiveProductionNativeFrameId,
    pub content_transaction: Option<TransactionId>,
    pub submitted_frame: Option<LiveProductionNativeFrameId>,
    pub in_flight_frame: Option<LiveProductionNativeFrameId>,
    pub in_flight_transaction: Option<TransactionId>,
}

impl LiveProductionNativeOwnershipMismatch {
    /// Which shape of disagreement this is.
    ///
    /// A scheduler holding nothing, a scheduler holding a later present, and a
    /// frame reserved but never submitted are three different bugs. Reporting
    /// them with one sentence is what made this expensive to diagnose.
    pub fn kind(self) -> &'static str {
        if self.content_frame != self.retired_frame {
            "content_names_another_frame"
        } else if self.in_flight_frame == Some(self.retired_frame) {
            "reserved_but_not_submitted"
        } else if self.submitted_frame.is_none() && self.in_flight_frame.is_none() {
            "no_present_in_flight"
        } else {
            "superseded_by_later_present"
        }
    }
}

impl core::fmt::Display for LiveProductionNativeOwnershipMismatch {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "DMA Present retired on a native frame with different ownership: \
kind={} retired_frame={} content_frame={} content_transaction={:?} \
submitted_frame={:?} in_flight_frame={:?} in_flight_transaction={:?}",
            self.kind(),
            self.retired_frame.raw(),
            self.content_frame.raw(),
            self.content_transaction.map(TransactionId::raw),
            self.submitted_frame.map(LiveProductionNativeFrameId::raw),
            self.in_flight_frame.map(LiveProductionNativeFrameId::raw),
            self.in_flight_transaction.map(TransactionId::raw),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeRetirementOwner {
    IndependentFrame,
    SubmittedDmaPresent,
    /// A frame this session submitted, displaced by a later present before the
    /// kernel retired it. Ordinary: the kernel retires what it scanned out,
    /// not what the scheduler now names. It settles without advancing present
    /// feedback, because a superseded frame is not the frame on glass.
    SupersededDmaPresent,
    InvalidDmaOwnership,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeSubmissionOwner {
    IndependentFrame,
    SubmittedDmaPresent,
    /// Present content whose identity the cohort no longer names. It is still
    /// this session's frame on its way to the kernel, so it is recorded as
    /// owned and its retirement settles quietly; the cohort is not advanced,
    /// because feedback for one present must not be earned by another's frame.
    StalePresentContent,
    /// Scene-driven content submitted while a present was still pending. The
    /// plane has moved on, so that present can never complete on this output
    /// and is skipped the way topology quiescence skips one: settled as
    /// Skipped so no client hangs waiting for feedback.
    OvertookPendingPresent,
    /// Content and cohort half-agree on identity: same frame with a different
    /// transaction, or same transaction with a different frame. That is state
    /// that has split rather than raced, and it stays fatal.
    InvalidDmaOwnership,
}

/// Classifies a physical submission before any Present state advances.
///
/// Ordinary compositor frames and DMA-BUF Presents share the same native
/// scheduler. The content tag and the output-scoped Present reservation must
/// therefore agree; the mere fact that KMS accepted a frame does not make it a
/// Present submission.
pub fn reduce_live_production_native_submission_owner(
    submitted_content: LiveProductionScanoutContent,
    expected_present: Option<(LiveProductionNativeFrameId, TransactionId)>,
) -> LiveProductionNativeSubmissionOwner {
    match (submitted_content, expected_present) {
        (
            LiveProductionScanoutContent::MixedPresent {
                frame, transaction, ..
            },
            Some((expected_frame, expected_transaction)),
        ) if frame == expected_frame && transaction == expected_transaction => {
            LiveProductionNativeSubmissionOwner::SubmittedDmaPresent
        }
        // Exactly one half of the identity matches. Content and cohort agree
        // on which present or which frame but not both, which is bookkeeping
        // that has split, not a race that has ordered itself badly.
        (
            LiveProductionScanoutContent::MixedPresent {
                frame, transaction, ..
            },
            Some((expected_frame, expected_transaction)),
        ) if frame == expected_frame || transaction == expected_transaction => {
            LiveProductionNativeSubmissionOwner::InvalidDmaOwnership
        }
        // Present content whose identity the cohort no longer names at all: a
        // stale mixed frame reaching the kernel after its present moved on.
        (LiveProductionScanoutContent::MixedPresent { .. }, _) => {
            LiveProductionNativeSubmissionOwner::StalePresentContent
        }
        // Scene-driven content submitting while a present is still pending:
        // the plane has moved on, and the pending present can no longer
        // complete on this output. Killing the session here is what a click
        // on a browser popup used to cost.
        (_, Some(_)) => LiveProductionNativeSubmissionOwner::OvertookPendingPresent,
        (_, None) => LiveProductionNativeSubmissionOwner::IndependentFrame,
    }
}

impl LiveProductionVisualRuntime {
    /// Assembles the mismatch record from the retirement and what the
    /// scheduler currently holds for that output.
    pub(super) fn ownership_mismatch(
        &self,
        output: OutputId,
        retirement: LiveProductionNativeFrameRetirement,
    ) -> LiveProductionNativeOwnershipMismatch {
        LiveProductionNativeOwnershipMismatch {
            retired_frame: retirement.frame,
            content_frame: retirement.content.frame(),
            content_transaction: match retirement.content {
                LiveProductionScanoutContent::MixedPresent { transaction, .. } => Some(transaction),
                _ => None,
            },
            submitted_frame: self.present_scheduler.submitted_frame(output),
            in_flight_frame: self.present_scheduler.in_flight_frame(output),
            in_flight_transaction: self.present_scheduler.in_flight_transaction(),
        }
    }
}

pub fn reduce_live_production_native_retirement_owner(
    retired_frame: LiveProductionNativeFrameId,
    retired_content: LiveProductionScanoutContent,
    submitted_dma_frame: Option<LiveProductionNativeFrameId>,
    // Whether this session ever gave the kernel this frame for this output.
    // Ownership is that question; `submitted_dma_frame` answers only which
    // frame is current.
    session_submitted: bool,
) -> LiveProductionNativeRetirementOwner {
    if retired_content.frame() != retired_frame {
        return LiveProductionNativeRetirementOwner::InvalidDmaOwnership;
    }
    match (retired_content, submitted_dma_frame) {
        (LiveProductionScanoutContent::MixedPresent { .. }, Some(submitted))
            if submitted == retired_frame =>
        {
            LiveProductionNativeRetirementOwner::SubmittedDmaPresent
        }
        (LiveProductionScanoutContent::MixedPresent { .. }, _) if session_submitted => {
            LiveProductionNativeRetirementOwner::SupersededDmaPresent
        }
        (LiveProductionScanoutContent::MixedPresent { .. }, _) => {
            LiveProductionNativeRetirementOwner::InvalidDmaOwnership
        }
        (_, _) => LiveProductionNativeRetirementOwner::IndependentFrame,
    }
}

impl LiveProductionVisualRuntime {
    /// Skips a present whose frame can no longer complete, without touching
    /// the exporter.
    ///
    /// Topology quiescence rolls back the skipped present's renderer image,
    /// and it may: nothing else is composing while it forces the seat quiet.
    /// A supersession skip runs in the middle of frame service, where the
    /// exporter is already at work on the successor — rolling the image back
    /// there poisoned the very next export, and one InvalidTarget later the
    /// session was gone. The skipped frame has retired; there is no pending
    /// layer to un-stage, only a live exporter to break.
    fn skip_superseded_present(&mut self) -> Option<u64> {
        let skipped = self
            .present_scheduler
            .take_submitted()
            .or_else(|| self.present_scheduler.take_rendering())?;
        let _ = self.reject_gpu_presentation(skipped.transaction);
        Some(skipped.transaction.raw())
    }

    /// Settles one submission against what the cohort expected of it.
    ///
    /// Only an exact identity match advances the cohort. Stale present
    /// content is remembered so its retirement is owned; scene content that
    /// overtook a pending present skips that present, settling its client as
    /// Skipped, because the plane has moved on and the wait can never end.
    /// Half-matching identity stays fatal, with every fact in the message.
    pub(super) fn settle_submission_ownership<T: NativeSubmissionTarget>(
        &mut self,
        native_scanout: &mut T,
        output: OutputId,
        submitted_content: LiveProductionScanoutContent,
        expected_present: Option<(LiveProductionNativeFrameId, TransactionId)>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let expected =
            expected_present.map(|(frame, transaction)| (frame.raw(), transaction.raw()));
        match reduce_live_production_native_submission_owner(submitted_content, expected_present) {
            LiveProductionNativeSubmissionOwner::IndependentFrame => {}
            LiveProductionNativeSubmissionOwner::SubmittedDmaPresent => {
                if let Some(transaction) = self.present_scheduler.mark_output_submitted(output)? {
                    native_scanout.discard_presentation_feedback(Some(output));
                    self.presentation_feedback
                        .resources_mut()
                        .mark_submitted(transaction)?;
                }
                native_scanout.activate_deferred_mirror_generation(output)?;
            }
            LiveProductionNativeSubmissionOwner::StalePresentContent => {
                // Ours, late. Own its retirement; advance nothing.
                if let Some(frame) = native_scanout.submitted_frame(output) {
                    self.present_scheduler
                        .remember_kernel_submission(output, frame);
                }
                tracing::warn!(
                    "sophia_live_native_scanout schema=1 status=superseded output={} reason=stale_present_submitted expected={expected:?}",
                    output.raw(),
                );
            }
            LiveProductionNativeSubmissionOwner::OvertookPendingPresent => {
                let skipped = self.skip_superseded_present();
                tracing::warn!(
                    "sophia_live_native_scanout schema=1 status=superseded output={} reason=present_overtaken_at_submit skipped_transaction={skipped:?} expected={expected:?}",
                    output.raw(),
                );
            }
            LiveProductionNativeSubmissionOwner::InvalidDmaOwnership => {
                return Err(format!(
                    "native output submission does not match its Present ownership: \
output={} submitted_content={submitted_content:?} expected={expected:?}",
                    output.raw(),
                )
                .into());
            }
        }
        Ok(())
    }

    /// Settles a superseded retirement, and the cohort that may still wait on
    /// it.
    ///
    /// Settling the frame while leaving its cohort was the zombie a browser
    /// popup exposed: the cohort's wait could never end, and the next scene
    /// submission found a pending present and died on it.
    pub(super) fn settle_superseded_retirement(
        &mut self,
        output: OutputId,
        frame: LiveProductionNativeFrameId,
    ) {
        tracing::warn!(
            "sophia_live_native_scanout schema=1 status=superseded output={} frame={} reason=retired_after_successor",
            output.raw(),
            frame.raw(),
        );
        if self.present_scheduler.in_flight_frame(output) == Some(frame) {
            let skipped = self.skip_superseded_present();
            tracing::warn!(
                "sophia_live_native_scanout schema=1 status=superseded output={} reason=present_skipped_after_supersession skipped_transaction={skipped:?}",
                output.raw(),
            );
        }
    }
}
