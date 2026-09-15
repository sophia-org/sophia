#[cfg(feature = "libdrm-events")]
use crate::prelude::*;

#[cfg(feature = "libdrm-events")]
#[derive(Debug)]
pub struct LivePreparedRenderedPrimaryPlaneScanout<Owner> {
    pub(super) scanout_buffer: Owner,
    pub(super) primary_plane: LibdrmNativePrimaryPlanePreparedScanout,
    pub(super) correlation: Option<crate::LiveRendererFrameCorrelation>,
    pub(super) layout_probe: Option<Box<super::LiveScanoutLayoutProbeReport>>,
}

#[cfg(feature = "libdrm-events")]
impl<Owner> LivePreparedRenderedPrimaryPlaneScanout<Owner> {
    pub const fn correlation(&self) -> Option<crate::LiveRendererFrameCorrelation> {
        self.correlation
    }
}

#[cfg(feature = "libdrm-events")]
#[derive(Debug)]
pub struct LivePreparedRenderedTopologyHead<Owner> {
    pub(super) scanout_buffer: Owner,
    primary_plane: LibdrmNativePrimaryPlanePreparedTopologyHead,
    correlation: Option<crate::LiveRendererFrameCorrelation>,
}

#[cfg(feature = "libdrm-events")]
impl<Owner> LivePreparedRenderedTopologyHead<Owner> {
    pub const fn correlation(&self) -> Option<crate::LiveRendererFrameCorrelation> {
        self.correlation
    }

    pub const fn atomic_head(&self) -> LibdrmNativeAtomicHead {
        self.primary_plane.atomic_head()
    }
}

#[cfg(feature = "libdrm-events")]
pub struct LiveCancelledPreparedPrimaryPlaneScanout<Owner> {
    pub destroy: LibdrmNativePrimaryPlaneResourceDestroyStatus,
    pub cleanup: Option<LiveRenderedPrimaryPlaneScanoutCleanup<Owner>>,
}

#[cfg(feature = "libdrm-events")]
#[derive(Debug)]
pub struct LiveRenderedPrimaryPlaneScanoutPrepareResult<Owner> {
    pub status: LiveRenderedPrimaryPlaneScanoutPrepareStatus,
    pub scanout_target: LiveKmsScanoutTargetStatus,
    pub target: Option<LiveGbmEglFrameTargetStatus>,
    pub export: Option<LiveRendererScanoutBufferExportStatus>,
    pub scanout_buffer: Option<LiveRendererScanoutBufferStatus>,
    pub buffer_format: Option<LibdrmNativeScanoutBufferFormatDetail>,
    pub buffer_modifier: Option<LibdrmNativeScanoutBufferModifierDetail>,
    pub buffer_planes: Option<LibdrmNativeScanoutBufferPlaneDetail>,
    pub properties: Option<LibdrmNativePrimaryPlanePropertyDiscoveryStatus>,
    pub format_table: Option<LibdrmNativePrimaryPlaneFormatTableStatus>,
    pub resources: Option<LibdrmNativePrimaryPlaneResourceCreateStatus>,
    pub framebuffer: Option<LibdrmNativePrimaryPlaneFramebufferCreateDetail>,
    pub framebuffer_rejection: Option<LibdrmNativeFramebufferRejection>,
    pub request: Option<LibdrmNativeAtomicRequestBuildStatus>,
    pub submit: Option<LibdrmNativePrimaryPlaneScanoutSubmitStatus>,
    pub request_scope: Option<LibdrmNativeAtomicCommitRequestScope>,
    pub commit_flags: Option<LibdrmNativeAtomicCommitFlagsReport>,
    pub prepared: Option<LivePreparedRenderedPrimaryPlaneScanout<Owner>>,
    pub cleanup: Option<LiveRenderedPrimaryPlaneScanoutCleanup<Owner>>,
}

#[cfg(feature = "libdrm-events")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRenderedPrimaryPlaneScanoutPrepareStatus {
    Prepared,
    ScanoutExportPending,
    ScanoutTargetNotReady,
    FrameTargetUnavailable,
    ScanoutExportFailed,
    PrimaryPlanePrepareFailed,
}

