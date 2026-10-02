use super::*;

impl LiveProductionVisualRuntime {
    /// One retirement-only turn before rollback may replace KMS and its
    /// completion trackers. The candidate's displayed owner stays retained;
    /// only accepted flips can retire a predecessor. No first-presentation
    /// acceptance, scene service or new submission runs through this seam.
    pub fn service_output_topology_rollback_quiescence(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        native_scanout.drain_output_topology_rollback_frames()?;
        native_scanout.pump_native_completions()?;
        self.retire_native_scanout_for_drain(native_scanout)?;
        if self.native_scanout_in_flight() || !native_scanout.output_topology_rollback_quiescent() {
            return Ok(false);
        }
        // Physical submissions have settled. Queued or unframed client
        // presents now lose their candidate layout through the existing skip
        // path, without releasing the buffer still displayed by the CRTC.
        self.settle_detached_preview_failures(native_scanout.take_detached_preview_failures())?;
        // Native retirement above has consumed every presented claim. All
        // remaining frame bindings were discarded by the rollback drain.
        self.rearm_all_shell_retirement_claims()?;
        self.skip_presentations_for_topology(Some(native_scanout));
        Ok(self.topology_rebind_quiescent())
    }
}
