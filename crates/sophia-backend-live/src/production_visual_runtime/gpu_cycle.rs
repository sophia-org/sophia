impl LiveProductionVisualRuntime {
    pub fn run_gpu_production_cycle(
        &mut self,
        request: LiveProductionCycleRequest<'_>,
    ) -> Result<
        (
            LiveProductionCpuSubmission,
            Vec<CommittedSurfaceState>,
            LiveProductionCpuProgress,
        ),
        Box<dyn std::error::Error>,
    > {
        let LiveProductionCycleRequest {
            batch,
            scene,
            raised_surface,
            focused_surface,
            cursor_presentation,
            defer_frame,
            output_descriptors,
            native_scanout,
            wm_update,
            presentation_layout,
            geometry_routed_surfaces,
            chrome_surfaces,
            indicator_publication,
            staged_cpu_buffer_handles,
        } = request;
        let native_enabled = native_scanout.is_some();
        let batch = self.ready_surface_content_batch(batch)?;
        self.last_primary_logical_target = None;
        let mut cpu_progress = authority_batch_cpu_progress(&batch);
        let mut updates = authority_batch_cpu_buffer_updates(&batch);
        record_recent_cpu_buffer_updates(&mut self.recent_cpu_buffer_updates, &updates);
        write_cpu_buffer_residency(
            &mut self.cpu_buffer_residency,
            self.production.committed_surfaces(),
            &batch,
            self.surface_content_stream
                .deferred_items()
                .chain(self.released_surface_content.iter()),
            self.present_scheduler.retained_cpu_buffer_handles(),
            staged_cpu_buffer_handles,
            &self.recent_cpu_buffer_updates,
        );
        retain_relevant_cpu_buffer_updates(scene, &mut updates, &self.cpu_buffer_residency);
        self.focused_surface = self.chrome_focus(focused_surface, chrome_surfaces);
        let _ = self.apply_presentation_layout(presentation_layout, geometry_routed_surfaces);
        self.set_chrome_surfaces(chrome_surfaces);
        self.set_indicator_publication(indicator_publication);
        let committed_surfaces = self.committed_surfaces().to_vec();
        scene.apply_production_updates(updates)?;
        scene.reconcile_buffer_residency(&self.cpu_buffer_residency);
        let missing_buffers = scene.missing_committed_buffer_count(&committed_surfaces);
        if missing_buffers != 0 {
            return Err(format!(
                "production GPU scene is missing {missing_buffers} committed CPU buffer(s)"
            )
            .into());
        }
        let compose_started = Instant::now();
        let mut composition = if defer_frame {
            scene
                .last_report()
                .cloned()
                .ok_or("software redraw coalescing has no prior composed frame")?
        } else {
            let presentation_order =
                raised_presentation_order(&self.presentation_order, raised_surface);
            let display_list = self.display_list(&committed_surfaces, &presentation_order)?;
            let output = output_descriptors
                .first()
                .copied()
                .ok_or("software composition has no output descriptor")?;
            scene
                .compose_display_list(
                    output,
                    &committed_surfaces,
                    &display_list,
                    cursor_presentation.composition_position(),
                )?
                .clone()
        };
        // A Present-bearing authority group owns the next native visual
        // candidate. Do not queue a retained CPU frame ahead of it: that can
        // expose new layout/chrome around old or absent client pixels.
        let native_frames = if defer_frame || batch.has_dma_buf_present_submissions() {
            None
        } else {
            native_scanout
                .as_ref()
                .map(|_| scene.frames_for_outputs(output_descriptors))
                .transpose()?
        };
        let cpu_layers =
            scene.presentation_variant_layers(&committed_surfaces, &self.presentation_order);
        let tick = self.run_batch(
            &batch,
            presentation_layout,
            if defer_frame { None } else { native_scanout },
            native_frames,
            scene,
            cpu_layers,
            wm_update,
        )?;
        let software_present_frame_required = !self.software_presents_unframed.is_empty();
        cpu_progress.bind_primary_logical_target(self.last_primary_logical_target);
        if software_present_frame_required {
            let committed_surfaces = self.committed_surfaces().to_vec();
            let presentation_order =
                raised_presentation_order(&self.presentation_order, raised_surface);
            let display_list = self.display_list(&committed_surfaces, &presentation_order)?;
            let output = output_descriptors
                .first()
                .copied()
                .ok_or("software Present has no output descriptor")?;
            composition = scene
                .compose_display_list(
                    output,
                    &committed_surfaces,
                    &display_list,
                    cursor_presentation.composition_position(),
                )?
                .clone();
            if native_enabled {
                self.frame_unframed_software_presents(scene, output_descriptors)?;
            } else if self.native_suspended {
                self.reject_software_presents();
            } else {
                self.settle_unframed_software_presents_without_native()?;
            }
        }
        Ok((
            LiveProductionCpuSubmission {
                tick,
                composition,
                composed: !defer_frame || software_present_frame_required,
                compose_elapsed: if defer_frame && !software_present_frame_required {
                    Duration::ZERO
                } else {
                    compose_started.elapsed()
                },
                primary_logical_target: cpu_progress.primary_logical_target,
            },
            committed_surfaces,
            cpu_progress,
        ))
    }
}
