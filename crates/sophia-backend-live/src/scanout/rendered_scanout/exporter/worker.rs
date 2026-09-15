use super::correlation::{LiveRendererFrameCorrelation, LiveRendererWorkerRequestId};
use super::discovery::PendingRenderedFrame;
use super::frame_slots::{
    LiveRendererFrameSlotMetrics, LiveRendererFrameSlotMetricsHandle, LiveRendererFrameSlotPool,
    LiveRendererFrameSlotToken,
};
use crate::api::*;
use sophia_renderer_live::{
    LiveMixedCompositionError, LiveNativePersistentRenderStats, LiveRendererImageId,
    LiveRendererImageSnapshot, LiveRendererScanoutBufferDescriptor,
    LiveRendererScanoutBufferExportDetail, LiveRendererScanoutBufferExportStatus,
    NativeFrameTargetSetId, NativeGbmOwnedScanoutBuffer, NativeGbmOwnedScanoutBufferExportReport,
    NativeGbmRenderedScanoutContext, NativeGbmRenderedScanoutContextStatus,
};
use std::io;
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{
    Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel,
};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

mod lifecycle;
mod shutdown;
use lifecycle::{DeviceIdentity, OutputClaim, WorkerControl, WorkerRegistry, WorkerThread};

const WORKER_COMMAND_CAPACITY: usize = 32;
const WORKER_RESULT_CAPACITY: usize = 2;
const WORKER_FREE_CPU_BUFFER_CAPACITY: usize = 3;
pub const LIVE_RENDERER_WORKER_SOFT_STALL: Duration = Duration::from_millis(100);
pub const LIVE_RENDERER_WORKER_HARD_STALL: Duration = Duration::from_secs(1);
const WORKER_MAINTENANCE_TIMEOUT: Duration = Duration::from_secs(1);

