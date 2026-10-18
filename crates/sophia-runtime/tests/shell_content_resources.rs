use sophia_protocol::*;
use sophia_runtime::*;
#[path = "support/content_isb.rs"]
mod isb;

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 11,
        content_grant_epoch: 3,
    }
}
fn tx() -> TransactionId {
    TransactionId::from_raw(1)
}
fn begin(id: u64) -> ContentResourceBegin {
    ContentResourceBegin {
        grant: grant(),
        resource: ContentResourceId { id, generation: 1 },
        width_px: 2,
        height_px: 1,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 8,
    }
}
fn chunk(id: u64) -> ContentResourceChunk {
    ContentResourceChunk {
        grant: grant(),
        resource: begin(id).resource,
        ordinal: 0,
        offset: 0,
        bytes: vec![0, 0, 255, 255, 0, 128, 0, 128],
    }
}
fn owner() -> ContentResourceStore {
    ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap()
}
fn upload(store: &mut ContentResourceStore, id: u64) {
    isb::accept(store, tx(), begin(id), &chunk(id).bytes, 0);
}
fn drain(store: &mut ContentResourceStore) -> Vec<ContentResourceEvent> {
    std::iter::from_fn(|| store.take_event()).collect()
}

#[test]
fn incomplete_pixels_never_acquire_a_lease_and_completion_moves_credit_once() {
    let mut store = owner();
    store.begin_isb(tx(), begin(1), 0).unwrap();
    assert_eq!(
        store.usage(),
        ContentMemoryUsage {
            staging: 8,
            resident: 0,
            retiring: 0,
            reserved_resident: 8,
            backing: 8,
        }
    );
    assert!(store.lease(grant(), begin(1).resource).is_err());
    store
        .complete_isb(
            grant(),
            begin(1).resource,
            &mut Some(Ok(isb::validated(&chunk(1).bytes))),
            0,
        )
        .unwrap();
    assert_eq!(
        store.usage(),
        ContentMemoryUsage {
            staging: 0,
            resident: 8,
            retiring: 0,
            reserved_resident: 0,
            backing: 8,
        }
    );
    assert_eq!(
        store.lease(grant(), begin(1).resource).unwrap().bytes(),
        chunk(1).bytes
    );
    assert_eq!(
        store.complete_isb(
            grant(),
            begin(1).resource,
            &mut Some(Ok(isb::validated(&chunk(1).bytes))),
            0
        ),
        Err(ContentStoreError::Stale)
    );
    assert_eq!(drain(&mut store).len(), 1);
}

#[test]
fn retirement_waits_for_every_consumer_and_emits_one_release() {
    let mut store = owner();
    upload(&mut store, 1);
    drain(&mut store);
    let lease = store.lease(grant(), begin(1).resource).unwrap();
    let renderer = lease.clone();
    let retire = ContentResourceRetire {
        grant: grant(),
        resource: begin(1).resource,
    };
    store.retire(tx(), &retire).unwrap();
    assert_eq!(store.usage().resident, 0);
    assert_eq!(store.usage().retiring, 8);
    assert_eq!(store.usage().backing, 8);
    assert!(store.lease(grant(), retire.resource).is_err());
    assert!(drain(&mut store).is_empty());
    drop(lease);
    store.collect();
    assert!(!store.quiescent());
    assert_eq!(renderer.bytes(), chunk(1).bytes);
    drop(renderer);
    store.collect();
    assert!(!store.quiescent());
    assert_eq!(store.usage().backing, 8);
    let events = drain(&mut store);
    store.confirm_isb_release(begin(1).resource).unwrap();
    assert!(store.quiescent());
    assert_eq!(store.usage().backing, 0);
    assert!(matches!(
        events.as_slice(),
        [ContentResourceEvent {
            record: ShellContentRecord::ResourceReleased(_),
            ..
        }]
    ));
    assert_eq!(store.retire(tx(), &retire), Err(ContentStoreError::Stale));
    store.collect();
    assert!(drain(&mut store).is_empty());
}

