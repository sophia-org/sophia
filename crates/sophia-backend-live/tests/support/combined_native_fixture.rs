#![cfg(any(test, feature = "test-support"))]
//! G3-5a (GATE3-SCOPE G3-5; G3-5 design review DR1): one device-free native
//! authority for the Session content path. The fixture holds the owner's own
//! parts, heads, exporters, the deferred composition queue and the native
//! frame counter, and reaches them only through the owner's rules:
//! `native_facts` for head targets, readiness and retirement protection,
//! `prepare_native_composition_batch` and the queue's `admit_batch` for
//! ordinary work, and `CombinedRoutes` and the G3-4 accept for combined
//! work. Ordinary and combined frames draw from one counter.
//!
//! Not the owner: no KMS group or session, no topology transaction (frame
//! service is always available), no preview recovery, mirror lifecycle,
//! preview custody or topology cleanup, and no owner tick wrapper. Ordinary
//! queued generations are admitted, not rendered. The renderer is G3-4's
//! stub; its one render is the injected reply. No shell capture endpoint:
//! the owner facts read the queue and exporter captures only, and the
//! endpoint predicate is the owner's, checked under the capture feature.
//!
//! G3-5b Q1: `declare_compose_fault` replaces the result at the compose
//! boundary with an unclassified error after the real composition ran. It
//! proves the error's propagation and ownership, not that a real compositor
//! defect can reach it. `declare_settle_fault` is the same kind of declared
//! fault at settlement, once: a refusal that returns everything, or a credit
//! refused after the facade's real half. A retry that succeeds afterwards is
//! not evidence that a real identity mismatch heals.
//!
//! G3-5c: `declare_classify_fault` is the same kind of declared fault at the
//! consumption classification boundary. Three Release facts are counted
//! apart, and one is not evidence of another: `release_attempts` counts
//! calls into the facade's held release or flush, `release_transfers` the
//! Transferred answers among them (the enqueue the owner was told
//! succeeded), and `renderer_releases` the Release commands the renderer
//! side actually drained. `accepts` counts committed combined accepts, and
//! `ordinary_queues` the retained repaints ordinary installs queued.
//! `installs` counts entries into `install_shell_content` (F5), whatever
//! the setter then answers: an identical frame returns before any queue.
//!
//! G3-5c declared states, each labelled where a control uses it: a declared
//! classification replaces a real recomposition's class after it ran, and
//! `declare_head` sets a head as G3-4's refused-preflight controls set it.
//! They prove the consumption arm's disposition of that class or refusal,
//! not that Session state reaches it: with the authority check first, a
//! moved target or owner is refused before any classification, and the
//! backend's scene controls prove each class from real composition.
//!
//! Public under test-support and the combined feature for Session's tests,
//! which reach it only through `new` and `ShellContentNative`; its parts stay
//! crate-private.
use super::super::composition_admission::{
    NativeCompositionOutput, NativeHeadCompositionBatch, prepare_native_composition_batch,
};
use super::super::composition_queue::DeferredNativeCompositions;
use super::super::native_facts;
use super::super::renderer_images::LiveProductionHeadCompositionContent;
use super::*;

#[path = "combined_native_fixture_tests.rs"]
mod tests;
// G3-5b: the compose/plan bridge and the owner facts.
#[path = "combined_native_bridge_tests.rs"]
mod bridge_tests;

/// No render device: renders come from the deterministic boundary.
pub(crate) struct NoDevice;

impl RenderDeviceDiscoveryBackend for NoDevice {
    type Device = std::fs::File;

    fn open_render_device(&self) -> io::Result<std::fs::File> {
        Err(io::Error::other("no render device in the combined fixture"))
    }
}

type Frames = BTreeMap<OutputId, LiveProductionNativeFrameId>;