pub(super) fn frame_correlation(
    frame: &PendingRenderedFrame,
    request: Option<LiveRendererWorkerRequestId>,
) -> LiveRendererFrameCorrelation {
    let (native, trace, direct_scanout) = match frame {
        PendingRenderedFrame::Mixed(frame, native) => {
            (*native, frame.trace, Some(frame.direct_scanout))
        }
        PendingRenderedFrame::Cpu { native, .. } => (*native, None, None),
        PendingRenderedFrame::DmaBuf(_) => (None, None, None),
    };
    LiveRendererFrameCorrelation {
        native,
        request,
        trace,
        direct_scanout,
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LiveRendererWorkerLeaseId(u64);

/// Which output a command or result belongs to.
///
/// One worker serves every output of a device group, so identity can no
/// longer be inferred from position: with a request outstanding per output,
/// the next result on a shared channel is not necessarily the answer to the
/// one this caller asked. Every render, every release, and every reply names
/// its output instead.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LiveRendererWorkerOutputKey(u64);

impl LiveRendererWorkerOutputKey {
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    /// This output's private target slots inside the shared render context.
    pub const fn target_set(self) -> NativeFrameTargetSetId {
        NativeFrameTargetSetId::from_raw(self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveRendererWorkerMetrics {
    pub requests: usize,
    pub completions: usize,
    pub failures: usize,
    /// Results that arrived on this output's channel naming another output.
    /// Structurally impossible; counted so the claim is evidence rather than
    /// an assertion nobody checks.
    pub result_misroutes: usize,
    pub soft_stalls: usize,
    pub hard_stalls: usize,
    pub release_enqueue_failures: usize,
    pub max_request_age: Duration,
    pub frame_slots: LiveRendererFrameSlotMetrics,
}

#[derive(Debug)]
pub struct NativeGbmRendererWorkerScanoutLease {
    descriptor: LiveRendererScanoutBufferDescriptor,
    correlation: LiveRendererFrameCorrelation,
    output: LiveRendererWorkerOutputKey,
    lease_id: LiveRendererWorkerLeaseId,
    slot_token: LiveRendererFrameSlotToken,
    command_sender: SyncSender<WorkerCommand>,
    release_enqueue_failures: Arc<AtomicUsize>,
}

impl NativeGbmRendererWorkerScanoutLease {
    pub const fn correlation(&self) -> LiveRendererFrameCorrelation {
        self.correlation
    }

    pub const fn descriptor(&self) -> LiveRendererScanoutBufferDescriptor {
        self.descriptor
    }

    pub const fn lease_id(&self) -> LiveRendererWorkerLeaseId {
        self.lease_id
    }

    pub const fn slot_token(&self) -> LiveRendererFrameSlotToken {
        self.slot_token
    }

    pub fn export_scanout_dma_buf_fds(
        &self,
    ) -> io::Result<Option<(u8, [Option<std::os::fd::OwnedFd>; 4])>> {
        Ok(None)
    }
}

impl Drop for NativeGbmRendererWorkerScanoutLease {
    fn drop(&mut self) {
        if self
            .command_sender
            .try_send(WorkerCommand::Release {
                output: self.output,
                lease_id: self.lease_id,
                slot_token: self.slot_token,
            })
            .is_err()
        {
            self.release_enqueue_failures
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// The thread itself, and everything a device group shares.
///
/// One core serves every output of one DRM device: one EGL display, one GBM
/// device, one renderer-image store. Outputs attach to it and get a facade
/// each; the core outlives them and shuts the thread down when the last
/// reference goes.
pub struct NativeGbmRendererWorkerCore {
    command_sender: SyncSender<WorkerCommand>,
    _thread: std::sync::Mutex<WorkerThread>,
    control: Arc<WorkerControl>,
    inventory_replacement: std::sync::Mutex<Option<Receiver<io::Result<u64>>>>,
    release_enqueue_failures: Arc<AtomicUsize>,
}

impl NativeGbmRendererWorkerCore {
    pub fn spawn<D>(device: io::Result<D>) -> io::Result<Arc<Self>>
    where
        D: AsFd + Send + 'static,
    {
        Self::spawn_with_image_import_devices(device, Vec::new())
    }

    pub fn spawn_with_image_import_devices<D>(
        device: io::Result<D>,
        import_devices: Vec<std::os::fd::OwnedFd>,
    ) -> io::Result<Arc<Self>>
    where
        D: AsFd + Send + 'static,
    {
        if import_devices.len() > sophia_renderer_live::NATIVE_IMAGE_IMPORT_DEVICE_CAPACITY {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "renderer import device capacity exceeded",
            ));
        }
        let device = device?;
        let identity = DeviceIdentity::from_device(&device)?;
        let reservation = WorkerRegistry::shared().reserve(identity)?;
        let control = Arc::new(WorkerControl::default());
        let service_control = Arc::clone(&control);
        let (command_sender, command_receiver) = sync_channel(WORKER_COMMAND_CAPACITY);
        let thread = reservation.start(|| {
            thread::Builder::new()
                .name("sophia-render-gpu".to_owned())
                .spawn(move || {
                    run_worker(
                        Ok(device),
                        import_devices,
                        command_receiver,
                        service_control,
                    )
                })
        })?;
        Ok(Arc::new(Self {
            command_sender,
            _thread: std::sync::Mutex::new(thread),
            control,
            inventory_replacement: std::sync::Mutex::new(None),
            release_enqueue_failures: Arc::new(AtomicUsize::new(0)),
        }))
    }

    /// Queue one inventory generation; the caller retains its original descriptors until acknowledged.
    pub fn request_image_import_device_replacement(
        &self,
        generation: u64,
        devices: Vec<std::os::fd::OwnedFd>,
    ) -> io::Result<()> {
        if devices.len() > sophia_renderer_live::NATIVE_IMAGE_IMPORT_DEVICE_CAPACITY {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "renderer import device capacity exceeded",
            ));
        }
        let mut pending = self
            .inventory_replacement
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if pending.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "renderer inventory replacement pending",
            ));
        }
        let (completion_sender, completion_receiver) = sync_channel(1);
        self.command_sender
            .try_send(WorkerCommand::ReplaceImageImportDevices {
                generation,
                devices,
                completion_sender,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    io::Error::new(io::ErrorKind::WouldBlock, "renderer command queue full")
                }
                TrySendError::Disconnected(_) => {
                    io::Error::new(io::ErrorKind::BrokenPipe, "renderer worker disconnected")
                }
            })?;
        *pending = Some(completion_receiver);
        Ok(())
    }

    /// Return the applied generation without waiting; a busy bridge reports WouldBlock for retry.
    pub fn poll_image_import_device_replacement(&self) -> io::Result<Option<u64>> {
        let mut pending = self
            .inventory_replacement
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(receiver) = pending.as_ref() else {
            return Ok(None);
        };
        match receiver.try_recv() {
            Ok(result) => {
                *pending = None;
                result.map(Some)
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                *pending = None;
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "renderer inventory acknowledgement disconnected",
                ))
            }
        }
    }

    /// Attach one output. Its results come back on a channel of its own, which
    /// is what makes demultiplexing structural rather than a check performed
    /// after the fact.
    pub(super) fn attach(
        self: &Arc<Self>,
        output: LiveRendererWorkerOutputKey,
    ) -> NativeGbmRendererWorker {
        let (reply, result_receiver) = sync_channel(WORKER_RESULT_CAPACITY);
        let frame_slot_metrics = LiveRendererFrameSlotMetricsHandle::default();
        let claim = Arc::new(OutputClaim::default());
        // A refused registration quarantines its facade before it can render.
        let registered = self
            .command_sender
            .try_send(WorkerCommand::Register {
                output,
                reply,
                frame_slot_metrics: frame_slot_metrics.clone(),
                claim: Arc::clone(&claim),
            })
            .is_ok();
        NativeGbmRendererWorker {
            core: Arc::clone(self),
            output,
            claim,
            result_receiver,
            next_request_id: 1,
            in_flight: None,
            discarded_release: None,
            context_status: None,
            persistent_render_stats: LiveNativePersistentRenderStats::default(),
            composition_nonzero_rgb_pixels: 0,
            frame_slot_metrics,
            metrics: LiveRendererWorkerMetrics::default(),
            quarantined: !registered,
        }
    }
}