#[test]
fn revoke_never_manufactures_gpu_completion_or_resurrects_old_epoch() {
    let mut store = owner();
    upload(&mut store, 1);
    drain(&mut store);
    let lease = store.lease(grant(), begin(1).resource).unwrap();
    store.revoke();
    assert!(!store.quiescent());
    assert_eq!(store.usage().resident, 8);
    assert!(drain(&mut store).is_empty());
    assert!(store.lease(grant(), begin(1).resource).is_err());
    assert_eq!(
        store.begin_isb(tx(), begin(2), 0),
        Err(ContentStoreError::Revoked)
    );
    assert_eq!(lease.bytes(), chunk(1).bytes);
    drop(lease);
    store.collect();
    assert!(store.quiescent());
    assert_eq!(drain(&mut store).len(), 1);
    store.revoke();
    assert!(drain(&mut store).is_empty());
}

#[test]
fn timeout_retains_reserved_capacity_until_validation_returns() {
    let mut store = owner();
    store.begin_isb(tx(), begin(1), 100).unwrap();
    drain(&mut store);
    store.expire(5099).unwrap();
    assert!(!store.quiescent());
    store.expire(5100).unwrap();
    assert!(!store.quiescent());
    assert_eq!(store.usage().staging, 8);
    assert_eq!(
        store.complete_isb(
            grant(),
            begin(1).resource,
            &mut Some(Ok(isb::validated(&chunk(1).bytes))),
            5100
        ),
        Err(ContentStoreError::Stale)
    );
    assert!(store.quiescent());
    let events = drain(&mut store);
    assert!(
        matches!(&events[0].record, ShellContentRecord::ResourceStatus(s) if s.status == 3 && s.reason == 6)
    );
    assert_eq!(store.usage(), ContentMemoryUsage::default());
    store.expire(5101).unwrap();
    assert!(drain(&mut store).is_empty());
    assert_eq!(
        store.begin_isb(tx(), begin(1), 5101),
        Err(ContentStoreError::Stale)
    );
    let mut next = begin(1);
    next.resource.generation = 2;
    store.begin_isb(tx(), next, 5101).unwrap();
}

#[test]
fn rejected_reservation_preserves_the_generation_for_retry() {
    let mut store = owner();
    let mut invalid = begin(1);
    invalid.total_bytes = 9;
    assert_eq!(
        store.begin_isb(tx(), invalid, 0),
        Err(ContentStoreError::Malformed)
    );
    store.begin_isb(tx(), begin(1), 0).unwrap();
    assert_eq!(store.usage().staging, 8);
}

#[test]
fn outcome_capacity_is_reserved_before_accepting_a_transfer() {
    let mut limits = ContentLimits::prototype(grant());
    limits.max_control_records = 3;
    let mut store = ContentResourceStore::new(limits).unwrap();
    store.begin_isb(tx(), begin(1), 0).unwrap();
    assert_eq!(
        store.begin_isb(tx(), begin(2), 0),
        Err(ContentStoreError::Budget)
    );
    store
        .complete_isb(
            grant(),
            begin(1).resource,
            &mut Some(Ok(isb::validated(&chunk(1).bytes))),
            0,
        )
        .unwrap();
    // Even a peer that stops draining feedback cannot prevent terminal release.
    store
        .retire(
            tx(),
            &ContentResourceRetire {
                grant: grant(),
                resource: begin(1).resource,
            },
        )
        .unwrap();
    assert_eq!(drain(&mut store).len(), 2);
    store.confirm_isb_release(begin(1).resource).unwrap();
    store.begin_isb(tx(), begin(3), 0).unwrap();
}

#[test]
fn a_foreign_grant_cannot_abort_the_live_transfer() {
    let mut store = owner();
    store.begin_isb(tx(), begin(1), 0).unwrap();
    let wrong = ContentGrant {
        content_grant_epoch: 99,
        ..grant()
    };
    let mut result = Some(Ok(isb::validated(&chunk(1).bytes)));
    assert_eq!(
        store.complete_isb(wrong, begin(1).resource, &mut result, 0),
        Err(ContentStoreError::Stale)
    );
    assert!(result.is_some());
    store
        .complete_isb(
            grant(),
            begin(1).resource,
            &mut Some(Ok(isb::validated(&chunk(1).bytes))),
            0,
        )
        .unwrap();
    assert_eq!(store.usage().resident, 8);
}

