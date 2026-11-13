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
//! back and retry only their own step. Their invariant error is the slot's
//! failure, reported once, and reporting never discards the row or an owner.
use super::*;
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
    CombinedReserve, CombinedResult, CombinedToken, IsbValidationFailure, IsbValidationWorker,
    ValidatedIsbPixels,
};
use std::sync::mpsc;
use std::time::Instant;

type Worker = IsbValidationWorker<CombinedRenderPlan>;
type Failures = [Option<(ComponentConnectionKey, ShellTransportError)>; MAX_SHELL_COMPONENTS];

/// What the hooks read from the owner turn, built once per controlled turn.
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
}

/// The owner loop's cursor rule over one pointer reading, and that reading
/// as the plane position.
pub(crate) fn turn_cursor(
    native: bool,
    position: Option<Point>,
) -> (LiveProductionCursorPresentation, Option<Point>) {
    (
        crate::live_session::turn_cursor_presentation(native, position),
        position,
    )
}

#[derive(Default)]
pub(crate) struct CombinedRow {
    next_ticket: u64,
    /// The one admission between `reserve` and its end in `cancel` or
    /// `submitted`, inside one proposal.
    open: Option<Open>,
    jobs: Vec<Job>,
    /// Reservations whose cancellation the facade did not confirm; the walk
    /// retries them.
    cancels: Vec<CombinedHead>,
    /// Invariant failures not yet reported.
    errors: Vec<String>,
    /// Declared delivery faults still to inject.
    refusals: usize,
}

struct Open {
    ticket: u64,
    grant: ContentGrant,
    head: CombinedHead,
    stage: Stage,
}

enum Stage {
    Composed(CombinedComposition),
    Planned(CombinedDispatch),
    /// The plan was not built; the composition went with that attempt.
    Unplanned,
}

struct Job {
    job: u64,
    grant: ContentGrant,
    head: CombinedHead,
    /// c's consumption reads it; b only keeps it with its job.
    #[allow(dead_code)]
    dispatch: CombinedDispatch,
    state: State,
    /// This job's invariant failure was reported; its retries stay quiet.
    reported: bool,
}

enum State {
    /// The validator owns the job; a failed validation or a refused move
    /// returns the plan on `returned`.
    InFlight {
        reservation: CombinedReservation,
        returned: mpsc::Receiver<CombinedRenderPlan>,
    },
    /// Settlement refused and returned everything; it is retried.
    Unsettled {
        reservation: CombinedReservation,
        result: Box<CombinedRenderResult>,
        disposition: CombinedDisposition,
    },
    /// The facade took its part; only the credit clear is retried, with the
    /// original worker and the returned reservation and token.
    Credit {
        reservation: CombinedReservation,
        token: CombinedToken,
        pixels: ValidatedIsbPixels,
        disposition: CombinedDisposition,
    },
    /// Settled; the delivery was refused and is offered again.
    Undelivered {
        result: CombinedResult,
        disposition: CombinedDisposition,
    },
    /// Exported and delivered: the issuing facade holds the lease.
    Held,
    /// Delivered with no lease held, for c's terminal or its grant's end.
    Failed(#[expect(dead_code, reason = "c reads it as its terminal input")] CombinedDisposition),
    /// A released Held result whose Release did not transfer.
    Debt(#[expect(dead_code, reason = "kept as the last Release outcome")] CombinedRelease),
}

/// What one collection step leaves: a state to keep, or a settled result to
/// deliver.
enum Step {
    Keep(State),
    Deliver(CombinedResult, CombinedDisposition),
}

fn report(errors: &mut Vec<String>, reported: &mut bool, job: u64, what: &str) {
    if !*reported {
        *reported = true;
        errors.push(format!("combined job {job}: {what}"));
    }
}

/// Mismatch's pixels drop here, on the owner thread; its job is Malformed.
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

    /// Each job and the name of its state, for the controls.
    pub(crate) fn states(&self) -> Vec<(u64, &'static str)> {
        self.jobs
            .iter()
            .map(|job| {
                let state = match &job.state {
                    State::InFlight { .. } => "in flight",
                    State::Unsettled { .. } => "unsettled",
                    State::Credit { .. } => "credit",
                    State::Undelivered { .. } => "undelivered",
                    State::Held => "held",
                    State::Failed(_) => "failed",
                    State::Debt(_) => "debt",
                };
                (job.job, state)
            })
            .collect()
    }

    pub(crate) fn unconfirmed_cancels(&self) -> usize {
        self.cancels.len()
    }

    /// A declared fault: the next delivery is refused at the runtime
    /// boundary, as a refused `accept` would be. It proves the row keeps and
    /// re-offers the result and reports once, not that a real refusal
    /// happens.
    pub(crate) fn declare_delivery_refusal(&mut self) {
        self.refusals += 1;
    }

    /// Declines a Held result of the current grant (c's decline, and b's
    /// controls'): its lease is retained for Release. `None` when `job` is
    /// not Held.
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
        if self.released(&mut held, outcome) {
            self.jobs.push(held);
        }
        Some(outcome)
    }

    /// True when `job` must stay, as Debt, after a Release attempt.
    fn released(&mut self, job: &mut Job, outcome: CombinedRelease) -> bool {
        if outcome == CombinedRelease::Transferred {
            return false;
        }
        if outcome == CombinedRelease::Mismatch {
            report(
                &mut self.errors,
                &mut job.reported,
                job.job,
                "the Release cell does not hold this job",
            );
        }
        job.state = State::Debt(outcome);
        true
    }

