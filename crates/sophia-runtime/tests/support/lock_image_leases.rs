#![cfg(test)]
//! ISB leases move from live image to retiring to orphan without adding
//! entries, and the next epoch's resources count orphans against the same
//! live-resource and sealed-byte budgets until every clone has gone.
use super::*;
use crate::lock_files::resources::Resources;
use sophia_protocol::lock_files::LockResourceId;
use std::sync::Arc;

fn id(id: u64) -> LockResourceId {
    LockResourceId { id, generation: 1 }
}

fn pixels(len: usize) -> SharedPixels {
    SharedPixels::from(Arc::<[u8]>::from(vec![0; len]))
}

#[test]
fn orphans_keep_their_places_and_bytes_in_the_next_epoch_until_released() {
    let mut ended = Resources::new();
    let live = pixels(64);
    let retired = pixels(16);
    ended.insert_isb(id(1), 4, 4, live.clone());
    ended.insert_isb(id(2), 2, 2, retired.clone());
    ended.retire(5, id(2));
    // Retiring keeps its identity, place, bytes and release credit.
    assert!(ended.holds(id(2)));
    assert_eq!((ended.live(), ended.isb_bytes()), (2, 80));
    assert_eq!(ended.leases.credits(), 1);

    let mut orphans = OrphanLeases(ended.take_isb_leases());
    drop(ended);
    assert_eq!(orphans.len(), 2);
    assert!(orphans.poll());

    let mut next = Resources::new();
    next.leases.orphans.append(&mut orphans.0);
    // Orphans hold places and bytes but no identity and no credit.
    assert!(!next.holds(id(1)));
    assert_eq!((next.live(), next.isb_bytes()), (2, 80));
    assert_eq!(next.leases.credits(), 0);
    // One reader lets go: one place and its bytes return, silently.
    drop(retired);
    assert!(next.leases.released().is_empty());
    assert_eq!((next.live(), next.isb_bytes()), (1, 64));
    drop(live);
    next.leases.released();
    assert_eq!((next.live(), next.isb_bytes()), (0, 0));
}

#[test]
fn a_retiring_lease_is_released_only_after_its_last_clone() {
    let mut resources = Resources::new();
    let held = pixels(32);
    resources.insert_isb(id(1), 4, 2, held.clone());
    let candidate = held.clone();
    resources.retire(7, id(1));
    drop(held);
    assert!(resources.leases.released().is_empty());
    drop(candidate);
    assert_eq!(resources.leases.released(), [(7, id(1))]);
    // Still charged until custody journals the release and removes it.
    assert_eq!(resources.live(), 1);
    resources.leases.remove(id(1));
    assert_eq!((resources.live(), resources.leases.credits()), (0, 0));
}
