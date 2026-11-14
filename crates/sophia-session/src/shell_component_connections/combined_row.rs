//! G3-5b: the combined frame job's per-slot row, its hooks and the controlled
//! driver (REV2 R1-R4), until gate 4 makes the row production's.
//!
//! The row sits beside the slot's image owner and lives as long: every
//! attempt in the slot reuses it, and stop, close and replacement never drop
//! it. Plain close collects nothing, so a job whose result comes back after
//! close is collected, settled and disposed of on a later hooked visit, with
//! the same worker and the exact head and job stored with it. No entry ends
//! because the grant changed; each ends only through its own transition.
//!
//! Collection (R2) reads the disposition before settlement consumes the
//! result. An exported render becomes Held. A failed render, an expiry and a
//! stop become Failed, with no lease held. A Mismatch drops its pixels here
//! and is delivered as Malformed, before any reconciliation. A settlement
//! refusal, a refused credit clear and a refused delivery each keep what came
//! back and retry only their own step. Their invariant error is reported
//! once, and reporting never discards the row or an owner.
//!
//! Every error and unconfirmed cancellation keeps the grant it arose under.
//! Only an error of the slot's current, unrevoked attempt is that attempt's
//! failure. An error of an attempt that already ended is a cleanup
//! diagnostic: it never stops the replacement, and the old job keeps its
//! owners and its retry.
//!
//! G3-5c: the module is feature-compiled under the row's three gates (CS),
//! so the content service's consumption arm is the same code production
//! builds. No production caller supplies a row until gate 4; the
//! fault-injection and observation storage stay test-only.
use super::*;
#[cfg(test)]
use sophia_backend_live::LiveShellContentFrame;
use sophia_backend_live::{
    CombinedComposeRefusal, CombinedComposition, CombinedDispatch, CombinedDisposition,
    CombinedHead, CombinedRelease, CombinedRenderPlan, CombinedRenderResult, CombinedSettleRefused,
    LiveProductionCpuScene, LiveProductionCursorPresentation, LiveProductionVisualRuntime,
    LiveShellContentLayer, ShellContentNative,
};
use sophia_engine::HeadlessOutput;
use sophia_protocol::Point;
use sophia_protocol::shell_files::ShellFrameCommit;
use sophia_runtime::{
    CombinedFrameAdmitted, CombinedFrameFacts, CombinedFrameHooks, CombinedReservation,
    CombinedReserve, CombinedResult, CombinedToken, ContentRenderBundle, FrameCandidateState,
    FrameCopyBinding, IsbValidationFailure, IsbValidationWorker, ValidatedIsbPixels,
};
use std::sync::mpsc;
use std::time::Instant;

// G3-5d d-2b1: `turn_combined`'s two halves for the Session split driver.
#[cfg(test)]
#[path = "../../tests/support/combined_row_steps.rs"]
pub(crate) mod steps;
// G3-5d d-2b3d: slot-indexed read-only observers (REV2 D5).
#[cfg(test)]
#[path = "../../tests/support/combined_observers.rs"]
mod observers;

#[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
type Worker = IsbValidationWorker<CombinedRenderPlan>;
#[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
type Failures = [Option<(ComponentConnectionKey, ShellTransportError)>; MAX_SHELL_COMPONENTS];

/// What the hooks read from the owner turn, built once per controlled turn.
#[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
pub(crate) struct CombinedTurn<'a, N> {
    pub native: &'a mut N,
    pub runtime: &'a LiveProductionVisualRuntime,
    pub scene: &'a LiveProductionCpuScene,
    pub outputs: &'a [HeadlessOutput],
    /// Each slot's content layer; `None` takes today's path. The layer is
    /// the driver's to supply until gate 4 binds it to the role.
    pub layers: [Option<LiveShellContentLayer>; MAX_SHELL_COMPONENTS],
    /// The turn's one cursor snapshot, which every proposal uses (R4).
    pub cursor: LiveProductionCursorPresentation,
    pub plane: Option<Point>,
    /// The turn's one Instant: the image-owner service and the walk's
    /// deadlines read it (G3-5c).
    pub instant: Instant,
}

/// The owner loop's cursor rule over one pointer reading, and that reading
/// as the plane position.
#[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
pub(crate) fn turn_cursor(
    native: bool,
    position: Option<Point>,
) -> (LiveProductionCursorPresentation, Option<Point>) {
    (
        crate::live_session::turn_cursor_presentation(native, position),
        position,
    )
}

