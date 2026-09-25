//! Test-only bridge for Session's actual disconnect/revocation join. Admission,
//! owned queues, copied backing custody and input publication use production
//! code. Explicit completion simulates the worker, device and page-flip edge.
use super::*;
use composition_test_target::Target;

#[doc(hidden)]
pub struct SessionContentFixture {
    runtime: LiveProductionVisualRuntime,
    scene: LiveProductionCpuScene,
    target: Target,
}

impl SessionContentFixture {
    pub fn new(outputs: &[HeadlessOutput]) -> Result<Self, Box<dyn std::error::Error>> {
        let first = outputs.first().ok_or("fixture requires outputs")?;
        Ok(Self {
            runtime: LiveProductionVisualRuntime::new(outputs, None)?,
            scene: LiveProductionCpuScene::new(first.size),
            target: Target::new(outputs),
        })
    }

    pub fn runtime(&self) -> &LiveProductionVisualRuntime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut LiveProductionVisualRuntime {
        &mut self.runtime
    }

    pub fn admit(
        &mut self,
        frame: LiveShellContentFrame,
        layer: LiveShellContentLayer,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.runtime.set_shell_component_content_on_target(
            frame,
            layer,
            &self.scene,
            Some(&mut self.target),
        )
    }

    /// Copy real queued sources into separate backing bytes, without a device.
    pub fn simulate_submit(&mut self, output: OutputId) {
        self.target.begin_render(output);
        self.target.finish_render(output);
    }

    /// Only an actually submitted fixture frame may acquire a completion.
    pub fn simulate_completion(&mut self, output: OutputId) {
        assert!(self.target.flip(output, None));
        self.runtime.publish_presented_input_layers(&self.target);
    }

    pub fn retry(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        self.runtime
            .queue_retained_projection(&self.scene, &mut self.target)
    }

    pub fn claims(&self) -> Vec<(OutputId, LiveShellContentLayer, ContentGrant)> {
        self.runtime
            .retained_projection_retirements
            .iter()
            .map(|((output, layer), grant)| (*output, *layer, *grant))
            .collect()
    }

    pub fn queued(&self, output: OutputId) -> bool {
        self.target.queue.pending(output)
    }

    pub fn copied_backings(&self) -> usize {
        self.target.backing_owners.get()
    }

    /// Fixture cleanup is not evidence of native disposition or GPU reclamation.
    pub fn teardown(&mut self) {
        self.target.teardown();
    }
}
