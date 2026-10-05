//! t294: Session's side of the provider's frames. A permit is granted only
//! while the allocation has nothing unretired, a candidate replaces what its
//! output shows, and an outcome is Presented only once every head retired
//! that exact candidate.
use std::sync::Arc;

use sophia_engine::{SessionLockEpoch, SessionLockImageIdentity};
use sophia_protocol::OutputId;
use sophia_protocol::lock_files::*;
use sophia_session::session_lock_frames::SessionLockFrames;

const CONNECTION: u64 = 7;

fn resource(id: u64) -> LockResourceId {
    LockResourceId { id, generation: 1 }
}

fn candidate(generation: u64, id: u64) -> LockCandidate {
    LockCandidate {
        transaction: 100 + generation,
        lock_epoch: 3,
        output_id: 1,
        output_generation: 1,
        allocation_id: 1,
        allocation_generation: 1,
        candidate_generation: generation,
        pacing_permit: generation,
        resource: resource(id),
    }
}

fn demand(id: u64) -> LockFrameDemand {
    LockFrameDemand {
        transaction: id,
        lock_epoch: 3,
        allocation_id: 1,
        allocation_generation: 1,
        demand_id: id,
    }
}

fn frames() -> SessionLockFrames {
    let mut frames = SessionLockFrames::default();
    frames.lock(SessionLockEpoch::from_raw(3));
    frames.connected(CONNECTION);
    for id in [1, 2] {
        frames.resource_ready(CONNECTION, resource(id), 2, 1, Arc::from(vec![id as u8; 8]));
    }
    frames
}

fn shown(id: u64, generation: u64) -> Option<(SessionLockImageIdentity, u64)> {
    Some((
        SessionLockImageIdentity {
            output: OutputId::from_raw(1),
            connection_epoch: CONNECTION,
            resource_id: id,
            resource_generation: 1,
        },
        generation,
    ))
}

#[test]
fn presentation_paces_the_provider() {
    let mut frames = frames();
    frames.demand(CONNECTION, demand(1));
    assert_eq!(frames.permits(), [demand(1)], "nothing in flight yet");
    assert!(frames.permits().is_empty(), "one permit per demand");
    let (changed, owed) = frames.candidate(CONNECTION, candidate(1, 1));
    assert!(changed && owed.is_empty());
    assert_eq!(frames.images()[&OutputId::from_raw(1)].generation, 1);
    // A new demand waits while the candidate is unretired.
    frames.demand(CONNECTION, demand(2));
    assert!(frames.permits().is_empty());
    // A head still showing something else is not the candidate.
    assert_eq!(frames.presented(OutputId::from_raw(1), shown(2, 1)), None);
    assert_eq!(frames.presented(OutputId::from_raw(1), shown(1, 9)), None);
    let presented = frames
        .presented(OutputId::from_raw(1), shown(1, 1))
        .unwrap();
    assert_eq!(presented.status, LockCandidateStatus::Presented);
    assert_eq!(frames.permits(), [demand(2)], "retired, so the next permit");
}

#[test]
fn a_newer_candidate_supersedes_one_no_head_showed() {
    let mut frames = frames();
    frames.candidate(CONNECTION, candidate(1, 1));
    let (_, owed) = frames.candidate(CONNECTION, candidate(2, 2));
    assert_eq!(owed.len(), 1);
    assert_eq!(
        (owed[0].status, owed[0].candidate_generation),
        (LockCandidateStatus::Superseded, 1)
    );
    assert_eq!(
        frames.images()[&OutputId::from_raw(1)]
            .image
            .identity
            .resource_id,
        2
    );
}

#[test]
fn a_candidate_for_another_lock_or_connection_or_resource_is_rejected() {
    let mut frames = frames();
    let stale_lock = LockCandidate {
        lock_epoch: 2,
        ..candidate(1, 1)
    };
    for (connection, sent) in [
        (CONNECTION, stale_lock),
        (CONNECTION + 1, candidate(1, 1)),
        (CONNECTION, candidate(1, 9)),
    ] {
        let (changed, owed) = frames.candidate(connection, sent);
        assert!(!changed);
        assert_eq!(owed[0].status, LockCandidateStatus::Rejected);
    }
    assert!(frames.images().is_empty());
}

