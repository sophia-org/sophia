use super::*;

impl LiveProductionNativeScanout {
    pub(super) fn settle_deferred_renderer_content(
        &mut self,
        index: usize,
    ) -> Result<(), &'static str> {
        let identity = self.exporters[index]
            .queued_frame_correlation()
            .and_then(|correlation| correlation.native)
            .ok_or("deferred renderer has no queued native identity")?;
        let frame = LiveProductionNativeFrameId::from_raw(identity.frame());
        if identity != self.native_frame_identity(index, self.heads[index].output.id, frame) {
            return Err("deferred renderer names another owner/head/target");
        }
        let head = &mut self.heads[index];
        let cohort = head.rendering_content.and_then(|content| {
            self.output_cohorts
                .get_mut(&(head.output.id, content.frame()))
                .map(|cohort| &mut cohort.presentation)
        });
        head.defer_renderer_content(frame, cohort)
    }
}

impl LiveProductionNativeHead {
    pub(super) fn defer_renderer_content(
        &mut self,
        queued: LiveProductionNativeFrameId,
        cohort: Option<&mut sophia_engine::OutputPresentationCohort>,
    ) -> Result<(), &'static str> {
        let rendering = self
            .rendering_content
            .ok_or("deferred renderer has no rendering content")?;
        let next = self.pending_content.unwrap_or(rendering);
        if next.frame() != queued {
            return Err("deferred renderer queue does not match retained content");
        }
        if cohort.as_ref().is_some_and(|cohort| {
            cohort.output() != self.output.id
                || cohort.scene_generation() != rendering.frame().raw()
        }) {
            return Err("deferred renderer cohort does not match rendered content");
        }
        // Deferred means no target was rendered. Match the exporter's latest-
        // wins queue: retry the returned frame only if no successor replaced it.
        let damage = self.output_frames.discard_rendering();
        let damage = if self.pending_content.is_some() {
            self.output_frames.discard_pending()
        } else {
            damage
        };
        self.rendering_content = None;
        self.pending_content = Some(next);
        // Recompute relative to the submitted/displayed predecessor, not the
        // render that never happened. The renderer has its own per-slot damage.
        if let Some(damage) = damage {
            self.output_frames
                .queue(damage.snapshot)
                .map_err(|_| "deferred renderer damage could not be requeued")?;
        }
        if next.frame() != rendering.frame()
            && let Some(cohort) = cohort
        {
            // No buffer was produced for this member. Settle the same stale
            // generation that a completed, coalesced preparation would skip.
            let _ = cohort.mark_skipped(self.head);
        }
        Ok(())
    }
}

#[path = "../../../../tests/support/native_renderer_deferral.rs"]
mod tests;