#[cfg(feature = "libdrm-events")]
impl<Owner> LiveRenderedPrimaryPlaneScanoutPrepareResult<Owner> {
    fn stopped(
        status: LiveRenderedPrimaryPlaneScanoutPrepareStatus,
        scanout_target: LiveKmsScanoutTargetStatus,
        target: Option<LiveGbmEglFrameTargetStatus>,
        export: Option<LiveRendererScanoutBufferExportStatus>,
    ) -> Self {
        Self {
            status,
            scanout_target,
            target,
            export,
            scanout_buffer: None,
            buffer_format: None,
            buffer_modifier: None,
            buffer_planes: None,
            properties: None,
            format_table: None,
            resources: None,
            framebuffer: None,
            framebuffer_rejection: None,
            request: None,
            submit: None,
            request_scope: None,
            commit_flags: None,
            prepared: None,
            cleanup: None,
        }
    }
}

#[cfg(feature = "libdrm-events")]
pub fn prepare_rendered_primary_plane_scanout_from_target_and_selection_with<D, E>(
    scanout_target: LiveKmsScanoutTargetStatus,
    target: Option<LiveGbmEglFrameTargetRecord>,
    selection: LibdrmNativePrimaryPlaneSelectionResult,
    vrr_enabled: Option<bool>,
    device: &D,
    exporter: &mut E,
) -> LiveRenderedPrimaryPlaneScanoutPrepareResult<E::Owner>
where
    D: LibdrmNativePropertyLookupDevice + LibdrmNativePrimaryPlaneResourceDevice,
    E: LiveRenderedScanoutBufferExporter,
    E::Owner: LiveRenderedScanoutBufferPrimeSource,
{
    prepare_rendered_primary_plane_scanout_from_target_and_selection_with_cursor(
        scanout_target,
        target,
        selection,
        vrr_enabled,
        None,
        device,
        exporter,
    )
}

/// The same, with a cursor riding the frame's commit.
///
/// The ride is the cheap half of the transaction owner: the request is being
/// built anyway, and a cursor aboard it waits for nothing. The policy is
/// where it lands because the policy is what already travels this far --
/// VRR rides it the same way.
#[cfg(feature = "libdrm-events")]
pub fn prepare_rendered_primary_plane_scanout_from_target_and_selection_with_cursor<D, E>(
    scanout_target: LiveKmsScanoutTargetStatus,
    target: Option<LiveGbmEglFrameTargetRecord>,
    selection: LibdrmNativePrimaryPlaneSelectionResult,
    vrr_enabled: Option<bool>,
    cursor_ride: Option<crate::LibdrmNativeAtomicCursor>,
    device: &D,
    exporter: &mut E,
) -> LiveRenderedPrimaryPlaneScanoutPrepareResult<E::Owner>
where
    D: LibdrmNativePropertyLookupDevice + LibdrmNativePrimaryPlaneResourceDevice,
    E: LiveRenderedScanoutBufferExporter,
    E::Owner: LiveRenderedScanoutBufferPrimeSource,
{
    let mut policy = LibdrmNativePrimaryPlaneScanoutSubmitPolicy::page_flip();
    if let Some(cursor) = cursor_ride {
        policy = policy.with_cursor(cursor);
    }
    prepare_rendered_primary_plane_scanout_from_target_and_selection_with_policy(
        scanout_target,
        target,
        selection,
        vrr_enabled,
        device,
        exporter,
        policy,
    )
}

#[cfg(feature = "libdrm-events")]
pub fn prepare_rendered_primary_plane_topology_head_from_target_and_selection_with<D, E>(
    scanout_target: LiveKmsScanoutTargetStatus,
    target: Option<LiveGbmEglFrameTargetRecord>,
    selection: LibdrmNativePrimaryPlaneSelectionResult,
    vrr_enabled: Option<bool>,
    device: &D,
    exporter: &mut E,
) -> LiveRenderedPrimaryPlaneScanoutPrepareResult<E::Owner>
where
    D: LibdrmNativePropertyLookupDevice + LibdrmNativePrimaryPlaneResourceDevice,
    E: LiveRenderedScanoutBufferExporter,
    E::Owner: LiveRenderedScanoutBufferPrimeSource,
{
    prepare_rendered_primary_plane_scanout_from_target_and_selection_with_policy(
        scanout_target,
        target,
        selection,
        vrr_enabled,
        device,
        exporter,
        LibdrmNativePrimaryPlaneScanoutSubmitPolicy::blocking_modeset(),
    )
}

