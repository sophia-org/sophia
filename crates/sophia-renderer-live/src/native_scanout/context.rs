impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: AsFd,
{
    pub fn set_image_import_devices(
        &mut self,
        devices: Vec<std::os::fd::OwnedFd>,
    ) -> Result<(), LiveRendererScanoutBufferExportDetail> {
        self.inner
            .set_image_import_devices(devices)
            .map_err(reduced_native_owned_scanout_buffer_export_detail)
    }

    pub fn replace_image_import_devices(
        &mut self,
        devices: Vec<std::os::fd::OwnedFd>,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        self.inner
            .replace_image_import_devices(devices)
            .map_err(reduced_native_owned_scanout_buffer_export_detail)
    }

    pub fn image_transfer_stats(&self) -> sophia_renderer_native_egl::NativeImageTransferStats {
        self.inner.image_transfer_stats()
    }

    pub fn persistent_render_stats(&self) -> LiveNativePersistentRenderStats {
        let stats = self.inner.persistent_render_stats();
        LiveNativePersistentRenderStats {
            target_creations: stats.target_creations,
            target_recreations: stats.target_recreations,
            gl_pipeline_creations: stats.gl_pipeline_creations,
            frame_surface_creations: stats.frame_surface_creations,
            cpu_target_creations: stats.cpu_target_creations,
            dmabuf_target_creations: stats.dmabuf_target_creations,
            composition_target_creations: stats.composition_target_creations,
            composition_target_reuses: stats.composition_target_reuses,
            generation_replacements: stats.generation_replacements,
            recovery_replacements: stats.recovery_replacements,
            frame_uploads: stats.frame_uploads,
            snapshot_captures: stats.snapshot_captures,
            snapshot_promotions: stats.snapshot_promotions,
            snapshot_rollbacks: stats.snapshot_rollbacks,
            snapshot_evictions: stats.snapshot_evictions,
            snapshot_live_entries: stats.snapshot_live_entries,
            snapshot_live_bytes: stats.snapshot_live_bytes,
            import_cache: LiveNativeDmaBufImportCacheStats {
                imports: stats.import_cache.imports,
                hits: stats.import_cache.hits,
                evictions: stats.import_cache.evictions,
                live_entries: stats.import_cache.live_entries,
                descriptor_mismatches: stats.import_cache.descriptor_mismatches,
                capacity_rejections: stats.import_cache.capacity_rejections,
            },
            exact_nearest_draws: stats.sampling.exact_nearest_draws,
            sharp_downscale_draws: stats.sampling.sharp_downscale_draws,
            sharp_upscale_draws: stats.sampling.sharp_upscale_draws,
            linear_fallback_draws: stats.sampling.linear_fallback_draws,
            max_target_create: stats.max_target_create,
            max_frame_surface_create: stats.max_frame_surface_create,
            max_render: stats.max_render,
            max_upload: stats.max_upload,
        }
    }

    pub fn composition_nonzero_rgb_pixels(
        &self,
        set: sophia_renderer_native_egl::NativeFrameTargetSetId,
    ) -> usize {
        self.inner.composition_nonzero_rgb_pixels(set)
    }

    /// Capture pixels on every composed render. Smoke-test instrumentation.
    pub fn force_composition_pixel_capture(&mut self) {
        self.inner.force_composition_pixel_capture();
    }

    /// The most recent composed render's captured pixel metrics, when capture
    /// ran for that render.
    pub fn composition_pixel_metrics(
        &self,
        set: sophia_renderer_native_egl::NativeFrameTargetSetId,
    ) -> Option<sophia_renderer_native_egl::NativeCompositionPixelMetrics> {
        self.inner.composition_pixel_metrics(set)
    }

    pub fn from_backend_device_result(
        device: std::io::Result<T>,
    ) -> NativeGbmRenderedScanoutContextReport<T> {
        let report = sophia_renderer_native_egl::NativeGbmRenderedScanoutContext::
            from_backend_device_result_with_import_cache_capacity(
                device,
                crate::LIVE_PRESENTATION_REGISTRY_CAPACITY,
            );
        NativeGbmRenderedScanoutContextReport {
            status: match report.status {
                sophia_renderer_native_egl::NativeGbmRenderedScanoutContextStatus::Ready => {
                    NativeGbmRenderedScanoutContextStatus::Ready
                }
                sophia_renderer_native_egl::NativeGbmRenderedScanoutContextStatus::Unavailable => {
                    NativeGbmRenderedScanoutContextStatus::Unavailable
                }
                sophia_renderer_native_egl::NativeGbmRenderedScanoutContextStatus::Degraded => {
                    NativeGbmRenderedScanoutContextStatus::Degraded
                }
            },
            context: report
                .context
                .map(|inner| NativeGbmRenderedScanoutContext { inner }),
        }
    }
}
