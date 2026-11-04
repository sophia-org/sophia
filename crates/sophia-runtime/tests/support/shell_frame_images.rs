//! Frame-owned pixels share the ordinary resource budgets and lease lifetime.
use super::*;

fn captured(registry: &mut ContentEpochRegistry) {
    open(registry);
    let (file, mut ring) = ring(registry);
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let job = reserve(&mut jobs, registry, &mut worker, &ring, 1);
    publish(&file, request(1));
    assert!(
        jobs.submit(
            &mut worker,
            job,
            ring.offer(request(1).slot, 16000).unwrap()
        )
        .is_ok()
    );
    assert!(jobs.receive(completed(&mut worker)).is_ok());
    let mut settlement = jobs.reconcile(registry, 2).unwrap();
    assert_eq!(settlement.outcome, Ok(()));
    settlement.finish_frame_copy(registry, 2).unwrap();
}

fn retained(resources: &mut ContentResourceStore) -> ContentResourceLease {
    isb::accept(
        resources,
        tx(50),
        ContentResourceBegin {
            grant: grant(),
            resource: request(2).resource,
            width_px: 200,
            height_px: 20,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 16000,
        },
        &[2; 16000],
        2,
    );
    resources.lease(grant(), request(2).resource).unwrap()
}

fn release(resources: &mut ContentResourceStore, id: u64) {
    resources.collect();
    assert_eq!(
        resources.confirm_isb_release(request(id).resource),
        Err(ContentStoreError::Incomplete)
    );
    assert!(matches!(resources.take_event().unwrap().record,
        ShellContentRecord::ResourceReleased(released) if released.resource == request(id).resource));
    resources.confirm_isb_release(request(id).resource).unwrap();
    assert!(resources.take_event().is_none());
}

#[test]
fn another_frame_generation_or_output_cannot_lease_the_private_image() {
    let mut registry = image_registry();
    captured(&mut registry);
    for other_output in [false, true] {
        let output = if other_output {
            ContentOutputId {
                id: 3,
                generation: 1,
            }
        } else {
            output()
        };
        let generation = if other_output { 1 } else { 2 };
        let mut allocations = registry.allocations(grant()).unwrap().snapshots();
        allocations[0].output = output;
        let context = ContentCandidateContext {
            output,
            facts_generation: 1,
            interaction_generation: 1,
            allocations: &allocations,
        };
        let mut candidates = ContentCandidateStore::new(ContentLimits::prototype(grant())).unwrap();
        candidates
            .open_frame_stream(
                tx(10),
                FrameStreamKey {
                    grant: grant(),
                    output,
                    allocation: request(1).allocation,
                    facts_generation: 1,
                    stream_id: 1,
                },
                1,
                context,
                0,
            )
            .unwrap();
        candidates.take_frame_stream_event().unwrap();
        let mut begin = begin(generation);
        begin.output = output;
        candidates.begin(tx(11), begin, 1).unwrap();
        let mut scene = scene_record(request(1).resource);
        scene.candidate_generation = generation;
        candidates.chunk(tx(12), scene, 1).unwrap();
        assert_eq!(
            candidates.end(
                tx(13),
                ContentCandidateEnd {
                    grant: grant(),
                    candidate_generation: generation,
                    surface_count: 1,
                    placement_count: 1,
                    target_count: 0,
                },
                context,
                registry.resources(grant()).unwrap(),
                2
            ),
            Err(ContentCandidateError::Stale)
        );
    }
    scene_with_resource(&mut registry, request(1).resource, 2);
    end(&mut registry, 2).unwrap();
}

