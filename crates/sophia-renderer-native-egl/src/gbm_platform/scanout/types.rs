use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::time::Duration;

use super::super::{NativeCompositionDamageStats, NativeCompositionRepaintTable};
use super::import_cache::{NativeDmaBufImportCacheStats, NativeRendererImageId};
use crate::gl::GlCompositionRect;

#[derive(Clone, Copy, Debug)]
pub struct NativeDmaBufFrame<'a> {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub modifier: u64,
    pub fd: BorrowedFd<'a>,
    pub offset: u32,
    pub stride: u32,
}

impl NativeDmaBufFrame<'_> {
    pub fn is_valid(&self) -> bool {
        const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;
        const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;
        self.width > 0
            && self.height > 0
            && matches!(self.format, DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888)
            && self.stride >= self.width.saturating_mul(4)
            && (self.modifier == 0 || self.modifier == u64::from(gbm::Modifier::Invalid))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NativeDmaBufPlane<'a> {
    pub fd: BorrowedFd<'a>,
    pub offset: u32,
    pub stride: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeMultiPlaneDmaBufFrame<'a> {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub modifier: u64,
    pub plane_count: u8,
    pub planes: [Option<NativeDmaBufPlane<'a>>; 4],
}

#[derive(Debug)]
pub struct NativeRendererImageSnapshot {
    // Duplicated plane FDs keep pixels alive without retaining the old DRM
    // device, EGL display, or GBM context across a renderer generation change.
    pub(super) image_id: NativeRendererImageId,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) format: u32,
    pub(super) modifier: u64,
    pub(super) plane_count: u8,
    pub(super) planes: [Option<NativeOwnedDmaBufPlane>; 4],
    pub(super) charge: Option<std::sync::Arc<super::snapshot_custody::SnapshotCharge>>,
    pub(super) epoch: Option<std::sync::Arc<super::NativeRendererSnapshotEpoch>>,
}

#[derive(Debug)]
pub struct NativeOwnedDmaBufPlane {
    pub(super) fd: OwnedFd,
    pub(super) offset: u32,
    pub(super) stride: u32,
}

impl NativeRendererImageSnapshot {
    /// Duplicate only descriptor ownership, preserving the original snapshot
    /// through a fallible replacement-renderer import.
    pub fn try_clone(&self) -> std::io::Result<Self> {
        let mut planes = std::array::from_fn(|_| None);
        for (destination, source) in planes.iter_mut().zip(&self.planes) {
            if let Some(source) = source {
                *destination = Some(NativeOwnedDmaBufPlane {
                    fd: source.fd.try_clone()?,
                    offset: source.offset,
                    stride: source.stride,
                });
            }
        }
        Ok(Self {
            image_id: self.image_id,
            width: self.width,
            height: self.height,
            format: self.format,
            modifier: self.modifier,
            plane_count: self.plane_count,
            planes,
            charge: self.charge.clone(),
            epoch: self.epoch.clone(),
        })
    }

    pub const fn image_id(&self) -> NativeRendererImageId {
        self.image_id
    }

    pub fn is_current(&self) -> bool {
        self.epoch.as_ref().is_none_or(|epoch| epoch.is_valid())
    }

    pub fn as_frame(&self) -> NativeMultiPlaneDmaBufFrame<'_> {
        NativeMultiPlaneDmaBufFrame {
            width: self.width,
            height: self.height,
            format: self.format,
            modifier: self.modifier,
            plane_count: self.plane_count,
            planes: std::array::from_fn(|index| {
                self.planes[index].as_ref().map(|plane| NativeDmaBufPlane {
                    fd: plane.fd.as_fd(),
                    offset: plane.offset,
                    stride: plane.stride,
                })
            }),
        }
    }
}

