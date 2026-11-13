use crate::prelude::*;
use sophia_engine::{AuthorityTransactionInbox, AuthorityTransactionIntake};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveSessionCompositionSmokeStatus {
    Passed,
    NoAuthorityBatches,
    RuntimeTickFailed,
    RenderedScanoutSubmitMissing,
    AuthorityBatchNotCommitted,
    RenderedScanoutNotSubmitted,
    RenderedScanoutNotRetired,
    RenderedScanoutCleanupPending,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LiveSessionCompositionSmokeReport {
    pub status: LiveSessionCompositionSmokeStatus,
    pub authority_batches_input: usize,
    pub authority_batches_drained: usize,
    pub authority_transactions_committed: u64,
    pub authority_surfaces_applied: u64,
    pub rendered_scanout_submit: Option<LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus>,
    pub rendered_scanout_retire: Option<LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus>,
    pub rendered_scanout_cleanup: Option<LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus>,
    pub runtime_scanout_state: Option<RuntimeScanoutState>,
    pub rendered_scanout_in_flight: bool,
    pub cleanup_pending: bool,
}

impl LiveSessionCompositionSmokeReport {
    pub fn reduced_log_line(&self) -> String {
        format!(
            "sophia_live_session_composition schema=2 status={:?} authority_batches_input={} authority_batches_drained={} authority_transactions_committed={} authority_surfaces_applied={} rendered_scanout_submit={} rendered_scanout_retire={} rendered_scanout_cleanup={} runtime_scanout_state={} rendered_scanout_in_flight={} cleanup_pending={}",
            self.status,
            self.authority_batches_input,
            self.authority_batches_drained,
            self.authority_transactions_committed,
            self.authority_surfaces_applied,
            reduced_status(self.rendered_scanout_submit),
            reduced_status(self.rendered_scanout_retire),
            reduced_status(self.rendered_scanout_cleanup),
            reduced_status(self.runtime_scanout_state),
            self.rendered_scanout_in_flight,
            self.cleanup_pending,
        )
    }
}

pub fn run_live_session_composition_smoke(
    authority_batches: Vec<AuthorityTransactionIntake>,
) -> LiveSessionCompositionSmokeReport {
    let authority_batches_input = authority_batches.len();
    if authority_batches.is_empty() {
        return LiveSessionCompositionSmokeReport {
            status: LiveSessionCompositionSmokeStatus::NoAuthorityBatches,
            authority_batches_input,
            authority_batches_drained: 0,
            authority_transactions_committed: 0,
            authority_surfaces_applied: 0,
            rendered_scanout_submit: None,
            rendered_scanout_retire: None,
            rendered_scanout_cleanup: None,
            runtime_scanout_state: None,
            rendered_scanout_in_flight: false,
            cleanup_pending: false,
        };
    }

    let output = HeadlessOutput::deterministic();
    let transactions = authority_batches
        .iter()
        .flat_map(|batch| batch.transactions.iter().cloned())
        .collect::<Vec<_>>();
    let (sender, receiver) = std::sync::mpsc::sync_channel(authority_batches.len().max(1));
    for batch in authority_batches {
        if sender.try_send(batch).is_err() {
            break;
        }
    }
    drop(sender);

    let assembly = HeadlessCompositorBackendAssembly::new(output).with_authority_inbox(
        AuthorityTransactionInbox::new(receiver, authority_batches_input),
    );
    let mut runtime = LiveBackendRuntimeAssembly::from_ready_headless_scanout(
        assembly,
        output,
        LiveRendererRuntimeObservation::from_startup_status(
            LiveRendererImportStartupStatus::from_path_statuses(
                LiveRendererImportPathStatus::Disabled,
                LiveRendererImportPathStatus::Disabled,
            ),
            LiveRendererSelectionObservation::CpuFallback,
        ),
    );
    let device = DeterministicPrimaryPlaneScanoutDevice::ready();
    let mut exporter = DeterministicRenderedScanoutExporter::exported(output.size);

    let tick = match runtime.run_tick_with_rendered_primary_plane_scanout_with(
        CompositorBackendTickInput {
            x_event_count: u32::try_from(transactions.len()).unwrap_or(u32::MAX),
            authority_commits: Vec::new(),
            authority_batches: Vec::new(),
            wm_update: None,
            portal_commands: Vec::new(),
            chrome_command_count: 0,
            layer_templates: sophia_engine::layer_templates_from_surface_transactions(
                &transactions,
            ),
            scanout_submit_state: None,
            scanout_lifecycle_states: Vec::new(),
        },
        &device,
        &mut exporter,
    ) {
        Ok(tick) => tick,
        Err(_) => {
            return LiveSessionCompositionSmokeReport {
                status: LiveSessionCompositionSmokeStatus::RuntimeTickFailed,
                authority_batches_input,
                authority_batches_drained: 0,
                authority_transactions_committed: 0,
                authority_surfaces_applied: 0,
                rendered_scanout_submit: None,
                rendered_scanout_retire: None,
                rendered_scanout_cleanup: None,
                runtime_scanout_state: None,
                rendered_scanout_in_flight: false,
                cleanup_pending: false,
            };
        }
    };

    let authority_batches_drained = tick.engine.authority_inbox.drained;
    let authority_transactions_committed = tick
        .engine
        .runtime
        .runtime_state
        .authority_transactions_committed;
    let authority_surfaces_applied = tick.engine.runtime.runtime_state.authority_surfaces_applied;
    let rendered_scanout_submit = tick
        .rendered_primary_plane_scanout_submit
        .as_ref()
        .map(|submit| submit.status);
    let submit_runtime_scanout_state = tick
        .rendered_primary_plane_scanout_submit
        .as_ref()
        .and_then(|submit| submit.runtime_scanout_state);

    let page_flip = LivePageFlipCallbackReport {
        decision: LivePageFlipCallbackDecision::Accepted,
        event: LivePageFlipEvent {
            status: LivePageFlipEventStatus::Presented,
            frame_serial: Some(1),
        },
    };
    let retire =
        runtime.retire_tracked_rendered_primary_plane_scanout_after_page_flip(&device, &page_flip);
    let cleanup = runtime.retry_tracked_rendered_primary_plane_scanout_cleanup(&device);
    let rendered_scanout_retire = Some(retire.status);
    let rendered_scanout_cleanup = Some(cleanup.status);
    let runtime_scanout_state = retire
        .runtime_scanout_state
        .or(submit_runtime_scanout_state);
    let rendered_scanout_in_flight = runtime.rendered_primary_plane_scanout_in_flight();
    let cleanup_pending = runtime.rendered_primary_plane_scanout_cleanup_pending();

    let status = if authority_transactions_committed == 0 || authority_surfaces_applied == 0 {
        LiveSessionCompositionSmokeStatus::AuthorityBatchNotCommitted
    } else if rendered_scanout_submit.is_none() {
        LiveSessionCompositionSmokeStatus::RenderedScanoutSubmitMissing
    } else if rendered_scanout_submit
        != Some(LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip)
    {
        LiveSessionCompositionSmokeStatus::RenderedScanoutNotSubmitted
    } else if rendered_scanout_retire
        != Some(LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip)
    {
        LiveSessionCompositionSmokeStatus::RenderedScanoutNotRetired
    } else if cleanup_pending
        || !matches!(
            rendered_scanout_cleanup,
            Some(
                LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::CleanedUp
                    | LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::NoCleanupPending
            )
        )
    {
        LiveSessionCompositionSmokeStatus::RenderedScanoutCleanupPending
    } else {
        LiveSessionCompositionSmokeStatus::Passed
    };

    LiveSessionCompositionSmokeReport {
        status,
        authority_batches_input,
        authority_batches_drained,
        authority_transactions_committed,
        authority_surfaces_applied,
        rendered_scanout_submit,
        rendered_scanout_retire,
        rendered_scanout_cleanup,
        runtime_scanout_state,
        rendered_scanout_in_flight,
        cleanup_pending,
    }
}

#[derive(Debug)]
struct DeterministicRenderedScanoutOwner;

impl LiveRenderedScanoutBufferPrimeSource for DeterministicRenderedScanoutOwner {
    fn shares_kms_drm_file(&self) -> bool {
        true
    }

    fn export_scanout_dma_buf_fds(&self) -> std::io::Result<Option<LiveRenderedScanoutDmaBufFds>> {
        Ok(None)
    }
}

struct DeterministicRenderedScanoutExporter {
    descriptor: LiveRendererScanoutBufferDescriptor,
    owner: Option<DeterministicRenderedScanoutOwner>,
}

impl DeterministicRenderedScanoutExporter {
    fn exported(size: Size) -> Self {
        Self {
            descriptor: LiveRendererScanoutBufferDescriptor::new(
                size,
                u32::try_from(size.width).unwrap_or(0).saturating_mul(4),
                LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                17,
            ),
            owner: Some(DeterministicRenderedScanoutOwner),
        }
    }
}

impl LiveRenderedScanoutBufferExporter for DeterministicRenderedScanoutExporter {
    type Owner = DeterministicRenderedScanoutOwner;

    fn export_rendered_scanout_buffer(
        &mut self,
        _target: LiveGbmEglFrameTargetRecord,
    ) -> LiveRenderedScanoutBufferExport<Self::Owner> {
        LiveRenderedScanoutBufferExport {
            status: LiveRendererScanoutBufferExportStatus::Exported,
            detail: LiveRendererScanoutBufferExportDetail::Exported,
            descriptor: Some(self.descriptor),
            owner: self.owner.take(),
            correlation: None,
        }
    }
}

/// Also the device boundary of the combined accept controls (G3-4).
#[derive(Debug)]
pub(crate) struct DeterministicPrimaryPlaneScanoutDevice {
    selection: DeterministicKmsSelectionDevice,
    properties: DeterministicPropertyLookupDevice,
    resources: DeterministicPrimaryPlaneResourceDevice,
}

impl DeterministicPrimaryPlaneScanoutDevice {
    pub(crate) fn ready() -> Self {
        Self {
            selection: DeterministicKmsSelectionDevice::ready(),
            properties: DeterministicPropertyLookupDevice::ready(),
            resources: DeterministicPrimaryPlaneResourceDevice,
        }
    }

    /// G3-5d d-2b2: `ready` with its one connector mode at `size`, so a
    /// fixture whose output is not the deterministic size can match it.
    #[cfg(all(
        any(test, feature = "test-support"),
        feature = "experimental-combined-frame-job"
    ))]
    pub(crate) fn sized(size: Size) -> Self {
        Self {
            selection: DeterministicKmsSelectionDevice { size },
            ..Self::ready()
        }
    }
}

