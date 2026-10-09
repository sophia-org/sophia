use crate::api::*;

#[cfg(feature = "libdrm-events")]
use std::os::fd::OwnedFd;

#[cfg(feature = "libdrm-events")]
use sophia_renderer_live::{
    LiveRendererScanoutBufferDescriptor, LiveRendererScanoutBufferExportDetail,
    LiveRendererScanoutBufferExportStatus,
};

#[cfg(feature = "libdrm-events")]
#[derive(Debug)]
pub struct LiveRenderedScanoutBufferExport<Owner> {
    pub status: LiveRendererScanoutBufferExportStatus,
    pub detail: LiveRendererScanoutBufferExportDetail,
    pub descriptor: Option<LiveRendererScanoutBufferDescriptor>,
    pub owner: Option<Owner>,
    pub correlation: Option<super::LiveRendererFrameCorrelation>,
}

#[cfg(feature = "libdrm-events")]
impl<Owner> LiveRenderedScanoutBufferExport<Owner> {
    pub fn new(
        status: LiveRendererScanoutBufferExportStatus,
        detail: LiveRendererScanoutBufferExportDetail,
        descriptor: Option<LiveRendererScanoutBufferDescriptor>,
        owner: Option<Owner>,
    ) -> Self {
        match (status, descriptor.is_some() && owner.is_some()) {
            (LiveRendererScanoutBufferExportStatus::Exported, true) => Self {
                status,
                detail,
                descriptor,
                owner,
                correlation: None,
            },
            (LiveRendererScanoutBufferExportStatus::Exported, false) => Self {
                status: LiveRendererScanoutBufferExportStatus::Degraded,
                detail: if descriptor.is_none() {
                    LiveRendererScanoutBufferExportDetail::ExportedDescriptorMissing
                } else {
                    LiveRendererScanoutBufferExportDetail::ExportedOwnerMissing
                },
                descriptor: None,
                owner: None,
                correlation: None,
            },
            (status, _) => Self {
                status,
                detail,
                descriptor: None,
                owner: None,
                correlation: None,
            },
        }
    }

    pub fn normalized(self) -> Self {
        let correlation = self.correlation;
        Self::new(self.status, self.detail, self.descriptor, self.owner)
            .with_correlation(correlation)
    }

    /// Only an owned successful export can identify a rendered frame.
    pub fn with_correlation(
        mut self,
        correlation: Option<super::LiveRendererFrameCorrelation>,
    ) -> Self {
        self.correlation = (self.status == LiveRendererScanoutBufferExportStatus::Exported
            && self.descriptor.is_some()
            && self.owner.is_some())
        .then_some(correlation)
        .flatten();
        self
    }
}

#[cfg(feature = "libdrm-events")]
pub struct LiveScanoutLayoutProbeSource<Owner> {
    pub export: LiveRenderedScanoutBufferExport<Owner>,
    pub image: sophia_renderer_live::LiveRendererImageId,
}

#[cfg(feature = "libdrm-events")]
pub trait LiveRenderedScanoutBufferExporter {
    type Owner;

    fn export_rendered_scanout_buffer(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
    ) -> LiveRenderedScanoutBufferExport<Self::Owner>;

    /// Whether a direct attempt from this exporter still owes the driver a
    /// validating commit before it may reach a screen.
    ///
    /// True on the composition-to-direct edge and after anything that ended an
    /// eligibility episode; false while a run of direct frames continues, so
    /// the steady state costs no extra ioctl. The three hooks below default to
    /// doing nothing so that an exporter with no direct path -- every test
    /// fake, and the hardware-validation exporters -- is unaffected by this
    /// row and cannot accidentally acquire half of it.
    fn direct_scanout_test_required(&self) -> bool {
        false
    }

    /// Record the driver's answer to that validating commit.
    fn record_direct_scanout_test(&mut self, _accepted: bool) {}

    /// An actual TEST_ONLY result; a skipped test never calls this hook.
    fn record_direct_scanout_test_result(&mut self, report: crate::LibdrmNativeAtomicTestReport) {
        self.record_direct_scanout_test(
            report.status == crate::LibdrmNativeAtomicCommitSubmitStatus::Submitted,
        );
    }

    /// Actual explicit framebuffer refusal after complete DMA-BUF import.
    fn record_direct_scanout_framebuffer_rejection(
        &mut self,
        _rejection: crate::LibdrmNativeFramebufferRejection,
    ) {
    }

    /// The direct buffer reached the driver. Release the composed form kept
    /// against a refusal; never the client's buffer, which is on glass.
    fn commit_direct_scanout(&mut self) {}

    /// The direct attempt did not reach a screen. Re-offer the same content
    /// for composition and report whether anything was re-offered.
    fn fall_back_from_direct(&mut self) -> bool {
        false
    }

    /// Optional diagnostic work has its own bounded cleanup owner. A failed
    /// release must not replace cleanup owed by the ordinary submission.
    fn layout_probe_cleanup(
        &mut self,
    ) -> Option<&mut Option<crate::LiveRenderedPrimaryPlaneScanoutCleanup<Self::Owner>>> {
        None
    }

