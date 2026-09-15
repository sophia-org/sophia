use crate::prelude::*;

impl<P> LiveBackendRuntimeAssembly<P>
where
    P: NonBlockingInputPoller,
{
    #[cfg(feature = "libdrm-events")]
    pub fn run_tick_with_rendered_primary_plane_scanout_with<D, E>(
        &mut self,
        mut input: CompositorBackendTickInput,
        device: &D,
        exporter: &mut E,
    ) -> Result<LiveBackendRuntimeTickReport, CompositorBackendAssemblyError>
    where
        D: LibdrmNativeKmsSelectionDevice
            + LibdrmNativePropertyLookupDevice
            + LibdrmNativePrimaryPlaneResourceDevice
            + LibdrmNativeAtomicCommitDevice,
        E: LiveRenderedScanoutBufferExporter,
        E::Owner: LiveRenderedScanoutBufferPrimeSource + 'static,
    {
        let rendered_primary_plane_scanout_cleanup_retry = self
            .rendered_primary_plane_scanout_cleanup_pending()
            .then(|| self.retry_tracked_rendered_primary_plane_scanout_cleanup(device));
        let page_flip_callbacks = self.drain_page_flip_callback_queue();
        let rendered_primary_plane_scanout_retire =
            page_flip_callbacks.last_accepted.map(|callback| {
                self.retire_tracked_rendered_primary_plane_scanout_after_page_flip(
                    device, &callback,
                )
            });
        self.advance_rendered_primary_plane_scanout_age_if_in_flight();
        let runtime_scanout_states = self.drain_pending_runtime_scanout_states_into(&mut input);

        let state = self
            .outputs
            .get_mut(self.primary_output)
            .expect("live runtime primary output must remain registered");
        let target = state.gbm_egl_frame_target;
        let output_size = state.output_size;
        let native_selection = state.native_selection();
        let scanout_target = state.kms_scanout_target.status;
        let custody = &mut state.scanout_custody;
        let rendered_primary_plane_runtime_scanout_state =
            &mut state.rendered_primary_plane_runtime_scanout_state;
        let rendered_primary_plane_scanout_in_flight_ticks =
            &mut state.rendered_primary_plane_scanout_in_flight_ticks;
        let submitted_after_page_flip_serial = state.page_flip_callback_intake.last_frame_serial();
        let selection = native_selection.map_or_else(
            || select_native_primary_plane_target(device),
            |selection| LibdrmNativePrimaryPlaneSelectionResult {
                status: LibdrmNativePrimaryPlaneSelectionStatus::Selected,
                selection: Some(selection),
            },
        );
        let vrr_enabled = state.vrr_property_request;
        let cursor_ride = state.cursor_ride_request;
        let mut rendered_primary_plane_scanout_submit = None;

        let engine = self.assembly.run_tick_with_live_runtime_adapter(
            input,
            |engine: &HeadlessEngine, intake: LiveRuntimeDriverIntake| {
                let inner = LiveRuntimeDriverAdapter::from_authority_batches(engine, intake);
                LiveRenderedPrimaryPlaneRuntimeAdapter {
                    inner,
                    scanout_target,
                    output_size,
                    target,
                    custody,
                    rendered_primary_plane_runtime_scanout_state,
                    rendered_primary_plane_scanout_in_flight_ticks,
                    submitted_after_page_flip_serial,
                    selection,
                    vrr_enabled,
                    cursor_ride,
                    device,
                    exporter,
                    submit_report: &mut rendered_primary_plane_scanout_submit,
                }
            },
            |adapter| adapter.inner.renderer.committed_surfaces.clone(),
        )?;

        Ok(self.build_tick_report(LiveBackendRuntimeTickReportInput {
            engine,
            page_flip_callbacks,
            runtime_scanout_states,
            rendered_primary_plane_scanout_cleanup_retry,
            rendered_primary_plane_scanout_submit,
            rendered_primary_plane_scanout_retire,
        }))
    }

    #[cfg(feature = "libdrm-events")]
    #[allow(clippy::too_many_arguments)]
    pub fn run_tick_with_rendered_primary_plane_scanout_and_native_page_flip_events_with<D, E, R>(
        &mut self,
        input: CompositorBackendTickInput,
        device: &D,
        exporter: &mut E,
        reader: &mut R,
        poller: &mut NativeLibdrmPageFlipEventPoller,
        sender: &std::sync::mpsc::SyncSender<LivePageFlipCallback>,
        max_read: usize,
        max_emit: usize,
    ) -> Result<LiveBackendRuntimeNativePageFlipTickReport, CompositorBackendAssemblyError>
    where
        D: LibdrmNativeKmsSelectionDevice
            + LibdrmNativePropertyLookupDevice
            + LibdrmNativePrimaryPlaneResourceDevice
            + LibdrmNativeAtomicCommitDevice,
        E: LiveRenderedScanoutBufferExporter,
        E::Owner: LiveRenderedScanoutBufferPrimeSource + 'static,
        R: LibdrmNativePageFlipReader,
    {
        let native_page_flip =
            poller.read_and_poll_page_flip_events(reader, sender, max_read, max_emit);
        self.observe_native_libdrm_poller_diagnostics(poller.diagnostics());
        let tick =
            self.run_tick_with_rendered_primary_plane_scanout_with(input, device, exporter)?;

        Ok(LiveBackendRuntimeNativePageFlipTickReport {
            native_page_flip,
            tick,
        })
    }
}
