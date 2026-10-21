//! Owned-copy jobs use actual allocation, resource and worker owners.
use super::*;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::time::{Duration, Instant};

fn ring(registry: &mut ContentEpochRegistry) -> (File, OwnedRing) {
    let geometry = OwnedRingGeometry::new(1, 16000).unwrap();
    let file = File::from(
        rustix::fs::memfd_create("owned-job-test", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    file.set_len(geometry.source_bytes() as u64).unwrap();
    let reservation = registry
        .reserve_owned_ring_source(grant(), geometry)
        .unwrap();
    let ring = OwnedRing::admit(file.try_clone().unwrap(), reservation).unwrap();
    (file, ring)
}
fn request(offer: u64) -> OwnedCopyImageRequest {
    OwnedCopyImageRequest {
        slot: OwnedSlotIdentity {
            offer,
            slot: 0,
            generation: offer * 2,
        },
        resource: ContentResourceId {
            id: offer,
            generation: 1,
        },
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        output: facts(100).output,
        topology_generation: 1,
        width: 200,
        height: 20,
    }
}
fn publish(file: &File, request: OwnedCopyImageRequest) {
    file.write_all_at(&vec![request.slot.offer as u8; 16000], 4096)
        .unwrap();
    file.write_all_at(&request.slot.generation.to_le_bytes(), 0)
        .unwrap();
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn completed(worker: &mut IsbValidationWorker) -> IsbValidationCompletion {
    let bound = deadline();
    loop {
        if let Some(done) = worker.take_completed().unwrap() {
            return done;
        }
        assert!(Instant::now() < bound, "worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn slot_release_allows_new_copies_while_prior_resource_leases_remain_immutable() {
    let mut registry = image_registry();
    let (file, mut ring) = ring(&mut registry);
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let mut jobs = OwnedCopyImageJobs::default();
    let mut leases = Vec::new();
    for offer in 1..=3 {
        let request = request(offer);
        let job = jobs
            .reserve(
                &mut registry,
                &mut worker,
                &ring,
                request,
                offer,
                deadline(),
            )
            .unwrap();
        publish(&file, request);
        assert!(
            jobs.submit(&mut worker, job, ring.offer(request.slot, 16000).unwrap())
                .is_ok()
        );
        assert!(jobs.receive(completed(&mut worker)).is_ok());
        // Source release neither settles nor retires its owned destination.
        assert_eq!(ring.pending_release(0), Some(request.slot));
        assert!(ring.confirm_release(request.slot));
        assert_eq!(jobs.pending(), 1);
        let settled = jobs.reconcile(&mut registry, offer).unwrap();
        assert_eq!(settled.outcome, Ok(()));
        let store = registry.resources_mut(grant()).unwrap();
        assert!(matches!(
            store.take_event().unwrap().record,
            ShellContentRecord::ResourceStatus(_)
        ));
        leases.push(store.lease(grant(), request.resource).unwrap());
        store
            .retire(
                tx(offer),
                &ContentResourceRetire {
                    grant: grant(),
                    resource: request.resource,
                },
            )
            .unwrap();
        store.collect();
        assert!(store.take_event().is_none());
        assert_eq!(store.usage().backing, offer * 16000);
    }
    drop(ring);
    file.set_len(0).unwrap();
    for (index, lease) in leases.iter().enumerate() {
        assert_eq!(lease.cpu_bytes().unwrap(), vec![(index + 1) as u8; 16000]);
    }
    drop(leases);
    let store = registry.resources_mut(grant()).unwrap();
    store.collect();
    for offer in 1..=3 {
        assert!(matches!(
            store.take_event().unwrap().record,
            ShellContentRecord::ResourceReleased(_)
        ));
        store.confirm_isb_release(request(offer).resource).unwrap();
    }
    assert_eq!(store.usage().backing, 0);
    assert_eq!(registry.accounting().source_bytes, 0);
}

#[test]
fn slot_capacity_is_not_copied_or_charged_as_resource_bytes() {
    let mut registry = image_registry();
    let (file, mut ring) = ring(&mut registry);
    let mut request = request(1);
    request.height = 10;
    let pixels = request.pixel_bytes().unwrap();
    assert_eq!(pixels, 8000);
    // Invalid premultiplied padding must not enter the owned image or scan.
    file.write_all_at(&[255, 0, 0, 0].repeat(4000), 4096)
        .unwrap();
    file.write_all_at(&vec![1; pixels], 4096).unwrap();
    file.write_all_at(&request.slot.generation.to_le_bytes(), 0)
        .unwrap();
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let job = jobs
        .reserve(&mut registry, &mut worker, &ring, request, 0, deadline())
        .unwrap();
    assert_eq!(
        registry.resources(grant()).unwrap().usage().staging,
        pixels as u64
    );
    assert!(
        jobs.submit(&mut worker, job, ring.offer(request.slot, pixels).unwrap())
            .is_ok()
    );
    assert!(jobs.receive(completed(&mut worker)).is_ok());
    assert_eq!(jobs.reconcile(&mut registry, 1).unwrap().outcome, Ok(()));
    let store = registry.resources_mut(grant()).unwrap();
    assert_eq!(store.usage().backing, pixels as u64);
    let lease = store.lease(grant(), request.resource).unwrap();
    assert_eq!(lease.cpu_bytes().unwrap(), vec![1; pixels]);
    assert_eq!(ring.pending_release(0), Some(request.slot));
}

#[test]
fn another_ring_or_slot_generation_cannot_supply_a_reserved_job() {
    let mut registry = image_registry();
    let (_, mut original) = ring(&mut registry);
    let (_, mut other) = ring(&mut registry);
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let wanted = request(1);
    let job = jobs
        .reserve(&mut registry, &mut worker, &original, wanted, 0, deadline())
        .unwrap();
    for wrong_ring in [true, false] {
        let source_ring = if wrong_ring {
            &mut other
        } else {
            &mut original
        };
        let wrong = if wrong_ring {
            wanted.slot
        } else {
            request(2).slot
        };
        let read = source_ring.offer(wrong, 16000).unwrap();
        let (error, returned) = jobs.submit(&mut worker, job, read).err().unwrap();
        assert_eq!(error, IsbSubmitFailure::Invalid);
        assert_eq!(source_ring.pending_release(0), None);
        drop(returned);
        assert!(source_ring.confirm_release(wrong));
        assert_eq!(worker.pending(), 1);
    }
    jobs.withdraw(
        &mut registry,
        &mut worker,
        job,
        IsbImageWithdrawal::Cancelled,
        1,
    )
    .unwrap();
    assert_eq!(
        jobs.reconcile(&mut registry, 1).unwrap().stopped,
        Some(ContentReason::Cancelled)
    );
    assert_eq!(registry.resources(grant()).unwrap().usage().backing, 0);
}

#[test]
fn admission_refuses_wrong_destination_allocation_and_slot_before_any_charge() {
    let mut registry = image_registry();
    let (_, ring) = ring(&mut registry);
    let baseline = registry.accounting();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let mut jobs = OwnedCopyImageJobs::default();
    for case in 0..7 {
        let mut request = request(1);
        match case {
            0 => request.width = 201,
            1 => request.allocation.generation = 2,
            2 => request.output.generation += 1,
            3 => request.topology_generation += 1,
            4 => request.slot.slot = 1,
            5 => request.slot.generation = 3,
            _ => request.resource.generation = 0,
        }
        assert!(
            jobs.reserve(&mut registry, &mut worker, &ring, request, 0, deadline())
                .is_err(),
            "case {case}"
        );
        assert_eq!(worker.pending(), 0);
        assert_eq!(jobs.pending(), 0);
        assert_eq!(registry.accounting(), baseline);
    }
}

#[test]
fn matching_epoch_numbers_do_not_transfer_source_custody_between_registries() {
    let mut source_owner = image_registry();
    let mut other_owner = image_registry();
    let (_, ring) = ring(&mut source_owner);
    let before_source = source_owner.accounting();
    let before_other = other_owner.accounting();
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    assert_eq!(
        jobs.reserve(
            &mut other_owner,
            &mut worker,
            &ring,
            request(1),
            0,
            deadline()
        ),
        Err(IsbImageAdmissionError::Resource(ContentStoreError::Stale))
    );
    assert_eq!(worker.pending(), 0);
    assert_eq!(jobs.pending(), 0);
    assert_eq!(source_owner.accounting(), before_source);
    assert_eq!(other_owner.accounting(), before_other);
    assert!(
        jobs.reserve(
            &mut source_owner,
            &mut worker,
            &ring,
            request(1),
            0,
            deadline()
        )
        .is_ok()
    );
}

#[test]
fn completed_copy_cannot_be_accepted_after_cancel_expiry_topology_change_or_disconnect() {
    for cause in 0..4 {
        let mut registry = image_registry();
        let (file, mut ring) = ring(&mut registry);
        let mut jobs = OwnedCopyImageJobs::default();
        let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
        let request = request(1);
        let expiry = deadline();
        let job = jobs
            .reserve(&mut registry, &mut worker, &ring, request, 10, expiry)
            .unwrap();
        publish(&file, request);
        assert!(
            jobs.submit(&mut worker, job, ring.offer(request.slot, 16000).unwrap())
                .is_ok()
        );
        let done = completed(&mut worker);
        assert!(done.result.is_ok());
        assert!(jobs.receive(done).is_ok());
        // Result custody survives a regressed owner clock.
        assert!(jobs.reconcile(&mut registry, 9).is_none());
        assert_eq!(jobs.pending(), 1);
        match cause {
            0 => jobs
                .withdraw(
                    &mut registry,
                    &mut worker,
                    job,
                    IsbImageWithdrawal::Cancelled,
                    11,
                )
                .unwrap(),
            1 => jobs
                .withdraw(
                    &mut registry,
                    &mut worker,
                    job,
                    IsbImageWithdrawal::Expired,
                    11,
                )
                .unwrap(),
            2 => registry
                .allocations_mut(grant())
                .unwrap()
                .publish_outputs(tx(9), 2, vec![facts(100)])
                .unwrap(),
            _ => assert!(registry.disconnect(grant())),
        }
        let result = jobs.reconcile(&mut registry, 12).unwrap();
        assert!(result.outcome.is_err());
        assert_eq!(
            result.stopped,
            Some(match cause {
                0 => ContentReason::Cancelled,
                1 => ContentReason::Timeout,
                2 => ContentReason::AllocationLost,
                _ => ContentReason::Revoked,
            })
        );
        // A completed resource refusal must not consume the source release.
        assert_eq!(ring.pending_release(0), Some(request.slot));
        drop(ring);
        registry.collect();
        assert_eq!(registry.accounting().source_bytes, 0);
        assert_eq!(jobs.pending(), 0);
        assert!(
            jobs.receive(IsbValidationCompletion {
                job,
                result: Err(IsbValidationFailure::Cancelled)
            })
            .is_err()
        );
    }
}
