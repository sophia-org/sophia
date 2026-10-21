//! Worker result classification and request diagnostics.
use super::*;

pub(super) fn inventory_replacement_result(
    generation: u64,
    result: Result<bool, LiveRendererScanoutBufferExportDetail>,
) -> io::Result<u64> {
    match result {
        Ok(true) => Ok(generation),
        Ok(false) => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "renderer bridge still in use",
        )),
        Err(detail) => Err(io::Error::other(format!(
            "renderer inventory replacement failed: {detail:?}"
        ))),
    }
}

pub(super) fn reduce_worker_command_send_error<T>(
    error: TrySendError<T>,
) -> LiveRendererScanoutBufferExportDetail {
    match error {
        TrySendError::Full(_) => LiveRendererScanoutBufferExportDetail::WorkerQueueFull,
        TrySendError::Disconnected(_) => LiveRendererScanoutBufferExportDetail::WorkerDisconnected,
    }
}

pub(super) fn reduce_worker_maintenance_receive_error(
    error: RecvTimeoutError,
) -> LiveRendererScanoutBufferExportDetail {
    match error {
        RecvTimeoutError::Timeout => LiveRendererScanoutBufferExportDetail::WorkerStalled,
        RecvTimeoutError::Disconnected => LiveRendererScanoutBufferExportDetail::WorkerDisconnected,
    }
}

pub(super) fn pending_frame_kind_name(frame: &PendingRenderedFrame) -> &'static str {
    match frame {
        PendingRenderedFrame::Cpu { .. } => "cpu",
        PendingRenderedFrame::DmaBuf(_) => "dmabuf",
        PendingRenderedFrame::Mixed(..) => "mixed",
    }
}

pub(super) fn trace_worker_request(request: LiveRendererWorkerRequestId) -> bool {
    static MINIMUM: OnceLock<Option<u64>> = OnceLock::new();
    MINIMUM
        .get_or_init(|| {
            std::env::var("SOPHIA_LIVE_RENDERER_WORKER_TRACE_AFTER_REQUEST")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
        })
        .as_ref()
        .copied()
        .is_some_and(|minimum| request.0 >= minimum)
}

pub(super) fn worker_outcome_name(outcome: &WorkerOutcome) -> &'static str {
    match outcome {
        WorkerOutcome::Exported { .. } => "exported",
        WorkerOutcome::Deferred(_) => "deferred",
        WorkerOutcome::Failed(_) => "failed",
    }
}
