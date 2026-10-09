//! Device boundary for topology rebind and the final stage of native resume.
//! The runtime keeps the transition, scene lowering and lock cover. Targets
//! supply already prepared output owners, custody transfer and first commits.
use super::*;

pub(crate) trait NativeTopologyTarget: NativeCompositionTarget {
    fn adopt_output_runtimes(
        &mut self,
        outputs: &[HeadlessOutput],
        committed: &[CommittedSurfaceState],
    ) -> Result<LiveProductionOutputRuntimeSet, Box<dyn std::error::Error>>;

    fn stop_translation_motion(&mut self);

    fn handoff_topology_custody(
        &mut self,
        previous: &mut LiveProductionOutputRuntimeSet,
        next: &mut LiveProductionOutputRuntimeSet,
    ) -> Result<(), Box<dyn std::error::Error>>;

    fn initialize_output_composition(
        &mut self,
        outputs: &mut LiveProductionOutputRuntimeSet,
        output: OutputId,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

impl NativeTopologyTarget for LiveProductionNativeScanout {
    fn adopt_output_runtimes(
        &mut self,
        outputs: &[HeadlessOutput],
        committed: &[CommittedSurfaceState],
    ) -> Result<LiveProductionOutputRuntimeSet, Box<dyn std::error::Error>> {
        LiveProductionOutputRuntimeSet::adopt_native_topology(outputs, committed, self)
    }

    fn stop_translation_motion(&mut self) {
        self.set_translation_motion_active(false);
    }

    fn handoff_topology_custody(
        &mut self,
        previous: &mut LiveProductionOutputRuntimeSet,
        next: &mut LiveProductionOutputRuntimeSet,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.handoff_installed_topology_custody(previous, next)?;
        Ok(())
    }

    fn initialize_output_composition(
        &mut self,
        outputs: &mut LiveProductionOutputRuntimeSet,
        output: OutputId,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        outputs.initialize_native_head_composition(self, output, frames)?;
        Ok(())
    }
}
