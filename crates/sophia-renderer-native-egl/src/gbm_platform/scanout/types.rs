use std::os::fd::BorrowedFd;
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

pub use sophia_renderer_image::{
    DmaBufPlane as NativeDmaBufPlane, MultiPlaneDmaBufFrame as NativeMultiPlaneDmaBufFrame,
    OwnedDmaBufPlane as NativeOwnedDmaBufPlane, OwnedSnapshotData as NativeOwnedSnapshotData,
    RendererImageSnapshot as NativeRendererImageSnapshot,
};

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
    /// Cold capture selector calls; a retained context carries its exact config key.
    pub capture_config_selections: u64,
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
    /// Pooled-capture source import, nested in and already counted by
    /// `capture_copy_*`; never add it to the setup/copy/cleanup partition.
    pub capture_import_cpu: std::time::Duration,
    pub capture_import_elapsed: std::time::Duration,
    /// Pooled-capture source imports destroyed, by eviction or after a
    /// transient use, and the subset of imports that were transient.
    pub capture_source_evictions: usize,
    pub capture_source_transient_imports: usize,
    /// Capture destination allocation attempts, counted whether or not
    /// timing is enabled (unlike import-cache `texture_calls`, which counts
    /// timed calls only), and their time when enabled. Nested in
    /// `capture_setup_*` for captures; admission preparation is outside every
    /// capture span. Never add to the setup/copy/cleanup partition.
    /// Physical pool gauges; peak includes all pooled capture paths in this context.
    pub capture_pool_live_count: usize,
    pub capture_pool_live_bytes: u64,
    pub capture_pool_peak_bytes: u64,
    pub capture_pool_idle_count: usize,
    pub capture_pool_idle_bytes: u64,
    pub capture_managed_images: usize,
    pub capture_pending_fences: usize,
    pub capture_prepared_destinations: usize,
    pub capture_destination_allocations: usize,
    pub capture_destination_allocation_cpu: std::time::Duration,
    pub capture_destination_allocation_elapsed: std::time::Duration,
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
