//! G3-5d b4: the row trace checker (design REV3 sections 3-4 with the REV4
//! delta). `replay` folds a row's trace into one record per admission and
//! rejects the first event the row's own rules do not permit: one `Bound`
//! per key, first (rule 1); at most one `Collected`, in flight (2); every
//! step, removal and retention an edge of the table with its prerequisite,
//! `released`'s one rule included (3); every join complete over the live
//! set, with the arm the table gives and `Consumed` written once (4);
//! `Accepted` straight after the key's Held join (5); a decline from Held
//! only (6); one live entry per grant, output and generation (7);
//! cancellation identities as a multiset (8); a removed key ends (9); the
//! fail-closed disposal undeclared (10). The checkpoints compare the
//! replayed live set and owed cancellations with the row's own snapshot.
//! This is a scenario and order check over the row's recorded calls: it
//! sees no validator, renderer, journal, native presentation or peer, and
//! it is not a refinement of any model.
use super::*;
use crate::shell_component_connections::combined_row::steps::{
    Arm, Cause, Dispose, Event, HeadIdentity, Key, Label,
};
use crate::shell_component_connections::combined_row::{CombinedRow, Verdict};
use sophia_backend_live::CombinedDisposition::{Expired, Mismatch, Rendered};
use sophia_backend_live::{CombinedDisposition, CombinedRelease};
use sophia_protocol::ContentOutputId;
use sophia_runtime::FrameCandidateState;
use sophia_runtime::FrameCandidateState::{Assembling, Ended, Pending, Submitted, Unknown};

type Check = Result<(), String>;

fn require(ok: bool, why: &str) -> Check {
    if ok { Ok(()) } else { Err(why.to_string()) }
}

/// One admission as the trace describes it so far.
#[derive(Debug)]
struct Row {
    key: Key,
    grant: ContentGrant,
    output: ContentOutputId,
    generation: u64,
    head: HeadIdentity,
    label: Label,
    verdict: Verdict,
    disposition: Option<CombinedDisposition>,
    collected: bool,
    joined: Option<Arm>,
    /// The key's last event was its Held join (rule 5).
    just_joined_held: bool,
    ended: bool,
}

#[derive(Debug, Default)]
pub(super) struct Replay {
    rows: Vec<Row>,
    /// Cancellations owed, in order: every unconfirmed returned plan or
    /// open-reservation cancel not yet confirmed by a `CancelLater`.
    pending: Vec<(ContentGrant, HeadIdentity)>,
    cancelled_tickets: Vec<u64>,
    /// Joins that found no entry.
    ordinary: usize,
}

/// The trace replayed, or the first event the rules reject, named.
pub(super) fn replay(trace: &[Event]) -> Result<Replay, String> {
    let mut replay = Replay::default();
    for (index, event) in trace.iter().enumerate() {
        replay
            .step(event)
            .map_err(|why| format!("event {index} {event:?}: {why}"))?;
    }
    Ok(replay)
}

impl Replay {
    fn live(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter().filter(|row| !row.ended)
    }

    /// Every live key and its replayed state, by key.
    pub(super) fn live_set(&self) -> Vec<(Key, Label)> {
        let mut live: Vec<_> = self.live().map(|row| (row.key, row.label)).collect();
        live.sort_by_key(|(key, _)| *key);
        live
    }

    /// The owed cancellations, sorted, duplicates kept (REV4 R2).
    pub(super) fn pending(&self) -> Vec<(ContentGrant, HeadIdentity)> {
        let mut pending = self.pending.clone();
        pending.sort();
        pending
    }

    pub(super) fn ordinary_joins(&self) -> usize {
        self.ordinary
    }

    fn row(&mut self, key: Key) -> Result<&mut Row, String> {
        let row = self
            .rows
            .iter_mut()
            .find(|row| row.key == key)
            .ok_or("rule 1: an event before its Bound")?;
        require(!row.ended, "rule 9: an event after the key ended")?;
        Ok(row)
    }