    fn take_layout_probe_source(
        &mut self,
        _completed: Option<super::LiveRendererFrameCorrelation>,
        _descriptor: LiveRendererScanoutBufferDescriptor,
    ) -> Option<LiveScanoutLayoutProbeSource<Self::Owner>> {
        None
    }

    /// A consecutive pair of current-state tests is an observation, never a
    /// reusable permission to submit or to signal client reallocation.
    fn record_layout_probe(&mut self, _report: crate::LiveScanoutLayoutProbeReport) {}
}

#[cfg(feature = "libdrm-events")]
pub trait LiveRenderedScanoutBufferPrimeSource {
    /// Whether this owner is a client's own buffer taking the direct path.
    ///
    /// The submit path asks because a direct buffer is the one case where a
    /// refusal must not be terminal: there is a composed form of the same
    /// frame waiting, and the frame is owed that second chance rather than a
    /// failed session.
    fn is_direct_client_buffer(&self) -> bool {
        false
    }

    /// Whether the renderer and KMS device descriptors share one DRM file.
    ///
    /// PRIME-importing a buffer back into the same DRM file can return the
    /// renderer's existing GEM handle. KMS cleanup would then close a handle
    /// still owned by GBM, so shared-file owners must submit their descriptor
    /// directly.
    fn shares_kms_drm_file(&self) -> bool;

    fn export_scanout_dma_buf_fds(&self) -> std::io::Result<Option<LiveRenderedScanoutDmaBufFds>>;
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl LiveRenderedScanoutBufferPrimeSource for sophia_renderer_live::NativeGbmOwnedScanoutBuffer {
    fn shares_kms_drm_file(&self) -> bool {
        true
    }

    fn export_scanout_dma_buf_fds(&self) -> std::io::Result<Option<LiveRenderedScanoutDmaBufFds>> {
        self.export_scanout_dma_buf_fds()
            .map(LiveRenderedScanoutDmaBufFds::from_native_gbm)
            .map(Some)
    }
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl LiveRenderedScanoutBufferPrimeSource for super::NativeGbmRenderedScanoutOwner {
    fn is_direct_client_buffer(&self) -> bool {
        matches!(self, super::NativeGbmRenderedScanoutOwner::Direct(_))
    }

    fn shares_kms_drm_file(&self) -> bool {
        match self {
            // Both compositor-owned buffers come from the renderer's own `dup`
            // of the KMS card file, so re-importing them would hand back a
            // handle GBM still owns.
            super::NativeGbmRenderedScanoutOwner::Inline(_)
            | super::NativeGbmRenderedScanoutOwner::Worker(_) => true,
            // A client's buffer was allocated against the client's device.
            // PRIME import is the only way it becomes a handle this file can
            // hang a framebuffer on, and the handle it produces is ours to
            // close when the bundle retires.
            super::NativeGbmRenderedScanoutOwner::Direct(_) => false,
        }
    }

    fn export_scanout_dma_buf_fds(&self) -> std::io::Result<Option<LiveRenderedScanoutDmaBufFds>> {
        match self {
            super::NativeGbmRenderedScanoutOwner::Inline(owner) => owner
                .export_scanout_dma_buf_fds()
                .map(LiveRenderedScanoutDmaBufFds::from_native_gbm)
                .map(Some),
            super::NativeGbmRenderedScanoutOwner::Worker(owner) => {
                owner.export_scanout_dma_buf_fds().map(|fds| {
                    fds.map(|(plane_count, plane_fds)| LiveRenderedScanoutDmaBufFds {
                        plane_count,
                        plane_fds,
                    })
                })
            }
            super::NativeGbmRenderedScanoutOwner::Direct(buffer) => {
                buffer.try_clone_plane_fds().map(|plane_fds| {
                    Some(LiveRenderedScanoutDmaBufFds {
                        plane_count: buffer.descriptor.plane_count,
                        plane_fds,
                    })
                })
            }
        }
    }
}

#[cfg(feature = "libdrm-events")]
pub struct LiveRenderedScanoutDmaBufFds {
    plane_count: u8,
    plane_fds: [Option<OwnedFd>; 4],
}

#[cfg(feature = "libdrm-events")]
impl LiveRenderedScanoutDmaBufFds {
    #[cfg(feature = "gbm-probe")]
    fn from_native_gbm(fds: sophia_renderer_live::NativeGbmScanoutBufferPlaneFds) -> Self {
        Self {
            plane_count: fds.plane_count(),
            plane_fds: fds.into_plane_fds(),
        }
    }

    pub fn new_for_test(plane_fds: [Option<OwnedFd>; 4], plane_count: u8) -> Self {
        Self {
            plane_count,
            plane_fds,
        }
    }

    pub const fn plane_count(&self) -> u8 {
        self.plane_count
    }

    pub fn into_plane_fds(self) -> [Option<OwnedFd>; 4] {
        self.plane_fds
    }
}
