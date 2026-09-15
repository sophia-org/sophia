use super::*;

impl LiveProductionVisualRuntime {
    /// Coalesce compositor changes until the candidate that owns Present has
    /// retired. A repaint can supersede its exact retirement proof even if it
    /// reuses the candidate pixels, stranding surface admission indefinitely.
    pub(in crate::production_visual_runtime) fn queue_retained_projection<
        T: NativeCompositionTarget,
    >(
        &mut self,
        scene: &LiveProductionCpuScene,
        native: &mut T,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.retained_projection_pending = true;
        if self.native_publication_blocked() || !native.frame_service_available() {
            return Ok(false);
        }
        let required_outputs = self
            .retained_projection_retirements
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        if required_outputs
            .iter()
            .any(|output| native.head_targets(*output).is_empty())
        {
            return Err("required retained retirement targets an absent output".into());
        }
        // Shell candidates own independent output retirements. A busy output
        // keeps its exact claim without preventing ready outputs from service.
        // Cross-output client Present cohorts remain blocked above and use
        // their separate whole-cohort admission path.
        let ready = required_outputs
            .iter()
            .copied()
            .filter(|output| native.required_outputs_ready(&BTreeSet::from([*output])))
            .collect::<BTreeSet<_>>();
        let frames = self
            .retained_output_head_composition_frames(scene, native)?
            .into_iter()
            .filter(|(output, _)| !required_outputs.contains(output) || ready.contains(output))
            .collect();
        let queued = native.queue_retained_batch(frames, &ready)?;
        self.retained_projection_retirements
            .retain(|output, _| !queued.contains_key(output));
        if !self.retained_projection_retirements.is_empty() {
            return Ok(!queued.is_empty());
        }
        self.retained_projection_pending = native.retained_repaint_deferred();
        Ok(!queued.is_empty())
    }
}
