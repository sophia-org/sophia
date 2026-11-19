//! G3-5d d-2b1, test-support only: `turn_combined` split into the steps of
//! the controlled owner turn (REV2 R5, as amended by REV3 section 6), so the
//! Session driver can run content between the image-owner service and the
//! walk. Each step is `turn_combined`'s own code for its half: the images
//! step is its hooked image-owner service for every slot, in every phase;
//! the walk step is its walk and `take_error` for every slot. Nothing here
//! is a production caller; gate 4 decides the production turn.
//!
//! G3-5d b4: the row trace. Every call of the row that admits, collects,
//! steps, joins, removes, keeps or cancels an entry pushes one `Event` in
//! call order, named by the admission's `Key`; the checker in
//! tests/support/component_reconnect/combined_trace.rs replays them. The
//! records read what the call already holds; no clock, no service call.
use super::*;

/// One admission's key. Worker job numbers restart with every validator,
/// so the key is the row ticket with the reservation's validator serial and
/// job; it is made once, where the admission is submitted.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Key {
    pub(crate) ticket: u64,
    pub(crate) serial: u64,
    pub(crate) job: u64,
}

/// A job's state as the trace names it; `Failed` carries its disposition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Label {
    InFlight,
    Unsettled,
    Credit,
    Undelivered,
    Held,
    Failed(CombinedDisposition),
    Debt,
}

impl Label {
    pub(super) fn of(state: &State) -> Self {
        match state {
            State::InFlight { .. } => Label::InFlight,
            State::Unsettled { .. } => Label::Unsettled,
            State::Credit { .. } => Label::Credit,
            State::Undelivered { .. } => Label::Undelivered,
            State::Held => Label::Held,
            State::Failed(disposition) => Label::Failed(*disposition),
            State::Debt(_) => Label::Debt,
        }
    }

    /// The name `states()` reports.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Label::InFlight => "in flight",
            Label::Unsettled => "unsettled",
            Label::Credit => "credit",
            Label::Undelivered => "undelivered",
            Label::Held => "held",
            Label::Failed(_) => "failed",
            Label::Debt => "debt",
        }
    }
}

/// Why an entry left the row or stayed after a decision. The walk's
/// branches carry the current grant the walk compared against, as it
/// read it: ended when it differs from the entry's, current for a
/// disposal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Cause {
    Accepted,
    ConsumedFailed,
    Decline,
    WalkEndedHeld { current: Option<ContentGrant> },
    WalkEndedFailed { current: Option<ContentGrant> },
    DebtFlush,
    Dispose { current: Option<ContentGrant> },
    ReturnedPlan { confirmed: bool },
}

/// The facts of one `dispose` decision: the candidate state it read, the
/// deadline, whether it released, whether it set `Expired` in this call
/// and whether it took the fail-closed (Held, Expired) branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Dispose {
    pub(crate) candidate: FrameCandidateState,
    pub(crate) past: bool,
    pub(crate) released: bool,
    pub(crate) expired: bool,
    pub(crate) fail_closed: bool,
}

/// (native owner, head, facade ticket), from `CombinedHead`.
pub(crate) type HeadIdentity = (u64, u64, u64);

/// The arm `join` returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Arm {
    Ordinary,
    Held,
    Failed,
    Debt,
    Invariant,
}

impl Arm {
    fn of(join: &CombinedJoin) -> Self {
        match join {
            CombinedJoin::Ordinary => Arm::Ordinary,
            CombinedJoin::Held(_) => Arm::Held,
            CombinedJoin::Failed(_) => Arm::Failed,
            CombinedJoin::Debt(_) => Arm::Debt,
            CombinedJoin::Invariant(_) => Arm::Invariant,
        }
    }
}

