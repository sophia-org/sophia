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
                let mut head = super::super::native_head_fixture::head(
                    id,
                    output.id.raw(),
                    crate::LibdrmNativePlaneFormatCapabilities::parse(201, 0, &[]),
                );
                head.output = *output;
                head.output_frames = OutputFramePresentationState::new(*output).unwrap();
                head
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
        self.routes().release_held(head, job)
    }

    pub(crate) fn flush_combined_release(
        &mut self,
        head: &CombinedHead,
        job: u64,
    ) -> CombinedRelease {
        self.routes().flush(head, job)
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
        self.routes().commit_accept(head, plan);
    }
}
