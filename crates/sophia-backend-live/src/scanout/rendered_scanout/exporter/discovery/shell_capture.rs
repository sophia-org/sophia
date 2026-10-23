use super::*;
use sophia_renderer_live::{
    LiveRendererSnapshotBudget, LiveRendererSnapshotEpoch, LiveShellCaptureCustody,
    LiveShellCaptureInbox,
};
use std::{io, sync::Arc};

impl<R: RenderDeviceDiscoveryBackend> NativeGbmRenderedScanoutBufferDiscoveryExporter<R> {
    // Phase one uses the existing worker context. An inline exporter refuses
    // explicitly: it must not silently start a second context or device.
    pub(crate) fn request_shell_capture(
        &self,
        epoch: Arc<LiveRendererSnapshotEpoch>,
        budget: Arc<LiveRendererSnapshotBudget>,
        custody: LiveShellCaptureCustody,
    ) -> io::Result<()> {
        self.worker
            .as_ref()
            .ok_or(io::ErrorKind::Unsupported)?
            .request_shell_capture(epoch, budget, custody)
    }
    pub(crate) fn poll_shell_capture(
        &self,
    ) -> io::Result<
        Option<(
            LiveShellCaptureInbox,
            sophia_renderer_live::LiveShellCaptureFacts,
        )>,
    > {
        self.worker
            .as_ref()
            .ok_or(io::ErrorKind::Unsupported)?
            .poll_shell_capture()
    }
    pub(crate) fn request_prepare_shell_capture_destination(
        &self,
        width: u32,
        height: u32,
        wake: &sophia_wake::Notifier,
    ) -> io::Result<()> {
        self.worker
            .as_ref()
            .ok_or(io::ErrorKind::Unsupported)?
            .request_prepare_shell_capture_destination(width, height, wake)
    }
    pub(crate) fn poll_prepare_shell_capture_destination(&self) -> io::Result<Option<u64>> {
        self.worker
            .as_ref()
            .ok_or(io::ErrorKind::Unsupported)?
            .poll_prepare_shell_capture_destination()
    }
    pub(crate) fn close_shell_capture(&self) -> io::Result<()> {
        self.worker
            .as_ref()
            .ok_or(io::ErrorKind::Unsupported)?
            .close_shell_capture()
    }
}