/// What a caller lends the content service for one turn so `submit_bundle`
/// can join its bundles with the row (G3-5c C2). Only the controlled caller
/// builds one until gate 4.
pub(crate) struct CombinedConsume<'a> {
    pub(crate) row: &'a mut CombinedRow,
    pub(crate) cursor: LiveProductionCursorPresentation,
    pub(crate) plane: Option<Point>,
    /// The owner turn's one Instant, the one its image-owner service used.
    pub(crate) instant: Instant,
}

#[derive(Default)]
pub(crate) struct CombinedRow {
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    next_ticket: u64,
    /// The one admission between `reserve` and its end in `cancel` or
    /// `submitted`, inside one proposal.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    open: Option<Open>,
    jobs: Vec<Job>,
    /// Reservations whose cancellation the facade did not confirm, with the
    /// grant they were made under; the walk retries them.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    cancels: Vec<(ContentGrant, CombinedHead)>,
    /// Invariant failures not yet reported, with their originating grant.
    errors: Vec<(ContentGrant, String)>,
    /// Invariant failures of attempts that had already ended when they were
    /// reported: kept for the controls, never a slot failure.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    diagnostics: Vec<(ContentGrant, String)>,
    /// Declared delivery faults still to inject.
    #[cfg(test)]
    refusals: usize,
    /// Every reserve-time projection, made from the queued frame and the
    /// host's description before admission, for the R4 comparison.
    #[cfg(test)]
    projections: Vec<LiveShellContentFrame>,
    /// A declared image-service fault still to inject (G3-5d d-2b1).
    #[cfg(test)]
    image_fault: Option<&'static str>,
    /// Every admission's binding, recorded where it is submitted (d-2b2).
    #[cfg(test)]
    bindings: Vec<steps::Binding>,
    /// Every collected result's job and disposition, recorded where the row
    /// reads it, before settlement (d-2b2).
    #[cfg(test)]
    collected: Vec<(u64, CombinedDisposition)>,
    /// Every collected result's identity, from its job and reservation,
    /// recorded at the same point (d-2b3b2).
    #[cfg(test)]
    collections: Vec<steps::Collection>,
    /// Every bundle's join, with the entries present when it was made
    /// (d-2b3b2).
    #[cfg(test)]
    joins: Vec<steps::Join>,
    /// b4: the row's own calls in order, one event each (REV3/REV4).
    #[cfg(test)]
    trace: Vec<steps::Event>,
}

#[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
struct Open {
    ticket: u64,
    grant: ContentGrant,
    head: CombinedHead,
    stage: Stage,
}

enum Stage {
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    Composed(CombinedComposition),
    /// The dispatch, with the admission's exact candidate and its original
    /// validation deadline (G3-5c).
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    Planned(CombinedDispatch, FrameCopyBinding, Instant),
    /// The plan was not built; the composition went with that attempt.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    Unplanned,
}

struct Job {
    job: u64,
    grant: ContentGrant,
    head: CombinedHead,
    dispatch: CombinedDispatch,
    /// The runtime's own identity of the admitted candidate (G3-5c row
    /// identity): grant, output, allocation, facts generation, stream and
    /// candidate generation.
    binding: FrameCopyBinding,
    /// The admission's validation deadline, unchanged since admission.
    deadline: Instant,
    verdict: Verdict,
    state: State,
    /// This job's invariant failure was reported; its retries stay quiet.
    reported: bool,
    /// b4: the admission's key, named by every trace event.
    #[cfg(test)]
    key: steps::Key,
}

/// The owner's one-way decision for the job's candidate (G3-5c C4, REV3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Verdict {
    /// No decision yet.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    Open,
    /// The walk released a Held result at its deadline while its candidate
    /// was live: a later bundle takes the failure terminal.
    Expired,
    /// The arm decided for this candidate. The entry never joins again.
    Consumed,
}

