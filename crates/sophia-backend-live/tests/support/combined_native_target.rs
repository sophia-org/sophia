#![cfg(all(test, feature = "experimental-combined-frame-job"))]
//! G3-5a: the combined fixture as the runtime's composition target, over its
//! one native authority. Each method is the owner's (composition_target.rs)
//! for a singleton output with no preview or topology state.
use super::*;
use crate::CombinedNativeFixture as Fixture;

impl NativeCompositionTarget for Fixture {
    fn frame_owner(&self) -> crate::NativeFrameOwner {
        self.native_owner
    }
    fn frame_service_available(&self) -> bool {
        true
    }
    fn head_targets(&self, output: OutputId) -> Vec<HeadRenderTarget> {
        Fixture::head_targets(self, output)
    }
    fn has_in_flight_direct(&self) -> bool {
        self.heads.iter().any(|head| head.submitted_direct)
    }
    /// `retained_retirements_ready`.
    fn required_outputs_ready(&self, outputs: &BTreeSet<OutputId>) -> bool {
        outputs
            .iter()
            .all(|output| self.queue_ready(*output) && !self.protected(*output))
    }
    fn queue_retained_batch(
        &mut self,
        frames: Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
        required: &BTreeSet<OutputId>,
    ) -> Result<BTreeMap<OutputId, crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>>
    {
        self.queue_retained(frames, required)
    }
    fn queue_ordinary_batch(
        &mut self,
        frames: Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>>
    {
        self.queue_ordinary(frames)
    }
    fn retained_repaint_deferred(&self) -> bool {
        self.heads.iter().any(|head| self.protected(head.output.id))
    }
    fn presented_frame_id(&self, output: OutputId) -> Option<crate::LiveProductionNativeFrameId> {
        self.heads
            .iter()
            .find(|head| head.enabled && head.output.id == output)?
            .presented_content
            .map(crate::LiveProductionScanoutContent::frame)
    }
    fn presented_frame(&self, output: OutputId) -> Option<&OutputFrameDamageSnapshot> {
        self.heads
            .iter()
            .find(|head| head.enabled && head.output.id == output)?
            .output_frames
            .presented()
    }
    /// The fixture never presents an idle frame.
    fn idle_presented_frame(
        &self,
        _output: OutputId,
        _outputs: &LiveProductionOutputRuntimeSet,
    ) -> Option<composition_target::IdleNativePresentation<'_>> {
        None
    }
}
