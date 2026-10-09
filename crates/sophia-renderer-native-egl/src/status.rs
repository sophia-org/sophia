#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeEglProbeStatus {
    NativeDrawingCapable,
    PlatformUnavailable,
    PlatformDegraded,
    ContextUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeEglDrawSmokeStatus {
    ClearColorReady,
    PlatformUnavailable,
    PlatformDegraded,
    ContextUnavailable,
    SurfaceUnavailable,
    MakeCurrentUnavailable,
    GlUnavailable,
}

#[cfg(feature = "gbm-platform")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeGbmBackedEglPlatformStatus {
    NativePlatformCapable,
    PlatformUnavailable,
    PlatformDegraded,
}

#[cfg(feature = "gbm-platform")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativePresentationSmokeStatus {
    Ready,
    Unavailable,
    Degraded,
}

#[cfg(feature = "gbm-platform")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeGbmEglFrameTargetAllocationStatus {
    Ready,
    Unavailable,
    Degraded,
}

#[cfg(feature = "gbm-platform")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeGbmScanoutBufferExportStatus {
    Exported,
    InvalidTarget,
    Unavailable,
    Degraded,
}

#[cfg(feature = "gbm-platform")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeGbmScanoutBufferExportDetail {
    Exported,
    InvalidTarget,
    BackendDeviceUnavailable,
    GbmDeviceUnavailable,
    EglUnavailable,
    EglDisplayUnavailable,
    EglInitializeFailed,
    EglBindApiFailed,
    EglConfigUnavailable,
    GbmSurfaceUnavailable,
    EglSurfaceUnavailable,
    EglContextUnavailable,
    EglMakeCurrentFailed,
    GlSmokeFailed,
    CpuLayerUploadFailed,
    DmaBufImageCreateFailed,
    DmaBufImageBindFailed,
    CompositionDrawFailed,
    CompositionFinishFailed,
    EglImageDestroyFailed,
    DmaBufImportFailed,
    EglSwapBuffersFailed,
    FrontBufferLockFailed,
    InvalidBufferDescriptor,
    InvalidRendererImageId,
    DmaBufDescriptorMismatch,
    DmaBufImportCacheFull,
    RendererImageStoreFull,
    /// Every image bridge is still behind a GPU completion; a later attempt
    /// may succeed without anything being freed. Not a bounds refusal.
    RendererImageTransferBusy,
}

#[cfg(feature = "gbm-platform")]
impl NativeGbmScanoutBufferExportDetail {
    pub const fn status(self) -> NativeGbmScanoutBufferExportStatus {
        match self {
            Self::Exported => NativeGbmScanoutBufferExportStatus::Exported,
            Self::InvalidTarget => NativeGbmScanoutBufferExportStatus::InvalidTarget,
            Self::BackendDeviceUnavailable
            | Self::GbmDeviceUnavailable
            | Self::EglUnavailable
            | Self::EglDisplayUnavailable
            | Self::GbmSurfaceUnavailable => NativeGbmScanoutBufferExportStatus::Unavailable,
            Self::EglInitializeFailed
            | Self::EglBindApiFailed
            | Self::EglConfigUnavailable
            | Self::EglSurfaceUnavailable
            | Self::EglContextUnavailable
            | Self::EglMakeCurrentFailed
            | Self::GlSmokeFailed
            | Self::CpuLayerUploadFailed
            | Self::DmaBufImageCreateFailed
            | Self::DmaBufImageBindFailed
            | Self::CompositionDrawFailed
            | Self::CompositionFinishFailed
            | Self::EglImageDestroyFailed
            | Self::DmaBufImportFailed
            | Self::EglSwapBuffersFailed
            | Self::FrontBufferLockFailed
            | Self::InvalidBufferDescriptor
            | Self::InvalidRendererImageId
            | Self::DmaBufDescriptorMismatch
            | Self::DmaBufImportCacheFull
            | Self::RendererImageStoreFull
            | Self::RendererImageTransferBusy => NativeGbmScanoutBufferExportStatus::Degraded,
        }
    }

    /// A restore's capacity refusal, read against pooled storage still held
    /// behind GPU uses: full while such uses are outstanding is busy, since
    /// they finish without any image counter changing; with none outstanding
    /// the store is full or quarantined and stays full.
    pub const fn restore_capacity(self, fences_outstanding: bool) -> Self {
        match self {
            Self::RendererImageStoreFull if fences_outstanding => Self::RendererImageTransferBusy,
            detail => detail,
        }
    }

    pub const fn render_target_retryable(self) -> bool {
        matches!(
            self,
            Self::EglMakeCurrentFailed
                | Self::EglSwapBuffersFailed
                | Self::GlSmokeFailed
                | Self::CpuLayerUploadFailed
                | Self::CompositionDrawFailed
                | Self::CompositionFinishFailed
                | Self::EglImageDestroyFailed
        )
    }

    pub const fn import_cache_rejection(self) -> bool {
        matches!(
            self,
            Self::InvalidRendererImageId
                | Self::DmaBufDescriptorMismatch
                | Self::DmaBufImportCacheFull
        )
    }
}

#[cfg(feature = "gbm-platform")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeGbmRenderedScanoutContextStatus {
    Ready,
    Unavailable,
    Degraded,
}
