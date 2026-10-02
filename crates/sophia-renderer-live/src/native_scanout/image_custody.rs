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

    pub fn evict_renderer_image_imports(
        &mut self,
        image: LiveRendererImageId,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        self.inner
            .evict_renderer_image_imports(
                sophia_renderer_native_egl::NativeRendererImageId::from_raw(image.raw()),
            )
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

    /// One worker visit promotes and, only when requested, exports the same
    /// immutable image. Export failure cannot undo the completed promotion.
    pub fn promote_and_export_renderer_image(
        &mut self,
        image_id: LiveRendererImageId,
    ) -> Result<LiveRendererImagePromotion, LiveRendererScanoutBufferExportDetail> {
        let promoted = self.promote_renderer_image(image_id)?;
        let snapshot = if promoted {
            self.export_promoted_renderer_image(image_id)
        } else {
            Ok(None)
        };
        Ok(LiveRendererImagePromotion { promoted, snapshot })
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