impl LibdrmNativeKmsSelectionDevice for DeterministicPrimaryPlaneScanoutDevice {
    fn connector_handles(&self) -> std::io::Result<Vec<drm::control::connector::Handle>> {
        self.selection.connector_handles()
    }

    fn crtc_handles(&self) -> std::io::Result<Vec<drm::control::crtc::Handle>> {
        self.selection.crtc_handles()
    }

    fn connector_snapshot(
        &self,
        connector: drm::control::connector::Handle,
    ) -> std::io::Result<LibdrmNativeConnectorSnapshot> {
        self.selection.connector_snapshot(connector)
    }

    fn encoder_snapshot(
        &self,
        encoder: drm::control::encoder::Handle,
    ) -> std::io::Result<LibdrmNativeEncoderSnapshot> {
        self.selection.encoder_snapshot(encoder)
    }

    fn plane_handles(&self) -> std::io::Result<Vec<drm::control::plane::Handle>> {
        self.selection.plane_handles()
    }

    fn plane_snapshot(
        &self,
        plane: drm::control::plane::Handle,
    ) -> std::io::Result<LibdrmNativePlaneSnapshot> {
        self.selection.plane_snapshot(plane)
    }

    fn plane_type(
        &self,
        plane: drm::control::plane::Handle,
    ) -> std::io::Result<Option<drm::control::PlaneType>> {
        self.selection.plane_type(plane)
    }
}

