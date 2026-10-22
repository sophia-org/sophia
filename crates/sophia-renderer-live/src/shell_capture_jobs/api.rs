//! Proof-preserving handoff to Session. A receipt permits custody settlement,
//! not resource publication: Session must still revalidate its grant and lease.
use super::*;
use crate::{LiveRetainedRendererImageSnapshot, NativeGbmRenderedScanoutContext};

pub use sophia_renderer_native_egl::{
    NativeShellCaptureOutputFailure as LiveShellCaptureOutputFailure,
    NativeShellCaptureSource as LiveShellCaptureSource,
};

/// Immutable capabilities snapshot for one installed capture endpoint.
/// Session advertises only what this snapshot supports, for its exact
/// epoch; it is not an admission or completion proof.
#[derive(Clone, Debug)]
pub struct LiveShellCaptureFacts {
    pub epoch: Arc<LiveRendererSnapshotEpoch>,
    pub budget: Arc<crate::LiveRendererSnapshotBudget>,
    /// Renderer DRM node; the backend binds this to the epoch's render node before publication.
    pub device: u64,
    /// Importable source formats for capture; slice one is XRGB8888 only.
    pub formats: Vec<sophia_renderer_native_egl::NativeDmaBufImportFormat>,
    pub max_width: u32,
    pub max_height: u32,
}

/// Destination reservation is acquired before enqueue, against this exact
/// renderer budget/epoch. This request carries no shell authorization.
#[derive(Debug)]
pub struct LiveShellCaptureRequest {
    pub source: LiveShellCaptureSource,
    pub image_id: crate::LiveRendererImageId,
    pub budget: Arc<crate::LiveRendererSnapshotBudget>,
    pub reservation: crate::LiveRendererSnapshotReservation,
    pub epoch: Arc<LiveRendererSnapshotEpoch>,
}

impl LiveShellCaptureRequest {
    fn into_native(self) -> sophia_renderer_native_egl::NativeShellCaptureRequest {
        sophia_renderer_native_egl::NativeShellCaptureRequest {
            source: self.source,
            image_id: sophia_renderer_native_egl::NativeRendererImageId::from_raw(
                self.image_id.raw(),
            ),
            budget: self.budget,
            reservation: self.reservation,
            epoch: self.epoch,
        }
    }
    fn from_native(r: sophia_renderer_native_egl::NativeShellCaptureRequest) -> Self {
        Self {
            source: r.source,
            image_id: crate::LiveRendererImageId::from_raw(r.image_id.raw()),
            budget: r.budget,
            reservation: r.reservation,
            epoch: r.epoch,
        }
    }
}

/// Opaque, process-unique job identity. Neither a renderer image nor a grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveShellCaptureJob(JobId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveShellCaptureSubmitFailure {
    Capacity,
    Closed,
    Generation,
    IdExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveShellCaptureStop {
    Cancelled,
    Deadline,
    Generation,
    ExecutorClosed,
    RendererRefused,
}

pub type LiveShellCaptureOutput =
    Result<LiveRetainedRendererImageSnapshot, LiveShellCaptureOutputFailure>;

/// Only Completed proves reads ended after GPU submission. Returned proves
/// pre-use rollback. Uncertain never supplies source descriptors or credits.
#[derive(Debug)]
pub enum LiveShellCaptureOutcome {
    Completed {
        source: LiveShellCaptureSource,
        output: LiveShellCaptureOutput,
        stopped: Option<LiveShellCaptureStop>,
    },
    Returned {
        request: LiveShellCaptureRequest,
        reason: LiveShellCaptureStop,
    },
    Uncertain,
}

/// No public constructor or supplied-completion API. Only take_completed can
/// issue this receipt, following the private native adapter's exact-copy proof.
///
/// ```compile_fail
/// use sophia_renderer_live::{LiveShellCaptureCompletion, LiveShellCaptureJob,
///     LiveShellCaptureOutcome};
/// fn forge(job: LiveShellCaptureJob, outcome: LiveShellCaptureOutcome)
///     -> LiveShellCaptureCompletion {
///     LiveShellCaptureCompletion { job, outcome }
/// }
/// ```
#[derive(Debug)]
#[must_use = "Every receipt must be reconciled with its shell reservation"]
pub struct LiveShellCaptureCompletion {
    job: LiveShellCaptureJob,
    outcome: LiveShellCaptureOutcome,
}

