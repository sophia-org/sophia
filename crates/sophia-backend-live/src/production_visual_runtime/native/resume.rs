//! Native resume onto a replacement owner (t306): renderer workers and
//! retained images are prepared before the first presentation, at a resolved
//! layout's viewports (t310); and the return to suspension when a resume fails.

use super::*;

/// What abandoning a failed resume left the runtime as. Neither case touches
/// the caller's retained-image handoff: a failed resume never takes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeResumeAbandonment {
    /// The resume failed before the runtime adopted the replacement's
    /// outputs. The runtime is still suspended on the retired owner's set,
    /// which nothing changed; only the replacement is to be retired.
    BeforeInstall,
    /// The runtime had adopted the replacement and is suspended from it again.
    /// The report is the replacement's, for its retirement mode.
    Suspended(LiveProductionNativeSuspendReport),
}

impl LiveProductionVisualRuntime {
    /// Returns the runtime to suspension after `resume_native_scanout_at`
    /// failed on `native_scanout`, so the caller can
    /// retire that replacement and try another owner or wait with the same
    /// retained images. Source availability the failed resume derived is
    /// derived again by the next resume, and nothing composes meanwhile.
    ///
    /// An error here means the runtime could not be detached from the
    /// replacement even by forced revocation: no suspended state can be
    /// established, and the session cannot continue on it.
    pub fn abandon_native_resume(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        outputs: &[sophia_engine::HeadlessOutput],
        timeout: Duration,
    ) -> Result<LiveProductionNativeResumeAbandonment, Box<dyn std::error::Error>> {
        self.abandon_native_resume_with(outputs, |runtime| {
            runtime.suspend_native_scanout(native_scanout, outputs, timeout)
        })
    }

    /// The device-free rule of `abandon_native_resume`. `suspend` is the
    /// bounded drain and detach of the replacement. A drain failure whose
    /// forced detach completed is already a suspension; only a detach that did
    /// not complete falls back to forced revocation, so nothing detaches twice.
    pub(crate) fn abandon_native_resume_with(
        &mut self,
        outputs: &[sophia_engine::HeadlessOutput],
        suspend: impl FnOnce(
            &mut Self,
        )
            -> Result<LiveProductionNativeSuspendReport, Box<dyn std::error::Error>>,
    ) -> Result<LiveProductionNativeResumeAbandonment, Box<dyn std::error::Error>> {
        if self.native_suspended {
            return Ok(LiveProductionNativeResumeAbandonment::BeforeInstall);
        }
        let report = match suspend(self) {
            Ok(report) => report,
            Err(error) => match error.downcast::<LiveProductionNativeSuspendError>() {
                Ok(error) if error.detach_report.is_some() => {
                    error.detach_report.expect("detach report checked")
                }
                _ => self.suspend_revoked_native_scanout(outputs)?,
            },
        };
        Ok(LiveProductionNativeResumeAbandonment::Suspended(report))
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
            self.resumed_output_set(outputs, Some(native_scanout), Some(logical_viewports))?;
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
