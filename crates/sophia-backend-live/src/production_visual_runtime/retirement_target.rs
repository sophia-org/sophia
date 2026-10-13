//! Native retirement boundary. Frame ownership and settlement remain in the
//! visual runtime; the target owns completion and renderer-image custody.
use super::*;

pub(crate) trait NativeRetirementTarget: NativeCompositionTarget {
    fn submitted_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId>;
    fn retire_ready(
        &mut self,
        output: OutputId,
        runtime: &mut LiveBackendRuntimeAssembly,
        draining: bool,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn take_presentation_feedback(
        &mut self,
        output: OutputId,
    ) -> Option<LiveProductionNativeFrameRetirement>;
    fn promote_renderer_image(
        &mut self,
        image: LiveRendererImageId,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail>;
    fn promote_renderer_image_for_previews(
        &mut self,
        image: LiveRendererImageId,
        source: Option<SurfaceId>,
        capturing_outputs: &BTreeSet<OutputId>,
        preview_outputs: &BTreeSet<OutputId>,
    ) -> Result<(usize, Result<(), LivePreviewImageRefusal>), LiveRendererScanoutBufferExportDetail>;
    fn evict_renderer_image(
        &mut self,
        image: LiveRendererImageId,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail>;
}

impl NativeRetirementTarget for LiveProductionNativeScanout {
    fn submitted_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
        LiveProductionNativeScanout::submitted_frame(self, output)
    }

    fn retire_ready(
        &mut self,
        output: OutputId,
        runtime: &mut LiveBackendRuntimeAssembly,
        draining: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if draining {
            self.retire_ready_for_drain(output, runtime)
        } else {
            self.retire_ready_and_retry_cleanup(output, runtime)
        }
    }

    fn take_presentation_feedback(
        &mut self,
        output: OutputId,
    ) -> Option<LiveProductionNativeFrameRetirement> {
        LiveProductionNativeScanout::take_presentation_feedback(self, output)
    }

    fn promote_renderer_image(
        &mut self,
        image: LiveRendererImageId,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail> {
        LiveProductionNativeScanout::promote_renderer_image(self, image)
    }

    fn promote_renderer_image_for_previews(
        &mut self,
        image: LiveRendererImageId,
        source: Option<SurfaceId>,
        capturing_outputs: &BTreeSet<OutputId>,
        preview_outputs: &BTreeSet<OutputId>,
    ) -> Result<(usize, Result<(), LivePreviewImageRefusal>), LiveRendererScanoutBufferExportDetail>
    {
        LiveProductionNativeScanout::promote_renderer_image_for_previews(
            self,
            image,
            source,
            capturing_outputs,
            preview_outputs,
        )
    }

    fn evict_renderer_image(
        &mut self,
        image: LiveRendererImageId,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail> {
        LiveProductionNativeScanout::evict_renderer_image(self, image)
    }
}