/// What the row holds for a bundle's exact candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CombinedJoin {
    /// No entry for the bundle's (grant, output, candidate generation):
    /// the ordinary arm, unchanged.
    Ordinary,
    /// A Held result with no decision yet. Marked Consumed.
    Held(u64),
    /// A delivered failure, or a lease-free expiry, with no decision yet.
    /// Marked Consumed.
    Failed(u64),
    /// Release debt of an expired result. Marked Consumed; the debt stays.
    Debt(u64),
    /// An exact entry in a state no bundle can meet. Nothing is changed.
    Invariant(u64),
}

enum State {
    /// The validator owns the job; a failed validation or a refused move
    /// returns the plan on `returned`.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    InFlight {
        reservation: CombinedReservation,
        returned: mpsc::Receiver<CombinedRenderPlan>,
    },
    /// Settlement refused and returned everything; it is retried.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    Unsettled {
        reservation: CombinedReservation,
        result: Box<CombinedRenderResult>,
        disposition: CombinedDisposition,
    },
    /// The facade took its part; only the credit clear is retried, with the
    /// original worker and the returned reservation and token.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    Credit {
        reservation: CombinedReservation,
        token: CombinedToken,
        pixels: ValidatedIsbPixels,
        disposition: CombinedDisposition,
    },
    /// Settled; the delivery was refused and is offered again.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    Undelivered {
        result: CombinedResult,
        disposition: CombinedDisposition,
    },
    /// Exported and delivered: the issuing facade holds the lease.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    Held,
    /// Delivered with no lease held, for c's terminal or its retirement; or
    /// a lease-free expiry whose Release transferred (`Expired`).
    Failed(CombinedDisposition),
    /// A released Held result whose Release did not transfer.
    Debt(
        #[cfg_attr(
            not(test),
            expect(dead_code, reason = "kept as the last Release outcome")
        )]
        CombinedRelease,
    ),
}

/// What one collection step leaves: a state to keep, or a settled result to
/// deliver.
#[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
enum Step {
    Keep(State),
    Deliver(CombinedResult, CombinedDisposition),
}

fn report(errors: &mut Vec<(ContentGrant, String)>, job: &mut Job, what: &str) {
    if !job.reported {
        job.reported = true;
        errors.push((job.grant, format!("combined job {}: {what}", job.job)));
    }
}

/// Mismatch's pixels drop here, on the owner thread; its job is Malformed.
#[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
fn delivered(pixels: ValidatedIsbPixels, disposition: CombinedDisposition) -> CombinedResult {
    if disposition == CombinedDisposition::Mismatch {
        drop(pixels);
        Err(IsbValidationFailure::Malformed)
    } else {
        Ok(pixels)
    }
}

impl CombinedRow {
    /// Owed work: an open admission, a job in any state, or an unconfirmed
    /// cancellation. It joins final quiescence only.
    pub(crate) fn pending(&self) -> usize {
        usize::from(self.open.is_some()) + self.jobs.len() + self.cancels.len()
    }

    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    pub(crate) fn unconfirmed_cancels(&self) -> usize {
        self.cancels.len()
    }

    /// Declines a Held result (the combined arm's decline, and b's
    /// controls'): its lease goes to Release, completed through `released`.
    /// `None` when `job` is not Held.
    pub(crate) fn decline<N: ShellContentNative>(
        &mut self,
        native: &mut N,
        job: u64,
    ) -> Option<CombinedRelease> {
        let index = self
            .jobs
            .iter()
            .position(|held| held.job == job && matches!(held.state, State::Held))?;
        let mut held = self.jobs.remove(index);
        let outcome = native.release_combined_held(&held.head, held.job);
        let keep = self.released(&mut held, outcome);
        #[cfg(test)]
        self.record_release(&held, steps::Cause::Decline, outcome, keep, None);
        if keep {
            self.jobs.push(held);
        }
        Some(outcome)
    }

    /// The one completion rule for every Release attempt, from `decline`,
    /// a walk release or a Debt flush (REV3). True when `job` must stay.
    /// Transferred ends the renderer's custody only: an `Expired` decision
    /// not yet consumed stays as a lease-free `Failed(Expired)`. Every other
    /// outcome is Debt, and no outcome changes the verdict.
    fn released(&mut self, job: &mut Job, outcome: CombinedRelease) -> bool {
        if outcome == CombinedRelease::Transferred {
            if job.verdict == Verdict::Expired {
                job.state = State::Failed(CombinedDisposition::Expired);
                return true;
            }
            return false;
        }
        if outcome == CombinedRelease::Mismatch {
            report(
                &mut self.errors,
                job,
                "the Release cell does not hold this job",
            );
        }
        job.state = State::Debt(outcome);
        true
    }

