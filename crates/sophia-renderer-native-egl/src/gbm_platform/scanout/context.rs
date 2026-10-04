#[derive(Debug)]
pub struct NativeGbmOwnedScanoutBufferExportReport {
    pub status: NativeGbmScanoutBufferExportStatus,
    pub detail: NativeGbmScanoutBufferExportDetail,
    pub buffer: Option<NativeGbmOwnedScanoutBuffer>,
    /// The age the surface reported for the buffer this export rendered into,
    /// in renders into the same slot. `None` means the driver would not say,
    /// which is not the same as fresh.
    pub buffer_age: Option<u32>,
    /// Which target bundle served this export. A caller keying retained state
    /// to a slot needs this: a rebuilt bundle is a new GBM surface with new
    /// buffers, so anything it remembered about the old one is void. Reporting
    /// the generation lets the caller notice every rebuild, including the ones
    /// that happen inside a single export call.
    pub target_generation: Option<u64>,
    /// What the render actually painted.
    pub repaint: NativeCompositionRepaintOutcome,
}

/// What a completed render painted into its target.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NativeCompositionRepaintOutcome {
    /// Everything, because no damage plan applied or none was offered.
    #[default]
    Full,
    /// Only the planned damage, in this many rectangles.
    Partial { rects: usize },
}

