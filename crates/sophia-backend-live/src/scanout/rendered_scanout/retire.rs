#[cfg(feature = "libdrm-events")]
use super::*;
#[cfg(feature = "libdrm-events")]
use crate::prelude::*;

#[cfg(feature = "libdrm-events")]
pub fn retire_rendered_primary_plane_scanout_after_page_flip<D, Owner>(
    device: &D,
    submission: LiveRenderedPrimaryPlaneScanoutSubmission<Owner>,
    callback: &LivePageFlipCallbackReport,
) -> LiveRenderedPrimaryPlaneScanoutRetireResult<Owner>
where
    D: LibdrmNativePrimaryPlaneResourceDevice,
{
    let waiting_for_newer_page_flip = callback.decision == LivePageFlipCallbackDecision::Accepted
        && submission
            .submitted_after_page_flip_serial
            .is_some_and(|baseline| match callback.event.frame_serial {
                Some(serial) => serial <= baseline,
                None => true,
            });
    if waiting_for_newer_page_flip {
        return LiveRenderedPrimaryPlaneScanoutRetireResult {
            status: LibdrmNativePrimaryPlaneScanoutRetireStatus::WaitingForAcceptedPageFlip,
            destroy: None,
            layout_witness: None,
            submission: Some(submission),
            cleanup: None,
        };
    }

    let LiveRenderedPrimaryPlaneScanoutSubmission {
        scanout_buffer,
        correlation,
        primary_plane,
        submitted_after_page_flip_serial,
        layout_witness,
    } = submission;
    let retired =
        retire_native_primary_plane_scanout_after_page_flip(device, primary_plane, callback);

    if let Some(primary_plane) = retired.submission {
        return LiveRenderedPrimaryPlaneScanoutRetireResult {
            status: retired.status,
            destroy: retired.destroy,
            layout_witness: None,
            submission: Some(LiveRenderedPrimaryPlaneScanoutSubmission {
                scanout_buffer,
                correlation,
                primary_plane,
                submitted_after_page_flip_serial,
                layout_witness,
            }),
            cleanup: None,
        };
    }

    if let Some(primary_plane) = retired.cleanup {
        return LiveRenderedPrimaryPlaneScanoutRetireResult {
            status: retired.status,
            destroy: retired.destroy,
            layout_witness,
            submission: None,
            cleanup: Some(LiveRenderedPrimaryPlaneScanoutCleanup {
                scanout_buffer,
                correlation,
                primary_plane,
            }),
        };
    }

    LiveRenderedPrimaryPlaneScanoutRetireResult {
        status: retired.status,
        destroy: retired.destroy,
        layout_witness,
        submission: None,
        cleanup: None,
    }
}

#[cfg(feature = "libdrm-events")]
#[derive(Debug)]
pub struct LiveRenderedPrimaryPlaneScanoutCleanupResult<Owner> {
    pub destroy: LibdrmNativePrimaryPlaneResourceDestroyStatus,
    pub cleanup: Option<LiveRenderedPrimaryPlaneScanoutCleanup<Owner>>,
}

#[cfg(feature = "libdrm-events")]
pub fn retry_rendered_primary_plane_scanout_cleanup<D, Owner>(
    device: &D,
    cleanup: LiveRenderedPrimaryPlaneScanoutCleanup<Owner>,
) -> LiveRenderedPrimaryPlaneScanoutCleanupResult<Owner>
where
    D: LibdrmNativePrimaryPlaneResourceDevice,
{
    let owner = cleanup.scanout_buffer;
    let report = cleanup.primary_plane.retry(device);
    let cleanup = report
        .cleanup
        .map(|primary_plane| LiveRenderedPrimaryPlaneScanoutCleanup {
            scanout_buffer: owner,
            correlation: cleanup.correlation,
            primary_plane,
        });

    LiveRenderedPrimaryPlaneScanoutCleanupResult {
        destroy: report.status,
        cleanup,
    }
}
