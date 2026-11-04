//! Joined admission uses the real bounded worker and owned-ring resource path.
use super::*;
use sophia_runtime::frame_credits::FrameStreamKey;
use std::cell::Cell;

#[path = "shell_frame_commit.rs"]
mod commits;
#[path = "shell_frame_images.rs"]
mod images;
#[path = "content_isb.rs"]
mod isb;

fn begin(generation: u64) -> ContentCandidateBegin {
    ContentCandidateBegin {
        grant: grant(),
        candidate_generation: generation,
        output: output(),
        facts_generation: 1,
        pacing_permit: 1,
        interaction_generation: 1,
        surface_count: 1,
        placement_count: 1,
        target_count: 0,
    }
}

fn open(registry: &mut ContentEpochRegistry) -> FrameStreamKey {
    let allocations = registry.allocations(grant()).unwrap().snapshots();
    let key = FrameStreamKey {
        grant: grant(),
        output: output(),
        allocation: request(1).allocation,
        facts_generation: 1,
        stream_id: 1,
    };
    let store = registry.active_candidates_mut(grant()).unwrap();
    store
        .open_frame_stream(
            tx(10),
            key,
            1,
            ContentCandidateContext {
                output: output(),
                facts_generation: 1,
                interaction_generation: 1,
                allocations: &allocations,
            },
            0,
        )
        .unwrap();
    store.take_frame_stream_event().unwrap();
    key
}

fn reserve(
    jobs: &mut OwnedCopyImageJobs,
    registry: &mut ContentEpochRegistry,
    worker: &mut IsbValidationWorker,
    ring: &OwnedRing,
    generation: u64,
) -> u64 {
    let result = jobs
        .reserve_frame_copy(
            registry,
            worker,
            ring,
            OwnedFrameCopyRequest {
                image: request(1),
                transaction: tx(11),
                begin: begin(generation),
            },
            64,
            1,
            deadline(),
            || {},
        )
        .unwrap();
    let OwnedFrameAdmission::Accepted { job } = result else {
        panic!("{result:?}");
    };
    job
}

fn end(registry: &mut ContentEpochRegistry, now: u64) -> Result<(), ContentCandidateError> {
    let allocations = registry.allocations(grant()).unwrap().snapshots();
    let (resources, store) = registry.active_pair_mut(grant()).unwrap();
    store.end(
        tx(13),
        ContentCandidateEnd {
            grant: grant(),
            candidate_generation: 1,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        },
        ContentCandidateContext {
            output: output(),
            facts_generation: 1,
            interaction_generation: 1,
            allocations: &allocations,
        },
        resources,
        now,
    )
}

fn scene(registry: &mut ContentEpochRegistry) {
    scene_with_resource(registry, request(1).resource, 1);
}

fn scene_with_resource(registry: &mut ContentEpochRegistry, resource: ContentResourceId, now: u64) {
    registry
        .active_candidates_mut(grant())
        .unwrap()
        .chunk(tx(12), scene_record(resource), now)
        .unwrap();
}

fn scene_record(resource: ContentResourceId) -> ContentCandidateChunk {
    ContentCandidateChunk {
        grant: grant(),
        candidate_generation: 1,
        chunk_ordinal: 0,
        surfaces: vec![ContentSurface {
            allocation: request(1).allocation,
            scale_generation: 1,
            role: 1,
            edge: 1,
            margins: ContentMargins::default(),
            reservation_extent: 20,
            parent_surface_index: u16::MAX,
            anchor_parent_rect: ContentPixelRect::default(),
        }],
        placements: vec![ContentPlacement {
            resource,
            surface_index: 0,
            destination_x_px: 0,
            destination_y_px: 0,
        }],
        targets: vec![],
    }
}