impl NativeMultiPlaneDmaBufFrame<'_> {
    pub fn is_valid(&self) -> bool {
        const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;
        const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;
        self.width > 0
            && self.height > 0
            && matches!(self.format, DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888)
            && self.plane_count > 0
            && usize::from(self.plane_count) <= self.planes.len()
            && self.planes[..usize::from(self.plane_count)]
                .iter()
                .all(Option::is_some)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCompositionRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl From<NativeCompositionRect> for GlCompositionRect {
    fn from(rect: NativeCompositionRect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NativeCpuCompositionLayer<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    pub pixels: &'a [u8],
    pub target: NativeCompositionRect,
    pub clip: Option<NativeCompositionRect>,
    pub alpha: f32,
    pub sampling: crate::NativeCompositionSampling,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeDmaBufCompositionLayer<'a> {
    pub image_id: NativeRendererImageId,
    pub frame: NativeMultiPlaneDmaBufFrame<'a>,
    /// Immutable foreign storage retained by both the import and the output.
    pub custody: Option<&'a std::sync::Arc<NativeRendererImageSnapshot>>,
    pub target: NativeCompositionRect,
    pub clip: Option<NativeCompositionRect>,
    pub alpha: f32,
    pub sampling: crate::NativeCompositionSampling,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeRendererImageCompositionLayer {
    pub image_id: NativeRendererImageId,
    pub target: NativeCompositionRect,
    pub clip: Option<NativeCompositionRect>,
    pub alpha: f32,
    pub sampling: crate::NativeCompositionSampling,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeSolidCompositionLayer {
    pub target: NativeCompositionRect,
    pub color: [u8; 3],
}

#[derive(Clone, Copy, Debug)]
pub enum NativeCompositionLayer<'a> {
    Cpu(NativeCpuCompositionLayer<'a>),
    DmaBuf(NativeDmaBufCompositionLayer<'a>),
    RendererImage(NativeRendererImageCompositionLayer),
    Solid(NativeSolidCompositionLayer),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCompositionFormatRequest {
    Required(u32),
    /// May use normal allocation order if format admission fails before drawing.
    Preferred(u32),
}

impl NativeCompositionFormatRequest {
    pub const fn fourcc(self) -> u32 {
        match self {
            Self::Required(format) | Self::Preferred(format) => format,
        }
    }
}

/// Allocation constraints for a composed output. An absent format keeps the
/// renderer's normal candidate order.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeCompositionOutputRequest<'a> {
    pub preferred_modifiers: &'a [u64],
    /// Only XR24 and AR24 are supported. Other requested fourccs are refused.
    pub format: Option<NativeCompositionFormatRequest>,
}

impl NativeCompositionOutputRequest<'_> {
    pub const fn is_valid(self) -> bool {
        match self.format {
            None => true,
            Some(request) => matches!(request.fourcc(), 0x3432_5258 | 0x3432_5241),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NativeCompositionFrame<'a> {
    pub width: u32,
    pub height: u32,
    pub layers: &'a [NativeCompositionLayer<'a>],
    pub trace: Option<NativeCompositionTrace>,
    /// What this frame may repaint instead of the whole target, indexed by the
    /// age its surface reports. The caller cannot select the entry itself: the
    /// age is only knowable after the context is current, which happens here.
    pub repaint: Option<&'a NativeCompositionRepaintTable>,
}

/// A damage rectangle in top-left output coordinates, as Engine reduces them.
/// The Y flip into GL's bottom-left space happens where the scissor is set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCompositionDamageRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeCompositionTrace {
    pub output: u64,
    pub head: u64,
    pub scene_generation: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeGbmPersistentRenderStats {
    pub transfer_captures: u64,
    pub transfer_attempts: u64,
    pub transfer_failures: u64,
    pub capture_setup_cpu: std::time::Duration,
    pub capture_copy_cpu: std::time::Duration,
    pub capture_cleanup_cpu: std::time::Duration,
    pub composition_cpu: std::time::Duration,

    pub capture_context_creations: u64,
    pub capture_context_reuses: u64,
    pub capture_surface_creations: u64,
    pub capture_failures: u64,
    pub composition_full_frames: u64,
    pub composition_partial_frames: u64,
    pub composition_repaint_pixels: u64,
    pub composition_target_pixels: u64,
    pub composition_damage: NativeCompositionDamageStats,
    pub capture_setup_elapsed: std::time::Duration,
    pub capture_copy_elapsed: std::time::Duration,
    pub capture_cleanup_elapsed: std::time::Duration,
    pub composition_elapsed: std::time::Duration,

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
    pub import_cache: NativeDmaBufImportCacheStats,
    pub sampling: crate::NativeCompositionSamplingStats,
    pub max_target_create: Duration,
    pub max_frame_surface_create: Duration,
    pub max_render: Duration,
    pub max_upload: Duration,
}

#[cfg(test)]
#[path = "../../../tests/support/image_snapshot_ownership.rs"]
mod ownership_tests;