    fn step(&mut self, event: &Event) -> Check {
        match event {
            Event::Bound {
                key,
                grant,
                output,
                generation,
                head,
            } => {
                require(
                    !self.rows.iter().any(|row| row.key == *key),
                    "rule 1: a second Bound of the key",
                )?;
                require(
                    !self.rows.iter().any(|row| row.key.ticket == key.ticket)
                        && !self.cancelled_tickets.contains(&key.ticket),
                    "rule 1: the ticket was used before",
                )?;
                require(
                    !self.live().any(|row| {
                        (row.grant, row.output, row.generation) == (*grant, *output, *generation)
                    }),
                    "rule 7: a live entry has this grant, output and generation",
                )?;
                self.rows.push(Row {
                    key: *key,
                    grant: *grant,
                    output: *output,
                    generation: *generation,
                    head: *head,
                    label: Label::InFlight,
                    verdict: Verdict::Open,
                    disposition: None,
                    collected: false,
                    joined: None,
                    just_joined_held: false,
                    ended: false,
                });
                Ok(())
            }
            Event::Collected {
                key,
                serial,
                reserved,
                grant,
                head,
                disposition,
            } => {
                let row = self.row(*key)?;
                row.just_joined_held = false;
                require(
                    (*serial, *reserved) == (key.serial, key.job),
                    "rule 2: the collected reservation is not the bound one",
                )?;
                require(
                    *grant == row.grant,
                    "rule 2: the collected grant is not the bound one",
                )?;
                require(
                    *head == row.head,
                    "rule 2: the collected head is not the bound one",
                )?;
                require(
                    row.label == Label::InFlight,
                    "rule 2: collected outside in flight",
                )?;
                require(!row.collected, "rule 2: a second Collected")?;
                row.collected = true;
                row.disposition = Some(*disposition);
                Ok(())
            }
            Event::Stepped { key, from, to } => {
                let row = self.row(*key)?;
                row.just_joined_held = false;
                require(
                    row.label == *from,
                    "rule 3: the step's from is not the replayed state",
                )?;
                edge(row, *from, *to)?;
                row.label = *to;
                Ok(())
            }
            Event::Joined {
                grant,
                output,
                generation,
                arm,
                matched,
                admitted,
                verdict_after,
                present,
            } => self.joined(
                (*grant, *output, *generation),
                *arm,
                *matched,
                *admitted,
                *verdict_after,
                present,
            ),
            Event::Removed {
                key,
                cause,
                outcome,
                dispose,
            } => self.removed(*key, *cause, *outcome, *dispose),
            Event::Retained {
                key,
                cause,
                outcome,
                state,
                dispose,
            } => {
                let row = self.row(*key)?;
                row.just_joined_held = false;
                retained(row, *cause, *outcome, *state, *dispose)
            }
            Event::ReserveCancelled {
                ticket,
                grant,
                head,
                confirmed,
            } => {
                require(
                    !self.rows.iter().any(|row| row.key.ticket == *ticket),
                    "rule 8: a bound ticket was cancelled as an open reservation",
                )?;
                self.cancelled_tickets.push(*ticket);
                if !confirmed {
                    self.pending.push((*grant, *head));
                }
                Ok(())
            }
            Event::CancelLater {
                grant,
                head,
                confirmed,
            } => {
                let position = self
                    .pending
                    .iter()
                    .position(|owed| *owed == (*grant, *head))
                    .ok_or("rule 8: no owed cancellation with this identity")?;
                self.pending.remove(position);
                if !confirmed {
                    self.pending.push((*grant, *head));
                }
                Ok(())
            }
        }
    }

