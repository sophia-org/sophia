use super::*;

impl LiveProductionVisualRuntime {
    /// Installs one complete shell content candidate and queues a retained
    /// repaint. The prior candidate remains installed if native queueing fails.
    pub fn set_shell_content(
        &mut self,
        frame: LiveShellContentFrame,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        self.set_shell_content_on_target(frame, scene, native_scanout)
    }

    pub(in crate::production_visual_runtime) fn set_shell_content_on_target<
        T: NativeCompositionTarget,
    >(
        &mut self,
        frame: LiveShellContentFrame,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut T>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if !frame.output.is_valid()
            || frame.candidate_generation == 0
            || frame.images.is_empty()
            || frame.images.iter().any(|image| {
                !matches!(
                    image.node,
                    CompositorNodeId::ShellContent {
                        output,
                        candidate,
                        ..
                    } if output == frame.output && candidate == frame.candidate_generation
                )
            })
        {
            return Err("shell content frame is malformed".into());
        }
        if self.shell_content.get(&frame.output) == Some(&frame) {
            return Ok(false);
        }
        let native_scanout = native_scanout
            .ok_or("shell content candidate cannot be accepted without native presentation")?;
        if self.native_suspended || !native_scanout.frame_service_available() {
            return Err(
                "shell content candidate cannot be accepted while native presentation is quiesced"
                    .into(),
            );
        }
        let output = frame.output;
        let grant = frame.grant;
        if self
            .retained_projection_retirements
            .get(&output)
            .is_some_and(|owner| *owner != grant)
        {
            return Err(
                "shell content candidate found an unreconciled retirement from another grant"
                    .into(),
            );
        }
        let previous = self.shell_content.insert(output, frame);
        let previous_retirement = self.retained_projection_retirements.insert(output, grant);
        if let Err(error) = self.queue_retained_projection(scene, native_scanout) {
            match previous_retirement {
                Some(previous) => {
                    self.retained_projection_retirements
                        .insert(output, previous);
                }
                None => {
                    self.retained_projection_retirements.remove(&output);
                }
            }
            match previous {
                Some(previous) => {
                    self.shell_content.insert(output, previous);
                }
                None => {
                    self.shell_content.remove(&output);
                }
            }
            return Err(error);
        }
        Ok(true)
    }

    /// Retires the physical-presentation claims owned by a revoked shell
    /// connection. The caller must revoke that connection first: clearing a
    /// live claim without closing its protocol obligation would strand an
    /// accepted candidate without either Presented or Rejected.
    pub fn revoke_shell_content_retirement_claims(
        &mut self,
        grant: sophia_protocol::ContentGrant,
    ) -> usize {
        let before = self.retained_projection_retirements.len();
        self.retained_projection_retirements
            .retain(|_, owner| *owner != grant);
        before.saturating_sub(self.retained_projection_retirements.len())
    }

    /// Drops content for outputs that no longer exist after a quiescent
    /// topology replacement. Presentation claims must already have been
    /// revoked with their shell connection.
    pub(in crate::production_visual_runtime) fn retain_shell_content_outputs(
        &mut self,
        outputs: &BTreeSet<OutputId>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if self
            .retained_projection_retirements
            .keys()
            .any(|output| !outputs.contains(output))
        {
            return Err(
                "native topology replacement would orphan a shell content retirement claim".into(),
            );
        }
        let before = self.shell_content.len();
        self.shell_content
            .retain(|output, _| outputs.contains(output));
        Ok(before.saturating_sub(self.shell_content.len()))
    }

    pub fn shell_content_presentation_epoch(
        &self,
        output: OutputId,
        candidate_generation: u64,
    ) -> Option<u64> {
        let frame = self.shell_content.get(&output)?;
        if frame.candidate_generation != candidate_generation {
            return None;
        }
        let projection = self
            .input_projections
            .iter()
            .find(|projection| projection.output == output)?;
        let presented = self.tab_frames.get(&output).is_some_and(|display_list| {
            crate::production_visual_runtime::projection::presented_content_list_matches(
                display_list,
                frame,
            )
        });
        presented.then_some(projection.epoch.max(1))
    }
}
