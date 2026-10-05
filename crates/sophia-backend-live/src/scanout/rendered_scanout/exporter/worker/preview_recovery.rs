//! An unsubmitted preview withdrawal may discard only its own renderer result.
use super::*;

impl NativeGbmRendererWorker {
    pub(crate) fn poll_discarded_preview_render(
        &mut self,
        expected: crate::LiveNativeFrameIdentity,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        use LiveRendererScanoutBufferExportDetail as D;
        if self.stalled.is_some() {
            return Err(D::WorkerStalled);
        }
        if self
            .in_flight_correlation()
            .is_some_and(|correlation| correlation.native != Some(expected))
        {
            return Err(D::InvalidTarget);
        }
        match self.poll() {
            WorkerPoll::Pending { .. } => Ok(false),
            WorkerPoll::HardStalled(_) | WorkerPoll::Stalled { .. } => Err(D::WorkerStalled),
            WorkerPoll::Failed(
                D::InvalidRendererImageId
                | D::RendererImageStoreFull
                | D::RendererImageTransferBusy,
            ) => Ok(true),
            WorkerPoll::Failed(detail) => Err(detail),
            WorkerPoll::Exported(lease) => {
                if lease.correlation().native != Some(expected) {
                    return Err(D::InvalidTarget);
                }
                drop(lease);
                Ok(true)
            }
            WorkerPoll::Deferred(frame) => {
                if frame_correlation(&frame, None).native != Some(expected) {
                    return Err(D::InvalidTarget);
                }
                drop(frame);
                Ok(true)
            }
            WorkerPoll::Idle => Ok(true),
        }
    }
}
