use std::os::fd::{AsFd, BorrowedFd, OwnedFd};

use crate::{
    LiveCompositionTrace, LiveCpuBufferSourceRef, LiveGbmEglFrameTargetRecord, LiveRendererImageId,
    LiveRendererScanoutBufferDescriptor, LiveRendererScanoutBufferExportDetail,
    LiveRendererScanoutBufferExportStatus, LiveRendererScanoutBufferPlanes, Size,
};
use sophia_engine::{CompositorRgb8, HeadSamplingClass};
use sophia_protocol::{DRM_FORMAT_ARGB8888, DRM_FORMAT_XRGB8888, Rect, Transform};

pub use sophia_renderer_native_egl::{
    NativeCompositionFormatRequest as LiveCompositionFormatRequest,
    NativeCompositionOutputRequest as LiveCompositionOutputRequest,
};

mod renderer_images;
pub use renderer_images::LiveRendererImageSnapshot;

#[derive(Debug)]
pub struct NativeGbmOwnedScanoutBuffer {
    descriptor: LiveRendererScanoutBufferDescriptor,
    _buffer: sophia_renderer_native_egl::NativeGbmOwnedScanoutBuffer,
}

impl NativeGbmOwnedScanoutBuffer {
    pub const fn descriptor(&self) -> LiveRendererScanoutBufferDescriptor {
        self.descriptor
    }

    pub fn export_scanout_dma_buf_fds(&self) -> std::io::Result<NativeGbmScanoutBufferPlaneFds> {
        self._buffer
            .export_plane_fds()
            .map(NativeGbmScanoutBufferPlaneFds::new)
            .map_err(|_error| std::io::Error::other("GBM scanout DMA-BUF export failed"))
    }
}

pub struct NativeGbmScanoutBufferPlaneFds {
    inner: sophia_renderer_native_egl::NativeGbmOwnedScanoutBufferPlaneFds,
}

impl NativeGbmScanoutBufferPlaneFds {
    fn new(inner: sophia_renderer_native_egl::NativeGbmOwnedScanoutBufferPlaneFds) -> Self {
        Self { inner }
    }

    pub const fn plane_count(&self) -> u8 {
        self.inner.plane_count()
    }

    pub fn into_plane_fds(self) -> [Option<OwnedFd>; 4] {
        self.inner.into_plane_fds()
    }
}

#[derive(Debug)]
pub struct NativeGbmOwnedScanoutBufferExportReport {
    pub status: LiveRendererScanoutBufferExportStatus,
    pub detail: LiveRendererScanoutBufferExportDetail,
    pub buffer: Option<NativeGbmOwnedScanoutBuffer>,
    /// The age the surface reported for the buffer this export rendered into.
    pub buffer_age: Option<u32>,
    /// Which target bundle served the export. A caller keying retained state to
    /// a slot compares this to notice a rebuild, whose new buffers hold nothing
    /// it remembers.
    pub target_generation: Option<u64>,
    /// Whether the render repainted the whole target or only its damage.
    pub repaint: LiveNativeCompositionRepaintOutcome,
}

/// What a completed render painted, mirrored for consumers above the renderer.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LiveNativeCompositionRepaintOutcome {
    #[default]
    Full,
    Partial {
        rects: usize,
    },
}