impl LiveShellCaptureCompletion {
    pub fn job(&self) -> LiveShellCaptureJob {
        self.job
    }

    /// Inspect while retaining the receipt through fallible owner settlement.
    pub fn outcome(&self) -> &LiveShellCaptureOutcome {
        &self.outcome
    }

    pub fn into_outcome(self) -> LiveShellCaptureOutcome {
        self.outcome
    }
}

#[must_use = "Reconcile every receipt even when the batch reports a fault"]
pub struct LiveShellCaptureCompletions {
    pub receipts: Vec<LiveShellCaptureCompletion>,
    /// Even on fault, reconcile all returned receipts. A fault is not rollback.
    pub fault: bool,
}

/// May travel to the Session resource owner. Its fd signals completions only.
pub struct LiveShellCaptureInbox(native::Inbox);

/// Construct and service on the existing renderer owner; this type is !Send.
/// Its fd signals requests, not fences. While pending_fences != 0 the owner must
/// schedule bounded follow-up turns, including when no new request arrives.
///
/// ```compile_fail
/// fn send<T: Send>() {}
/// send::<sophia_renderer_live::LiveShellCaptureExecutor>();
/// ```
pub struct LiveShellCaptureExecutor(native::NativeDriver);

/// Persistent renderer-owner custody across mailbox generations. Closing an
/// executor does not reset this bound or discard uncertain source ownership.
/// Keep this owner through context migration/teardown; teardown alone is not
/// proof that GPU reads ended. There is deliberately no reset/clear operation.
#[derive(Clone)]
pub struct LiveShellCaptureCustody(Arc<CaptureGraveyard<native::Payload>>);

impl Default for LiveShellCaptureCustody {
    fn default() -> Self {
        Self(CaptureGraveyard::new())
    }
}

