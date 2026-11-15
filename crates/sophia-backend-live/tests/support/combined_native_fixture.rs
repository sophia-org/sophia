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
// G3-5d d-1: the same fixture on a running worker with the deterministic seam.
#[path = "combined_native_running.rs"]
mod running;
pub use running::CombinedSeen;
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
    /// served only when a test calls `serve_combined`. None on a running
    /// worker (G3-5d d-1), whose own thread serves the route.
    pub(crate) renderer: Option<crate::StubRenderer>,
    /// The occurrences the running seam read, in order (G3-5d d-1).
    seen: std::sync::Arc<std::sync::Mutex<Vec<CombinedSeen>>>,
    /// Each output's native service on a running worker; empty on the stub.
    native: Vec<running::NativeService>,
    /// The running worker's core, for arming its renderer gate (G3-5d
    /// d-2b2). None on the stub.
    core: Option<std::sync::Arc<crate::NativeGbmRendererWorkerCore>>,
    /// The declared composition fault, if any (Q1).
    pub(crate) compose_fault: Option<&'static str>,
    /// Compositions through the bridge entry.
    pub(crate) composes: std::cell::Cell<usize>,
    /// The next settlement's declared fault, if any.
    pub(crate) settle_fault: Option<CombinedSettleFault>,
    /// Declared plan refusals still to inject.
    pub(crate) plan_refusals: usize,
    /// G3-5d d-2b3d-2: the native frames the last `queue_retained` admitted,
    /// per output (the F a declared completion must name).
    last_retained_admission: Option<Frames>,
    /// Declared plan/description mismatches still to inject (G3-5d d-2b3b1).
    plan_mismatches: usize,
    /// Plans a declared mismatch widened, each with a placeholder present.
    plan_mismatches_applied: usize,
    /// The running seam's declared one-shot draw failure (G3-5d d-2b3b1).
    render_fault: std::sync::Arc<
        std::sync::Mutex<Option<sophia_renderer_live::LiveRendererScanoutBufferExportDetail>>,
    >,
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
    /// G3-5d d-2b3e: consumption checks run so far, and the last one's own
    /// result, recorded where it runs (`check_combined_accept`); the
    /// preflight is not recorded. None until one ran.
    checks: std::cell::Cell<usize>,
    last_check: std::cell::Cell<Option<Result<(), crate::CombinedAcceptRefusal>>>,
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
    /// The head is disabled, as a topology change leaves it; its exporter
    /// and the facade it issued stay (G3-5d d-2b3c MD1). No other topology
    /// work runs.
    Disabled,
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
        Self::on_core(outputs, &core, Some(renderer))
    }

    /// One enabled head per output, each with an exporter on `core`.
    fn on_core(
        outputs: &[sophia_engine::HeadlessOutput],
        core: &std::sync::Arc<crate::NativeGbmRendererWorkerCore>,
        renderer: Option<crate::StubRenderer>,
    ) -> Self {
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
                exporter.attach_shared_worker(core);
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
            seen: Default::default(),
            native: Vec::new(),
            core: None,
            compose_fault: None,
            composes: std::cell::Cell::new(0),
            settle_fault: None,
            plan_refusals: 0,
            last_retained_admission: None,
            plan_mismatches: 0,
            plan_mismatches_applied: 0,
            render_fault: Default::default(),
            reserve_attempts: 0,
            classify_fault: None,
            classifies: std::cell::Cell::new(0),
            release_attempts: 0,
            release_transfers: 0,
            accepts: 0,
            classification: None,
            ordinary_queues: 0,
            installs: 0,
            checks: std::cell::Cell::new(0),
            last_check: std::cell::Cell::new(None),
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
        // A running worker takes the registration itself.
        if let Some(renderer) = self.renderer.as_mut() {
            let key = crate::LiveRendererWorkerOutputKey::from_raw(output.raw());
            renderer.register(key, &head.facade);
        }
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
        let planned = self
            .routes()
            .plan(composition, head, frame, admitted, deadline);
        match planned {
            Some((mut plan, dispatch)) if self.plan_mismatches > 0 => {
                self.plan_mismatches -= 1;
                if plan.widen_first_placeholder() {
                    self.plan_mismatches_applied += 1;
                }
                Some((plan, dispatch))
            }
            planned => planned,
        }
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
            CombinedDeclaredHead::Disabled => head.enabled = false,
        }
    }

    /// G3-5d d-0, a declared owner sequence through real exporter calls: on
    /// `output`, after reserve, an identified ordinary frame is queued, its
    /// capture requested, and the frame discarded, as preview recovery's
    /// discard leaves it. Both capture flags stay. This fixture has no
    /// CaptureOwner endpoint; its guards read the exporter and queue facts.
    pub fn capture_left_by_discard(&mut self, output: OutputId) {
        let index = self
            .heads
            .iter()
            .position(|head| head.output.id == output)
            .expect("a fixture head serves the output");
        let head = &self.heads[index];
        let identity = self
            .native_owner
            .frame(output, head.head, head.target_generation, 999);
        let reservation = sophia_renderer_live::LiveSnapshotReservation::for_image(16, 16).unwrap();
        let exporter = &mut self.exporters[index];
        exporter.set_pending_identified_mixed_frame(Default::default(), Some(identity));
        assert!(exporter.request_pending_frame_capture(identity, reservation));
        assert!(exporter.discard_pending_frame(), "the queued frame goes");
        assert!(exporter.combined_capture_pending(), "the flags stay");
    }

    /// G3-5d d-2b3e form 2: `capture_left_by_discard` without its discard, a
    /// declared fixture state: an identified ordinary frame queued on
    /// `output`'s exporter with its capture requested, both still present.
    pub fn capture_queued_on_pending_frame(&mut self, output: OutputId) {
        let index = self.head_index(output);
        let head = &self.heads[index];
        let identity = self
            .native_owner
            .frame(output, head.head, head.target_generation, 999);
        let reservation = sophia_renderer_live::LiveSnapshotReservation::for_image(16, 16).unwrap();
        let exporter = &mut self.exporters[index];
        exporter.set_pending_identified_mixed_frame(Default::default(), Some(identity));
        assert!(exporter.request_pending_frame_capture(identity, reservation));
        assert!(exporter.pending_mixed_frame(), "the frame is queued");
        assert!(
            exporter.combined_capture_pending(),
            "its capture is requested"
        );
    }

    /// `output`'s exporter has queued mixed work. Not the worker's in-flight
    /// state, which `pending_frame` would include. Reads only.
    pub fn pending_mixed_frame(&self, output: OutputId) -> bool {
        self.exporters[self.head_index(output)].pending_mixed_frame()
    }

    /// `output`'s exporter has a capture requested or reserved. Reads only.
    pub fn capture_pending(&self, output: OutputId) -> bool {
        self.exporters[self.head_index(output)].combined_capture_pending()
    }

    /// The consumption checks run so far and the last one's result: None
    /// when none ran, `Some(Ok(()))` when it accepted, `Some(Err(refusal))`
    /// when it refused. Reads only; runs no guard.
    pub fn check_observation(&self) -> (usize, Option<Result<(), crate::CombinedAcceptRefusal>>) {
        (self.checks.get(), self.last_check.get())
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

    /// `output`'s head is enabled (G3-5d d-2b3c MD1). Reads only.
    pub fn head_enabled(&self, output: OutputId) -> bool {
        self.heads[self.head_index(output)].enabled
    }

    /// The Release receipt of `output`'s exporter as (admission ticket,
    /// job), read without consuming it (G3-5d d-2b3c). The output's exporter
    /// whatever its head's state; the caller compares the ticket with the
    /// issuing identity it recorded.
    pub fn combined_receipt(&self, output: OutputId) -> Option<(u64, u64)> {
        self.exporters[self.head_index(output)].combined_receipt_for_control()
    }

    /// Labelled native-only service (G3-5d d-2b3c MD2): one ordinary
    /// `poll()` of `output`'s worker, as the owner's export service reaches
    /// it between owner visits. No Session call, collection or walk runs.
    /// True when the poll found nothing in flight.
    pub fn ordinary_poll(&mut self, output: OutputId) -> bool {
        let index = self.head_index(output);
        self.exporters[index].poll_worker_for_control()
    }

    fn head_index(&self, output: OutputId) -> usize {
        self.heads
            .iter()
            .position(|head| head.output.id == output)
            .expect("a fixture head serves the output")
    }

    /// Declared fault: the next plan is a refused send, as a plan that no
    /// longer matches its composition would be.
    pub fn declare_plan_refusal(&mut self) {
        self.plan_refusals += 1;
    }

    /// Labelled fault (G3-5d d-2b3b1): the next real plan, after the real
    /// planning ran, has its first placeholder widened by one pixel, so the
    /// worker's admission check disagrees with the job's real description.
    /// Once; the real admission, bytes and worker path are unchanged.
    pub fn declare_plan_mismatch(&mut self) {
        self.plan_mismatches += 1;
    }

    /// Plans a declared mismatch widened (each had a placeholder), and the
    /// declarations not yet consumed.
    pub fn plan_mismatches(&self) -> (usize, usize) {
        (self.plan_mismatches_applied, self.plan_mismatches)
    }

    /// The next settlement takes `fault`, once.
    pub fn declare_settle_fault(&mut self, fault: CombinedSettleFault) {
        self.settle_fault = Some(fault);
    }

    // The renderer's turn, for Session's tests: the real route decides
    // Stopped and Expired, `reply` is the injected render otherwise.

    fn stub(&mut self) -> &mut crate::StubRenderer {
        self.renderer
            .as_mut()
            .expect("a stub-core fixture: a running worker serves itself")
    }

    /// Serves the one forwarded combined frame on `output` at `at`.
    pub fn serve_combined(
        &mut self,
        output: OutputId,
        at: std::time::Instant,
        reply: crate::StubReply,
    ) {
        let key = crate::LiveRendererWorkerOutputKey::from_raw(output.raw());
        self.stub().serve(key, at, reply);
    }

    /// Parks the one forwarded combined frame until `serve_combined`, so a
    /// stop can land between forwarding and render.
    pub fn park_combined(&mut self) -> bool {
        self.stub().park()
    }

    /// Release commands the renderer has received so far; this also drains
    /// `fill_renderer_queue`'s filler.
    pub fn renderer_releases(&mut self) -> usize {
        self.stub().releases()
    }

    /// Fills the renderer's command queue, so the next Release enqueue sees
    /// `Full`, until `renderer_releases` drains it.
    pub fn fill_renderer_queue(&mut self) {
        self.stub().fill();
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

    /// Test-support read (G3-5d d-2b3b1): `queue_ready`, for a Session
    /// precondition on an ordinary entry. Reads only.
    pub fn ordinary_queue_ready(&self, output: OutputId) -> bool {
        self.queue_ready(output)
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
        let frames = self.admit(
            batches,
            required,
            LiveProductionHeadCompositionContent::Retained,
        )?;
        self.last_retained_admission = Some(frames.clone());
        Ok(frames)
    }

    /// G3-5d d-2b3d-2: what the last retained queueing admitted, read only.
    pub fn last_retained_admission(&self) -> Option<&Frames> {
        self.last_retained_admission.as_ref()
    }

    /// G3-5d d-2b3d-2: `output`'s presented native frame, if any (the
    /// target's `presented_frame_id`), read only.
    pub fn presented_retained(&self, output: OutputId) -> Option<LiveProductionNativeFrameId> {
        self.heads[self.head_index(output)]
            .presented_content
            .map(crate::LiveProductionScanoutContent::frame)
    }

    /// G3-5d d-2b3d-2 F4-O, a SIMULATED completion of the exact queued
    /// fallback on `output` (design REV3 D4-O, REV4 D4-O): takes the one
    /// queued retained generation out of the deferred queue and walks the
    /// head's engine presentation state pending, submitted, presented, then
    /// records its content as presented. Omitted, because this fixture has
    /// no device: the owner's `activate_deferred_mirror_generation`, the
    /// exporter's render, `PersistentScanoutCustody::submit`, the DRM
    /// atomic commit, the page-flip event, `custody.present`, cursor plane
    /// and capture. Every check runs read-only before the first write; a
    /// refused preflight returns `Err` with the queue and head untouched,
    /// and no fallible exit follows the first write. Returns F, the frame
    /// `queue_retained` admitted.
    pub fn present_queued_retained(
        &mut self,
        output: OutputId,
    ) -> Result<LiveProductionNativeFrameId, String> {
        let index = self.head_index(output);
        let queued = self
            .deferred
            .get(output)
            .ok_or("no queued generation for the output")?;
        let frame = queued.frame;
        if queued.heads.len() != 1 {
            return Err(format!("{} queued heads, not one", queued.heads.len()));
        }
        let head_frame = &queued.heads[0];
        if head_frame.head_index != index {
            return Err(format!(
                "queued head index {} is not the output's {index}",
                head_frame.head_index
            ));
        }
        if head_frame.identity.output() != output || head_frame.identity.frame() != frame.raw() {
            return Err(format!(
                "queued identity {:?} is not ({output:?}, {frame:?})",
                head_frame.identity
            ));
        }
        if head_frame.content.frame() != frame {
            return Err(format!(
                "queued content frame {:?} is not {frame:?}",
                head_frame.content.frame()
            ));
        }
        let content = head_frame.content;
        let snapshot = head_frame
            .output_damage_snapshot
            .clone()
            .ok_or("retained frame carries no damage snapshot")?;
        let head = &self.heads[index];
        if !head.enabled {
            return Err("the head is disabled".into());
        }
        if head.pending_content.is_some()
            || head.rendering_content.is_some()
            || head.submitted_content.is_some()
            || head.presented_content.is_some()
        {
            return Err("the head already holds content".into());
        }
        if head.scanout_custody.submitted().is_some() {
            return Err("the head's custody holds a submission".into());
        }
        if head.output_frames.pending().is_some()
            || head.output_frames.rendering().is_some()
            || head.output_frames.submitted().is_some()
        {
            return Err("the head's presentation state is not idle".into());
        }
        head.output_frames
            .presentation_for(snapshot.clone())
            .map_err(|error| format!("the snapshot would not queue: {error:?}"))?;
        // Ownership: an idle head reduces to Install, so the exact queued
        // generation leaves the queue. Under this exclusive borrow and that
        // reducer no other generation can be taken: an invariant, not a
        // recoverable case (source-01 review).
        let Some(taken) = self.deferred.take_ready(output, None, None, None) else {
            return Err("take_ready returned nothing after preflight".into());
        };
        assert_eq!(
            taken.frame, frame,
            "preflighted: take_ready returns the exact queued generation"
        );
        // Writes, none fallible after the preflight above.
        let head = &mut self.heads[index];
        head.output_frames
            .queue(snapshot)
            .expect("preflighted: the snapshot queues");
        head.output_frames
            .mark_submitted()
            .expect("preflighted: pending becomes submitted");
        head.output_frames
            .mark_presented()
            .expect("preflighted: submitted becomes presented");
        head.presented_content = Some(content);
        Ok(frame)
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
        let result = super::accept::combined_consumption_guards(
            self.native_owner,
            &self.heads,
            &self.exporters,
            &self.deferred,
            // No capture endpoint here: the exporter and queue facts are read.
            &|_| false,
            head,
            accept,
        )
        .map(|_| ());
        // The observation (d-2b3e): the result as computed, returned unchanged.
        self.checks.set(self.checks.get() + 1);
        self.last_check.set(Some(result));
        result
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
            &|_| false,
            head,
            accept,
        )
    }

    fn commit_combined_accept(&mut self, head: &CombinedHead, plan: CombinedAcceptPlan) {
        self.accepts += 1;
        self.routes().commit_accept(head, plan);
    }

    fn frame_owner(&self) -> crate::NativeFrameOwner {
        self.native_owner
    }

    fn head_targets(&self, output: OutputId) -> Vec<sophia_engine::HeadRenderTarget> {
        native_facts::head_render_targets(&self.heads, output)
    }
}
