#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
mod shutdown;
mod worker_export;

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
use super::LiveRenderedScanoutBufferExport;
#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
use crate::api::*;

use super::worker::LiveRendererWorkerOutputKey;
use super::{
    NativeGbmRendererWorker, NativeGbmRendererWorkerCore, NativeGbmRendererWorkerScanoutLease,
    WorkerPoll,
};
#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
use sophia_renderer_live::{
    LiveCpuComposedFrame, LiveRendererScanoutBufferExportDetail,
    LiveRendererScanoutBufferExportStatus, NativeGbmOwnedScanoutBuffer,
    NativeGbmRenderedScanoutContext, NativeGbmRenderedScanoutContextStatus,
};

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
pub(crate) enum PendingRenderedFrame {
    Cpu {
        native: Option<super::LiveNativeFrameIdentity>,
        frame: LiveCpuComposedFrame,
        checksum: u64,
        damage_snapshot: Option<sophia_engine::OutputFrameDamageSnapshot>,
    },
    DmaBuf(sophia_renderer_live::LiveOwnedDmaBufFrame),
    Mixed(
        sophia_renderer_live::LiveOwnedMixedCompositionFrame,
        Option<super::LiveNativeFrameIdentity>,
    ),
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
pub struct NativeGbmRenderedScanoutBufferDiscoveryExporter<R>
where
    R: RenderDeviceDiscoveryBackend,
{
    discovery: R,
    pub(super) layout_probe: super::layout_probe::ExporterLayoutProbe,
    /// Which output this exporter speaks for. It names the worker's reply
    /// route and, inside a shared render context, this output's own target
    /// slots -- the inline path uses it for the latter alone.
    pub(super) output: LiveRendererWorkerOutputKey,
    context: Option<NativeGbmRenderedScanoutContext<R::Device>>,
    worker: Option<NativeGbmRendererWorker>,
    worker_frame_kind: Option<PendingRenderedFrameKind>,
    context_status: Option<NativeGbmRenderedScanoutContextStatus>,
    context_open_attempts: usize,
    export_attempts: usize,
    preferred_modifiers: Vec<u64>,
    pub(super) last_target: Option<LiveGbmEglFrameTargetRecord>,
    last_target_lifecycle: Option<LiveGbmEglFrameTargetLifecycleReport>,
    pub(super) last_export_status: Option<LiveRendererScanoutBufferExportStatus>,
    pub(super) pending_frame: Option<PendingRenderedFrame>,
    pub(super) frame_offered_at: Option<std::time::Instant>,
    pub(super) cost: crate::DirectScanoutCost,
    direct_cpu_bootstrap_armed: bool,
    direct_cpu_bootstrap_attempts: usize,
    direct_cpu_bootstrap_exports: usize,
    cpu_frame_export_attempts: usize,
    dmabuf_frame_export_attempts: usize,
    dmabuf_frame_exports: usize,
    mixed_frame_export_attempts: usize,
    mixed_frame_exports: usize,
    last_cpu_frame_checksum: Option<u64>,
    last_cpu_frame_export_status: Option<LiveRendererScanoutBufferExportStatus>,
    /// Whether this output may hand a client buffer straight to the plane.
    ///
    /// Off unless the session enabled it. A disabled exporter never even
    /// derives a candidate, so the flag's off state is the pre-row behaviour
    /// exactly, not a different path that happens to compose.
    pub(super) direct_scanout_enabled: bool,
    /// How many lowered frames carried each verdict, in `VERDICTS` order.
    direct_scanout_verdicts: [usize; sophia_engine::DirectScanoutVerdict::COUNT],
    /// Whether this output has already said what its geometry refusal
    /// measured. One line is the diagnostic; one per frame is noise.
    pub(super) direct_scanout_geometry_reported: bool,
    /// The composed form of a frame handed out directly, kept until the
    /// submission that took it says whether it reached a screen.
    ///
    /// This is the fallback ladder's whole mechanism. A direct attempt that
    /// the driver refuses does not lose its frame: the composed form is still
    /// here, and `fall_back_from_direct` reinstalls it as pending with its
    /// proof cleared, so the retry composes instead of refusing again.
    pub(super) direct_fallback: Option<(
        sophia_renderer_live::LiveOwnedMixedCompositionFrame,
        Option<super::LiveNativeFrameIdentity>,
    )>,
    /// Whether the driver has already accepted a direct commit in this
    /// eligibility episode.
    ///
    /// Cleared by every export that is not direct, which is what makes the
    /// test happen on the composition-to-direct edge rather than once per
    /// session: an overlay opening composes one frame, and that alone means
    /// the next direct frame is validated afresh. See
    /// `PresentFlipOwnership.tla`, `ReProveAfterEpisodeChange`.
    pub(super) direct_scanout_tested: bool,
    pub(super) direct_scanout_attempts: usize,
    pub(super) direct_scanout_exports: usize,
    pub(super) direct_scanout_flips: usize,
    pub(super) direct_scanout_tests: usize,
    pub(super) direct_scanout_test_rejections: usize,
    pub(super) direct_scanout_refusals: usize,
    /// Attempts the backend declined for a reason of its own -- a format or
    /// plane layout it cannot use. Legitimate, and counted apart from a
    /// structural disagreement, which is a defect.
    pub(super) direct_scanout_unsupported: usize,
    pub(super) direct_scanout_fallbacks: usize,
    pub(super) last_direct_scanout_refusal: Option<sophia_renderer_live::LiveDirectScanoutRefusal>,
    /// Frames the latest-wins cell dropped without rendering them.
    ///
    /// Holding one newest frame is the point, so a supersession is ordinary
    /// backpressure rather than a fault. It was invisible: the cell is an
    /// `Option`, so a newer frame overwrote an older one silently and a
    /// returned deferred frame was discarded silently. Counting it is what
    /// distinguishes a session that kept one frame pending from one that never
    /// had a second frame to keep.
    pending_frame_supersessions: usize,
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[derive(Debug)]
pub enum NativeGbmRenderedScanoutOwner {
    Inline(NativeGbmOwnedScanoutBuffer),
    Worker(NativeGbmRendererWorkerScanoutLease),
    /// A client's own buffer, on its way to the plane uncomposed.
    ///
    /// Unlike the other two this owns no compositor memory: it holds the
    /// duplicated plane descriptors and nothing else. It is also the only
    /// variant whose buffer lives in another process, which is why it reports
    /// `shares_kms_drm_file() == false` and takes the PRIME transport -- the
    /// branch that imports descriptors into the KMS file rather than handing
    /// over a GEM handle that only the renderer's file knows.
    Direct(sophia_renderer_live::LiveDirectScanoutBuffer),
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PendingRenderedFrameKind {
    Cpu,
    DmaBuf,
    Mixed,
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl<R> NativeGbmRenderedScanoutBufferDiscoveryExporter<R>
where
    R: RenderDeviceDiscoveryBackend,
{
    pub fn new(discovery: R) -> Self {
        Self {
            discovery,
            layout_probe: super::layout_probe::ExporterLayoutProbe::default(),
            output: LiveRendererWorkerOutputKey::from_raw(0),
            context: None,
            worker: None,
            worker_frame_kind: None,
            context_status: None,
            context_open_attempts: 0,
            export_attempts: 0,
            preferred_modifiers: Vec::new(),
            last_target: None,
            last_target_lifecycle: None,
            last_export_status: None,
            pending_frame: None,
            frame_offered_at: None,
            cost: crate::DirectScanoutCost::default(),
            direct_cpu_bootstrap_armed: false,
            direct_cpu_bootstrap_attempts: 0,
            direct_cpu_bootstrap_exports: 0,
            cpu_frame_export_attempts: 0,
            dmabuf_frame_export_attempts: 0,
            dmabuf_frame_exports: 0,
            mixed_frame_export_attempts: 0,
            mixed_frame_exports: 0,
            last_cpu_frame_checksum: None,
            last_cpu_frame_export_status: None,
            direct_scanout_enabled: false,
            direct_scanout_verdicts: [0; sophia_engine::DirectScanoutVerdict::COUNT],
            direct_scanout_geometry_reported: false,
            direct_scanout_tested: false,
            direct_fallback: None,
            direct_scanout_attempts: 0,
            direct_scanout_exports: 0,
            direct_scanout_flips: 0,
            direct_scanout_tests: 0,
            direct_scanout_test_rejections: 0,
            direct_scanout_refusals: 0,
            direct_scanout_unsupported: 0,
            direct_scanout_fallbacks: 0,
            last_direct_scanout_refusal: None,
            pending_frame_supersessions: 0,
        }
    }

    pub fn new_worker(discovery: R) -> std::io::Result<Self>
    where
        R::Device: Send + 'static,
    {
        let device = discovery.open_render_device();
        let mut exporter = Self::new(discovery);
        let output = exporter.output;
        exporter.context_open_attempts = 1;
        exporter.worker = Some(NativeGbmRendererWorker::spawn(device, output)?);
        Ok(exporter)
    }

    /// Name the output this exporter speaks for.
    ///
    /// Set before a worker is attached. Two exporters sharing a core must not
    /// share a key: it is the only thing separating their replies, their
    /// slots, and their leases.
    pub fn set_output(&mut self, output: LiveRendererWorkerOutputKey) {
        self.invalidate_layout_probe();
        self.output = output;
    }

    pub const fn output(&self) -> LiveRendererWorkerOutputKey {
        self.output
    }

    pub fn enable_worker(&mut self) -> std::io::Result<()>
    where
        R::Device: Send + 'static,
    {
        self.enable_worker_with_image_import_devices(Vec::new())
    }

    pub fn enable_worker_with_image_import_devices(
        &mut self,
        import_devices: Vec<std::os::fd::OwnedFd>,
    ) -> std::io::Result<()>
    where
        R::Device: Send + 'static,
    {
        if self.worker.is_some() {
            return Ok(());
        }
        self.invalidate_layout_probe();
        self.context_open_attempts = self.context_open_attempts.saturating_add(1);
        let core = NativeGbmRendererWorkerCore::spawn_with_image_import_devices(
            self.discovery.open_render_device(),
            import_devices,
        )?;
        self.worker = Some(core.attach(self.output));
        self.context = None;
        self.context_status = None;
        Ok(())
    }

    /// Attach this output to a worker shared with the rest of its device
    /// group, rather than giving it a thread of its own.
    pub fn attach_shared_worker(&mut self, core: &std::sync::Arc<NativeGbmRendererWorkerCore>) {
        if self.worker.is_some() {
            return;
        }
        self.invalidate_layout_probe();
        self.context_open_attempts = self.context_open_attempts.saturating_add(1);
        self.worker = Some(core.attach(self.output));
        self.context = None;
        self.context_status = None;
    }

    pub fn request_image_import_device_replacement(
        &self,
        generation: u64,
        devices: Vec<std::os::fd::OwnedFd>,
    ) -> std::io::Result<()> {
        self.worker
            .as_ref()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "renderer worker is not enabled",
                )
            })?
            .request_image_import_device_replacement(generation, devices)
    }

    pub fn poll_image_import_device_replacement(&self) -> std::io::Result<Option<u64>> {
        self.worker
            .as_ref()
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "renderer worker is not enabled",
                )
            })?
            .poll_image_import_device_replacement()
    }

    /// Arms the next CPU export as a direct-GBM-only bootstrap.
    ///
    /// This is intentionally incompatible with either renderer owner. Mirror
    /// initialization must never create an inline EGL context and then replace
    /// it with a worker while its first scanout buffer remains displayed.
    pub fn arm_direct_cpu_bootstrap(&mut self) -> Result<(), &'static str> {
        if self.worker.is_some() || self.context.is_some() || self.direct_cpu_bootstrap_armed {
            return Err("direct CPU bootstrap requires an uninitialized renderer owner");
        }
        self.direct_cpu_bootstrap_armed = true;
        Ok(())
    }

    pub const fn worker_enabled(&self) -> bool {
        self.worker.is_some()
    }

    /// The submitted worker frame, independently of any newer pending frame.
    pub fn rendering_frame_correlation(&self) -> Option<super::LiveRendererFrameCorrelation> {
        self.worker
            .as_ref()
            .and_then(NativeGbmRendererWorker::in_flight_correlation)
    }

    pub const fn direct_cpu_bootstrap_attempts(&self) -> usize {
        self.direct_cpu_bootstrap_attempts
    }

    pub const fn direct_cpu_bootstrap_exports(&self) -> usize {
        self.direct_cpu_bootstrap_exports
    }

    pub fn with_preferred_modifiers(mut self, preferred_modifiers: impl Into<Vec<u64>>) -> Self {
        self.invalidate_layout_probe();
        self.preferred_modifiers = reduced_preferred_scanout_modifiers(preferred_modifiers.into());
        self
    }

    pub const fn context_open_attempts(&self) -> usize {
        self.context_open_attempts
    }

    pub const fn export_attempts(&self) -> usize {
        self.export_attempts
    }

    pub const fn last_export_status(&self) -> Option<LiveRendererScanoutBufferExportStatus> {
        self.last_export_status
    }

    pub const fn last_target(&self) -> Option<LiveGbmEglFrameTargetRecord> {
        self.last_target
    }

    pub const fn last_target_lifecycle(&self) -> Option<LiveGbmEglFrameTargetLifecycleReport> {
        self.last_target_lifecycle
    }

    pub const fn context_status(&self) -> Option<NativeGbmRenderedScanoutContextStatus> {
        self.context_status
    }

    pub const fn context_ready(&self) -> bool {
        self.context.is_some()
            || matches!(
                self.context_status,
                Some(NativeGbmRenderedScanoutContextStatus::Ready)
            )
    }

    pub fn persistent_render_stats(&self) -> sophia_renderer_live::LiveNativePersistentRenderStats {
        self.worker.as_ref().map_or_else(
            || {
                self.context.as_ref().map_or_else(
                    sophia_renderer_live::LiveNativePersistentRenderStats::default,
                    NativeGbmRenderedScanoutContext::persistent_render_stats,
                )
            },
            NativeGbmRendererWorker::persistent_render_stats,
        )
    }

    pub fn discovery(&self) -> &R {
        &self.discovery
    }

    pub fn discovery_mut(&mut self) -> &mut R {
        &mut self.discovery
    }

    pub fn set_pending_cpu_frame(&mut self, frame: LiveCpuComposedFrame) {
        let checksum = cpu_frame_checksum(&frame);
        self.set_pending_cpu_frame_with_checksum(frame, checksum);
    }

    pub fn set_pending_cpu_frame_with_checksum(
        &mut self,
        frame: LiveCpuComposedFrame,
        checksum: u64,
    ) {
        self.set_pending_cpu_frame_with_damage(frame, checksum, None);
    }

    pub fn set_pending_cpu_frame_with_damage(
        &mut self,
        frame: LiveCpuComposedFrame,
        checksum: u64,
        damage_snapshot: Option<sophia_engine::OutputFrameDamageSnapshot>,
    ) {
        self.set_pending_identified_cpu_frame(frame, checksum, damage_snapshot, None);
    }

    pub(crate) fn set_pending_identified_cpu_frame(
        &mut self,
        frame: LiveCpuComposedFrame,
        checksum: u64,
        damage_snapshot: Option<sophia_engine::OutputFrameDamageSnapshot>,
        native: Option<super::LiveNativeFrameIdentity>,
    ) {
        self.replace_pending_frame(PendingRenderedFrame::Cpu {
            native,
            frame,
            checksum,
            damage_snapshot,
        });
    }

    /// Install the newest frame, counting whatever it displaced.
    pub(super) fn replace_pending_frame(&mut self, frame: PendingRenderedFrame) {
        self.invalidate_layout_probe();
        self.requeue_pending_frame(frame);
    }

    pub(super) fn requeue_pending_frame(&mut self, frame: PendingRenderedFrame) {
        if self.pending_frame.is_some() {
            self.pending_frame_supersessions = self.pending_frame_supersessions.saturating_add(1);
        }
        // Superseding restamps: the cost of a frame is measured from the
        // content that actually reached the plane, not from one the scene
        // moved past while it waited.
        self.frame_offered_at = Some(std::time::Instant::now());
        self.pending_frame = Some(frame);
    }

    pub fn cost(&self) -> &crate::DirectScanoutCost {
        &self.cost
    }

    /// One line per direct-scanout episode transition.
    ///
    /// The whole episode is observable from these: which output, which scene
    /// generation, and every step from a proven frame through the validating
    /// commit to a flip or a fall back. A physical gate asserts the shape of
    /// the sequence rather than a single count, because the counts alone
    /// cannot say whether a flip happened before its test or a fallback lost
    /// its frame.
    pub const fn pending_frame_supersessions(&self) -> usize {
        self.pending_frame_supersessions
    }

    pub const fn pending_cpu_frame(&self) -> bool {
        matches!(self.pending_frame, Some(PendingRenderedFrame::Cpu { .. }))
    }

    pub fn set_pending_dmabuf_frame(&mut self, frame: sophia_renderer_live::LiveOwnedDmaBufFrame) {
        self.replace_pending_frame(PendingRenderedFrame::DmaBuf(frame));
    }

    pub const fn pending_dmabuf_frame(&self) -> bool {
        matches!(self.pending_frame, Some(PendingRenderedFrame::DmaBuf(_)))
    }

    pub fn set_pending_mixed_frame(
        &mut self,
        frame: sophia_renderer_live::LiveOwnedMixedCompositionFrame,
    ) {
        self.set_pending_identified_mixed_frame(frame, None);
    }

    pub(crate) fn set_pending_identified_mixed_frame(
        &mut self,
        frame: sophia_renderer_live::LiveOwnedMixedCompositionFrame,
        native: Option<super::LiveNativeFrameIdentity>,
    ) {
        // Counted here because this is the one place a lowered frame reaches
        // an exporter, and because zeros in the direct-scanout counters cannot
        // say whether the path is off, the scene was never eligible, or the
        // proof was computed wrongly. The histogram says which.
        //
        // Frames whose verdict a mirror or topology path deliberately cleared
        // count as `composition_required`, which is what they are by then.
        self.direct_scanout_verdicts[frame.direct_scanout.reduced_index()] =
            self.direct_scanout_verdicts[frame.direct_scanout.reduced_index()].saturating_add(1);
        self.report_direct_scanout_geometry_refusal(&frame);
        self.replace_pending_frame(PendingRenderedFrame::Mixed(frame, native));
    }

    /// Whether this output has said what its geometry refusal measured.
    pub const fn direct_scanout_geometry_reported(&self) -> bool {
        self.direct_scanout_geometry_reported
    }

    /// How many lowered frames carried each verdict, indexed as
    /// `DirectScanoutVerdict::VERDICTS`.
    pub const fn direct_scanout_verdicts(
        &self,
    ) -> [usize; sophia_engine::DirectScanoutVerdict::COUNT] {
        self.direct_scanout_verdicts
    }

    pub const fn pending_mixed_frame(&self) -> bool {
        matches!(self.pending_frame, Some(PendingRenderedFrame::Mixed(..)))
    }

    pub const fn pending_frame(&self) -> bool {
        self.pending_frame.is_some()
            || matches!(self.worker.as_ref(), Some(worker) if worker.in_flight())
    }

    /// Discards work that has not crossed into the renderer worker.
    ///
    /// An in-flight command must still be polled so its resulting lease can be
    /// released. Returning false makes that ownership distinction explicit to
    /// topology-abort code.
    pub fn discard_pending_frame(&mut self) -> bool {
        self.invalidate_layout_probe();
        if self.worker_in_flight() {
            return false;
        }
        self.pending_frame.take().is_some()
    }

    pub const fn worker_in_flight(&self) -> bool {
        matches!(self.worker.as_ref(), Some(worker) if worker.in_flight())
    }

    pub const fn renderer_image_owner_initialized(&self) -> bool {
        self.worker.is_some() || self.context.is_some()
    }

    pub fn evict_renderer_image(
        &mut self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<bool, sophia_renderer_live::LiveRendererScanoutBufferExportDetail> {
        if let Some(worker) = &self.worker {
            return worker.evict_renderer_image(image_id);
        }
        self.context
            .as_mut()
            .map_or(Ok(false), |context| context.evict_renderer_image(image_id))
    }

    pub fn promote_renderer_image(
        &mut self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<bool, sophia_renderer_live::LiveRendererScanoutBufferExportDetail> {
        if let Some(worker) = &self.worker {
            return worker.promote_renderer_image(image_id);
        }
        self.context.as_mut().map_or(Ok(false), |context| {
            context.promote_renderer_image(image_id)
        })
    }

    pub fn rollback_renderer_image(
        &mut self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<bool, sophia_renderer_live::LiveRendererScanoutBufferExportDetail> {
        if let Some(worker) = &self.worker {
            return worker.rollback_renderer_image(image_id);
        }
        self.context.as_mut().map_or(Ok(false), |context| {
            context.rollback_renderer_image(image_id)
        })
    }

    pub fn export_promoted_renderer_image(
        &mut self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Result<
        Option<sophia_renderer_live::LiveRendererImageSnapshot>,
        sophia_renderer_live::LiveRendererScanoutBufferExportDetail,
    > {
        self.settle_worker_for_image_maintenance()?;
        if let Some(worker) = &mut self.worker {
            return worker.export_promoted_renderer_image(image_id);
        }
        self.context.as_ref().map_or(Ok(None), |context| {
            context.export_promoted_renderer_image(image_id)
        })
    }

    pub fn restore_promoted_renderer_image(
        &mut self,
        snapshot: sophia_renderer_live::LiveRendererImageSnapshot,
    ) -> Result<bool, sophia_renderer_live::LiveRendererScanoutBufferExportDetail> {
        if let Some(worker) = &mut self.worker {
            return worker.restore_promoted_renderer_image(snapshot);
        }
        self.context.as_mut().map_or(Ok(false), |context| {
            context.restore_promoted_renderer_image(snapshot)
        })
    }

    pub fn clear_renderer_images(
        &mut self,
    ) -> Result<usize, sophia_renderer_live::LiveRendererScanoutBufferExportDetail> {
        self.settle_worker_for_image_maintenance()?;
        if let Some(worker) = &mut self.worker {
            return worker.clear_renderer_images();
        }
        self.context.as_mut().map_or(
            Ok(0),
            sophia_renderer_live::NativeGbmRenderedScanoutContext::clear_renderer_images,
        )
    }

    fn settle_worker_for_image_maintenance(
        &mut self,
    ) -> Result<(), sophia_renderer_live::LiveRendererScanoutBufferExportDetail> {
        let Some(worker) = &mut self.worker else {
            return Ok(());
        };
        // Handoff or teardown has detached the skipped Present. Collect its
        // worker result before touching the older promoted image set.
        if worker.discard_in_flight_for_maintenance()? {
            self.worker_frame_kind = None;
            tracing::info!("sophia_renderer_worker schema=1 status=maintenance_frame_discarded");
        }
        Ok(())
    }

    pub const fn cpu_frame_export_attempts(&self) -> usize {
        self.cpu_frame_export_attempts
    }

    pub const fn dmabuf_frame_export_attempts(&self) -> usize {
        self.dmabuf_frame_export_attempts
    }

    pub const fn dmabuf_frame_exports(&self) -> usize {
        self.dmabuf_frame_exports
    }

    pub const fn mixed_frame_export_attempts(&self) -> usize {
        self.mixed_frame_export_attempts
    }

    pub const fn mixed_frame_exports(&self) -> usize {
        self.mixed_frame_exports
    }

    pub const fn last_cpu_frame_checksum(&self) -> Option<u64> {
        self.last_cpu_frame_checksum
    }

    pub const fn last_cpu_frame_export_status(
        &self,
    ) -> Option<LiveRendererScanoutBufferExportStatus> {
        self.last_cpu_frame_export_status
    }

    pub fn composition_nonzero_rgb_pixels(&self) -> usize {
        if let Some(worker) = &self.worker {
            return worker.composition_nonzero_rgb_pixels();
        }
        self.context.as_ref().map_or(0, |context| {
            context.composition_nonzero_rgb_pixels(self.output.target_set())
        })
    }
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl<R> NativeGbmRenderedScanoutBufferDiscoveryExporter<R>
where
    R: RenderDeviceDiscoveryBackend,
{
    pub(super) fn export_rendered_scanout_buffer_measured(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
    ) -> LiveRenderedScanoutBufferExport<NativeGbmRenderedScanoutOwner> {
        self.layout_probe
            .candidate
            .expire(std::time::Instant::now());
        if self.last_target.is_some_and(|previous| previous != target) {
            self.invalidate_layout_probe();
        }
        self.export_attempts = self.export_attempts.saturating_add(1);
        // Every export ends the eligibility episode unless it is itself
        // direct; the direct branch below restores this. Clearing first and
        // restoring in one place means a path added later cannot forget to
        // end the episode, only to continue it -- and forgetting to continue
        // costs one validating commit, while forgetting to end one costs a
        // flip the driver never agreed to.
        let continuing_episode = self.direct_scanout_tested;
        self.direct_scanout_tested = false;
        let target_lifecycle =
            LiveGbmEglFrameTargetLifecycleReport::from_size_update(self.last_target, target);
        self.last_target = Some(target);
        self.last_target_lifecycle = Some(target_lifecycle);

        if !target.is_valid_scanout_target() {
            self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::InvalidTarget);
            return LiveRenderedScanoutBufferExport::new(
                LiveRendererScanoutBufferExportStatus::InvalidTarget,
                LiveRendererScanoutBufferExportDetail::InvalidTarget,
                None,
                None,
            );
        }

        if self.direct_cpu_bootstrap_armed {
            self.direct_cpu_bootstrap_armed = false;
            self.direct_cpu_bootstrap_attempts =
                self.direct_cpu_bootstrap_attempts.saturating_add(1);
            let correlation = self
                .pending_frame
                .as_ref()
                .map(|frame| super::worker::frame_correlation(frame, None));
            let report = match self.pending_frame.take() {
                Some(PendingRenderedFrame::Cpu {
                    frame, checksum, ..
                }) => {
                    self.cpu_frame_export_attempts =
                        self.cpu_frame_export_attempts.saturating_add(1);
                    self.last_cpu_frame_checksum = Some(checksum);
                    sophia_renderer_live::NativeGbmScanoutBufferExporter::export_direct_cpu_owned_scanout_buffer_from_backend_device_result(
                        self.discovery.open_render_device(),
                        target,
                        &frame,
                    )
                }
                Some(frame) => {
                    self.pending_frame = Some(frame);
                    sophia_renderer_live::NativeGbmOwnedScanoutBufferExportReport::new(
                        LiveRendererScanoutBufferExportStatus::InvalidTarget,
                        LiveRendererScanoutBufferExportDetail::InvalidTarget,
                        None,
                    )
                }
                None => sophia_renderer_live::NativeGbmOwnedScanoutBufferExportReport::new(
                    LiveRendererScanoutBufferExportStatus::Degraded,
                    LiveRendererScanoutBufferExportDetail::RetainedBufferMissing,
                    None,
                ),
            };
            if report.status == LiveRendererScanoutBufferExportStatus::Exported {
                self.direct_cpu_bootstrap_exports =
                    self.direct_cpu_bootstrap_exports.saturating_add(1);
            }
            let descriptor = report.buffer.as_ref().map(|buffer| buffer.descriptor());
            self.last_cpu_frame_export_status = Some(report.status);
            self.last_export_status = Some(report.status);
            return LiveRenderedScanoutBufferExport::new(
                report.status,
                report.detail,
                descriptor,
                report.buffer.map(NativeGbmRenderedScanoutOwner::Inline),
            )
            .with_correlation(correlation);
        }

        // The direct path, ahead of both the worker and the inline context
        // because it needs neither: no render happens, no target slot is
        // acquired, and the buffer that reaches the plane is the client's.
        if let Some(export) = self.try_direct_scanout_export(target, continuing_episode) {
            return export;
        }

        if self.worker.is_some() {
            return self.export_from_worker(target);
        }

        if self.context.is_none() {
            self.context_open_attempts = self.context_open_attempts.saturating_add(1);
            let report = NativeGbmRenderedScanoutContext::from_backend_device_result(
                self.discovery.open_render_device(),
            );
            self.context_status = Some(report.status);
            self.context = report.context;
        }

        let Some(context) = &mut self.context else {
            let status = match self.context_status {
                Some(NativeGbmRenderedScanoutContextStatus::Degraded) => {
                    LiveRendererScanoutBufferExportStatus::Degraded
                }
                Some(NativeGbmRenderedScanoutContextStatus::Ready) => {
                    LiveRendererScanoutBufferExportStatus::Degraded
                }
                Some(NativeGbmRenderedScanoutContextStatus::Unavailable) | None => {
                    LiveRendererScanoutBufferExportStatus::Unavailable
                }
            };
            self.last_export_status = Some(status);
            return LiveRenderedScanoutBufferExport::new(
                status,
                LiveRendererScanoutBufferExportDetail::from_status(status),
                None,
                None,
            );
        };

        let frame = self.pending_frame.take();
        let correlation = frame
            .as_ref()
            .map(|frame| super::worker::frame_correlation(frame, None));
        let output_format = correlation.and_then(|correlation| {
            self.layout_probe
                .candidate
                .output_format(correlation, std::time::Instant::now())
        });
        if let Some(correlation) = correlation {
            self.layout_probe
                .candidate
                .bind(correlation, std::time::Instant::now());
        }
        let report = match frame {
            Some(PendingRenderedFrame::Mixed(frame, _)) => {
                self.mixed_frame_export_attempts =
                    self.mixed_frame_export_attempts.saturating_add(1);
                match context.export_owned_mixed_frame(
                    target,
                    &frame,
                    sophia_renderer_live::LiveCompositionOutputRequest {
                        preferred_modifiers: &self.preferred_modifiers,
                        format: output_format
                            .map(sophia_renderer_live::LiveCompositionFormatRequest::Preferred),
                    },
                ) {
                    Ok(report) => {
                        if report.status == LiveRendererScanoutBufferExportStatus::Exported {
                            self.mixed_frame_exports = self.mixed_frame_exports.saturating_add(1);
                        }
                        report
                    }
                    Err(sophia_renderer_live::LiveMixedCompositionError::Renderer(detail)) => {
                        sophia_renderer_live::NativeGbmOwnedScanoutBufferExportReport::new(
                            detail.status(),
                            detail,
                            None,
                        )
                    }
                    Err(_) => sophia_renderer_live::NativeGbmOwnedScanoutBufferExportReport::new(
                        LiveRendererScanoutBufferExportStatus::InvalidTarget,
                        LiveRendererScanoutBufferExportDetail::InvalidTarget,
                        None,
                    ),
                }
            }
            Some(PendingRenderedFrame::DmaBuf(frame)) => {
                self.dmabuf_frame_export_attempts =
                    self.dmabuf_frame_export_attempts.saturating_add(1);
                let report = context.export_dmabuf_owned_scanout_buffer_with_modifiers(
                    target,
                    frame.as_frame(),
                    &self.preferred_modifiers,
                );
                if report.status == LiveRendererScanoutBufferExportStatus::Exported {
                    self.dmabuf_frame_exports = self.dmabuf_frame_exports.saturating_add(1);
                }
                report
            }
            Some(PendingRenderedFrame::Cpu {
                frame, checksum, ..
            }) => {
                self.cpu_frame_export_attempts = self.cpu_frame_export_attempts.saturating_add(1);
                self.last_cpu_frame_checksum = Some(checksum);
                let report = context.export_xrgb8888_owned_scanout_buffer_with_modifiers(
                    target,
                    &frame,
                    &self.preferred_modifiers,
                );
                self.last_cpu_frame_export_status = Some(report.status);
                report
            }
            None => context.export_rendered_owned_scanout_buffer_with_modifiers(
                target,
                &self.preferred_modifiers,
            ),
        };
        let descriptor = report.buffer.as_ref().map(|buffer| buffer.descriptor());
        self.last_export_status = Some(report.status);
        LiveRenderedScanoutBufferExport::new(
            report.status,
            report.detail,
            descriptor,
            report.buffer.map(NativeGbmRenderedScanoutOwner::Inline),
        )
        .with_correlation(correlation)
    }
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
fn reduced_preferred_scanout_modifiers(mut modifiers: Vec<u64>) -> Vec<u64> {
    let mut reduced = Vec::new();
    for modifier in modifiers.drain(..) {
        if modifier == sophia_protocol::DRM_FORMAT_MOD_INVALID
            || modifier == u64::MAX
            || reduced.contains(&modifier)
        {
            continue;
        }
        reduced.push(modifier);
        if reduced.len() >= MAX_PREFERRED_SCANOUT_MODIFIERS {
            break;
        }
    }
    reduced
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
const MAX_PREFERRED_SCANOUT_MODIFIERS: usize = 16;

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
fn cpu_frame_checksum(frame: &LiveCpuComposedFrame) -> u64 {
    frame
        .bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3)
        })
}