    /// Disposal and debt maintenance, after the slot's one hooked service.
    /// It takes no results. A Held or Failed job whose grant ended can never
    /// be accepted: Held releases, Failed retires, since its delivery already
    /// transferred. Debt flushes until Transferred.
    fn walk<N: ShellContentNative>(&mut self, native: &mut N, current: Option<ContentGrant>) {
        for head in std::mem::take(&mut self.cancels) {
            if !matches!(native.cancel_combined_frame(&head), Ok(true)) {
                self.cancels.push(head);
            }
        }
        for mut job in std::mem::take(&mut self.jobs) {
            let ended = current != Some(job.grant);
            let keep = match job.state {
                State::Held if ended => {
                    let outcome = native.release_combined_held(&job.head, job.job);
                    self.released(&mut job, outcome)
                }
                State::Debt(_) => {
                    let outcome = native.flush_combined_release(&job.head, job.job);
                    self.released(&mut job, outcome)
                }
                State::Failed(_) if ended => false,
                _ => true,
            };
            if keep {
                self.jobs.push(job);
            }
        }
    }

    fn take_error(&mut self) -> Option<ShellTransportError> {
        (!self.errors.is_empty()).then(|| ShellTransportError::CombinedRow(self.errors.remove(0)))
    }

    fn cancel<N: ShellContentNative>(&mut self, native: &mut N, head: CombinedHead, what: String) {
        if !matches!(native.cancel_combined_frame(&head), Ok(true)) {
            self.errors.push(what);
            self.cancels.push(head);
        }
    }

    /// One job's collection; `None` when the job ended here.
    fn collect_one<N: ShellContentNative>(
        &mut self,
        native: &mut N,
        worker: &mut Worker,
        deliver: &mut dyn FnMut(u64, CombinedResult) -> Result<(), CombinedResult>,
        mut job: Job,
    ) -> Option<Job> {
        // Every path below sets the state again or ends the job.
        let state = std::mem::replace(&mut job.state, State::Held);
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
                    self.cancel(native, job.head, what);
                    return None;
                }
                match native.take_combined_frame_result(&job.head) {
                    Ok(None) => Step::Keep(State::InFlight {
                        reservation,
                        returned,
                    }),
                    Ok(Some(result)) => {
                        let disposition = result.disposition();
                        self.settle(native, worker, &mut job, reservation, result, disposition)
                    }
                    Err(error) => {
                        let what = format!("result route: {error}");
                        report(&mut self.errors, &mut job.reported, job.job, &what);
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
                    report(
                        &mut self.errors,
                        &mut job.reported,
                        job.job,
                        "the runtime refused its result",
                    );
                    State::Undelivered {
                        result,
                        disposition,
                    }
                }
            },
        };
        Some(job)
    }

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
        report(&mut self.errors, &mut job.reported, job.job, what);
        Step::Keep(state)
    }
}

/// The hooks for one slot's one image-owner service.
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
        open.stage = Stage::Planned(dispatch);
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
        self.row.cancel(&mut *self.turn.native, open.head, what);
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
        let Stage::Planned(dispatch) = open.stage else {
            unreachable!("only a planned admission is submitted");
        };
        self.row.jobs.push(Job {
            job: reservation.job(),
            grant: open.grant,
            head: open.head,
            dispatch,
            state: State::InFlight {
                reservation,
                returned,
            },
            reported: false,
        });
    }

    fn collect(
        &mut self,
        worker: &mut Worker,
        deliver: &mut dyn FnMut(u64, CombinedResult) -> Result<(), CombinedResult>,
    ) {
        let native = &mut *self.turn.native;
        let mut refusals = std::mem::take(&mut self.row.refusals);
        let mut offer = |job: u64, result: CombinedResult| {
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
        self.row.refusals = refusals;
    }
}

impl ShellComponentConnections {
    /// The controlled driver's owner pass (R1, R4): `turn_connected` for the
    /// turn, whose one image-owner service per slot is hooked over that
    /// slot's persistent row, in every phase, revoked and replaced attempts
    /// included; then the row's walk. A row invariant is the slot's failure.
    pub(crate) fn turn_combined<N: ShellContentNative>(
        &mut self,
        turn: &mut CombinedTurn<'_, N>,
    ) -> Failures {
        self.turn_with(|slot, connection, epochs| {
            let current = connection
                .attempt
                .filter(|(_, phase)| *phase != ComponentConnectionPhase::Revoked)
                .map(|(grant, _)| grant);
            let serviced = match connection.images.as_mut() {
                Some(owner) => owner.service_combined(
                    &mut connection.transport,
                    epochs,
                    content_now_msec(),
                    Instant::now(),
                    &mut CombinedHooks {
                        row: &mut connection.combined,
                        turn: &mut *turn,
                        slot,
                    },
                ),
                None => Ok(()),
            };
            connection.combined.walk(&mut *turn.native, current);
            serviced?;
            connection.combined.take_error().map_or(Ok(()), Err)
        })
    }

    pub(crate) fn combined_row(&self, slot: usize) -> &CombinedRow {
        &self.connections[slot].combined
    }

    pub(crate) fn combined_row_mut(&mut self, slot: usize) -> &mut CombinedRow {
        &mut self.connections[slot].combined
    }

    /// The slot's image-owner work, the other final-quiescence blocker.
    pub(crate) fn image_pending(&self, slot: usize) -> usize {
        self.connections[slot]
            .images
            .as_ref()
            .map_or(0, |owner| owner.pending())
    }
}
