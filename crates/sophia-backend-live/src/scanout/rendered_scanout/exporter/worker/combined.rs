//! The renderer half of the combined frame job (BRIDGE.md 4c-2): one bounded
//! inbox and one bounded result route per output, and at most one forwarded
//! frame parked until that output has a free slot.

use super::*;
use sophia_runtime::{CombinedInbox, CombinedToken, ForwardedValidation, ValidatedIsbPixels};

/// Owner-numbered requests start at 1, so this never names one in flight.
pub(super) const COMBINED_REQUEST: LiveRendererWorkerRequestId = LiveRendererWorkerRequestId(0);

/// What the owner froze for the renderer at dispatch; opaque to the runtime.
pub struct CombinedRenderPlan {
    pub(super) deadline: Instant,
    pub(super) target: LiveGbmEglFrameTargetRecord,
    pub(super) frame: PendingRenderedFrame,
    pub(super) preferred_modifiers: Vec<u64>,
    pub(super) output_format: Option<sophia_renderer_live::LiveCompositionFormatRequest>,
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
    ) -> Self {
        Self {
            deadline,
            target,
            frame,
            preferred_modifiers,
            output_format,
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
}

/// What one render attempt did. Moved once per attempt and never stored.
#[allow(clippy::large_enum_variant)]
pub(super) enum RenderAttempt {
    Rendered(WorkerResult),
    /// No free slot: the plan comes back to be parked.
    NoSlot(CombinedRenderPlan),
}

/// The token and pixels always come home, whatever happened to the render.
#[cfg_attr(not(test), allow(dead_code))]
pub struct CombinedRenderResult {
    pub(super) token: CombinedToken,
    pub(super) pixels: ValidatedIsbPixels,
    pub(super) outcome: CombinedOutcome,
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
    #[cfg_attr(not(test), allow(dead_code))]
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

    #[cfg_attr(not(test), allow(dead_code))]
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
    /// output holds one combined job. `render` gives the plan back when no
    /// slot is free. The deadline is checked where the frame leaves the inbox
    /// or the parking place, which is also immediately before the render.
    pub(super) fn serve(
        &mut self,
        now: Instant,
        render: impl FnOnce(CombinedRenderPlan) -> RenderAttempt,
    ) {
        let job = match self.parked.take() {
            Some((_, job)) => job,
            None => match self.inbox.try_recv() {
                Ok(job) => job,
                Err(_) => return,
            },
        };
        let (token, pixels, plan) = job.into_parts();
        let outcome = if now >= plan.deadline {
            CombinedOutcome::Expired
        } else {
            match render(plan) {
                RenderAttempt::Rendered(result) => CombinedOutcome::Rendered(result),
                RenderAttempt::NoSlot(plan) => {
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
