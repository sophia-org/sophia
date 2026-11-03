use super::*;

const MAX_TEXTURES: usize = 32;
const MAX_BYTES: usize = 64 * 1024 * 1024;

struct Entry {
    texture: glow::NativeTexture,
    width: u32,
    height: u32,
    version: Option<std::num::NonZeroU64>,
    format: u32,
    frame_layer: Option<usize>,
}

#[derive(Default)]
pub(super) struct CpuTextureCache {
    entries: Vec<Entry>,
}

impl PersistentXrgb8888GlPipeline {
    pub(crate) fn cpu_upload_stats(&self) -> crate::NativeCpuUploadStats {
        let mut stats = self.cpu_upload_stats.get();
        stats.retained_bytes = self
            .cpu_cache
            .borrow()
            .entries
            .iter()
            .map(|e| u64::from(e.width) * u64::from(e.height) * 4)
            .sum();
        stats
    }
    pub(super) fn record_cpu_upload(&self, bytes: usize, allocation: bool) {
        let mut stats = self.cpu_upload_stats.get();
        stats.uploads = stats.uploads.saturating_add(1);
        stats.bytes = stats.bytes.saturating_add(bytes as u64);
        stats.allocations = stats.allocations.saturating_add(u64::from(allocation));
        self.cpu_upload_stats.set(stats);
    }
    pub(crate) fn begin_cpu_composition(&self) {
        self.cpu_cache.borrow_mut().entries.retain(|entry| {
            if entry.frame_layer.is_none() {
                unsafe {
                    self.delete_texture(entry.texture);
                }
                false
            } else {
                true
            }
        });
        for entry in &mut self.cpu_cache.borrow_mut().entries {
            entry.frame_layer = None;
        }
    }

    pub(crate) fn invalidate_cpu_composition(&self) {
        for entry in &mut self.cpu_cache.borrow_mut().entries {
            entry.version = None;
            entry.frame_layer = None;
        }
    }

    pub(crate) fn draw_cached_cpu_layer(
        &self,
        index: usize,
        format: u32,
        version: Option<std::num::NonZeroU64>,
        layer: GlCpuLayer<'_>,
        target: GlCompositionRect,
        clip: Option<GlCompositionRect>,
        sampling: NativeCompositionSampling,
        trace: Option<NativeCompositionTrace>,
    ) -> Result<(), NativeEglDrawSmokeStatus> {
        let bytes = usize::try_from(layer.width)
            .ok()
            .and_then(|w| w.checked_mul(layer.height as usize))
            .and_then(|n| n.checked_mul(4))
            .ok_or(NativeEglDrawSmokeStatus::GlUnavailable)?;
        if layer.width == 0
            || layer.height == 0
            || layer.width.checked_mul(4) != Some(layer.stride)
            || bytes != layer.pixels.len()
        {
            return Err(NativeEglDrawSmokeStatus::GlUnavailable);
        }
        // Clipping must precede upload: a damage pass need not touch this layer.
        let mut visible = intersect_composition_rects(
            target,
            GlCompositionRect {
                x: 0,
                y: 0,
                width: self.width as i32,
                height: self.height as i32,
            },
        );
        if let Some(clip) = clip {
            visible = intersect_composition_rects(visible, clip);
        }
        if let Some(clip) = self.ambient_clip.get() {
            visible = intersect_composition_rects(visible, clip);
        }
        if visible.width <= 0 || visible.height <= 0 {
            return Ok(());
        }
        let mut cache = self.cpu_cache.borrow_mut();
        let hit = cache.entries.iter().position(|e| {
            e.width == layer.width
                && e.height == layer.height
                && e.format == format
                && (e.frame_layer == Some(index) || (version.is_some() && e.version == version))
        });
        let texture = if let Some(hit) = hit {
            let mut stats = self.cpu_upload_stats.get();
            stats.hits = stats.hits.saturating_add(1);
            self.cpu_upload_stats.set(stats);
            let entry = &mut cache.entries[hit];
            entry.frame_layer = Some(index);
            entry.texture
        } else {
            // Never evict a texture already drawn in this composition: later damage passes reuse it.
            let reuse = cache.entries.iter().position(|e| {
                e.frame_layer.is_none() && e.width == layer.width && e.height == layer.height
            });
            let retained: usize = cache
                .entries
                .iter()
                .map(|e| e.width as usize * e.height as usize * 4)
                .sum();
            if reuse.is_none()
                && (cache.entries.len() >= MAX_TEXTURES
                    || bytes > MAX_BYTES.saturating_sub(retained))
            {
                drop(cache);
                return self.draw_cpu_layer(layer, target, clip, sampling, trace);
            }
            let slot = if let Some(slot) = reuse {
                slot
            } else {
                let texture = unsafe { self.gl.create_texture() }
                    .map_err(|_| NativeEglDrawSmokeStatus::GlUnavailable)?;
                cache.entries.push(Entry {
                    texture,
                    width: 0,
                    height: 0,
                    version: None,
                    format,
                    frame_layer: None,
                });
                cache.entries.len() - 1
            };
            let entry = &mut cache.entries[slot];
            let reallocate = entry.width == 0;
            self.record_cpu_upload(bytes, reallocate);
            unsafe {
                self.gl.active_texture(glow::TEXTURE0);
                self.gl.bind_texture(glow::TEXTURE_2D, Some(entry.texture));
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_S,
                    glow::CLAMP_TO_EDGE as i32,
                );
                self.gl.tex_parameter_i32(
                    glow::TEXTURE_2D,
                    glow::TEXTURE_WRAP_T,
                    glow::CLAMP_TO_EDGE as i32,
                );
                if reallocate {
                    self.gl.tex_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        glow::RGBA as i32,
                        layer.width as i32,
                        layer.height as i32,
                        0,
                        glow::BGRA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(layer.pixels)),
                    );
                } else {
                    self.gl.tex_sub_image_2d(
                        glow::TEXTURE_2D,
                        0,
                        0,
                        0,
                        layer.width as i32,
                        layer.height as i32,
                        glow::BGRA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelUnpackData::Slice(Some(layer.pixels)),
                    );
                }
            }
            entry.width = layer.width;
            entry.height = layer.height;
            entry.version = version;
            entry.format = format;
            entry.frame_layer = Some(index);
            entry.texture
        };
        drop(cache);
        self.draw_texture_layer(
            texture,
            (layer.width, layer.height),
            target,
            clip,
            layer.alpha,
            layer.alpha_mode,
            sampling,
            trace,
        )
    }
}

