//! The renderer half of the combined frame job (BRIDGE.md 4c-2): one bounded
//! inbox and one bounded result route per output, and at most one forwarded
//! frame parked until that output has a free slot.

use super::*;
#[path = "combined_render.rs"]
mod render;
pub use render::CombinedPlaceholder;
pub(super) use render::{CombinedSlots, RenderInput, RenderTarget, attempt};
// Named by the worker's tests; production reaches them through attempt.
#[cfg_attr(not(test), allow(unused_imports))]
pub(super) use render::{admits, bind};
use sophia_runtime::{
    CombinedInbox, CombinedReservation, CombinedToken, ForwardedValidation, IsbValidationWorker,
    ValidatedIsbPixels,
};

/// A settlement that did not complete. Nothing is lost in any variant.
pub enum CombinedSettleRefused {
    /// No live facade issued this job. Nothing changed; the row waits for
    /// qualified retirement.
    RendererNotFound(CombinedReservation, Box<CombinedRenderResult>),
    /// The issuing facade refused a foreign token, another admission's
    /// result or a foreign output. Nothing changed.
    Refused(CombinedReservation, Box<CombinedRenderResult>),
    /// The facade took its part (an exported result is `Held`); the validator
    /// refused the credit, which the caller still holds with the pixels.
    Credit(CombinedReservation, CombinedToken, ValidatedIsbPixels),
}

/// The issuing facade and the admission it granted. The claim is created per
/// attach, so a replacement facade under the same output key never matches.
pub(crate) struct CombinedFacade {
    image_store: u64,
    claim: Arc<OutputClaim>,
    ticket: u64,
}

impl CombinedFacade {
    fn admission(&self, job: u64) -> CombinedAdmission {
        CombinedAdmission {
            ticket: self.ticket,
            job,
        }
    }

    /// The issuing facade's claim, for the accept fixture's stub renderer.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn claim(&self) -> &Arc<OutputClaim> {
        &self.claim
    }
}

/// Why a held combined result cannot be accepted now (AUTHORITY-STEP3 F2,
/// R5). Every check reads only: on a refusal nothing has changed, and the
/// held result stays for the owner's ordinary disposal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CombinedAcceptRefusal {
    /// No live facade of this native owner and head issued the admission.
    NotIssuing,
    /// The facade does not hold this admission's result.
    NotHeld,
    Quarantined,
    Stalled,
    /// An ordinary render is in flight on this facade.
    OrdinaryInFlight,
    /// An ordinary frame was queued during validation: the head is busy
    /// (R4), never a scene-only fallback.
    OrdinaryPending,
    HeadPendingContent,
    HeadRenderingContent,
    /// Content on this head, or a queued generation for its output, still
    /// owes a retirement.
    RetirementProtected,
    /// The head no longer serves the admitted output alone.
    Topology,
    TargetGeneration,
    /// A capture may read the output's next frame (G3-5d d-0): the owner's
    /// capture endpoint, a queued generation's reservation, or the serving
    /// exporter's request or reservation, read in the consuming owner call.
    CaptureActive,
    /// The expected correlation does not name this owner, head, target and
    /// native frame as the combined request.
    Identity,
    /// The output refuses the plan's damage snapshot as its pending frame.
    PendingTransition,
    /// The output already has a pending frame.
    PendingFrame,
    /// The output already has a rendering frame: mark-rendering would fail.
    RenderingTransition,
}

/// Owner-numbered requests start at 1, so this never names one in flight.
pub(crate) const COMBINED_REQUEST: LiveRendererWorkerRequestId = LiveRendererWorkerRequestId(0);

/// What the owner froze for the renderer at dispatch; opaque to the runtime.
pub struct CombinedRenderPlan {
    pub(super) deadline: Instant,
    pub(super) target: LiveGbmEglFrameTargetRecord,
    /// Missing every placeholder's layer: private, so only a successful
    /// bind can make it renderable.
    frame: PendingRenderedFrame,
    pub(super) preferred_modifiers: Vec<u64>,
    pub(super) output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
    /// One per lowered occurrence of the copied resource, in final layer
    /// order. Empty when the frame shows none of it.
    placeholders: Box<[CombinedPlaceholder]>,
}

