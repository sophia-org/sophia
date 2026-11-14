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
            Event::Collected { key, disposition } => {
                let row = self.row(*key)?;
                row.just_joined_held = false;
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
            (Cause::WalkEndedFailed, None) => require(
                matches!(row.label, Label::Failed(_)),
                "rule 3: the walk drops only a failed entry of an ended grant",
            )?,
            (Cause::ReturnedPlan { confirmed }, None) => {
                require(
                    row.label == Label::InFlight && !row.collected,
                    "rule 3: a returned plan ends only an uncollected in-flight entry",
                )?;
                if !confirmed {
                    owed = Some((row.grant, row.head));
                }
            }
            (Cause::Dispose, None) => {
                let facts = dispose.ok_or("rule 3: a disposal removal carries its facts")?;
                require(
                    matches!(row.label, Label::Failed(_)) && !facts.released,
                    "rule 3: a disposal retires only a failed entry, without a release",
                )?;
                retire_allowed(row, facts)?;
            }
            (
                Cause::Decline | Cause::WalkEndedHeld | Cause::Dispose | Cause::DebtFlush,
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
fn released(
    row: &mut Row,
    cause: Cause,
    outcome: CombinedRelease,
    kept: Option<Label>,
    dispose: Option<Dispose>,
) -> Check {
    match cause {
        Cause::Decline | Cause::WalkEndedHeld => require(
            row.label == Label::Held,
            "rule 6: a decline or ended-grant release needs a held entry",
        )?,
        Cause::DebtFlush => require(
            row.label == Label::Debt,
            "rule 3: a debt flush needs a debt entry",
        )?,
        Cause::Dispose => {
            let facts = dispose.ok_or("rule 3: a disposal release carries its facts")?;
            require(
                row.label == Label::Held && facts.released,
                "rule 3: a disposal releases only a held entry",
            )?;
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
        require(
            cause == Cause::Dispose,
            "rule 3: only a disposal keeps without a Release",
        )?;
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
                present,
                ..
            } if present.is_empty() => joins += 1,
            other => return Err(format!("not an ordinary join over an empty row: {other:?}")),
        }
    }
    Ok(joins)
}

// Synthetic traces: the checker's own sensitivity, not row evidence.

fn grant() -> ContentGrant {
    ContentGrant {
        content_grant_epoch: 2,
        ..ContentGrant::default()
    }
}

fn other_grant() -> ContentGrant {
    ContentGrant {
        content_grant_epoch: 3,
        ..ContentGrant::default()
    }
}

fn key(ticket: u64) -> Key {
    Key {
        ticket,
        serial: 1,
        job: ticket,
    }
}

fn head(ticket: u64) -> HeadIdentity {
    (1, 1, ticket)
}

fn bound(key: Key, grant: ContentGrant, generation: u64) -> Event {
    Event::Bound {
        key,
        grant,
        output: OUTPUT,
        generation,
        head: head(key.ticket),
    }
}

fn step(key: Key, from: Label, to: Label) -> Event {
    Event::Stepped { key, from, to }
}

fn held(key: Key) -> [Event; 2] {
    [
        Event::Collected {
            key,
            disposition: Rendered,
        },
        step(key, Label::InFlight, Label::Held),
    ]
}

fn joined(
    grant: ContentGrant,
    generation: u64,
    arm: Arm,
    matched: Option<Key>,
    verdict_after: Option<Verdict>,
    present: &[(Key, ContentGrant, Label, Verdict)],
) -> Event {
    Event::Joined {
        grant,
        output: OUTPUT,
        generation,
        arm,
        matched,
        admitted: true,
        verdict_after,
        present: present.to_vec(),
    }
}

fn release(
    key: Key,
    cause: Cause,
    outcome: CombinedRelease,
    kept: Option<Label>,
    dispose: Option<Dispose>,
) -> Event {
    match kept {
        Some(state) => Event::Retained {
            key,
            cause,
            outcome: Some(outcome),
            state,
            dispose,
        },
        None => Event::Removed {
            key,
            cause,
            outcome: Some(outcome),
            dispose,
        },
    }
}

fn kept(key: Key, state: Label, dispose: Dispose) -> Event {
    Event::Retained {
        key,
        cause: Cause::Dispose,
        outcome: None,
        state,
        dispose: Some(dispose),
    }
}

fn retired(key: Key, dispose: Dispose) -> Event {
    Event::Removed {
        key,
        cause: Cause::Dispose,
        outcome: None,
        dispose: Some(dispose),
    }
}

fn facts(candidate: FrameCandidateState, past: bool, released: bool, expired: bool) -> Dispose {
    Dispose {
        candidate,
        past,
        released,
        expired,
        fail_closed: false,
    }
}

fn removed(key: Key, cause: Cause) -> Event {
    Event::Removed {
        key,
        cause,
        outcome: None,
        dispose: None,
    }
}

fn accepted(trace: &[Event]) -> Replay {
    replay(trace).unwrap_or_else(|why| panic!("{why}"))
}

fn rejected(trace: &[Event], rule: &str) {
    let why = replay(trace).err().expect("the checker rejects the trace");
    assert!(why.contains(rule), "{why}");
}

const TRANSFERRED: CombinedRelease = CombinedRelease::Transferred;
const FULL: CombinedRelease = CombinedRelease::Full;

#[test]
fn the_accepted_path_and_the_direct_decline_replay_to_an_empty_row() {
    let k = key(1);
    let present = [(k, grant(), Label::Held, Verdict::Open)];
    let [c, s] = held(k);
    let accept = [
        bound(k, grant(), 1),
        c.clone(),
        s.clone(),
        joined(
            grant(),
            1,
            Arm::Held,
            Some(k),
            Some(Verdict::Consumed),
            &present,
        ),
        removed(k, Cause::Accepted),
    ];
    assert!(accepted(&accept).live_set().is_empty());
    let decline = [
        bound(k, grant(), 1),
        c,
        s,
        release(k, Cause::Decline, TRANSFERRED, None, None),
    ];
    assert!(accepted(&decline).live_set().is_empty());
}

#[test]
fn a_refused_release_keeps_debt_until_a_flush_transfers() {
    let k = key(1);
    let [c, s] = held(k);
    let mut trace = vec![
        bound(k, grant(), 1),
        c,
        s,
        release(k, Cause::Decline, FULL, Some(Label::Debt), None),
    ];
    assert_eq!(accepted(&trace).live_set(), [(k, Label::Debt)]);
    trace.push(release(k, Cause::DebtFlush, FULL, Some(Label::Debt), None));
    assert_eq!(accepted(&trace).live_set(), [(k, Label::Debt)]);
    trace.push(release(k, Cause::DebtFlush, TRANSFERRED, None, None));
    assert!(accepted(&trace).live_set().is_empty());
    // REV4 R1 negatives: debt kept on Transferred without Expired; held
    // removed on a refusal.
    let mut wrong = trace.clone();
    wrong.pop();
    wrong.push(release(
        k,
        Cause::DebtFlush,
        TRANSFERRED,
        Some(Label::Debt),
        None,
    ));
    rejected(&wrong, "REV4 R1");
    let [c, s] = held(k);
    rejected(
        &[
            bound(k, grant(), 1),
            c,
            s,
            release(k, Cause::Decline, FULL, None, None),
        ],
        "REV4 R1",
    );
}

#[test]
fn an_expiry_survives_a_debt_transfer_as_failed_expired_and_joins_failed() {
    let k = key(1);
    let [c, s] = held(k);
    let expired = facts(Pending, true, true, true);
    let mut trace = vec![
        bound(k, grant(), 1),
        c,
        s,
        release(k, Cause::Dispose, FULL, Some(Label::Debt), Some(expired)),
        release(
            k,
            Cause::DebtFlush,
            TRANSFERRED,
            Some(Label::Failed(Expired)),
            None,
        ),
    ];
    assert_eq!(accepted(&trace).live_set(), [(k, Label::Failed(Expired))]);
    let present = [(k, grant(), Label::Failed(Expired), Verdict::Expired)];
    trace.push(joined(
        grant(),
        1,
        Arm::Failed,
        Some(k),
        Some(Verdict::Consumed),
        &present,
    ));
    trace.push(removed(k, Cause::ConsumedFailed));
    assert!(accepted(&trace).live_set().is_empty());
    // The immediate transfer keeps failed(Expired) too; a removal is wrong.
    let [c, s] = held(k);
    let immediate = [
        bound(k, grant(), 1),
        c.clone(),
        s.clone(),
        release(
            k,
            Cause::Dispose,
            TRANSFERRED,
            Some(Label::Failed(Expired)),
            Some(expired),
        ),
    ];
    assert_eq!(
        accepted(&immediate).live_set(),
        [(k, Label::Failed(Expired))]
    );
    rejected(
        &[
            bound(k, grant(), 1),
            c.clone(),
            s.clone(),
            release(k, Cause::Dispose, TRANSFERRED, None, Some(expired)),
        ],
        "REV4 R1",
    );
    // Expired debt transferred and recorded as removed.
    rejected(
        &[
            bound(k, grant(), 1),
            c,
            s,
            release(k, Cause::Dispose, FULL, Some(Label::Debt), Some(expired)),
            release(k, Cause::DebtFlush, TRANSFERRED, None, None),
        ],
        "REV4 R1",
    );
}

#[test]
fn the_debt_arm_consumes_and_a_consumed_entry_joins_invariant_after() {
    let k = key(1);
    let [c, s] = held(k);
    let expired = facts(Submitted, true, true, true);
    let mut trace = vec![
        bound(k, grant(), 1),
        c,
        s,
        release(k, Cause::Dispose, FULL, Some(Label::Debt), Some(expired)),
    ];
    let present = [(k, grant(), Label::Debt, Verdict::Expired)];
    trace.push(joined(
        grant(),
        1,
        Arm::Debt,
        Some(k),
        Some(Verdict::Consumed),
        &present,
    ));
    assert_eq!(accepted(&trace).live_set(), [(k, Label::Debt)]);
    let consumed = [(k, grant(), Label::Debt, Verdict::Consumed)];
    trace.push(joined(
        grant(),
        1,
        Arm::Invariant,
        Some(k),
        Some(Verdict::Consumed),
        &consumed,
    ));
    accepted(&trace);
    // A Debt arm claimed again after Consumed, or a surface mismatch
    // claimed as Debt.
    let mut again = trace.clone();
    again.push(joined(
        grant(),
        1,
        Arm::Debt,
        Some(k),
        Some(Verdict::Consumed),
        &consumed,
    ));
    rejected(&again, "rule 4: the arm");
    let mut unadmitted = trace[..4].to_vec();
    unadmitted.push(Event::Joined {
        grant: grant(),
        output: OUTPUT,
        generation: 1,
        arm: Arm::Debt,
        matched: Some(k),
        admitted: false,
        verdict_after: Some(Verdict::Consumed),
        present: present.to_vec(),
    });
    rejected(&unadmitted, "rule 4: the arm");
}

#[test]
fn a_replacement_joins_ordinary_beside_the_old_entry_under_its_own_key() {
    let old = key(1);
    let new = Key {
        ticket: 2,
        serial: 2,
        job: 1,
    };
    let [c, s] = held(old);
    let present = [(old, grant(), Label::Held, Verdict::Open)];
    let trace = [
        bound(old, grant(), 1),
        c,
        s,
        joined(other_grant(), 1, Arm::Ordinary, None, None, &present),
        bound(new, other_grant(), 1),
    ];
    let replay = accepted(&trace);
    assert_eq!(replay.ordinary_joins(), 1);
    assert_eq!(
        replay.live_set(),
        [(old, Label::Held), (new, Label::InFlight)]
    );
    // The same job under the same key twice, or the same triple live twice.
    rejected(&[bound(old, grant(), 1), bound(old, grant(), 2)], "rule 1");
    rejected(
        &[bound(old, grant(), 1), bound(key(2), grant(), 1)],
        "rule 7",
    );
}

#[test]
fn join_completeness_needs_the_exact_live_set_and_the_exact_match() {
    let k = key(1);
    let [c, s] = held(k);
    let base = [bound(k, grant(), 1), c, s];
    let mut missing = base.to_vec();
    missing.push(joined(other_grant(), 1, Arm::Ordinary, None, None, &[]));
    rejected(&missing, "rule 4: present");
    let mut stale = base.to_vec();
    stale.push(joined(
        other_grant(),
        1,
        Arm::Ordinary,
        None,
        None,
        &[(k, grant(), Label::Held, Verdict::Consumed)],
    ));
    rejected(&stale, "rule 4: present");
    let mut unmatched = base.to_vec();
    unmatched.push(joined(
        grant(),
        1,
        Arm::Ordinary,
        None,
        None,
        &[(k, grant(), Label::Held, Verdict::Open)],
    ));
    rejected(&unmatched, "rule 4: matched");
    // MT3's shape: the Held arm taken, the verdict left Open.
    let mut open = base.to_vec();
    open.push(joined(
        grant(),
        1,
        Arm::Held,
        Some(k),
        Some(Verdict::Open),
        &[(k, grant(), Label::Held, Verdict::Open)],
    ));
    rejected(&open, "rule 4: a matched join leaves Consumed");
}

#[test]
fn the_disposal_table_release_keep_and_retire() {
    let k = key(1);
    let [c, s] = held(k);
    let base = [bound(k, grant(), 1), c, s];
    let with = |event: Event| {
        let mut trace = base.to_vec();
        trace.push(event);
        trace
    };
    accepted(&with(kept(
        k,
        Label::Held,
        facts(Unknown, false, false, false),
    )));
    rejected(
        &with(kept(k, Label::Held, facts(Assembling, true, false, false))),
        "rule 3: a Held Open entry is kept",
    );
    rejected(
        &with(kept(k, Label::Held, facts(Unknown, true, false, false))),
        "rule 3: a Held Open entry is kept",
    );
    rejected(
        &with(kept(k, Label::Held, facts(Ended, false, false, false))),
        "rule 3: a Held Open entry is kept",
    );
    accepted(&with(release(
        k,
        Cause::Dispose,
        TRANSFERRED,
        None,
        Some(facts(Ended, false, true, false)),
    )));
    accepted(&with(release(
        k,
        Cause::Dispose,
        TRANSFERRED,
        None,
        Some(facts(Unknown, true, true, false)),
    )));
    rejected(
        &with(release(
            k,
            Cause::Dispose,
            TRANSFERRED,
            None,
            Some(facts(Pending, true, true, false)),
        )),
        "rule 3: a Held Open entry releases",
    );
    rejected(
        &with(release(
            k,
            Cause::Dispose,
            TRANSFERRED,
            None,
            Some(Dispose {
                candidate: Unknown,
                past: true,
                released: true,
                expired: false,
                fail_closed: true,
            }),
        )),
        "rule 10",
    );
    // A failed entry: kept while live past its deadline, or Unknown before
    // it; retired Ended or Unknown past it.
    let failed = [
        bound(k, grant(), 1),
        Event::Collected {
            key: k,
            disposition: CombinedDisposition::RenderFailed,
        },
        step(
            k,
            Label::InFlight,
            Label::Failed(CombinedDisposition::RenderFailed),
        ),
    ];
    let label = Label::Failed(CombinedDisposition::RenderFailed);
    let with_failed = |event: Event| {
        let mut trace = failed.to_vec();
        trace.push(event);
        trace
    };
    accepted(&with_failed(kept(
        k,
        label,
        facts(Submitted, true, false, false),
    )));
    accepted(&with_failed(kept(
        k,
        label,
        facts(Unknown, false, false, false),
    )));
    accepted(&with_failed(retired(k, facts(Ended, false, false, false))));
    accepted(&with_failed(retired(k, facts(Unknown, true, false, false))));
    rejected(
        &with_failed(retired(k, facts(Pending, true, false, false))),
        "rule 3: a failed entry retires",
    );
    rejected(
        &with_failed(kept(k, label, facts(Unknown, true, false, false))),
        "rule 3: a failed entry is kept",
    );
    rejected(
        &with_failed(release(k, Cause::Decline, TRANSFERRED, None, None)),
        "rule 6",
    );
}

#[test]
fn the_retry_edges_go_forward_only_after_the_collected_result() {
    use Label::{Credit, Held, InFlight, Undelivered, Unsettled};
    let k = key(1);
    let trace = [
        bound(k, grant(), 1),
        Event::Collected {
            key: k,
            disposition: Rendered,
        },
        step(k, InFlight, Unsettled),
        step(k, Unsettled, Unsettled),
        step(k, Unsettled, Credit),
        step(k, Credit, Credit),
        step(k, Credit, Undelivered),
        step(k, Undelivered, Undelivered),
        step(k, Undelivered, Held),
    ];
    assert_eq!(accepted(&trace).live_set(), [(k, Held)]);
    // MT1's shape: the step with no Collected; the wrong disposition; a
    // backward retry edge; in flight to in flight.
    rejected(
        &[bound(k, grant(), 1), step(k, InFlight, Held)],
        "rule 3: an edge out of in flight needs",
    );
    rejected(
        &[
            bound(k, grant(), 1),
            Event::Collected {
                key: k,
                disposition: CombinedDisposition::RenderFailed,
            },
            step(k, InFlight, Held),
        ],
        "rule 3: held needs",
    );
    rejected(
        &trace[..7]
            .iter()
            .cloned()
            .chain([step(k, Undelivered, Unsettled)])
            .collect::<Vec<_>>(),
        "rule 3: unsettled follows",
    );
    rejected(
        &[bound(k, grant(), 1), step(k, InFlight, InFlight)],
        "rule 3: in flight to in flight",
    );
}

#[test]
fn cancellations_are_owed_by_identity_as_a_multiset() {
    let k = key(1);
    let owed = (grant(), head(1));
    let mut trace = vec![
        bound(k, grant(), 1),
        removed(k, Cause::ReturnedPlan { confirmed: false }),
        Event::ReserveCancelled {
            ticket: 2,
            grant: grant(),
            head: head(1),
            confirmed: false,
        },
    ];
    assert_eq!(accepted(&trace).pending(), [owed, owed]);
    trace.push(Event::CancelLater {
        grant: grant(),
        head: head(1),
        confirmed: false,
    });
    assert_eq!(accepted(&trace).pending(), [owed, owed]);
    trace.push(Event::CancelLater {
        grant: grant(),
        head: head(1),
        confirmed: true,
    });
    assert_eq!(accepted(&trace).pending(), [owed]);
    trace.push(Event::CancelLater {
        grant: grant(),
        head: head(1),
        confirmed: true,
    });
    assert!(accepted(&trace).pending().is_empty());
    trace.push(Event::CancelLater {
        grant: grant(),
        head: head(1),
        confirmed: true,
    });
    rejected(&trace, "rule 8");
    rejected(
        &[
            bound(k, grant(), 1),
            Event::ReserveCancelled {
                ticket: 1,
                grant: grant(),
                head: head(1),
                confirmed: true,
            },
        ],
        "rule 8: a bound ticket",
    );
    let [c, s] = held(k);
    rejected(
        &[
            bound(k, grant(), 1),
            c,
            s,
            removed(k, Cause::ReturnedPlan { confirmed: true }),
        ],
        "rule 3: a returned plan",
    );
}

#[test]
fn the_remaining_rules_each_reject_their_trace() {
    let k = key(1);
    let [c, s] = held(k);
    rejected(
        &[Event::Collected {
            key: k,
            disposition: Rendered,
        }],
        "rule 1",
    );
    rejected(&[bound(k, grant(), 1), c.clone(), c.clone()], "rule 2");
    rejected(
        &[
            bound(k, grant(), 1),
            c.clone(),
            s.clone(),
            removed(k, Cause::Accepted),
        ],
        "rule 5",
    );
    let present = [(k, grant(), Label::Held, Verdict::Open)];
    rejected(
        &[
            bound(k, grant(), 1),
            c.clone(),
            s.clone(),
            joined(
                grant(),
                1,
                Arm::Held,
                Some(k),
                Some(Verdict::Consumed),
                &present,
            ),
            release(k, Cause::DebtFlush, TRANSFERRED, None, None),
            removed(k, Cause::Accepted),
        ],
        "rule 3: a debt flush",
    );
    rejected(
        &[
            bound(k, grant(), 1),
            c,
            s,
            release(k, Cause::Decline, TRANSFERRED, None, None),
            removed(k, Cause::Accepted),
        ],
        "rule 9",
    );
}

#[test]
fn a_no_admission_trace_counts_its_ordinary_joins() {
    let ordinary = joined(grant(), 1, Arm::Ordinary, None, None, &[]);
    assert_eq!(ordinary_only(&[ordinary.clone(), ordinary]).unwrap(), 2);
    assert_eq!(ordinary_only(&[]).unwrap(), 0);
    assert!(ordinary_only(&[bound(key(1), grant(), 1)]).is_err());
}