    fn joined(
        &mut self,
        bundle: (ContentGrant, ContentOutputId, u64),
        arm: Arm,
        matched: Option<Key>,
        admitted: bool,
        verdict_after: Option<Verdict>,
        present: &[(Key, ContentGrant, Label, Verdict)],
    ) -> Check {
        let mut live: Vec<_> = self
            .live()
            .map(|row| (row.key, row.grant, row.label, row.verdict))
            .collect();
        live.sort_by_key(|(key, ..)| *key);
        let mut recorded = present.to_vec();
        recorded.sort_by_key(|(key, ..)| *key);
        require(
            live == recorded,
            "rule 4: present differs from the replayed live set",
        )?;
        let hits: Vec<Key> = self
            .live()
            .filter(|row| (row.grant, row.output, row.generation) == bundle)
            .map(|row| row.key)
            .collect();
        require(
            hits.len() <= 1,
            "rule 7: two live entries share the bundle's grant, output and generation",
        )?;
        require(
            matched == hits.first().copied(),
            "rule 4: matched is not the exact live entry",
        )?;
        let Some(key) = matched else {
            require(
                arm == Arm::Ordinary && verdict_after.is_none(),
                "rule 4: with no entry the join is Ordinary and writes nothing",
            )?;
            self.ordinary += 1;
            return Ok(());
        };
        let row = self.row(key)?;
        let expected = if admitted {
            match (row.label, row.verdict) {
                (Label::Held, Verdict::Open) => Arm::Held,
                (Label::Failed(d), Verdict::Open | Verdict::Expired) if d != Mismatch => {
                    Arm::Failed
                }
                (Label::Debt, Verdict::Expired) => Arm::Debt,
                _ => Arm::Invariant,
            }
        } else {
            Arm::Invariant
        };
        require(
            arm == expected,
            "rule 4: the arm is not the table's for the entry's state and verdict",
        )?;
        if arm == Arm::Invariant {
            require(
                verdict_after == Some(row.verdict),
                "rule 4: an Invariant join changed the verdict",
            )?;
            row.just_joined_held = false;
        } else {
            require(
                verdict_after == Some(Verdict::Consumed),
                "rule 4: a matched join leaves Consumed",
            )?;
            row.verdict = Verdict::Consumed;
            row.joined = Some(arm);
            row.just_joined_held = arm == Arm::Held;
        }
        Ok(())
    }

    fn removed(
        &mut self,
        key: Key,
        cause: Cause,
        outcome: Option<CombinedRelease>,
        dispose: Option<Dispose>,
    ) -> Check {
        let row = self.row(key)?;
        let just_joined_held = std::mem::replace(&mut row.just_joined_held, false);
        let mut owed = None;
        match (cause, outcome) {
            (Cause::Accepted, None) => {
                require(
                    row.label == Label::Held && row.verdict == Verdict::Consumed,
                    "rule 5: accepted needs a Consumed held entry",
                )?;
                require(
                    just_joined_held,
                    "rule 5: accepted follows the key's Held join with nothing between",
                )?;
            }
            (Cause::ConsumedFailed, None) => require(
                matches!(row.label, Label::Failed(_))
                    && row.verdict == Verdict::Consumed
                    && row.joined == Some(Arm::Failed),
                "rule 3: consumed_failed needs a Consumed failed entry after its Failed join",
            )?,
            (Cause::WalkEndedFailed { current }, None) => {
                require(
                    matches!(row.label, Label::Failed(_)),
                    "rule 3: the walk drops only a failed entry of an ended grant",
                )?;
                ended_grant(row, current)?;
            }
            (Cause::ReturnedPlan { confirmed }, None) => {
                require(
                    row.label == Label::InFlight && !row.collected,
                    "rule 3: a returned plan ends only an uncollected in-flight entry",
                )?;
                if !confirmed {
                    owed = Some((row.grant, row.head));
                }
            }
            (Cause::Dispose { current }, None) => {
                let facts = dispose.ok_or("rule 3: a disposal removal carries its facts")?;
                require(
                    matches!(row.label, Label::Failed(_)) && !facts.released,
                    "rule 3: a disposal retires only a failed entry, without a release",
                )?;
                current_grant(row, current)?;
                retire_allowed(row, facts)?;
            }
            (
                Cause::Decline
                | Cause::WalkEndedHeld { .. }
                | Cause::Dispose { .. }
                | Cause::DebtFlush,
                Some(outcome),
            ) => released(row, cause, outcome, None, dispose)?,
            _ => {
                return Err(format!(
                    "rule 3: {cause:?} with outcome {outcome:?} is not a removal"
                ));
            }
        }
        row.ended = true;
        if let Some(owed) = owed {
            self.pending.push(owed);
        }
        Ok(())
    }
}