impl CombinedRenderPlan {
    // First production caller is the 4c-3 owner join.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(
        deadline: Instant,
        target: LiveGbmEglFrameTargetRecord,
        frame: PendingRenderedFrame,
        preferred_modifiers: Vec<u64>,
        output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
        placeholders: Box<[CombinedPlaceholder]>,
    ) -> Self {
        Self {
            deadline,
            target,
            frame,
            preferred_modifiers,
            output_format,
            placeholders,
        }
    }

    /// The frame's correlation, for the result: metadata only, so no caller
    /// outside the binding ever holds the unbound frame.
    pub(crate) fn correlation(&self) -> LiveRendererFrameCorrelation {
        frame_correlation(&self.frame, Some(COMBINED_REQUEST))
    }

    /// The target and request without the frame, which drops here unbound.
    fn render_target(self) -> RenderTarget {
        RenderTarget {
            target: self.target,
            preferred_modifiers: self.preferred_modifiers,
            output_format: self.output_format,
        }
    }
}

// Read by the 4c-3 owner join; until then only tests inspect results.
// Moved once per frame through a one-slot channel; boxing the result would
// add an allocation per frame on the path this lane measures.
#[cfg_attr(not(test), allow(dead_code))]
#[allow(clippy::large_enum_variant)]
pub(super) enum CombinedOutcome {
    Rendered(WorkerResult),
    Expired,
    /// The owner stopped the job before a slot was taken.
    Stopped,
    /// The pixels or placeholders disagree with the job's admitted
    /// description; nothing was bound or rendered.
    Mismatch,
}

/// What became of a combined job, readable before settlement consumes the
/// result. Settling the credit never implies the pixels rendered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CombinedDisposition {
    /// The render exported a frame: the one outcome settlement holds as the
    /// combined result (G3-5b R2).
    Rendered,
    /// The render ran and did not export; nothing is held.
    RenderFailed,
    Expired,
    Stopped,
    /// For the owner to publish Malformed through failure reconciliation.
    Mismatch,
}

/// What one render attempt did. The pixels always come back with it.
#[allow(clippy::large_enum_variant)]
pub(super) enum RenderAttempt {
    Done(CombinedOutcome, ValidatedIsbPixels),
    /// No free slot: the unbound plan and the pixels come back to be parked.
    NoSlot(CombinedRenderPlan, ValidatedIsbPixels),
}

/// The token and pixels always come home, whatever happened to the render.
#[cfg_attr(not(test), allow(dead_code))]
pub struct CombinedRenderResult {
    pub(super) token: CombinedToken,
    pub(super) pixels: ValidatedIsbPixels,
    pub(super) outcome: CombinedOutcome,
}

impl CombinedRenderResult {
    pub fn disposition(&self) -> CombinedDisposition {
        match &self.outcome {
            CombinedOutcome::Rendered(rendered)
                if matches!(rendered.outcome, WorkerOutcome::Exported { .. }) =>
            {
                CombinedDisposition::Rendered
            }
            CombinedOutcome::Rendered(_) => CombinedDisposition::RenderFailed,
            CombinedOutcome::Expired => CombinedDisposition::Expired,
            CombinedOutcome::Stopped => CombinedDisposition::Stopped,
            CombinedOutcome::Mismatch => CombinedDisposition::Mismatch,
        }
    }
}

/// Sent once after `Register`; carries the worker's halves of both routes.
pub(super) struct Registration {
    output: LiveRendererWorkerOutputKey,
    claim: Arc<OutputClaim>,
    inbox: Receiver<ForwardedValidation<CombinedRenderPlan>>,
    results: sophia_wake::SignalSender<CombinedRenderResult>,
}

/// The facade's halves, registered on first use so a facade that never
/// takes combined work sends nothing; inboxes are handed out per submission.
pub(super) struct FacadeRoute {
    inbox: SyncSender<ForwardedValidation<CombinedRenderPlan>>,
    ring: sophia_wake::WakeSlot,
    results: Receiver<CombinedRenderResult>,
}

pub(super) fn register(
    core: &NativeGbmRendererWorkerCore,
    output: LiveRendererWorkerOutputKey,
    claim: &Arc<OutputClaim>,
) -> Option<FacadeRoute> {
    let (inbox, inbox_receiver) = sync_channel(1);
    let (results, result_receiver) = sync_channel(1);
    core.command_sender
        .try_send(WorkerCommand::RegisterCombined(Registration {
            output,
            claim: Arc::clone(claim),
            inbox: inbox_receiver,
            results: sophia_wake::SignalSender::new(results, core.control.result_wake.clone()),
        }))
        .ok()?;
    Some(FacadeRoute {
        inbox,
        ring: core.control.command_wake.clone(),
        results: result_receiver,
    })
}