impl NativeGbmOwnedScanoutBufferExportReport {
    pub fn new(
        status: LiveRendererScanoutBufferExportStatus,
        detail: LiveRendererScanoutBufferExportDetail,
        buffer: Option<NativeGbmOwnedScanoutBuffer>,
    ) -> Self {
        match status {
            LiveRendererScanoutBufferExportStatus::Exported => Self {
                status: if buffer.is_some() {
                    LiveRendererScanoutBufferExportStatus::Exported
                } else {
                    LiveRendererScanoutBufferExportStatus::Degraded
                },
                detail: if buffer.is_some() {
                    detail
                } else {
                    LiveRendererScanoutBufferExportDetail::RetainedBufferMissing
                },
                buffer,
                buffer_age: None,
                target_generation: None,
                repaint: LiveNativeCompositionRepaintOutcome::Full,
            },
            LiveRendererScanoutBufferExportStatus::Pending => Self {
                status,
                detail,
                buffer: None,
                buffer_age: None,
                target_generation: None,
                repaint: LiveNativeCompositionRepaintOutcome::Full,
            },
            status => Self {
                status,
                detail,
                buffer: None,
                buffer_age: None,
                target_generation: None,
                repaint: LiveNativeCompositionRepaintOutcome::Full,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeGbmRenderedScanoutContextStatus {
    Ready,
    Unavailable,
    Degraded,
}

pub struct NativeGbmRenderedScanoutContext<T: AsFd> {
    inner: sophia_renderer_native_egl::NativeGbmRenderedScanoutContext<T>,
}

#[derive(Debug)]
pub struct LiveOwnedDmaBufFrame {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub modifier: u64,
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
}

#[derive(Debug)]
pub struct LiveOwnedDmaBufPlane {
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
}

#[derive(Debug)]
pub struct LiveOwnedMultiPlaneDmaBufFrame {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub modifier: u64,
    pub plane_count: u8,
    pub planes: [Option<LiveOwnedDmaBufPlane>; 4],
}

impl LiveOwnedMultiPlaneDmaBufFrame {
    pub fn try_clone(&self) -> std::io::Result<Self> {
        let mut planes = std::array::from_fn(|_| None);
        for (target, source) in planes.iter_mut().zip(&self.planes) {
            if let Some(source) = source {
                *target = Some(LiveOwnedDmaBufPlane {
                    fd: source.fd.try_clone()?,
                    offset: source.offset,
                    stride: source.stride,
                });
            }
        }
        Ok(Self {
            width: self.width,
            height: self.height,
            format: self.format,
            modifier: self.modifier,
            plane_count: self.plane_count,
            planes,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LiveCompositionPlacement {
    pub target: Rect,
    pub clip: Option<Rect>,
    pub transform: Transform,
    pub alpha: f32,
    /// Sampling required by the realized source extent, not merely the plan.
    pub sampling: HeadSamplingClass,
}

#[derive(Clone, Copy, Debug)]
pub enum LiveMixedCompositionLayer<'a> {
    Cpu {
        buffer: LiveCpuBufferSourceRef<'a>,
        placement: LiveCompositionPlacement,
    },
    DmaBuf {
        image_id: LiveRendererImageId,
        frame: &'a LiveOwnedMultiPlaneDmaBufFrame,
        placement: LiveCompositionPlacement,
    },
    RendererImage {
        image_id: LiveRendererImageId,
        placement: LiveCompositionPlacement,
    },
    Solid {
        geometry: Rect,
        color: CompositorRgb8,
    },
}

#[derive(Debug)]
pub enum LiveOwnedMixedCompositionLayer {
    Cpu {
        buffer: crate::LiveSharedCpuBufferSource,
        placement: LiveCompositionPlacement,
    },
    DmaBuf {
        image_id: LiveRendererImageId,
        frame: LiveOwnedMultiPlaneDmaBufFrame,
        placement: LiveCompositionPlacement,
    },
    RendererImage {
        image_id: LiveRendererImageId,
        size: Size,
        format: u32,
        placement: LiveCompositionPlacement,
    },
    Solid {
        geometry: Rect,
        color: CompositorRgb8,
    },
}

#[derive(Debug, Default)]
pub struct LiveOwnedMixedCompositionFrame {
    pub layers: Vec<LiveOwnedMixedCompositionLayer>,
    pub output_damage_snapshot: Option<sophia_engine::OutputFrameDamageSnapshot>,
    pub trace: Option<LiveCompositionTrace>,
    /// Engine's verdict on the plan this frame was lowered from.
    ///
    /// Carried on the lowered frame rather than threaded beside it because
    /// the frame already travels every path a verdict would have to follow,
    /// and because a frame that arrives without one defaults to
    /// `CompositionRequired` -- an unproven frame composes. The backend never
    /// treats this as permission on its own: it re-derives the same structure
    /// from the layers below and refuses if the two disagree.
    pub direct_scanout: sophia_engine::DirectScanoutVerdict,
}

/// Why a lowered frame cannot hand its client buffer straight to the plane.
///
/// Engine's `DirectScanoutVerdict` answers the same question about the plan;
/// this answers it about the pixels that plan lowered to. The two are checked
/// independently and both must agree, so a lowering that silently added a
/// layer, or a verdict computed from a stale plan, refuses rather than puts
/// the wrong image on a screen. `NotProven` is the arm that fires when Engine
/// did not prove the frame; every other arm is this module disagreeing with a
/// verdict that said `Eligible`, which is a defect and is named as one in
/// evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveDirectScanoutRefusal {
    /// Engine did not prove this exact frame needs no composition.
    NotProven(sophia_engine::DirectScanoutVerdict),
    /// Not exactly one layer after lowering.
    LayerCount(usize),
    /// The single layer is not a client DMA-BUF.
    LayerNotDmaBuf,
    /// The layer would be filtered onto the head rather than copied.
    LayerResampled,
    /// The layer is translucent, so what is behind it is part of the image.
    LayerTranslucent,
    /// The layer carries a transform the plane cannot express here.
    LayerTransformed,
    /// The layer is the head's size but not at its origin.
    LayerOffset,
    /// The layer sits at the origin but is not the head's size.
    LayerNotHeadSized,
    /// The layer is clipped, so part of the head shows something else.
    LayerClipped,
    /// The client buffer's own extent is not the head's.
    BufferSizeMismatch,
    /// Not an opaque scanout format. ARGB8888 is excluded deliberately: its
    /// alpha is part of the image, and nothing behind it would be drawn.
    FormatNotOpaque(u32),
    /// A plane the frame claims is absent, or its descriptor is unusable.
    PlaneLayoutUnusable,
    /// The plane descriptors could not be duplicated for the plane.
    PlaneFdCloneFailed,
}

impl LiveDirectScanoutRefusal {
    /// Whether this refusal means Engine's proof and the pixels it was
    /// computed from disagree.
    ///
    /// Those are defects: an ineligible frame never becomes an attempt, so a
    /// structural refusal can only be the two halves of the proof contradicting
    /// each other. A format or plane the backend cannot use is different --
    /// Engine proves structure and never looks at pixel formats, which is the
    /// documented split -- and counting the two together made a legitimate
    /// refusal look like a defect on the first run that produced one.
    pub const fn is_structural_disagreement(self) -> bool {
        match self {
            Self::LayerCount(_)
            | Self::LayerNotDmaBuf
            | Self::LayerResampled
            | Self::LayerTranslucent
            | Self::LayerTransformed
            | Self::LayerOffset
            | Self::LayerNotHeadSized
            | Self::LayerClipped
            | Self::BufferSizeMismatch => true,
            // Not proven at all, so nothing disagreed.
            Self::NotProven(_)
            // The backend's own domain: format, plane layout, descriptors.
            | Self::FormatNotOpaque(_)
            | Self::PlaneLayoutUnusable
            | Self::PlaneFdCloneFailed => false,
        }
    }

    /// A stable name for evidence records.
    pub const fn reduced_name(self) -> &'static str {
        match self {
            Self::NotProven(_) => "not_proven",
            Self::LayerCount(_) => "layer_count",
            Self::LayerNotDmaBuf => "layer_not_dma_buf",
            Self::LayerResampled => "layer_resampled",
            Self::LayerTranslucent => "layer_translucent",
            Self::LayerTransformed => "layer_transformed",
            Self::LayerOffset => "layer_offset",
            Self::LayerNotHeadSized => "layer_not_head_sized",
            Self::LayerClipped => "layer_clipped",
            Self::BufferSizeMismatch => "buffer_size_mismatch",
            Self::FormatNotOpaque(_) => "format_not_opaque",
            Self::PlaneLayoutUnusable => "plane_layout_unusable",
            Self::PlaneFdCloneFailed => "plane_fd_clone_failed",
        }
    }
}

/// A client buffer ready to be handed to a primary plane without composition.
///
/// It carries duplicated plane descriptors rather than borrowing the frame's,
/// so the frame it came from stays whole and remains usable as the fallback
/// if the driver refuses this buffer.
#[derive(Debug)]
pub struct LiveDirectScanoutBuffer {
    pub descriptor: crate::LiveRendererScanoutBufferDescriptor,
    pub planes: [Option<LiveOwnedDmaBufPlane>; 4],
    pub image_id: LiveRendererImageId,
}

impl LiveDirectScanoutBuffer {
    /// The plane file descriptors, in plane order, for PRIME import.
    pub fn into_plane_fds(self) -> [Option<OwnedFd>; 4] {
        self.planes.map(|plane| plane.map(|plane| plane.fd))
    }

    /// The same descriptors, duplicated, for a caller holding only a borrow.
    ///
    /// The prepare path asks an owner for its descriptors without consuming
    /// it, because a failed prepare hands the owner back for cleanup. Every
    /// plane must duplicate or none does: a partial set would import some
    /// planes of a buffer and leave the rest, which the import loop reads as
    /// a missing plane and refuses -- correctly, but after the syscalls.
    pub fn try_clone_plane_fds(&self) -> std::io::Result<[Option<OwnedFd>; 4]> {
        let mut cloned = std::array::from_fn(|_| None);
        for (target, source) in std::iter::zip(&mut cloned, self.planes.iter().map(Option::as_ref))
        {
            if let Some(plane) = source {
                *target = Some(plane.fd.try_clone()?);
            }
        }
        Ok(cloned)
    }
}

impl LiveOwnedMixedCompositionFrame {
    /// The client buffer this frame could scan out directly, or why it cannot.
    ///
    /// `head_size` is the head's own framebuffer extent. "Covers the head"
    /// means exactly that rect, unclipped and unscaled: a direct flip has no
    /// composition step in which anything else could be drawn, so a layer
    /// that leaves even one row uncovered would show whatever the plane held
    /// before.
    pub fn direct_scanout_buffer(
        &self,
        head_size: Size,
    ) -> Result<LiveDirectScanoutBuffer, LiveDirectScanoutRefusal> {
        if !self.direct_scanout.is_eligible() {
            return Err(LiveDirectScanoutRefusal::NotProven(self.direct_scanout));
        }
        if self.layers.len() != 1 {
            return Err(LiveDirectScanoutRefusal::LayerCount(self.layers.len()));
        }
        let LiveOwnedMixedCompositionLayer::DmaBuf {
            image_id,
            frame,
            placement,
        } = &self.layers[0]
        else {
            return Err(LiveDirectScanoutRefusal::LayerNotDmaBuf);
        };
        if placement.transform != Transform::IDENTITY {
            return Err(LiveDirectScanoutRefusal::LayerTransformed);
        }
        if placement.sampling != sophia_engine::HeadSamplingClass::Exact {
            return Err(LiveDirectScanoutRefusal::LayerResampled);
        }
        if placement.alpha < 1.0 {
            return Err(LiveDirectScanoutRefusal::LayerTranslucent);
        }
        let head = Rect {
            x: 0,
            y: 0,
            width: head_size.width,
            height: head_size.height,
        };
        // Split for the same reason Engine's verdict is: "not the head" is
        // true of a layer at the wrong place and a layer of the wrong size,
        // and the two send you to different fixes.
        if placement.target.x != 0 || placement.target.y != 0 {
            return Err(LiveDirectScanoutRefusal::LayerOffset);
        }
        if placement.target.width != head.width || placement.target.height != head.height {
            return Err(LiveDirectScanoutRefusal::LayerNotHeadSized);
        }
        if placement.clip.is_some_and(|clip| clip != head) {
            return Err(LiveDirectScanoutRefusal::LayerClipped);
        }
        if frame.width != head_size.width.max(0) as u32
            || frame.height != head_size.height.max(0) as u32
        {
            return Err(LiveDirectScanoutRefusal::BufferSizeMismatch);
        }
        // XRGB always; ARGB only here, where the checks above have already
        // established that this layer covers the head exactly.
        //
        // Alpha is part of the image when something is behind it. On a primary
        // plane filling the whole head nothing is, so it cannot change what is
        // displayed -- and the driver still decides, because the atomic test
        // runs on the framebuffer this descriptor produces. Offering it is not
        // assuming it.
        //
        // Refusing ARGB outright left this row with no client on this machine:
        // Kitty allocates ARGB whatever its background opacity says, and it is
        // the one client here known to hand over a DMA-BUF at all.
        if frame.format != crate::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888
            && frame.format != crate::LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888
        {
            return Err(LiveDirectScanoutRefusal::FormatNotOpaque(frame.format));
        }
        let plane_count = frame.plane_count;
        if plane_count == 0 || plane_count as usize > crate::LIVE_RENDERER_SCANOUT_MAX_PLANES {
            return Err(LiveDirectScanoutRefusal::PlaneLayoutUnusable);
        }
        let mut pitches = [0u32; 4];
        let mut offsets = [0u32; 4];
        for index in 0..plane_count as usize {
            let Some(plane) = frame.planes[index].as_ref() else {
                return Err(LiveDirectScanoutRefusal::PlaneLayoutUnusable);
            };
            pitches[index] = plane.stride;
            offsets[index] = plane.offset;
        }
        let descriptor = crate::LiveRendererScanoutBufferDescriptor::for_imported_dma_buf_planes(
            head_size,
            frame.format,
            plane_count,
            pitches,
            offsets,
            Some(frame.modifier),
        );
        if !descriptor.is_valid_scanout_buffer() {
            return Err(LiveDirectScanoutRefusal::PlaneLayoutUnusable);
        }
        let cloned = frame
            .try_clone()
            .map_err(|_| LiveDirectScanoutRefusal::PlaneFdCloneFailed)?;
        Ok(LiveDirectScanoutBuffer {
            descriptor,
            planes: cloned.planes,
            image_id: *image_id,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveMixedCompositionError {
    InvalidOutput,
    InvalidLayer,
    UnsupportedTransform,
    Renderer(LiveRendererScanoutBufferExportDetail),
}

#[derive(Clone, Copy, Debug)]
pub struct LiveDmaBufFrame<'a> {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub modifier: u64,
    pub fd: BorrowedFd<'a>,
    pub offset: u32,
    pub stride: u32,
}

impl LiveOwnedDmaBufFrame {
    pub fn as_frame(&self) -> LiveDmaBufFrame<'_> {
        LiveDmaBufFrame {
            width: self.width,
            height: self.height,
            format: self.format,
            modifier: self.modifier,
            fd: self.fd.as_fd(),
            offset: self.offset,
            stride: self.stride,
        }
    }

    pub fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self {
            width: self.width,
            height: self.height,
            format: self.format,
            modifier: self.modifier,
            fd: self.fd.try_clone()?,
            offset: self.offset,
            stride: self.stride,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveNativePersistentRenderStats {
    pub target_creations: usize,
    pub target_recreations: usize,
    pub gl_pipeline_creations: usize,
    pub frame_surface_creations: usize,
    pub cpu_target_creations: usize,
    pub dmabuf_target_creations: usize,
    pub composition_target_creations: usize,
    pub composition_target_reuses: usize,
    pub generation_replacements: usize,
    pub recovery_replacements: usize,
    pub frame_uploads: usize,
    pub snapshot_captures: usize,
    pub snapshot_promotions: usize,
    pub snapshot_rollbacks: usize,
    pub snapshot_evictions: usize,
    pub snapshot_live_entries: usize,
    pub snapshot_live_bytes: u64,
    pub import_cache: LiveNativeDmaBufImportCacheStats,
    pub exact_nearest_draws: usize,
    pub sharp_downscale_draws: usize,
    pub sharp_upscale_draws: usize,
    pub linear_fallback_draws: usize,
    pub max_target_create: std::time::Duration,
    pub max_frame_surface_create: std::time::Duration,
    pub max_render: std::time::Duration,
    pub max_upload: std::time::Duration,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveNativeDmaBufImportCacheStats {
    pub imports: usize,
    pub hits: usize,
    pub evictions: usize,
    pub live_entries: usize,
    pub descriptor_mismatches: usize,
    pub capacity_rejections: usize,
}

include!("native_scanout/context.rs");
include!("native_scanout/image_custody.rs");
include!("native_scanout/cpu_export.rs");
include!("native_scanout/dmabuf_export.rs");
include!("native_scanout/mixed_export.rs");
include!("native_scanout/owned_mixed_export.rs");
include!("native_scanout/backend_export.rs");

fn validate_placement(
    placement: LiveCompositionPlacement,
) -> Result<(), LiveMixedCompositionError> {
    if placement.transform != Transform::IDENTITY {
        return Err(LiveMixedCompositionError::UnsupportedTransform);
    }
    if placement.target.is_empty() || !placement.alpha.is_finite() {
        return Err(LiveMixedCompositionError::InvalidLayer);
    }
    Ok(())
}

const fn native_rect(rect: Rect) -> sophia_renderer_native_egl::NativeCompositionRect {
    sophia_renderer_native_egl::NativeCompositionRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

const fn native_sampling(
    sampling: HeadSamplingClass,
) -> sophia_renderer_native_egl::NativeCompositionSampling {
    match sampling {
        HeadSamplingClass::Exact => {
            sophia_renderer_native_egl::NativeCompositionSampling::ExactNearest
        }
        HeadSamplingClass::Downsampled => {
            sophia_renderer_native_egl::NativeCompositionSampling::SharpDownscale
        }
        HeadSamplingClass::Upsampled => {
            sophia_renderer_native_egl::NativeCompositionSampling::SharpUpscale
        }
        HeadSamplingClass::Mixed => {
            sophia_renderer_native_egl::NativeCompositionSampling::SharpMixed
        }
    }
}

pub struct NativeGbmRenderedScanoutContextReport<T: AsFd> {
    pub status: NativeGbmRenderedScanoutContextStatus,
    pub context: Option<NativeGbmRenderedScanoutContext<T>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeGbmScanoutBufferExporter;

impl NativeGbmScanoutBufferExporter {
    pub fn export_owned_scanout_buffer_from_backend_device_result<T: AsFd>(
        device: std::io::Result<T>,
        target: LiveGbmEglFrameTargetRecord,
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        export_native_owned_scanout_buffer_from_backend_device_result(
            device,
            target,
            sophia_renderer_native_egl::export_gbm_scanout_buffer_from_backend_device_result,
        )
    }

    pub fn export_rendered_owned_scanout_buffer_from_backend_device_result<T: AsFd>(
        device: std::io::Result<T>,
        target: LiveGbmEglFrameTargetRecord,
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        export_native_owned_scanout_buffer_from_backend_device_result(
            device,
            target,
            sophia_renderer_native_egl::export_rendered_gbm_scanout_buffer_from_backend_device_result,
        )
    }

    pub fn export_direct_cpu_owned_scanout_buffer_from_backend_device_result<T: AsFd>(
        device: std::io::Result<T>,
        target: LiveGbmEglFrameTargetRecord,
        frame: &crate::LiveCpuComposedFrame,
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        if !target.is_valid_scanout_target()
            || frame.size != target.size
            || frame.format != crate::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888
        {
            return NativeGbmOwnedScanoutBufferExportReport::new(
                LiveRendererScanoutBufferExportStatus::InvalidTarget,
                LiveRendererScanoutBufferExportDetail::InvalidTarget,
                None,
            );
        }
        reduced_native_owned_scanout_buffer_export_report(
            sophia_renderer_native_egl::export_direct_cpu_xrgb8888_gbm_scanout_buffer_from_backend_device_result(
                device,
                target.size.width as u32,
                target.size.height as u32,
                frame.stride,
                &frame.bytes,
            ),
        )
    }
}

fn export_native_owned_scanout_buffer_from_backend_device_result<T, F>(
    device: std::io::Result<T>,
    target: LiveGbmEglFrameTargetRecord,
    export: F,
) -> NativeGbmOwnedScanoutBufferExportReport
where
    T: AsFd,
    F: FnOnce(
        std::io::Result<T>,
        u32,
        u32,
    ) -> sophia_renderer_native_egl::NativeGbmOwnedScanoutBufferExportReport,
{
    if !target.is_valid_scanout_target() {
        return NativeGbmOwnedScanoutBufferExportReport::new(
            LiveRendererScanoutBufferExportStatus::InvalidTarget,
            LiveRendererScanoutBufferExportDetail::InvalidTarget,
            None,
        );
    }

    let report = export(device, target.size.width as u32, target.size.height as u32);
    reduced_native_owned_scanout_buffer_export_report(report)
}

fn reduced_native_owned_scanout_buffer_export_report(
    report: sophia_renderer_native_egl::NativeGbmOwnedScanoutBufferExportReport,
) -> NativeGbmOwnedScanoutBufferExportReport {
    let status = match report.status {
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportStatus::Exported => {
            LiveRendererScanoutBufferExportStatus::Exported
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportStatus::InvalidTarget => {
            LiveRendererScanoutBufferExportStatus::InvalidTarget
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportStatus::Unavailable => {
            LiveRendererScanoutBufferExportStatus::Unavailable
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportStatus::Degraded => {
            LiveRendererScanoutBufferExportStatus::Degraded
        }
    };

    let buffer = report.buffer.and_then(|buffer| {
        let descriptor = LiveRendererScanoutBufferDescriptor::new_with_planes(
            Size {
                width: buffer.width() as i32,
                height: buffer.height() as i32,
            },
            buffer.pitch(),
            buffer.format(),
            buffer.gem_handle(),
            LiveRendererScanoutBufferPlanes {
                count: buffer.plane_count(),
                handles: buffer.plane_handles(),
                pitches: buffer.plane_pitches(),
                offsets: buffer.plane_offsets(),
                modifier: buffer.modifier(),
            },
        );
        descriptor
            .is_valid_scanout_buffer()
            .then_some(NativeGbmOwnedScanoutBuffer {
                descriptor,
                _buffer: buffer,
            })
    });
    let mut reduced = NativeGbmOwnedScanoutBufferExportReport::new(
        status,
        reduced_native_owned_scanout_buffer_export_detail(report.detail),
        buffer,
    );
    reduced.buffer_age = report.buffer_age;
    reduced.target_generation = report.target_generation;
    reduced.repaint = match report.repaint {
        sophia_renderer_native_egl::NativeCompositionRepaintOutcome::Full => {
            LiveNativeCompositionRepaintOutcome::Full
        }
        sophia_renderer_native_egl::NativeCompositionRepaintOutcome::Partial { rects } => {
            LiveNativeCompositionRepaintOutcome::Partial { rects }
        }
    };
    reduced
}

fn reduced_native_owned_scanout_buffer_export_detail(
    detail: sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail,
) -> LiveRendererScanoutBufferExportDetail {
    match detail {
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::Exported => {
            LiveRendererScanoutBufferExportDetail::Exported
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::InvalidTarget => {
            LiveRendererScanoutBufferExportDetail::InvalidTarget
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::BackendDeviceUnavailable => {
            LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::GbmDeviceUnavailable => {
            LiveRendererScanoutBufferExportDetail::GbmDeviceUnavailable
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglUnavailable => {
            LiveRendererScanoutBufferExportDetail::EglUnavailable
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglDisplayUnavailable => {
            LiveRendererScanoutBufferExportDetail::EglDisplayUnavailable
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglInitializeFailed => {
            LiveRendererScanoutBufferExportDetail::EglInitializeFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglBindApiFailed => {
            LiveRendererScanoutBufferExportDetail::EglBindApiFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglConfigUnavailable => {
            LiveRendererScanoutBufferExportDetail::EglConfigUnavailable
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::GbmSurfaceUnavailable => {
            LiveRendererScanoutBufferExportDetail::GbmSurfaceUnavailable
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglSurfaceUnavailable => {
            LiveRendererScanoutBufferExportDetail::EglSurfaceUnavailable
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglContextUnavailable => {
            LiveRendererScanoutBufferExportDetail::EglContextUnavailable
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglMakeCurrentFailed => {
            LiveRendererScanoutBufferExportDetail::EglMakeCurrentFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::GlSmokeFailed => {
            LiveRendererScanoutBufferExportDetail::GlSmokeFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::CpuLayerUploadFailed => {
            LiveRendererScanoutBufferExportDetail::CpuLayerUploadFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::DmaBufImageCreateFailed => {
            LiveRendererScanoutBufferExportDetail::DmaBufImageCreateFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::DmaBufImageBindFailed => {
            LiveRendererScanoutBufferExportDetail::DmaBufImageBindFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::CompositionDrawFailed => {
            LiveRendererScanoutBufferExportDetail::CompositionDrawFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::CompositionFinishFailed => {
            LiveRendererScanoutBufferExportDetail::CompositionFinishFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed => {
            LiveRendererScanoutBufferExportDetail::EglImageDestroyFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::DmaBufImportFailed => {
            LiveRendererScanoutBufferExportDetail::DmaBufImportFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::EglSwapBuffersFailed => {
            LiveRendererScanoutBufferExportDetail::EglSwapBuffersFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::FrontBufferLockFailed => {
            LiveRendererScanoutBufferExportDetail::FrontBufferLockFailed
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor => {
            LiveRendererScanoutBufferExportDetail::InvalidBufferDescriptor
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::InvalidRendererImageId => {
            LiveRendererScanoutBufferExportDetail::InvalidRendererImageId
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::DmaBufDescriptorMismatch => {
            LiveRendererScanoutBufferExportDetail::DmaBufDescriptorMismatch
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::DmaBufImportCacheFull => {
            LiveRendererScanoutBufferExportDetail::DmaBufImportCacheFull
        }
        sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail::RendererImageStoreFull => {
            LiveRendererScanoutBufferExportDetail::RendererImageStoreFull
        }
    }
}
