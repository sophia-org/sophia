use sophia_protocol::*;
use sophia_runtime::*;

const MIB: u64 = 1024 * 1024;

fn grant(epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    }
}

fn bar(epoch: u64) -> ContentLimits {
    ContentLimits::prototype(grant(epoch))
}

fn launcher(epoch: u64) -> ContentLimits {
    let mut limits = bar(epoch);
    limits.max_staging_bytes = 4 * MIB;
    limits.max_resident_bytes = 12 * MIB;
    limits.max_retiring_bytes = 8 * MIB;
    limits
}

fn upload(registry: &mut ContentEpochRegistry, epoch: u64, value: u8) -> ContentResourceLease {
    let grant = grant(epoch);
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let store = registry.resources_mut(grant).unwrap();
    let tx = TransactionId::from_raw(1);
    store
        .begin(
            tx,
            ContentResourceBegin {
                grant,
                resource,
                width_px: 1,
                height_px: 1,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 4,
            },
            0,
        )
        .unwrap();
    store
        .chunk(
            tx,
            &ContentResourceChunk {
                grant,
                resource,
                ordinal: 0,
                offset: 0,
                bytes: vec![value, 0, 0, 255],
            },
            1,
        )
        .unwrap();
    store
        .end(
            tx,
            &ContentResourceEnd {
                grant,
                resource,
                total_bytes: 4,
                chunk_count: 1,
            },
            2,
        )
        .unwrap();
    store.lease(grant, resource).unwrap()
}

#[test]
fn two_real_stores_share_one_budget_and_keep_identical_resource_ids_distinct() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    registry.admit(bar(1)).unwrap();
    registry.admit(launcher(2)).unwrap();
    let a = upload(&mut registry, 1, 30);
    let b = upload(&mut registry, 2, 70);
    assert_ne!(a.bytes(), b.bytes());
    assert_eq!(registry.accounting().active_epochs, 2);
    assert_eq!(registry.accounting().resources, 2);
    assert_eq!(registry.reserved_bytes(), 64 * MIB);
    assert_eq!(registry.reserved_backing_bytes(), 52 * MIB);
    assert_eq!(registry.admit(launcher(3)), Err(ContentStoreError::Budget));
    assert!(!registry.disconnect(grant(3)));
    assert_eq!(registry.accounting().active_epochs, 2);
}

#[test]
fn independent_disconnect_keeps_neighbor_live_and_blocks_replacement_until_real_release() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    registry.admit(bar(1)).unwrap();
    registry.admit(launcher(2)).unwrap();
    let bar_pixels = upload(&mut registry, 1, 1);
    let old_launcher = upload(&mut registry, 2, 2);
    assert!(registry.disconnect(grant(2)));
    assert!(registry.resources(grant(2)).is_none());
    assert!(registry.resources(grant(1)).is_some());
    assert_eq!(registry.retired_bytes(), 4);
    assert_eq!(registry.reserved_bytes(), 40 * MIB + 4);
    assert_eq!(registry.admit(launcher(3)), Err(ContentStoreError::Budget));
    assert_eq!(bar_pixels.bytes(), [1, 0, 0, 255]);
    assert_eq!(old_launcher.bytes(), [2, 0, 0, 255]);
    drop(old_launcher);
    registry.collect();
    assert_eq!(registry.retired_bytes(), 0);
    // The refused reservation did not consume the identity or change the bar.
    registry.admit(launcher(3)).unwrap();
    assert!(!registry.disconnect(grant(2)));
    assert!(registry.resources(grant(3)).is_some());
    assert!(registry.resources(grant(1)).is_some());
    assert_eq!(registry.accounting().active_epochs, 2);
    assert!(registry.disconnect(grant(3)));
    assert!(registry.disconnect(grant(1)));
    assert!(!registry.accounting().quiescent());
    drop(bar_pixels);
    registry.collect();
    assert!(registry.accounting().quiescent());
}

#[test]
fn no_duplicated_budget_and_no_replayed_or_half_matching_grant() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    registry.admit(bar(1)).unwrap();
    assert_eq!(registry.admit(bar(2)), Err(ContentStoreError::Budget));
    assert_eq!(registry.accounting().active_epochs, 1);
    registry.admit(launcher(2)).unwrap();
    assert!(registry.disconnect(grant(1)));
    let wrong = ContentGrant {
        connection_epoch: 2,
        content_grant_epoch: 1,
    };
    assert!(registry.resources(wrong).is_none());
    assert!(registry.candidates_mut(wrong).is_none());
    assert!(!registry.disconnect(wrong));
    assert_eq!(registry.admit(bar(1)), Err(ContentStoreError::Stale));
    let mut replay = bar(3);
    replay.grant.content_grant_epoch = 2;
    assert_eq!(registry.admit(replay), Err(ContentStoreError::Stale));
    registry.admit(bar(3)).unwrap();
}

#[test]
fn final_backend_disposition_requires_every_live_grant_to_end() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    registry.admit(bar(1)).unwrap();
    registry.admit(launcher(2)).unwrap();
    let held = upload(&mut registry, 2, 2);
    registry.disconnect(grant(2));
    let backend = registry
        .finish_after_backend_drop(Box::new(71))
        .unwrap_err();
    assert_eq!(*backend, 71);
    registry.disconnect(grant(1));
    assert_eq!(registry.finish_after_backend_drop(backend).unwrap(), 0);
    assert!(!registry.accounting().quiescent());
    assert_eq!(held.bytes(), [2, 0, 0, 255]);
    drop(held);
    registry.collect();
    assert!(registry.accounting().quiescent());
}

#[test]
fn global_retirement_inventory_reserves_room_for_both_active_disconnects() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut consumers = Vec::new();
    // Small live limits allow accumulating metadata without byte exhaustion.
    let small = |epoch| {
        let mut limits = launcher(epoch);
        limits.max_resource_bytes = MIB;
        limits.max_staging_bytes = MIB;
        limits.max_resident_bytes = MIB;
        limits.max_retiring_bytes = MIB;
        limits
    };
    for epoch in 1..=14 {
        registry.admit(small(epoch)).unwrap();
        consumers.push(upload(&mut registry, epoch, 1));
        registry.disconnect(grant(epoch));
    }
    registry.admit(small(15)).unwrap();
    registry.admit(small(16)).unwrap();
    consumers.push(upload(&mut registry, 15, 1));
    consumers.push(upload(&mut registry, 16, 1));
    registry.disconnect(grant(15));
    assert_eq!(registry.admit(small(17)), Err(ContentStoreError::Budget));
    registry.disconnect(grant(16));
    assert_eq!(registry.accounting().retired_epochs, 16);
    assert_eq!(registry.admit(small(17)), Err(ContentStoreError::Budget));
    consumers.clear();
    registry.collect();
    registry.admit(small(17)).unwrap();
}
