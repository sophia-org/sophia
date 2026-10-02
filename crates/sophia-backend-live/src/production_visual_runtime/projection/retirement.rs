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
        for (output, _) in self.outputs.logical_viewports().collect::<Vec<_>>() {
            if let Some(frame) = native.presented_frame_id(output) {
                self.settle_shell_retirement_claims(output, frame);
            }
        }
        self.retained_projection_pending = true;
        if self.native_publication_blocked() || !native.frame_service_available() {
            return Ok(false);
        }
        let required_outputs = self
            .retained_projection_retirements
            .keys()
            .map(|(output, _)| *output)
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
            .filter(|output| {
                !self
                    .queued_shell_retirements
                    .keys()
                    .any(|(owned, _)| owned == output)
                    && native.required_outputs_ready(&BTreeSet::from([*output]))
            })
            .collect::<BTreeSet<_>>();
        let frames = self
            .retained_output_head_composition_frames(scene, native)?
            .into_iter()
            .filter(|(output, _)| {
                !native.output_recovering(*output)
                    && (!required_outputs.contains(output) || ready.contains(output))
            })
            .collect::<Vec<_>>();
        let mut queued = BTreeMap::new();
        let mut deferred = false;
        for (output, frames) in frames {
            let required = ready.iter().copied().filter(|id| *id == output).collect();
            match native.queue_retained_batch(vec![(output, frames)], &required) {
                Ok(admitted) => queued.extend(admitted),
                Err(error) => {
                    let had_publication = self.policy_presentation.is_some();
                    if !self.handle_preview_refusal(error.as_ref()) {
                        return Err(error);
                    }
                    deferred = true;
                    if had_publication && self.policy_presentation.is_none() {
                        // Remaining lowered frames still describe that tier.
                        // Rebuild them without it on the next retained pass.
                        break;
                    }
                }
            }
        }
        for (output, native_frame) in &queued {
            if !ready.contains(output) {
                continue;
            }
            for (_, content) in self
                .shell_content
                .iter()
                .filter(|((id, _), _)| id == output)
            {
                let content = &content.frame;
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
                        "sophia_shell_native_binding schema=1 connection_epoch={} content_grant_epoch={} output={} candidate_generation={} native_owner={} native_frame={} head={} target_generation={} heads={} mode_refresh_millihz={}",
                        content.grant.connection_epoch,
                        content.grant.content_grant_epoch,
                        output.raw(),
                        content.candidate_generation,
                        identity.owner(),
                        identity.frame(),
                        target.head.raw(),
                        target.target_generation,
                        targets.len(),
                        target.refresh_millihz,
                    );
                }
            }
        }
        self.bind_shell_retirement_claims(&queued);
        if !self.retained_projection_retirements.is_empty() {
            return Ok(!queued.is_empty());
        }
        self.retained_projection_pending = deferred || native.retained_repaint_deferred();
        Ok(!queued.is_empty())
    }
}

impl LiveProductionVisualRuntime {
    pub(in crate::production_visual_runtime) fn bind_shell_retirement_claims(
        &mut self,
        queued: &BTreeMap<OutputId, LiveProductionNativeFrameId>,
    ) {
        let claims = std::mem::take(&mut self.retained_projection_retirements);
        for (key, grant) in claims {
            if let Some(frame) = queued.get(&key.0) {
                self.queued_shell_retirements
                    .entry((key.0, *frame))
                    .or_default()
                    .insert(key, grant);
            } else {
                self.retained_projection_retirements.insert(key, grant);
            }
        }
    }

    pub(in crate::production_visual_runtime) fn settle_shell_retirement_claims(
        &mut self,
        output: OutputId,
        frame: LiveProductionNativeFrameId,
    ) {
        self.queued_shell_retirements.remove(&(output, frame));
    }

    pub(in crate::production_visual_runtime) fn rearm_all_shell_retirement_claims(
        &mut self,
    ) -> Result<(), &'static str> {
        let claims = self
            .queued_shell_retirements
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let mut error = None;
        for (output, frame) in claims {
            if let Err(detail) = self.rearm_shell_retirement_claims(output, frame) {
                error.get_or_insert(detail);
            }
        }
        error.map_or(Ok(()), Err)
    }

    pub(in crate::production_visual_runtime) fn rearm_shell_retirement_claims(
        &mut self,
        output: OutputId,
        frame: LiveProductionNativeFrameId,
    ) -> Result<(), &'static str> {
        let Some(claims) = self.queued_shell_retirements.get(&(output, frame)) else {
            return Ok(());
        };
        if claims.iter().any(|(key, grant)| {
            self.retained_projection_retirements
                .get(key)
                .is_some_and(|current| current != grant)
        }) {
            return Err("preview recovery found a conflicting shell retirement claim");
        }
        let claims = self
            .queued_shell_retirements
            .remove(&(output, frame))
            .expect("checked claims");
        self.retained_projection_retirements.extend(claims);
        self.retained_projection_pending = true;
        Ok(())
    }
}
