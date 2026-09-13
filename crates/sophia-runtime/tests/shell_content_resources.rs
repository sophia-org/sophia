use sophia_protocol::*;
use sophia_runtime::*;

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
fn end(id: u64) -> ContentResourceEnd {
    ContentResourceEnd {
        grant: grant(),
        resource: begin(id).resource,
        total_bytes: 8,
        chunk_count: 1,
    }
}
fn owner() -> ContentResourceStore {
    ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap()
}
fn upload(store: &mut ContentResourceStore, id: u64) {
    store.begin(tx(), begin(id), 0).unwrap();
    store.chunk(tx(), &chunk(id), 0).unwrap();
    store.end(tx(), &end(id), 0).unwrap();
}
fn drain(store: &mut ContentResourceStore) -> Vec<ContentResourceEvent> {
    std::iter::from_fn(|| store.take_event()).collect()
}

#[test]
fn incomplete_pixels_never_acquire_a_lease_and_end_moves_credit_once() {
    let mut store = owner();
    store.begin(tx(), begin(1), 0).unwrap();
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
    store.chunk(tx(), &chunk(1), 0).unwrap();
    assert!(store.lease(grant(), begin(1).resource).is_err());
    store.end(tx(), &end(1), 0).unwrap();
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
    assert_eq!(store.end(tx(), &end(1), 0), Err(ContentStoreError::Stale));
    assert_eq!(drain(&mut store).len(), 2);
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
    assert!(store.quiescent());
    assert_eq!(store.usage().backing, 0);
    let events = drain(&mut store);
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
        store.begin(tx(), begin(2), 0),
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
fn timeout_settles_the_begin_and_releases_reserved_capacity() {
    let mut store = owner();
    store.begin(tx(), begin(1), 100).unwrap();
    drain(&mut store);
    store.expire(599).unwrap();
    assert!(!store.quiescent());
    store.expire(600).unwrap();
    assert!(store.quiescent());
    let events = drain(&mut store);
    assert!(
        matches!(&events[0].record, ShellContentRecord::ResourceStatus(s) if s.status == 3 && s.reason == 6)
    );
    assert_eq!(store.usage(), ContentMemoryUsage::default());
    store.expire(601).unwrap();
    assert!(drain(&mut store).is_empty());
    assert_eq!(
        store.begin(tx(), begin(1), 601),
        Err(ContentStoreError::Stale)
    );
    let mut next = begin(1);
    next.resource.generation = 2;
    store.begin(tx(), next, 601).unwrap();
}

#[test]
fn malformed_or_duplicate_chunks_settle_without_partial_acceptance() {
    for variant in 0..4 {
        let mut store = owner();
        store.begin(tx(), begin(1), 0).unwrap();
        drain(&mut store);
        let mut bytes = chunk(1);
        match variant {
            0 => bytes.offset = 4,
            1 => bytes.ordinal = 1,
            2 => {
                bytes.bytes.pop();
            }
            _ => bytes.bytes[3] = 0,
        }
        assert_eq!(
            store.chunk(tx(), &bytes, 1),
            Err(ContentStoreError::Malformed)
        );
        assert!(store.quiescent());
        assert_eq!(drain(&mut store).len(), 1);
        assert!(store.lease(grant(), begin(1).resource).is_err());
    }
    let mut store = owner();
    store.begin(tx(), begin(1), 0).unwrap();
    store.chunk(tx(), &chunk(1), 0).unwrap();
    assert_eq!(
        store.chunk(tx(), &chunk(1), 0),
        Err(ContentStoreError::Malformed)
    );
    assert!(store.quiescent());
}

#[test]
fn rejected_begin_burns_the_generation_without_evicting_replay_state() {
    let mut store = owner();
    let mut invalid = begin(1);
    invalid.total_bytes = 9;
    assert_eq!(
        store.begin(tx(), invalid, 0),
        Err(ContentStoreError::Malformed)
    );
    assert_eq!(
        store.begin(tx(), begin(1), 0),
        Err(ContentStoreError::Stale)
    );
    let mut next = begin(1);
    next.resource.generation = 2;
    store.begin(tx(), next, 0).unwrap();
    assert_eq!(store.usage().staging, 8);
}

#[test]
fn outcome_capacity_is_reserved_before_accepting_a_transfer() {
    let mut limits = ContentLimits::prototype(grant());
    limits.max_control_records = 3;
    let mut store = ContentResourceStore::new(limits).unwrap();
    store.begin(tx(), begin(1), 0).unwrap();
    assert_eq!(
        store.begin(tx(), begin(2), 0),
        Err(ContentStoreError::Budget)
    );
    store.chunk(tx(), &chunk(1), 0).unwrap();
    store.end(tx(), &end(1), 0).unwrap();
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
    assert_eq!(drain(&mut store).len(), 3);
    store.begin(tx(), begin(3), 0).unwrap();
}

#[test]
fn a_foreign_grant_cannot_abort_the_live_transfer() {
    let mut store = owner();
    store.begin(tx(), begin(1), 0).unwrap();
    let mut wrong = chunk(1);
    wrong.grant.content_grant_epoch += 1;
    assert_eq!(store.chunk(tx(), &wrong, 0), Err(ContentStoreError::Stale));
    store.chunk(tx(), &chunk(1), 0).unwrap();
    store.end(tx(), &end(1), 0).unwrap();
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
        let mut c = chunk(1);
        c.grant = g;
        let mut e = end(1);
        e.grant = g;
        owner.begin(tx(), b, 0).unwrap();
        owner.chunk(tx(), &c, 0).unwrap();
        owner.end(tx(), &e, 0).unwrap();
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
    store.begin(tx(), begin(1), 0).unwrap();
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
            owner.begin(tx(), b.clone(), 0).unwrap();
            for ordinal in 0..128 {
                owner
                    .chunk(
                        tx(),
                        &ContentResourceChunk {
                            grant: g,
                            resource: b.resource,
                            ordinal,
                            offset: u64::from(ordinal) * 32768,
                            bytes: vec![0; 32768],
                        },
                        0,
                    )
                    .unwrap();
            }
            owner
                .end(
                    tx(),
                    &ContentResourceEnd {
                        grant: g,
                        resource: b.resource,
                        total_bytes: 4194304,
                        chunk_count: 128,
                    },
                    0,
                )
                .unwrap();
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