pub struct NativeGbmRenderedScanoutContext<T: std::os::fd::AsFd> {
    egl: khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    gbm_device: gbm::Device<T>,
    stats: NativeGbmPersistentRenderStats,
    composition_target: Option<PersistentCompositionTarget>,
    // Execution only: no client import or captured pixel storage survives here.
    // One compatible context per supported capture format (XR24 and AR24).
    capture_targets: [Option<NativeCaptureTarget>; 2],
    /// Target slots and pixel-proof state, one set per output the context
    /// serves. A device-shared context renders for several outputs, and a
    /// slot index alone does not identify a bundle across them: two outputs
    /// both use slots 0 through 2, at their own sizes and modifiers, so a
    /// shared array would rebuild a bundle on every alternation. That rebuild
    /// is exactly what `target_recreations` counts and what the gates require
    /// to stay at zero.
    target_sets: std::collections::BTreeMap<NativeFrameTargetSetId, NativeFrameTargetSet>,
    /// Whose set the inline `composition_target` currently belongs to.
    current_target_set: NativeFrameTargetSetId,
    /// Whether this display reports `EGL_BUFFER_AGE_EXT`. Queried once: the
    /// extension set does not change under a live display, and a per-frame
    /// string search on the render path would cost more than the damage saves.
    buffer_age_supported: bool,
    /// Monotonic identity for target bundles. Never reused, so a caller can
    /// compare the generation it last saw against the one an export reports
    /// and know whether the buffers it remembers still exist.
    next_target_generation: u64,
    /// Facts from the most recent composed render, stashed here for the export
    /// to report. The render returns a buffer and the report is built two
    /// frames up the call stack; widening every signature between them to carry
    /// three values would be worse than the pattern `last_composition_pixel_
    /// metrics` already set.
    last_render_buffer_age: Option<u32>,
    last_render_repaint: NativeCompositionRepaintOutcome,
    last_render_target_generation: Option<u64>,
    /// Capture pixels on every composed render, past the bounded startup
    /// proof budget. For equivalence smokes only: per-frame `glReadPixels` on
    /// a session hot path is exactly what the budget exists to prevent.
    capture_pixels_always: bool,
    render_timing_enabled: bool,
    import_cache_capacity: usize,
    renderer_images: std::collections::BTreeMap<NativeRendererImageId, NativeRendererImage>,
    renderer_image_bytes: u64,
    import_devices: Option<Vec<NativeImageImportDevice>>,
    transfer_stats: NativeImageTransferStats,
    image_bridges: Vec<NativeImageBridgeSlot>,
    transfer_sources: std::collections::BTreeMap<(u32, u64), usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NativeRendererImageState {
    Staged,
    Promoted,
}

struct NativeRendererImage {
    buffer: NativeGbmOwnedScanoutBuffer,
    state: NativeRendererImageState,
    bytes: u64,
}

struct NativeRenderTarget {
    width: u32,
    height: u32,
    surface_format: gbm::Format,
    egl_context: khronos_egl::Context,
    pipeline: PersistentXrgb8888GlPipeline,
}

/// Identifies one output's private target state within a device context.
///
/// Opaque on purpose: the renderer has no notion of an output, and the
/// backend supplies whatever identity it uses for a head.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NativeFrameTargetSetId(u64);

impl NativeFrameTargetSetId {
    /// The set a context uses when nobody names one, which is every
    /// single-output caller and every test.
    pub const DEFAULT: Self = Self(0);

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// One output's slots and its own pixel-proof budget.
///
/// The proof is per output because it is that output's evidence: a capture
/// latched by whichever head rendered first would report one screen's pixels
/// as another's.
#[derive(Default)]
struct NativeFrameTargetSet {
    targets: [Option<PersistentCompositionTarget>; NATIVE_FRAME_TARGET_SLOT_CAPACITY],
    last_composition_pixel_metrics: Option<NativeCompositionPixelMetrics>,
    proven_composition_nonzero_rgb_pixels: usize,
    composition_pixel_proof_attempts: usize,
}

struct PersistentCompositionTarget {
    target: NativeRenderTarget,
    surface: std::rc::Rc<NativeFrameSurface>,
    import_cache: NativeDmaBufImportCache,
    preferred_modifiers: Vec<gbm::Modifier>,
    /// Identifies this bundle for the lifetime of the context. A rebuild takes
    /// a fresh generation because it takes fresh buffers.
    generation: u64,
}

pub const DEFAULT_NATIVE_DMA_BUF_IMPORT_CACHE_CAPACITY: usize = 256;
pub const DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY: usize = 256;
pub const DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET: u64 = 512 * 1024 * 1024;
pub const NATIVE_FRAME_TARGET_SLOT_CAPACITY: usize = 3;

impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: std::os::fd::AsFd,
{
    pub fn from_backend_device_result(
        device: std::io::Result<T>,
    ) -> NativeGbmRenderedScanoutContextReport<T> {
        match device {
            Ok(device) => match Self::new(device, DEFAULT_NATIVE_DMA_BUF_IMPORT_CACHE_CAPACITY) {
                Ok(context) => NativeGbmRenderedScanoutContextReport {
                    status: NativeGbmRenderedScanoutContextStatus::Ready,
                    context: Some(context),
                },
                Err(status) => NativeGbmRenderedScanoutContextReport {
                    status,
                    context: None,
                },
            },
            Err(_error) => NativeGbmRenderedScanoutContextReport {
                status: NativeGbmRenderedScanoutContextStatus::Unavailable,
                context: None,
            },
        }
    }

    pub fn from_backend_device_result_with_import_cache_capacity(
        device: std::io::Result<T>,
        import_cache_capacity: usize,
    ) -> NativeGbmRenderedScanoutContextReport<T> {
        match device {
            Ok(device) => match Self::new(device, import_cache_capacity) {
                Ok(context) => NativeGbmRenderedScanoutContextReport {
                    status: NativeGbmRenderedScanoutContextStatus::Ready,
                    context: Some(context),
                },
                Err(status) => NativeGbmRenderedScanoutContextReport {
                    status,
                    context: None,
                },
            },
            Err(_error) => NativeGbmRenderedScanoutContextReport {
                status: NativeGbmRenderedScanoutContextStatus::Unavailable,
                context: None,
            },
        }
    }

    fn new(
        device: T,
        import_cache_capacity: usize,
    ) -> Result<Self, NativeGbmRenderedScanoutContextStatus> {
        use gbm::AsRaw as _;

        let gbm_device = gbm::Device::new(device)
            .map_err(|_error| NativeGbmRenderedScanoutContextStatus::Unavailable)?;
        let egl = unsafe { khronos_egl::DynamicInstance::<khronos_egl::EGL1_5>::load_required() }
            .map_err(|_error| NativeGbmRenderedScanoutContextStatus::Unavailable)?;
        let native_display = gbm_device.as_raw() as khronos_egl::NativeDisplayType;
        let display = unsafe {
            egl.get_platform_display(
                EGL_PLATFORM_GBM_KHR,
                native_display,
                &[khronos_egl::ATTRIB_NONE],
            )
        }
        .map_err(|_error| NativeGbmRenderedScanoutContextStatus::Unavailable)?;

        egl.initialize(display)
            .map_err(|_error| NativeGbmRenderedScanoutContextStatus::Degraded)?;

        // Buffer age is what makes damage-limited repaint safe: without it a
        // reused slot's content age is a guess. Absence is not a failure, it
        // just means every repaint is full.
        let buffer_age_supported = egl
            .query_string(Some(display), khronos_egl::EXTENSIONS)
            .ok()
            .and_then(|extensions| extensions.to_str().ok())
            .is_some_and(|extensions| {
                extensions
                    .split_whitespace()
                    .any(|extension| extension == EGL_EXT_BUFFER_AGE_NAME)
            });

        Ok(Self {
            egl,
            display,
            gbm_device,
            stats: NativeGbmPersistentRenderStats::default(),
            composition_target: None,
            capture_targets: [None, None],
            target_sets: std::collections::BTreeMap::new(),
            current_target_set: NativeFrameTargetSetId::DEFAULT,
            import_cache_capacity,
            renderer_images: std::collections::BTreeMap::new(),
            renderer_image_bytes: 0,
            import_devices: None,
            transfer_stats: NativeImageTransferStats::default(),
            image_bridges: Vec::new(),
            transfer_sources: std::collections::BTreeMap::new(),
            buffer_age_supported,
            next_target_generation: 1,
            last_render_buffer_age: None,
            last_render_repaint: NativeCompositionRepaintOutcome::Full,
            last_render_target_generation: None,
            capture_pixels_always: false,
            render_timing_enabled: false,
        })
    }

    /// Take the next bundle identity. Generations are never reused, so a
    /// caller that remembers a slot's buffers can tell a surviving bundle from
    /// a rebuilt one by comparing this alone.
    fn allocate_target_generation(&mut self) -> u64 {
        let generation = self.next_target_generation;
        self.next_target_generation = self.next_target_generation.saturating_add(1);
        generation
    }

    /// Qualification-only wall-clock timings; normal rendering counts work only.
    pub fn set_render_timing_enabled(&mut self, enabled: bool) {
        self.render_timing_enabled = enabled;
    }

    pub fn persistent_render_stats(&self) -> NativeGbmPersistentRenderStats {
        let mut stats = self.stats;
        stats.transfer_captures = self.transfer_stats.captures;
        stats.transfer_attempts = self.transfer_stats.attempts;
        stats.transfer_failures = self.transfer_stats.failures;
        if let Some(persistent) = self.composition_target.as_ref() {
            accumulate_import_cache_stats(&mut stats.import_cache, persistent.import_cache.stats());
            stats.sampling = stats
                .sampling
                .saturating_add(persistent.target.pipeline.sampling_stats());
        }
        for persistent in self
            .target_sets
            .values()
            .flat_map(|set| set.targets.iter().flatten())
        {
            accumulate_import_cache_stats(&mut stats.import_cache, persistent.import_cache.stats());
            stats.sampling = stats
                .sampling
                .saturating_add(persistent.target.pipeline.sampling_stats());
        }
        for cached in self.capture_targets.iter().flatten() {
            stats.sampling = stats
                .sampling
                .saturating_add(cached.target.pipeline.sampling_stats());
        }
        stats
    }

    pub fn composition_pixel_metrics(
        &self,
        set: NativeFrameTargetSetId,
    ) -> Option<NativeCompositionPixelMetrics> {
        self.target_sets
            .get(&set)
            .and_then(|set| set.last_composition_pixel_metrics)
    }

    /// The set the inline target currently belongs to, created on demand.
    fn current_set_mut(&mut self) -> &mut NativeFrameTargetSet {
        self.target_sets.entry(self.current_target_set).or_default()
    }

    fn current_proof_attempts(&self) -> usize {
        self.target_sets
            .get(&self.current_target_set)
            .map_or(0, |set| set.composition_pixel_proof_attempts)
    }

    fn current_proven_nonzero(&self) -> usize {
        self.target_sets
            .get(&self.current_target_set)
            .map_or(0, |set| set.proven_composition_nonzero_rgb_pixels)
    }

    /// Capture pixels on every composed render. Smoke-test instrumentation:
    /// the equivalence proof must read frames after the startup budget would
    /// have stopped capturing.
    pub fn force_composition_pixel_capture(&mut self) {
        self.capture_pixels_always = true;
    }

    pub fn composition_nonzero_rgb_pixels(&self, set: NativeFrameTargetSetId) -> usize {
        self.target_sets
            .get(&set)
            .map_or(0, |set| set.proven_composition_nonzero_rgb_pixels)
    }

    pub fn export_rendered_owned_scanout_buffer(
        &self,
        width: u32,
        height: u32,
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        self.export_rendered_owned_scanout_buffer_with_modifiers(width, height, &[])
    }

    pub fn export_rendered_owned_scanout_buffer_with_modifiers(
        &self,
        width: u32,
        height: u32,
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        if width == 0 || height == 0 {
            return NativeGbmOwnedScanoutBufferExportReport {
                status: NativeGbmScanoutBufferExportStatus::InvalidTarget,
                detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
                buffer: None,
                buffer_age: None,
                target_generation: None,
                repaint: NativeCompositionRepaintOutcome::Full,
            };
        }

        match render_initialized_gbm_scanout_front_buffer(
            &self.egl,
            self.display,
            &self.gbm_device,
            width,
            height,
            preferred_modifiers,
            None,
        ) {
            Ok(buffer) => exported_scanout_buffer_report(buffer),
            Err(detail) => failed_scanout_buffer_report(detail),
        }
    }

    pub fn export_xrgb8888_owned_scanout_buffer_with_modifiers(
        &mut self,
        width: u32,
        height: u32,
        stride: u32,
        pixels: &[u8],
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        if width == 0 || height == 0 {
            return NativeGbmOwnedScanoutBufferExportReport {
                status: NativeGbmScanoutBufferExportStatus::InvalidTarget,
                detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
                buffer: None,
                buffer_age: None,
                target_generation: None,
                repaint: NativeCompositionRepaintOutcome::Full,
            };
        }

        let expected_stride = width.saturating_mul(4);
        let expected_len = usize::try_from(expected_stride)
            .ok()
            .and_then(|stride| stride.checked_mul(usize::try_from(height).ok()?));
        if stride != expected_stride || expected_len != Some(pixels.len()) {
            return NativeGbmOwnedScanoutBufferExportReport {
                status: NativeGbmScanoutBufferExportStatus::InvalidTarget,
                detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
                buffer: None,
                buffer_age: None,
                target_generation: None,
                repaint: NativeCompositionRepaintOutcome::Full,
            };
        }
        let started = Instant::now();
        let result = self
            .write_cpu_xrgb8888_scanout_buffer(width, height, pixels)
            .or_else(|_| {
                self.render_one_shot_xrgb8888_with_recovery(
                    width,
                    height,
                    pixels,
                    preferred_modifiers,
                )
            });
        self.stats.max_upload = self.stats.max_upload.max(started.elapsed());
        match result {
            Ok(buffer) => exported_scanout_buffer_report(buffer),
            Err(detail) => failed_scanout_buffer_report(detail),
        }
    }

    pub fn export_xrgb8888_owned_scanout_buffer_with_modifiers_in_frame_slot(
        &mut self,
        set: NativeFrameTargetSetId,
        frame_slot: usize,
        width: u32,
        height: u32,
        stride: u32,
        pixels: &[u8],
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        self.with_frame_target_slot(set, frame_slot, |context| {
            context.export_xrgb8888_owned_scanout_buffer_with_modifiers(
                width,
                height,
                stride,
                pixels,
                preferred_modifiers,
            )
        })
        .unwrap_or_else(invalid_frame_slot_report)
    }

    pub fn rewrite_xrgb8888_owned_scanout_buffer_damage(
        &mut self,
        buffer: &mut NativeGbmOwnedScanoutBuffer,
        pixels: &[u8],
        damage: &[NativeCompositionRect],
    ) -> Result<(), NativeGbmScanoutBufferExportDetail> {
        let started = Instant::now();
        let result = buffer.rewrite_xrgb8888_damage(pixels, damage);
        self.stats.max_upload = self.stats.max_upload.max(started.elapsed());
        if result.is_ok() {
            self.stats.frame_uploads = self.stats.frame_uploads.saturating_add(1);
        }
        result
    }

    pub fn export_dmabuf_owned_scanout_buffer_with_modifiers(
        &mut self,
        frame: NativeDmaBufFrame<'_>,
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        if !frame.is_valid() {
            return NativeGbmOwnedScanoutBufferExportReport {
                status: NativeGbmScanoutBufferExportStatus::InvalidTarget,
                detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
                buffer: None,
                buffer_age: None,
                target_generation: None,
                repaint: NativeCompositionRepaintOutcome::Full,
            };
        }
        let result = self.render_one_shot_dmabuf_with_recovery(frame, preferred_modifiers);
        match result {
            Ok(buffer) => exported_scanout_buffer_report(buffer),
            Err(detail) => failed_scanout_buffer_report(detail),
        }
    }

    pub fn export_dmabuf_owned_scanout_buffer_with_modifiers_in_frame_slot(
        &mut self,
        set: NativeFrameTargetSetId,
        frame_slot: usize,
        frame: NativeDmaBufFrame<'_>,
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        self.with_frame_target_slot(set, frame_slot, |context| {
            context.export_dmabuf_owned_scanout_buffer_with_modifiers(frame, preferred_modifiers)
        })
        .unwrap_or_else(invalid_frame_slot_report)
    }

    pub fn export_composed_owned_scanout_buffer_with_modifiers(
        &mut self,
        frame: NativeCompositionFrame<'_>,
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        self.export_composed_owned_scanout_buffer(
            frame,
            NativeCompositionOutputRequest {
                preferred_modifiers,
                format: None,
            },
        )
    }

    pub fn export_composed_owned_scanout_buffer(
        &mut self,
        frame: NativeCompositionFrame<'_>,
        request: NativeCompositionOutputRequest<'_>,
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        if !request.is_valid()
            || frame.width == 0
            || frame.height == 0
            || frame.layers.iter().any(|layer| match layer {
                NativeCompositionLayer::Cpu(layer) => {
                    layer.width == 0
                        || layer.height == 0
                        || !matches!(layer.format, 0x3432_5258 | 0x3432_5241)
                        || layer.target.width <= 0
                        || layer.target.height <= 0
                        || !layer.alpha.is_finite()
                }
                NativeCompositionLayer::DmaBuf(layer) => {
                    !layer.frame.is_valid()
                        || layer.target.width <= 0
                        || layer.target.height <= 0
                        || !layer.alpha.is_finite()
                }
                NativeCompositionLayer::RendererImage(layer) => {
                    !layer.image_id.is_valid()
                        || layer.target.width <= 0
                        || layer.target.height <= 0
                        || !layer.alpha.is_finite()
                }
                NativeCompositionLayer::Solid(layer) => {
                    layer.target.width <= 0 || layer.target.height <= 0
                }
            })
        {
            return NativeGbmOwnedScanoutBufferExportReport {
                status: NativeGbmScanoutBufferExportStatus::InvalidTarget,
                detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
                buffer: None,
                buffer_age: None,
                target_generation: None,
                repaint: NativeCompositionRepaintOutcome::Full,
            };
        }
        if frame.layers.iter().any(|layer| {
            matches!(layer, NativeCompositionLayer::RendererImage(layer)
                if !self.renderer_images.contains_key(&layer.image_id))
                || matches!(layer, NativeCompositionLayer::DmaBuf(layer)
                    if layer.custody.is_some_and(|snapshot| !snapshot.is_current()))
        }) {
            // The render epilogue will not run. Release the stale imports even
            // when an old queued snapshot is refused before drawing.
            for image in frame.layers.iter().filter_map(|layer| match layer {
                NativeCompositionLayer::DmaBuf(layer)
                    if layer
                        .custody
                        .is_some_and(|snapshot| !snapshot.import_cacheable()) =>
                {
                    Some(layer.image_id)
                }
                _ => None,
            }) {
                let _ = self.evict_renderer_image_imports(image);
            }
            return failed_scanout_buffer_report(
                NativeGbmScanoutBufferExportDetail::InvalidRendererImageId,
            );
        }
        self.last_render_buffer_age = None;
        self.last_render_repaint = NativeCompositionRepaintOutcome::Full;
        self.last_render_target_generation = None;
        let mut report = match self.render_one_shot_composition_with_recovery(frame, request) {
            Ok(buffer) => exported_scanout_buffer_report(buffer),
            Err(detail) => failed_scanout_buffer_report(detail),
        };
        if let Some(buffer) = &mut report.buffer {
            buffer._sampled_snapshots = frame
                .layers
                .iter()
                .filter_map(|layer| match layer {
                    NativeCompositionLayer::DmaBuf(layer) => layer.custody.cloned(),
                    _ => None,
                })
                .collect();
        }
        // A frame may outlive the publication's demand and the broadcast
        // eviction. It still owns valid pixels, but must not resurrect an
        // unbounded import-cache owner after drawing those pixels once.
        for image in frame.layers.iter().filter_map(|layer| match layer {
            NativeCompositionLayer::DmaBuf(layer)
                if layer
                    .custody
                    .is_some_and(|snapshot| !snapshot.import_cacheable()) =>
            {
                Some(layer.image_id)
            }
            _ => None,
        }) {
            // Cleanup cannot erase the successful render's buffer or fence.
            // The eviction path destroys a target whose import cleanup fails.
            let _ = self.evict_renderer_image_imports(image);
        }
        report.buffer_age = self.last_render_buffer_age;
        report.repaint = self.last_render_repaint;
        report.target_generation = self.last_render_target_generation;
        report
    }

    pub fn export_composed_owned_scanout_buffer_with_modifiers_in_frame_slot(
        &mut self,
        set: NativeFrameTargetSetId,
        frame_slot: usize,
        frame: NativeCompositionFrame<'_>,
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        self.export_composed_owned_scanout_buffer_in_frame_slot(
            set,
            frame_slot,
            frame,
            NativeCompositionOutputRequest {
                preferred_modifiers,
                format: None,
            },
        )
    }

    pub fn export_composed_owned_scanout_buffer_in_frame_slot(
        &mut self,
        set: NativeFrameTargetSetId,
        frame_slot: usize,
        frame: NativeCompositionFrame<'_>,
        request: NativeCompositionOutputRequest<'_>,
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        self.with_frame_target_slot(set, frame_slot, |context| {
            context.export_composed_owned_scanout_buffer(frame, request)
        })
        .unwrap_or_else(invalid_frame_slot_report)
    }

    /// Swap one set's slot into the inline target for the duration of an
    /// operation, then put it back where it came from.
    ///
    /// The set travels with the slot because the inline target is the only
    /// thing the render path knows about: while it is swapped in, anything
    /// recording pixel-proof state must record it against the set that owns
    /// the bundle, not against whoever rendered last.
    fn with_frame_target_slot<R>(
        &mut self,
        set: NativeFrameTargetSetId,
        frame_slot: usize,
        operation: impl FnOnce(&mut Self) -> R,
    ) -> Option<R> {
        if frame_slot >= NATIVE_FRAME_TARGET_SLOT_CAPACITY {
            return None;
        }
        let persistent = self
            .target_sets
            .entry(set)
            .or_default()
            .targets
            .get_mut(frame_slot)?
            .take();
        let inline = std::mem::replace(&mut self.composition_target, persistent);
        let restored_set = std::mem::replace(&mut self.current_target_set, set);
        let result = operation(self);
        self.current_target_set = restored_set;
        let rendered = self.composition_target.take();
        self.target_sets.entry(set).or_default().targets[frame_slot] = rendered;
        self.composition_target = inline;
        Some(result)
    }

    fn create_render_target(
        &mut self,
        spec: RenderTargetSpec,
    ) -> Result<
        (
            NativeRenderTarget,
            std::rc::Rc<NativeFrameSurface>,
            std::time::Duration,
        ),
        NativeGbmScanoutBufferExportDetail,
    > {
        let started = Instant::now();
        let created = create_native_render_target(
            NativeEglScanoutDevice {
                egl: &self.egl,
                display: self.display,
                gbm_device: &self.gbm_device,
            },
            spec,
        );
        self.stats.max_target_create = self.stats.max_target_create.max(started.elapsed());
        if let Ok((_, _, surface_create_duration)) = &created {
            self.stats.target_creations = self.stats.target_creations.saturating_add(1);
            self.stats.gl_pipeline_creations = self.stats.gl_pipeline_creations.saturating_add(1);
            self.stats.frame_surface_creations =
                self.stats.frame_surface_creations.saturating_add(1);
            self.stats.max_frame_surface_create = self
                .stats
                .max_frame_surface_create
                .max(*surface_create_duration);
        }
        created
    }
}
include!("context/render_once.rs");
include!("context/renderer_images.rs");
include!("context/image_capture.rs");
include!("context/timing.rs");
include!("context/image_transfer.rs");
include!("context/image_bridge.rs");
impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: std::os::fd::AsFd,
{
    fn destroy_native_render_target(&self, target: NativeRenderTarget) {
        trace_native_lifecycle("native_render_target_destroy_started");
        let _ = self.egl.make_current(self.display, None, None, None);
        drop(target.pipeline);
        let _ = self.egl.destroy_context(self.display, target.egl_context);
        trace_native_lifecycle("egl_context_destroyed");
    }

    fn destroy_persistent_composition_target(
        &mut self,
        mut persistent: PersistentCompositionTarget,
    ) {
        let surface = persistent.surface.egl_surface();
        if self
            .egl
            .make_current(
                self.display,
                Some(surface),
                Some(surface),
                Some(persistent.target.egl_context),
            )
            .is_ok()
        {
            let _ =
                persistent
                    .import_cache
                    .clear(&self.egl, self.display, &persistent.target.pipeline);
        } else {
            persistent.import_cache.abandon(&self.egl, self.display);
        }
        accumulate_import_cache_stats(
            &mut self.stats.import_cache,
            persistent.import_cache.stats(),
        );
        self.stats.sampling = self
            .stats
            .sampling
            .saturating_add(persistent.target.pipeline.sampling_stats());
        self.destroy_native_render_target(persistent.target);
    }
}

impl<T> Drop for NativeGbmRenderedScanoutContext<T>
where
    T: std::os::fd::AsFd,
{
    fn drop(&mut self) {
        if let Some(persistent) = self.composition_target.take() {
            self.destroy_persistent_composition_target(persistent);
        }
        for mut set in std::mem::take(&mut self.target_sets).into_values() {
            for slot in &mut set.targets {
                if let Some(persistent) = slot.take() {
                    self.destroy_persistent_composition_target(persistent);
                }
            }
        }
        for cached in std::mem::take(&mut self.capture_targets)
            .into_iter()
            .flatten()
        {
            self.destroy_native_render_target(cached.target);
        }
        // Target imports are gone; release retained buffer surfaces while their
        // EGL display and its dynamically loaded entry points still exist.
        self.renderer_images.clear();
        self.renderer_image_bytes = 0;
        self.destroy_image_bridges();
        self.import_devices.take();
        let _ = self.egl.terminate(self.display);
        trace_native_lifecycle("egl_display_terminated");
    }
}

fn invalid_frame_slot_report() -> NativeGbmOwnedScanoutBufferExportReport {
    NativeGbmOwnedScanoutBufferExportReport {
        status: NativeGbmScanoutBufferExportStatus::InvalidTarget,
        detail: NativeGbmScanoutBufferExportDetail::InvalidTarget,
        buffer: None,
        buffer_age: None,
        target_generation: None,
        repaint: NativeCompositionRepaintOutcome::Full,
    }
}

fn accumulate_import_cache_stats(
    total: &mut NativeDmaBufImportCacheStats,
    additional: NativeDmaBufImportCacheStats,
) {
    total.imports = total.imports.saturating_add(additional.imports);
    total.hits = total.hits.saturating_add(additional.hits);
    total.evictions = total.evictions.saturating_add(additional.evictions);
    total.live_entries = total.live_entries.saturating_add(additional.live_entries);
    total.descriptor_mismatches = total
        .descriptor_mismatches
        .saturating_add(additional.descriptor_mismatches);
    total.capacity_rejections = total
        .capacity_rejections
        .saturating_add(additional.capacity_rejections);
}
