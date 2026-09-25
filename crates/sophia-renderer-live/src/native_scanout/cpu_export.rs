impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: AsFd,
{
    pub fn export_rendered_owned_scanout_buffer(
        &self,
        target: LiveGbmEglFrameTargetRecord,
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        self.export_rendered_owned_scanout_buffer_with_modifiers(target, &[])
    }

    pub fn export_rendered_owned_scanout_buffer_with_modifiers(
        &self,
        target: LiveGbmEglFrameTargetRecord,
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        if !target.is_valid_scanout_target() {
            return NativeGbmOwnedScanoutBufferExportReport::new(
                LiveRendererScanoutBufferExportStatus::InvalidTarget,
                LiveRendererScanoutBufferExportDetail::InvalidTarget,
                None,
            );
        }

        reduced_native_owned_scanout_buffer_export_report(
            self.inner
                .export_rendered_owned_scanout_buffer_with_modifiers(
                    target.size.width as u32,
                    target.size.height as u32,
                    preferred_modifiers,
                ),
        )
    }

    pub fn export_xrgb8888_owned_scanout_buffer_with_modifiers(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
        frame: &crate::LiveCpuComposedFrame,
        preferred_modifiers: &[u64],
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
            self.inner
                .export_xrgb8888_owned_scanout_buffer_with_modifiers(
                    target.size.width as u32,
                    target.size.height as u32,
                    frame.stride,
                    &frame.bytes,
                    preferred_modifiers,
                ),
        )
    }

    pub fn export_xrgb8888_owned_scanout_buffer_with_modifiers_in_frame_slot(
        &mut self,
        set: sophia_renderer_native_egl::NativeFrameTargetSetId,
        frame_slot: usize,
        target: LiveGbmEglFrameTargetRecord,
        frame: &crate::LiveCpuComposedFrame,
        preferred_modifiers: &[u64],
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
            self.inner
                .export_xrgb8888_owned_scanout_buffer_with_modifiers_in_frame_slot(
                    set,
                    frame_slot,
                    target.size.width as u32,
                    target.size.height as u32,
                    frame.stride,
                    &frame.bytes,
                    preferred_modifiers,
                ),
        )
    }

    pub fn rewrite_xrgb8888_owned_scanout_buffer_damage(
        &mut self,
        buffer: &mut NativeGbmOwnedScanoutBuffer,
        frame: &crate::LiveCpuComposedFrame,
        damage: &[Rect],
    ) -> Result<(), LiveRendererScanoutBufferExportDetail> {
        if buffer.descriptor.size != frame.size
            || buffer.descriptor.format != crate::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888
        {
            return Err(LiveRendererScanoutBufferExportDetail::InvalidTarget);
        }
        let damage = damage
            .iter()
            .map(|rect| sophia_renderer_native_egl::NativeCompositionRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            })
            .collect::<Vec<_>>();
        self.inner
            .rewrite_xrgb8888_owned_scanout_buffer_damage(
                &mut buffer._buffer,
                &frame.bytes,
                &damage,
            )
            .map_err(reduced_native_owned_scanout_buffer_export_detail)
    }
}
