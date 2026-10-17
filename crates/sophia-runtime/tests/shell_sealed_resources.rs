//! Store custody with real sealed mappings; no socket or worker-pool claim.
use sophia_linux_peer::SealedMapping;
use sophia_protocol::*;
use sophia_runtime::*;
use std::io::Write;

fn mapped(bytes: &[u8]) -> SealedMapping {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "sealed-resource",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(bytes).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    SealedMapping::map(file.into(), bytes.len()).unwrap()
}
fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 2,
    }
}
fn description(id: u64) -> ContentResourceBegin {
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
fn tx() -> TransactionId {
    TransactionId::from_raw(1)
}
fn owner() -> ContentResourceStore {
    ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap()
}
fn validated() -> ValidatedSealedPixels {
    let mut work = SealedPixelValidation::new(mapped(&[1, 2, 3, 255, 4, 5, 6, 255])).unwrap();
    assert!(work.step().unwrap());
    work.finish().unwrap()
}

#[test]
fn mapping_moves_without_copy_and_lives_through_every_retiring_lease() {
    let mut store = owner();
    let begin = description(1);
    store.begin_sealed(tx(), begin.clone(), 0).unwrap();
    assert!(
        store.take_event().is_none(),
        "ImageAdmitted belongs to offer owner"
    );
    assert_eq!(store.usage().staging, 8);
    assert_eq!(store.usage().backing, 8);
    assert!(store.lease(grant(), begin.resource).is_err());
    let pixels = validated();
    let address = pixels.bytes().as_ptr();
    store
        .complete_sealed(grant(), begin.resource, Ok(pixels), 1)
        .unwrap();
    let lease = store.lease(grant(), begin.resource).unwrap();
    assert_eq!(
        lease.bytes().as_ptr(),
        address,
        "acceptance must not copy pixels"
    );
    assert_eq!(store.usage().staging, 0);
    assert_eq!(store.usage().reserved_resident, 0);
    assert_eq!(store.usage().resident, 8);
    assert_eq!(store.usage().backing, 8);
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::ResourceStatus(ContentResourceStatus { status: 2, .. })
    ));
    let renderer = lease.clone();
    store
        .retire(
            tx(),
            &ContentResourceRetire {
                grant: grant(),
                resource: begin.resource,
            },
        )
        .unwrap();
    assert!(store.lease(grant(), begin.resource).is_err());
    assert_eq!(store.usage().retiring, 8);
    drop(lease);
    store.collect();
    assert!(store.take_event().is_none());
    assert_eq!(renderer.bytes(), &[1, 2, 3, 255, 4, 5, 6, 255]);
    drop(renderer);
    store.collect();
    assert_eq!(
        store.confirm_sealed_release(begin.resource),
        Err(ContentStoreError::Incomplete)
    );
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::ResourceReleased(_)
    ));
    assert_eq!(
        store.usage().retiring,
        8,
        "queued release still owns backing"
    );
    assert_eq!(store.usage().backing, 8);
    store.confirm_sealed_release(begin.resource).unwrap();
    assert_eq!(store.usage(), ContentMemoryUsage::default());
    store.collect();
    assert!(store.take_event().is_none());
}

#[test]
fn stopped_validation_keeps_credit_until_worker_returns() {
    for stop in 0..3 {
        let mut store = owner();
        let begin = description(1);
        store.begin_sealed(tx(), begin.clone(), 0).unwrap();
        let pixels = validated();
        let (release, wait) = std::sync::mpsc::sync_channel(0);
        let worker = std::thread::spawn(move || {
            wait.recv().unwrap();
            pixels
        });
        match stop {
            0 => store
                .cancel(
                    tx(),
                    &ContentResourceCancel {
                        grant: grant(),
                        resource: begin.resource,
                    },
                )
                .unwrap(),
            1 => store.expire(2000).unwrap(),
            _ => store.revoke(),
        }
        assert_eq!(store.usage().staging, 8);
        assert_eq!(store.usage().reserved_resident, 8);
        assert_eq!(store.usage().backing, 8);
        assert!(!store.quiescent());
        assert!(store.take_event().is_none());
        release.send(()).unwrap();
        assert!(
            store
                .complete_sealed(
                    grant(),
                    begin.resource,
                    Ok(worker.join().unwrap()),
                    if stop == 1 { 2000 } else { 1 }
                )
                .is_err()
        );
        assert_eq!(store.usage(), ContentMemoryUsage::default());
        assert!(store.quiescent());
        let ShellContentRecord::ResourceStatus(status) = store.take_event().unwrap().record else {
            panic!("status")
        };
        assert_eq!(
            status.reason,
            match stop {
                0 => ContentReason::Cancelled,
                1 => ContentReason::Timeout,
                _ => ContentReason::Revoked,
            } as u16
        );
        assert!(store.take_event().is_none());
    }
}

