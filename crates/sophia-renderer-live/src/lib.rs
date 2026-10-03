//! Live renderer boundary.
//!
//! This crate is the future home for renderer-private resources such as GBM,
//! EGL, DMA-BUF import, explicit sync fences, and upload caches. Public types
//! stay reduced so backend-live can prove scanout behavior without leaking
//! native renderer identity into the engine.

pub use sophia_engine::BufferImportPath;
pub use sophia_protocol::{BufferSource, Size};
/// Passive damage records the worker fills in and the renderer consumes.
/// Re-exported here so backend-live keeps a single renderer dependency. Gated
/// with the renderer itself, which is optional.
#[cfg(feature = "gbm-probe")]
pub use sophia_renderer_native_egl::{
    NATIVE_IMAGE_IMPORT_DEVICE_CAPACITY, NativeCompositionDamageRect, NativeCompositionDamageStats,
    NativeCompositionRepaintTable, NativeFrameTargetSetId, NativeFullRepaintReason,
    NativeImageTransferStats, NativeRepaintPlan,
};
#[cfg(feature = "gbm-probe")]
pub use sophia_renderer_native_egl::{
    NativeDmaBufCapabilityError as LiveDmaBufCapabilityError,
    NativeDmaBufImportFormat as LiveDmaBufImportFormat,
    query_native_dmabuf_import_formats as query_dma_buf_import_formats,
};

mod buffer_registry;
mod cpu_buffer_registry;
mod cpu_composition;
mod cursor_theme;
mod frame_target;
mod import;
mod indicator_strip;
mod presentation;
mod production_cpu_scene;
mod renderer_image;
mod scanout_buffer;
mod text_raster;

#[cfg(feature = "egl-probe")]
mod egl_probe;
#[cfg(feature = "gbm-probe")]
mod gbm_probe;
#[cfg(feature = "gbm-probe")]
mod head_composition;
#[cfg(feature = "gbm-probe")]
mod native_scanout;
mod shared_buffer;
#[cfg(feature = "gbm-probe")]
mod shared_pixmap;

pub use buffer_registry::*;
pub use cpu_buffer_registry::*;
pub use cpu_composition::*;
pub use cursor_theme::*;
pub use frame_target::*;
pub use import::*;
pub use indicator_strip::*;
pub use presentation::*;
pub use production_cpu_scene::*;
pub use renderer_image::LiveRendererImageId;
pub use scanout_buffer::*;
pub use text_raster::*;

#[cfg(feature = "egl-probe")]
pub use egl_probe::{
    EglCapabilityProbeReport, EglCapabilityProbeStatus, EglContextProbeStatus, EglDrawSmokeReport,
    EglDrawSmokeStatus, EglPlatformStatus, FakeEglCapabilityProbe, FakeEglDrawSmoke,
    NativeEglCapabilityProbe, NativeEglDrawSmoke,
};
#[cfg(all(feature = "egl-probe", feature = "gbm-probe"))]
pub use egl_probe::{
    NativeGbmBackedEglDrawSmoke, NativeGbmBackedEglFrameTargetAllocator,
    NativeGbmBackedEglPlatformProbe, NativeGbmBackedEglPresentationSmoke,
};

#[cfg(feature = "gbm-probe")]
pub use gbm_probe::{
    FakeGbmCapabilityProbe, GbmCapabilityProbeReport, GbmCapabilityProbeStatus,
    GbmRenderDeviceToken, NativeGbmCapabilityProbe,
};
#[cfg(feature = "gbm-probe")]
pub use head_composition::*;
#[cfg(feature = "gbm-probe")]
pub use native_scanout::*;
#[cfg(feature = "gbm-probe")]
pub use shared_buffer::allocate_shared_buffer;
pub use shared_buffer::{LiveSharedBufferAllocation, LiveSharedBufferError};
#[cfg(feature = "gbm-probe")]
pub use shared_pixmap::*;

pub const LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888: u32 = 875_713_089;
pub const LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888: u32 = 875_713_112;