impl PersistentXrgb8888GlPipeline {
    pub(crate) fn draw_cpu_layer(
        &self,
        layer: GlCpuLayer<'_>,
        target: GlCompositionRect,
        clip: Option<GlCompositionRect>,
        sampling: NativeCompositionSampling,
        trace: Option<NativeCompositionTrace>,
    ) -> Result<(), NativeEglDrawSmokeStatus> {
        let GlCpuLayer {
            width,
            height,
            stride,
            pixels,
            alpha,
            alpha_mode,
        } = layer;
        let expected_stride = width
            .checked_mul(4)
            .ok_or(NativeEglDrawSmokeStatus::GlUnavailable)?;
        let expected = usize::try_from(stride)
            .ok()
            .and_then(|stride| stride.checked_mul(usize::try_from(height).ok()?))
            .ok_or(NativeEglDrawSmokeStatus::GlUnavailable)?;
        if width == 0 || height == 0 || stride != expected_stride || pixels.len() != expected {
            return Err(NativeEglDrawSmokeStatus::GlUnavailable);
        }
        let upload = native_cpu_texture_upload(
            self.cpu_layer_texture_width.get(),
            self.cpu_layer_texture_height.get(),
            width,
            height,
        );
        self.record_cpu_upload(pixels.len(), upload == NativeCpuTextureUpload::Reallocate);
        // Per draw, not once at creation: filter state lives on the texture object
        // and this one texture is reused for layers at different scales. The
        // filter comes from the same resolution that picks the program, so a
        // reconstructed draw is guaranteed the unfiltered texels its kernel
        // gathers rather than a hardware blend of them.
        let filter = self.draw_plan(sampling).texture_filter;
        unsafe {
            self.gl.active_texture(glow::TEXTURE0);
            self.gl
                .bind_texture(glow::TEXTURE_2D, Some(self.cpu_layer_texture));
            self.gl
                .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filter as i32);
            self.gl
                .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, filter as i32);
            match upload {
                NativeCpuTextureUpload::Update => self.gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    glow::BGRA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(pixels)),
                ),
                NativeCpuTextureUpload::Reallocate => self.gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA as i32,
                    width as i32,
                    height as i32,
                    0,
                    glow::BGRA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(pixels)),
                ),
            }
            if upload == NativeCpuTextureUpload::Reallocate {
                self.cpu_layer_texture_width.set(width);
                self.cpu_layer_texture_height.set(height);
            }
        }
        self.draw_bound_texture(
            target,
            clip,
            alpha,
            alpha_mode,
            (width, height),
            sampling,
            trace,
        )
    }
}