/// One row call, recorded where it happens with the facts it already has.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Event {
    /// The admission submitted: the open ticket became an in-flight entry.
    Bound {
        key: Key,
        grant: ContentGrant,
        output: sophia_protocol::ContentOutputId,
        generation: u64,
        head: HeadIdentity,
    },
    /// The owner took this key's result, before settlement, with the
    /// identity read at that site: the reservation's validator serial and
    /// job, the entry's stored grant and the issuing head. The checker
    /// compares them with `Bound`.
    Collected {
        key: Key,
        serial: u64,
        reserved: u64,
        grant: ContentGrant,
        head: HeadIdentity,
        disposition: CombinedDisposition,
    },
    /// One `collect_one` call changed the state, or retried a refused
    /// settlement, credit clear or delivery (`from == to`).
    Stepped { key: Key, from: Label, to: Label },
    /// One bundle's join: the live entries before it, the exact entry it
    /// found, the arm it took and the matched entry's verdict after it.
    /// `admitted` is meaningful only with a match.
    Joined {
        grant: ContentGrant,
        output: sophia_protocol::ContentOutputId,
        generation: u64,
        arm: Arm,
        matched: Option<Key>,
        admitted: bool,
        verdict_after: Option<Verdict>,
        present: Vec<(Key, ContentGrant, Label, Verdict)>,
    },
    /// The entry left the row; `outcome` where a Release was attempted.
    Removed {
        key: Key,
        cause: Cause,
        outcome: Option<CombinedRelease>,
        dispose: Option<Dispose>,
    },
    /// The entry stayed after a Release attempt (`outcome: Some`) or a
    /// disposal keep (`outcome: None`), in `state`.
    Retained {
        key: Key,
        cause: Cause,
        outcome: Option<CombinedRelease>,
        state: Label,
        dispose: Option<Dispose>,
    },
    /// An open admission cancelled before submission; never a key.
    ReserveCancelled {
        ticket: u64,
        grant: ContentGrant,
        head: HeadIdentity,
        confirmed: bool,
    },
    /// The walk's retry of an unconfirmed cancellation.
    CancelLater {
        grant: ContentGrant,
        head: HeadIdentity,
        confirmed: bool,
    },
}

/// G3-5d d-2b2 (split review, b4 boundary): one admission's exact binding,
/// recorded in `submitted`, the one point where the row ticket, the
/// reservation's validator serial and job, the grant and the issuing head's
/// identity coexist. Nothing reconstructs it later from current state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Binding {
    pub(crate) ticket: u64,
    pub(crate) serial: u64,
    pub(crate) job: u64,
    pub(crate) grant: ContentGrant,
    /// (native owner, head, facade ticket), from `CombinedHead`.
    pub(crate) head: (u64, u64, u64),
}

/// G3-5d d-2b3b2 (row 5): one collected result's identity, recorded where
/// the row reads it, from the job and reservation stored at submission;
/// nothing is taken from the slot's current attempt or validator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Collection {
    pub(crate) job: u64,
    pub(crate) grant: ContentGrant,
    /// The reservation's own validator serial and job.
    pub(crate) serial: u64,
    pub(crate) reserved: u64,
    pub(crate) head: (u64, u64, u64),
    pub(crate) disposition: CombinedDisposition,
}

/// G3-5d d-2b3b2 (row 5): one bundle's join, recorded on entry with the
/// bundle's (grant, output, candidate generation) and every entry present,
/// as (job, grant stored at submission, state); `matched` is the entry the
/// join selected, None when it took the ordinary arm.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Join {
    pub(crate) grant: ContentGrant,
    pub(crate) output: sophia_protocol::ContentOutputId,
    pub(crate) generation: u64,
    pub(crate) present: Vec<(u64, ContentGrant, &'static str)>,
    pub(crate) matched: Option<u64>,
}

impl CombinedRow {
    pub(super) fn record_collection(
        &mut self,
        job: &Job,
        reservation: &CombinedReservation,
        disposition: CombinedDisposition,
    ) {
        let collection = Collection {
            job: job.job,
            grant: job.grant,
            serial: reservation.validator_serial(),
            reserved: reservation.job(),
            head: job.head.trace_identity(),
            disposition,
        };
        self.collections.push(collection);
        self.trace.push(Event::Collected {
            key: job.key,
            serial: collection.serial,
            reserved: collection.reserved,
            grant: collection.grant,
            head: collection.head,
            disposition,
        });
    }

    /// On entry: the join and its `Joined`, with every live entry before
    /// the join; the arm and match are filled in at the end.
    pub(super) fn record_join(&mut self, bundle: &ContentRenderBundle) {
        let present = self
            .jobs
            .iter()
            .zip(self.states())
            .map(|(job, (_, state))| (job.job, job.grant, state))
            .collect();
        self.joins.push(Join {
            grant: bundle.grant,
            output: bundle.output,
            generation: bundle.candidate_generation,
            present,
            matched: None,
        });
        let present = self
            .jobs
            .iter()
            .map(|job| (job.key, job.grant, Label::of(&job.state), job.verdict))
            .collect();
        self.trace.push(Event::Joined {
            grant: bundle.grant,
            output: bundle.output,
            generation: bundle.candidate_generation,
            arm: Arm::Ordinary,
            matched: None,
            admitted: false,
            verdict_after: None,
            present,
        });
    }

