//! Native resume onto a replacement owner (t306): renderer workers and
//! retained images are prepared before the first presentation, at the
//! ordinary row or at a resolved layout's viewports (t310).

use super::*;

impl LiveProductionVisualRuntime {
    /// Resumes onto a replacement owner with each logical output at its
    /// ordinary row position.
    pub fn resume_native_scanout(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        outputs: &[sophia_engine::HeadlessOutput],
        scene: &LiveProductionCpuScene,
        renderer_handoff: Option<&LiveProductionRendererImageHandoff>,
    ) -> Result<crate::LiveProductionRendererImageRestore, Box<dyn std::error::Error>> {
        self.resume_native_scanout_with(native_scanout, outputs, scene, renderer_handoff, None)
    }

    /// Resumes onto a replacement owner with every logical output at the given
    /// root-space viewport. The viewports are installed before retained images
    /// are restored by demand and before the first presentation, so neither
    /// is planned against a placement the resolved layout no longer has.
    pub fn resume_native_scanout_at(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        outputs: &[sophia_engine::HeadlessOutput],
        scene: &LiveProductionCpuScene,
        renderer_handoff: Option<&LiveProductionRendererImageHandoff>,
        logical_viewports: &[(OutputId, Rect)],
    ) -> Result<crate::LiveProductionRendererImageRestore, Box<dyn std::error::Error>> {
        self.resume_native_scanout_with(
            native_scanout,
            outputs,
            scene,
            renderer_handoff,
            Some(logical_viewports),
        )
    }

    fn resume_native_scanout_with(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        outputs: &[sophia_engine::HeadlessOutput],
        scene: &LiveProductionCpuScene,
        renderer_handoff: Option<&LiveProductionRendererImageHandoff>,
        logical_viewports: Option<&[(OutputId, Rect)]>,
    ) -> Result<crate::LiveProductionRendererImageRestore, Box<dyn std::error::Error>> {
        native_scanout.use_renderer_image_reads(self.image_reads.clone())?;
        self.validate_native_retirement_disposition()?;
        let retained = self.retained_renderer_image_ids();
        validate_renderer_image_resume_admission(
            &retained,
            renderer_handoff.as_ref().map(|handoff| handoff.image_ids()),
        )?;
        let mut resume_phase = crate::LiveRendererImageResumePhase::default();
        // Build runtime-only output state first. Renderer workers and retained
        // images must exist before the semantic head plans are lowered, while
        // KMS must remain untouched until every resulting owner is prepared.
        let resumed_outputs =
            self.resumed_output_set(outputs, Some(native_scanout), logical_viewports)?;
        let workers = native_scanout.enable_renderer_workers()?;
        if workers != native_scanout.enabled_head_count() {
            return Err("native resume established partial renderer-worker coverage".into());
        }
        if !native_scanout.renderer_image_owners_initialized() {
            return Err("native resume did not initialize every renderer image owner".into());
        }
        resume_phase = advance_renderer_image_resume(
            resume_phase,
            crate::LiveRendererImageResumeObservation::OutputOwnerInitialized,
        )?;
        // Availability scoped to the retired outputs described them; it is
        // derived again below from where this restore puts each image.
        self.source_availability.outputs_replaced();
        let demand = self.retained_image_demand(&resumed_outputs)?;
        let restore = match renderer_handoff {
            Some(handoff) => native_scanout.restore_renderer_image_handoff(handoff, &demand)?,
            None => crate::LiveProductionRendererImageRestore::default(),
        };
        self.mark_unrestored_sources(&restore);
        resume_phase = advance_renderer_image_resume(
            resume_phase,
            crate::LiveRendererImageResumeObservation::ImagesRestored,
        )?;
        if resume_phase != crate::LiveRendererImageResumePhase::Ready {
            return Err("native resume renderer-image lifecycle did not become ready".into());
        }
        self.resume_prepared_outputs_on(native_scanout, resumed_outputs, scene)?;
        tracing::info!(
            "sophia_live_renderer_image_handoff schema=2 status=restored restored_images={} pending_images={} unavailable_pairs={} unavailable_surfaces={}",
            restore.restored.len(),
            restore.pending.len(),
            restore.unavailable.len(),
            self.source_availability.len(),
        );
        Ok(restore)
    }

    /// The runtime set a resume installs, at the given viewports when a
    /// resolved layout supplies them. They are in place before retained
    /// images are restored by demand and before the first presentation, which
    /// both plan against the installed viewports.
    pub(crate) fn resumed_output_set(
        &self,
        outputs: &[sophia_engine::HeadlessOutput],
        native_scanout: Option<&mut LiveProductionNativeScanout>,
        logical_viewports: Option<&[(OutputId, Rect)]>,
    ) -> Result<LiveProductionOutputRuntimeSet, Box<dyn std::error::Error>> {
        let mut resumed_outputs = LiveProductionOutputRuntimeSet::new(
            outputs,
            self.production.committed_surfaces(),
            native_scanout,
        )?;
        if let Some(logical_viewports) = logical_viewports {
            resumed_outputs.replace_logical_viewports(logical_viewports)?;
        }
        Ok(resumed_outputs)
    }

    /// Native resume reaches this only after worker coverage and retained-image
    /// restoration are validated. Tests substitute those prepared device facts,
    /// then share the runtime installation and the first covered head plans.
    pub(crate) fn resume_prepared_outputs_on<T: NativeTopologyTarget>(
        &mut self,
        native_scanout: &mut T,
        resumed_outputs: LiveProductionOutputRuntimeSet,
        scene: &LiveProductionCpuScene,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Install the runtime privately, lower the restored scene for every
        // native head, and synchronously present each complete output cohort.
        self.outputs = resumed_outputs;
        self.native_suspended = false;
        self.resume_input_projections()?;
        let batches = self.retained_output_head_composition_frames(scene, native_scanout)?;
        if batches.len() != self.outputs.output_count() {
            return Err("native resume produced partial logical-output coverage".into());
        }
        for (output, frames) in batches {
            native_scanout.initialize_output_composition(&mut self.outputs, output, frames)?;
        }
        self.publish_presented_input_layers(native_scanout);
        // Published. An image held by some store but not yet by an output that
        // samples it is ordinary from here: that output's frames wait while
        // the cold migration brings it over. Only images no store holds stay
        // marked; the caller hands their snapshots back with
        // keep_pending_renderer_handoff once it has taken the handoff.
        self.source_availability.release_output_scoped_pending();
        Ok(())
    }
}
