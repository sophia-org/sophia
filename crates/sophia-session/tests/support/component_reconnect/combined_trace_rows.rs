//! The checker's synthetic rows: its own sensitivity to each rule, positive
//! and negative. Not row evidence: no control, driver or worker runs here.

use super::*;

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

/// The collection of `key`'s result with the facts its Bound recorded.
fn collected(key: Key, grant: ContentGrant, disposition: CombinedDisposition) -> Event {
    Event::Collected {
        key,
        serial: key.serial,
        reserved: key.job,
        grant,
        head: head(key.ticket),
        disposition,
    }
}

fn held(key: Key) -> [Event; 2] {
    [
        collected(key, grant(), Rendered),
        step(key, Label::InFlight, Label::Held),
    ]
}

/// A disposal under the entry's own (current) grant.
fn dispose() -> Cause {
    Cause::Dispose {
        current: Some(grant()),
    }
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
        cause: dispose(),
        outcome: None,
        state,
        dispose: Some(dispose),
    }
}

fn retired(key: Key, dispose: Dispose) -> Event {
    Event::Removed {
        key,
        cause: dispose(),
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
        release(k, dispose(), FULL, Some(Label::Debt), Some(expired)),
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
            dispose(),
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
            release(k, dispose(), TRANSFERRED, None, Some(expired)),
        ],
        "REV4 R1",
    );
    // Expired debt transferred and recorded as removed.
    rejected(
        &[
            bound(k, grant(), 1),
            c,
            s,
            release(k, dispose(), FULL, Some(Label::Debt), Some(expired)),
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
        release(k, dispose(), FULL, Some(Label::Debt), Some(expired)),
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
        dispose(),
        TRANSFERRED,
        None,
        Some(facts(Ended, false, true, false)),
    )));
    accepted(&with(release(
        k,
        dispose(),
        TRANSFERRED,
        None,
        Some(facts(Unknown, true, true, false)),
    )));
    rejected(
        &with(release(
            k,
            dispose(),
            TRANSFERRED,
            None,
            Some(facts(Pending, true, true, false)),
        )),
        "rule 3: a Held Open entry releases",
    );
    rejected(
        &with(release(
            k,
            dispose(),
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
        collected(k, grant(), CombinedDisposition::RenderFailed),
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
        collected(k, grant(), Rendered),
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
            collected(k, grant(), CombinedDisposition::RenderFailed),
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
    rejected(&[collected(k, grant(), Rendered)], "rule 1");
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
    // An ordinary join that wrote a verdict is not a no-admission join.
    let wrote = joined(
        grant(),
        1,
        Arm::Ordinary,
        None,
        Some(Verdict::Consumed),
        &[],
    );
    assert!(
        ordinary_only(&[wrote])
            .unwrap_err()
            .contains("no verdict write")
    );
}

#[test]
fn a_collected_result_names_the_bound_reservation_grant_and_head() {
    let k = key(1);
    let good = collected(k, grant(), Rendered);
    assert_eq!(
        accepted(&[bound(k, grant(), 1), good.clone()]).live_set(),
        [(k, Label::InFlight)]
    );
    let wrong = |event: Event| rejected(&[bound(k, grant(), 1), event], "rule 2: the collected");
    let Event::Collected {
        serial,
        reserved,
        head,
        ..
    } = good.clone()
    else {
        unreachable!()
    };
    wrong(match good.clone() {
        Event::Collected {
            key,
            grant,
            disposition,
            ..
        } => Event::Collected {
            key,
            serial: serial + 1,
            reserved,
            grant,
            head,
            disposition,
        },
        _ => unreachable!(),
    });
    wrong(match good.clone() {
        Event::Collected {
            key,
            grant,
            disposition,
            ..
        } => Event::Collected {
            key,
            serial,
            reserved: reserved + 1,
            grant,
            head,
            disposition,
        },
        _ => unreachable!(),
    });
    wrong(collected(k, other_grant(), Rendered));
    wrong(match good {
        Event::Collected {
            key,
            grant,
            disposition,
            ..
        } => Event::Collected {
            key,
            serial,
            reserved,
            grant,
            head: (head.0, head.1, head.2 + 1),
            disposition,
        },
        _ => unreachable!(),
    });
}

#[test]
fn the_walk_branches_name_the_current_grant_they_read() {
    let k = key(1);
    let [c, s] = held(k);
    let ended = |current| Cause::WalkEndedHeld { current };
    // Held released on grant end: under another grant, or none.
    for current in [Some(other_grant()), None] {
        assert!(
            accepted(&[
                bound(k, grant(), 1),
                c.clone(),
                s.clone(),
                release(k, ended(current), TRANSFERRED, None, None),
            ])
            .live_set()
            .is_empty()
        );
    }
    rejected(
        &[
            bound(k, grant(), 1),
            c.clone(),
            s.clone(),
            release(k, ended(Some(grant())), TRANSFERRED, None, None),
        ],
        "rule 3: an ended-grant branch",
    );
    // A failed entry dropped on grant end, and not on the current grant.
    let failed = [
        bound(k, grant(), 1),
        collected(k, grant(), CombinedDisposition::RenderFailed),
        step(
            k,
            Label::InFlight,
            Label::Failed(CombinedDisposition::RenderFailed),
        ),
    ];
    let dropped = |current| removed(k, Cause::WalkEndedFailed { current });
    assert!(
        accepted(&[failed.as_slice(), &[dropped(None)]].concat())
            .live_set()
            .is_empty()
    );
    rejected(
        &[failed.as_slice(), &[dropped(Some(grant()))]].concat(),
        "rule 3: an ended-grant branch",
    );
    // A disposal keep, release or retirement of an entry whose grant is
    // not the current one.
    let unknown = facts(Unknown, false, false, false);
    for current in [Some(other_grant()), None] {
        let cause = Cause::Dispose { current };
        rejected(
            &[
                bound(k, grant(), 1),
                c.clone(),
                s.clone(),
                Event::Retained {
                    key: k,
                    cause,
                    outcome: None,
                    state: Label::Held,
                    dispose: Some(unknown),
                },
            ],
            "rule 3: a disposal of an entry whose grant is not current",
        );
        rejected(
            &[
                bound(k, grant(), 1),
                c.clone(),
                s.clone(),
                release(
                    k,
                    cause,
                    TRANSFERRED,
                    None,
                    Some(facts(Ended, false, true, false)),
                ),
            ],
            "rule 3: a disposal of an entry whose grant is not current",
        );
        rejected(
            &[
                failed.as_slice(),
                &[Event::Removed {
                    key: k,
                    cause,
                    outcome: None,
                    dispose: Some(facts(Ended, false, false, false)),
                }],
            ]
            .concat(),
            "rule 3: a disposal of an entry whose grant is not current",
        );
    }
}