#[cfg(feature = "libdrm-events")]
fn prepare_rendered_primary_plane_scanout_from_target_and_selection_with_policy<D, E>(
    scanout_target: LiveKmsScanoutTargetStatus,
    target: Option<LiveGbmEglFrameTargetRecord>,
    selection: LibdrmNativePrimaryPlaneSelectionResult,
    vrr_enabled: Option<bool>,
    device: &D,
    exporter: &mut E,
    policy: LibdrmNativePrimaryPlaneScanoutSubmitPolicy,
) -> LiveRenderedPrimaryPlaneScanoutPrepareResult<E::Owner>
where
    D: LibdrmNativePropertyLookupDevice + LibdrmNativePrimaryPlaneResourceDevice,
    E: LiveRenderedScanoutBufferExporter,
    E::Owner: LiveRenderedScanoutBufferPrimeSource,
{
    if scanout_target != LiveKmsScanoutTargetStatus::Ready {
        return LiveRenderedPrimaryPlaneScanoutPrepareResult::stopped(
            LiveRenderedPrimaryPlaneScanoutPrepareStatus::ScanoutTargetNotReady,
            scanout_target,
            target.map(|target| target.status),
            None,
        );
    }
    let Some(target) = target else {
        return LiveRenderedPrimaryPlaneScanoutPrepareResult::stopped(
            LiveRenderedPrimaryPlaneScanoutPrepareStatus::FrameTargetUnavailable,
            scanout_target,
            None,
            None,
        );
    };
    let scanout_target =
        reduced_scanout_target_status_from_native_selection(scanout_target, target, &selection);
    if scanout_target != LiveKmsScanoutTargetStatus::Ready {
        return LiveRenderedPrimaryPlaneScanoutPrepareResult::stopped(
            LiveRenderedPrimaryPlaneScanoutPrepareStatus::ScanoutTargetNotReady,
            scanout_target,
            Some(target.status),
            None,
        );
    }
    let export = exporter.export_rendered_scanout_buffer(target).normalized();
    if export.status == LiveRendererScanoutBufferExportStatus::Pending {
        return LiveRenderedPrimaryPlaneScanoutPrepareResult::stopped(
            LiveRenderedPrimaryPlaneScanoutPrepareStatus::ScanoutExportPending,
            scanout_target,
            Some(target.status),
            Some(export.status),
        );
    }
    if export.status != LiveRendererScanoutBufferExportStatus::Exported {
        return LiveRenderedPrimaryPlaneScanoutPrepareResult::stopped(
            LiveRenderedPrimaryPlaneScanoutPrepareStatus::ScanoutExportFailed,
            scanout_target,
            Some(target.status),
            Some(export.status),
        );
    }
    let (Some(descriptor), Some(owner)) = (export.descriptor, export.owner) else {
        return LiveRenderedPrimaryPlaneScanoutPrepareResult::stopped(
            LiveRenderedPrimaryPlaneScanoutPrepareStatus::ScanoutExportFailed,
            scanout_target,
            Some(target.status),
            Some(export.status),
        );
    };
    let shares_kms_drm_file = owner.shares_kms_drm_file();
    let prime_fds = (!shares_kms_drm_file)
        .then(|| owner.export_scanout_dma_buf_fds().ok().flatten())
        .flatten();
    if !shares_kms_drm_file && prime_fds.is_none() {
        return LiveRenderedPrimaryPlaneScanoutPrepareResult::stopped(
            LiveRenderedPrimaryPlaneScanoutPrepareStatus::ScanoutExportFailed,
            scanout_target,
            Some(target.status),
            Some(export.status),
        );
    }
    let mut native = if shares_kms_drm_file {
        prepare_native_primary_plane_scanout_from_selection_and_renderer_descriptor_with_policy(
            device,
            selection,
            descriptor,
            rendered_policy(policy, vrr_enabled),
        )
    } else {
        prepare_native_primary_plane_scanout_from_selection_and_renderer_dma_bufs_with_policy(
            device,
            selection,
            descriptor,
            prime_fds
                .expect("independent DRM files require PRIME transport")
                .into_plane_fds(),
            rendered_policy(policy, vrr_enabled),
        )
    };
    let (prepared, cleanup) = match native.prepared.take() {
        Some(primary_plane) => (
            Some(LivePreparedRenderedPrimaryPlaneScanout {
                scanout_buffer: owner,
                primary_plane,
                correlation: export.correlation,
                layout_probe: None,
            }),
            None,
        ),
        None => (
            None,
            native
                .cleanup
                .take()
                .map(|primary_plane| LiveRenderedPrimaryPlaneScanoutCleanup {
                    scanout_buffer: owner,
                    correlation: export.correlation,
                    primary_plane,
                }),
        ),
    };
    let status = if prepared.is_some() {
        LiveRenderedPrimaryPlaneScanoutPrepareStatus::Prepared
    } else {
        LiveRenderedPrimaryPlaneScanoutPrepareStatus::PrimaryPlanePrepareFailed
    };
    let submit = match native.status {
        LibdrmNativePrimaryPlaneScanoutPrepareStatus::Prepared => None,
        LibdrmNativePrimaryPlaneScanoutPrepareStatus::KmsTargetUnavailable => {
            Some(LibdrmNativePrimaryPlaneScanoutSubmitStatus::KmsTargetUnavailable)
        }
        LibdrmNativePrimaryPlaneScanoutPrepareStatus::ScanoutBufferUnavailable => {
            Some(LibdrmNativePrimaryPlaneScanoutSubmitStatus::ScanoutBufferUnavailable)
        }
        LibdrmNativePrimaryPlaneScanoutPrepareStatus::PropertyDiscoveryUnavailable => {
            Some(LibdrmNativePrimaryPlaneScanoutSubmitStatus::PropertyDiscoveryUnavailable)
        }
        LibdrmNativePrimaryPlaneScanoutPrepareStatus::ResourceCreationUnavailable => {
            Some(LibdrmNativePrimaryPlaneScanoutSubmitStatus::ResourceCreationUnavailable)
        }
        LibdrmNativePrimaryPlaneScanoutPrepareStatus::AtomicRequestBuildFailed => {
            Some(LibdrmNativePrimaryPlaneScanoutSubmitStatus::AtomicRequestBuildFailed)
        }
    };
    LiveRenderedPrimaryPlaneScanoutPrepareResult {
        status,
        scanout_target,
        target: Some(target.status),
        export: Some(export.status),
        scanout_buffer: Some(native.scanout_buffer),
        buffer_format: native.buffer_format,
        buffer_modifier: native.buffer_modifier,
        buffer_planes: native.buffer_planes,
        properties: native.properties,
        format_table: native.format_table,
        resources: native.resources,
        framebuffer: native.framebuffer,
        framebuffer_rejection: native.framebuffer_rejection,
        request: native.request,
        submit,
        request_scope: native.request_scope,
        commit_flags: native.commit_flags,
        prepared,
        cleanup,
    }
}

