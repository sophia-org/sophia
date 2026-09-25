impl LiveProductionVisualRuntime {
    pub fn run_batch(
        &mut self,
        batch: &LiveProductionAuthorityBatch,
        presentation_layout: &[LayerSnapshot],
        mut native_scanout: Option<&mut LiveProductionNativeScanout>,
        native_frames: Option<Vec<LiveProductionComposedFrame>>,
        scene: &LiveProductionCpuScene,
        cpu_layers: Vec<LiveCpuPresentationLayer>,
        wm_update: Option<WmTransactionUpdate>,
    ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        batch.validate()?;
        self.presentation_feedback
            .observe_authority_resource_registrations(batch)?;
        let _ = self.reject_superseded_surface_content()?;
        let removed_surfaces = authority_batch_removed_surfaces(batch);
        self.release_removed_presentations(&removed_surfaces, native_scanout.as_deref_mut())?;
        self.displayed_surfaces
            .retain(|surface, _| !removed_surfaces.contains(surface));
        let mut authority_groups = Vec::new();
        let mut has_present_submissions = false;
        for group in &batch.groups {
            if group.present_submissions.is_empty() {
                authority_groups.push(group.clone());
            } else {
                has_present_submissions = true;
                let superseded = self.present_scheduler.enqueue_group(
                    group,
                    presentation_layout,
                    self.presentation_feedback.resources_mut(),
                    Instant::now(),
                )?;
                for transaction in superseded {
                    self.reject_gpu_presentation(transaction);
                }
            }
        }
        // Software and DMA-BUF Presents can arrive in separate authority
        // groups in one owner batch. Queue the software feedback before the
        // GPU group drives the shared native frame so both retire on its
        // page-flip clock.
        self.enqueue_software_presents(&authority_groups)?;
        self.observe_content_ordered_resource_releases(batch);
        if has_present_submissions && !authority_groups.is_empty() {
            let prepared = self.prepare_authority_groups(&authority_groups)?;
            let _ = self.run_prepared_authority_transactions(
                prepared,
                authority_transaction_count_for_groups(&authority_groups),
                None,
                None,
                wm_update.clone(),
            )?;
        }
        if has_present_submissions {
            for group in &batch.groups {
                self.observe_surface_metadata(&group.transactions, &group.removed_surfaces);
            }
            if !self.present_scheduler.has_eligible() {
                return self.run_observation_tick();
            }
            return self.drive_gpu_presentation(scene, native_scanout.as_deref_mut());
        }
        if authority_groups.is_empty() {
            return self.run_observation_tick();
        }
        let prepared = self.prepare_authority_groups(&authority_groups)?;
        let scene_generation = self
            .production
            .committed_surfaces()
            .iter()
            .map(|state| state.committed_generation)
            .max()
            .unwrap_or(1)
            .max(1);
        let native_head_frames = if native_frames.is_some() {
            native_scanout
                .as_deref()
                .map(|native| {
                    self.cpu_output_head_composition_frames_from_layers(
                        native,
                        &cpu_layers,
                        scene_generation,
                    )
                })
                .transpose()?
        } else {
            None
        };
        let run = self.run_prepared_authority_transactions_with_targets(
            prepared,
            authority_transaction_count_for_groups(&authority_groups),
            native_scanout,
            native_head_frames,
            wm_update,
        )?;
        self.last_primary_logical_target = run.primary_logical_target;
        Ok(run.report)
    }
}
