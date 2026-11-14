//! G3-5d d-2b3d-2, beside `combined_journal.rs` for the file-size limit:
//! the declared completion of F4's queued fallback (F4-O, a simulated
//! completion) and the nonempty outbox cleared by disconnect (BP). Both
//! run on the parent's lane with its helpers; neither withholds an ACK.
use super::*;

/// F4-O (SIMULATED completion): F4-P's churn leaves candidate 1 prepared
/// internally, installed as one ordinary entry whose retained frame F is
/// queued in the fixture and claimed by the runtime. The declared
/// completion `present_queued_retained` takes exactly F out of the queue
/// and presents it (no exporter render, custody submit, DRM commit, flip
/// event, `custody.present`, cursor or capture: the fixture has no
/// device); `publish_presented_from` settles the claim keyed by (output,
/// F); the owner's `observe_presentation` then finds the presentation
/// epoch and journals FrameOutcome(2, None) for the exact candidate, which
/// ends the deferred run so the acceptance becomes visible, and the peer
/// reads both. Evidence of the publication response to a declared
/// completion of the exact queued fallback, not native completion
/// evidence.
#[test]
fn f4o_a_declared_completion_of_the_queued_fallback_journals_presented_and_settles_the_claim() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let (key, n) = lane.admitted_key(job);
    assert!(lane.native.ordinary_queue_ready(output().id));
    assert_eq!(lane.native.last_retained_admission(), None);
    assert!(lane.runtime.queued_claims().is_empty());
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "F4-O O0");
    eprintln!("F4-O PP0 {pp0:?}; O0 journal {:?}", s0.journal());
    let (steps, observed) = lane.observed_turn(Instant::now(), MOVED);
    assert_eq!(steps, TURN);
    assert_eq!(observed, DECLINED, "declined before the walk");
    assert_eq!(lane.native.installs(), 1, "one ordinary install entry");
    assert_eq!(lane.native.ordinary_queues(), 1);
    let s1 = lane.snapshot(&a, "F4-O O1");
    let w1 = window(&s0, &s1, "F4-O T1");
    assert_eq!(kinds(&w1), ["ResourceStatus(2)"], "the acceptance alone");
    let status = one_status(&w1, a.key.grant, "F4-O T1");
    assert_eq!(
        s1.journal().deferred,
        Some((status.start, status.end)),
        "deferred"
    );
    assert_eq!(s1.facts.lean_frames_in_flight, Some(1));
    assert_eq!(
        lane.h.owner.processes.candidate_prepared(0, OUTPUT, n),
        Some(true)
    );
    assert_eq!(lane.pending(&a), [(a.key.grant, OUTPUT, 1)]);
    assert!(
        !lane.native.ordinary_queue_ready(output().id),
        "the fallback is queued, not ready"
    );
    let frame = *lane
        .native
        .last_retained_admission()
        .expect("one retained admission")
        .get(&output().id)
        .expect("for the lane's output");
    assert_eq!(
        lane.runtime.queued_claims(),
        [(
            output().id,
            frame,
            LiveShellContentLayer::Shell,
            a.key.grant
        )],
        "the runtime's claim on the exact queued frame"
    );
    assert_eq!(lane.native.presented_retained(output().id), None);
    // The declared completion.
    let presented = lane.native.present_queued_retained(output().id).unwrap();
    assert_eq!(presented, frame, "the exact queued fallback");
    assert_eq!(lane.native.presented_retained(output().id), Some(frame));
    assert!(
        lane.native.ordinary_queue_ready(output().id),
        "the queue no longer owns it"
    );
    assert_eq!(lane.native.installs(), 1);
    assert_eq!(lane.native.ordinary_queues(), 1);
    assert_eq!(lane.native.accepts(), 0);
    lane.runtime.publish_presented_from(&lane.native);
    assert!(
        lane.runtime.queued_claims().is_empty(),
        "the claim settled on (output, F)"
    );
    // The owner observes the presentation: one labelled transport pass.
    let runtime = &lane.runtime;
    let found = lane
        .h
        .owner
        .with_service(a.key, |service, t| {
            let ShellComponentService::Bar(bar) = service else {
                panic!("bar required")
            };
            let found = bar.observe_presentation(t, runtime).unwrap();
            t.poll_io().unwrap();
            found
        })
        .unwrap();
    assert!(
        found,
        "the presentation epoch was found for the installed frame"
    );
    let s2 = lane.snapshot(&a, "F4-O O2");
    let w2 = window(&s1, &s2, "F4-O OP");
    assert_eq!(kinds(&w2), ["FrameOutcome(2,0)"], "the Presented outcome");
    assert_eq!(peer_terminals(&frames_in(&w2), key, n), [PRESENTED]);
    assert_all_visible(s2.journal(), "F4-O O2");
    assert_eq!(s2.facts.lean_frames_in_flight, Some(0));
    assert!(lane.pending(&a).is_empty(), "the pending entry removed");
    s2.assert_stores_empty("F4-O O2");
    assert!(s2.facts.outbox.is_empty());
    peer_reads_terminal(&mut a, n, "F4-O");
    assert_eq!(a.terminals(n), [PRESENTED]);
    assert_eq!(
        peer_statuses(&a.contents, a.key.grant),
        [(2, status_of(status).unwrap().reason)],
        "the acceptance became visible with the terminal"
    );
    assert_eq!(a.accepted, 1);
    assert_eq!(lane.releases(), (1, 1));
    assert!(lane.states().is_empty());
    lane.trace_end(&[], &[]);
    lane.slot_freed("F4-O");
}