    /// Disposal and debt maintenance, after the slot's one hooked service.
    /// It takes no results. A Held or Failed job whose grant ended can never
    /// be accepted: Held releases, Failed retires, since its delivery already
    /// transferred. Debt flushes until Transferred. For the current grant,
    /// Held and Failed follow REV3's table over `candidate`, the runtime's
    /// own candidate fact, and the job's unchanged deadline at `instant`.
    /// Every Release completes through `released`.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    fn walk<N: ShellContentNative>(
        &mut self,
        native: &mut N,
        current: Option<ContentGrant>,
        candidate: &dyn Fn(FrameCopyBinding, u64) -> Option<FrameCandidateState>,
        instant: Instant,
    ) {
        for (grant, head) in std::mem::take(&mut self.cancels) {
            let confirmed = matches!(native.cancel_combined_frame(&head), Ok(true));
            #[cfg(test)]
            self.record_cancel_later(grant, &head, confirmed);
            if !confirmed {
                self.cancels.push((grant, head));
            }
        }
        for mut job in std::mem::take(&mut self.jobs) {
            let ended = current != Some(job.grant);
            let keep = match job.state {
                State::Held if ended => {
                    let outcome = native.release_combined_held(&job.head, job.job);
                    let keep = self.released(&mut job, outcome);
                    #[cfg(test)]
                    self.record_release(
                        &job,
                        steps::Cause::WalkEndedHeld { current },
                        outcome,
                        keep,
                        None,
                    );
                    keep
                }
                State::Debt(_) => {
                    let outcome = native.flush_combined_release(&job.head, job.job);
                    let keep = self.released(&mut job, outcome);
                    #[cfg(test)]
                    self.record_release(&job, steps::Cause::DebtFlush, outcome, keep, None);
                    keep
                }
                State::Failed(_) if ended => {
                    #[cfg(test)]
                    self.record_removed(job.key, steps::Cause::WalkEndedFailed { current });
                    false
                }
                State::Held | State::Failed(_) => {
                    let state = candidate(job.binding, job.job);
                    #[cfg(not(test))]
                    let keep = self.dispose(native, &mut job, state, instant);
                    #[cfg(test)]
                    let keep = self.dispose(native, &mut job, state, instant, current);
                    keep
                }
                _ => true,
            };
            if keep {
                self.jobs.push(job);
            }
        }
    }

    /// REV3's table for a Held or Failed job of the current grant. A missing
    /// candidate state is Unknown: absence is never death. True to keep.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    fn dispose<N: ShellContentNative>(
        &mut self,
        native: &mut N,
        job: &mut Job,
        state: Option<FrameCandidateState>,
        instant: Instant,
        #[cfg(test)] current: Option<ContentGrant>,
    ) -> bool {
        use FrameCandidateState::{Assembling, Ended, Pending, Submitted, Unknown};
        let state = state.unwrap_or(Unknown);
        let past = instant >= job.deadline;
        let live = matches!(state, Assembling | Pending | Submitted);
        #[cfg(test)]
        let before = job.verdict;
        // Some(true) releases, Some(false) keeps, None retires.
        let release = match (&job.state, job.verdict) {
            // The recorded decision, whatever the candidate (R2-2).
            (State::Held, Verdict::Consumed) => Some(true),
            (State::Held, Verdict::Open) if state == Ended || (state == Unknown && past) => {
                Some(true)
            }
            (State::Held, Verdict::Open) if live && past => {
                job.verdict = Verdict::Expired;
                Some(true)
            }
            (State::Held, Verdict::Open) => Some(false),
            (State::Failed(_), Verdict::Consumed) => None,
            (State::Failed(_), Verdict::Open | Verdict::Expired)
                if state == Ended || (state == Unknown && past) =>
            {
                None
            }
            // Unknown before its deadline, or live at any time: unchanged.
            (State::Failed(_), Verdict::Open | Verdict::Expired) => Some(false),
            // The walk expires and releases in one step, so Held never
            // keeps Expired. Fail closed: report, and release through the
            // one rule rather than drop the lease.
            (State::Held, Verdict::Expired) => {
                report(&mut self.errors, job, "a Held result kept an expiry");
                Some(true)
            }
            // The walk passes only Held and Failed; anything else stays.
            _ => Some(false),
        };
        #[cfg(test)]
        let facts = steps::Dispose {
            candidate: state,
            past,
            released: release == Some(true),
            expired: before != job.verdict,
            fail_closed: before == Verdict::Expired && matches!(job.state, State::Held),
        };
        let keep = match release {
            Some(true) => {
                let outcome = native.release_combined_held(&job.head, job.job);
                let keep = self.released(job, outcome);
                #[cfg(test)]
                self.record_release(
                    job,
                    steps::Cause::Dispose { current },
                    outcome,
                    keep,
                    Some(facts),
                );
                return keep;
            }
            Some(false) => true,
            None => false,
        };
        #[cfg(test)]
        self.record_kept(job, keep, facts, current);
        keep
    }

    /// The join of a bundle with the row (G3-5c): the exact entry for its
    /// grant, output and candidate generation. A matched decision is marked
    /// `Consumed` here, before any fallible step of the arm. An exact entry
    /// in any other state, or a bundle surface outside the admitted
    /// allocation, is an invariant and changes nothing.
    pub(crate) fn join(&mut self, bundle: &ContentRenderBundle) -> CombinedJoin {
        #[cfg(test)]
        self.record_join(bundle);
        let Some(job) = self.jobs.iter_mut().find(|held| {
            held.binding.key.grant == bundle.grant
                && held.binding.key.output == bundle.output
                && held.binding.generation == bundle.candidate_generation
        }) else {
            return CombinedJoin::Ordinary;
        };
        let admitted = bundle
            .surfaces
            .iter()
            .all(|surface| surface.allocation == job.binding.key.allocation);
        let join = match (&job.state, job.verdict) {
            _ if !admitted => CombinedJoin::Invariant(job.job),
            (State::Held, Verdict::Open) => CombinedJoin::Held(job.job),
            (State::Failed(disposition), Verdict::Open | Verdict::Expired)
                if *disposition != CombinedDisposition::Mismatch =>
            {
                CombinedJoin::Failed(job.job)
            }
            (State::Debt(_), Verdict::Expired) => CombinedJoin::Debt(job.job),
            _ => CombinedJoin::Invariant(job.job),
        };
        if !matches!(join, CombinedJoin::Invariant(_)) {
            job.verdict = Verdict::Consumed;
        }
        #[cfg(test)]
        let matched = (job.job, job.key, admitted, job.verdict);
        #[cfg(test)]
        self.record_join_match(matched, &join);
        join
    }

    /// A Held job's head, dispatch and deadline, for its consumption.
    pub(crate) fn consumption(
        &self,
        job: u64,
    ) -> Option<(&CombinedHead, &CombinedDispatch, Instant)> {
        self.jobs
            .iter()
            .find(|held| held.job == job && matches!(held.state, State::Held))
            .map(|held| (&held.head, &held.dispatch, held.deadline))
    }

    /// Removes a Held job after its accept committed: the lease is the
    /// native pending frame's now. False when no such entry is here.
    pub(crate) fn accepted(&mut self, job: u64) -> bool {
        let removed = self.remove(job, |state| matches!(state, State::Held));
        #[cfg(test)]
        self.record_consumed(removed.as_ref(), steps::Cause::Accepted);
        removed.is_some()
    }

    /// Removes a Failed job after its terminal was issued. It holds no
    /// lease. False when no such entry is here.
    pub(crate) fn consumed_failed(&mut self, job: u64) -> bool {
        let removed = self.remove(job, |state| matches!(state, State::Failed(_)));
        #[cfg(test)]
        self.record_consumed(removed.as_ref(), steps::Cause::ConsumedFailed);
        removed.is_some()
    }

    /// The removed job, for the trace; it holds nothing the row needs.
    fn remove(&mut self, job: u64, state: impl Fn(&State) -> bool) -> Option<Job> {
        let index = self.jobs.iter().position(|held| {
            held.job == job && held.verdict == Verdict::Consumed && state(&held.state)
        })?;
        Some(self.jobs.remove(index))
    }

    /// The first error of `current`, the slot's unrevoked attempt; errors of
    /// any other grant become cleanup diagnostics first (R1).
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    fn take_error(&mut self, current: Option<ContentGrant>) -> Option<ShellTransportError> {
        let (errors, ended): (Vec<_>, Vec<_>) = std::mem::take(&mut self.errors)
            .into_iter()
            .partition(|(grant, _)| Some(*grant) == current);
        self.errors = errors;
        self.diagnostics.extend(ended);
        (!self.errors.is_empty()).then(|| ShellTransportError::CombinedRow(self.errors.remove(0).1))
    }

    /// True when the native confirmed the cancellation; otherwise it is
    /// reported and retried by the walk.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    fn cancel<N: ShellContentNative>(
        &mut self,
        native: &mut N,
        grant: ContentGrant,
        head: CombinedHead,
        what: String,
    ) -> bool {
        let confirmed = matches!(native.cancel_combined_frame(&head), Ok(true));
        if !confirmed {
            self.errors.push((grant, what));
            self.cancels.push((grant, head));
        }
        confirmed
    }

    /// One job's collection; `None` when the job ended here.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    fn collect_one<N: ShellContentNative>(
        &mut self,
        native: &mut N,
        worker: &mut Worker,
        deliver: &mut dyn FnMut(u64, CombinedResult) -> Result<(), CombinedResult>,
        mut job: Job,
    ) -> Option<Job> {
        // Every path below sets the state again or ends the job.
        let state = std::mem::replace(&mut job.state, State::Held);
        #[cfg(test)]
        let from = steps::Label::of(&state);
        let step = match state {
            State::InFlight {
                reservation,
                returned,
            } => {
                // The validator's own completion carries this job's result
                // (G3-2); only the Release reservation is the row's.
                if let Ok(plan) = returned.try_recv() {
                    drop((plan, reservation));
                    let what = format!("combined job {}: returned plan not cancelled", job.job);
                    let _confirmed = self.cancel(native, job.grant, job.head, what);
                    #[cfg(test)]
                    self.record_removed(
                        job.key,
                        steps::Cause::ReturnedPlan {
                            confirmed: _confirmed,
                        },
                    );
                    return None;
                }
                match native.take_combined_frame_result(&job.head) {
                    Ok(None) => Step::Keep(State::InFlight {
                        reservation,
                        returned,
                    }),
                    Ok(Some(result)) => {
                        let disposition = result.disposition();
                        #[cfg(test)]
                        self.collected.push((job.job, disposition));
                        #[cfg(test)]
                        self.record_collection(&job, &reservation, disposition);
                        self.settle(native, worker, &mut job, reservation, result, disposition)
                    }
                    Err(error) => {
                        let what = format!("result route: {error}");
                        report(&mut self.errors, &mut job, &what);
                        Step::Keep(State::InFlight {
                            reservation,
                            returned,
                        })
                    }
                }
            }
            State::Unsettled {
                reservation,
                result,
                disposition,
            } => self.settle(native, worker, &mut job, reservation, *result, disposition),
            State::Credit {
                reservation,
                token,
                pixels,
                disposition,
            } => match worker.settle_combined_token(reservation, token) {
                Ok(()) => Step::Deliver(delivered(pixels, disposition), disposition),
                Err((reservation, token)) => Step::Keep(State::Credit {
                    reservation,
                    token,
                    pixels,
                    disposition,
                }),
            },
            State::Undelivered {
                result,
                disposition,
            } => Step::Deliver(result, disposition),
            state => Step::Keep(state),
        };
        job.state = match step {
            Step::Keep(state) => state,
            Step::Deliver(result, disposition) => match deliver(job.job, result) {
                Ok(()) if disposition == CombinedDisposition::Rendered => State::Held,
                Ok(()) => State::Failed(disposition),
                Err(result) => {
                    report(&mut self.errors, &mut job, "the runtime refused its result");
                    State::Undelivered {
                        result,
                        disposition,
                    }
                }
            },
        };
        #[cfg(test)]
        self.record_step(&job, from);
        Some(job)
    }

    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    fn settle<N: ShellContentNative>(
        &mut self,
        native: &mut N,
        worker: &mut Worker,
        job: &mut Job,
        reservation: CombinedReservation,
        result: CombinedRenderResult,
        disposition: CombinedDisposition,
    ) -> Step {
        let refusal = match native.settle_combined_frame(&job.head, worker, reservation, result) {
            Ok(pixels) => return Step::Deliver(delivered(pixels, disposition), disposition),
            Err(refusal) => refusal,
        };
        let (what, state) = match refusal {
            CombinedSettleRefused::RendererNotFound(reservation, result) => (
                "no live facade issued it",
                State::Unsettled {
                    reservation,
                    result,
                    disposition,
                },
            ),
            CombinedSettleRefused::Refused(reservation, result) => (
                "the issuing facade refused its result",
                State::Unsettled {
                    reservation,
                    result,
                    disposition,
                },
            ),
            CombinedSettleRefused::Credit(reservation, token, pixels) => (
                "the validator refused its credit",
                State::Credit {
                    reservation,
                    token,
                    pixels,
                    disposition,
                },
            ),
        };
        report(&mut self.errors, job, what);
        Step::Keep(state)
    }
}