    /// At the end of a join that found the exact entry: (job, key, the
    /// admitted bit, the verdict after the join's own write) and the arm.
    pub(super) fn record_join_match(
        &mut self,
        (job, key, admitted, verdict): (u64, Key, bool, Verdict),
        join: &CombinedJoin,
    ) {
        if let Some(join) = self.joins.last_mut() {
            join.matched = Some(job);
        }
        if let Some(Event::Joined {
            arm,
            matched,
            admitted: was_admitted,
            verdict_after,
            ..
        }) = self.trace.last_mut()
        {
            *arm = Arm::of(join);
            *matched = Some(key);
            *was_admitted = admitted;
            *verdict_after = Some(verdict);
        }
    }

    /// After `released` decided: kept in its new state, or removed.
    pub(super) fn record_release(
        &mut self,
        job: &Job,
        cause: Cause,
        outcome: CombinedRelease,
        kept: bool,
        dispose: Option<Dispose>,
    ) {
        let key = job.key;
        let outcome = Some(outcome);
        self.trace.push(if kept {
            Event::Retained {
                key,
                cause,
                outcome,
                state: Label::of(&job.state),
                dispose,
            }
        } else {
            Event::Removed {
                key,
                cause,
                outcome,
                dispose,
            }
        });
    }

    /// A disposal that attempted no Release: kept unchanged, or retired.
    pub(super) fn record_kept(
        &mut self,
        job: &Job,
        kept: bool,
        dispose: Dispose,
        current: Option<ContentGrant>,
    ) {
        let key = job.key;
        let cause = Cause::Dispose { current };
        self.trace.push(if kept {
            Event::Retained {
                key,
                cause,
                outcome: None,
                state: Label::of(&job.state),
                dispose: Some(dispose),
            }
        } else {
            Event::Removed {
                key,
                cause,
                outcome: None,
                dispose: Some(dispose),
            }
        });
    }

    /// A removal with no Release and no disposal facts.
    pub(super) fn record_removed(&mut self, key: Key, cause: Cause) {
        self.trace.push(Event::Removed {
            key,
            cause,
            outcome: None,
            dispose: None,
        });
    }

    /// `accepted`/`consumed_failed`: the removal, when one happened.
    pub(super) fn record_consumed(&mut self, removed: Option<&Job>, cause: Cause) {
        if let Some(job) = removed {
            self.record_removed(job.key, cause);
        }
    }

    /// At the end of `collect_one`: the step, when the state changed or a
    /// refused settlement, credit clear or delivery was retried.
    pub(super) fn record_step(&mut self, job: &Job, from: Label) {
        let to = Label::of(&job.state);
        if to != from || matches!(from, Label::Unsettled | Label::Credit | Label::Undelivered) {
            self.trace.push(Event::Stepped {
                key: job.key,
                from,
                to,
            });
        }
    }

    pub(super) fn record_cancel_later(
        &mut self,
        grant: ContentGrant,
        head: &CombinedHead,
        confirmed: bool,
    ) {
        self.trace.push(Event::CancelLater {
            grant,
            head: head.trace_identity(),
            confirmed,
        });
    }

    pub(super) fn record_reserve_cancelled(
        &mut self,
        ticket: u64,
        (grant, head): (ContentGrant, HeadIdentity),
        confirmed: bool,
    ) {
        self.trace.push(Event::ReserveCancelled {
            ticket,
            grant,
            head,
            confirmed,
        });
    }

    /// The row's trace, in call order.
    pub(crate) fn trace(&self) -> &[Event] {
        &self.trace
    }

    /// Every live entry's key and state, for the trace's terminal check.
    pub(crate) fn live_keys(&self) -> Vec<(Key, Label)> {
        self.jobs
            .iter()
            .map(|job| (job.key, Label::of(&job.state)))
            .collect()
    }

    /// Every unconfirmed cancellation's grant and head identity, in order
    /// (REV4 R2); the count is `unconfirmed_cancels`.
    pub(crate) fn cancel_identities(&self) -> Vec<(ContentGrant, HeadIdentity)> {
        self.cancels
            .iter()
            .map(|(grant, head)| (*grant, head.trace_identity()))
            .collect()
    }

    /// Each job and the name of its state, for the controls.
    pub(crate) fn states(&self) -> Vec<(u64, &'static str)> {
        self.jobs
            .iter()
            .map(|job| (job.job, Label::of(&job.state).name()))
            .collect()
    }

    /// The reserve-time projections so far, in proposal order.
    pub(crate) fn projections(&self) -> &[LiveShellContentFrame] {
        &self.projections
    }

    /// The job's candidate binding, for the controls' candidate facts.
    pub(crate) fn binding(&self, job: u64) -> Option<FrameCopyBinding> {
        self.jobs
            .iter()
            .find(|held| held.job == job)
            .map(|held| held.binding)
    }