#[test]
fn response_and_worker_capacity_refuse_before_any_copy_charge() {
    let mut registry = image_registry();
    open(&mut registry);
    let (_, ring) = ring(&mut registry);
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let consumed = Cell::new(0);
    for (generation, room, expected) in [
        (1, 0, OwnedFrameAdmission::Deferred),
        (
            1,
            3,
            OwnedFrameAdmission::Refused(ContentCandidateError::Budget),
        ),
    ] {
        assert_eq!(
            jobs.reserve_frame_copy(
                &mut registry,
                &mut worker,
                &ring,
                OwnedFrameCopyRequest {
                    image: request(1),
                    transaction: tx(11),
                    begin: begin(generation)
                },
                room,
                1,
                deadline(),
                || consumed.set(consumed.get() + 1)
            )
            .unwrap(),
            expected
        );
        assert_eq!(jobs.pending(), 0);
        assert_eq!(worker.pending(), 0);
        assert_eq!(registry.resources(grant()).unwrap().usage().backing, 0);
    }
    assert_eq!(consumed.get(), 1);
    registry
        .active_candidates_mut(grant())
        .unwrap()
        .take_frame_stream_event()
        .unwrap();
    let occupied = worker.reserve(4, deadline()).unwrap();
    assert_eq!(
        jobs.reserve_frame_copy(
            &mut registry,
            &mut worker,
            &ring,
            OwnedFrameCopyRequest {
                image: request(1),
                transaction: tx(11),
                begin: begin(2)
            },
            64,
            1,
            deadline(),
            || {}
        )
        .unwrap(),
        OwnedFrameAdmission::Refused(ContentCandidateError::Budget)
    );
    assert_eq!(worker.pending(), 1);
    assert_eq!(registry.resources(grant()).unwrap().usage().backing, 0);
    assert!(worker.release_reserved(occupied));
    registry
        .active_candidates_mut(grant())
        .unwrap()
        .take_frame_stream_event()
        .unwrap();
    let _job = reserve(&mut jobs, &mut registry, &mut worker, &ring, 3);
    assert_eq!(worker.pending(), 1);
    assert_eq!(registry.resources(grant()).unwrap().usage().backing, 16000);
}

