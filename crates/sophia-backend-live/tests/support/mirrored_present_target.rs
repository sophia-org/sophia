//! Deferred worker completion uses the actual composition queue and custody.
//! Renderer allocation and device completion remain supplied.
use super::*;

impl NativeSubmissionTarget for MirroredTarget {
    fn submitted_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
        NativeRetirementTarget::submitted_frame(self, output)
    }

    fn discard_presentation_feedback(&mut self, output: Option<OutputId>) {
        self.retirements
            .as_mut()
            .unwrap()
            .discard_retirements(output);
    }

    fn activate_deferred_mirror_generation(&mut self, _: OutputId) -> Result<bool, &'static str> {
        // prepare() has already accepted the exact generation into every
        // mirror's custody. This fixture never queues a later generation here.
        Ok(false)
    }
}

impl NativePresentTarget for MirroredTarget {
    fn pending_frame(&self, output: OutputId) -> bool {
        self.queue.pending(output)
    }

    fn frame_queue_ready(&self, output: OutputId) -> bool {
        self.ready(output)
    }

    fn has_preview_frame_failure(&self, output: OutputId) -> bool {
        self.preview_failures.contains_key(&output)
    }

    fn submitted_content(&self, output: OutputId) -> Option<LiveProductionScanoutContent> {
        let frame = NativeRetirementTarget::submitted_frame(self, output)?;
        self.outputs[&output]
            .iter()
            .find_map(|index| self.heads[*index].content.filter(|c| c.frame() == frame))
    }

    fn queue_present_output_head_composition_frames(
        &mut self,
        transaction: TransactionId,
        frames: Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        assert!(!frames.is_empty());
        let required = frames.iter().map(|(output, _)| *output).collect();
        self.queue_scene_batch(
            frames,
            &required,
            crate::LiveProductionHeadCompositionContent::MixedPresent(transaction),
        )
    }

    fn rollback_renderer_image(
        &mut self,
        image: LiveRendererImageId,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail> {
        if self
            .images
            .get(&image)
            .is_some_and(|image| !image.is_promoted())
        {
            self.images.remove(&image);
            Ok(1)
        } else {
            Ok(0)
        }
    }

    fn diagnose_mixed_frame(
        &mut self,
        _: OutputId,
        _: LiveOwnedMixedCompositionFrame,
    ) -> (
        LiveRendererScanoutBufferExportStatus,
        LiveRendererScanoutBufferExportDetail,
    ) {
        panic!("fixture cannot diagnose a native GPU export")
    }
}
