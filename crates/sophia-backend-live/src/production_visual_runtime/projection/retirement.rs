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
        for (output, native_frame) in &queued {
            if !ready.contains(output) {
                continue;
            }
            let Some(content) = self.shell_content.get(output) else {
                continue;
            };
            let targets = native.head_targets(*output);
            for target in &targets {
                let identity = native.frame_owner().frame(
                    *output,
                    target.head,
                    target.target_generation,
                    native_frame.raw(),
                );
                tracing::info!(
                    target: "sophia_scanout_evidence",
                    "sophia_shell_native_binding schema=1 connection_epoch={} content_grant_epoch={} output={} candidate_generation={} native_owner={} native_frame={} head={} target_generation={} heads={}",
                    content.grant.connection_epoch,
                    content.grant.content_grant_epoch,
                    output.raw(),
                    content.candidate_generation,
                    identity.owner(),
                    identity.frame(),
                    target.head.raw(),
                    target.target_generation,
                    targets.len(),
                );
            }
        }
        self.retained_projection_retirements
            .retain(|output, _| !queued.contains_key(output));
        if !self.retained_projection_retirements.is_empty() {
            return Ok(!queued.is_empty());
        }
        self.retained_projection_pending = native.retained_repaint_deferred();
        Ok(!queued.is_empty())
    }
}
