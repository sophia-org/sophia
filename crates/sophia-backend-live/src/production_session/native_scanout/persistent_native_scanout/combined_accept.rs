//! The two-phase accept of a held combined result on the native scanout
//! (AUTHORITY-STEP3 F2, R5; GATE3-SCOPE G3-4). The preflight reads only. The
//! commit then writes the head and the issuing facade, with no fallible step.
use super::*;
use crate::CombinedAcceptRefusal;

#[path = "../../../../tests/support/combined_frame_accept.rs"]
mod accept_tests;

/// What the owner froze for one combined frame at dispatch, accepted now.
pub struct CombinedAccept {
    /// The runtime job of the admission whose result the facade holds.
    pub job: u64,
    /// The native frame the plan was composed as.
    pub frame: LiveProductionNativeFrameId,
    /// The plan's logical content checksum.
    pub checksum: u64,
    /// The plan's damage snapshot: it becomes the output's rendering frame.
    pub snapshot: sophia_engine::OutputFrameDamageSnapshot,
    /// The correlation the renderer was given; `poll()` checks the reply
    /// against it.
    pub correlation: crate::LiveRendererFrameCorrelation,
    pub output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
}

/// A passed native preflight. Only the preflight makes one, and the commit
/// in the same owner turn consumes it.
pub struct CombinedAcceptPlan {
    index: usize,
    job: u64,
    content: LiveProductionScanoutContent,
    checksum: u64,
    presentation: sophia_engine::OutputFramePresentation,
    correlation: crate::LiveRendererFrameCorrelation,
    output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
}

/// The native half of a combined accept, as the visual runtime drives it.
/// The head and plan are the native owner's own types; the runtime only
/// passes them on.
pub(crate) trait CombinedAcceptNative {
    type Head;
    type Plan;
    /// The setter's native gate: whether frame service is available now.
    fn frame_service_available(&self) -> bool;
    /// The preflight's guards without the presentation (G3-5c C1). Reads
    /// only, with the preflight's refusal precedence.
    fn check_combined_accept(
        &self,
        head: &Self::Head,
        accept: &CombinedAccept,
    ) -> Result<(), CombinedAcceptRefusal>;
    fn preflight_combined_accept(
        &self,
        head: &Self::Head,
        accept: CombinedAccept,
    ) -> Result<Self::Plan, CombinedAcceptRefusal>;
    fn commit_combined_accept(&mut self, head: &Self::Head, plan: Self::Plan);
}

/// The content a combined shell frame installs: retained content owing the
/// shell retirement the visual runtime binds to `frame`, as ordinary
/// admission builds it for an output whose retirement is required.
fn combined_shell_content(
    frame: LiveProductionNativeFrameId,
    checksum: u64,
) -> LiveProductionScanoutContent {
    LiveProductionScanoutContent::RetainedMixed {
        frame,
        requires_retirement: true,
        logical_content_checksum: Some(checksum),
        nonzero_rgb_pixels: 0,
    }
}

/// The checks of F2's phase 2 that precede the presentation, in their
/// order: the issuer, the facade's own refusal, topology, the target
/// generation, the head's pending and rendering content, retirement
/// protection, identity, and the head's pending and rendering frames. Reads
/// only. G3-5c runs them at consumption before any classification, so a
/// scene change cannot excuse their refusal (C1); the accept's preflight
/// runs them again. Answers the issuing head's index.
pub(super) fn combined_consumption_guards<R: RenderDeviceDiscoveryBackend>(
    native_owner: crate::NativeFrameOwner,
    heads: &[LiveProductionNativeHead],
    exporters: &[Exporter<R>],
    deferred: &super::super::composition_queue::DeferredNativeCompositions,
    head: &CombinedHead,
    accept: &CombinedAccept,
) -> Result<usize, CombinedAcceptRefusal> {
    use CombinedAcceptRefusal as Refusal;
    if head.native_owner != native_owner {
        return Err(Refusal::NotIssuing);
    }
    let index = exporters
        .iter()
        .zip(heads)
        .position(|(exporter, current)| {
            current.head == head.head && exporter.issued_combined(&head.facade)
        })
        .ok_or(Refusal::NotIssuing)?;
    if let Some(refusal) = exporters[index].combined_accept_refusal(&head.facade, accept.job) {
        return Err(refusal);
    }
    let current = &heads[index];
    let output = accept.snapshot.output.id;
    let serving = heads
        .iter()
        .filter(|other| other.enabled && other.output.id == output)
        .count();
    if !current.enabled || current.output.id != output || serving != 1 {
        return Err(Refusal::Topology);
    }
    if current.target_generation != head.target_generation {
        return Err(Refusal::TargetGeneration);
    }
    if current.pending_content.is_some() {
        return Err(Refusal::HeadPendingContent);
    }
    if current.rendering_content.is_some() {
        return Err(Refusal::HeadRenderingContent);
    }
    // The output-level fact ordinary admission uses: the one serving head's
    // content, or a queued generation that owes a retirement.
    if current.retirement_protected() || deferred.protected(output) {
        return Err(Refusal::RetirementProtected);
    }
    let expected = native_owner.frame(
        output,
        current.head,
        current.target_generation,
        accept.frame.raw(),
    );
    if accept.correlation.request != Some(crate::COMBINED_REQUEST)
        || accept.correlation.native != Some(expected)
    {
        return Err(Refusal::Identity);
    }
    if current.output_frames.pending().is_some() {
        return Err(Refusal::PendingFrame);
    }
    if current.output_frames.rendering().is_some() {
        return Err(Refusal::RenderingTransition);
    }
    Ok(index)
}