#[test]
fn a_departed_provider_or_a_new_lock_leaves_the_fill() {
    let mut frames = frames();
    frames.candidate(CONNECTION, candidate(1, 1));
    assert!(!frames.disconnected(CONNECTION + 1), "not this provider");
    assert!(frames.disconnected(CONNECTION));
    assert!(frames.images().is_empty());
    assert_eq!(frames.waiting().count(), 0);

    let mut frames = self::frames();
    frames.candidate(CONNECTION, candidate(1, 1));
    frames.demand(CONNECTION, demand(2));
    assert!(frames.lock(SessionLockEpoch::from_raw(4)));
    assert!(frames.images().is_empty());
    assert!(frames.permits().is_empty(), "old demands lapse");
    // Resources survive a new lock: they belong to the connection.
    let next = LockCandidate {
        lock_epoch: 4,
        ..candidate(2, 1)
    };
    assert!(frames.candidate(CONNECTION, next).0);
    // A retired resource cannot be placed again.
    frames.resource_retired(CONNECTION, resource(2));
    let retired = LockCandidate {
        lock_epoch: 4,
        ..candidate(3, 2)
    };
    assert_eq!(
        frames.candidate(CONNECTION, retired).1[0].status,
        LockCandidateStatus::Rejected
    );
}

/// t308: with the diagnostic enabled, the pacing sample follows one
/// allocation through demand, permit, candidate and outcome, and names its
/// lock, connection and allocation generation.
#[test]
fn the_pacing_sample_follows_an_allocation_through_its_handshake() {
    use sophia_session::session_lock_frames::{
        SessionLockPacing, SessionLockPacingCounts, session_lock_pacing_record,
    };
    let mut frames = frames();
    frames.set_pacing_diagnostics(true);
    assert!(frames.pacing().is_empty(), "nothing seen yet");
    frames.demand(CONNECTION, demand(1));
    let _ = frames.permits();
    frames.candidate(CONNECTION, candidate(1, 1));
    frames.demand(CONNECTION, demand(2));
    let stuck = SessionLockPacing {
        lock_epoch: Some(3),
        connection_epoch: Some(CONNECTION),
        allocation_id: 1,
        allocation_generation: 1,
        output: Some(OutputId::from_raw(1)),
        demand_held: true,
        in_flight_generation: Some(1),
        counts: SessionLockPacingCounts {
            demands: 2,
            permits: 1,
            candidates: 1,
            ..SessionLockPacingCounts::default()
        },
    };
    // Unretired: the demand waits and the same generation stays in flight.
    assert!(frames.permits().is_empty());
    assert_eq!(frames.pacing(), [stuck]);
    assert_eq!(
        session_lock_pacing_record(&stuck),
        "sophia_live_lock_pacing schema=1 lock_epoch=3 connection_epoch=7 allocation=1 allocation_generation=1 output=1 demand=held in_flight_generation=1 demands=2 permits=1 candidates=1 presented=0 superseded=0 rejected=0"
    );
    frames
        .presented(OutputId::from_raw(1), shown(1, 1))
        .unwrap();
    let _ = frames.permits();
    frames.candidate(CONNECTION, candidate(2, 2));
    frames.candidate(CONNECTION, candidate(3, 1));
    // This lock and connection, but a resource never offered: rejected here.
    frames.candidate(CONNECTION, candidate(4, 9));
    assert_eq!(
        frames.pacing()[0].counts,
        SessionLockPacingCounts {
            demands: 2,
            permits: 2,
            candidates: 3,
            presented: 1,
            superseded: 1,
            rejected: 1,
        }
    );
    assert_eq!(frames.pacing()[0].in_flight_generation, Some(3));
    assert!(!frames.pacing()[0].demand_held);
}

/// Off (the default), the handshake works exactly as before and keeps no
/// diagnostic state.
#[test]
fn the_pacing_diagnostic_keeps_nothing_unless_enabled() {
    let mut frames = frames();
    frames.demand(CONNECTION, demand(1));
    assert_eq!(frames.permits(), [demand(1)]);
    frames.candidate(CONNECTION, candidate(1, 1));
    frames
        .presented(OutputId::from_raw(1), shown(1, 1))
        .unwrap();
    assert!(frames.pacing().is_empty());
    frames.set_pacing_diagnostics(true);
    frames.demand(CONNECTION, demand(2));
    assert_eq!(frames.pacing().len(), 1);
    frames.set_pacing_diagnostics(false);
    assert!(
        frames.pacing().is_empty(),
        "turning it off drops what it kept"
    );
}