/// The hooks for one slot's one image-owner service.
#[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
struct CombinedHooks<'r, 't, 'a, N> {
    row: &'r mut CombinedRow,
    turn: &'t mut CombinedTurn<'a, N>,
    slot: usize,
}

impl<N: ShellContentNative> CombinedFrameHooks<CombinedRenderPlan>
    for CombinedHooks<'_, '_, '_, N>
{
    /// Projection, one composition, then the renderer reservation. Only an
    /// unclassified composition error fails; anything else not combined is
    /// today's path, with nothing reserved.
    fn reserve(
        &mut self,
        frame: &ShellFrameCommit,
        facts: &CombinedFrameFacts<'_>,
    ) -> CombinedReserve<CombinedRenderPlan> {
        let turn = &mut *self.turn;
        let candidate = &frame.candidate;
        let layer = turn.layers.get(self.slot).copied().flatten();
        let output = turn
            .outputs
            .iter()
            .find(|output| output.id.raw() == candidate.output.id)
            .copied();
        let (Some(layer), Some(output), None) = (layer, output, self.row.open.as_ref()) else {
            return CombinedReserve::Ordinary;
        };
        // A candidate this cannot project is refused the same way later on
        // today's path, which keeps it.
        let Ok(projected) = crate::live_session::metadata_shell::project_frame_commit(
            candidate,
            facts.description,
            output,
            facts.allocations,
        ) else {
            return CombinedReserve::Ordinary;
        };
        #[cfg(test)]
        self.row.projections.push(projected.clone());
        let composed = turn.native.compose_combined_frame(
            turn.runtime,
            turn.scene,
            layer,
            &projected,
            turn.cursor,
            turn.plane,
        );
        let composition = match composed {
            Ok(composition) => composition,
            Err(CombinedComposeRefusal::Unclassified(error)) => {
                return CombinedReserve::Fail(error.to_string());
            }
            Err(CombinedComposeRefusal::Ineligible(_) | CombinedComposeRefusal::Topology) => {
                return CombinedReserve::Ordinary;
            }
        };
        let Ok((head, inbox)) = turn.native.reserve_combined_frame(output.id) else {
            return CombinedReserve::Ordinary;
        };
        let ticket = self.row.next_ticket;
        self.row.next_ticket = ticket.wrapping_add(1);
        self.row.open = Some(Open {
            ticket,
            grant: candidate.grant,
            head,
            stage: Stage::Composed(composition),
        });
        CombinedReserve::Combined(ticket, inbox)
    }

    fn plan(
        &mut self,
        ticket: u64,
        admitted: &CombinedFrameAdmitted<'_>,
    ) -> Option<CombinedRenderPlan> {
        let open = self
            .row
            .open
            .as_mut()
            .filter(|open| open.ticket == ticket)?;
        let Stage::Composed(composition) = std::mem::replace(&mut open.stage, Stage::Unplanned)
        else {
            return None;
        };
        let frame = self.turn.native.allocate_combined_frame();
        let (plan, dispatch) = self.turn.native.plan_combined_frame(
            composition,
            &open.head,
            frame,
            admitted.description,
            admitted.deadline,
        )?;
        open.stage = Stage::Planned(dispatch, admitted.binding, admitted.deadline);
        Some(plan)
    }

    /// One cancel per reservation attempt; the entry ends with it, so a
    /// second proposal composes again with fresh facts (R4). Only a
    /// confirmed cancellation completes it.
    fn cancel(&mut self, ticket: u64) {
        let Some(open) = self.row.open.take_if(|open| open.ticket == ticket) else {
            return;
        };
        let what = format!("combined reservation {ticket} not confirmed cancelled");
        #[cfg(test)]
        let identity = (open.grant, open.head.trace_identity());
        let _confirmed = self
            .row
            .cancel(&mut *self.turn.native, open.grant, open.head, what);
        #[cfg(test)]
        self.row
            .record_reserve_cancelled(ticket, identity, _confirmed);
    }

    fn submitted(
        &mut self,
        ticket: u64,
        reservation: CombinedReservation,
        returned: mpsc::Receiver<CombinedRenderPlan>,
    ) {
        let open = self
            .row
            .open
            .take_if(|open| open.ticket == ticket)
            .expect("the runtime submits the admission it planned");
        #[cfg(test)]
        let key = self.row.record_binding(ticket, &reservation, &open);
        let Stage::Planned(dispatch, binding, deadline) = open.stage else {
            unreachable!("only a planned admission is submitted");
        };
        self.row.jobs.push(Job {
            job: reservation.job(),
            grant: open.grant,
            head: open.head,
            dispatch,
            binding,
            deadline,
            verdict: Verdict::Open,
            state: State::InFlight {
                reservation,
                returned,
            },
            reported: false,
            #[cfg(test)]
            key,
        });
    }

    fn collect(
        &mut self,
        worker: &mut Worker,
        deliver: &mut dyn FnMut(u64, CombinedResult) -> Result<(), CombinedResult>,
    ) {
        let native = &mut *self.turn.native;
        #[cfg(test)]
        let mut refusals = std::mem::take(&mut self.row.refusals);
        let mut offer = |job: u64, result: CombinedResult| {
            #[cfg(test)]
            if refusals > 0 {
                refusals -= 1;
                return Err(result);
            }
            deliver(job, result)
        };
        for job in std::mem::take(&mut self.row.jobs) {
            if let Some(job) = self.row.collect_one(native, worker, &mut offer, job) {
                self.row.jobs.push(job);
            }
        }
        #[cfg(test)]
        {
            self.row.refusals = refusals;
        }
    }
}