#[test]
fn reconnect_reserves_global_credit_and_keeps_old_pixels_alive() {
    let mut pool = ContentEpochPool::new(64 * 1024 * 1024).unwrap();
    pool.admit(ContentLimits::prototype(grant())).unwrap();
    assert_eq!(pool.reserved_bytes(), 40 * 1024 * 1024);
    assert_eq!(pool.reserved_backing_bytes(), 32 * 1024 * 1024);
    let store = pool.active_mut().unwrap();
    upload(store, 1);
    let old = store.lease(grant(), begin(1).resource).unwrap();
    pool.disconnect();
    assert_eq!(pool.retired_bytes(), 8);
    let retired = pool.accounting();
    assert_eq!(retired.active_epochs, 0);
    assert_eq!(retired.retired_epochs, 1);
    assert_eq!((retired.resources, retired.resource_ids), (1, 1));
    assert!(!retired.quiescent());
    assert_eq!(pool.retired_backing_bytes(), 8);
    assert_eq!(
        pool.admit(ContentLimits::prototype(grant())),
        Err(ContentStoreError::Stale)
    );
    let next = ContentGrant {
        connection_epoch: 12,
        content_grant_epoch: 4,
    };
    pool.admit(ContentLimits::prototype(next)).unwrap();
    assert_eq!(pool.accounting().grant, next);
    assert_eq!(pool.accounting().retired_epochs, 1);
    assert_eq!(pool.reserved_bytes(), 40 * 1024 * 1024 + 8);
    assert_eq!(pool.reserved_backing_bytes(), 32 * 1024 * 1024 + 8);
    assert_eq!(old.bytes(), chunk(1).bytes);
    drop(old);
    pool.collect();
    assert_eq!(pool.retired_bytes(), 0);
    assert_eq!(pool.reserved_backing_bytes(), 32 * 1024 * 1024);
    pool.disconnect();
    assert_eq!(pool.reserved_bytes(), 0);
    assert_eq!(pool.reserved_backing_bytes(), 0);
    assert!(pool.accounting().quiescent());
    assert_eq!(pool.accounting().grant, next);
}

#[test]
fn inventory_reads_preserve_real_staging_and_last_consumer_custody() {
    let mut pool = ContentEpochPool::new(64 * 1024 * 1024).unwrap();
    assert!(pool.accounting().quiescent());
    pool.admit(ContentLimits::prototype(grant())).unwrap();
    pool.active_mut()
        .unwrap()
        .begin_isb(tx(), begin(1), 0)
        .unwrap();
    let staged = pool.accounting();
    assert_eq!(
        (staged.transfers, staged.resources, staged.resource_ids),
        (1, 0, 1)
    );
    assert_eq!(staged.memory.staging, 8);
    assert_eq!(staged.memory.reserved_resident, 8);
    assert!(staged.response_records > 0);
    assert_eq!(pool.accounting(), staged); // Observation does not drain a response or collect.
    let store = pool.active_mut().unwrap();
    store
        .complete_isb(
            grant(),
            begin(1).resource,
            &mut Some(Ok(isb::validated(&chunk(1).bytes))),
            2,
        )
        .unwrap();
    let consumer = store.lease(grant(), begin(1).resource).unwrap();
    let live = pool.accounting();
    assert_eq!(
        (live.transfers, live.resources, live.resource_ids),
        (0, 1, 1)
    );
    assert_eq!(live.memory.resident, 8);
    assert_eq!(live.memory.backing, 8);
    pool.disconnect();
    let retained = pool.accounting();
    assert_eq!(retained.reserved_bytes, 8);
    assert!(!retained.quiescent());
    drop(consumer);
    assert_eq!(pool.accounting(), retained); // Ending a consumer is not a fabricated collection.
    pool.collect();
    assert!(pool.accounting().quiescent());
    assert_eq!(pool.accounting().grant, grant());
}

