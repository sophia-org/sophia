//! Device boundary for the CPU authority cycle. Native selection, framing and
//! repaint admission remain in the shared cycle body.
use super::*;

pub(crate) trait NativeCycleTarget: NativeRetirementTarget {
    fn set_translation_motion_active(&mut self, active: bool);
    fn initialize_head_composition(
        &mut self,
        output: OutputId,
        runtime: &mut LiveBackendRuntimeAssembly,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>>;
    fn run_tick(
        &mut self,
        output: OutputId,
        runtime: &mut LiveBackendRuntimeAssembly,
        input: CompositorBackendTickInput,
    ) -> Result<LiveBackendRuntimeTickReport, Box<dyn std::error::Error>>;
}

impl NativeCycleTarget for LiveProductionNativeScanout {
    fn set_translation_motion_active(&mut self, active: bool) {
        LiveProductionNativeScanout::set_translation_motion_active(self, active);
    }

    fn initialize_head_composition(
        &mut self,
        output: OutputId,
        runtime: &mut LiveBackendRuntimeAssembly,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
        LiveProductionNativeScanout::initialize_head_composition(self, output, runtime, frames)
    }

    fn run_tick(
        &mut self,
        output: OutputId,
        runtime: &mut LiveBackendRuntimeAssembly,
        input: CompositorBackendTickInput,
    ) -> Result<LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        LiveProductionNativeScanout::run_tick(self, output, runtime, input)
    }
}