impl ShellComponentConnections {
    /// The controlled driver's owner pass (R1, R4): `turn_connected` for the
    /// turn, whose one image-owner service per slot is hooked over that
    /// slot's persistent row, in every phase, revoked and replaced attempts
    /// included; then the row's walk. A row invariant of the current attempt
    /// is the slot's failure; one of an ended attempt is a diagnostic (R1).
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    pub(crate) fn turn_combined<N: ShellContentNative>(
        &mut self,
        turn: &mut CombinedTurn<'_, N>,
    ) -> Failures {
        self.turn_with(|slot, connection, epochs| {
            let current = connection
                .attempt
                .filter(|(_, phase)| *phase != ComponentConnectionPhase::Revoked)
                .map(|(grant, _)| grant);
            let instant = turn.instant;
            let serviced = match connection.images.as_mut() {
                Some(owner) => owner.service_combined(
                    &mut connection.transport,
                    epochs,
                    content_now_msec(),
                    instant,
                    &mut CombinedHooks {
                        row: &mut connection.combined,
                        turn: &mut *turn,
                        slot,
                    },
                ),
                None => Ok(()),
            };
            let view = connection.transport.connection(epochs);
            connection.combined.walk(
                &mut *turn.native,
                current,
                &|binding: FrameCopyBinding, job: u64| {
                    view.content_frame_candidate_state(binding, job)
                },
                instant,
            );
            serviced?;
            connection.combined.take_error(current).map_or(Ok(()), Err)
        })
    }

    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    pub(crate) fn combined_row(&self, slot: usize) -> &CombinedRow {
        &self.connections[slot].combined
    }

    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    pub(crate) fn combined_row_mut(&mut self, slot: usize) -> &mut CombinedRow {
        &mut self.connections[slot].combined
    }

    /// The slot's image-owner work, the other final-quiescence blocker.
    #[cfg_attr(not(test), allow(dead_code, reason = "used once gate 4 wires the row"))]
    pub(crate) fn image_pending(&self, slot: usize) -> usize {
        self.connections[slot]
            .images
            .as_ref()
            .map_or(0, |owner| owner.pending())
    }
}
