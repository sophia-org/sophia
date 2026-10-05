use std::os::fd::{AsFd, AsRawFd};

use super::{
    NativeDmaBufCompositionLayer, NativeGbmScanoutBufferExportDetail, PersistentXrgb8888GlPipeline,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NativeRendererImageId(u64);

impl NativeRendererImageId {
    pub const INVALID: Self = Self(0);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeRendererImageCacheAdmission {
    Hit { slot: usize },
    Vacant { slot: usize },
    Full,
    Invalid,
}

pub fn native_renderer_image_cache_admission(
    image_ids: impl IntoIterator<Item = Option<NativeRendererImageId>>,
    image_id: NativeRendererImageId,
) -> NativeRendererImageCacheAdmission {
    if !image_id.is_valid() {
        return NativeRendererImageCacheAdmission::Invalid;
    }
    let mut vacant = None;
    for (slot, resident) in image_ids.into_iter().enumerate() {
        match resident {
            Some(resident) if resident == image_id => {
                return NativeRendererImageCacheAdmission::Hit { slot };
            }
            None if vacant.is_none() => vacant = Some(slot),
            Some(_) | None => {}
        }
    }
    vacant.map_or(NativeRendererImageCacheAdmission::Full, |slot| {
        NativeRendererImageCacheAdmission::Vacant { slot }
    })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeDmaBufImportCacheStats {
    pub imports: usize,
    pub hits: usize,
    pub evictions: usize,
    pub live_entries: usize,
    pub descriptor_mismatches: usize,
    pub capacity_rejections: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NativeDmaBufFingerprint {
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
    plane_count: u8,
    device: [u64; 4],
    inode: [u64; 4],
    offsets: [u32; 4],
    strides: [u32; 4],
}

impl NativeDmaBufFingerprint {
    fn from_layer(
        layer: NativeDmaBufCompositionLayer<'_>,
    ) -> Result<Self, NativeGbmScanoutBufferExportDetail> {
        let mut device = [0; 4];
        let mut inode = [0; 4];
        let mut offsets = [0; 4];
        let mut strides = [0; 4];
        for (index, plane) in layer
            .frame
            .planes
            .iter()
            .copied()
            .enumerate()
            .take(usize::from(layer.frame.plane_count))
        {
            let plane = plane.ok_or(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?;
            let stat = rustix::fs::fstat(plane.fd.as_fd())
                .map_err(|_| NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?;
            device[index] = stat.st_dev;
            inode[index] = stat.st_ino;
            offsets[index] = plane.offset;
            strides[index] = plane.stride;
        }
        Ok(Self {
            width: layer.frame.width,
            height: layer.frame.height,
            format: layer.frame.format,
            modifier: layer.frame.modifier,
            plane_count: layer.frame.plane_count,
            device,
            inode,
            offsets,
            strides,
        })
    }
}

struct NativeDmaBufImport {
    image_id: NativeRendererImageId,
    fingerprint: NativeDmaBufFingerprint,
    image: khronos_egl::Image,
    texture: glow::NativeTexture,
    _custody: Option<std::sync::Arc<super::NativeRendererImageSnapshot>>,
}

pub(crate) struct NativeDmaBufImportCache {
    entries: Vec<Option<NativeDmaBufImport>>,
    snapshot_entries: std::collections::VecDeque<NativeSnapshotImport>,
    snapshot_cleanup_failed: bool,
    stats: NativeDmaBufImportCacheStats,
}

impl NativeDmaBufImportCache {
    pub(crate) fn with_capacity_and_stats(
        capacity: usize,
        stats: NativeDmaBufImportCacheStats,
    ) -> Self {
        Self {
            entries: std::iter::repeat_with(|| None).take(capacity).collect(),
            snapshot_entries: std::collections::VecDeque::new(),
            snapshot_cleanup_failed: false,
            stats,
        }
    }

    pub(crate) const fn stats(&self) -> NativeDmaBufImportCacheStats {
        self.stats
    }

    pub(crate) fn texture(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
        layer: NativeDmaBufCompositionLayer<'_>,
    ) -> Result<glow::NativeTexture, NativeGbmScanoutBufferExportDetail> {
        if self.snapshot_cleanup_failed {
            return Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed);
        }
        // One immutable image can have copies in several stores. A snapshot
        // from a replacement donor is a new backing even when its image ID
        // still names the same pixels. Old custody never authenticates it.
        let replace = self.entries.iter().flatten().any(|entry| {
            entry.image_id == layer.image_id
                && entry._custody.as_ref().is_some_and(|old| {
                    !old.import_cacheable()
                        || layer
                            .custody
                            .is_none_or(|new| !std::sync::Arc::ptr_eq(old, new))
                })
        });
        if replace {
            self.evict(egl, display, pipeline, layer.image_id)?;
        }
        let fingerprint = NativeDmaBufFingerprint::from_layer(layer)?;
        let admission = native_renderer_image_cache_admission(
            self.entries
                .iter()
                .map(|entry| entry.as_ref().map(|entry| entry.image_id)),
            layer.image_id,
        );
        if let NativeRendererImageCacheAdmission::Hit { slot } = admission {
            let entry = self.entries[slot]
                .as_ref()
                .expect("DMA-BUF cache hit slot must be occupied");
            if entry.fingerprint != fingerprint {
                self.stats.descriptor_mismatches =
                    self.stats.descriptor_mismatches.saturating_add(1);
                return Err(NativeGbmScanoutBufferExportDetail::DmaBufDescriptorMismatch);
            }
            self.stats.hits = self.stats.hits.saturating_add(1);
            return Ok(entry.texture);
        }
        let slot = match admission {
            NativeRendererImageCacheAdmission::Vacant { slot } => slot,
            NativeRendererImageCacheAdmission::Full => {
                self.stats.capacity_rejections = self.stats.capacity_rejections.saturating_add(1);
                return Err(NativeGbmScanoutBufferExportDetail::DmaBufImportCacheFull);
            }
            NativeRendererImageCacheAdmission::Invalid => {
                return Err(NativeGbmScanoutBufferExportDetail::InvalidRendererImageId);
            }
            NativeRendererImageCacheAdmission::Hit { .. } => unreachable!(),
        };
        self.make_snapshot_room(egl, display, pipeline)?;
        let image = create_dma_buf_image(egl, display, layer.frame)?;
        let texture = match unsafe { pipeline.create_egl_image_texture(egl, image.as_ptr()) } {
            Ok(texture) => texture,
            Err(error) => {
                let _ = egl.destroy_image(display, image);
                return Err(error);
            }
        };
        self.entries[slot] = Some(NativeDmaBufImport {
            image_id: layer.image_id,
            fingerprint,
            image,
            texture,
            _custody: layer.custody.cloned(),
        });
        self.stats.imports = self.stats.imports.saturating_add(1);
        self.stats.live_entries = self.stats.live_entries.saturating_add(1);
        Ok(texture)
    }

    pub(crate) fn evict(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
        image_id: NativeRendererImageId,
    ) -> Result<bool, NativeGbmScanoutBufferExportDetail> {
        let snapshot_evicted = self.evict_snapshot_imports(egl, display, pipeline, image_id)?;
        let Some(index) = self.entries.iter().position(|entry| {
            entry
                .as_ref()
                .is_some_and(|entry| entry.image_id == image_id)
        }) else {
            return Ok(snapshot_evicted);
        };
        let entry = self.entries[index]
            .take()
            .expect("DMA-BUF cache entry index was checked above");
        unsafe { pipeline.delete_texture(entry.texture) };
        let image_destroyed = egl.destroy_image(display, entry.image).is_ok();
        self.stats.evictions = self.stats.evictions.saturating_add(1);
        self.stats.live_entries = self.stats.live_entries.saturating_sub(1);
        if image_destroyed {
            Ok(true)
        } else {
            Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed)
        }
    }

    pub(crate) fn clear(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
        pipeline: &PersistentXrgb8888GlPipeline,
    ) -> Result<usize, NativeGbmScanoutBufferExportDetail> {
        let mut cleared = 0usize;
        let mut image_destroy_failed = false;
        while let Some(entry) = self.snapshot_entries.pop_front() {
            image_destroy_failed |= self
                .destroy_snapshot_import(entry, egl, display, pipeline)
                .is_err();
            cleared += 1;
        }
        for entry in &mut self.entries {
            let Some(entry) = entry.take() else {
                continue;
            };
            unsafe { pipeline.delete_texture(entry.texture) };
            image_destroy_failed |= egl.destroy_image(display, entry.image).is_err();
            cleared = cleared.saturating_add(1);
        }
        self.stats.evictions = self.stats.evictions.saturating_add(cleared);
        self.stats.live_entries = 0;
        if image_destroy_failed {
            Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed)
        } else {
            Ok(cleared)
        }
    }

    pub(crate) fn abandon(
        &mut self,
        egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
        display: khronos_egl::Display,
    ) {
        let mut cleared = 0usize;
        for entry in self.snapshot_entries.drain(..) {
            // With no current context the texture was not deleted. Even an
            // EGLImage success would not prove its sibling released storage.
            entry.allocation.quarantine_output_image(entry.image);
            self.snapshot_cleanup_failed = true;
            cleared += 1;
        }
        for entry in &mut self.entries {
            let Some(entry) = entry.take() else {
                continue;
            };
            let _ = egl.destroy_image(display, entry.image);
            cleared = cleared.saturating_add(1);
        }
        self.stats.evictions = self.stats.evictions.saturating_add(cleared);
        self.stats.live_entries = 0;
    }
}

include!("import_cache/snapshots.rs");

pub(crate) fn create_dma_buf_image(
    egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    frame: super::NativeMultiPlaneDmaBufFrame<'_>,
) -> Result<khronos_egl::Image, NativeGbmScanoutBufferExportDetail> {
    use std::{ffi::c_void, ptr};

    const EGL_LINUX_DMA_BUF_EXT: khronos_egl::Enum = 0x3270;
    const EGL_WIDTH: khronos_egl::Attrib = 0x3057;
    const EGL_HEIGHT: khronos_egl::Attrib = 0x3056;
    const EGL_LINUX_DRM_FOURCC_EXT: khronos_egl::Attrib = 0x3271;
    const PLANE_ATTRIBUTES: [[khronos_egl::Attrib; 5]; 4] = [
        [0x3272, 0x3273, 0x3274, 0x3443, 0x3444],
        [0x3275, 0x3276, 0x3277, 0x3445, 0x3446],
        [0x3278, 0x3279, 0x327A, 0x3447, 0x3448],
        [0x3440, 0x3441, 0x3442, 0x3449, 0x344A],
    ];
    let mut attributes = vec![
        EGL_WIDTH,
        frame.width as khronos_egl::Attrib,
        EGL_HEIGHT,
        frame.height as khronos_egl::Attrib,
        EGL_LINUX_DRM_FOURCC_EXT,
        frame.format as khronos_egl::Attrib,
    ];
    for (index, keys) in PLANE_ATTRIBUTES
        .iter()
        .copied()
        .enumerate()
        .take(usize::from(frame.plane_count))
    {
        let plane = frame.planes[index]
            .ok_or(NativeGbmScanoutBufferExportDetail::InvalidBufferDescriptor)?;
        attributes.extend_from_slice(&[
            keys[0],
            plane.fd.as_raw_fd() as khronos_egl::Attrib,
            keys[1],
            plane.offset as khronos_egl::Attrib,
            keys[2],
            plane.stride as khronos_egl::Attrib,
        ]);
        if frame.modifier != u64::from(gbm::Modifier::Invalid) {
            attributes.extend_from_slice(&[
                keys[3],
                (frame.modifier & u64::from(u32::MAX)) as khronos_egl::Attrib,
                keys[4],
                (frame.modifier >> 32) as khronos_egl::Attrib,
            ]);
        }
    }
    attributes.push(khronos_egl::ATTRIB_NONE);
    let no_context = unsafe { khronos_egl::Context::from_ptr(khronos_egl::NO_CONTEXT) };
    let no_buffer = unsafe { khronos_egl::ClientBuffer::from_ptr(ptr::null_mut::<c_void>()) };
    egl.create_image(
        display,
        no_context,
        EGL_LINUX_DMA_BUF_EXT,
        no_buffer,
        &attributes,
    )
    .map_err(|_| NativeGbmScanoutBufferExportDetail::DmaBufImageCreateFailed)
}
