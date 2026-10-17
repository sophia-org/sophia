#![cfg(test)]
//! Sealed leases move from live image to retiring to orphan without adding
//! entries, and the next epoch's resources count orphans against the same
//! live-resource and sealed-byte budgets until every clone has gone.
use super::*;
use crate::lock_files::resources::{ResourcePlan, Resources};
use sophia_protocol::lock_files::{LockFileLimits, LockResourceBegin, LockResourceId};
use std::sync::Arc;

fn limits() -> LockFileLimits {
    LockFileLimits {
        max_outputs: 1,
        upload_slots: 1,
        max_chords: 0,
        max_width_px: 16,
        max_height_px: 16,
        max_resource_bytes: 1024,
        max_live_resources: 2,
        journal_records: 32,
        journal_bytes: 8192,
        assembly_timeout_ms: 1000,
        ack_progress_timeout_ms: 2000,
    }
}

fn id(id: u64) -> LockResourceId {
    LockResourceId { id, generation: 1 }
}

fn pixels(len: usize) -> SharedPixels {
    SharedPixels::from(Arc::<[u8]>::from(vec![0; len]))
}

fn begin(resource: LockResourceId) -> LockResourceBegin {
    LockResourceBegin {
        transaction: 9,
        resource,
        width_px: 2,
        height_px: 2,
        slot: 0,
    }
}

fn budget(plan: &ResourcePlan) -> bool {
    matches!(plan, ResourcePlan::Reject { reason, .. } if *reason == reason::BUDGET)
}

#[test]
fn orphans_keep_their_places_and_bytes_in_the_next_epoch_until_released() {
    let mut ended = Resources::new(limits());
    let live = pixels(64);
    let retired = pixels(16);
    ended.insert_sealed(id(1), 4, 4, live.clone());
    ended.insert_sealed(id(2), 2, 2, retired.clone());
    ended.apply(ResourcePlan::Retire {
        transaction: 5,
        resource: id(2),
    });
    // Retiring keeps its identity, place, bytes and release credit.
    assert!(ended.holds(id(2)));
    assert_eq!((ended.live(), ended.sealed_bytes()), (2, 80));
    assert_eq!(ended.leases.credits(), 1);
    assert!(budget(&ended.plan_begin(begin(id(3)), &[]).unwrap()));

    let mut orphans = OrphanLeases(ended.take_sealed_leases());
    drop(ended);
    assert_eq!(orphans.len(), 2);
    assert!(orphans.poll());

    let mut next = Resources::new(limits());
    next.leases.orphans.append(&mut orphans.0);
    // Orphans hold places and bytes but no identity and no credit.
    assert!(!next.holds(id(1)));
    assert_eq!((next.live(), next.sealed_bytes()), (2, 80));
    assert_eq!(next.leases.credits(), 0);
    assert!(budget(&next.plan_begin(begin(id(3)), &[]).unwrap()));
    // One reader lets go: one place and its bytes return, silently.
    drop(retired);
    assert!(next.leases.released().is_empty());
    assert_eq!((next.live(), next.sealed_bytes()), (1, 64));
    assert!(!budget(&next.plan_begin(begin(id(3)), &[]).unwrap()));
    drop(live);
    next.leases.released();
    assert_eq!((next.live(), next.sealed_bytes()), (0, 0));
}

#[test]
fn a_retiring_lease_is_released_only_after_its_last_clone() {
    let mut resources = Resources::new(limits());
    let held = pixels(32);
    resources.insert_sealed(id(1), 4, 2, held.clone());
    let candidate = held.clone();
    resources.apply(ResourcePlan::Retire {
        transaction: 7,
        resource: id(1),
    });
    drop(held);
    assert!(resources.leases.released().is_empty());
    drop(candidate);
    assert_eq!(resources.leases.released(), [(7, id(1))]);
    // Still charged until custody journals the release and removes it.
    assert_eq!(resources.live(), 1);
    resources.leases.remove(id(1));
    assert_eq!((resources.live(), resources.leases.credits()), (0, 0));
}
