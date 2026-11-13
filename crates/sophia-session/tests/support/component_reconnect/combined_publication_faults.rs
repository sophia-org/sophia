//! G3-5d d-2b3d-1 fault rows (F1-F3 and F4-P) of the publication
//! controls, over the parent's lane, schedule and observers; split from
//! `combined_publication.rs` for the file-size limit only.
use super::*;

/// F1: the labelled draw failure (declared before submission). T1 commits
/// the candidate's one RendererFailed terminal under its exact key; the
/// resource's statuses, if any, are printed beside it. Nothing is deferred
/// at the tail.
#[test]
fn f1_the_render_failure_terminal_is_journaled_once_in_t1() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    lane.native.declare_render_failure();
    let job = faulted(&mut lane, &wake, &mut a);
    let (key, n) = lane.admitted_key(job);
    assert_eq!(lane.leased(), 0, "the failed draw freed its slot");
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F1 O0");
    eprintln!("F1 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let (steps, observed) = lane.observed_turn(Instant::now(), None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, NOTHING, "content neither accepted nor released");
    let s1 = lane.snapshot(&a, "F1 O1");
    let w1 = window(&s0, &s1, "F1 T1");
    assert_eq!(
        peer_terminals(&frames_in(&w1), key, n),
        [FAILED],
        "one RendererFailed terminal"
    );
    let statuses: Vec<(u16, u16)> = w1
        .iter()
        .filter_map(status_of)
        .filter(|s| s.grant == a.key.grant && s.resource == RESOURCE)
        .map(|s| (s.status, s.reason))
        .collect();
    eprintln!("F1: statuses in T1 {statuses:?}");
    assert_all_visible(s1.journal(), "F1 O1");
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    assert!(s1.facts.outbox.is_empty(), "{:?}", s1.facts.outbox);
    s1.assert_stores_empty("F1 O1");
    let (verdict, _, _, _) = lane.schedule(&mut a, &s0, &s1, "F1");
    verdict.assert_established("F1");
    assert_eq!(
        peer_terminals(&a.frames, key, n),
        [FAILED],
        "the terminal once"
    );
    assert_eq!(
        peer_statuses(&a.contents, a.key.grant),
        statuses,
        "the peer read exactly T1's statuses"
    );
    assert_eq!(lane.releases(), (0, 0), "nothing was leased to release");
}

/// F2: the labelled plan mismatch, as separately witnessed facts: (1) the
/// fixture applied its declared widening (`plan_mismatches`, a fault
/// counter, not a validator error); (2) the row collected the worker's
/// Mismatch disposition (the existing collection record); (3) the owner's
/// conversion of Mismatch to `Err(Malformed)` at delivery (`combined_row.rs`
/// `delivered`) is source-only evidence with no record at that boundary,
/// not claimed as executed; (4) the refusal status, predicted exactly from
/// `complete_isb` -> `abort_finished(Malformed, None)`: status 3, reason
/// Malformed, under the attempt's grant and the lane's resource; (5) the
/// candidate's one Malformed terminal under its exact key; (6) the peer's
/// consumption of (4) and (5).
#[test]
fn f2_the_mismatch_journals_the_refusal_status_and_the_malformed_terminal() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    lane.native.declare_plan_mismatch();
    let job = faulted(&mut lane, &wake, &mut a);
    assert_eq!(
        lane.native.plan_mismatches(),
        (1, 0),
        "(1) the declared plan widening was applied"
    );
    let binding = lane.h.owner.processes.combined_row(0).binding(job).unwrap();
    let (key, n) = lane.admitted_key(job);
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F2 O0");
    eprintln!("F2 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let (steps, observed) = lane.observed_turn(Instant::now(), None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, NOTHING, "no bundle met the failed entry");
    assert_eq!(
        lane.row().collected(),
        [(job, CombinedDisposition::Mismatch)],
        "(2) the collected disposition"
    );
    let s1 = lane.snapshot(&a, "F2 O1");
    let w1 = window(&s0, &s1, "F2 T1");
    let status = one_status(&w1, a.key.grant, "F2 T1");
    let refusal = status_of(status).unwrap();
    assert_eq!(
        (
            refusal.grant,
            refusal.resource,
            refusal.status,
            refusal.reason
        ),
        (a.key.grant, RESOURCE, 3, ContentReason::Malformed as u16),
        "(4) the copy's Malformed refusal"
    );
    assert_eq!(
        peer_terminals(&frames_in(&w1), key, n),
        [MALFORMED],
        "(5) one Malformed terminal"
    );
    let terminal = w1
        .iter()
        .find(|r| frame_of(r).is_some_and(|f| outcome_for(f, key, n).is_some()))
        .unwrap();
    eprintln!(
        "F2: status {} {} terminal {}",
        status.sequence,
        if status.sequence < terminal.sequence {
            "before"
        } else {
            "after"
        },
        terminal.sequence
    );
    assert_all_visible(s1.journal(), "F2 O1");
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    s1.assert_stores_empty("F2 O1");
    let (verdict, _, _, _) = lane.schedule(&mut a, &s0, &s1, "F2");
    verdict.assert_established("F2");
    assert_eq!(
        peer_terminals(&a.frames, key, n),
        [MALFORMED],
        "(6) the terminal once"
    );
    assert_eq!(
        peer_statuses(&a.contents, a.key.grant),
        [(3, ContentReason::Malformed as u16)],
        "(6) the refusal once"
    );
    assert_eq!((a.accepted, a.refused), (0, 1));
    assert_eq!(lane.candidate_of(&a, binding, job), Some(Unknown));
    assert_eq!(lane.states(), [(job, "failed")]);
    assert_eq!(lane.releases(), (0, 0), "no lease, no Release");
}

