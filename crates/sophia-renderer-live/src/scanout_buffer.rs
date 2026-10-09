use crate::Size;
use crate::{
    LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888, LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
    LiveGbmEglFrameTargetRecord,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveRendererScanoutBufferDescriptor {
    pub status: LiveRendererScanoutBufferStatus,
    pub size: Size,
    pub pitch: u32,
    pub format: u32,
    pub gem_handle: u32,
    pub plane_count: u8,
    pub plane_handles: [u32; 4],
    pub plane_pitches: [u32; 4],
    pub plane_offsets: [u32; 4],
    pub modifier: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveRendererScanoutBufferPlanes {
    pub count: u8,
    pub handles: [u32; 4],
    pub pitches: [u32; 4],
    pub offsets: [u32; 4],
    pub modifier: Option<u64>,
}

impl LiveRendererScanoutBufferPlanes {
    pub const fn single(handle: u32, pitch: u32) -> Self {
        Self {
            count: 1,
            handles: [handle, 0, 0, 0],
            pitches: [pitch, 0, 0, 0],
            offsets: [0, 0, 0, 0],
            modifier: None,
        }
    }
}

impl LiveRendererScanoutBufferDescriptor {
    pub const fn new(size: Size, pitch: u32, format: u32, gem_handle: u32) -> Self {
        Self::new_with_planes(
            size,
            pitch,
            format,
            gem_handle,
            LiveRendererScanoutBufferPlanes::single(gem_handle, pitch),
        )
    }

    pub const fn new_with_planes(
        size: Size,
        pitch: u32,
        format: u32,
        gem_handle: u32,
        planes: LiveRendererScanoutBufferPlanes,
    ) -> Self {
        Self {
            status: if is_valid_scanout_buffer_shape(size, pitch, format, gem_handle, planes) {
                LiveRendererScanoutBufferStatus::Ready
            } else {
                LiveRendererScanoutBufferStatus::Invalid
            },
            size,
            pitch,
            format,
            gem_handle,
            plane_count: planes.count,
            plane_handles: planes.handles,
            plane_pitches: planes.pitches,
            plane_offsets: planes.offsets,
            modifier: planes.modifier,
        }
    }

    /// A descriptor for a buffer this process holds only as DMA-BUF file
    /// descriptors -- a client's own buffer on the direct scanout path.
    ///
    /// There is no local GEM handle for such a buffer until PRIME import
    /// creates one, and import happens inside resource creation, after the
    /// descriptor has already been validated. The handle fields therefore
    /// carry `LIVE_RENDERER_SCANOUT_IMPORTED_PLANE_HANDLE` -- a value the
    /// shape rules accept as nonzero and that never reaches the kernel,
    /// because `from_descriptor_and_imported_plane_handles` replaces every
    /// handle with the imported one before a framebuffer is created. It is a
    /// distinguishable sentinel rather than `1` so that a handle appearing in
    /// evidence says plainly that import owns it.
    pub const fn for_imported_dma_buf_planes(
        size: Size,
        format: u32,
        plane_count: u8,
        plane_pitches: [u32; 4],
        plane_offsets: [u32; 4],
        modifier: Option<u64>,
    ) -> Self {
        let mut plane_handles = [0u32; 4];
        let mut index = 0;
        while index < LIVE_RENDERER_SCANOUT_MAX_PLANES {
            if index < plane_count as usize {
                plane_handles[index] = LIVE_RENDERER_SCANOUT_IMPORTED_PLANE_HANDLE;
            }
            index += 1;
        }
        Self::new_with_planes(
            size,
            plane_pitches[0],
            format,
            LIVE_RENDERER_SCANOUT_IMPORTED_PLANE_HANDLE,
            LiveRendererScanoutBufferPlanes {
                count: plane_count,
                handles: plane_handles,
                pitches: plane_pitches,
                offsets: plane_offsets,
                modifier,
            },
        )
    }

    pub const fn is_valid_scanout_buffer(self) -> bool {
        matches!(self.status, LiveRendererScanoutBufferStatus::Ready)
            && is_valid_scanout_buffer_shape(
                self.size,
                self.pitch,
                self.format,
                self.gem_handle,
                LiveRendererScanoutBufferPlanes {
                    count: self.plane_count,
                    handles: self.plane_handles,
                    pitches: self.plane_pitches,
                    offsets: self.plane_offsets,
                    modifier: self.modifier,
                },
            )
    }
}

const fn is_valid_scanout_buffer_shape(
    size: Size,
    pitch: u32,
    format: u32,
    gem_handle: u32,
    planes: LiveRendererScanoutBufferPlanes,
) -> bool {
    size.width > 0
        && size.height > 0
        && size.width <= (u32::MAX / LIVE_RENDERER_SCANOUT_BYTES_PER_XRGB8888_PIXEL) as i32
        && pitch >= minimum_xrgb8888_pitch(size.width)
        && gem_handle > 0
        && is_supported_scanout_format(format)
        && is_valid_scanout_planes(
            size.width,
            gem_handle,
            planes.count,
            planes.handles,
            planes.pitches,
        )
}

/// The plane handle a descriptor carries while its buffer exists only as
/// DMA-BUF file descriptors. Replaced by the imported handle before any
/// framebuffer is created; see `for_imported_dma_buf_planes`.
pub const LIVE_RENDERER_SCANOUT_IMPORTED_PLANE_HANDLE: u32 = u32::MAX;

const LIVE_RENDERER_SCANOUT_BYTES_PER_XRGB8888_PIXEL: u32 = 4;
pub const LIVE_RENDERER_SCANOUT_MAX_PLANES: usize = 4;

pub const fn is_supported_scanout_format(format: u32) -> bool {
    format == LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888
        || format == LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888
}

const fn minimum_xrgb8888_pitch(width: i32) -> u32 {
    width as u32 * LIVE_RENDERER_SCANOUT_BYTES_PER_XRGB8888_PIXEL
}

const fn is_valid_scanout_planes(
    width: i32,
    gem_handle: u32,
    plane_count: u8,
    plane_handles: [u32; 4],
    plane_pitches: [u32; 4],
) -> bool {
    if plane_count == 0 || plane_count as usize > LIVE_RENDERER_SCANOUT_MAX_PLANES {
        return false;
    }
    if plane_handles[0] != gem_handle || plane_pitches[0] < minimum_xrgb8888_pitch(width) {
        return false;
    }

    let mut index = 0;
    while index < LIVE_RENDERER_SCANOUT_MAX_PLANES {
        if index < plane_count as usize {
            if plane_handles[index] == 0 || plane_pitches[index] == 0 {
                return false;
            }
        } else if plane_handles[index] != 0 || plane_pitches[index] != 0 {
            return false;
        }
        index += 1;
    }

    true
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRendererScanoutBufferStatus {
    Ready,
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveRendererScanoutBufferExportReport {
    pub status: LiveRendererScanoutBufferExportStatus,
    pub descriptor: Option<LiveRendererScanoutBufferDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRendererScanoutBufferExportStatus {
    Exported,
    Pending,
    InvalidTarget,
    Unavailable,
    Degraded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveRendererScanoutBufferExportDetail {
    Exported,
    WorkerPending,
    WorkerQueueFull,
    WorkerDisconnected,
    WorkerStalled,
    InvalidTarget,
    /// A compose refused the frame it was handed -- a layer the renderer
    /// cannot source, an output it does not know, a transform it does not
    /// implement.
    ///
    /// Distinct from `InvalidTarget`, which says the render target is wrong.
    /// Reporting a layer fault as a target fault sent one diagnosis after the
    /// wrong half of the system: the target was fine, and the frame named a
    /// renderer image that had never been imported.
    ComposeRefused,
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
    PendingFrameMissing,
    ExportedDescriptorMissing,
    ExportedOwnerMissing,
    WorkerLeaseIdExhausted,
    FrameSlotIncarnationExhausted,
    RetainedBufferMissing,
}

impl std::fmt::Display for LiveRendererScanoutBufferExportDetail {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "live renderer scanout export failed: {self:?}")
    }
}

impl std::error::Error for LiveRendererScanoutBufferExportDetail {}

impl LiveRendererScanoutBufferExportDetail {
    pub const fn status(self) -> LiveRendererScanoutBufferExportStatus {
        match self {
            Self::Exported => LiveRendererScanoutBufferExportStatus::Exported,
            Self::WorkerPending => LiveRendererScanoutBufferExportStatus::Pending,
            Self::InvalidTarget => LiveRendererScanoutBufferExportStatus::InvalidTarget,
            Self::BackendDeviceUnavailable
            | Self::GbmDeviceUnavailable
            | Self::EglUnavailable
            | Self::EglDisplayUnavailable
            | Self::GbmSurfaceUnavailable => LiveRendererScanoutBufferExportStatus::Unavailable,
            _ => LiveRendererScanoutBufferExportStatus::Degraded,
        }
    }

    pub const fn from_status(status: LiveRendererScanoutBufferExportStatus) -> Self {
        match status {
            LiveRendererScanoutBufferExportStatus::Exported => Self::Exported,
            LiveRendererScanoutBufferExportStatus::Pending => Self::WorkerPending,
            LiveRendererScanoutBufferExportStatus::InvalidTarget => Self::InvalidTarget,
            LiveRendererScanoutBufferExportStatus::Unavailable => Self::BackendDeviceUnavailable,
            LiveRendererScanoutBufferExportStatus::Degraded => Self::RetainedBufferMissing,
        }
    }
}

pub trait LiveRendererScanoutBufferExporter {
    fn export_scanout_buffer(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
    ) -> LiveRendererScanoutBufferExportReport;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FakeRendererScanoutBufferExporter {
    pub status: LiveRendererScanoutBufferExportStatus,
    pub pitch: u32,
    pub format: u32,
    pub gem_handle: u32,
}

impl FakeRendererScanoutBufferExporter {
    pub const fn new(status: LiveRendererScanoutBufferExportStatus) -> Self {
        Self {
            status,
            pitch: 0,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            gem_handle: 0,
        }
    }

    pub const fn with_descriptor(mut self, pitch: u32, format: u32, gem_handle: u32) -> Self {
        self.pitch = pitch;
        self.format = format;
        self.gem_handle = gem_handle;
        self
    }
}

impl LiveRendererScanoutBufferExporter for FakeRendererScanoutBufferExporter {
    fn export_scanout_buffer(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
    ) -> LiveRendererScanoutBufferExportReport {
        if !target.is_valid_scanout_target() {
            return LiveRendererScanoutBufferExportReport {
                status: LiveRendererScanoutBufferExportStatus::InvalidTarget,
                descriptor: None,
            };
        }

        match self.status {
            LiveRendererScanoutBufferExportStatus::Exported => {
                let descriptor = LiveRendererScanoutBufferDescriptor::new(
                    target.size,
                    self.pitch,
                    self.format,
                    self.gem_handle,
                );
                if descriptor.is_valid_scanout_buffer() {
                    LiveRendererScanoutBufferExportReport {
                        status: LiveRendererScanoutBufferExportStatus::Exported,
                        descriptor: Some(descriptor),
                    }
                } else {
                    LiveRendererScanoutBufferExportReport {
                        status: LiveRendererScanoutBufferExportStatus::Degraded,
                        descriptor: None,
                    }
                }
            }
            LiveRendererScanoutBufferExportStatus::Pending => {
                LiveRendererScanoutBufferExportReport {
                    status: LiveRendererScanoutBufferExportStatus::Pending,
                    descriptor: None,
                }
            }
            status => LiveRendererScanoutBufferExportReport {
                status,
                descriptor: None,
            },
        }
    }
}
