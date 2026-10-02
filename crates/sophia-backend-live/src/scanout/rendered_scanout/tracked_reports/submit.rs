use crate::prelude::*;

use super::{reduced_size, reduced_status};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport {
    pub status: LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus,
    pub layout_witness: Option<crate::LiveScanoutLayoutWitness>,
    pub scanout_target: LiveKmsScanoutTargetStatus,
    pub output_size: Option<Size>,
    pub target: Option<LiveGbmEglFrameTargetStatus>,
    pub target_size: Option<Size>,
    pub export: Option<LiveRendererScanoutBufferExportStatus>,
    pub export_detail: Option<LiveRendererScanoutBufferExportDetail>,
    pub scanout_buffer: Option<LiveRendererScanoutBufferStatus>,
    pub buffer_format: Option<LibdrmNativeScanoutBufferFormatDetail>,
    pub buffer_modifier: Option<LibdrmNativeScanoutBufferModifierDetail>,
    pub buffer_planes: Option<LibdrmNativeScanoutBufferPlaneDetail>,
    pub properties: Option<LibdrmNativePrimaryPlanePropertyDiscoveryStatus>,
    pub format_table: Option<LibdrmNativePrimaryPlaneFormatTableStatus>,
    pub resources: Option<LibdrmNativePrimaryPlaneResourceCreateStatus>,
    pub framebuffer: Option<LibdrmNativePrimaryPlaneFramebufferCreateDetail>,
    pub request: Option<LibdrmNativeAtomicRequestBuildStatus>,
    pub submit: Option<LibdrmNativePrimaryPlaneScanoutSubmitStatus>,
    pub request_scope: Option<LibdrmNativeAtomicCommitRequestScope>,
    pub commit_flags: Option<LibdrmNativeAtomicCommitFlagsReport>,
    pub commit_submit: Option<LibdrmNativeAtomicCommitSubmitStatus>,
    pub atomic_test: Option<LibdrmNativeAtomicTestReport>,
    pub runtime_scanout_state: Option<RuntimeScanoutState>,
    pub in_flight: bool,
    pub in_flight_ticks: u64,
    pub cleanup_pending: bool,
    /// The commit was accepted only after its cursor was dropped.
    pub cursor_dropped: bool,
}

impl LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport {
    pub fn reduced_log_line(&self) -> String {
        let (commit_page_flip_event, commit_nonblocking, commit_allow_modeset, commit_test_only) =
            self.commit_flags
                .map(|flags| {
                    (
                        flags.page_flip_event.to_string(),
                        flags.nonblocking.to_string(),
                        flags.allow_modeset.to_string(),
                        flags.test_only.to_string(),
                    )
                })
                .unwrap_or_else(|| {
                    (
                        "none".to_owned(),
                        "none".to_owned(),
                        "none".to_owned(),
                        "none".to_owned(),
                    )
                });

        format!(
            "sophia_runtime_rendered_scanout_submit schema=6 status={:?} scanout_target={:?} output_size={} target={} target_size={} export={} scanout_buffer={} buffer_format={} buffer_modifier={} buffer_planes={} properties={} format_table={} resources={} framebuffer={} request={} submit={} request_scope={} commit_page_flip_event={} commit_nonblocking={} commit_allow_modeset={} commit_test_only={} commit_submit={} runtime_scanout_state={} in_flight={} in_flight_ticks={} cleanup_pending={}",
            self.status,
            self.scanout_target,
            reduced_size(self.output_size),
            reduced_status(self.target),
            reduced_size(self.target_size),
            reduced_status(self.export),
            reduced_status(self.scanout_buffer),
            reduced_status(self.buffer_format),
            reduced_status(self.buffer_modifier),
            reduced_status(self.buffer_planes),
            reduced_status(self.properties),
            reduced_status(self.format_table),
            reduced_status(self.resources),
            reduced_status(self.framebuffer),
            reduced_status(self.request),
            reduced_status(self.submit),
            reduced_status(self.request_scope),
            commit_page_flip_event,
            commit_nonblocking,
            commit_allow_modeset,
            commit_test_only,
            reduced_status(self.commit_submit),
            reduced_status(self.runtime_scanout_state),
            self.in_flight,
            self.in_flight_ticks,
            self.cleanup_pending,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus {
    SubmittedWaitingForPageFlip,
    ScanoutExportPending,
    ScanoutTargetNotReady,
    FrameTargetUnavailable,
    ScanoutExportFailed,
    PrimaryPlaneSubmitFailed,
    AlreadyInFlight,
    CleanupPending,
}

impl From<LiveRenderedPrimaryPlaneScanoutSubmitStatus>
    for LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus
{
    fn from(status: LiveRenderedPrimaryPlaneScanoutSubmitStatus) -> Self {
        match status {
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip => {
                Self::SubmittedWaitingForPageFlip
            }
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::ScanoutExportPending => {
                Self::ScanoutExportPending
            }
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::ScanoutTargetNotReady => {
                Self::ScanoutTargetNotReady
            }
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::FrameTargetUnavailable => {
                Self::FrameTargetUnavailable
            }
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::ScanoutExportFailed => {
                Self::ScanoutExportFailed
            }
            LiveRenderedPrimaryPlaneScanoutSubmitStatus::PrimaryPlaneSubmitFailed => {
                Self::PrimaryPlaneSubmitFailed
            }
        }
    }
}