#[test]
fn epoch_pool_collects_an_active_resource_after_its_last_lease_drains() {
    let mut pool = ContentEpochPool::new(64 * 1024 * 1024).unwrap();
    pool.admit(ContentLimits::prototype(grant())).unwrap();
    let store = pool.active_mut().unwrap();
    upload(store, 1);
    drain(store);
    let lease = store.lease(grant(), begin(1).resource).unwrap();
    store
        .retire(
            tx(),
            &ContentResourceRetire {
                grant: grant(),
                resource: begin(1).resource,
            },
        )
        .unwrap();
    assert!(store.pending_event().is_none());
    drop(lease);
    pool.collect();
    assert!(matches!(
        pool.active_mut().unwrap().take_event(),
        Some(ContentResourceEvent {
            record: ShellContentRecord::ResourceReleased(_),
            ..
        })
    ));
}

#[test]
fn tiny_pinned_epochs_also_meet_a_metadata_bound() {
    let mut pool = ContentEpochPool::new(64 * 1024 * 1024).unwrap();
    let mut leases = Vec::new();
    for i in 1..=16 {
        let g = ContentGrant {
            connection_epoch: i,
            content_grant_epoch: i,
        };
        pool.admit(ContentLimits::prototype(g)).unwrap();
        let owner = pool.active_mut().unwrap();
        let mut b = begin(1);
        b.grant = g;
        isb::accept(owner, tx(), b, &chunk(1).bytes, 0);
        leases.push(owner.lease(g, begin(1).resource).unwrap());
        pool.disconnect();
    }
    let next = ContentGrant {
        connection_epoch: 17,
        content_grant_epoch: 17,
    };
    assert_eq!(pool.retired_bytes(), 128);
    assert_eq!(
        pool.admit(ContentLimits::prototype(next)),
        Err(ContentStoreError::Budget)
    );
    leases.pop();
    pool.collect();
    pool.admit(ContentLimits::prototype(next)).unwrap();
}

#[test]
fn cancelled_transfer_echoes_the_cancel_not_the_original_begin() {
    let mut store = owner();
    store.begin_isb(tx(), begin(1), 0).unwrap();
    drain(&mut store);
    let cancel_tx = TransactionId::from_raw(42);
    store
        .cancel(
            cancel_tx,
            &ContentResourceCancel {
                grant: grant(),
                resource: begin(1).resource,
            },
        )
        .unwrap();
    assert!(drain(&mut store).is_empty());
    assert_eq!(
        store.complete_isb(
            grant(),
            begin(1).resource,
            &mut Some(Err(ContentStoreError::Malformed)),
            0
        ),
        Err(ContentStoreError::Stale)
    );
    assert_eq!(drain(&mut store)[0].transaction, cancel_tx);
    store.collect();
    assert!(drain(&mut store).is_empty());
}

#[test]
fn partial_retirement_reopens_exactly_the_reserved_global_headroom() {
    let mut pool = ContentEpochPool::new(64 * 1024 * 1024).unwrap();
    let mut leases = Vec::new();
    for epoch in 1..=2 {
        let g = ContentGrant {
            connection_epoch: epoch,
            content_grant_epoch: epoch,
        };
        pool.admit(ContentLimits::prototype(g)).unwrap();
        let owner = pool.active_mut().unwrap();
        for id in 1..=4 {
            let b = ContentResourceBegin {
                grant: g,
                resource: ContentResourceId { id, generation: 1 },
                width_px: 8192,
                height_px: 128,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 128,
                total_bytes: 4194304,
            };
            isb::accept(owner, tx(), b.clone(), &vec![0; b.total_bytes as usize], 0);
            leases.push(owner.lease(g, b.resource).unwrap());
            drain(owner);
        }
        pool.disconnect();
    }
    let next = ContentGrant {
        connection_epoch: 3,
        content_grant_epoch: 3,
    };
    assert_eq!(pool.retired_bytes(), 32 * 1024 * 1024);
    assert_eq!(
        pool.admit(ContentLimits::prototype(next)),
        Err(ContentStoreError::Budget)
    );
    leases.pop();
    pool.collect();
    assert_eq!(
        pool.admit(ContentLimits::prototype(next)),
        Err(ContentStoreError::Budget)
    );
    leases.pop();
    pool.collect();
    assert_eq!(pool.retired_bytes(), 24 * 1024 * 1024);
    pool.admit(ContentLimits::prototype(next)).unwrap();
    assert_eq!(pool.reserved_bytes(), 64 * 1024 * 1024);
    assert_eq!(leases[0].bytes().len(), 4194304);
}
