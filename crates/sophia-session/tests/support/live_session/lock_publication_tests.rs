//! t294: Session tells the lock provider about a new lock object only when
//! the lock phase or the topology moved and the object changed, and copies
//! the topology only then.

use crate::session_lock::{SessionLockPhase, SessionUnlockAttempt};
use sophia_engine::SessionLockEpoch;
use std::cell::Cell;
use std::num::NonZeroU64;

fn snapshot(epoch: u64) -> sophia_protocol::OutputAuthoritySnapshot {
    use sophia_protocol::*;
    OutputAuthoritySnapshot {
        topology_epoch: epoch,
        primary_output: OutputId::from_raw(1),
        heads: vec![OutputHeadDescriptor {
            head: DisplayHeadId::from_raw(1),
            generation: 1,
            label: "panel".into(),
            connected: true,
            enabled: true,
            vrr_capable: false,
            transforms: OutputTransformSet::ALL,
            current_mode: Some(DisplayModeId::from_raw(1)),
            modes: vec![OutputModeDescriptor {
                mode: DisplayModeId::from_raw(1),
                pixel_size: Size {
                    width: 1920,
                    height: 1080,
                },
                refresh_millihz: 60_000,
                preferred: true,
            }],
        }],
        groups: vec![OutputLogicalGroupState {
            output: OutputId::from_raw(1),
            generation: epoch,
            logical: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            members: vec![OutputGroupMember {
                head: DisplayHeadId::from_raw(1),
                mapping: OutputHeadMapping::Exact,
            }],
        }],
    }
}

fn locked(attempt: Option<u64>) -> SessionLockPhase {
    let epoch = SessionLockEpoch::from_raw(3).unwrap();
    SessionLockPhase::Locked {
        epoch,
        attempt: attempt.map(|serial| SessionUnlockAttempt {
            epoch,
            serial: NonZeroU64::new(serial).unwrap(),
        }),
    }
}

#[test]
fn the_lock_object_is_sent_once_per_change_and_the_topology_copied_only_then() {
    let mut publication = super::lock_provider::LockPublication::default();
    let copies = Cell::new(0);
    let mut update = |phase, epoch: u64| {
        publication.update(phase, Some(epoch), || {
            copies.set(copies.get() + 1);
            Some(snapshot(epoch))
        })
    };
    let first = update(SessionLockPhase::Unlocked, 5).expect("the first object");
    assert_eq!(first.lock_epoch, 0);
    assert!(update(SessionLockPhase::Unlocked, 5).is_none());
    assert_eq!(copies.get(), 1, "an unchanged key copies nothing");
    let locked_object = update(locked(None), 5).expect("locking changes the object");
    assert_eq!(locked_object.allocations.len(), 1);
    // An attempt moves the phase but not the object: nothing is sent.
    assert!(update(locked(Some(1)), 5).is_none());
    // A topology change is a new object.
    let moved = update(locked(Some(1)), 6).expect("a new topology");
    assert_eq!(moved.allocations[0].allocation_generation, 6);
    assert_eq!(copies.get(), 4);
}
