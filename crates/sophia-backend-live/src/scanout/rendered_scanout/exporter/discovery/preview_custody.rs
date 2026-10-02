//! Preview maintenance never discards a donor's unfinished frame.
use super::*;

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl<R: RenderDeviceDiscoveryBackend> NativeGbmRenderedScanoutBufferDiscoveryExporter<R> {
    pub(crate) fn image_store_identity(&self) -> Option<u64> {
        self.worker
            .as_ref()
            .map(NativeGbmRendererWorker::image_store_identity)
    }

    /// Preview intake never discards a donor render. A busy donor is deferred.
    pub fn try_export_promoted_renderer_image(
        &mut self,
        image: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<
        Option<sophia_renderer_live::LiveRendererImageSnapshot>,
        sophia_renderer_live::LiveRendererScanoutBufferExportDetail,
    > {
        if let Some(worker) = &mut self.worker {
            return worker.export_promoted_renderer_image(image);
        }
        self.context.as_ref().map_or(Ok(None), |context| {
            context.export_promoted_renderer_image(image)
        })
    }

    pub fn evict_renderer_image_imports(
        &mut self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<bool, sophia_renderer_live::LiveRendererScanoutBufferExportDetail> {
        if let Some(worker) = &self.worker {
            return worker.evict_renderer_image_imports(image_id);
        }
        self.context.as_mut().map_or(Ok(false), |context| {
            context.evict_renderer_image_imports(image_id)
        })
    }

    pub fn promote_and_export_renderer_image(
        &mut self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<
        sophia_renderer_live::LiveRendererImagePromotion,
        sophia_renderer_live::LiveRendererScanoutBufferExportDetail,
    > {
        if let Some(worker) = &self.worker {
            return worker.promote_and_export_renderer_image(image_id);
        }
        self.context.as_mut().map_or_else(
            || Ok(sophia_renderer_live::LiveRendererImagePromotion::default()),
            |context| context.promote_and_export_renderer_image(image_id),
        )
    }

    /// Recovery polling owns only the failed generation's outstanding render.
    /// A returned scanout lease was never submitted and may be dropped here.
    pub(crate) fn poll_discarded_preview_render(
        &mut self,
        expected: crate::LiveNativeFrameIdentity,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        let Some(worker) = &mut self.worker else {
            return Ok(true);
        };
        let result = worker.poll_discarded_preview_render(expected);
        if !matches!(result, Ok(false)) {
            self.worker_frame_kind = None;
        }
        result
    }
}