impl Drop for NativeGbmRendererWorkerCore {
    fn drop(&mut self) {
        self.control.shutdown();
        let _ = self.command_sender.try_send(WorkerCommand::Shutdown);
    }
}

pub(super) struct NativeGbmRendererWorker {
    core: Arc<NativeGbmRendererWorkerCore>,
    claim: Arc<OutputClaim>,
    output: LiveRendererWorkerOutputKey,
    result_receiver: Receiver<WorkerResult>,
    next_request_id: u64,
    in_flight: Option<InFlightRequest>,
    discarded_release: Option<DiscardedWorkerLease>,
    context_status: Option<NativeGbmRenderedScanoutContextStatus>,
    persistent_render_stats: LiveNativePersistentRenderStats,
    composition_nonzero_rgb_pixels: usize,
    frame_slot_metrics: LiveRendererFrameSlotMetricsHandle,
    metrics: LiveRendererWorkerMetrics,
    quarantined: bool,
}

impl NativeGbmRendererWorker {
    pub fn request_image_import_device_replacement(
        &self,
        generation: u64,
        devices: Vec<std::os::fd::OwnedFd>,
    ) -> io::Result<()> {
        self.core
            .request_image_import_device_replacement(generation, devices)
    }

    pub fn poll_image_import_device_replacement(&self) -> io::Result<Option<u64>> {
        self.core.poll_image_import_device_replacement()
    }

