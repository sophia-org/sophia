//! Source extents cannot escape the Session budget across reconnect or workers.
use sophia_protocol::*;
use sophia_runtime::*;
use std::fs::File;

fn grant(epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    }
}
fn registry() -> ContentEpochRegistry {
    let mut registry = ContentEpochRegistry::new(64 << 20).unwrap();
    registry.admit(ContentLimits::prototype(grant(1))).unwrap();
    registry
}
fn source(geometry: OwnedRingGeometry) -> File {
    let file = File::from(
        rustix::fs::memfd_create("owned-source-budget", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    file.set_len(geometry.source_bytes() as u64).unwrap();
    file
}

#[test]
fn reservation_charges_source_and_backing_without_spending_resource_allowance() {
    let mut registry = registry();
    let before = registry.accounting();
    let geometry = OwnedRingGeometry::new(3, 4 << 20).unwrap();
    let bytes = geometry.source_bytes() as u64;
    let held = registry
        .reserve_owned_ring_source(grant(1), geometry)
        .unwrap();
    assert_eq!(held.grant(), grant(1));
    assert_eq!(registry.accounting().source_bytes, bytes);
    assert_eq!(registry.reserved_bytes(), before.reserved_bytes + bytes);
    assert_eq!(
        registry.reserved_backing_bytes(),
        before.reserved_backing_bytes + bytes
    );
    assert_eq!(registry.accounting().memory, before.memory);
    // A second large ring would exceed the fixed 64 MiB aggregate envelope.
    assert!(matches!(
        registry.reserve_owned_ring_source(grant(1), geometry),
        Err(ContentStoreError::Budget)
    ));
    assert_eq!(registry.accounting().source_bytes, bytes);
    drop(held);
    assert_eq!(registry.accounting(), before);
    assert!(
        registry
            .reserve_owned_ring_source(grant(1), geometry)
            .is_ok()
    );
}

#[test]
fn rejected_fd_returns_reservation_and_wrong_or_revoked_grants_cannot_reserve() {
    let mut registry = registry();
    let before = registry.accounting();
    let geometry = OwnedRingGeometry::new(2, 8).unwrap();
    assert!(matches!(
        registry.reserve_owned_ring_source(grant(2), geometry),
        Err(ContentStoreError::Stale)
    ));
    let held = registry
        .reserve_owned_ring_source(grant(1), geometry)
        .unwrap();
    let file = source(geometry);
    file.set_len(0).unwrap();
    assert!(OwnedRing::admit(file, held).is_err());
    assert_eq!(registry.accounting(), before);
    registry.resources_mut(grant(1)).unwrap().revoke();
    assert!(matches!(
        registry.reserve_owned_ring_source(grant(1), geometry),
        Err(ContentStoreError::Revoked)
    ));
}

#[test]
fn disconnected_source_reads_keep_epoch_and_reconnect_charge_until_last_ticket_drops() {
    let mut registry = registry();
    let geometry = OwnedRingGeometry::new(3, 4 << 20).unwrap();
    let bytes = geometry.source_bytes() as u64;
    let held = registry
        .reserve_owned_ring_source(grant(1), geometry)
        .unwrap();
    let mut ring = OwnedRing::admit(source(geometry), held).unwrap();
    let read = ring
        .offer(
            OwnedSlotIdentity {
                offer: 1,
                slot: 0,
                generation: 2,
            },
            8,
        )
        .unwrap();
    assert!(registry.disconnect(grant(1)));
    drop(ring);
    registry.collect();
    assert_eq!(registry.accounting().retired_epochs, 1);
    assert_eq!(registry.accounting().source_bytes, bytes);
    assert_eq!(registry.retired_bytes(), bytes);
    assert_eq!(registry.retired_backing_bytes(), bytes);
    let budget = registry.reconnect_budget(ContentStoreProfile::Legacy);
    assert_eq!(budget.own_retired_bytes, bytes);
    assert_eq!(budget.own_retired_epochs, 1);
    assert_eq!(budget.reserved_bytes, bytes);
    // New admission cannot make a retired read disappear or allocate another
    // large source against its retained charge.
    registry.admit(ContentLimits::prototype(grant(2))).unwrap();
    assert!(matches!(
        registry.reserve_owned_ring_source(grant(2), geometry),
        Err(ContentStoreError::Budget)
    ));
    assert!(matches!(
        registry.reserve_owned_ring_source(grant(1), geometry),
        Err(ContentStoreError::Stale)
    ));
    drop(read);
    registry.collect();
    assert_eq!(registry.accounting().retired_epochs, 0);
    assert_eq!(registry.accounting().source_bytes, 0);
    assert!(
        registry
            .reserve_owned_ring_source(grant(2), geometry)
            .is_ok()
    );
}

#[test]
fn slot_release_does_not_return_a_live_rings_extent_reservation() {
    let mut registry = registry();
    let geometry = OwnedRingGeometry::new(1, 8).unwrap();
    let held = registry
        .reserve_owned_ring_source(grant(1), geometry)
        .unwrap();
    let mut ring = OwnedRing::admit(source(geometry), held).unwrap();
    let identity = OwnedSlotIdentity {
        offer: 1,
        slot: 0,
        generation: 2,
    };
    drop(ring.offer(identity, 8).unwrap());
    assert!(ring.confirm_release(identity));
    assert_eq!(
        registry.accounting().source_bytes,
        geometry.source_bytes() as u64
    );
    drop(ring);
    assert_eq!(registry.accounting().source_bytes, 0);
}