/// Events from an earlier lock or provider connection are handled as before
/// but never counted against the current one.
#[test]
fn stale_lock_or_connection_events_do_not_reach_the_current_sample() {
    let mut frames = frames();
    frames.set_pacing_diagnostics(true);
    frames.demand(CONNECTION, demand(1));
    frames.lock(SessionLockEpoch::from_raw(4));
    assert!(frames.pacing().is_empty(), "a new lock starts again");
    // A demand and a candidate for lock 3 after lock 4 began.
    frames.demand(CONNECTION, demand(5));
    frames.candidate(CONNECTION, candidate(5, 1));
    // A candidate from the departed connection, for the current lock.
    frames.candidate(
        CONNECTION + 1,
        LockCandidate {
            lock_epoch: 4,
            ..candidate(6, 1)
        },
    );
    assert!(frames.pacing().is_empty(), "{:?}", frames.pacing());
    frames.connected(CONNECTION + 2);
    frames.candidate(CONNECTION, candidate(7, 1));
    assert!(frames.pacing().is_empty());
}

/// A newer allocation generation starts its counts again, and a late event
/// for the generation it replaced does not count against it.
#[test]
fn a_new_allocation_generation_restarts_and_ignores_its_predecessor() {
    use sophia_session::session_lock_frames::SessionLockPacingCounts;
    let mut frames = frames();
    frames.set_pacing_diagnostics(true);
    frames.demand(CONNECTION, demand(1));
    let _ = frames.permits();
    frames.candidate(CONNECTION, candidate(1, 1));
    assert_eq!(frames.pacing()[0].counts.candidates, 1);
    let renewed = LockFrameDemand {
        allocation_generation: 2,
        ..demand(2)
    };
    frames.demand(CONNECTION, renewed);
    // The generation-1 candidate retires late and is superseded late.
    frames
        .presented(OutputId::from_raw(1), shown(1, 1))
        .unwrap();
    frames.candidate(CONNECTION, candidate(3, 2));
    frames.candidate(CONNECTION, candidate(4, 1));
    let sample = frames.pacing();
    assert_eq!(sample.len(), 1);
    assert_eq!(sample[0].allocation_generation, 2);
    assert_eq!(
        sample[0].counts,
        SessionLockPacingCounts {
            demands: 1,
            ..SessionLockPacingCounts::default()
        },
        "only generation 2's own demand"
    );
    assert!(sample[0].demand_held);
    assert_eq!(
        sample[0].in_flight_generation, None,
        "generation 1's candidate"
    );
}

/// Allocation churn is bounded: past the bound new allocations are counted as
/// untracked, a current stuck allocation is kept, and a published lock object
/// keeps only the allocations it names.
#[test]
fn the_pacing_history_is_bounded_and_keeps_current_allocations() {
    use sophia_session::session_lock_frames::{
        SESSION_LOCK_PACING_ALLOCATIONS, session_lock_pacing_untracked_record,
    };
    let mut frames = frames();
    frames.set_pacing_diagnostics(true);
    frames.demand(CONNECTION, demand(1));
    let _ = frames.permits();
    frames.candidate(CONNECTION, candidate(1, 1));
    let churn = SESSION_LOCK_PACING_ALLOCATIONS as u64 + 10;
    for allocation in 100..100 + churn {
        frames.demand(
            CONNECTION,
            LockFrameDemand {
                allocation_id: allocation,
                ..demand(allocation)
            },
        );
    }
    let sample = frames.pacing();
    assert_eq!(sample.len(), SESSION_LOCK_PACING_ALLOCATIONS);
    assert_eq!(
        sample[0].in_flight_generation,
        Some(1),
        "the stuck allocation stays tracked"
    );
    assert_eq!(frames.pacing_untracked_observations(), 11);
    assert_eq!(
        session_lock_pacing_untracked_record(11),
        format!(
            "sophia_live_lock_pacing schema=1 status=untracked observations_over_bound=11 bound={SESSION_LOCK_PACING_ALLOCATIONS}"
        )
    );
    let live = |allocation_id, allocation_generation| LockAllocation {
        output_id: 1,
        output_generation: 1,
        allocation_id,
        allocation_generation,
        pixel_width: 2,
        pixel_height: 1,
        scale_numerator: 1,
        scale_denominator: 1,
    };
    frames.retain_pacing(&[live(1, 1), live(100, 2)]);
    let kept = frames.pacing();
    assert_eq!(kept.len(), 1, "allocation 100 is now another generation");
    assert_eq!(kept[0].allocation_id, 1);
}

fn live_allocation(allocation_id: u64, allocation_generation: u64) -> LockAllocation {
    LockAllocation {
        output_id: 1,
        output_generation: 1,
        allocation_id,
        allocation_generation,
        pixel_width: 2,
        pixel_height: 1,
        scale_numerator: 1,
        scale_denominator: 1,
    }
}