impl LibdrmNativePropertyLookupDevice for DeterministicPrimaryPlaneScanoutDevice {
    fn connector_property_handles(
        &self,
        connector: drm::control::connector::Handle,
    ) -> std::io::Result<LibdrmNativePropertyHandleSet> {
        self.properties.connector_property_handles(connector)
    }

    fn crtc_property_handles(
        &self,
        crtc: drm::control::crtc::Handle,
    ) -> std::io::Result<LibdrmNativePropertyHandleSet> {
        self.properties.crtc_property_handles(crtc)
    }

    fn plane_property_handles(
        &self,
        plane: drm::control::plane::Handle,
    ) -> std::io::Result<LibdrmNativePropertyHandleSet> {
        self.properties.plane_property_handles(plane)
    }
}

impl LibdrmNativePrimaryPlaneResourceDevice for DeterministicPrimaryPlaneScanoutDevice {
    fn create_mode_blob_for_selection(
        &self,
        selection: LibdrmNativePrimaryPlaneSelection,
    ) -> std::io::Result<u64> {
        self.resources.create_mode_blob_for_selection(selection)
    }

    fn create_mode_blob(&self, mode: drm::control::Mode) -> std::io::Result<u64> {
        self.resources.create_mode_blob(mode)
    }

    fn add_scanout_framebuffer_with_modifiers<B>(
        &self,
        buffer: &B,
    ) -> std::io::Result<drm::control::framebuffer::Handle>
    where
        B: drm::buffer::PlanarBuffer + ?Sized,
    {
        self.resources
            .add_scanout_framebuffer_with_modifiers(buffer)
    }