#[test]
fn byte_operations_cannot_complete_or_abort_a_sealed_worker() {
    let mut store = owner();
    let begin = description(1);
    store.begin_sealed(tx(), begin.clone(), 0).unwrap();
    assert_eq!(
        store.chunk(
            tx(),
            &ContentResourceChunk {
                grant: grant(),
                resource: begin.resource,
                ordinal: 0,
                offset: 0,
                bytes: vec![0; 8],
            },
            1
        ),
        Err(ContentStoreError::Malformed)
    );
    assert_eq!(
        store.end(
            tx(),
            &ContentResourceEnd {
                grant: grant(),
                resource: begin.resource,
                chunk_count: 1,
                total_bytes: 8,
            },
            1
        ),
        Err(ContentStoreError::Malformed)
    );
    assert_eq!(store.usage().backing, 8);
    assert!(store.take_event().is_none());
    store
        .complete_sealed(grant(), begin.resource, Ok(validated()), 2)
        .unwrap();
    assert_eq!(store.usage().resident, 8);
}

#[test]
fn validation_is_bounded_and_never_certifies_an_unchecked_or_bad_tail() {
    let bytes = vec![0; SEALED_PIXEL_VALIDATION_BYTES + 4];
    let mut work = SealedPixelValidation::new(mapped(&bytes)).unwrap();
    assert!(!work.step().unwrap());
    assert!(work.finish().is_err());
    let mut bad = bytes.clone();
    bad[SEALED_PIXEL_VALIDATION_BYTES] = 1;
    let mut work = SealedPixelValidation::new(mapped(&bad)).unwrap();
    assert!(!work.step().unwrap());
    assert_eq!(work.step(), Err(ContentStoreError::Malformed));
    assert!(work.finish().is_err());
    let mut work = SealedPixelValidation::new(mapped(&bytes)).unwrap();
    assert!(!work.step().unwrap());
    assert!(work.step().unwrap());
    assert_eq!(work.finish().unwrap().bytes(), bytes);
}

#[test]
fn stopped_worker_holds_transfer_capacity_and_wrong_size_never_accepts() {
    let mut limits = ContentLimits::prototype(grant());
    limits.max_open_transfers = 1;
    let mut store = ContentResourceStore::new(limits).unwrap();
    store.begin_sealed(tx(), description(1), 0).unwrap();
    store
        .cancel(
            tx(),
            &ContentResourceCancel {
                grant: grant(),
                resource: description(1).resource,
            },
        )
        .unwrap();
    assert_eq!(
        store.begin_sealed(tx(), description(2), 1),
        Err(ContentStoreError::Budget)
    );
    assert!(
        store
            .complete_sealed(
                grant(),
                description(1).resource,
                Err(ContentStoreError::Malformed),
                1
            )
            .is_err()
    );
    store.take_event().unwrap();
    store.begin_sealed(tx(), description(3), 1).unwrap();
    let mut short = SealedPixelValidation::new(mapped(&[0; 4])).unwrap();
    assert!(short.step().unwrap());
    assert_eq!(
        store.complete_sealed(
            grant(),
            description(3).resource,
            Ok(short.finish().unwrap()),
            2
        ),
        Err(ContentStoreError::Malformed)
    );
    assert_eq!(store.usage(), ContentMemoryUsage::default());
    assert!(store.lease(grant(), description(3).resource).is_err());
}

#[test]
fn dead_epoch_needs_no_live_journal_confirmation_but_keeps_leases() {
    let mut store = owner();
    let resource = description(1).resource;
    store.begin_sealed(tx(), description(1), 0).unwrap();
    store
        .complete_sealed(grant(), resource, Ok(validated()), 1)
        .unwrap();
    store.take_event().unwrap();
    let lease = store.lease(grant(), resource).unwrap();
    store.revoke();
    assert_eq!(store.usage().backing, 8);
    assert!(!store.quiescent());
    assert_eq!(lease.bytes().len(), 8);
    drop(lease);
    store.collect();
    assert_eq!(store.usage(), ContentMemoryUsage::default());
    assert!(store.quiescent());

    let mut store = owner();
    store.begin_sealed(tx(), description(1), 0).unwrap();
    store
        .complete_sealed(grant(), resource, Ok(validated()), 1)
        .unwrap();
    store.take_event().unwrap();
    store
        .retire(
            tx(),
            &ContentResourceRetire {
                grant: grant(),
                resource,
            },
        )
        .unwrap();
    store.take_event().unwrap();
    assert_eq!(store.usage().backing, 8);
    store.revoke();
    assert_eq!(store.usage(), ContentMemoryUsage::default());
    assert!(store.quiescent());
}