/// The step table (REV3 section 3): every edge out of in flight needs the
/// key's Collected; held needs Rendered; failed(d) carries the collected
/// d; the retry states go forward only.
fn edge(row: &Row, from: Label, to: Label) -> Check {
    use Label::{Credit, Debt, Held, InFlight, Undelivered, Unsettled};
    require(
        !matches!(from, Held | Label::Failed(_) | Debt),
        "rule 3: held, failed and debt change only through Removed or Retained",
    )?;
    require(
        !(from == InFlight && to == InFlight),
        "rule 3: in flight to in flight is never recorded",
    )?;
    require(
        row.collected,
        "rule 3: an edge out of in flight needs the key's Collected",
    )?;
    match to {
        Held => require(
            row.disposition == Some(Rendered),
            "rule 3: held needs the collected Rendered",
        ),
        Label::Failed(d) => require(
            row.disposition == Some(d) && d != Rendered,
            "rule 3: failed(d) carries the collected d, never Rendered",
        ),
        Unsettled => require(
            matches!(from, InFlight | Unsettled),
            "rule 3: unsettled follows only in flight or unsettled",
        ),
        Credit => require(
            matches!(from, InFlight | Unsettled | Credit),
            "rule 3: credit follows only in flight, unsettled or credit",
        ),
        Undelivered => Ok(()),
        InFlight | Debt => Err("rule 3: no step enters in flight or debt".to_string()),
    }
}

/// `released`'s one rule (REV4 R1) after the cause's own precondition:
/// Transferred removes, or keeps failed(Expired) under an Expired verdict;
/// any other outcome keeps debt. `kept` is the recorded post-state, None
/// for a removal.
/// The walk's ended branches: the current grant it read is not the entry's.
fn ended_grant(row: &Row, current: Option<ContentGrant>) -> Check {
    require(
        current != Some(row.grant),
        "rule 3: an ended-grant branch on the entry's current grant",
    )
}

/// The walk's disposal: the current grant it read is the entry's.
fn current_grant(row: &Row, current: Option<ContentGrant>) -> Check {
    require(
        current == Some(row.grant),
        "rule 3: a disposal of an entry whose grant is not current",
    )
}

fn released(
    row: &mut Row,
    cause: Cause,
    outcome: CombinedRelease,
    kept: Option<Label>,
    dispose: Option<Dispose>,
) -> Check {
    match cause {
        Cause::Decline => require(
            row.label == Label::Held,
            "rule 6: a decline needs a held entry",
        )?,
        Cause::WalkEndedHeld { current } => {
            require(
                row.label == Label::Held,
                "rule 6: an ended-grant release needs a held entry",
            )?;
            ended_grant(row, current)?;
        }
        Cause::DebtFlush => require(
            row.label == Label::Debt,
            "rule 3: a debt flush needs a debt entry",
        )?,
        Cause::Dispose { current } => {
            let facts = dispose.ok_or("rule 3: a disposal release carries its facts")?;
            require(
                row.label == Label::Held && facts.released,
                "rule 3: a disposal releases only a held entry",
            )?;
            current_grant(row, current)?;
            release_allowed(row, facts)?;
        }
        _ => return Err(format!("rule 3: {cause:?} attempts no Release")),
    }
    let expected = match (outcome, row.verdict) {
        (CombinedRelease::Transferred, Verdict::Expired) => Some(Label::Failed(Expired)),
        (CombinedRelease::Transferred, _) => None,
        _ => Some(Label::Debt),
    };
    require(
        kept == expected,
        "rule 3 (REV4 R1): Transferred removes, or keeps failed(Expired) under Expired; any other outcome keeps debt",
    )?;
    if let Some(label) = kept {
        row.label = label;
        if label == Label::Failed(Expired) {
            row.disposition = Some(Expired);
        }
    }
    Ok(())
}

/// The disposal table's release half for a Held entry; applies `expired`.
fn release_allowed(row: &mut Row, facts: Dispose) -> Check {
    require(
        !facts.fail_closed,
        "rule 10: the fail-closed (Held, Expired) release is declared by no control",
    )?;
    let live = matches!(facts.candidate, Assembling | Pending | Submitted);
    if facts.expired {
        require(
            row.verdict == Verdict::Open && live && facts.past,
            "rule 3: Expired is set only for a live Open candidate past its deadline",
        )?;
        row.verdict = Verdict::Expired;
        return Ok(());
    }
    match row.verdict {
        Verdict::Consumed => Ok(()),
        Verdict::Open => require(
            facts.candidate == Ended || (facts.candidate == Unknown && facts.past),
            "rule 3: a Held Open entry releases only Ended, or Unknown past its deadline",
        ),
        Verdict::Expired => {
            Err("rule 10: a Held entry never carries Expired into a disposal".to_string())
        }
    }
}