    fn add_scanout_framebuffer_without_modifiers<B>(
        &self,
        buffer: &B,
    ) -> std::io::Result<drm::control::framebuffer::Handle>
    where
        B: drm::buffer::PlanarBuffer + ?Sized,
    {
        self.resources
            .add_scanout_framebuffer_without_modifiers(buffer)
    }

    fn add_legacy_scanout_framebuffer<B>(
        &self,
        buffer: &B,
        depth: u32,
        bpp: u32,
    ) -> std::io::Result<drm::control::framebuffer::Handle>
    where
        B: drm::buffer::Buffer + ?Sized,
    {
        self.resources
            .add_legacy_scanout_framebuffer(buffer, depth, bpp)
    }

    fn destroy_scanout_framebuffer(
        &self,
        framebuffer: drm::control::framebuffer::Handle,
    ) -> std::io::Result<()> {
        self.resources.destroy_scanout_framebuffer(framebuffer)
    }

    fn destroy_mode_blob(&self, mode_blob: u64) -> std::io::Result<()> {
        self.resources.destroy_mode_blob(mode_blob)
    }
}

impl LibdrmNativeAtomicCommitDevice for DeterministicPrimaryPlaneScanoutDevice {
    fn submit_atomic_commit(
        &self,
        _flags: drm::control::AtomicCommitFlags,
        _request: drm::control::atomic::AtomicModeReq,
    ) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct DeterministicKmsSelectionDevice {
    size: Size,
}

impl DeterministicKmsSelectionDevice {
    fn ready() -> Self {
        Self {
            size: HeadlessOutput::deterministic().size,
        }
    }
}

impl LibdrmNativeKmsSelectionDevice for DeterministicKmsSelectionDevice {
    fn connector_handles(&self) -> std::io::Result<Vec<drm::control::connector::Handle>> {
        Ok(vec![connector_handle()])
    }

    fn crtc_handles(&self) -> std::io::Result<Vec<drm::control::crtc::Handle>> {
        Ok(vec![crtc_handle()])
    }

    fn connector_snapshot(
        &self,
        _connector: drm::control::connector::Handle,
    ) -> std::io::Result<LibdrmNativeConnectorSnapshot> {
        Ok(LibdrmNativeConnectorSnapshot::new(
            true,
            Some(encoder_handle()),
            [encoder_handle()],
            Some(self.size),
        ))
    }

    fn encoder_snapshot(
        &self,
        _encoder: drm::control::encoder::Handle,
    ) -> std::io::Result<LibdrmNativeEncoderSnapshot> {
        Ok(LibdrmNativeEncoderSnapshot::new(
            Some(crtc_handle()),
            [crtc_handle()],
        ))
    }

    fn plane_handles(&self) -> std::io::Result<Vec<drm::control::plane::Handle>> {
        Ok(vec![plane_handle()])
    }

    fn plane_snapshot(
        &self,
        _plane: drm::control::plane::Handle,
    ) -> std::io::Result<LibdrmNativePlaneSnapshot> {
        Ok(LibdrmNativePlaneSnapshot::new([crtc_handle()]))
    }

    fn plane_type(
        &self,
        _plane: drm::control::plane::Handle,
    ) -> std::io::Result<Option<drm::control::PlaneType>> {
        Ok(Some(drm::control::PlaneType::Primary))
    }
}

#[derive(Debug)]
struct DeterministicPropertyLookupDevice;

impl DeterministicPropertyLookupDevice {
    fn ready() -> Self {
        Self
    }
}

impl LibdrmNativePropertyLookupDevice for DeterministicPropertyLookupDevice {
    fn connector_property_handles(
        &self,
        _connector: drm::control::connector::Handle,
    ) -> std::io::Result<LibdrmNativePropertyHandleSet> {
        Ok(LibdrmNativePropertyHandleSet::new([(
            "CRTC_ID",
            property_handle(101),
        )]))
    }