impl NativeGbmRendererWorker {
    /// `None` when this facade cannot take combined work: quarantined,
    /// detached, or its registration could not be queued.
    pub(super) fn combined_inbox(&mut self) -> Option<CombinedInbox<CombinedRenderPlan>> {
        if self.quarantined || self.claim.is_detached() {
            return None;
        }
        if self.combined.is_none() {
            self.combined = register(&self.core, self.output, &self.claim);
        }
        let route = self.combined.as_ref()?;
        Some(CombinedInbox::new(route.inbox.clone(), route.ring.clone()))
    }

    /// Takes the Release cell and returns an inbox, or neither.
    pub(crate) fn reserve_combined_frame(
        &mut self,
    ) -> Option<(CombinedFacade, CombinedInbox<CombinedRenderPlan>)> {
        let ticket = self.reserve_combined_release()?;
        let Some(inbox) = self.combined_inbox() else {
            self.cancel_combined_release(ticket);
            return None;
        };
        let facade = CombinedFacade {
            image_store: self.core.image_store,
            claim: Arc::clone(&self.claim),
            ticket,
        };
        Some((facade, inbox))
    }

    pub(crate) fn issued(&self, facade: &CombinedFacade) -> bool {
        self.core.image_store == facade.image_store && Arc::ptr_eq(&self.claim, &facade.claim)
    }

    /// The validator refused the submission: no frame can come back. Frees
    /// only this admission's reservation.
    pub(crate) fn cancel_combined_frame(&mut self, facade: &CombinedFacade) -> bool {
        self.cancel_combined_release(facade.ticket)
    }

