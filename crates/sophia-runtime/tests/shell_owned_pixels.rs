//! D4's immutable destination in the real store; no ring/wire/native claim.
use sophia_protocol::*;
use sophia_runtime::*;

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
fn validated(bytes: Box<[u8]>) -> ValidatedCpuPixels {
    let mut scan = CpuPixelValidation::from_owned(bytes).unwrap();
    while !scan.step().unwrap() {}
    scan.finish().unwrap()
}

#[test]
fn owned_copy_keeps_allocation_and_charge_through_consumer_and_journal_release() {
    let mut store = ContentResourceStore::new(ContentLimits::prototype(grant())).unwrap();
    let begin = description(1);
    store.begin_isb(tx(), begin.clone(), 0).unwrap();
    let bytes = Box::new([1, 2, 3, 255, 4, 5, 6, 255]);
    let address = bytes.as_ptr();
    let pixels = validated(bytes);
    assert_eq!(pixels.bytes().as_ptr(), address);
    let mut result = Some(Ok(pixels));
    // A wrong owner cannot consume the worker result or its reservation.
    assert_eq!(
        store.complete_isb(grant(), description(2).resource, &mut result, 0),
        Err(ContentStoreError::Stale)
    );
    assert!(result.is_some());
    store
        .complete_isb(grant(), begin.resource, &mut result, 1)
        .unwrap();
    assert!(result.is_none());
    assert_eq!(store.usage().resident, 8);
    assert_eq!(store.usage().backing, 8);
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::ResourceStatus(_)
    ));
    let lease = store.lease(grant(), begin.resource).unwrap();
    let renderer = lease.clone().into_cpu().ok().unwrap();
    assert_eq!(renderer.bytes().as_ptr(), address);
    store
        .retire(
            tx(),
            &ContentResourceRetire {
                grant: grant(),
                resource: begin.resource,
            },
        )
        .unwrap();
    drop(lease);
    store.collect();
    assert!(store.take_event().is_none());
    assert_eq!(renderer.bytes(), &[1, 2, 3, 255, 4, 5, 6, 255]);
    drop(renderer);
    store.collect();
    assert_eq!(store.usage().backing, 8);
    assert_eq!(
        store.confirm_isb_release(begin.resource),
        Err(ContentStoreError::Incomplete)
    );
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::ResourceReleased(_)
    ));
    assert_eq!(store.usage().backing, 8);
    store.confirm_isb_release(begin.resource).unwrap();
    assert_eq!(store.usage(), ContentMemoryUsage::default());
}

#[test]
fn owned_shared_view_keeps_the_box_without_a_second_pixel_copy() {
    let bytes = vec![255; 65540].into_boxed_slice();
    let address = bytes.as_ptr();
    let shared = validated(bytes).into_shared();
    let watch = shared.watch();
    let renderer = shared.clone();
    drop(shared);
    assert!(!watch.released());
    assert_eq!(renderer.as_ptr(), address);
    assert!(renderer.iter().all(|&b| b == 255));
    drop(renderer);
    assert!(watch.released());
}

#[test]
fn owned_scan_refuses_unchecked_tails_and_invalid_premultiplication() {
    let mut incomplete = CpuPixelValidation::from_owned(vec![0; 65540].into_boxed_slice()).unwrap();
    assert!(!incomplete.step().unwrap());
    assert!(matches!(
        incomplete.finish(),
        Err(ContentStoreError::Incomplete)
    ));
    let mut bytes = vec![0; 65540];
    bytes[65536] = 1;
    let mut bad = CpuPixelValidation::from_owned(bytes.into_boxed_slice()).unwrap();
    assert!(!bad.step().unwrap());
    assert_eq!(bad.step(), Err(ContentStoreError::Malformed));
    assert!(bad.finish().is_err());
    for len in [1, 2, 3, 5, 6, 7] {
        assert!(matches!(
            CpuPixelValidation::from_owned(vec![0; len].into_boxed_slice()),
            Err(ContentStoreError::Malformed)
        ));
    }
}

#[test]
fn each_owned_generation_reserves_bytes_and_stopped_work_keeps_its_charge() {
    for stop in 0..3 {
        let mut limits = ContentLimits::prototype(grant());
        limits.max_open_transfers = 1;
        let mut store = ContentResourceStore::new(limits).unwrap();
        let begin = description(1);
        store.begin_isb(tx(), begin.clone(), 0).unwrap();
        let result = validated(Box::new([0; 8]));
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
        assert_eq!(store.usage().backing, 8);
        assert_eq!(store.usage().staging, 8);
        if stop != 2 {
            assert_eq!(
                store.begin_isb(tx(), description(2), if stop == 1 { 2000 } else { 0 }),
                Err(ContentStoreError::Budget)
            );
        }
        assert!(
            store
                .complete_isb(
                    grant(),
                    begin.resource,
                    &mut Some(Ok(result)),
                    if stop == 1 { 2000 } else { 1 }
                )
                .is_err()
        );
        assert_eq!(store.usage(), ContentMemoryUsage::default());
        assert!(store.quiescent());
    }
}

#[test]
fn owned_resources_have_independent_generations_and_resident_capacity() {
    let mut limits = ContentLimits::prototype(grant());
    limits.max_resident_bytes = 16;
    limits.max_resource_bytes = 8;
    let mut store = ContentResourceStore::new(limits).unwrap();
    let mut leases = Vec::new();
    for id in 1..=2 {
        let begin = description(id);
        store.begin_isb(tx(), begin.clone(), 0).unwrap();
        store
            .complete_isb(
                grant(),
                begin.resource,
                &mut Some(Ok(validated(Box::new([id as u8; 8])))),
                0,
            )
            .unwrap();
        store.take_event().unwrap();
        leases.push(store.lease(grant(), begin.resource).unwrap());
    }
    assert_eq!(store.usage().resident, 16);
    assert_eq!(store.usage().backing, 16);
    assert_eq!(
        store.begin_isb(tx(), description(3), 0),
        Err(ContentStoreError::Budget)
    );
    assert_ne!(
        leases[0].cpu_bytes().unwrap().as_ptr(),
        leases[1].cpu_bytes().unwrap().as_ptr()
    );
    assert_eq!(leases[0].cpu_bytes().unwrap(), &[1; 8]);
    assert_eq!(leases[1].cpu_bytes().unwrap(), &[2; 8]);
    // Disconnect cannot free retained renderer bytes, even without a live journal.
    store.revoke();
    store.collect();
    assert_eq!(store.usage().backing, 16);
    drop(leases);
    store.collect();
    assert_eq!(store.usage(), ContentMemoryUsage::default());
}
