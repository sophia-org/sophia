use super::*;

impl LiveProductionVisualRuntime {
    /// Coalesce compositor changes until the candidate that owns Present has
    /// retired. A repaint can supersede its exact retirement proof even if it
    /// reuses the candidate pixels, stranding surface admission indefinitely.
    pub(in crate::production_visual_runtime) fn queue_retained_projection(
        &mut self,
        scene: &LiveProductionCpuScene,
        native: &mut LiveProductionNativeScanout,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.retained_projection_pending = true;
        if self.native_publication_blocked() || !native.output_topology_allows_frame_service() {
            return Ok(false);
        }
        let required_outputs = self
            .retained_projection_retirements
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        if !native.retained_retirements_ready(&required_outputs) {
            return Ok(false);
        }
        let frames = self.retained_output_head_composition_frames(scene, native)?;
        let queued = native.queue_retained_output_head_composition_frames_requiring_retirement(
            frames,
            &required_outputs,
        )?;
        self.retained_projection_retirements
            .retain(|output, _| !queued.contains_key(output));
        if !self.retained_projection_retirements.is_empty() {
            return Ok(!queued.is_empty());
        }
        self.retained_projection_pending = false;
        Ok(!queued.is_empty())
    }
}