/// F2's phase 2 with R5's additions, over the owner's heads, exporters and
/// deferred compositions: the guards, then the presentation. Reads only. On
/// a refusal the held result stays for ordinary disposal.
pub(super) fn preflight_accept<R: RenderDeviceDiscoveryBackend>(
    native_owner: crate::NativeFrameOwner,
    heads: &[LiveProductionNativeHead],
    exporters: &[Exporter<R>],
    deferred: &super::super::composition_queue::DeferredNativeCompositions,
    head: &CombinedHead,
    accept: CombinedAccept,
) -> Result<CombinedAcceptPlan, CombinedAcceptRefusal> {
    let index =
        combined_consumption_guards(native_owner, heads, exporters, deferred, head, &accept)?;
    let presentation = heads[index]
        .output_frames
        .presentation_for(accept.snapshot)
        .map_err(|_| CombinedAcceptRefusal::PendingTransition)?;
    Ok(CombinedAcceptPlan {
        index,
        job: accept.job,
        content: combined_shell_content(accept.frame, accept.checksum),
        checksum: accept.checksum,
        presentation,
        correlation: accept.correlation,
        output_format: accept.output_format,
    })
}

impl<R: RenderDeviceDiscoveryBackend> CombinedRoutes<'_, R> {
    /// F2's commit steps 2 to 4, after both preflights passed in this owner
    /// turn. Every check ran there, so no step here can fail.
    pub(super) fn commit_accept(&mut self, head: &CombinedHead, plan: CombinedAcceptPlan) {
        let current = &mut self.heads[plan.index];
        // Step 2, as composition installation writes it.
        current.rendering_content = Some(plan.content);
        current.last_checksum = plan.checksum;
        // Step 3: pending, then rendering, with the plan's damage snapshot.
        current.output_frames.install_pending(plan.presentation);
        current
            .output_frames
            .mark_rendering()
            .expect("the accept preflight found no rendering frame");
        // Step 4: the held result goes in flight on its issuing facade.
        self.exporters[plan.index].accept_combined_held(
            &head.facade,
            plan.job,
            plan.correlation,
            plan.output_format,
        );
    }
}

impl CombinedAcceptNative for LiveProductionNativeScanout {
    type Head = CombinedHead;
    type Plan = CombinedAcceptPlan;

    fn frame_service_available(&self) -> bool {
        self.output_topology_allows_frame_service()
    }

    fn check_combined_accept(
        &self,
        head: &CombinedHead,
        accept: &CombinedAccept,
    ) -> Result<(), CombinedAcceptRefusal> {
        combined_consumption_guards(
            self.native_frame_owner,
            &self.heads,
            &self.exporters,
            &self.deferred_mirror_generations,
            head,
            accept,
        )
        .map(|_| ())
    }

    fn preflight_combined_accept(
        &self,
        head: &CombinedHead,
        accept: CombinedAccept,
    ) -> Result<CombinedAcceptPlan, CombinedAcceptRefusal> {
        preflight_accept(
            self.native_frame_owner,
            &self.heads,
            &self.exporters,
            &self.deferred_mirror_generations,
            head,
            accept,
        )
    }

    fn commit_combined_accept(&mut self, head: &CombinedHead, plan: CombinedAcceptPlan) {
        self.combined_routes().commit_accept(head, plan);
    }
}