#[doc(hidden)]
pub struct CombinedNativeFixture {
    pub(crate) native_owner: crate::NativeFrameOwner,
    pub(crate) heads: Vec<LiveProductionNativeHead>,
    pub(crate) exporters: Vec<Exporter<NoDevice>>,
    pub(crate) deferred: DeferredNativeCompositions,
    /// The one native frame counter, as the owner's `next_frame_id`.
    pub(crate) next_frame: u64,
    /// The renderer side of the shared worker core: G3-4's stub, one route,
    /// served only when a test calls `serve_combined`, until G3-5d.
    pub(crate) renderer: crate::StubRenderer,
    /// The declared composition fault, if any (Q1).
    pub(crate) compose_fault: Option<&'static str>,
    /// Compositions through the bridge entry.
    pub(crate) composes: std::cell::Cell<usize>,
    /// The next settlement's declared fault, if any.
    pub(crate) settle_fault: Option<CombinedSettleFault>,
    /// Declared plan refusals still to inject.
    pub(crate) plan_refusals: usize,
    /// Renderer reservations attempted through the bridge, refused or not.
    pub(crate) reserve_attempts: usize,
    /// The declared consumption classification fault, if any (G3-5c).
    pub(crate) classify_fault: Option<&'static str>,
    /// Classifications through the consumption entry.
    pub(crate) classifies: std::cell::Cell<usize>,
    /// Held releases and flushes attempted through the facade.
    pub(crate) release_attempts: usize,
    /// Of those, the ones the facade answered Transferred.
    pub(crate) release_transfers: usize,
    /// Committed combined accepts.
    pub(crate) accepts: usize,
    /// The declared class every later consumption classification takes.
    pub(crate) classification: Option<crate::CombinedRecompose>,
    /// Retained repaints queued through this native target.
    pub(crate) ordinary_queues: usize,
    /// Entries into `install_shell_content`, before the runtime setter.
    pub(crate) installs: usize,
}

/// A declared head state for G3-5c's refused-consumption controls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CombinedDeclaredHead {
    /// An ordinary frame pending on the head.
    PendingOrdinary,
    /// Submitted content that owes a shell retirement.
    RetirementOwed,
    /// The native owner was replaced: no head issues the held result.
    OwnerReplaced,
}

/// A declared, one-shot settlement fault (G3-5b R2 refusal controls).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CombinedSettleFault {
    /// The facade refuses and returns the reservation and result unchanged;
    /// nothing is held or freed.
    Refused,
    /// The facade's real half runs, and the owed credit comes back refused
    /// as the validator would refuse it.
    Credit,
}

impl CombinedNativeFixture {
    /// One enabled head per output, each with an exporter on one shared
    /// worker core whose renderer is the stub.
    pub fn new(outputs: &[sophia_engine::HeadlessOutput]) -> Self {
        let (core, renderer) = crate::stub_core();
        let heads = outputs
            .iter()
            .zip(1u32..)
            .map(|(output, id)| {
                super::super::native_head_fixture::head_for(
                    id,
                    *output,
                    crate::LibdrmNativePlaneFormatCapabilities::parse(201, 0, &[]),
                )
            })
            .collect();
        let exporters = outputs
            .iter()
            .map(|output| {
                let mut exporter = Exporter::new(NoDevice);
                exporter.set_output(crate::LiveRendererWorkerOutputKey::from_raw(
                    output.id.raw(),
                ));
                exporter.attach_shared_worker(&core);
                exporter
            })
            .collect();
        Self {
            native_owner: crate::NativeFrameOwner::new(),
            heads,
            exporters,
            deferred: DeferredNativeCompositions::default(),
            next_frame: 1,
            renderer,
            compose_fault: None,
            composes: std::cell::Cell::new(0),
            settle_fault: None,
            plan_refusals: 0,
            reserve_attempts: 0,
            classify_fault: None,
            classifies: std::cell::Cell::new(0),
            release_attempts: 0,
            release_transfers: 0,
            accepts: 0,
            classification: None,
            ordinary_queues: 0,
            installs: 0,
        }
    }