    fn crtc_property_handles(
        &self,
        _crtc: drm::control::crtc::Handle,
    ) -> std::io::Result<LibdrmNativePropertyHandleSet> {
        Ok(LibdrmNativePropertyHandleSet::new([
            ("MODE_ID", property_handle(102)),
            ("ACTIVE", property_handle(103)),
        ]))
    }

    fn plane_property_handles(
        &self,
        _plane: drm::control::plane::Handle,
    ) -> std::io::Result<LibdrmNativePropertyHandleSet> {
        Ok(LibdrmNativePropertyHandleSet::new([
            ("FB_ID", property_handle(104)),
            ("CRTC_ID", property_handle(105)),
            ("SRC_X", property_handle(106)),
            ("SRC_Y", property_handle(107)),
            ("SRC_W", property_handle(108)),
            ("SRC_H", property_handle(109)),
            ("CRTC_X", property_handle(110)),
            ("CRTC_Y", property_handle(111)),
            ("CRTC_W", property_handle(112)),
            ("CRTC_H", property_handle(113)),
            ("IN_FORMATS", property_handle(114)),
        ]))
    }
}

#[derive(Debug)]
struct DeterministicPrimaryPlaneResourceDevice;

impl LibdrmNativePrimaryPlaneResourceDevice for DeterministicPrimaryPlaneResourceDevice {
    fn create_mode_blob_for_selection(
        &self,
        _selection: LibdrmNativePrimaryPlaneSelection,
    ) -> std::io::Result<u64> {
        Ok(15)
    }

    fn create_mode_blob(&self, _mode: drm::control::Mode) -> std::io::Result<u64> {
        Ok(15)
    }

    fn add_scanout_framebuffer_with_modifiers<B>(
        &self,
        _buffer: &B,
    ) -> std::io::Result<drm::control::framebuffer::Handle>
    where
        B: drm::buffer::PlanarBuffer + ?Sized,
    {
        Ok(framebuffer_handle())
    }

    fn add_scanout_framebuffer_without_modifiers<B>(
        &self,
        _buffer: &B,
    ) -> std::io::Result<drm::control::framebuffer::Handle>
    where
        B: drm::buffer::PlanarBuffer + ?Sized,
    {
        Ok(framebuffer_handle())
    }

    fn add_legacy_scanout_framebuffer<B>(
        &self,
        _buffer: &B,
        _depth: u32,
        _bpp: u32,
    ) -> std::io::Result<drm::control::framebuffer::Handle>
    where
        B: drm::buffer::Buffer + ?Sized,
    {
        Ok(framebuffer_handle())
    }

    fn destroy_scanout_framebuffer(
        &self,
        _framebuffer: drm::control::framebuffer::Handle,
    ) -> std::io::Result<()> {
        Ok(())
    }

    fn destroy_mode_blob(&self, _mode_blob: u64) -> std::io::Result<()> {
        Ok(())
    }
}

fn reduced_status<T: core::fmt::Debug>(status: Option<T>) -> String {
    status
        .map(|status| format!("{status:?}"))
        .unwrap_or_else(|| "none".to_owned())
}

fn property_handle(raw: u32) -> drm::control::property::Handle {
    drm::control::from_u32(raw).expect("deterministic property handle should be nonzero")
}

fn connector_handle() -> drm::control::connector::Handle {
    drm::control::from_u32(11).expect("deterministic connector handle should be nonzero")
}

fn crtc_handle() -> drm::control::crtc::Handle {
    drm::control::from_u32(12).expect("deterministic crtc handle should be nonzero")
}

fn encoder_handle() -> drm::control::encoder::Handle {
    drm::control::from_u32(16).expect("deterministic encoder handle should be nonzero")
}

fn plane_handle() -> drm::control::plane::Handle {
    drm::control::from_u32(13).expect("deterministic plane handle should be nonzero")
}

fn framebuffer_handle() -> drm::control::framebuffer::Handle {
    drm::control::from_u32(14).expect("deterministic framebuffer handle should be nonzero")
}
