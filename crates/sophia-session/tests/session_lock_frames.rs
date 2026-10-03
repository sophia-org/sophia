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