    /// One worker owning its own thread, which is one output's worth of
    /// device state. This is what every head had before outputs could share a
    /// core, and it is what a session still gets when sharing is off.
    pub fn spawn<D>(device: io::Result<D>, output: LiveRendererWorkerOutputKey) -> io::Result<Self>
    where
        D: AsFd + Send + 'static,
    {
        Ok(NativeGbmRendererWorkerCore::spawn(device)?.attach(output))
    }

    pub const fn in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    pub const fn in_flight_correlation(&self) -> Option<LiveRendererFrameCorrelation> {
        match &self.in_flight {
            Some(request) => Some(request.correlation),
            None => None,
        }
    }

    pub const fn context_status(&self) -> Option<NativeGbmRenderedScanoutContextStatus> {
        self.context_status
    }

    pub const fn persistent_render_stats(&self) -> LiveNativePersistentRenderStats {
        self.persistent_render_stats
    }

    pub const fn composition_nonzero_rgb_pixels(&self) -> usize {
        self.composition_nonzero_rgb_pixels
    }

    pub fn metrics(&self) -> LiveRendererWorkerMetrics {
        LiveRendererWorkerMetrics {
            release_enqueue_failures: self.core.release_enqueue_failures.load(Ordering::Relaxed),
            frame_slots: self.frame_slot_metrics.snapshot(),
            ..self.metrics
        }
    }

    pub(crate) fn submit(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
        frame: PendingRenderedFrame,
        preferred_modifiers: Vec<u64>,
        output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
    ) -> Result<(), LiveRendererScanoutBufferExportDetail> {
        if self.quarantined || self.in_flight.is_some() {
            return Err(LiveRendererScanoutBufferExportDetail::WorkerPending);
        }
        let Some(next_request_id) = self.next_request_id.checked_add(1) else {
            self.quarantined = true;
            return Err(LiveRendererScanoutBufferExportDetail::WorkerDisconnected);
        };
        let request_id = LiveRendererWorkerRequestId(self.next_request_id);
        self.next_request_id = next_request_id;
        let correlation = frame_correlation(&frame, Some(request_id));
        let command = WorkerCommand::Render {
            output: self.output,
            request_id,
            target,
            frame,
            preferred_modifiers,
            output_format,
        };
        self.core
            .command_sender
            .try_send(command)
            .map_err(|error| match error {
                TrySendError::Full(_) => LiveRendererScanoutBufferExportDetail::WorkerQueueFull,
                TrySendError::Disconnected(_) => {
                    LiveRendererScanoutBufferExportDetail::WorkerDisconnected
                }
            })?;
        self.in_flight = Some(InFlightRequest {
            request_id,
            correlation,
            output_format,
            submitted_at: Instant::now(),
            soft_stall_reported: false,
        });
        self.metrics.requests = self.metrics.requests.saturating_add(1);
        if trace_worker_request(request_id) {
            tracing::debug!(
                "sophia_renderer_worker schema=2 status=request_submitted request={} requests={}",
                request_id.0,
                self.metrics.requests,
            );
        }
        Ok(())
    }

