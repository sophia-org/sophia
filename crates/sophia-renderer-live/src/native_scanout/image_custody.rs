impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: AsFd,
{
    pub fn evict_renderer_image(
        &mut self,
        image_id: LiveRendererImageId,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        self.inner
            .evict_renderer_image(sophia_renderer_native_egl::NativeRendererImageId::from_raw(
                image_id.raw(),
            ))
            .map_err(reduced_native_owned_scanout_buffer_export_detail)
    }

    pub fn promote_renderer_image(
        &mut self,
        image_id: LiveRendererImageId,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        self.inner
            .promote_renderer_image(sophia_renderer_native_egl::NativeRendererImageId::from_raw(
                image_id.raw(),
            ))
            .map_err(reduced_native_owned_scanout_buffer_export_detail)
    }

    pub fn export_promoted_renderer_image(
        &self,
        image_id: LiveRendererImageId,
    ) -> Result<Option<LiveRendererImageSnapshot>, LiveRendererScanoutBufferExportDetail> {
        self.inner
            .export_promoted_renderer_image(
                sophia_renderer_native_egl::NativeRendererImageId::from_raw(image_id.raw()),
            )
            .map(|snapshot| snapshot.map(|inner| LiveRendererImageSnapshot { image_id, inner }))
            .map_err(reduced_native_owned_scanout_buffer_export_detail)
    }

    pub fn restore_promoted_renderer_image(
        &mut self,
        snapshot: LiveRendererImageSnapshot,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        self.inner
            .restore_promoted_renderer_image(snapshot.inner)
            .map_err(reduced_native_owned_scanout_buffer_export_detail)
    }

    pub fn rollback_renderer_image(
        &mut self,
        image_id: LiveRendererImageId,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        self.inner
            .rollback_renderer_image(sophia_renderer_native_egl::NativeRendererImageId::from_raw(
                image_id.raw(),
            ))
            .map_err(reduced_native_owned_scanout_buffer_export_detail)
    }

    pub fn clear_renderer_images(
        &mut self,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail> {
        self.inner
            .clear_renderer_images()
            .map_err(reduced_native_owned_scanout_buffer_export_detail)
    }
}