/// F3: C3, the arm's own admitted-deadline decline. T1 commits the
/// acceptance (deferred: the frame is in flight) and the candidate's
/// terminal (RendererFailed, the arm's decline reason, by an ordinary
/// append that leaves the marker stale); then, after slot freedom, T2's
/// images step commits SlotReleased after the status, as R4, and the
/// store's ResourceReleased behind it (the pixels became exclusive with
/// slot freedom), both visible at once since the frame's flight ended.
#[test]
fn f3_the_c3_decline_journals_the_status_and_terminal_then_slot_released() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let candidate = lane.candidate_timeout(&a);
    let before = content_now_msec();
    let job = rendered(&mut lane, &wake, &mut a);
    let (key, n) = lane.admitted_key(job);
    let late = lane.deadline_of(job) + MARGIN;
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F3 O0");
    eprintln!("F3 PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let credit = resource_credit(&s0, "F3 O0");
    let (steps, observed) = lane.observed_turn(late, None);
    assert!(
        content_now_msec() < before + candidate,
        "precondition: before the candidate's own deadline"
    );
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "C3 declined at the content call");
    assert_eq!(lane.native.classifies(), 0, "C3 precedes classification");
    let s1 = lane.snapshot(&a, "F3 O1");
    let w1 = window(&s0, &s1, "F3 T1");
    assert_eq!(
        kinds(&w1),
        ["ResourceStatus(2)", "FrameOutcome(3,9)"],
        "the acceptance, then the decline's terminal"
    );
    let status = one_status(&w1, a.key.grant, "F3 T1");
    assert_eq!(peer_terminals(&frames_in(&w1), key, n), [FAILED]);
    let j1 = s1.journal();
    assert_all_visible(j1, "F3 O1");
    assert_eq!(
        j1.deferred,
        Some((status.start, status.end)),
        "the deferred acceptance's marker is stored but stale behind the ordinary terminal"
    );
    assert_eq!(s1.facts.lean_frames_in_flight, Some(0));
    assert_eq!(s1.releasing, [credit.offer.identity], "release owed");
    s1.assert_stores_empty("F3 O1");
    let (verdict, _, _, s3) = lane.schedule(&mut a, &s0, &s1, "F3");
    verdict.assert_established("F3");
    assert_eq!(
        peer_terminals(&a.frames, key, n),
        [FAILED],
        "the terminal once"
    );
    assert_eq!(
        peer_statuses(&a.contents, a.key.grant),
        [(2, status_of(status).unwrap().reason)]
    );
    assert_eq!((a.accepted, a.refused), (1, 0));
    assert_eq!(lane.releases(), (1, 1));
    lane.slot_freed("F3");
    let (steps, observed) = lane.observed_turn(late, None);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "nothing more");
    let s2 = lane.snapshot(&a, "F3 O2");
    let w2 = window(&s3, &s2, "F3 T2");
    // Slot freedom first makes the pixels exclusive: T2's images step
    // releases the slot, then its collect queues the store's release and
    // the same pass drains it. Two release domains, one window.
    assert_eq!(
        kinds(&w2),
        ["SlotReleased(0)", "ResourceReleased"],
        "the slot's release, then the backing's retirement"
    );
    let (transaction, record) = released_record(&credit, a.key.grant);
    assert_eq!(
        w2[1].identity,
        RecordIdentity::Content {
            transaction,
            record
        },
        "the release joined to the resource's admission"
    );
    let released = &w2[0];
    let event = slot_released_of(released).unwrap();
    assert_eq!(
        (event.identity, event.resource, event.slot, event.reason),
        (
            credit.offer.identity,
            RESOURCE,
            credit.offer.request.slot,
            0
        ),
        "the original source's release"
    );
    assert!(
        status.sequence < released.sequence,
        "status {} before SlotReleased {}",
        status.sequence,
        released.sequence
    );
    assert_all_visible(s2.journal(), "F3 O2");
    assert!(s2.releasing.is_empty());
    assert_eq!(lane.native.installs(), 0);
    assert_eq!(lane.acquisitions(), 1, "no later render");
}

