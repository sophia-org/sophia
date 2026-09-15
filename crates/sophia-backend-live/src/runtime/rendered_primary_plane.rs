#[cfg(feature = "libdrm-events")]
use super::*;
#[cfg(feature = "libdrm-events")]
use std::any::Any;

#[cfg(feature = "libdrm-events")]
impl<P> LiveBackendRuntimeAssembly<P>
where
    P: NonBlockingInputPoller,
{
    pub fn rendered_primary_plane_scanout_in_flight(&self) -> bool {
        self.primary_output_state().in_flight()
    }

    pub fn rendered_primary_plane_scanout_in_flight_for(&self, output: OutputId) -> bool {
        self.outputs
            .get(output)
            .is_some_and(LiveRenderedOutputState::in_flight)
    }
    pub fn rendered_primary_plane_completion_fence_status_for(
        &self,
        output: OutputId,
    ) -> std::io::Result<LibdrmNativeCompletionFenceStatus> {
        self.outputs
            .get(output)
            .and_then(|state| state.scanout_custody.submitted())
            .map_or(
                Ok(LibdrmNativeCompletionFenceStatus::Unsupported),
                LiveRenderedPrimaryPlaneScanoutSubmission::completion_fence_status,
            )
    }

    /// Arm from the current native head before callback intake or retirement.
    /// Absence invalidates authority; it never restores the legacy path.
    pub(crate) fn set_native_retirement_witness(
        &mut self,
        output: OutputId,
        expected: Option<crate::LiveNativeFrameIdentity>,
    ) {
        let state = self
            .outputs
            .get_mut(output)
            .expect("registered native output");
        state.retirement_authority = RenderedRetirementAuthority::Native(expected);
    }

    pub(crate) fn submitted_rendered_frame_correlation(
        &self,
        output: OutputId,
    ) -> Option<crate::LiveRendererFrameCorrelation> {
        self.outputs
            .get(output)?
            .scanout_custody
            .submitted()?
            .correlation()
    }

    pub fn rendered_primary_plane_scanout_cleanup_pending(&self) -> bool {
        self.primary_output_state().cleanup_pending()
    }

    pub fn rendered_primary_plane_scanout_cleanup_pending_for(&self, output: OutputId) -> bool {
        self.outputs
            .get(output)
            .is_some_and(LiveRenderedOutputState::cleanup_pending)
    }

    pub fn rendered_primary_plane_scanout_displayed(&self) -> bool {
        self.primary_output_state()
            .scanout_custody
            .displayed()
            .is_some()
    }

    /// Releases the final displayed submission during bounded session
    /// teardown. Persistent scanout intentionally retains that submission
    /// between frames, so callers must retire it through the DRM device before
    /// dropping the renderer-owned buffer.
    pub fn retire_displayed_rendered_primary_plane_scanout<D>(
        &mut self,
        device: &D,
    ) -> LiveTrackedRenderedPrimaryPlaneScanoutCleanupReport
    where
        D: LibdrmNativePrimaryPlaneResourceDevice,
    {
        let state = self.primary_output_state_mut();
        state.retain_rendered_primary_plane_displayed_submission = false;
        let outcome = state.scanout_custody.retire_displayed(device);
        let destroy = outcome
            .as_ref()
            .ok()
            .and_then(|result| result.as_ref())
            .map(|result| result.destroy);
        LiveTrackedRenderedPrimaryPlaneScanoutCleanupReport {
            status: match outcome {
                Err(()) => LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::CleanupFailed,
                Ok(Some(result)) if !result.released => {
                    LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::CleanupFailed
                }
                Ok(Some(_)) => LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::CleanedUp,
                Ok(None) => LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::NoCleanupPending,
            },
            destroy,
            cleanup_pending: state.cleanup_pending(),
        }
    }

    pub fn with_persistent_rendered_primary_plane_scanout(mut self) -> Self {
        self.primary_output_state_mut()
            .retain_rendered_primary_plane_displayed_submission = true;
        self
    }

    pub(crate) fn adopt_presented_rendered_primary_plane_scanout<Owner>(
        &mut self,
        submission: LiveRenderedPrimaryPlaneScanoutSubmission<Owner>,
    ) -> bool
    where
        Owner: 'static,
    {
        self.try_adopt_presented_rendered_primary_plane_scanout(submission)
            .is_ok()
    }

    /// Attempts to adopt a synchronously displayed owner without losing it on
    /// rejection. Startup has already mutated KMS when this transfer occurs, so
    /// the rejected affine owner must remain available to explicit teardown.
    #[expect(
        clippy::result_large_err,
        reason = "rejected adoption must return the existing displayed owner without allocation after KMS commit"
    )]
    pub(crate) fn try_adopt_presented_rendered_primary_plane_scanout<Owner>(
        &mut self,
        submission: LiveRenderedPrimaryPlaneScanoutSubmission<Owner>,
    ) -> Result<(), LiveRenderedPrimaryPlaneScanoutSubmission<Owner>>
    where
        Owner: 'static,
    {
        let state = self.primary_output_state_mut();
        if !state.retain_rendered_primary_plane_displayed_submission
            || state.scanout_custody.displayed().is_some()
            || state.scanout_custody.submitted().is_some()
            || state.scanout_custody.cleanup_pending()
        {
            return Err(submission);
        }
        state
            .scanout_custody
            .adopt_displayed(submission.map_scanout_buffer(|owner| Box::new(owner) as Box<dyn Any>))
            .expect("adoption checked before owner transfer");
        Ok(())
    }

    pub fn rendered_primary_plane_scanout_in_flight_ticks(&self) -> u64 {
        self.primary_output_state()
            .rendered_primary_plane_scanout_in_flight_ticks
    }

    pub fn rendered_primary_plane_scanout_backpressure_report(
        &self,
        threshold_ticks: u64,
    ) -> LiveRenderedPrimaryPlaneScanoutBackpressureReport {
        let state = self.primary_output_state();
        LiveRenderedPrimaryPlaneScanoutBackpressureReport::from_in_flight_state(
            state.in_flight(),
            state.rendered_primary_plane_scanout_in_flight_ticks,
            threshold_ticks,
        )
    }

    pub fn rendered_primary_plane_runtime_scanout_state(&self) -> Option<RuntimeScanoutState> {
        self.primary_output_state()
            .rendered_primary_plane_runtime_scanout_state
    }

    pub fn pending_runtime_scanout_state_count(&self) -> usize {
        self.primary_output_state()
            .pending_runtime_scanout_states
            .len()
    }

    pub fn submit_rendered_primary_plane_scanout_with<D, E>(
        &mut self,
        device: &D,
        exporter: &mut E,
    ) -> LiveRenderedPrimaryPlaneScanoutSubmitResult<E::Owner>
    where
        D: LibdrmNativeKmsSelectionDevice
            + LibdrmNativePropertyLookupDevice
            + LibdrmNativePrimaryPlaneResourceDevice
            + LibdrmNativeAtomicCommitDevice,
        E: LiveRenderedScanoutBufferExporter,
        E::Owner: LiveRenderedScanoutBufferPrimeSource,
    {
        let state = self.primary_output_state();
        let selection = state.native_selection().map_or_else(
            || select_native_primary_plane_target(device),
            |selection| LibdrmNativePrimaryPlaneSelectionResult {
                status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                selection: Some(selection),
            },
        );
        submit_rendered_primary_plane_scanout_from_scanout_target_and_selection_with(
            state.kms_scanout_target.status,
            state.gbm_egl_frame_target,
            selection,
            state.vrr_property_request,
            state.cursor_ride_request,
            device,
            exporter,
        )
    }

    pub fn submit_and_track_rendered_primary_plane_scanout_with<D, E>(
        &mut self,
        device: &D,
        exporter: &mut E,
    ) -> LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport
    where
        D: LibdrmNativeKmsSelectionDevice
            + LibdrmNativePropertyLookupDevice
            + LibdrmNativePrimaryPlaneResourceDevice
            + LibdrmNativeAtomicCommitDevice,
        E: LiveRenderedScanoutBufferExporter,
        E::Owner: LiveRenderedScanoutBufferPrimeSource + 'static,
    {
        let state = self.primary_output_state_mut();
        let selection = state.native_selection().map_or_else(
            || select_native_primary_plane_target(device),
            |selection| LibdrmNativePrimaryPlaneSelectionResult {
                status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                selection: Some(selection),
            },
        );
        track_rendered_primary_plane_scanout_submit_from_target_and_selection_with(
            state.kms_scanout_target.status,
            state.output_size,
            state.gbm_egl_frame_target,
            &mut state.scanout_custody,
            &mut state.rendered_primary_plane_runtime_scanout_state,
            &mut state.rendered_primary_plane_scanout_in_flight_ticks,
            state.page_flip_callback_intake.last_frame_serial(),
            Some(&mut state.pending_runtime_scanout_states),
            selection,
            state.vrr_property_request,
            state.cursor_ride_request,
            device,
            exporter,
        )
    }

    pub fn retire_tracked_rendered_primary_plane_scanout_after_page_flip<D>(
        &mut self,
        device: &D,
        callback: &LivePageFlipCallbackReport,
    ) -> LiveTrackedRenderedPrimaryPlaneScanoutRetireReport
    where
        D: LibdrmNativePrimaryPlaneResourceDevice,
    {
        retire_tracked_output_after_page_flip(self.primary_output_state_mut(), device, callback)
    }

    pub fn retry_tracked_rendered_primary_plane_scanout_cleanup<D>(
        &mut self,
        device: &D,
    ) -> LiveTrackedRenderedPrimaryPlaneScanoutCleanupReport
    where
        D: LibdrmNativePrimaryPlaneResourceDevice,
    {
        retry_tracked_output_cleanup(self.primary_output_state_mut(), device)
    }

    pub fn drain_rendered_primary_plane_page_flip_callbacks_with<D>(
        &mut self,
        device: &D,
    ) -> LiveRenderedPrimaryPlanePageFlipDrainReport
    where
        D: LibdrmNativePrimaryPlaneResourceDevice,
    {
        let page_flip_callbacks = self.drain_page_flip_callback_queue();
        let rendered_primary_plane_scanout_retire =
            page_flip_callbacks.last_accepted.map(|callback| {
                self.retire_tracked_rendered_primary_plane_scanout_after_page_flip(
                    device, &callback,
                )
            });
        LiveRenderedPrimaryPlanePageFlipDrainReport {
            page_flip_callbacks,
            rendered_primary_plane_scanout_retire,
        }
    }

    pub(crate) fn advance_rendered_primary_plane_scanout_age_if_in_flight(&mut self) {
        for state in self.outputs.outputs.values_mut() {
            if state.in_flight() {
                state.rendered_primary_plane_scanout_in_flight_ticks = state
                    .rendered_primary_plane_scanout_in_flight_ticks
                    .saturating_add(1);
            }
        }
    }
}

