use super::*;

/// Identity retained across Engine settlement; it grants no presentation authority.
pub(super) struct SubmittedLayoutIdentity {
    candidate: SurfaceTransactionKey,
    image: LiveRendererImageId,
    format: u32,
    output: OutputId,
    frame: LiveProductionNativeFrameId,
}

impl SubmittedLayoutIdentity {
    pub(super) fn from_submitted(submitted: &LiveProductionSubmittedPresent) -> Option<Self> {
        let mut frames = submitted.frames();
        let (output, frame) = frames.next()?;
        if frames.next().is_some()
            || submitted.candidate.transaction != submitted.transaction
            || submitted.candidate.surface != submitted.surface
            || submitted.prepared.transaction() != submitted.transaction
            || !submitted.prepared.candidate().iter().any(|state| {
                state.surface == submitted.candidate.surface
                    && state.buffer() == submitted.candidate.target_buffer
            })
        {
            return None;
        }
        Some(Self {
            candidate: submitted.candidate,
            image: submitted.displayed_layer.image_id,
            format: submitted.displayed_layer.format,
            output,
            frame,
        })
    }

    pub(super) fn settle(
        self,
        retirement: &LiveProductionNativeFrameRetirement,
        commit: &TransactionCommit,
    ) -> Option<LiveProductionRetiredLayoutWitness> {
        let retired = retirement.layout_witness?;
        let witness = retired.witness;
        let trace = witness.alternative.trace?;
        (commit.outcome == TransactionOutcome::Committed
            && commit.transaction == self.candidate.transaction
            && commit.applied_surfaces.contains(&self.candidate.surface)
            && !retirement.direct
            && retirement.output == self.output
            && retirement.frame == self.frame
            && matches!(retirement.content, LiveProductionScanoutContent::MixedPresent { frame, transaction, .. }
                if frame == self.frame && transaction == self.candidate.transaction)
            && witness.source_image == self.image
            && present_for_renderer_image(witness.source_image) == self.candidate.transaction
            && witness.format == self.format
            && trace.output == self.output
            && trace.head == retired.head)
        .then_some(retired)
    }

    pub(super) fn settle_feedback(
        self,
        retirement: &LiveProductionNativeFrameRetirement,
        commit: &TransactionCommit,
        feedback: &mut crate::LivePresentFeedbackOutcome,
    ) -> Option<LiveProductionRetiredLayoutWitness> {
        feedback.layout_comparison = None;
        let candidate = self.candidate;
        let retired = self.settle(retirement, commit)?;
        let mut completions = feedback
            .feedback
            .iter()
            .filter_map(|feedback| match feedback {
                crate::LivePresentProtocolFeedback::Complete {
                    transaction,
                    disposition,
                    ..
                } => Some((*transaction, *disposition)),
                crate::LivePresentProtocolFeedback::Idle { .. } => None,
            });
        if completions.next()
            != Some((
                candidate.transaction,
                crate::LivePresentBufferDisposition::Copied,
            ))
            || completions.next().is_some()
        {
            return None;
        }
        feedback.layout_comparison = Some(Box::new(crate::LivePresentLayoutComparison {
            candidate,
            retired,
        }));
        Some(retired)
    }
}

#[path = "../../../tests/support/present_layout_witness.rs"]
mod tests;
