//! A software Present keeps its frozen CPU sources and guarded local images.
//! Policy previews are absent from retries, which change only one native ID.
use super::*;

impl LiveProductionVisualRuntime {
    pub(super) fn image_reads_for_sources(
        &self,
        sources: &[sophia_renderer_live::LiveOwnedHeadCompositionSource],
    ) -> Vec<sophia_renderer_live::LiveRendererImageRead> {
        self.image_reads.prune();
        sources
            .iter()
            .filter_map(|source| match source.kind {
                sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::RendererImage {
                    image_id,
                    ..
                } => Some(self.image_reads.acquire(image_id)),
                _ => None,
            })
            .collect()
    }

    pub(super) fn retry_software_present_after_preview_refusal<
        T: composition_target::PreviewRecoveryTarget,
    >(
        &mut self,
        native: &mut T,
        failure: crate::LivePreviewFrameFailure,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let frames =
            self.software_preview_recovery_frames(native, failure.output, failure.frame)?;
        let queued = native.queue_preview_present_replacement(
            failure,
            None,
            vec![(failure.output, frames)],
        )?;
        let new = *queued
            .get(&failure.output)
            .ok_or("software preview recovery admitted no frame")?;
        self.replace_software_preview_frame(failure.output, failure.frame, new)?;
        Ok(())
    }

    pub(super) fn software_preview_recovery_frames<T: NativeCompositionTarget>(
        &self,
        native: &T,
        output: OutputId,
        old: LiveProductionNativeFrameId,
    ) -> Result<Vec<crate::LiveProductionHeadCompositionFrame>, Box<dyn std::error::Error>> {
        let root = *self
            .software_present_frame_owners
            .get(&old)
            .ok_or("software preview recovery lost its binding")?;
        let binding = self
            .software_present_frames_bound
            .get(&root)
            .ok_or("software preview recovery lost its source set")?;
        if binding.frames.get(&output) != Some(&old)
            || binding.output_cohort.output_submitted(output)
        {
            return Err("software preview recovery does not own an unsubmitted frame".into());
        }
        let sources = &binding.source_set;
        let list = self.recovery_display_list_for_output(
            output,
            &sources.committed,
            &sources.presentation_order,
        )?;
        self.compose_native_head_frames_from_sources(
            native,
            output,
            &sources.committed,
            list,
            (sources.scene_generation, None),
            &sources.sources,
        )
    }

    pub(super) fn replace_software_preview_frame(
        &mut self,
        output: OutputId,
        old: LiveProductionNativeFrameId,
        new: LiveProductionNativeFrameId,
    ) -> Result<(), &'static str> {
        if new <= old
            || self.software_present_frame_owners.contains_key(&new)
            || self.software_present_frames_bound.contains_key(&new)
        {
            return Err("software preview recovery reused a native frame identity");
        }
        let root = *self
            .software_present_frame_owners
            .get(&old)
            .ok_or("software preview recovery lost its frame mapping")?;
        let binding = self
            .software_present_frames_bound
            .get_mut(&root)
            .ok_or("software preview recovery lost its binding")?;
        if binding.frames.get(&output) != Some(&old)
            || binding.output_cohort.output_submitted(output)
        {
            return Err("software preview recovery cannot replace a submitted frame");
        }
        binding.frames.insert(output, new);
        // The original first-frame ID is now only an opaque binding key. It
        // never substitutes for a physical frame in submission or retirement.
        self.software_present_frame_owners.remove(&old);
        self.software_present_frame_owners.insert(new, root);
        Ok(())
    }
}
