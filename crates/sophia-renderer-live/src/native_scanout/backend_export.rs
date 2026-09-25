impl<T> NativeGbmRenderedScanoutContext<T>
where
    T: AsFd,
{
    pub fn export_rendered_owned_scanout_buffer_with_modifiers_from_backend_device_result<
        Device: AsFd,
    >(
        device: std::io::Result<Device>,
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
            sophia_renderer_native_egl::export_rendered_gbm_scanout_buffer_with_modifiers_from_backend_device_result(
                device,
                target.size.width as u32,
                target.size.height as u32,
                preferred_modifiers,
            ),
        )
    }
}
