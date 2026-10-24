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

    pub fn prepare_managed_shell_capture_destination(
        &mut self,
        width: u32,
        height: u32,
        budget: std::sync::Arc<LiveRendererSnapshotBudget>,
        epoch: std::sync::Arc<LiveRendererSnapshotEpoch>,
        wake: sophia_wake::Notifier,
    ) -> std::io::Result<Option<crate::LiveShellCaptureDestination>> {
        self.inner
            .prepare_managed_shell_capture_destination(width, height, budget, epoch, wake)
            .map(|token| token.map(crate::LiveShellCaptureDestination::Managed))
            .map_err(|detail| std::io::Error::other(format!("{detail:?}")))
    }

    pub fn maintain_managed_shell_captures(&mut self) -> std::io::Result<(bool, usize)> {
        self.inner
            .maintain_managed_shell_captures()
            .map(|progress| (progress.needs_wake, progress.pending_fences))
            .map_err(|detail| std::io::Error::other(format!("{detail:?}")))
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