    pub fn poll(&mut self) -> WorkerPoll {
        let Some(mut in_flight) = self.in_flight.take() else {
            self.flush_discarded_release();
            if self.quarantined
                && self.discarded_release.is_none()
                && let Ok(result) = self.result_receiver.try_recv()
            {
                self.release_discarded_result(&result);
            }
            return WorkerPoll::Idle;
        };
        match self.result_receiver.try_recv() {
            Ok(result) => {
                self.in_flight = None;
                let age = in_flight.submitted_at.elapsed();
                self.metrics.max_request_age = self.metrics.max_request_age.max(age);
                self.context_status = Some(result.context_status);
                self.persistent_render_stats = result.persistent_render_stats;
                self.composition_nonzero_rgb_pixels = result.composition_nonzero_rgb_pixels;
                if trace_worker_request(in_flight.request_id) {
                    tracing::debug!(
                        "sophia_renderer_worker schema=2 status=request_received request={} expected={} age_ms={}",
                        result.request_id.0,
                        in_flight.request_id.0,
                        age.as_millis(),
                    );
                }
                // Per-output reply channels make a misdelivered result
                // unreachable rather than merely unlikely, which is why this
                // asks rather than assumes: a result naming another output on
                // this output's own channel means the routing that structure
                // guarantees has been subverted, and the buffer it carries
                // belongs to a slot pool that is not ours to lease from.
                if result.output != self.output {
                    self.metrics.result_misroutes = self.metrics.result_misroutes.saturating_add(1);
                    self.metrics.failures = self.metrics.failures.saturating_add(1);
                    self.quarantined = true;
                    tracing::error!(
                        "sophia_renderer_worker schema=3 status=result_misrouted output={} observed={} request={}",
                        self.output.raw(),
                        result.output.raw(),
                        result.request_id.0,
                    );
                    self.release_discarded_result(&result);
                    return WorkerPoll::Failed(
                        LiveRendererScanoutBufferExportDetail::WorkerDisconnected,
                    );
                }
                if result.request_id != in_flight.request_id
                    || result.correlation != in_flight.correlation
                    || matches!(&result.outcome, WorkerOutcome::Exported { descriptor, .. }
                        if matches!(in_flight.output_format, Some(sophia_renderer_live::LiveCompositionFormatRequest::Required(format)) if descriptor.format != format))
                {
                    self.metrics.failures = self.metrics.failures.saturating_add(1);
                    self.quarantined = true;
                    self.release_discarded_result(&result);
                    return WorkerPoll::Failed(
                        LiveRendererScanoutBufferExportDetail::WorkerDisconnected,
                    );
                }
                match result.outcome {
                    WorkerOutcome::Exported {
                        descriptor,
                        lease_id,
                        slot_token,
                    } => {
                        self.metrics.completions = self.metrics.completions.saturating_add(1);
                        WorkerPoll::Exported(NativeGbmRendererWorkerScanoutLease {
                            output: self.output,
                            correlation: result.correlation,
                            descriptor,
                            lease_id,
                            slot_token,
                            command_sender: self.core.command_sender.clone(),
                            release_enqueue_failures: Arc::clone(
                                &self.core.release_enqueue_failures,
                            ),
                        })
                    }
                    WorkerOutcome::Failed(detail) => {
                        self.metrics.failures = self.metrics.failures.saturating_add(1);
                        WorkerPoll::Failed(detail)
                    }
                    WorkerOutcome::Deferred(frame) => {
                        if frame_correlation(&frame, Some(result.request_id)) != result.correlation
                        {
                            self.metrics.failures = self.metrics.failures.saturating_add(1);
                            self.quarantined = true;
                            return WorkerPoll::Failed(
                                LiveRendererScanoutBufferExportDetail::WorkerDisconnected,
                            );
                        }
                        WorkerPoll::Deferred(frame)
                    }
                }
            }
            Err(TryRecvError::Disconnected) => {
                self.in_flight = None;
                self.quarantined = true;
                self.metrics.failures = self.metrics.failures.saturating_add(1);
                WorkerPoll::Failed(LiveRendererScanoutBufferExportDetail::WorkerDisconnected)
            }
            Err(TryRecvError::Empty) => {
                let age = in_flight.submitted_at.elapsed();
                self.metrics.max_request_age = self.metrics.max_request_age.max(age);
                if age >= LIVE_RENDERER_WORKER_HARD_STALL {
                    self.in_flight = None;
                    self.quarantined = true;
                    self.metrics.hard_stalls = self.metrics.hard_stalls.saturating_add(1);
                    self.metrics.failures = self.metrics.failures.saturating_add(1);
                    WorkerPoll::HardStalled(age)
                } else {
                    let mut soft_stall_started = false;
                    if age >= LIVE_RENDERER_WORKER_SOFT_STALL && !in_flight.soft_stall_reported {
                        in_flight.soft_stall_reported = true;
                        soft_stall_started = true;
                        self.metrics.soft_stalls = self.metrics.soft_stalls.saturating_add(1);
                    }
                    self.in_flight = Some(in_flight);
                    WorkerPoll::Pending {
                        age,
                        soft_stall_started,
                    }
                }
            }
        }
    }