/// F4-P: scene churn, the result declined into one ordinary entry. The
/// candidate is prepared internally (`prepared_without_notice`: the lane
/// is terminal-only) and no Prepared record (kind 1) for the exact
/// candidate is committed, while the acceptance commits deferred,
/// invisible until a terminal. The ordinary entry's presentation outcome
/// needs the queued fallback's completion: b3d-2 F4-O, not asserted here.
#[test]
fn f4p_scene_churn_prepares_the_candidate_internally_without_a_prepared_record() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let (key, n) = lane.admitted_key(job);
    assert!(lane.native.ordinary_queue_ready(output().id));
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F4-P O0");
    eprintln!("F4-P PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    assert_eq!(
        lane.h.owner.processes.candidate_prepared(0, OUTPUT, n),
        None,
        "assembling: not yet a pending candidate"
    );
    let (steps, observed) = lane.observed_turn(Instant::now(), MOVED);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "declined before the walk");
    assert_eq!(lane.native.classifies(), 1);
    assert_eq!(lane.native.installs(), 1, "one ordinary install entry");
    assert_eq!(lane.native.ordinary_queues(), 1);
    let s1 = lane.snapshot(&a, "F4-P O1");
    let w1 = window(&s0, &s1, "F4-P T1");
    let outcomes = peer_terminals(&frames_in(&w1), key, n);
    assert!(
        outcomes.is_empty(),
        "no Prepared (kind 1) or terminal for the exact candidate: {outcomes:?} in {:?}",
        kinds(&w1)
    );
    assert_eq!(
        lane.h.owner.processes.candidate_prepared(0, OUTPUT, n),
        Some(true),
        "prepared internally, notice suppressed"
    );
    assert_eq!(kinds(&w1), ["ResourceStatus(2)"], "the acceptance alone");
    let status = one_status(&w1, a.key.grant, "F4-P T1");
    let j1 = s1.journal();
    assert_eq!(j1.deferred, Some((status.start, status.end)), "deferred");
    assert_eq!(j1.visible, status.start, "invisible");
    assert_eq!(s1.facts.lean_frames_in_flight, Some(1));
    assert_eq!(lane.pending(&a), [(a.key.grant, OUTPUT, 1)]);
    let (verdict, l1, l2, _) = lane.schedule(&mut a, &s0, &s1, "F4-P");
    assert!(
        l1.is_empty() && l2.is_empty(),
        "nothing visible to read: {l1:?} {l2:?}"
    );
    assert_eq!(verdict, H5::Absent, "no reply in T1");
    assert!(peer_terminals(&a.frames, key, n).is_empty());
    assert!(peer_statuses(&a.contents, a.key.grant).is_empty());
    assert_eq!(a.accepted, 0);
    lane.slot_freed("churn");
}
