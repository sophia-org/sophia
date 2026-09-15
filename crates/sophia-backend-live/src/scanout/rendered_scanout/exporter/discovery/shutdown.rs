use super::*;

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl<R: RenderDeviceDiscoveryBackend> NativeGbmRenderedScanoutBufferDiscoveryExporter<R> {
    pub(crate) fn request_worker_shutdown(&self) {
        if let Some(worker) = &self.worker {
            worker.request_shutdown();
        }
    }

    pub(crate) fn poll_worker_shutdown(&self) -> std::io::Result<bool> {
        self.worker
            .as_ref()
            .map_or(Ok(true), |worker| worker.poll_shutdown())
    }
}