impl LiveShellCaptureCustody {
    pub fn same_owner(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    /// Construct on the context owner, with its shared persistent budget.
    /// Requests from another budget are refused before any native use.
    pub fn channel(
        &self,
        epoch: Arc<LiveRendererSnapshotEpoch>,
        budget: Arc<crate::LiveRendererSnapshotBudget>,
    ) -> io::Result<(LiveShellCaptureInbox, LiveShellCaptureExecutor)> {
        let (inbox, driver) = native::native_owned_channel(epoch, self.0.clone(), Some(budget))?;
        Ok((
            LiveShellCaptureInbox(inbox),
            LiveShellCaptureExecutor(driver),
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveShellCaptureProgress {
    pub operations: usize,
    pub pending_fences: usize,
    pub budget_spent: bool,
}

pub fn live_shell_capture_channel(
    epoch: Arc<LiveRendererSnapshotEpoch>,
) -> io::Result<(LiveShellCaptureInbox, LiveShellCaptureExecutor)> {
    let (inbox, driver) = native::native_channel(epoch)?;
    Ok((
        LiveShellCaptureInbox(inbox),
        LiveShellCaptureExecutor(driver),
    ))
}

impl LiveShellCaptureInbox {
    /// Full/closed/withdrawn submission returns the original descriptors and
    /// destination reservation. No read has started on an unsuccessful enqueue.
    pub fn submit(
        &mut self,
        request: LiveShellCaptureRequest,
        deadline: Instant,
    ) -> Result<LiveShellCaptureJob, (LiveShellCaptureSubmitFailure, LiveShellCaptureRequest)> {
        self.0
            .submit(Request {
                payload: native::Payload::new(request.into_native()),
                deadline,
            })
            .map(LiveShellCaptureJob)
            .map_err(|(failure, request)| {
                let failure = match failure {
                    SubmitFailure::Capacity => LiveShellCaptureSubmitFailure::Capacity,
                    SubmitFailure::Closed => LiveShellCaptureSubmitFailure::Closed,
                    SubmitFailure::Generation => LiveShellCaptureSubmitFailure::Generation,
                    SubmitFailure::IdExhausted => LiveShellCaptureSubmitFailure::IdExhausted,
                };
                (
                    failure,
                    LiveShellCaptureRequest::from_native(
                        request
                            .payload
                            .request
                            .expect("unsubmitted request retained"),
                    ),
                )
            })
    }

    /// Withdrawal forbids publication; it never proves reads ended.
    pub fn cancel(&mut self, job: LiveShellCaptureJob) -> bool {
        self.0.cancel(job.0)
    }

    pub fn take_completed(&mut self, now: Instant) -> io::Result<LiveShellCaptureCompletions> {
        let batch = self.0.take_completed(now)?;
        Ok(LiveShellCaptureCompletions {
            receipts: batch.completions.into_iter().map(convert).collect(),
            fault: batch.fault,
        })
    }
}

impl LiveShellCaptureExecutor {
    /// Close before dropping/replacing the context. Queued work rolls back;
    /// submitted work becomes uncertain and retains its native escrow. The
    /// persistent custody owner must outlive this executor and its replacement.
    pub fn close(&mut self) {
        self.0.close();
    }

    pub fn service<T: AsFd>(
        &mut self,
        context: &mut NativeGbmRenderedScanoutContext<T>,
        now: Instant,
        operation_budget: usize,
    ) -> io::Result<LiveShellCaptureProgress> {
        context.service_shell_captures(self, now, operation_budget)
    }

    pub(crate) fn service_native<T: AsFd>(
        &mut self,
        context: &mut sophia_renderer_native_egl::NativeGbmRenderedScanoutContext<T>,
        now: Instant,
        budget: usize,
    ) -> io::Result<LiveShellCaptureProgress> {
        let p = self.0.service(context, now, budget)?;
        Ok(LiveShellCaptureProgress {
            operations: p.operations,
            pending_fences: p.pending_fences,
            budget_spent: p.budget_spent,
        })
    }
}

impl AsFd for LiveShellCaptureInbox {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
impl AsFd for LiveShellCaptureExecutor {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

fn stop(reason: StopReason) -> LiveShellCaptureStop {
    match reason {
        StopReason::Cancelled => LiveShellCaptureStop::Cancelled,
        StopReason::Deadline => LiveShellCaptureStop::Deadline,
        StopReason::Generation => LiveShellCaptureStop::Generation,
        StopReason::ExecutorClosed => LiveShellCaptureStop::ExecutorClosed,
        StopReason::RendererRefused => LiveShellCaptureStop::RendererRefused,
    }
}

fn convert(
    completion: Completion<
        native::Payload,
        Result<
            Arc<sophia_renderer_native_egl::NativeRendererImageSnapshot>,
            LiveShellCaptureOutputFailure,
        >,
    >,
) -> LiveShellCaptureCompletion {
    let completed = |payload: native::Payload,
                     output: Result<
        Arc<sophia_renderer_native_egl::NativeRendererImageSnapshot>,
        LiveShellCaptureOutputFailure,
    >,
                     stopped| {
        assert!(
            payload.request.is_none(),
            "completed request left native custody"
        );
        LiveShellCaptureOutcome::Completed {
            source: payload
                .completed_source
                .expect("native receipt retains completed source"),
            output: output.map(LiveRetainedRendererImageSnapshot::from_shell_capture),
            stopped,
        }
    };
    let outcome = match completion.outcome {
        Outcome::Completed { payload, output } => completed(payload, output, None),
        Outcome::CompletedDiscarded {
            payload,
            output,
            reason,
        } => completed(payload, output, Some(stop(reason))),
        Outcome::RolledBack { payload, reason } => LiveShellCaptureOutcome::Returned {
            request: LiveShellCaptureRequest::from_native(
                payload.request.expect("pre-use rollback retains request"),
            ),
            reason: stop(reason),
        },
        Outcome::FailedUncertain => LiveShellCaptureOutcome::Uncertain,
    };
    LiveShellCaptureCompletion {
        job: LiveShellCaptureJob(completion.id),
        outcome,
    }
}

#[cfg(test)]
#[path = "../../tests/support/shell_capture_api.rs"]
mod tests;