/// The disposal table's retire half for a Failed entry.
fn retire_allowed(row: &Row, facts: Dispose) -> Check {
    require(
        !facts.expired && !facts.fail_closed,
        "rule 3: a retirement sets nothing",
    )?;
    match row.verdict {
        Verdict::Consumed => Ok(()),
        Verdict::Open | Verdict::Expired => require(
            facts.candidate == Ended || (facts.candidate == Unknown && facts.past),
            "rule 3: a failed entry retires only Ended, or Unknown past its deadline",
        ),
    }
}

/// A retention: a Release attempt kept the entry (the one rule), or a
/// disposal kept it unchanged (the table's keep half).
fn retained(
    row: &mut Row,
    cause: Cause,
    outcome: Option<CombinedRelease>,
    state: Label,
    dispose: Option<Dispose>,
) -> Check {
    let Some(outcome) = outcome else {
        let Cause::Dispose { current } = cause else {
            return Err("rule 3: only a disposal keeps without a Release".to_string());
        };
        current_grant(row, current)?;
        let facts = dispose.ok_or("rule 3: a disposal keep carries its facts")?;
        require(
            !facts.released && !facts.expired && !facts.fail_closed && state == row.label,
            "rule 3: a disposal keep changes nothing",
        )?;
        let live = matches!(facts.candidate, Assembling | Pending | Submitted);
        return match (row.label, row.verdict) {
            (Label::Held, Verdict::Open) => require(
                !facts.past && facts.candidate != Ended,
                "rule 3: a Held Open entry is kept only before its deadline and not Ended",
            ),
            (Label::Held, _) => Err(
                "rule 3: a Held Consumed or Expired entry is never kept by a disposal".to_string(),
            ),
            (Label::Failed(_), Verdict::Consumed) => {
                Err("rule 3: a Consumed failed entry retires".to_string())
            }
            (Label::Failed(_), _) => require(
                live || (facts.candidate == Unknown && !facts.past),
                "rule 3: a failed entry is kept only while live, or Unknown before its deadline",
            ),
            _ => Err("rule 3: the walk disposes only held and failed entries".to_string()),
        };
    };
    released(row, cause, outcome, Some(state), dispose)
}

/// The checkpoint after a control's own final row assertion: the states as
/// today, the owed-cancellation count, the replay, then the replayed live
/// set and owed identities against the row's own snapshot.
pub(super) fn trace_end(
    row: &CombinedRow,
    expected: &[(u64, &'static str)],
    unconfirmed: &[(ContentGrant, HeadIdentity)],
) {
    assert_eq!(row.states(), expected, "trace end: the row's states");
    assert_eq!(
        row.unconfirmed_cancels(),
        unconfirmed.len(),
        "trace end: unconfirmed cancellations"
    );
    let replay = replay(row.trace()).unwrap_or_else(|why| panic!("trace end: {why}"));
    let mut actual = row.live_keys();
    actual.sort_by_key(|(key, _)| *key);
    assert_eq!(
        replay.live_set(),
        actual,
        "trace end: the replayed live set is not the row's"
    );
    let mut owed = row.cancel_identities();
    owed.sort();
    assert_eq!(
        replay.pending(),
        owed,
        "trace end: the replayed owed cancellations are not the row's"
    );
    let mut expected = unconfirmed.to_vec();
    expected.sort();
    assert_eq!(owed, expected, "trace end: the owed cancellations");
}

/// The checkpoint of a control that admits nothing: every event an
/// ordinary join over an empty row; their number.
pub(super) fn trace_no_admission(row: &CombinedRow) -> usize {
    assert!(row.states().is_empty(), "no admission: the row is empty");
    assert_eq!(row.unconfirmed_cancels(), 0, "no admission: nothing owed");
    ordinary_only(row.trace()).unwrap_or_else(|why| panic!("no admission: {why}"))
}

fn ordinary_only(trace: &[Event]) -> Result<usize, String> {
    let mut joins = 0;
    for event in trace {
        match event {
            Event::Joined {
                arm: Arm::Ordinary,
                matched: None,
                verdict_after: None,
                present,
                ..
            } if present.is_empty() => joins += 1,
            other => {
                return Err(format!(
                    "not an ordinary join over an empty row with no verdict write: {other:?}"
                ));
            }
        }
    }
    Ok(joins)
}

#[path = "combined_trace_rows.rs"]
mod rows;
