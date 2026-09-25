impl LiveProductionVisualRuntime {
    /// Publishes an ordinary cadence repaint when native ownership permits it.
    /// `None` preserves the caller's repaint obligation for a later cadence;
    /// forced startup and topology repaints use `run_cpu_repaint` directly.
    pub fn run_ordinary_cpu_repaint(
        &mut self,
        scene: &mut LiveProductionCpuScene,
        raised_surface: Option<SurfaceId>,
        focused_surface: Option<SurfaceId>,
        cursor_presentation: LiveProductionCursorPresentation,
        output_descriptors: &[sophia_engine::HeadlessOutput],
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<Option<LiveProductionCpuSubmission>, Box<dyn std::error::Error>> {
        if self.native_publication_blocked() {
            return Ok(None);
        }
        Ok(Some(self.run_cpu_repaint_inner(
            scene,
            raised_surface,
            focused_surface,
            cursor_presentation,
            output_descriptors,
            native_scanout,
        )?))
    }

    pub fn run_cpu_repaint(
        &mut self,
        scene: &mut LiveProductionCpuScene,
        raised_surface: Option<SurfaceId>,
        focused_surface: Option<SurfaceId>,
        cursor_presentation: LiveProductionCursorPresentation,
        output_descriptors: &[sophia_engine::HeadlessOutput],
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<LiveProductionCpuSubmission, Box<dyn std::error::Error>> {
        // Forced startup/topology work cannot be silently deferred. Validate
        // every output before the ordinary helper can transfer any owner.
        if native_scanout
            .outputs()
            .iter()
            .any(|output| native_scanout.output_retirement_protected(output.id))
        {
            return Err("forced repaint waits for an existing distinct retirement".into());
        }
        self.run_cpu_repaint_inner(
            scene,
            raised_surface,
            focused_surface,
            cursor_presentation,
            output_descriptors,
            native_scanout,
        )
    }

    fn run_cpu_repaint_inner(
        &mut self,
        scene: &mut LiveProductionCpuScene,
        raised_surface: Option<SurfaceId>,
        focused_surface: Option<SurfaceId>,
        cursor_presentation: LiveProductionCursorPresentation,
        output_descriptors: &[sophia_engine::HeadlessOutput],
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<LiveProductionCpuSubmission, Box<dyn std::error::Error>> {
        // The same view the retained head frames below are planned from, so a
        // repaint's software list, its chrome observation and its head frames
        // cannot describe three different scenes.
        let committed = self.displayed_surface_view().to_vec();
        let display_list = self.prepare_repaint(&committed, raised_surface, focused_surface)?;
        let output = output_descriptors
            .first()
            .copied()
            .ok_or("software composition has no output descriptor")?;
        let compose_started = Instant::now();
        let composition = scene
            .compose_display_list(
                output,
                &committed,
                &display_list,
                cursor_presentation.composition_position(),
            )?
            .clone();
        self.record_focus_ring_observation(&committed, LiveChromeObservationSource::Repaint, true)?;
        let head_batches = self.retained_output_head_composition_frames(scene, native_scanout)?;
        let output_count = self.outputs.output_count();
        let primary_output = self.outputs.primary_output();
        let production = &self.production;
        let surface_metadata = &self.surface_metadata;
        let ordinary_repaints_pending = &mut self.ordinary_repaints_pending;
        let outputs = &mut self.outputs;
        let mut head_batches = head_batches.into_iter().collect::<BTreeMap<_, _>>();
        let primary_logical_target = std::cell::Cell::new(None);
        let primary_logical_target_ref = &primary_logical_target;
        let mut adapter = crate::LiveProductionOutputRuntimeAdapter::new(
            output_count,
            |index, snapshot: &[CommittedSurfaceState]| -> Result<_, Box<dyn std::error::Error>> {
                let output_id = outputs
                    .output_id(index)
                    .ok_or("production output index was not registered")?;
                let frames = head_batches
                    .remove(&output_id)
                    .ok_or("CPU repaint omitted a logical-output head cohort")?;
                let logical_checksum = frames
                    .first()
                    .map(|frame| frame.logical_content_checksum)
                    .ok_or("CPU repaint produced an empty head cohort")?;
                if frames
                    .iter()
                    .any(|frame| frame.logical_content_checksum != logical_checksum)
                {
                    return Err("CPU repaint heads disagree on logical content checksum".into());
                }
                if outputs.native_initialized(output_id) {
                    let frame = ordinary_repaint::admit(
                        ordinary_repaints_pending,
                        native_scanout,
                        output_id,
                        frames,
                    )?;
                    if Some(output_id) == primary_output {
                        primary_logical_target_ref
                            .set(frame.map(|frame| {
                                LiveProductionCpuTarget::new(frame, logical_checksum)
                            }));
                    }
                } else {
                    outputs.initialize_native_head_composition(
                        native_scanout,
                        output_id,
                        frames,
                    )?;
                }
                outputs.run_output(index, snapshot, |runtime| {
                    let input = compositor_tick_input_for_committed(
                        snapshot,
                        surface_metadata,
                        0,
                        Vec::new(),
                        None,
                    );
                    Ok(if runtime.rendered_primary_plane_scanout_in_flight() {
                        runtime.run_tick(input)?
                    } else {
                        native_scanout.run_tick(output_id, runtime, input)?
                    })
                })
            },
        );
        let tick = production
            .run_outputs(&mut adapter)?
            .into_iter()
            .next()
            .ok_or("persistent backend runtime has no outputs")?;
        let primary_logical_target = primary_logical_target.get();
        self.last_primary_logical_target = primary_logical_target;
        Ok(LiveProductionCpuSubmission {
            tick,
            composition,
            composed: true,
            compose_elapsed: compose_started.elapsed(),
            primary_logical_target,
        })
    }

    pub fn run_observation_tick(
        &mut self,
    ) -> Result<crate::LiveBackendRuntimeTickReport, Box<dyn std::error::Error>> {
        // Both views from one read, and the assembly resynchronised before the
        // tick. This was the one tick that never replaced the committed list, so
        // it paired fresh templates against whatever an earlier cycle had left in
        // the assembly -- a mismatch the engine rejects as an invalid surface,
        // masked until the first client surface ever committed and deterministic
        // from then on. Nine call paths lead here, which is why the failure
        // looked unrelated to any of them.
        let (layer_templates, committed) = self.scene_views();
        let output = self
            .outputs
            .values_mut()
            .next()
            .ok_or("persistent backend runtime has no outputs")?;
        output
            .runtime
            .assembly_mut()
            .replace_committed_surfaces(committed);
        Ok(output
            .runtime
            .run_tick(compositor_tick_input(&layer_templates, 0, Vec::new(), None))?)
    }
}
