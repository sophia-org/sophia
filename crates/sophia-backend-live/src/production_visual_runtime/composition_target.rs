//! Device boundary for the shared retained-content intake and publication path.
//! Production supplies the native owner; fixtures substitute only target facts
//! and completion input while using the production planner, queue and custody.
use super::*;

pub(crate) trait NativeCompositionTarget {
    fn frame_owner(&self) -> crate::NativeFrameOwner;
    fn frame_service_available(&self) -> bool;
    fn head_targets(&self, output: OutputId) -> Vec<HeadRenderTarget>;
    fn has_in_flight_direct(&self) -> bool;
    fn required_outputs_ready(&self, outputs: &BTreeSet<OutputId>) -> bool;
    fn queue_retained_batch(
        &mut self,
        frames: Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
        required: &BTreeSet<OutputId>,
    ) -> Result<BTreeMap<OutputId, crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>>;
    fn queue_ordinary_batch(
        &mut self,
        frames: Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>>;
    fn retained_repaint_deferred(&self) -> bool;
    fn presented_frame(&self, output: OutputId) -> Option<&OutputFrameDamageSnapshot>;
    /// The frame each of this output's heads last retired, one entry per
    /// head, primary first. A mirror head that has retired nothing yet is
    /// None. What a whole output has presented is only what every head has.
    /// Consumed by t245's presented-input publish path; until that joins, only
    /// the lifecycle tests read it.
    #[allow(dead_code)]
    fn presented_head_frames(&self, output: OutputId) -> Vec<Option<&OutputFrameDamageSnapshot>> {
        vec![self.presented_frame(output)]
    }
}

impl NativeCompositionTarget for LiveProductionNativeScanout {
    fn frame_owner(&self) -> crate::NativeFrameOwner {
        self.frame_owner()
    }
    fn frame_service_available(&self) -> bool {
        self.output_topology_allows_frame_service()
    }
    fn head_targets(&self, output: OutputId) -> Vec<HeadRenderTarget> {
        self.head_render_targets(output)
    }
    fn has_in_flight_direct(&self) -> bool {
        self.heads.iter().any(|head| head.submitted_direct)
    }
    fn required_outputs_ready(&self, outputs: &BTreeSet<OutputId>) -> bool {
        self.retained_retirements_ready(outputs)
    }
    fn queue_retained_batch(
        &mut self,
        frames: Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
        required: &BTreeSet<OutputId>,
    ) -> Result<BTreeMap<OutputId, crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>>
    {
        self.queue_retained_output_head_composition_frames_requiring_retirement(frames, required)
    }
    fn queue_ordinary_batch(
        &mut self,
        frames: Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
    ) -> Result<BTreeMap<OutputId, crate::LiveProductionNativeFrameId>, Box<dyn std::error::Error>>
    {
        self.queue_ordinary_head_composition_batch(frames)
    }
    fn retained_repaint_deferred(&self) -> bool {
        LiveProductionNativeScanout::retained_repaint_deferred(self)
    }
    fn presented_frame(&self, output: OutputId) -> Option<&OutputFrameDamageSnapshot> {
        self.presented_output_frame(output)
    }
    fn presented_head_frames(&self, output: OutputId) -> Vec<Option<&OutputFrameDamageSnapshot>> {
        self.presented_output_head_frames(output)
    }
}