#[cfg(feature = "libdrm-events")]
pub fn submit_prepared_rendered_primary_plane_scanout<D, Owner>(
    device: &D,
    prepared: LivePreparedRenderedPrimaryPlaneScanout<Owner>,
) -> LiveRenderedPrimaryPlaneScanoutSubmitResult<Owner>
where
    D: LibdrmNativeAtomicCommitDevice + LibdrmNativePrimaryPlaneResourceDevice,
{
    let witness = prepared
        .layout_probe
        .as_ref()
        .and_then(|report| report.witness_for(&prepared));
    let native = submit_prepared_native_primary_plane_scanout(device, prepared.primary_plane);
    let layout_witness = (native.status
        == LibdrmNativePrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
        && !native.cursor_dropped)
        .then_some(witness)
        .flatten();
    let status = if native.status
        == LibdrmNativePrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    {
        LiveRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    } else {
        LiveRenderedPrimaryPlaneScanoutSubmitStatus::PrimaryPlaneSubmitFailed
    };
    let (submission, cleanup) = match native.submission {
        Some(primary_plane) => (
            Some(LiveRenderedPrimaryPlaneScanoutSubmission {
                scanout_buffer: prepared.scanout_buffer,
                correlation: prepared.correlation,
                primary_plane,
                submitted_after_page_flip_serial: None,
                layout_witness,
            }),
            None,
        ),
        None => (
            None,
            native
                .cleanup
                .map(|primary_plane| LiveRenderedPrimaryPlaneScanoutCleanup {
                    scanout_buffer: prepared.scanout_buffer,
                    correlation: prepared.correlation,
                    primary_plane,
                }),
        ),
    };
    LiveRenderedPrimaryPlaneScanoutSubmitResult {
        status,
        scanout_target: LiveKmsScanoutTargetStatus::Ready,
        target: Some(LiveGbmEglFrameTargetStatus::Ready),
        export: Some(LiveRendererScanoutBufferExportStatus::Exported),
        scanout_buffer: Some(native.scanout_buffer),
        buffer_format: native.buffer_format,
        buffer_modifier: native.buffer_modifier,
        buffer_planes: native.buffer_planes,
        properties: native.properties,
        format_table: native.format_table,
        resources: native.resources,
        framebuffer: native.framebuffer,
        request: native.request,
        submit: Some(native.status),
        request_scope: native.request_scope,
        commit_flags: native.commit_flags,
        commit_submit: native.submit,
        atomic_test: None,
        layout_witness,
        submission,
        cleanup,
        cursor_dropped: native.cursor_dropped,
    }
}

