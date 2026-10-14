//! Device boundary for DMA Present preparation and submission. Candidate
//! preparation, source selection, cohort construction and settlement stay in
//! the visual runtime's shared driver.
use super::*;

pub(crate) trait NativePresentTarget: NativeCycleTarget + NativeSubmissionTarget {
    fn pending_frame(&self, output: OutputId) -> bool;
    fn frame_queue_ready(&self, output: OutputId) -> bool;
    fn output_retirement_protected(&self, output: OutputId) -> bool;
    fn has_preview_frame_failure(&self, output: OutputId) -> bool;
    fn submitted_content(&self, output: OutputId) -> Option<LiveProductionScanoutContent>;
    fn queue_present_output_head_composition_frames(
        &mut self,
        transaction: TransactionId,
        frames: Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>>;
    fn rollback_renderer_image(
        &mut self,
        image: LiveRendererImageId,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail>;
    fn diagnose_mixed_frame(
        &mut self,
        output: OutputId,
        frame: LiveOwnedMixedCompositionFrame,
    ) -> (
        LiveRendererScanoutBufferExportStatus,
        LiveRendererScanoutBufferExportDetail,
    );
}

impl NativePresentTarget for LiveProductionNativeScanout {
    fn pending_frame(&self, output: OutputId) -> bool {
        LiveProductionNativeScanout::pending_frame(self, output)
    }

    fn frame_queue_ready(&self, output: OutputId) -> bool {
        LiveProductionNativeScanout::frame_queue_ready(self, output)
    }

    fn output_retirement_protected(&self, output: OutputId) -> bool {
        LiveProductionNativeScanout::output_retirement_protected(self, output)
    }

    fn has_preview_frame_failure(&self, output: OutputId) -> bool {
        LiveProductionNativeScanout::has_preview_frame_failure(self, output)
    }

    fn submitted_content(&self, output: OutputId) -> Option<LiveProductionScanoutContent> {
        LiveProductionNativeScanout::submitted_content(self, output)
    }

    fn queue_present_output_head_composition_frames(
        &mut self,
        transaction: TransactionId,
        frames: Vec<(OutputId, Vec<LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, LiveProductionNativeFrameId>, Box<dyn std::error::Error>> {
        LiveProductionNativeScanout::queue_present_output_head_composition_frames(
            self,
            transaction,
            frames,
        )
    }

    fn rollback_renderer_image(
        &mut self,
        image: LiveRendererImageId,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail> {
        LiveProductionNativeScanout::rollback_renderer_image(self, image)
    }

    fn diagnose_mixed_frame(
        &mut self,
        output: OutputId,
        frame: LiveOwnedMixedCompositionFrame,
    ) -> (
        LiveRendererScanoutBufferExportStatus,
        LiveRendererScanoutBufferExportDetail,
    ) {
        LiveProductionNativeScanout::diagnose_mixed_frame(self, output, frame)
    }
}
