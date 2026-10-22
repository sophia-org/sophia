//! The only bridge to the existing context; no context or device is recreated.
use super::*;

impl<T: AsFd> NativeGbmRenderedScanoutContext<T> {
    /// Admission facts from this context's own device; see the native
    /// context method of the same name. Discovery-time only, not per frame.
    pub fn shell_capture_facts(
        &self,
    ) -> Option<sophia_renderer_native_egl::NativeShellCaptureFacts> {
        self.inner.shell_capture_facts()
    }

    /// Bounded destination preparation; returns the proven destination
    /// byte bound for this size, or Ok(None) when capture is disabled.
    pub fn prepare_shell_capture_destination(
        &mut self,
        width: u32,
        height: u32,
    ) -> Result<Option<u64>, sophia_renderer_native_egl::NativeGbmScanoutBufferExportDetail> {
        self.inner.prepare_shell_capture_destination(width, height)
    }

    pub(crate) fn service_shell_captures(
        &mut self,
        executor: &mut crate::LiveShellCaptureExecutor,
        now: std::time::Instant,
        budget: usize,
    ) -> std::io::Result<crate::LiveShellCaptureProgress> {
        executor.service_native(&mut self.inner, now, budget)
    }
}
