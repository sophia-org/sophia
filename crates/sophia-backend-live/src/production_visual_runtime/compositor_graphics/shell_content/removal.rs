use super::*;

impl LiveProductionVisualRuntime {
    /// Disarm this exact candidate immediately. Once it has presented, enqueue
    /// a fresh composition without it. None means the candidate still owes its
    /// first presentation; the caller must retain and retry the close request.
    /// Errors retain the source owner but do not restore interaction authority.
    #[allow(clippy::too_many_arguments)]
    pub fn remove_shell_component_content(
        &mut self,
        output: OutputId,
        layer: LiveShellContentLayer,
        grant: sophia_protocol::ContentGrant,
        candidate: u64,
        scene: &LiveProductionCpuScene,
        native: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<Option<LiveShellContentRemoval>, Box<dyn std::error::Error>> {
        self.remove_shell_component_content_on_target(
            output, layer, grant, candidate, scene, native,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::production_visual_runtime) fn remove_shell_component_content_on_target<
        T: NativeCompositionTarget,
    >(
        &mut self,
        output: OutputId,
        layer: LiveShellContentLayer,
        grant: sophia_protocol::ContentGrant,
        candidate: u64,
        scene: &LiveProductionCpuScene,
        native: Option<&mut T>,
    ) -> Result<Option<LiveShellContentRemoval>, Box<dyn std::error::Error>> {
        let key = (output, layer);
        let owned = self
            .shell_content
            .get_mut(&key)
            .ok_or("component removal has no admitted owner")?;
        if owned.frame.grant != grant || owned.frame.candidate_generation != candidate {
            return Err("component removal does not name the admitted candidate".into());
        }
        owned.interaction_revoked = true;
        for projection in &mut self.input_projections {
            if projection.output == output {
                for binding in &mut projection.content {
                    if binding.grant == grant {
                        binding.authority_current = false;
                    }
                }
            }
        }
        let Some(epoch) = self.shell_content_presentation_epoch(output, grant, candidate) else {
            return Ok(None);
        };
        if self.retained_projection_retirements.contains_key(&key) {
            return Ok(None);
        }
        let native = native.ok_or("component removal requires native presentation")?;
        if self.native_suspended || !native.frame_service_available() {
            return Err("component removal cannot queue while presentation is unavailable".into());
        }
        let owned = self
            .shell_content
            .remove(&key)
            .expect("validated exact owner");
        self.retained_projection_retirements.insert(key, grant);
        if let Err(error) = self.queue_retained_projection(scene, native) {
            self.retained_projection_retirements.remove(&key);
            self.shell_content.insert(key, owned);
            return Err(error);
        }
        Ok(Some(LiveShellContentRemoval {
            output,
            grant,
            candidate,
            prior_presentation_epoch: epoch,
        }))
    }

    /// Requires an observed newer display without this grant, not merely its
    /// absence from admission. This does not imply that pixel consumers retired.
    pub fn shell_component_removal_presented(&self, removal: LiveShellContentRemoval) -> bool {
        if self.shell_content.iter().any(|((output, _), owned)| {
            *output == removal.output && owned.frame.grant == removal.grant
        }) || self
            .retained_projection_retirements
            .iter()
            .any(|((output, _), grant)| *output == removal.output && *grant == removal.grant)
        {
            return false;
        }
        let Some(projection) = self
            .input_projections
            .iter()
            .find(|p| p.output == removal.output)
        else {
            return false;
        };
        if projection.epoch <= removal.prior_presentation_epoch
            || projection.content.iter().any(|b| b.grant == removal.grant)
        {
            return false;
        }
        self.tab_frames.get(&removal.output).is_some_and(|list| {
            !list.content_images().any(|image| {
                matches!(image.node,
                CompositorNodeId::ShellContent { grant, output, .. }
                if grant == removal.grant && output == removal.output)
            })
        })
    }
}