#[test]
fn timed_out_frame_retains_credit_until_exact_copy_handoff() {
    let mut registry = image_registry();
    open(&mut registry);
    let (file, mut ring) = ring(&mut registry);
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let job = reserve(&mut jobs, &mut registry, &mut worker, &ring, 1);
    publish(&file, request(1));
    assert!(
        jobs.submit(
            &mut worker,
            job,
            ring.offer(request(1).slot, 16000).unwrap()
        )
        .is_ok()
    );
    let now = u64::from(ContentLimits::prototype(grant()).candidate_timeout_ms) + 2;
    let candidates = registry.active_candidates_mut(grant()).unwrap();
    candidates.expire(now).unwrap();
    assert_eq!(
        candidates.pending_event_count(),
        0,
        "expiry settled a live reader"
    );
    assert!(!candidates.quiescent());
    assert_eq!(
        candidates.begin(tx(20), begin(2), now),
        Err(ContentCandidateError::Stale)
    );
    jobs.withdraw(
        &mut registry,
        &mut worker,
        job,
        IsbImageWithdrawal::Cancelled,
        now,
    )
    .unwrap();
    assert!(jobs.receive(completed(&mut worker)).is_ok());
    let mut settlement = jobs.reconcile(&mut registry, now).unwrap();
    assert_eq!(
        registry
            .active_candidates(grant())
            .unwrap()
            .pending_event_count(),
        0
    );
    settlement.finish_frame_copy(&mut registry, now).unwrap();
    settlement.finish_frame_copy(&mut registry, now).unwrap();
    let (resources, candidates) = registry.active_pair_mut(grant()).unwrap();
    assert_eq!(candidates.pending_event_count(), 1);
    assert!(
        candidates
            .transfer_frame_event_at(resources, now, |event| {
                let ShellContentRecord::CandidateOutcome(outcome) = &event.record else {
                    panic!();
                };
                assert_eq!(outcome.reason, ContentReason::Timeout as u16);
                assert!(event.frame_credit.unwrap().expires_msec.is_some());
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(ring.pending_release(0), Some(request(1).slot));
}

#[test]
fn successful_copy_must_handoff_before_the_complete_scene_can_be_admitted() {
    let mut registry = image_registry();
    open(&mut registry);
    let (file, mut ring) = ring(&mut registry);
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let job = reserve(&mut jobs, &mut registry, &mut worker, &ring, 1);
    scene(&mut registry);
    assert_eq!(end(&mut registry, 1), Err(ContentCandidateError::Budget));
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
    let mut settlement = jobs.reconcile(&mut registry, 2).unwrap();
    assert_eq!(settlement.outcome, Ok(()));
    assert!(
        matches!(
            registry
                .resources(grant())
                .unwrap()
                .lease(grant(), request(1).resource),
            Err(ContentStoreError::Stale)
        ),
        "copied frame image escaped before Presented transfer"
    );
    assert_eq!(end(&mut registry, 2), Err(ContentCandidateError::Budget));
    let allocations = registry.allocations(grant()).unwrap().snapshots();
    let (resources, store) = registry.active_pair_mut(grant()).unwrap();
    assert_eq!(
        store.end_with_retire(
            tx(13),
            ContentCandidateEnd {
                grant: grant(),
                candidate_generation: 1,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            },
            ContentRetireOnPresent {
                retire_transaction: tx(99),
                resource: request(99).resource,
            },
            ContentCandidateContext {
                output: output(),
                facts_generation: 1,
                interaction_generation: 1,
                allocations: &allocations,
            },
            resources,
            2,
        ),
        Err(ContentCandidateError::Budget),
        "End with retirement bypassed the copy handoff"
    );
    settlement.admission.worker_job += 1;
    assert_eq!(
        settlement.finish_frame_copy(&mut registry, 2),
        Err(ContentCandidateError::Stale)
    );
    settlement.admission.worker_job -= 1;
    settlement.admission.grant.connection_epoch += 1;
    assert_eq!(
        settlement.finish_frame_copy(&mut registry, 2),
        Err(ContentCandidateError::Stale)
    );
    settlement.admission.grant.connection_epoch -= 1;
    settlement.finish_frame_copy(&mut registry, 2).unwrap();
    end(&mut registry, 2).unwrap();
    let (resources, candidates) = registry.active_pair_mut(grant()).unwrap();
    assert_eq!(candidates.pending_candidate_count(), 1);
    let _bundle = candidates.begin_submission(output(), 1, 3).unwrap();
    candidates.prepared(output(), 1, 1, 1, 3).unwrap();
    assert!(
        candidates
            .transfer_frame_event_at(resources, 3, |_| Ok(()))
            .unwrap()
    );
    candidates.presented_at(output(), 1, 1, 1, 1, 4).unwrap();
    assert!(matches!(
        resources.lease(grant(), request(1).resource),
        Err(ContentStoreError::Stale)
    ));
    let retire = ContentResourceRetire {
        grant: grant(),
        resource: request(1).resource,
    };
    assert_eq!(
        resources.additional_response_credit(
            tx(30),
            &ShellContentRecord::ResourceRetire(retire.clone())
        ),
        1
    );
    assert_eq!(
        resources.retire(tx(30), &retire),
        Err(ContentStoreError::Stale)
    );
    assert_eq!(
        resources.reserve_retire(
            ContentRetireOwner {
                grant: grant(),
                output: output(),
                candidate_generation: 2
            },
            ContentRetireOnPresent {
                retire_transaction: tx(30),
                resource: request(1).resource
            },
            &ContentRetireToken::default()
        ),
        Err(ContentStoreError::Stale)
    );
    assert_eq!(
        candidates.transfer_frame_event_at(resources, 5, |_| Err(ContentCandidateError::Budget)),
        Err(ContentCandidateError::Budget)
    );
    assert!(matches!(
        resources.lease(grant(), request(1).resource),
        Err(ContentStoreError::Stale)
    ));
    let mut foreign = ContentResourceStore::new(ContentLimits::prototype(ContentGrant {
        connection_epoch: 2,
        content_grant_epoch: 2,
    }))
    .unwrap();
    assert_eq!(
        candidates.transfer_frame_event_at(&mut foreign, 5, |_| panic!(
            "published to wrong resource owner"
        )),
        Err(ContentCandidateError::Stale)
    );
    assert!(
        candidates
            .transfer_frame_event_at(resources, 5, |event| {
                assert!(event.frame_credit.unwrap().expires_msec.is_some());
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(
        resources
            .lease(grant(), request(1).resource)
            .unwrap()
            .cpu_bytes()
            .unwrap(),
        vec![1; 16000]
    );
    assert!(
        !candidates
            .transfer_frame_event_at(resources, 5, |_| panic!("duplicate transfer"))
            .unwrap()
    );
}

#[test]
fn a_stale_copy_tuple_and_foreign_source_do_not_spend_frame_identity() {
    let mut registry = image_registry();
    open(&mut registry);
    let (_, ring) = ring(&mut registry);
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let mut wrong = request(1);
    wrong.allocation.generation += 1;
    assert_eq!(
        jobs.reserve_frame_copy(
            &mut registry,
            &mut worker,
            &ring,
            OwnedFrameCopyRequest {
                image: wrong,
                transaction: tx(11),
                begin: begin(1)
            },
            64,
            1,
            deadline(),
            || {}
        )
        .unwrap(),
        OwnedFrameAdmission::Refused(ContentCandidateError::Stale)
    );
    assert_eq!(worker.pending(), 0);
    registry
        .active_candidates_mut(grant())
        .unwrap()
        .take_frame_stream_event()
        .unwrap();
    let mut foreign = image_registry();
    open(&mut foreign);
    assert_eq!(
        jobs.reserve_frame_copy(
            &mut foreign,
            &mut worker,
            &ring,
            OwnedFrameCopyRequest {
                image: request(1),
                transaction: tx(11),
                begin: begin(1)
            },
            64,
            1,
            deadline(),
            || panic!("consumed foreign source")
        )
        .unwrap_err(),
        ContentCandidateError::Stale
    );
    let _job = reserve(&mut jobs, &mut registry, &mut worker, &ring, 1);
}

#[test]
fn disconnected_copy_cannot_complete_a_replacement_grants_frame() {
    let mut registry = image_registry();
    open(&mut registry);
    let (file, mut ring) = ring(&mut registry);
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let job = reserve(&mut jobs, &mut registry, &mut worker, &ring, 1);
    publish(&file, request(1));
    assert!(
        jobs.submit(
            &mut worker,
            job,
            ring.offer(request(1).slot, 16000).unwrap()
        )
        .is_ok()
    );
    assert!(registry.disconnect(grant()));
    let replacement = ContentGrant {
        connection_epoch: 2,
        content_grant_epoch: 2,
    };
    add_image_epoch(&mut registry, replacement);
    jobs.withdraw_epoch(&mut registry, &mut worker, grant(), 2)
        .unwrap();
    assert!(jobs.receive(completed(&mut worker)).is_ok());
    let mut settlement = jobs.reconcile(&mut registry, 2).unwrap();
    settlement.finish_frame_copy(&mut registry, 2).unwrap();
    assert_eq!(
        registry
            .active_candidates(replacement)
            .unwrap()
            .pending_event_count(),
        0
    );
    assert!(registry.active_candidates(replacement).unwrap().quiescent());
    assert!(registry.resources(grant()).is_none());
}

#[test]
fn resource_refusal_rolls_back_the_worker_and_keeps_frame_credit() {
    let mut registry = image_registry();
    open(&mut registry);
    let (_, ring) = ring(&mut registry);
    let mut jobs = OwnedCopyImageJobs::default();
    let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
    let mut wrong = request(1);
    wrong.resource.generation = 2;
    assert_eq!(
        jobs.reserve_frame_copy(
            &mut registry,
            &mut worker,
            &ring,
            OwnedFrameCopyRequest {
                image: wrong,
                transaction: tx(11),
                begin: begin(1)
            },
            64,
            1,
            deadline(),
            || {}
        )
        .unwrap(),
        OwnedFrameAdmission::Refused(ContentCandidateError::Malformed)
    );
    assert_eq!(worker.pending(), 0);
    assert_eq!(jobs.pending(), 0);
    assert_eq!(registry.resources(grant()).unwrap().usage().backing, 0);
    registry
        .active_candidates_mut(grant())
        .unwrap()
        .take_frame_stream_event()
        .unwrap();
    let _job = reserve(&mut jobs, &mut registry, &mut worker, &ring, 2);
}

#[test]
fn unsent_cancellation_settles_once_and_close_forbids_replenishment() {
    for close in [false, true] {
        let mut registry = image_registry();
        let key = open(&mut registry);
        let (_, ring) = ring(&mut registry);
        let mut jobs = OwnedCopyImageJobs::default();
        let mut worker = IsbValidationWorker::new(1, 16000).unwrap();
        let job = reserve(&mut jobs, &mut registry, &mut worker, &ring, 1);
        if close {
            let store = registry.active_candidates_mut(grant()).unwrap();
            store.close_frame_stream(tx(30), key, 2).unwrap();
            store.take_frame_stream_event().unwrap();
        }
        jobs.withdraw(
            &mut registry,
            &mut worker,
            job,
            IsbImageWithdrawal::Cancelled,
            2,
        )
        .unwrap();
        assert_eq!(worker.pending(), 0);
        assert_eq!(
            registry
                .active_candidates(grant())
                .unwrap()
                .pending_event_count(),
            0
        );
        let mut settlement = jobs.reconcile(&mut registry, 2).unwrap();
        settlement.finish_frame_copy(&mut registry, 2).unwrap();
        let (resources, store) = registry.active_pair_mut(grant()).unwrap();
        assert!(
            store
                .transfer_frame_event_at(resources, 2, |event| {
                    let ShellContentRecord::CandidateOutcome(outcome) = &event.record else {
                        panic!();
                    };
                    assert_eq!(outcome.reason, ContentReason::Cancelled as u16);
                    assert_eq!(event.frame_credit.unwrap().expires_msec.is_none(), close);
                    Ok(())
                })
                .unwrap()
        );
        assert!(store.pending_event().is_none());
    }
}