    /// Holds an exported lease, then clears the validation credit, in one
    /// call so no caller can clear the credit while the lease has no owner.
    /// The token is checked before the cell changes.
    pub(crate) fn settle_combined_frame(
        &mut self,
        facade: &CombinedFacade,
        validator: &mut IsbValidationWorker<CombinedRenderPlan>,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<ValidatedIsbPixels, CombinedSettleRefused> {
        let (reservation, token, pixels) =
            self.settle_combined_cell(facade, reservation, result)?;
        match validator.settle_combined_token(reservation, token) {
            Ok(()) => Ok(pixels),
            Err((reservation, token)) => {
                Err(CombinedSettleRefused::Credit(reservation, token, pixels))
            }
        }
    }

    /// The facade's half of settlement: the exported lease is held, or the
    /// cell frees; the credit is still owed. Callers: `settle_combined_frame`,
    /// and the combined fixture's declared credit fault, which returns the
    /// owed credit as refused.
    pub(crate) fn settle_combined_cell(
        &mut self,
        facade: &CombinedFacade,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
    ) -> Result<(CombinedReservation, CombinedToken, ValidatedIsbPixels), CombinedSettleRefused>
    {
        if !reservation.owns(&result.token) {
            return Err(CombinedSettleRefused::Refused(
                reservation,
                Box::new(result),
            ));
        }
        let CombinedRenderResult {
            token,
            pixels,
            outcome,
        } = result;
        let kept = match outcome {
            CombinedOutcome::Rendered(rendered)
                if matches!(rendered.outcome, WorkerOutcome::Exported { .. }) =>
            {
                match self.hold_combined_result(facade.ticket, reservation.job(), rendered) {
                    None => Ok(()),
                    Some(refused) => Err(CombinedOutcome::Rendered(refused)),
                }
            }
            // Nothing was exported: this admission's cell simply frees.
            outcome => {
                if self.cancel_combined_release(facade.ticket) {
                    Ok(())
                } else {
                    Err(outcome)
                }
            }
        };
        if let Err(outcome) = kept {
            return Err(CombinedSettleRefused::Refused(
                reservation,
                Box::new(CombinedRenderResult {
                    token,
                    pixels,
                    outcome,
                }),
            ));
        }
        Ok((reservation, token, pixels))
    }

    /// `None` for any admission but the one this facade is still waiting on,
    /// so an old handle cannot drain its successor's result.
    pub(crate) fn take_combined_frame(
        &self,
        facade: &CombinedFacade,
    ) -> Option<Result<CombinedRenderResult, TryRecvError>> {
        reserved(&self.combined_release, facade.ticket).then(|| self.take_combined_result())
    }

    /// The owner declined this admission's held result.
    pub(crate) fn release_combined_held(
        &mut self,
        facade: &CombinedFacade,
        job: u64,
    ) -> CombinedRelease {
        self.give_up_combined_held(facade.admission(job))
    }

    /// The facade's part of the accept preflight: reads only.
    pub(crate) fn combined_accept_refusal(
        &self,
        facade: &CombinedFacade,
        job: u64,
    ) -> Option<CombinedAcceptRefusal> {
        if self.quarantined {
            return Some(CombinedAcceptRefusal::Quarantined);
        }
        if self.stalled.is_some() {
            return Some(CombinedAcceptRefusal::Stalled);
        }
        if self.in_flight.is_some() {
            return Some(CombinedAcceptRefusal::OrdinaryInFlight);
        }
        match &self.combined_release {
            CombinedReleaseCell::Held { admission, .. } if *admission == facade.admission(job) => {
                None
            }
            _ => Some(CombinedAcceptRefusal::NotHeld),
        }
    }

    /// Accept step 4 (F2): the held result becomes this facade's one request
    /// in flight, as `COMBINED_REQUEST` with the owner's expected correlation
    /// and format, and the cell frees. `poll()` then checks it as the reply.
    /// The owner ran `combined_accept_refusal` earlier in this turn, so this
    /// cannot refuse.
    pub(crate) fn accept_combined_held(
        &mut self,
        facade: &CombinedFacade,
        job: u64,
        correlation: LiveRendererFrameCorrelation,
        output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
    ) {
        let CombinedReleaseCell::Held { admission, result } =
            std::mem::replace(&mut self.combined_release, CombinedReleaseCell::Free)
        else {
            unreachable!("the accept preflight found this admission held");
        };
        assert_eq!(
            admission,
            facade.admission(job),
            "accept preflighted another admission"
        );
        self.in_flight = Some(InFlightRequest {
            capture: false,
            request_id: COMBINED_REQUEST,
            correlation,
            output_format,
            submitted_at: Instant::now(),
            soft_stall_reported: false,
            ready: Some(result),
        });
    }

    pub(crate) fn flush_combined_release_for(
        &mut self,
        facade: &CombinedFacade,
        job: u64,
    ) -> CombinedRelease {
        self.combined_release_for(facade.admission(job))
    }

    pub(super) fn take_combined_result(&self) -> Result<CombinedRenderResult, TryRecvError> {
        self.combined
            .as_ref()
            .map_or(Err(TryRecvError::Disconnected), |route| {
                route.results.try_recv()
            })
    }
}

/// The worker's per-output combined state.
pub(super) struct OutputRoute {
    inbox: Receiver<ForwardedValidation<CombinedRenderPlan>>,
    parked: Option<(Instant, ForwardedValidation<CombinedRenderPlan>)>,
    results: sophia_wake::SignalSender<CombinedRenderResult>,
    pub(super) undelivered: usize,
}

impl Registration {
    /// The route, if it belongs to the registration this output still holds.
    pub(super) fn route_for(
        self,
        output: LiveRendererWorkerOutputKey,
        claim: &Arc<OutputClaim>,
    ) -> Option<OutputRoute> {
        (self.output == output && Arc::ptr_eq(&self.claim, claim) && !claim.is_detached()).then(
            || OutputRoute {
                inbox: self.inbox,
                parked: None,
                results: self.results,
                undelivered: 0,
            },
        )
    }