#[test]
fn presented_transfer_promotes_new_pixels_and_retires_the_predecessor_together() {
    let mut registry = image_registry();
    captured(&mut registry);
    let old = retained(registry.resources_mut(grant()).unwrap());
    scene_with_resource(&mut registry, request(1).resource, 2);
    let allocations = registry.allocations(grant()).unwrap().snapshots();
    let (resources, candidates) = registry.active_pair_mut(grant()).unwrap();
    candidates
        .end_with_retire(
            tx(13),
            ContentCandidateEnd {
                grant: grant(),
                candidate_generation: 1,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            },
            ContentRetireOnPresent {
                retire_transaction: tx(51),
                resource: request(2).resource,
            },
            ContentCandidateContext {
                output: output(),
                facts_generation: 1,
                interaction_generation: 1,
                allocations: &allocations,
            },
            resources,
            2,
        )
        .unwrap();
    let bundle = candidates.begin_submission(output(), 1, 3).unwrap();
    candidates.prepared(output(), 1, 1, 1, 3).unwrap();
    candidates.take_event().unwrap();
    candidates.presented_at(output(), 1, 1, 1, 1, 4).unwrap();
    while resources.take_event().is_some() {}
    assert_eq!(
        candidates.transfer_frame_event_at(resources, 5, |_| Err(ContentCandidateError::Budget)),
        Err(ContentCandidateError::Budget)
    );
    assert_eq!(resources.reserved_retiring(), 16000);
    assert_eq!(resources.usage().retiring, 0);
    assert!(matches!(
        resources.lease(grant(), request(1).resource),
        Err(ContentStoreError::Stale)
    ));
    assert_eq!(old.cpu_bytes().unwrap(), &[2; 16000]);
    assert!(
        candidates
            .transfer_frame_event_at(resources, 6, |_| Ok(()))
            .unwrap()
    );
    assert_eq!(resources.reserved_retiring(), 0);
    assert_eq!(resources.usage().retiring, 16000);
    assert!(resources.lease(grant(), request(1).resource).is_ok());
    assert!(
        resources.take_event().is_none(),
        "old display lease is still held"
    );
    drop(old);
    release(resources, 2);
    assert_eq!(resources.usage().resident, 16000);
    assert_eq!(
        bundle
            .resource(request(1).resource)
            .unwrap()
            .cpu_bytes()
            .unwrap(),
        &[1; 16000]
    );
}

#[test]
fn failed_frame_releases_after_last_renderer_lease_even_with_retiring_budget_full() {
    let mut registry = image_registry();
    let mut limits = ContentLimits::prototype(grant());
    limits.max_retiring_bytes = 16000;
    limits.max_resource_bytes = 16000;
    *registry.resources_mut(grant()).unwrap() = ContentResourceStore::new(limits).unwrap();
    captured(&mut registry);
    let resources = registry.resources_mut(grant()).unwrap();
    let old = retained(resources);
    resources
        .retire(
            tx(51),
            &ContentResourceRetire {
                grant: grant(),
                resource: request(2).resource,
            },
        )
        .unwrap();
    while resources.take_event().is_some() {}
    assert_eq!(resources.usage().retiring, 16000);
    scene_with_resource(&mut registry, request(1).resource, 2);
    end(&mut registry, 2).unwrap();
    let (resources, candidates) = registry.active_pair_mut(grant()).unwrap();
    let bundle = candidates.begin_submission(output(), 1, 3).unwrap();
    candidates.renderer_failed_at(output(), 1, 4).unwrap();
    assert_eq!(
        candidates.transfer_frame_event_at(resources, 5, |_| Err(ContentCandidateError::Budget)),
        Err(ContentCandidateError::Budget)
    );
    assert!(resources.take_event().is_none());
    assert!(
        candidates
            .transfer_frame_event_at(resources, 6, |event| {
                let ShellContentRecord::CandidateOutcome(outcome) = &event.record else {
                    panic!()
                };
                assert_eq!(outcome.kind, 3);
                assert_eq!(outcome.reason, ContentReason::RendererFailed as u16);
                Ok(())
            })
            .unwrap()
    );
    assert!(
        resources.take_event().is_none(),
        "renderer still owns the private image"
    );
    assert!(matches!(
        resources.lease(grant(), request(1).resource),
        Err(ContentStoreError::Stale)
    ));
    assert_eq!(resources.usage().resident, 16000);
    assert_eq!(resources.usage().retiring, 16000);
    drop(bundle);
    release(resources, 1);
    assert_eq!(resources.usage().resident, 0);
    assert_eq!(resources.usage().backing, 16000);
    assert_eq!(old.cpu_bytes().unwrap(), &[2; 16000]);
    drop(old);
    release(resources, 2);
    assert_eq!(resources.usage(), ContentMemoryUsage::default());
}

