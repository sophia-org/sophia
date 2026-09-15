// The direct scanout path: a client's own buffer to the plane, uncomposed.
//
// Split from the exporter that owns it because it is a distinct decision with
// its own state -- an eligibility episode, a fallback held against a refusal,
// and its own evidence -- and because the exporter's other paths all end in a
// render while this one exists precisely to avoid that.
//
// Model: `validation/tla/PresentFlipOwnership.tla`.

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
use super::{
    LiveRenderedScanoutBufferExport, NativeGbmRenderedScanoutBufferDiscoveryExporter,
    NativeGbmRenderedScanoutOwner, discovery::PendingRenderedFrame,
};
#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
use crate::RenderDeviceDiscoveryBackend;
#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
use sophia_renderer_live::{
    LiveGbmEglFrameTargetRecord, LiveRendererScanoutBufferExportDetail,
    LiveRendererScanoutBufferExportStatus,
};

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
impl<R> NativeGbmRenderedScanoutBufferDiscoveryExporter<R>
where
    R: RenderDeviceDiscoveryBackend,
{
    /// Hand this output's pending frame straight to the plane, if it may be.
    ///
    /// Returns `None` when the frame is not a direct candidate at all, which
    /// leaves it pending for the ordinary composed path; a refusal derived
    /// from the frame's own layers also returns `None`, having first cleared
    /// the proof so the same frame composes rather than being re-derived and
    /// re-refused on every pass after it.
    pub(super) fn try_direct_scanout_export(
        &mut self,
        target: LiveGbmEglFrameTargetRecord,
        continuing_episode: bool,
    ) -> Option<LiveRenderedScanoutBufferExport<NativeGbmRenderedScanoutOwner>> {
        // The direct path, ahead of both the worker and the inline context
        // because it needs neither: no render happens, no target slot is
        // acquired, and the buffer that reaches the plane is the client's.
        //
        // The Engine proof is necessary but never sufficient. The structural
        // re-derivation below reads the lowered layers themselves, and a
        // disagreement between the two is a refusal, not a flip: the cost of
        // refusing wrongly is one composed frame, and the cost of flipping
        // wrongly is the wrong image on someone's screen.
        if !self.direct_scanout_enabled
            || !matches!(
                self.pending_frame,
                Some(PendingRenderedFrame::Mixed(ref frame, _)) if frame.direct_scanout.is_eligible()
            )
        {
            return None;
        }
        {
            let Some(PendingRenderedFrame::Mixed(mut frame, native)) = self.pending_frame.take()
            else {
                unreachable!("the match above admitted only an eligible mixed frame")
            };
            self.direct_scanout_attempts = self.direct_scanout_attempts.saturating_add(1);
            // The candidate identity, so a gate can bind one episode's steps
            // to one scene rather than to whatever happened next.
            let generation = frame.trace.map_or(0, |trace| trace.scene_generation);
            match frame.direct_scanout_buffer(target.size) {
                Ok(buffer) => {
                    self.direct_scanout_exports = self.direct_scanout_exports.saturating_add(1);
                    self.record_direct_scanout_episode("exported", generation, "none");
                    let descriptor = buffer.descriptor;
                    let correlation = super::LiveRendererFrameCorrelation {
                        native,
                        request: None,
                        trace: frame.trace,
                        direct_scanout: Some(frame.direct_scanout),
                    };
                    // Keep the composed form. Nothing has reached a screen yet
                    // -- the driver has not been asked -- and if it refuses,
                    // this is the frame that gets composed instead.
                    self.direct_fallback = Some((frame, native));
                    self.direct_scanout_tested = continuing_episode;
                    self.last_export_status = Some(LiveRendererScanoutBufferExportStatus::Exported);
                    return Some(
                        LiveRenderedScanoutBufferExport::new(
                            LiveRendererScanoutBufferExportStatus::Exported,
                            LiveRendererScanoutBufferExportDetail::from_status(
                                LiveRendererScanoutBufferExportStatus::Exported,
                            ),
                            Some(descriptor),
                            Some(NativeGbmRenderedScanoutOwner::Direct(buffer)),
                        )
                        .with_correlation(Some(correlation)),
                    );
                }
                Err(refusal) => {
                    // Counted apart: a structural disagreement means Engine's
                    // proof and the pixels contradict each other, while a
                    // format the plane cannot take is the backend answering a
                    // question Engine never asked.
                    if refusal.is_structural_disagreement() {
                        self.direct_scanout_refusals =
                            self.direct_scanout_refusals.saturating_add(1);
                    } else {
                        self.direct_scanout_unsupported =
                            self.direct_scanout_unsupported.saturating_add(1);
                    }
                    self.last_direct_scanout_refusal = Some(refusal);
                    self.record_direct_scanout_episode(
                        "refused",
                        generation,
                        refusal.reduced_name(),
                    );
                    // Clear the proof before reinstalling, so the frame that
                    // falls through composes rather than arriving here again
                    // and being refused for the same reason every frame.
                    frame.direct_scanout =
                        sophia_engine::DirectScanoutVerdict::CompositionRequired("refused");
                    self.pending_frame = Some(PendingRenderedFrame::Mixed(frame, native));
                }
            }
        }
        None
    }

    /// Say what a geometry refusal actually measured, once per output.
    ///
    /// The histogram gives the category and the category is not the answer: a
    /// client that is not the head's size could be off by a scrollbar or by a
    /// monitor, and knowing which is the difference between a fix and another
    /// physical run. Once, because the same client refuses the same way every
    /// frame and a per-frame line would bury the session log.
    pub(super) fn report_direct_scanout_geometry_refusal(
        &mut self,
        frame: &sophia_renderer_live::LiveOwnedMixedCompositionFrame,
    ) {
        use sophia_engine::DirectScanoutVerdict as Verdict;

        if self.direct_scanout_geometry_reported
            || !matches!(
                frame.direct_scanout,
                Verdict::LayerOffset
                    | Verdict::LayerNotHeadSized
                    | Verdict::LayerClipped
                    | Verdict::CompositionRequired(_)
            )
        {
            return;
        }
        let Some(target) = self.last_target else {
            return;
        };
        let Some(sophia_renderer_live::LiveOwnedMixedCompositionLayer::DmaBuf {
            placement, ..
        }) = frame.layers.first()
        else {
            return;
        };
        self.direct_scanout_geometry_reported = true;
        // Which command painted, when one did. "Composition required" covers
        // every painting primitive, and a letterbox that should have been
        // empty and an indicator strip drawn on purpose need different work.
        let command = match frame.direct_scanout {
            Verdict::CompositionRequired(command) => command,
            _ => "none",
        };
        tracing::info!(
            "sophia_live_direct_scanout_geometry schema=2 status={} command={command} output={} head_width={} head_height={} layer_x={} layer_y={} layer_width={} layer_height={}",
            frame.direct_scanout.reduced_name(),
            self.output.raw(),
            target.size.width,
            target.size.height,
            placement.target.x,
            placement.target.y,
            placement.target.width,
            placement.target.height,
        );
    }

    /// The scene generation of the direct attempt still outstanding, so every
    /// step of one episode names the same scene rather than only its first.
    pub(super) fn outstanding_direct_generation(&self) -> u64 {
        self.outstanding_direct_scene_generation().unwrap_or(0)
    }

    /// The scene a direct attempt is outstanding for, or `None` when none is.
    pub fn outstanding_direct_scene_generation(&self) -> Option<u64> {
        self.direct_fallback
            .as_ref()
            .and_then(|(frame, _)| frame.trace)
            .map(|trace| trace.scene_generation)
    }

    pub(super) fn record_direct_scanout_episode(
        &self,
        status: &str,
        generation: u64,
        reason: &str,
    ) {
        tracing::info!(
            "sophia_live_direct_scanout schema=1 status={status} output={} scene_generation={generation} reason={reason}",
            self.output.raw(),
        );
    }

    /// Whether this output may hand an eligible client buffer to the plane.
    ///
    /// Set rather than latched, because the answer changes: a head that joins
    /// a mirror group must lose the direct path, and a head that leaves one
    /// may regain it. Turning it off also ends any episode in progress, so a
    /// head that returns to eligibility is validated again rather than
    /// flipping on a test taken when it was somebody else's clone.
    pub fn set_direct_scanout_enabled(&mut self, enabled: bool) {
        self.direct_scanout_enabled = enabled;
        if !enabled {
            self.invalidate_layout_probe();
            self.direct_scanout_tested = false;
        }
    }

    pub const fn direct_scanout_enabled(&self) -> bool {
        self.direct_scanout_enabled
    }

    pub const fn direct_scanout_attempts(&self) -> usize {
        self.direct_scanout_attempts
    }

    pub const fn direct_scanout_exports(&self) -> usize {
        self.direct_scanout_exports
    }

    pub const fn direct_scanout_tests(&self) -> usize {
        self.direct_scanout_tests
    }

    pub const fn direct_scanout_test_rejections(&self) -> usize {
        self.direct_scanout_test_rejections
    }

    pub const fn direct_scanout_refusals(&self) -> usize {
        self.direct_scanout_refusals
    }

    pub const fn direct_scanout_unsupported(&self) -> usize {
        self.direct_scanout_unsupported
    }

    pub const fn direct_scanout_fallbacks(&self) -> usize {
        self.direct_scanout_fallbacks
    }

    pub const fn last_direct_scanout_refusal(
        &self,
    ) -> Option<sophia_renderer_live::LiveDirectScanoutRefusal> {
        self.last_direct_scanout_refusal
    }

    /// Whether a direct export is outstanding -- handed to a submission that
    /// has not yet said whether the driver took it.
    pub const fn direct_scanout_outstanding(&self) -> bool {
        self.direct_fallback.is_some()
    }

    /// The driver took the direct buffer. Drop the composed form.
    ///
    /// The client's buffer itself is not released here and must not be: it is
    /// what the screen is scanning until a successor flip retires it. Only the
    /// compositor-side copy that was never used is dropped.
    /// See `PresentFlipOwnership.tla`, `DisplayedClientBufferIsNeverReleased`.
    pub fn commit_direct_scanout(&mut self) {
        self.invalidate_layout_probe();
        self.direct_fallback = None;
        self.direct_scanout_flips = self.direct_scanout_flips.saturating_add(1);
    }

    /// How many times a client's own buffer went to this output's plane.
    ///
    /// Read before and after a tick to learn whether the submission that tick
    /// produced was direct -- the same before/after idiom the pixel-proof and
    /// export counters already use here, and the reason this is a running
    /// count rather than a flag that a later path could forget to clear.
    pub const fn direct_scanout_flips(&self) -> usize {
        self.direct_scanout_flips
    }

    /// The direct attempt did not reach a screen. Compose the same content.
    ///
    /// Returns whether a frame was reinstated. The reinstated frame carries no
    /// proof, so it takes the ordinary composed path and cannot be refused
    /// again for the same reason -- which is what keeps a refusal from
    /// becoming a loop, and what keeps it off the terminal submit-failure
    /// path entirely. See `PresentFlipOwnership.tla`, `CommitRefused`.
    pub fn fall_back_from_direct(&mut self) -> bool {
        let Some((mut frame, native)) = self.direct_fallback.take() else {
            return false;
        };
        frame.direct_scanout = sophia_engine::DirectScanoutVerdict::CompositionRequired("refused");
        self.direct_scanout_fallbacks = self.direct_scanout_fallbacks.saturating_add(1);
        self.requeue_pending_frame(PendingRenderedFrame::Mixed(frame, native));
        true
    }
}
