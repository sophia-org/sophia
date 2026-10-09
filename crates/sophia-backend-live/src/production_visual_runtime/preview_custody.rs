//! Prepare the currently displayed sources before a publication needs them.
use super::*;

type PreviewPresentFrames = (
    TransactionId,
    sophia_renderer_live::LiveRendererImageId,
    Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>,
);

impl LiveProductionVisualRuntime {
    pub(super) fn prepare_policy_preview_images(
        &mut self,
        native: &mut LiveProductionNativeScanout,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut demand = BTreeMap::<_, (SurfaceId, BTreeSet<OutputId>)>::new();
        if let Some(publication) = &self.policy_presentation {
            for instance in &publication.presentation.instances {
                if let Some(displayed) = self.displayed_surfaces.get(&instance.source)
                    && !self.displayed_direct_presents.values().any(|transaction| {
                        crate::presentation::renderer_image_for_present(*transaction)
                            == displayed.layer.image_id
                    })
                {
                    demand
                        .entry(displayed.layer.image_id)
                        .or_insert_with(|| (instance.source, BTreeSet::new()))
                        .1
                        .insert(instance.output);
                }
            }
        }
        if let Err(refusal) = native.prepare_preview_images(&demand)
            && !self.handle_preview_refusal(&refusal)
        {
            return Err(refusal.into());
        }
        Ok(())
    }
}

impl LiveProductionVisualRuntime {
    pub(super) fn recover_policy_preview_frames<T: composition_target::PreviewRecoveryTarget>(
        &mut self,
        scene: &LiveProductionCpuScene,
        native: &mut T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for failure in native.preview_frame_failures() {
            self.revoke_failed_preview(failure);
            if !native.withdraw_preview_frame(failure)? {
                continue;
            }
            if self.present_scheduler.in_flight_frame(failure.output) == Some(failure.frame) {
                if self.present_scheduler.unsubmitted_frame(failure.output) != Some(failure.frame) {
                    return Err(
                        "preview recovery tried to replace a submitted Present frame".into(),
                    );
                }
                self.retry_present_after_preview_refusal(scene, native, failure)?;
            } else if self
                .software_present_frame_owners
                .contains_key(&failure.frame)
            {
                self.retry_software_present_after_preview_refusal(native, failure)?;
            } else {
                // Shell content may have awaited this exact first frame. Re-arm
                // its retirement claim on the replacement, rather than issuing
                // a receipt for pixels from the failed publication.
                self.rearm_shell_retirement_claims(failure.output, failure.frame)?;
                self.retained_projection_pending = true;
            }
            native.finish_preview_frame_recovery(failure)?;
        }
        Ok(())
    }

    pub(super) fn settle_detached_preview_failures(
        &mut self,
        failures: impl IntoIterator<Item = crate::LivePreviewFrameFailure>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut first_error = None;
        for failure in failures {
            self.revoke_failed_preview(failure);
            if let Err(error) = self.rearm_shell_retirement_claims(failure.output, failure.frame) {
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error.into()),
            None => Ok(()),
        }
    }

    pub(super) fn revoke_failed_preview(&mut self, failure: crate::LivePreviewFrameFailure) {
        // A late failure must not revoke a newer WM publication.
        if self
            .policy_presentation
            .as_ref()
            .is_some_and(|publication| {
                publication.owner_epoch == failure.owner_epoch
                    && publication.presentation.generation == failure.publication
            })
        {
            self.policy_presentation = None;
            self.policy_presentation_revocation = Some(LivePolicyPresentationRevocation {
                owner_epoch: failure.owner_epoch,
                generation: failure.publication,
                source: failure.source,
            });
            self.retained_projection_pending = true;
        }
    }

    fn retry_present_after_preview_refusal<T: composition_target::PreviewRecoveryTarget>(
        &mut self,
        scene: &LiveProductionCpuScene,
        native: &mut T,
        failure: crate::LivePreviewFrameFailure,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (transaction, image, batches) =
            self.present_preview_recovery_frames(scene, native, failure.output)?;
        if !live_present_head_frames_capture_image(&batches, image) {
            // A preview-only source has exactly one retirement owner. Revoking
            // its only visible use can skip it only before any physical submit.
            let only_output = self
                .outputs
                .logical_viewports()
                .map(|(output, _)| output)
                .filter(|output| self.present_scheduler.in_flight_frame(*output).is_some())
                .count()
                == 1;
            if !only_output
                || self
                    .present_scheduler
                    .submitted_frame(failure.output)
                    .is_some()
            {
                return Err("preview recovery cannot skip a partially submitted Present".into());
            }
            // Rollback is fallible: keep the logical owner until it succeeds.
            native.rollback_renderer_image(image)?;
            let dropped = self
                .present_scheduler
                .take_rendering()
                .ok_or("preview recovery lost its unsubmitted Present")?;
            // No physical owner remains. Never broadcast this rollback for a
            // mixed cohort whose other output still owns staged scanout pixels.
            self.reject_gpu_presentation(dropped.transaction);
            self.retained_projection_pending = true;
            return Ok(());
        }
        let queued =
            native.queue_preview_present_replacement(failure, Some(transaction), batches)?;
        let frame = *queued
            .get(&failure.output)
            .ok_or("preview recovery did not admit its replacement")?;
        if !self
            .present_scheduler
            .replace_unsubmitted_frame(failure.output, failure.frame, frame)
        {
            return Err("preview recovery could not transfer its unsubmitted frame mapping".into());
        }
        Ok(())
    }
    /// The same frozen Present candidate and still-held client planes used by
    /// the original admission. Kept generic only over head facts for fixtures.
    pub(super) fn present_preview_recovery_frames<T: NativeCompositionTarget>(
        &self,
        _scene: &LiveProductionCpuScene,
        native: &T,
        output: OutputId,
    ) -> Result<PreviewPresentFrames, Box<dyn std::error::Error>> {
        let transaction = self
            .present_scheduler
            .in_flight_transaction()
            .ok_or("preview recovery lost its Present transaction")?;
        let (presenting, layer) = self
            .present_scheduler
            .in_flight_displayed_layer()
            .ok_or("preview recovery lost its Present image")?;
        let image = layer.image_id;
        let prepared = self
            .present_scheduler
            .in_flight_prepared()
            .ok_or("preview recovery lost its prepared candidate")?;
        let (sources, order) = self
            .present_scheduler
            .in_flight_recovery_sources()
            .ok_or("preview recovery lost its frozen sources")?;
        let list = self.recovery_display_list_for_output(
            output,
            prepared.candidate(),
            order,
            Some(presenting),
        )?;
        let frames = self.compose_native_head_frames_from_sources(
            native,
            output,
            prepared.candidate(),
            list,
            (transaction.raw(), Some(prepared)),
            sources,
        )?;
        let batches = vec![(output, frames)];
        Ok((transaction, image, batches))
    }
}