    fn release_discarded_result(&mut self, result: &WorkerResult) {
        if let WorkerOutcome::Exported {
            lease_id,
            slot_token,
            ..
        } = result.outcome
        {
            self.discarded_release = Some(DiscardedWorkerLease {
                output: result.output,
                lease_id,
                slot_token,
            });
            self.flush_discarded_release();
        }
    }

    fn flush_discarded_release(&mut self) {
        let Some(release) = self.discarded_release.take() else {
            return;
        };
        if let Err(error) = self.core.command_sender.try_send(WorkerCommand::Release {
            output: release.output,
            lease_id: release.lease_id,
            slot_token: release.slot_token,
        }) {
            self.core
                .release_enqueue_failures
                .fetch_add(1, Ordering::Relaxed);
            if matches!(error, TrySendError::Full(_)) {
                self.discarded_release = Some(release);
            }
        }
    }

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
        if self.in_flight.is_some() {
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
        if self.in_flight.is_some() {
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
        if self.in_flight.is_none() {
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
                WorkerPoll::HardStalled(_) => {
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
        if self.in_flight.is_some() {
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

impl Drop for NativeGbmRendererWorker {
    fn drop(&mut self) {
        self.claim.detach();
        // The claim survives a full queue; this command only wakes an idle service.
        let _ = self
            .core
            .command_sender
            .try_send(WorkerCommand::Deregister {
                output: self.output,
                claim: Arc::clone(&self.claim),
            });
    }
}

pub(super) enum WorkerPoll {
    Idle,
    Pending {
        age: Duration,
        soft_stall_started: bool,
    },
    Exported(NativeGbmRendererWorkerScanoutLease),
    Deferred(PendingRenderedFrame),
    Failed(LiveRendererScanoutBufferExportDetail),
    HardStalled(Duration),
}

struct DiscardedWorkerLease {
    output: LiveRendererWorkerOutputKey,
    lease_id: LiveRendererWorkerLeaseId,
    slot_token: LiveRendererFrameSlotToken,
}

struct InFlightRequest {
    request_id: LiveRendererWorkerRequestId,
    correlation: LiveRendererFrameCorrelation,
    output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
    submitted_at: Instant,
    soft_stall_reported: bool,
}

enum WorkerCommand {
    /// Attach an output, giving the worker the channel its results go home on
    /// and the metrics handle its own slot pool publishes to.
    Register {
        output: LiveRendererWorkerOutputKey,
        reply: SyncSender<WorkerResult>,
        frame_slot_metrics: LiveRendererFrameSlotMetricsHandle,
        claim: Arc<OutputClaim>,
    },
    /// Detach an output and free everything it still holds.
    Deregister {
        output: LiveRendererWorkerOutputKey,
        claim: Arc<OutputClaim>,
    },
    Render {
        output: LiveRendererWorkerOutputKey,
        request_id: LiveRendererWorkerRequestId,
        target: LiveGbmEglFrameTargetRecord,
        frame: PendingRenderedFrame,
        preferred_modifiers: Vec<u64>,
        output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
    },
    ReplaceImageImportDevices {
        generation: u64,
        devices: Vec<std::os::fd::OwnedFd>,
        completion_sender: SyncSender<io::Result<u64>>,
    },
    Evict {
        image_id: LiveRendererImageId,
        completion_sender: SyncSender<Result<bool, LiveRendererScanoutBufferExportDetail>>,
    },
    Promote {
        image_id: LiveRendererImageId,
        completion_sender: SyncSender<Result<bool, LiveRendererScanoutBufferExportDetail>>,
    },
    Rollback {
        image_id: LiveRendererImageId,
        completion_sender: SyncSender<Result<bool, LiveRendererScanoutBufferExportDetail>>,
    },
    ExportPromotedImage {
        image_id: LiveRendererImageId,
        completion_sender: SyncSender<
            Result<Option<LiveRendererImageSnapshot>, LiveRendererScanoutBufferExportDetail>,
        >,
    },
    RestorePromotedImage {
        snapshot: LiveRendererImageSnapshot,
        completion_sender: SyncSender<WorkerRestoreImageResult>,
    },
    ClearImages {
        completion_sender: SyncSender<WorkerMaintenanceResult>,
    },
    Release {
        output: LiveRendererWorkerOutputKey,
        lease_id: LiveRendererWorkerLeaseId,
        slot_token: LiveRendererFrameSlotToken,
    },
    Shutdown,
}

struct WorkerResult {
    output: LiveRendererWorkerOutputKey,
    request_id: LiveRendererWorkerRequestId,
    correlation: LiveRendererFrameCorrelation,
    context_status: NativeGbmRenderedScanoutContextStatus,
    persistent_render_stats: LiveNativePersistentRenderStats,
    composition_nonzero_rgb_pixels: usize,
    outcome: WorkerOutcome,
}

struct WorkerMaintenanceResult {
    result: Result<usize, LiveRendererScanoutBufferExportDetail>,
    persistent_render_stats: LiveNativePersistentRenderStats,
}

struct WorkerRestoreImageResult {
    result: Result<bool, LiveRendererScanoutBufferExportDetail>,
    persistent_render_stats: LiveNativePersistentRenderStats,
}

enum WorkerOutcome {
    Exported {
        descriptor: LiveRendererScanoutBufferDescriptor,
        lease_id: LiveRendererWorkerLeaseId,
        slot_token: LiveRendererFrameSlotToken,
    },
    Deferred(PendingRenderedFrame),
    Failed(LiveRendererScanoutBufferExportDetail),
}

fn inventory_replacement_result(
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

fn reduce_worker_command_send_error<T>(
    error: TrySendError<T>,
) -> LiveRendererScanoutBufferExportDetail {
    match error {
        TrySendError::Full(_) => LiveRendererScanoutBufferExportDetail::WorkerQueueFull,
        TrySendError::Disconnected(_) => LiveRendererScanoutBufferExportDetail::WorkerDisconnected,
    }
}

fn reduce_worker_maintenance_receive_error(
    error: RecvTimeoutError,
) -> LiveRendererScanoutBufferExportDetail {
    match error {
        RecvTimeoutError::Timeout => LiveRendererScanoutBufferExportDetail::WorkerStalled,
        RecvTimeoutError::Disconnected => LiveRendererScanoutBufferExportDetail::WorkerDisconnected,
    }
}

fn pending_frame_kind_name(frame: &PendingRenderedFrame) -> &'static str {
    match frame {
        PendingRenderedFrame::Cpu { .. } => "cpu",
        PendingRenderedFrame::DmaBuf(_) => "dmabuf",
        PendingRenderedFrame::Mixed(..) => "mixed",
    }
}

fn trace_worker_request(request: LiveRendererWorkerRequestId) -> bool {
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

fn worker_outcome_name(outcome: &WorkerOutcome) -> &'static str {
    match outcome {
        WorkerOutcome::Exported { .. } => "exported",
        WorkerOutcome::Deferred(_) => "deferred",
        WorkerOutcome::Failed(_) => "failed",
    }
}

// Textual include rather than a module: slot damage is worker-private state
// whose helpers read like part of this file, and the split exists for file
// size rather than for a boundary.
include!("worker/damage.rs");
mod service;
#[path = "../../../../tests/support/renderer_worker.rs"]
mod tests;

use service::run_worker;
