//! Device completion is supplied; the tracker derives retirement from the
//! submitted mirror generation after the production custody owner accepts it.
use super::*;

impl MirroredTarget {
    pub(in crate::production_visual_runtime) fn track_retirements(&mut self) {
        let mut registry = EngineHeadRegistry::new();
        for head in &self.heads {
            assert!(registry.admit(head.target).is_admitted());
        }
        self.retirements = Some(LiveProductionPageFlipTracker::from_outputs(&registry));
    }
}

impl NativeRetirementTarget for MirroredTarget {
    fn submitted_frame(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
        self.groups.get(&output)?.logically_submitted_frame()
    }

    fn retire_ready(
        &mut self,
        output: OutputId,
        _: &mut LiveBackendRuntimeAssembly,
        _: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        while let Some(index) = self
            .completion_inputs
            .iter()
            .position(|(id, _)| *id == output)
        {
            let (_, head) = self.completion_inputs.remove(index).unwrap();
            self.flip(output, head);
        }
        Ok(())
    }

    fn take_presentation_feedback(
        &mut self,
        output: OutputId,
    ) -> Option<LiveProductionNativeFrameRetirement> {
        self.retirements
            .as_mut()
            .unwrap()
            .take_frame_retirement(output)
    }

    fn promote_renderer_image(
        &mut self,
        _: LiveRendererImageId,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail> {
        panic!("CPU-only retirement fixture cannot promote GPU images")
    }

    fn promote_renderer_image_for_previews(
        &mut self,
        _: LiveRendererImageId,
        _: Option<SurfaceId>,
        _: &BTreeSet<OutputId>,
        _: &BTreeSet<OutputId>,
    ) -> Result<(usize, Result<(), LivePreviewImageRefusal>), LiveRendererScanoutBufferExportDetail>
    {
        panic!("CPU-only retirement fixture cannot prepare GPU previews")
    }

    fn evict_renderer_image(
        &mut self,
        _: LiveRendererImageId,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail> {
        panic!("CPU-only retirement fixture cannot evict GPU images")
    }
}

impl NativeCycleTarget for MirroredTarget {
    fn set_translation_motion_active(&mut self, _: bool) {}

    fn initialize_head_composition(
        &mut self,
        _: OutputId,
        _: &mut LiveBackendRuntimeAssembly,
        _: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
        Err("fixture requires explicit simulated initialization".into())
    }

    fn run_tick(
        &mut self,
        output: OutputId,
        runtime: &mut LiveBackendRuntimeAssembly,
        input: CompositorBackendTickInput,
    ) -> Result<LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        if self.queue.pending(output) && self.ready(output) {
            self.install(output)?;
            self.prepare(output);
        }
        Ok(runtime.run_tick(input)?)
    }
}