/// BP: the real invalidation of the frame's own allocation (L, P1's
/// producer with reason Revoked), an owner call between turns, transfers
/// its AllocationResult to the outbox without a drain: the identified
/// nonempty pre-state, not blocked, the journal unchanged, nothing else
/// queued. P1's abort effects on the rendered frame (the transfer stopped
/// AllocationLost: staging, reserved-resident and backing decharged, a
/// refusal status queued, no ResourceReleased) follow only on the next
/// images step, which `stop` prevents: named, not observed here. `stop`
/// revokes the attempt and the persistent transport's outbox reads empty
/// afterwards through the slot-indexed observer (the wire is gone; the
/// outbox is the transport's own field, so the empty read is a read, not
/// a missing wire). The renderer lease is released by the next turn's
/// walk; nothing about source release or native retirement follows from
/// the clear. No ACK is withheld here.
#[test]
fn bp_the_frames_invalidation_leaves_the_outbox_nonempty_and_stop_clears_it() {
    let (mut lane, wake) = Lane::running();
    let mut a = lane.attempt();
    let job = rendered(&mut lane, &wake, &mut a);
    let (pp0, _) = peer_poll(&mut a);
    let s0 = lane.snapshot(&a, "BP O0");
    eprintln!(
        "BP PP0 {pp0:?}; O0 journal {:?}; usage {:?}",
        s0.journal(),
        usage(&lane)
    );
    assert!(s0.facts.outbox.is_empty());
    lane.h
        .owner
        .with_service(a.key, |_, t| {
            t.invalidate_content_allocation(INVALIDATE, ALLOCATION, ContentReason::Revoked)
        })
        .unwrap()
        .unwrap();
    let granted: Vec<ContentAllocationId> = lane
        .h
        .owner
        .with_service(a.key, |_, t| t.content_allocation_snapshots())
        .unwrap()
        .iter()
        .map(|snapshot| snapshot.allocation)
        .collect();
    let s_pre = lane.snapshot(&a, "BP pre-stop");
    assert_eq!(
        committed(s_pre.journal()),
        committed(s0.journal()),
        "the invalidation journals nothing itself"
    );
    assert_eq!(s_pre.facts.outbox.len(), 1, "{:?}", s_pre.facts.outbox);
    let entry = &s_pre.facts.outbox[0];
    let OutboxRecordFacts::Content(transaction, ShellContentRecord::AllocationResult(result)) =
        &entry.record
    else {
        panic!("BP pre-stop: an AllocationResult: {entry:?}")
    };
    assert_eq!(
        (*transaction, result.allocation, result.reason),
        (INVALIDATE, ALLOCATION, ContentReason::Revoked as u16),
        "the identified nonempty pre-state"
    );
    assert!(!entry.resource_settlement);
    assert!(!s_pre.facts.output_blocked);
    assert!(s_pre.facts.wired);
    s_pre.assert_stores_empty("BP pre-stop: transferred without a drain, nothing else queued");
    assert!(s_pre.releasing.is_empty(), "no SlotReleased was owed");
    assert_eq!(lane.states(), [(job, "in flight")]);
    assert_eq!(lane.leased(), 1);
    assert_eq!(lane.releases(), (0, 0));
    eprintln!(
        "BP pre-stop: allocations now {granted:?}; pending presentations {:?}; usage {:?}",
        lane.pending(&a),
        usage(&lane)
    );
    lane.h.owner.stop(a.key).unwrap();
    assert_eq!(
        lane.h.owner.phase(a.key).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    let (attempt, facts) = lane
        .h
        .owner
        .processes
        .transport_facts(0)
        .expect("slot 0 persists after stop");
    assert_eq!(
        attempt,
        Some((a.key.grant, ComponentConnectionPhase::Revoked)),
        "the slot still names the revoked attempt"
    );
    assert!(!facts.wired, "the wire is gone");
    assert!(
        facts.outbox.is_empty(),
        "stop cleared the outbox: {:?}",
        facts.outbox
    );
    assert!(!facts.output_blocked);
    assert!(facts.journal.is_none(), "no wire, no journal facts");
    eprintln!(
        "BP post-stop: releasing {:?}, image pending {}",
        lane.h.owner.processes.owned_releasing(0),
        lane.h.owner.processes.image_pending(0)
    );
    assert_eq!(
        lane.states(),
        [(job, "in flight")],
        "stop collected nothing"
    );
    let (steps, accepted) = lane.quiet_driver_turn_at(Instant::now());
    assert_eq!(
        steps,
        [Images, Walk, Native],
        "the revoked slot skips content"
    );
    assert!(accepted.is_empty());
    assert_eq!(
        lane.releases(),
        (1, 1),
        "the walk released the ended grant's Held lease"
    );
    assert!(lane.states().is_empty());
    lane.trace_end(&[], &[]);
    assert_eq!(lane.native.installs(), 0);
    lane.slot_freed("BP");
}
