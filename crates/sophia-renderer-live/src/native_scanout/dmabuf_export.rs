impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: AsFd,
{
    pub fn export_dmabuf_owned_scanout_buffer_with_modifiers(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
        frame: LiveDmaBufFrame<'_>,
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        if !target.is_valid_scanout_target()
            || target.size.width != i32::try_from(frame.width).unwrap_or(i32::MAX)
            || target.size.height != i32::try_from(frame.height).unwrap_or(i32::MAX)
        {
            return NativeGbmOwnedScanoutBufferExportReport::new(
                LiveRendererScanoutBufferExportStatus::InvalidTarget,
                LiveRendererScanoutBufferExportDetail::InvalidTarget,
                None,
            );
        }
        reduced_native_owned_scanout_buffer_export_report(
            self.inner
                .export_dmabuf_owned_scanout_buffer_with_modifiers(
                    sophia_renderer_native_egl::NativeDmaBufFrame {
                        width: frame.width,
                        height: frame.height,
                        format: frame.format,
                        modifier: frame.modifier,
                        fd: frame.fd,
                        offset: frame.offset,
                        stride: frame.stride,
                    },
                    preferred_modifiers,
                ),
        )
    }

    pub fn export_dmabuf_owned_scanout_buffer_with_modifiers_in_frame_slot(
        &mut self,
        set: sophia_renderer_native_egl::NativeFrameTargetSetId,
        frame_slot: usize,
        target: LiveGbmEglFrameTargetRecord,
        frame: LiveDmaBufFrame<'_>,
        preferred_modifiers: &[u64],
    ) -> NativeGbmOwnedScanoutBufferExportReport {
        if !target.is_valid_scanout_target()
            || target.size.width != i32::try_from(frame.width).unwrap_or(i32::MAX)
            || target.size.height != i32::try_from(frame.height).unwrap_or(i32::MAX)
        {
            return NativeGbmOwnedScanoutBufferExportReport::new(
                LiveRendererScanoutBufferExportStatus::InvalidTarget,
                LiveRendererScanoutBufferExportDetail::InvalidTarget,
                None,
            );
        }
        reduced_native_owned_scanout_buffer_export_report(
            self.inner
                .export_dmabuf_owned_scanout_buffer_with_modifiers_in_frame_slot(
                    set,
                    frame_slot,
                    sophia_renderer_native_egl::NativeDmaBufFrame {
                        width: frame.width,
                        height: frame.height,
                        format: frame.format,
                        modifier: frame.modifier,
                        fd: frame.fd,
                        offset: frame.offset,
                        stride: frame.stride,
                    },
                    preferred_modifiers,
                ),
        )
    }
}
