#[cfg(feature = "libdrm-events")]
use super::*;
#[cfg(feature = "libdrm-events")]
use crate::prelude::*;

#[cfg(feature = "libdrm-events")]
pub(crate) struct LiveRenderedPrimaryPlaneRuntimeAdapter<'a, D, E> {
    pub(crate) inner: LiveRuntimeDriverAdapter,
    pub(crate) scanout_target: LiveKmsScanoutTargetStatus,
    pub(crate) output_size: Option<Size>,
    pub(crate) target: Option<LiveGbmEglFrameTargetRecord>,
    pub(crate) custody: &'a mut crate::PersistentScanoutCustody,
    pub(crate) rendered_primary_plane_runtime_scanout_state: &'a mut Option<RuntimeScanoutState>,
    pub(crate) rendered_primary_plane_scanout_in_flight_ticks: &'a mut u64,
    pub(crate) submitted_after_page_flip_serial: Option<u64>,
    pub(crate) selection: LibdrmNativePrimaryPlaneSelectionResult,
    pub(crate) vrr_enabled: Option<bool>,
    pub(crate) cursor_ride: Option<crate::LibdrmNativeAtomicCursor>,
    pub(crate) device: &'a D,
    pub(crate) exporter: &'a mut E,
    pub(crate) submit_report: &'a mut Option<LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport>,
}

#[cfg(feature = "libdrm-events")]
impl<D, E> RuntimeDriverAdapter for LiveRenderedPrimaryPlaneRuntimeAdapter<'_, D, E>
where
    D: LibdrmNativeKmsSelectionDevice
        + LibdrmNativePropertyLookupDevice
        + LibdrmNativePrimaryPlaneResourceDevice
        + LibdrmNativeAtomicCommitDevice,
    E: LiveRenderedScanoutBufferExporter,
    E::Owner: LiveRenderedScanoutBufferPrimeSource + 'static,
{
    fn poll_x_events(&mut self) -> Result<SessionRuntimeObservation, sophia_engine::EngineError> {
        self.inner.poll_x_events()
    }

    fn poll_x_observations(
        &mut self,
    ) -> Result<Vec<SessionRuntimeObservation>, sophia_engine::EngineError> {
        self.inner.poll_x_observations()
    }

    fn request_wm_layout(
        &mut self,
    ) -> Result<SessionRuntimeObservation, sophia_engine::EngineError> {
        self.inner.request_wm_layout()
    }

    fn schedule_frame(
        &mut self,
        frame_serial: u64,
    ) -> Result<SessionRuntimeObservation, sophia_engine::EngineError> {
        self.inner.schedule_frame(frame_serial)
    }

    fn render_frame(
        &mut self,
        engine: &HeadlessEngine,
        output: OutputId,
        frame_serial: u64,
        last_committed: &mut LastCommittedLayout,
    ) -> Result<SessionTickReport, sophia_engine::EngineError> {
        self.inner
            .render_frame(engine, output, frame_serial, last_committed)
    }

    fn submit_scanout(
        &mut self,
        frame_serial: u64,
    ) -> Result<SessionRuntimeObservation, sophia_engine::EngineError> {
        let report = track_rendered_primary_plane_scanout_submit_from_target_and_selection_with(
            self.scanout_target,
            self.output_size,
            self.target,
            self.custody,
            self.rendered_primary_plane_runtime_scanout_state,
            self.rendered_primary_plane_scanout_in_flight_ticks,
            self.submitted_after_page_flip_serial,
            None,
            self.selection,
            self.vrr_enabled,
            self.cursor_ride,
            self.device,
            self.exporter,
        );
        let state = report
            .runtime_scanout_state
            .unwrap_or(RuntimeScanoutState::Rejected);
        *self.submit_report = Some(report);

        Ok(SessionRuntimeObservation::ScanoutStateChanged {
            state,
            frame_serial: Some(frame_serial),
        })
    }

    fn drain_portal_commands(
        &mut self,
    ) -> Result<SessionRuntimeObservation, sophia_engine::EngineError> {
        self.inner.drain_portal_commands()
    }

    fn present_chrome(&mut self) -> Result<SessionRuntimeObservation, sophia_engine::EngineError> {
        self.inner.present_chrome()
    }
}
