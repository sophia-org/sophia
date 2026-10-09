use super::*;

impl LiveProductionVisualRuntime {
    /// Lowers one immutable committed scene into candidate native-size frames
    /// for a provisional topology. CPU buffers and retained renderer images
    /// come from the ordinary authority-owned source set; committed DMA-BUF
    /// identities are not independently importable sources. This is read-only
    /// with respect to the live runtime: the caller must not publish or install
    /// the candidate until its KMS transaction and first-presentation barrier
    /// complete.
    pub fn compose_output_topology_head_frames(
        &self,
        scene: &LiveProductionCpuScene,
        resolved: &crate::LiveResolvedOutputTopology,
        scene_generation: u64,
    ) -> Result<Vec<crate::LiveProductionHeadCompositionFrame>, Box<dyn std::error::Error>> {
        if scene_generation == 0 {
            return Err("topology composition requires a valid scene generation".into());
        }
        // No submission accompanies a provisional topology, so nothing here can
        // be an in-flight direct frame; retained direct frames still resolve.
        let source_set = self.retained_composition_source_set(scene, None)?;
        let targets = resolved.head_render_targets();
        if targets.len() != resolved.targets.len() {
            return Err("topology render-target projection is incomplete".into());
        }
        let mut frames = Vec::with_capacity(targets.len());
        for viewport in &resolved.logical_viewports {
            // Topology first-frame accounting owns this generation. A WM
            // preview cannot replace it with an ordinary recovery generation.
            let display_list = self
                .display_list_without_policy(
                    viewport.output,
                    viewport.logical,
                    &source_set.committed,
                    &source_set.presentation_order,
                    None,
                )
                .map_err(|_| "topology composition display list invalid")?;
            let snapshot = sophia_engine::output_scene_snapshot_from_committed_in_view(
                viewport.output,
                scene_generation,
                viewport.logical,
                &source_set.committed,
                display_list,
                None,
            )?;
            let output_targets = targets
                .iter()
                .copied()
                .filter(|target| target.output == viewport.output)
                .collect::<Vec<_>>();
            let plans = sophia_engine::build_output_head_plans(&snapshot, &output_targets)?;
            for plan in &plans {
                let mut frame = sophia_renderer_live::lower_head_composition_plan_with_caches(
                    plan,
                    &source_set.sources,
                    &mut self.indicator_strip_cache.borrow_mut(),
                    &mut self.text_cache.borrow_mut(),
                )?;
                self.attach_frame_damage_history(
                    &mut frame,
                    plan,
                    &source_set.committed,
                    self.present_scheduler.in_flight_prepared(),
                )?;
                frames.push(crate::LiveProductionHeadCompositionFrame {
                    head: plan.head,
                    scene_generation: plan.scene_generation,
                    target_generation: plan.target_generation,
                    mapping: plan.mapping,
                    logical_content_checksum: plan.logical_content_checksum,
                    frame,
                });
            }
        }
        if frames.len() != targets.len() {
            return Err("topology composition omitted an enabled head".into());
        }
        let actual = frames
            .iter()
            .map(|frame| frame.head)
            .collect::<BTreeSet<_>>();
        let expected = targets
            .iter()
            .map(|target| target.head)
            .collect::<BTreeSet<_>>();
        if actual != expected || actual.len() != frames.len() {
            return Err("topology composition repeated or targeted an unknown head".into());
        }
        Ok(frames)
    }
}