    fn routes(&mut self) -> CombinedRoutes<'_, NoDevice> {
        CombinedRoutes {
            native_owner: self.native_owner,
            heads: &mut self.heads,
            exporters: &mut self.exporters,
        }
    }

    fn outputs(&self) -> BTreeSet<OutputId> {
        self.heads.iter().map(|head| head.output.id).collect()
    }

    // The owner's combined entries (combined_frame.rs), over these parts.

    pub(crate) fn allocate_combined_frame(&mut self) -> LiveProductionNativeFrameId {
        native_facts::allocate_frame_id(&mut self.next_frame)
    }

    pub(crate) fn reserve_combined_frame(
        &mut self,
        output: OutputId,
    ) -> io::Result<(CombinedHead, CombinedInbox<CombinedRenderPlan>)> {
        self.reserve_attempts += 1;
        let (head, inbox) = self.routes().reserve(output)?;
        // The facade queued its route registration in that call, on its
        // first reservation; the renderer side takes it now.
        let key = crate::LiveRendererWorkerOutputKey::from_raw(output.raw());
        self.renderer.register(key, &head.facade);
        Ok((head, inbox))
    }

    pub(crate) fn cancel_combined_frame(&mut self, head: &CombinedHead) -> io::Result<bool> {
        self.routes().cancel(head)
    }

    pub(crate) fn take_combined_frame_result(
        &mut self,
        head: &CombinedHead,
    ) -> io::Result<Option<CombinedRenderResult>> {
        self.routes().take(head)
    }

    pub(crate) fn settle_combined_frame(
        &mut self,
        head: &CombinedHead,
        validator: &mut IsbValidationWorker<CombinedRenderPlan>,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<ValidatedIsbPixels, CombinedSettleRefused> {
        match self.settle_fault.take() {
            None => self.routes().settle(head, validator, reservation, result),
            Some(CombinedSettleFault::Refused) => Err(CombinedSettleRefused::Refused(
                reservation,
                Box::new(result),
            )),
            Some(CombinedSettleFault::Credit) => {
                let (reservation, token, pixels) =
                    self.routes().settle_cell(head, reservation, result)?;
                Err(CombinedSettleRefused::Credit(reservation, token, pixels))
            }
        }
    }

    pub(crate) fn release_combined_held(
        &mut self,
        head: &CombinedHead,
        job: u64,
    ) -> CombinedRelease {
        let outcome = self.routes().release_held(head, job);
        self.count_release(outcome)
    }

    pub(crate) fn flush_combined_release(
        &mut self,
        head: &CombinedHead,
        job: u64,
    ) -> CombinedRelease {
        let outcome = self.routes().flush(head, job);
        self.count_release(outcome)
    }

    fn count_release(&mut self, outcome: CombinedRelease) -> CombinedRelease {
        self.release_attempts += 1;
        if outcome == CombinedRelease::Transferred {
            self.release_transfers += 1;
        }
        outcome
    }

    /// The owner's combined facts over these parts, with no capture
    /// endpoint.
    pub(crate) fn combined_owner_state(
        &self,
        output: OutputId,
    ) -> Option<crate::production_visual_runtime::CombinedOwnerState> {
        native_facts::combined_owner_state(
            &self.heads,
            &self.exporters,
            &self.deferred,
            output,
            false,
        )
    }

    pub(crate) fn plan_combined_frame(
        &mut self,
        composition: crate::CombinedComposition,
        head: &CombinedHead,
        frame: LiveProductionNativeFrameId,
        admitted: &sophia_protocol::ContentResourceBegin,
        deadline: std::time::Instant,
    ) -> Option<(CombinedRenderPlan, crate::CombinedDispatch)> {
        if self.plan_refusals > 0 {
            self.plan_refusals -= 1;
            drop(composition);
            return None;
        }
        self.routes()
            .plan(composition, head, frame, admitted, deadline)
    }

    /// Every later composition through the bridge returns `reason` as an
    /// unclassified error after the real one ran (Q1).
    pub fn declare_compose_fault(&mut self, reason: &'static str) {
        self.compose_fault = Some(reason);
    }

    /// Compositions through the bridge entry so far.
    pub fn composes(&self) -> usize {
        self.composes.get()
    }

    /// Renderer reservations attempted so far, refused ones included.
    pub fn reserve_attempts(&self) -> usize {
        self.reserve_attempts
    }

    /// Every later consumption classification returns `reason` as an
    /// unclassified error after the real recomposition ran (G3-5c).
    pub fn declare_classify_fault(&mut self, reason: &'static str) {
        self.classify_fault = Some(reason);
    }

    /// Consumption classifications so far.
    pub fn classifies(&self) -> usize {
        self.classifies.get()
    }

    /// Held releases and flushes attempted so far, whatever they answered.
    pub fn release_attempts(&self) -> usize {
        self.release_attempts
    }

    /// The attempts the facade answered Transferred. Not the renderer's
    /// receipt: see `renderer_releases`.
    pub fn release_transfers(&self) -> usize {
        self.release_transfers
    }

    /// Committed combined accepts so far.
    pub fn accepts(&self) -> usize {
        self.accepts
    }

    /// Declared: every later consumption classification takes `class`
    /// after the real recomposition ran (G3-5c controls 3 and 4).
    pub fn declare_classification(&mut self, class: crate::CombinedRecompose) {
        self.classification = Some(class);
    }

    /// Retained repaints queued through this target so far: ordinary
    /// installs that reached the native queue.
    pub fn ordinary_queues(&self) -> usize {
        self.ordinary_queues
    }

    /// Ordinary install entries so far, counted at the seam (G3-5c F5).
    pub fn installs(&self) -> usize {
        self.installs
    }

    /// Declared: `output`'s head takes `state`, as G3-4's refused-preflight
    /// controls set it (G3-5c control 9).
    pub fn declare_head(&mut self, output: OutputId, state: CombinedDeclaredHead) {
        let head = self
            .heads
            .iter_mut()
            .find(|head| head.output.id == output)
            .expect("a fixture head serves the output");
        match state {
            CombinedDeclaredHead::PendingOrdinary => {
                head.pending_content = Some(LiveProductionScanoutContent::HeadComposition {
                    frame: LiveProductionNativeFrameId::from_raw(99),
                    logical_content_checksum: 99,
                    nonzero_rgb_pixels: 0,
                });
            }
            CombinedDeclaredHead::RetirementOwed => {
                head.submitted_content = Some(LiveProductionScanoutContent::RetainedMixed {
                    frame: LiveProductionNativeFrameId::from_raw(98),
                    requires_retirement: true,
                    logical_content_checksum: None,
                    nonzero_rgb_pixels: 0,
                });
            }
            CombinedDeclaredHead::OwnerReplaced => {
                self.native_owner = crate::NativeFrameOwner::new();
            }
        }
    }

    /// `output`'s head as the accept guards read it: pending, rendering
    /// and submitted content, pending and rendering frames, and its target
    /// generation.
    pub fn head_observation(&self, output: OutputId) -> (bool, bool, bool, bool, bool, u64) {
        let head = self
            .heads
            .iter()
            .find(|head| head.output.id == output)
            .expect("a fixture head serves the output");
        (
            head.pending_content.is_some(),
            head.rendering_content.is_some(),
            head.submitted_content.is_some(),
            head.output_frames.pending().is_some(),
            head.output_frames.rendering().is_some(),
            head.target_generation,
        )
    }

    /// Declared fault: the next plan is a refused send, as a plan that no
    /// longer matches its composition would be.
    pub fn declare_plan_refusal(&mut self) {
        self.plan_refusals += 1;
    }

    /// The next settlement takes `fault`, once.
    pub fn declare_settle_fault(&mut self, fault: CombinedSettleFault) {
        self.settle_fault = Some(fault);
    }

    // The renderer's turn, for Session's tests: the real route decides
    // Stopped and Expired, `reply` is the injected render otherwise.

    /// Serves the one forwarded combined frame on `output` at `at`.
    pub fn serve_combined(
        &mut self,
        output: OutputId,
        at: std::time::Instant,
        reply: crate::StubReply,
    ) {
        let key = crate::LiveRendererWorkerOutputKey::from_raw(output.raw());
        self.renderer.serve(key, at, reply);
    }

    /// Parks the one forwarded combined frame until `serve_combined`, so a
    /// stop can land between forwarding and render.
    pub fn park_combined(&mut self) -> bool {
        self.renderer.park()
    }

    /// Release commands the renderer has received so far; this also drains
    /// `fill_renderer_queue`'s filler.
    pub fn renderer_releases(&mut self) -> usize {
        self.renderer.releases()
    }

    /// Fills the renderer's command queue, so the next Release enqueue sees
    /// `Full`, until `renderer_releases` drains it.
    pub fn fill_renderer_queue(&mut self) {
        self.renderer.fill();
    }

    // The owner's ordinary facts and admission, over the same parts.

    pub(crate) fn head_targets(&self, output: OutputId) -> Vec<sophia_engine::HeadRenderTarget> {
        native_facts::head_render_targets(&self.heads, output)
    }

    /// `frame_queue_ready` for a singleton output with no preview failure.
    pub(crate) fn queue_ready(&self, output: OutputId) -> bool {
        !native_facts::heads_pending_frame(&self.heads, &self.exporters, &self.deferred, output)
            && !native_facts::heads_in_flight(&self.heads, output)
            && !native_facts::heads_cleanup_pending(&self.heads, output)
    }

    /// `output_retirement_protected`: the queue's or an installed head's.
    pub(crate) fn protected(&self, output: OutputId) -> bool {
        native_facts::output_retirement_protected(&self.heads, &self.deferred, output)
    }

    /// `queue_retained_output_head_composition_frames_requiring_retirement`.
    pub(crate) fn queue_retained(
        &mut self,
        batches: NativeHeadCompositionBatch,
        required: &BTreeSet<OutputId>,
    ) -> Result<Frames, Box<dyn std::error::Error>> {
        self.ordinary_queues += 1;
        let batch_outputs = batches
            .iter()
            .map(|(output, _)| *output)
            .collect::<BTreeSet<_>>();
        if !required.is_subset(&batch_outputs) {
            return Err("required retained retirement targets an absent output".into());
        }
        self.admit(
            batches,
            required,
            LiveProductionHeadCompositionContent::Retained,
        )
    }

    /// `queue_ordinary_head_composition_batch`.
    pub(crate) fn queue_ordinary(
        &mut self,
        batches: NativeHeadCompositionBatch,
    ) -> Result<Frames, Box<dyn std::error::Error>> {
        self.admit(
            batches,
            &BTreeSet::new(),
            LiveProductionHeadCompositionContent::OrdinaryScene,
        )
    }

    /// The owner's frame validation, batch rule and queue admission over this
    /// authority's facts (composition_admission.rs
    /// `prepare_and_admit_head_batch`, less preview recovery and custody).
    fn admit(
        &mut self,
        batches: NativeHeadCompositionBatch,
        required: &BTreeSet<OutputId>,
        content: LiveProductionHeadCompositionContent,
    ) -> Result<Frames, Box<dyn std::error::Error>> {
        for (output, frames) in &batches {
            super::super::renderer_images::validate_live_head_composition_frame_batch(
                *output,
                &self.head_targets(*output),
                frames,
            )?;
        }
        let states = self
            .outputs()
            .into_iter()
            .map(|output| {
                let (targets, newest) = native_facts::composition_targets(&self.heads, output);
                let state = NativeCompositionOutput {
                    targets,
                    ready: self.queue_ready(output),
                    protected: self.protected(output),
                    available: true,
                    newest,
                    settled_mirror_checksum: None,
                };
                (output, state)
            })
            .collect();
        let generations = prepare_native_composition_batch(
            batches,
            required,
            &states,
            self.native_owner,
            &mut self.next_frame,
            content,
        )
        .map_err(|(reason, _)| -> Box<dyn std::error::Error> { reason.into() })?;
        let configured = self.outputs();
        self.deferred
            .admit_batch(generations, &configured)
            .map_err(|(reason, _)| reason.into())
    }
}

impl CombinedAcceptNative for CombinedNativeFixture {
    type Head = CombinedHead;
    type Plan = CombinedAcceptPlan;

    fn frame_service_available(&self) -> bool {
        true
    }

    fn check_combined_accept(
        &self,
        head: &CombinedHead,
        accept: &CombinedAccept,
    ) -> Result<(), crate::CombinedAcceptRefusal> {
        super::accept::combined_consumption_guards(
            self.native_owner,
            &self.heads,
            &self.exporters,
            &self.deferred,
            head,
            accept,
        )
        .map(|_| ())
    }

    fn preflight_combined_accept(
        &self,
        head: &CombinedHead,
        accept: CombinedAccept,
    ) -> Result<CombinedAcceptPlan, crate::CombinedAcceptRefusal> {
        super::accept::preflight_accept(
            self.native_owner,
            &self.heads,
            &self.exporters,
            &self.deferred,
            head,
            accept,
        )
    }

    fn commit_combined_accept(&mut self, head: &CombinedHead, plan: CombinedAcceptPlan) {
        self.accepts += 1;
        self.routes().commit_accept(head, plan);
    }
}