#[test]
fn scene_cannot_omit_its_new_image_and_promote_it_through_an_unrelated_presentation() {
    let mut registry = image_registry();
    captured(&mut registry);
    let resources = registry.resources_mut(grant()).unwrap();
    let old = retained(resources);
    while resources.take_event().is_some() {}
    scene_with_resource(&mut registry, request(2).resource, 2);
    assert_eq!(end(&mut registry, 3), Err(ContentCandidateError::Malformed));
    let (resources, candidates) = registry.active_pair_mut(grant()).unwrap();
    assert_eq!(candidates.pending_candidate_count(), 0);
    assert!(
        candidates
            .transfer_frame_event_at(resources, 4, |_| Ok(()))
            .unwrap()
    );
    release(resources, 1);
    assert!(resources.lease(grant(), request(2).resource).is_ok());
    assert_eq!(old.cpu_bytes().unwrap(), &[2; 16000]);
    assert_eq!(resources.usage().resident, 16000);
}

#[test]
fn assembly_and_pending_expiry_release_the_private_image_only_after_terminal_transfer() {
    for pending in [false, true] {
        let mut registry = image_registry();
        captured(&mut registry);
        if pending {
            scene_with_resource(&mut registry, request(1).resource, 2);
            end(&mut registry, 2).unwrap();
        }
        let (resources, candidates) = registry.active_pair_mut(grant()).unwrap();
        while resources.take_event().is_some() {}
        candidates.expire(10000).unwrap();
        while candidates.take_frame_stream_event().is_some() {}
        assert!(resources.take_event().is_none());
        assert_eq!(resources.usage().resident, 16000);
        assert!(
            candidates
                .transfer_frame_event_at(resources, 10000, |event| {
                    let ShellContentRecord::CandidateOutcome(outcome) = &event.record else {
                        panic!()
                    };
                    assert_eq!(outcome.reason, ContentReason::Timeout as u16);
                    Ok(())
                })
                .unwrap()
        );
        release(resources, 1);
        assert_eq!(resources.usage(), ContentMemoryUsage::default());
    }
}

#[test]
fn disconnect_reclaims_private_storage_without_publishing_into_the_replacement() {
    let mut registry = image_registry();
    captured(&mut registry);
    scene_with_resource(&mut registry, request(1).resource, 2);
    end(&mut registry, 2).unwrap();
    let bundle = registry
        .active_candidates_mut(grant())
        .unwrap()
        .begin_submission(output(), 1, 3)
        .unwrap();
    assert!(registry.disconnect(grant()));
    let replacement = ContentGrant {
        connection_epoch: 2,
        content_grant_epoch: 2,
    };
    add_image_epoch(&mut registry, replacement);
    assert_eq!(registry.accounting().memory.resident, 16000);
    registry
        .candidates_mut(grant())
        .unwrap()
        .renderer_failed(output(), 1)
        .unwrap();
    registry.collect();
    assert_eq!(registry.accounting().memory.resident, 16000);
    drop(bundle);
    registry.collect();
    assert_eq!(registry.accounting().memory.resident, 0);
    assert!(registry.resources(grant()).is_none());
    assert!(
        registry
            .resources_mut(replacement)
            .unwrap()
            .take_event()
            .is_none()
    );
    assert_eq!(
        registry
            .active_candidates(replacement)
            .unwrap()
            .pending_event_count(),
        0
    );
}
