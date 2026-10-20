//! The only bridge to the existing context; no context or device is recreated.
use super::*;

impl<T: AsFd> NativeGbmRenderedScanoutContext<T> {
    pub(crate) fn service_shell_captures(
        &mut self,
        executor: &mut crate::LiveShellCaptureExecutor,
        now: std::time::Instant,
        budget: usize,
    ) -> std::io::Result<crate::LiveShellCaptureProgress> {
        executor.service_native(&mut self.inner, now, budget)
    }
}
