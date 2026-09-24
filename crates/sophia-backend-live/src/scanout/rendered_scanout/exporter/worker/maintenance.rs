//! The facade's maintenance surface: renderer-image eviction, promotion,
//! rollback, export and restore, the in-flight discard and the image
//! clear, each refused while a render is out or stalled. Split from the
//! facade's rendering surface to keep the file within its ceiling (t026).

use super::*;

impl NativeGbmRendererWorker {
    pub fn evict_renderer_image(
        &self,
        image_id: LiveRendererImageId,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        self.renderer_image_transition(|completion_sender| WorkerCommand::Evict {
            image_id,
            completion_sender,
        })
    }

    pub fn promote_renderer_image(
        &self,
        image_id: LiveRendererImageId,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        self.renderer_image_transition(|completion_sender| WorkerCommand::Promote {
            image_id,
            completion_sender,
        })
    }

    pub fn rollback_renderer_image(
        &self,
        image_id: LiveRendererImageId,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        self.renderer_image_transition(|completion_sender| WorkerCommand::Rollback {
            image_id,
            completion_sender,
        })
    }

    pub fn export_promoted_renderer_image(
        &mut self,
        image_id: LiveRendererImageId,
    ) -> Result<Option<LiveRendererImageSnapshot>, LiveRendererScanoutBufferExportDetail> {
        if self.in_flight.is_some() || self.stalled.is_some() {
            return Err(LiveRendererScanoutBufferExportDetail::WorkerPending);
        }
        let (completion_sender, completion_receiver) = sync_channel(1);
        self.core
            .command_sender
            .try_send(WorkerCommand::ExportPromotedImage {
                image_id,
                completion_sender,
            })
            .map_err(reduce_worker_command_send_error)?;
        completion_receiver
            .recv_timeout(WORKER_MAINTENANCE_TIMEOUT)
            .map_err(reduce_worker_maintenance_receive_error)?
    }

    pub fn restore_promoted_renderer_image(
        &mut self,
        snapshot: LiveRendererImageSnapshot,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        if self.in_flight.is_some() || self.stalled.is_some() {
            return Err(LiveRendererScanoutBufferExportDetail::WorkerPending);
        }
        let (completion_sender, completion_receiver) = sync_channel(1);
        self.core
            .command_sender
            .try_send(WorkerCommand::RestorePromotedImage {
                snapshot,
                completion_sender,
            })
            .map_err(reduce_worker_command_send_error)?;
        let completion = completion_receiver
            .recv_timeout(WORKER_MAINTENANCE_TIMEOUT)
            .map_err(reduce_worker_maintenance_receive_error)?;
        self.persistent_render_stats = completion.persistent_render_stats;
        completion.result
    }

    fn renderer_image_transition(
        &self,
        command: impl FnOnce(
            SyncSender<Result<bool, LiveRendererScanoutBufferExportDetail>>,
        ) -> WorkerCommand,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        let (completion_sender, completion_receiver) = sync_channel(1);
        self.core
            .command_sender
            .try_send(command(completion_sender))
            .map_err(|error| match error {
                TrySendError::Full(_) => LiveRendererScanoutBufferExportDetail::WorkerQueueFull,
                TrySendError::Disconnected(_) => {
                    LiveRendererScanoutBufferExportDetail::WorkerDisconnected
                }
            })?;
        completion_receiver
            .recv_timeout(WORKER_MAINTENANCE_TIMEOUT)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => LiveRendererScanoutBufferExportDetail::WorkerStalled,
                RecvTimeoutError::Disconnected => {
                    LiveRendererScanoutBufferExportDetail::WorkerDisconnected
                }
            })?
    }

    pub fn discard_in_flight_for_maintenance(
        &mut self,
    ) -> Result<bool, LiveRendererScanoutBufferExportDetail> {
        if self.in_flight.is_none() && self.stalled.is_none() {
            return Ok(false);
        }
        let deadline = Instant::now() + WORKER_MAINTENANCE_TIMEOUT;
        loop {
            match self.poll() {
                WorkerPoll::Idle => return Ok(false),
                WorkerPoll::Exported(lease) => {
                    drop(lease);
                    return Ok(true);
                }
                WorkerPoll::Deferred(_) => return Ok(true),
                WorkerPoll::Failed(detail) => return Err(detail),
                WorkerPoll::HardStalled(_) | WorkerPoll::Stalled { .. } => {
                    return Err(LiveRendererScanoutBufferExportDetail::WorkerStalled);
                }
                WorkerPoll::Pending { .. } => {
                    if Instant::now() >= deadline {
                        return Err(LiveRendererScanoutBufferExportDetail::WorkerStalled);
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    pub fn clear_renderer_images(
        &mut self,
    ) -> Result<usize, LiveRendererScanoutBufferExportDetail> {
        // A stalled render is still out on the worker: as much in flight as
        // any other for what may be touched under it (t186).
        if self.in_flight.is_some() || self.stalled.is_some() {
            return Err(LiveRendererScanoutBufferExportDetail::WorkerPending);
        }
        let (completion_sender, completion_receiver) = sync_channel(1);
        self.core
            .command_sender
            .try_send(WorkerCommand::ClearImages { completion_sender })
            .map_err(|error| match error {
                TrySendError::Full(_) => LiveRendererScanoutBufferExportDetail::WorkerQueueFull,
                TrySendError::Disconnected(_) => {
                    LiveRendererScanoutBufferExportDetail::WorkerDisconnected
                }
            })?;
        let completion = completion_receiver
            .recv_timeout(WORKER_MAINTENANCE_TIMEOUT)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => LiveRendererScanoutBufferExportDetail::WorkerStalled,
                RecvTimeoutError::Disconnected => {
                    LiveRendererScanoutBufferExportDetail::WorkerDisconnected
                }
            })?;
        self.persistent_render_stats = completion.persistent_render_stats;
        completion.result
    }
}
