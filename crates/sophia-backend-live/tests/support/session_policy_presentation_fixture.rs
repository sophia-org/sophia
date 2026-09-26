//! Session's WM receipt join uses the same mirrored queue, composition,
//! submission custody and retirement owners as the backend controls. Only the
//! worker/device completion is simulated. No receipt or input stamp is supplied.
use super::*;
use mirrored_composition_test_target::MirroredTarget;

#[doc(hidden)]
pub struct SessionPolicyPresentationFixture {
    target: MirroredTarget,
    outputs: Vec<OutputId>,
}

impl SessionPolicyPresentationFixture {
    pub fn new(outputs: &[HeadlessOutput]) -> Self {
        Self {
            target: MirroredTarget::new(outputs),
            outputs: outputs.iter().map(|output| output.id).collect(),
        }
    }

    pub fn heads(&self) -> Vec<HeadRenderTarget> {
        self.outputs
            .iter()
            .flat_map(|output| self.target.head_targets(*output))
            .collect()
    }

    /// Capture the installed publication and committed source bytes before
    /// any simulated completion. Changing requested state cannot relabel it.
    pub fn queue(
        &mut self,
        runtime: &LiveProductionVisualRuntime,
        scene: &LiveProductionCpuScene,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let frames = runtime.retained_output_head_composition_frames(scene, &self.target)?;
        self.target.queue_retained_batch(frames, &BTreeSet::new())?;
        Ok(())
    }

    pub fn simulate_submit(&mut self, output: OutputId) {
        self.target.install(output).unwrap();
        self.target.prepare(output);
    }

    /// A page-flip input may retire only this target's submitted head. The
    /// runtime derives input publication from the actual retired frame lists.
    pub fn simulate_head_completion(
        &mut self,
        runtime: &mut LiveProductionVisualRuntime,
        output: OutputId,
        head_index: usize,
    ) {
        self.target.flip(output, head_index);
        runtime.publish_presented_input_layers(&self.target);
    }

    pub fn publish(&self, runtime: &mut LiveProductionVisualRuntime) {
        runtime.publish_presented_input_layers(&self.target);
    }
}

impl Drop for SessionPolicyPresentationFixture {
    fn drop(&mut self) {
        // Fixture cleanup releases fake framebuffer owners; it is not evidence
        // of physical scanout termination or driver resource reclamation.
        self.target.teardown();
    }
}