    /// The job's verdict, for the controls.
    pub(crate) fn verdict(&self, job: u64) -> Option<Verdict> {
        self.jobs
            .iter()
            .find(|held| held.job == job)
            .map(|held| held.verdict)
    }

    /// Errors reported after their attempt ended, with that attempt's grant.
    pub(crate) fn cleanup_diagnostics(&self) -> &[(ContentGrant, String)] {
        &self.diagnostics
    }

    /// A declared fault: the next delivery is refused at the runtime
    /// boundary, as a refused `accept` would be. It proves the row keeps and
    /// re-offers the result and reports once, not that a real refusal
    /// happens.
    pub(crate) fn declare_delivery_refusal(&mut self) {
        self.refusals += 1;
    }

    /// Every collection recorded on this row, in collection order.
    pub(crate) fn collections(&self) -> &[Collection] {
        &self.collections
    }

    /// Every join recorded on this row, in call order.
    pub(crate) fn joins(&self) -> &[Join] {
        &self.joins
    }

    /// The job's kept Release outcome, if the row holds it as Debt.
    pub(crate) fn debt(&self, job: u64) -> Option<CombinedRelease> {
        self.jobs
            .iter()
            .find(|held| held.job == job)
            .and_then(|held| match &held.state {
                State::Debt(release) => Some(*release),
                _ => None,
            })
    }

    /// The binding and the trace's `Bound`; the key every later event of
    /// this admission names.
    pub(super) fn record_binding(
        &mut self,
        ticket: u64,
        reservation: &CombinedReservation,
        open: &Open,
    ) -> Key {
        let key = Key {
            ticket,
            serial: reservation.validator_serial(),
            job: reservation.job(),
        };
        self.bindings.push(Binding {
            ticket,
            serial: key.serial,
            job: key.job,
            grant: open.grant,
            head: open.head.trace_identity(),
        });
        let Stage::Planned(_, binding, _) = &open.stage else {
            unreachable!("only a planned admission is submitted");
        };
        self.trace.push(Event::Bound {
            key,
            grant: open.grant,
            output: binding.key.output,
            generation: binding.generation,
            head: open.head.trace_identity(),
        });
        key
    }

    /// Every binding recorded on this row, in submission order.
    pub(crate) fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Every collected result's job and disposition, in collection order.
    pub(crate) fn collected(&self) -> &[(u64, CombinedDisposition)] {
        &self.collected
    }

    /// The job's admitted validation deadline, unchanged since admission.
    pub(crate) fn admitted_deadline(&self, job: u64) -> Option<Instant> {
        self.jobs
            .iter()
            .find(|held| held.job == job)
            .map(|held| held.deadline)
    }

    /// REV3 5.1, labelled: the slot's next image-owner service that returns
    /// Ok in the images step fails with `reason` instead, after the service
    /// collected and reconciled what came back. It proves the driver's
    /// failure order on a real collected result, not that the failure
    /// happens there.
    pub(crate) fn declare_image_service_fault(&mut self, reason: &'static str) {
        self.image_fault = Some(reason);
    }
}

impl ShellComponentConnections {
    /// The capture work final quiescence checks first (d-2b3b2 row 7), for
    /// a control showing that guard clear; only a capture build has it.
    #[cfg(feature = "experimental-shell-capture")]
    pub(crate) fn capture_work(&self) -> usize {
        self.capture_pending()
    }

    /// The slot's validation worker (d-2b2), for a control's hold, progress
    /// and serial handles only; None before the slot has an image owner.
    pub(crate) fn combined_validator(&mut self, slot: usize) -> Option<&mut Worker> {
        self.connections[slot]
            .images
            .as_mut()
            .map(|owner| owner.validation_worker())
    }
}

// G3-5d gate 4 C3b: the monolithic controlled turn and the row accessors,
// moved text-unchanged from combined_row.rs (test-only; the file keeps its
// source-layout limit). The monolithic turn writes the row's witness pass
// as the split steps do, and records nothing else.
impl ShellComponentConnections {
    /// The controlled driver's owner pass (R1, R4): `turn_connected` for the
    /// turn, whose one image-owner service per slot is hooked over that
    /// slot's persistent row, in every phase, revoked and replaced attempts
    /// included; then the row's walk. A row invariant of the current attempt
    /// is the slot's failure; one of an ended attempt is a diagnostic (R1).
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
            #[cfg(feature = "diagnostic-combined-witness")]
            {
                connection.combined.witness.at = PassSlot {
                    pass: turn.witness,
                    slot,
                };
            }
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