#[cfg(feature = "libdrm-events")]
#[expect(
    clippy::result_large_err,
    reason = "the error is the prepared scanout handed back for reuse. Boxing \
it would allocate on the submit path to move a value the caller already owns \
and is about to use again"
)]
pub fn prepare_rendered_topology_head_from_prepared_scanout<Owner>(
    prepared: LivePreparedRenderedPrimaryPlaneScanout<Owner>,
    vrr_enabled: Option<bool>,
) -> Result<LivePreparedRenderedTopologyHead<Owner>, LivePreparedRenderedPrimaryPlaneScanout<Owner>>
{
    let LivePreparedRenderedPrimaryPlaneScanout {
        scanout_buffer,
        primary_plane,
        correlation,
        layout_probe: _,
    } = prepared;
    let primary_plane =
        match prepare_native_topology_head_from_prepared_scanout(primary_plane, vrr_enabled) {
            Ok(primary_plane) => primary_plane,
            Err(primary_plane) => {
                return Err(LivePreparedRenderedPrimaryPlaneScanout {
                    scanout_buffer,
                    primary_plane,
                    correlation,
                    layout_probe: None,
                });
            }
        };
    Ok(LivePreparedRenderedTopologyHead {
        scanout_buffer,
        correlation,
        primary_plane,
    })
}

#[cfg(feature = "libdrm-events")]
pub fn adopt_prepared_rendered_topology_head_after_commit<Owner>(
    prepared: LivePreparedRenderedTopologyHead<Owner>,
) -> LiveRenderedPrimaryPlaneScanoutSubmission<Owner> {
    LiveRenderedPrimaryPlaneScanoutSubmission {
        scanout_buffer: prepared.scanout_buffer,
        correlation: prepared.correlation,
        primary_plane: adopt_prepared_native_topology_head_after_commit(prepared.primary_plane),
        submitted_after_page_flip_serial: None,
        layout_witness: None,
    }
}

#[cfg(feature = "libdrm-events")]
pub fn cancel_prepared_rendered_topology_head<D, Owner>(
    device: &D,
    prepared: LivePreparedRenderedTopologyHead<Owner>,
) -> LiveCancelledPreparedPrimaryPlaneScanout<Owner>
where
    D: LibdrmNativePrimaryPlaneResourceDevice,
{
    let destroy = cancel_prepared_native_topology_head(device, prepared.primary_plane);
    LiveCancelledPreparedPrimaryPlaneScanout {
        destroy: destroy.status,
        cleanup: destroy
            .cleanup
            .map(|primary_plane| LiveRenderedPrimaryPlaneScanoutCleanup {
                scanout_buffer: prepared.scanout_buffer,
                correlation: prepared.correlation,
                primary_plane,
            }),
    }
}

#[cfg(feature = "libdrm-events")]
pub fn cancel_prepared_rendered_primary_plane_scanout<D, Owner>(
    device: &D,
    prepared: LivePreparedRenderedPrimaryPlaneScanout<Owner>,
) -> LiveCancelledPreparedPrimaryPlaneScanout<Owner>
where
    D: LibdrmNativePrimaryPlaneResourceDevice,
{
    let destroy = cancel_prepared_native_primary_plane_scanout(device, prepared.primary_plane);
    LiveCancelledPreparedPrimaryPlaneScanout {
        destroy: destroy.status,
        cleanup: destroy
            .cleanup
            .map(|primary_plane| LiveRenderedPrimaryPlaneScanoutCleanup {
                scanout_buffer: prepared.scanout_buffer,
                correlation: prepared.correlation,
                primary_plane,
            }),
    }
}

#[cfg(feature = "libdrm-events")]
const fn rendered_policy(
    policy: LibdrmNativePrimaryPlaneScanoutSubmitPolicy,
    vrr_enabled: Option<bool>,
) -> LibdrmNativePrimaryPlaneScanoutSubmitPolicy {
    match vrr_enabled {
        Some(enabled) => policy.with_vrr_enabled(enabled),
        None => policy,
    }
}