// REVIEW-CODEX-02 controls, verbatim.
#[test]
fn reconciliation_does_not_let_a_late_retirement_recreate_an_old_generation() {
    let mut frames = frames();
    frames.set_pacing_diagnostics(true);
    frames.demand(CONNECTION, demand(1));
    let _ = frames.permits();
    frames.candidate(CONNECTION, candidate(1, 1));
    frames.retain_pacing(&[live_allocation(1, 2)]);
    assert!(frames.pacing().is_empty());
    frames
        .presented(OutputId::from_raw(1), shown(1, 1))
        .unwrap();
    assert!(
        frames.pacing().iter().all(|s| s.allocation_generation == 2),
        "late retirement recreated the withdrawn generation: {:?}",
        frames.pacing()
    );
}

#[test]
fn removed_allocation_stays_removed_after_a_late_retirement() {
    let mut frames = frames();
    frames.set_pacing_diagnostics(true);
    frames.candidate(CONNECTION, candidate(1, 1));
    frames.retain_pacing(&[]);
    frames
        .presented(OutputId::from_raw(1), shown(1, 1))
        .unwrap();
    assert!(
        frames.pacing().is_empty(),
        "removed allocation returned in the sample: {:?}",
        frames.pacing()
    );
}

#[test]
fn an_old_lock_demand_does_not_appear_held_under_current_lock_identity() {
    let mut frames = frames();
    frames.set_pacing_diagnostics(true);
    frames.lock(SessionLockEpoch::from_raw(4));
    frames.candidate(
        CONNECTION,
        LockCandidate {
            lock_epoch: 4,
            ..candidate(1, 1)
        },
    );
    frames.demand(CONNECTION, demand(2)); // Same allocation/generation, old lock 3.
    assert!(frames.permits().is_empty()); // The current candidate is still in flight.
    let sample = frames.pacing();
    assert_eq!(sample[0].lock_epoch, Some(4));
    assert!(
        !sample[0].demand_held,
        "old-lock demand shown as current: {sample:?}"
    );
}

/// After a publication, late demands and candidates for an allocation or
/// generation the object does not name are not observed, and the published
/// set outlives a reconnect, which clears only the counts.
#[test]
fn the_published_allocations_bound_what_is_observed_across_a_reconnect() {
    let mut frames = frames();
    frames.set_pacing_diagnostics(true);
    frames.retain_pacing(&[live_allocation(1, 2)]);
    frames.demand(CONNECTION, demand(1)); // generation 1: withdrawn
    frames.candidate(CONNECTION, candidate(1, 1));
    frames.demand(
        CONNECTION,
        LockFrameDemand {
            allocation_id: 5,
            ..demand(2)
        },
    ); // never published
    assert!(frames.pacing().is_empty(), "{:?}", frames.pacing());
    frames.connected(CONNECTION + 1);
    frames.demand(CONNECTION + 1, demand(3));
    assert!(
        frames.pacing().is_empty(),
        "generation 1 is still withdrawn"
    );
    frames.demand(
        CONNECTION + 1,
        LockFrameDemand {
            allocation_generation: 2,
            ..demand(4)
        },
    );
    let sample = frames.pacing();
    assert_eq!(sample.len(), 1);
    assert_eq!(
        (
            sample[0].connection_epoch,
            sample[0].allocation_generation,
            sample[0].counts.demands
        ),
        (Some(CONNECTION + 1), 2, 1)
    );
}

/// A dropped permit or outcome is counted every time and recorded the first
/// time and at each power of two of its kind.
#[test]
fn dropped_provider_commands_are_counted_and_recorded_sparsely() {
    use sophia_session::session_lock_frames::{
        SessionLockCommandDrops, SessionLockCommandKind as Kind, session_lock_pacing_enabled,
    };
    let mut drops = SessionLockCommandDrops::default();
    let recorded = (1..=9)
        .filter_map(|_| drops.dropped(Kind::Permit))
        .collect::<Vec<_>>();
    assert_eq!(
        recorded,
        [1, 2, 4, 8].map(|count| format!(
            "sophia_live_lock_provider schema=1 status=command_dropped kind=permit dropped={count}"
        ))
    );
    assert_eq!(
        drops.dropped(Kind::Outcome).as_deref(),
        Some("sophia_live_lock_provider schema=1 status=command_dropped kind=outcome dropped=1")
    );
    assert_eq!((drops.permits, drops.outcomes, drops.others), (9, 1, 0));
    assert!(session_lock_pacing_enabled(Some("1")));
    for opt_in in [None, Some("0"), Some("true"), Some(" 1")] {
        assert!(!session_lock_pacing_enabled(opt_in), "{opt_in:?}");
    }
}