#[cfg(feature = "libdrm-events")]
fn retire_tracked_output_after_page_flip<D>(
    state: &mut LiveRenderedOutputState,
    device: &D,
    callback: &LivePageFlipCallbackReport,
) -> LiveTrackedRenderedPrimaryPlaneScanoutRetireReport
where
    D: LibdrmNativePrimaryPlaneResourceDevice,
{
    use crate::PersistentFlipOutcome;
    use LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus as Status;
    let (status, layout_witness, destroy, runtime_scanout_state) =
        if state.scanout_custody.submitted().is_none() {
            (Status::NoSubmission, None, None, None)
        } else if state.lost_a_head() {
            let result = state
                .scanout_custody
                .discard_submitted(device)
                .expect("submission checked");
            (
                Status::HeadLost,
                None,
                Some(result.destroy),
                Some(RuntimeScanoutState::Rejected),
            )
        } else if !state.retirement_authority.permits(
            state
                .scanout_custody
                .submitted()
                .and_then(|owner| owner.correlation())
                .and_then(|frame| frame.native),
        ) {
            (Status::WaitingForAcceptedPageFlip, None, None, None)
        } else if !state.retain_rendered_primary_plane_displayed_submission {
            let result = state
                .scanout_custody
                .retire_transient(device, callback)
                .expect("submission checked");
            (
                result.status.into(),
                result.layout_witness,
                result.destroy,
                result.runtime_scanout_state(),
            )
        } else {
            let expected = match state.retirement_authority {
                RenderedRetirementAuthority::Legacy => None,
                RenderedRetirementAuthority::Native(expected) => expected,
            };
            match state.scanout_custody.present(device, callback, expected) {
                PersistentFlipOutcome::NoSubmission => (Status::NoSubmission, None, None, None),
                PersistentFlipOutcome::Waiting | PersistentFlipOutcome::IdentityMismatch => {
                    (Status::WaitingForAcceptedPageFlip, None, None, None)
                }
                PersistentFlipOutcome::Presented {
                    layout_witness,
                    previous_cleanup,
                    ..
                } => (
                    Status::RetiredAfterPageFlip,
                    layout_witness,
                    previous_cleanup.map(|result| result.destroy),
                    Some(RuntimeScanoutState::Retired),
                ),
            }
        };
    if !state.in_flight() {
        state.rendered_primary_plane_scanout_in_flight_ticks = 0;
    }
    if let Some(runtime_state) = runtime_scanout_state {
        state.rendered_primary_plane_runtime_scanout_state = Some(runtime_state);
        state
            .pending_runtime_scanout_states
            .push_back(runtime_state);
    }
    LiveTrackedRenderedPrimaryPlaneScanoutRetireReport {
        status,
        layout_witness,
        destroy,
        runtime_scanout_state,
        in_flight: state.in_flight(),
        in_flight_ticks: state.rendered_primary_plane_scanout_in_flight_ticks,
        cleanup_pending: state.cleanup_pending(),
    }
}

#[cfg(feature = "libdrm-events")]
fn retry_tracked_output_cleanup<D>(
    state: &mut LiveRenderedOutputState,
    device: &D,
) -> LiveTrackedRenderedPrimaryPlaneScanoutCleanupReport
where
    D: LibdrmNativePrimaryPlaneResourceDevice,
{
    let Some(result) = state.scanout_custody.retry_cleanup(device) else {
        return LiveTrackedRenderedPrimaryPlaneScanoutCleanupReport {
            status: LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::NoCleanupPending,
            destroy: None,
            cleanup_pending: false,
        };
    };
    LiveTrackedRenderedPrimaryPlaneScanoutCleanupReport {
        status: if !result.released {
            LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::CleanupFailed
        } else {
            LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::CleanedUp
        },
        destroy: Some(result.destroy),
        cleanup_pending: state.cleanup_pending(),
    }
}

#[cfg(all(test, feature = "libdrm-events"))]
#[path = "retirement_authority_tests.rs"]
mod retirement_authority_tests;