    pub(super) const fn output(&self) -> LiveRendererWorkerOutputKey {
        self.output
    }
}

impl OutputRoute {
    /// A parked frame first; the inbox only when nothing is parked, so one
    /// output holds one combined job. `render` gives the plan and pixels back
    /// when no slot is free. The stop flag and the deadline are checked where
    /// the frame leaves the inbox or the parking place, before any slot.
    pub(super) fn serve(
        &mut self,
        now: Instant,
        render: impl FnOnce(
            CombinedRenderPlan,
            ValidatedIsbPixels,
            Option<&sophia_protocol::ContentResourceBegin>,
        ) -> RenderAttempt,
    ) {
        let job = match self.parked.take() {
            Some((_, job)) => job,
            None => match self.inbox.try_recv() {
                Ok(job) => job,
                Err(_) => return,
            },
        };
        let stopped = job.stopped();
        let description = job.description().cloned();
        let (token, pixels, plan) = job.into_parts();
        let (outcome, pixels) = if stopped {
            (CombinedOutcome::Stopped, pixels)
        } else if now >= plan.deadline {
            (CombinedOutcome::Expired, pixels)
        } else {
            match render(plan, pixels, description.as_ref()) {
                RenderAttempt::Done(outcome, pixels) => (outcome, pixels),
                RenderAttempt::NoSlot(plan, pixels) => {
                    let deadline = plan.deadline;
                    self.parked = Some((
                        deadline,
                        ForwardedValidation::from_parts(token, pixels, plan),
                    ));
                    return;
                }
            }
        };
        self.deliver(CombinedRenderResult {
            token,
            pixels,
            outcome,
        });
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.parked.as_ref().map(|(deadline, _)| *deadline)
    }

    /// Test boundary: parks the next forwarded job unchanged, so one `serve`
    /// decides it whatever its outcome. The stop flag travels in the token.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn await_job(&mut self, wait: Duration) -> bool {
        if self.parked.is_some() {
            return true;
        }
        let Ok(job) = self.inbox.recv_timeout(wait) else {
            return false;
        };
        let (token, pixels, plan) = job.into_parts();
        self.parked = Some((
            plan.deadline,
            ForwardedValidation::from_parts(token, pixels, plan),
        ));
        true
    }

    /// Test boundary (G3-5d d-2a): the parked job's validator job number,
    /// deadline and stop flag, read without moving it.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn parked_facts(&self) -> Option<(u64, Instant, bool)> {
        self.parked
            .as_ref()
            .map(|(deadline, job)| (job.job(), *deadline, job.stopped()))
    }

    /// Admission allows one combined job per output, so a full route is a
    /// broken contract: say so and never block the renderer on it.
    fn deliver(&mut self, result: CombinedRenderResult) {
        if let Err(TrySendError::Full(_)) = self.results.try_send(result) {
            self.undelivered = self.undelivered.saturating_add(1);
            tracing::error!("sophia_renderer_worker schema=3 status=combined_result_undelivered");
        }
    }
}

/// The non-capture build's command wait: once an output has a combined
/// route, a plain `recv()` would leave a forwarded frame waiting for the next
/// ordinary command.
#[cfg(not(feature = "experimental-shell-capture"))]
pub(super) struct CommandWait {
    wake: sophia_wake::Wake,
    failed: bool,
}

#[cfg(not(feature = "experimental-shell-capture"))]
impl CommandWait {
    pub(super) fn new(control: &WorkerControl) -> io::Result<Self> {
        let wake = sophia_wake::Wake::new()?;
        control.command_wake.set(wake.notifier());
        Ok(Self {
            wake,
            failed: false,
        })
    }

    /// Before any inbox or command channel is inspected, never after.
    pub(super) fn clear(&mut self) {
        if !self.failed && self.wake.clear().is_err() {
            self.failed = true;
        }
    }

    /// `combined` is `serve_combined`'s answer. Without a route only the
    /// command channel can need the worker, and a route is registered through
    /// that channel, so blocking there loses no wake. `Err` means it closed.
    pub(super) fn next(
        &mut self,
        receive: &Receiver<WorkerCommand>,
        combined: Option<Option<Instant>>,
    ) -> Result<Option<WorkerCommand>, ()> {
        let Some(deadline) = combined else {
            return receive.recv().map(Some).map_err(|_| ());
        };
        match receive.try_recv() {
            Ok(command) => return Ok(Some(command)),
            Err(TryRecvError::Disconnected) => return Err(()),
            Err(TryRecvError::Empty) => (),
        }
        if self.failed {
            return match receive.recv_timeout(Duration::from_millis(1)) {
                Ok(command) => Ok(Some(command)),
                Err(RecvTimeoutError::Timeout) => Ok(None),
                Err(RecvTimeoutError::Disconnected) => Err(()),
            };
        }
        if self.wake.wait(deadline).is_err() {
            self.failed = true;
        }
        Ok(None)
    }
}
