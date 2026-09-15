use super::*;

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl<R> NativeGbmRenderedScanoutBufferDiscoveryExporter<R>
where
    R: RenderDeviceDiscoveryBackend,
{
    pub(super) fn export_from_worker(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
    ) -> LiveRenderedScanoutBufferExport<NativeGbmRenderedScanoutOwner> {
        let worker = self
            .worker
            .as_mut()
            .expect("worker export path requires a renderer worker");
        let in_flight = worker.in_flight_correlation();
        match worker.poll() {
            WorkerPoll::Exported(lease) => {
                let kind = self.worker_frame_kind.take();
                match kind {
                    Some(PendingRenderedFrameKind::Cpu) => {
                        self.last_cpu_frame_export_status =
                            Some(LiveRendererScanoutBufferExportStatus::Exported);
                    }
                    Some(PendingRenderedFrameKind::DmaBuf) => {
                        self.dmabuf_frame_exports = self.dmabuf_frame_exports.saturating_add(1);
                    }
                    Some(PendingRenderedFrameKind::Mixed) => {
                        self.mixed_frame_exports = self.mixed_frame_exports.saturating_add(1);
                    }
                    None => {}
                }
                self.context_status = worker.context_status();
                self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::Exported);
                let descriptor = lease.descriptor();
                let correlation = lease.correlation();
                return LiveRenderedScanoutBufferExport::new(
                    LiveRendererScanoutBufferExportStatus::Exported,
                    LiveRendererScanoutBufferExportDetail::Exported,
                    Some(descriptor),
                    Some(NativeGbmRenderedScanoutOwner::Worker(lease)),
                )
                .with_correlation(Some(correlation));
            }
            WorkerPoll::Failed(detail) => {
                self.layout_probe.candidate.invalidate();
                self.worker_frame_kind = None;
                self.context_status = worker.context_status();
                self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::Degraded);
                tracing::warn!("sophia_renderer_worker schema=1 status=failed detail={detail:?}");
                return LiveRenderedScanoutBufferExport::new(
                    LiveRendererScanoutBufferExportStatus::Degraded,
                    detail,
                    None,
                    None,
                );
            }
            WorkerPoll::Deferred(frame) => {
                if let Some(completed) = in_flight {
                    self.layout_probe
                        .candidate
                        .unbind_deferred(completed, std::time::Instant::now());
                }
                self.worker_frame_kind = None;
                if self.pending_frame.is_none() {
                    self.pending_frame = Some(frame);
                } else {
                    self.layout_probe.candidate.invalidate();
                    // A newer frame arrived while this one waited for a slot.
                    // Latest-wins discards the returned one; it is the same
                    // supersession as an overwrite and is counted as one.
                    self.pending_frame_supersessions =
                        self.pending_frame_supersessions.saturating_add(1);
                }
                self.context_status = worker.context_status();
                self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::Pending);
                return worker_pending_export();
            }
            WorkerPoll::HardStalled(age) => {
                self.layout_probe.candidate.invalidate();
                self.worker_frame_kind = None;
                self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::Degraded);
                tracing::error!(
                    "sophia_renderer_worker schema=1 status=hard_stall age_ms={} action=quarantine",
                    age.as_millis(),
                );
                return LiveRenderedScanoutBufferExport::new(
                    LiveRendererScanoutBufferExportStatus::Degraded,
                    LiveRendererScanoutBufferExportDetail::WorkerStalled,
                    None,
                    None,
                );
            }
            WorkerPoll::Pending {
                age,
                soft_stall_started,
            } => {
                if soft_stall_started {
                    tracing::warn!(
                        "sophia_renderer_worker schema=1 status=soft_stall age_ms={}",
                        age.as_millis(),
                    );
                }
                self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::Pending);
                return worker_pending_export();
            }
            WorkerPoll::Idle => {}
        }

        let Some(frame) = self.pending_frame.take() else {
            self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::Degraded);
            return LiveRenderedScanoutBufferExport::new(
                LiveRendererScanoutBufferExportStatus::Degraded,
                LiveRendererScanoutBufferExportDetail::RetainedBufferMissing,
                None,
                None,
            );
        };
        let kind = match &frame {
            PendingRenderedFrame::Cpu { checksum, .. } => {
                self.cpu_frame_export_attempts = self.cpu_frame_export_attempts.saturating_add(1);
                self.last_cpu_frame_checksum = Some(*checksum);
                PendingRenderedFrameKind::Cpu
            }
            PendingRenderedFrame::DmaBuf(_) => {
                self.dmabuf_frame_export_attempts =
                    self.dmabuf_frame_export_attempts.saturating_add(1);
                PendingRenderedFrameKind::DmaBuf
            }
            PendingRenderedFrame::Mixed(..) => {
                self.mixed_frame_export_attempts =
                    self.mixed_frame_export_attempts.saturating_add(1);
                PendingRenderedFrameKind::Mixed
            }
        };
        let output_format = self.layout_probe.candidate.output_format(
            super::super::worker::frame_correlation(&frame, None),
            std::time::Instant::now(),
        );
        match worker.submit(
            target,
            frame,
            self.preferred_modifiers.clone(),
            output_format.map(sophia_renderer_live::LiveCompositionFormatRequest::Preferred),
        ) {
            Ok(()) => {
                if let Some(correlation) = worker.in_flight_correlation() {
                    self.layout_probe
                        .candidate
                        .bind(correlation, std::time::Instant::now());
                }
                self.worker_frame_kind = Some(kind);
                self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::Pending);
                worker_pending_export()
            }
            Err(detail) => {
                self.layout_probe.candidate.invalidate();
                self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::Degraded);
                LiveRenderedScanoutBufferExport::new(
                    LiveRendererScanoutBufferExportStatus::Degraded,
                    detail,
                    None,
                    None,
                )
            }
        }
    }

    pub fn worker_metrics(&self) -> Option<crate::LiveRendererWorkerMetrics> {
        self.worker.as_ref().map(NativeGbmRendererWorker::metrics)
    }
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
fn worker_pending_export<Owner>() -> LiveRenderedScanoutBufferExport<Owner> {
    LiveRenderedScanoutBufferExport::new(
        LiveRendererScanoutBufferExportStatus::Pending,
        LiveRendererScanoutBufferExportDetail::WorkerPending,
        None,
        None,
    )
}
