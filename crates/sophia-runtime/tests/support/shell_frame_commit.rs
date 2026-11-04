//! Complete input uses real copy/lease owners and never asks the peer for End.
use super::*;

#[path = "shell_frame_native_profile.rs"]
mod native_profile;

struct Fixture {
    registry: ContentEpochRegistry,
    jobs: OwnedCopyImageJobs,
    worker: IsbValidationWorker,
    file: File,
    ring: OwnedRing,
}

impl Fixture {
    fn new() -> Self {
        Self::with_candidate_bytes(ContentLimits::prototype(grant()).max_candidate_bytes)
    }

    fn with_candidate_bytes(bytes: u32) -> Self {
        let mut registry = image_registry();
        let mut limits = ContentLimits::prototype(grant());
        limits.max_candidate_bytes = bytes;
        *registry.active_candidates_mut(grant()).unwrap() =
            ContentCandidateStore::new(limits).unwrap();
        let resources = registry.resources_mut(grant()).unwrap();
        for id in [1, 2] {
            isb::accept(
                resources,
                tx(id),
                ContentResourceBegin {
                    grant: grant(),
                    resource: request(id).resource,
                    width_px: 200,
                    height_px: 20,
                    rendered_scale_numerator: 1,
                    rendered_scale_denominator: 1,
                    pixel_format: 1,
                    chunk_count: 1,
                    total_bytes: 16000,
                },
                &[1; 16000],
                0,
            );
        }
        while resources.take_event().is_some() {}
        native_profile::open(&mut registry);
        let (file, ring) = native_profile::ring(&mut registry, 2);
        Self {
            registry,
            jobs: OwnedCopyImageJobs::default(),
            worker: IsbValidationWorker::new(1, 16000).unwrap(),
            file,
            ring,
        }
    }

    fn admit(
        &mut self,
        input: &mut Option<ContentFrameCommit>,
        room: usize,
        now: u64,
    ) -> ContentFrameAdmission {
        self.jobs
            .admit_frame_commit(
                &mut self.registry,
                &mut self.worker,
                Some(&self.ring),
                input,
                room,
                now,
                deadline(),
            )
            .unwrap()
    }

    fn copy(&mut self, job: u64) {
        publish(&self.file, request(3));
        assert!(
            self.jobs
                .submit(
                    &mut self.worker,
                    job,
                    self.ring.offer(request(3).slot, 16000).unwrap()
                )
                .is_ok()
        );
        assert!(self.jobs.receive(completed(&mut self.worker)).is_ok());
        let mut settlement = self.jobs.reconcile(&mut self.registry, 2).unwrap();
        assert_eq!(settlement.outcome, Ok(()));
        settlement.finish_frame_copy(&mut self.registry, 2).unwrap();
    }

    fn finish(
        &mut self,
        generation: u64,
        facts: u64,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        let allocations = self.registry.allocations(grant()).unwrap().snapshots();
        let (resources, candidates) = self.registry.active_pair_mut(grant()).unwrap();
        candidates.finish_frame_commit(
            output(),
            generation,
            ContentCandidateContext {
                output: output(),
                facts_generation: facts,
                interaction_generation: 1,
                allocations: &allocations,
            },
            resources,
            now,
        )
    }

    fn refusal(&mut self, generation: u64, reason: ContentReason) {
        let event = self
            .registry
            .active_candidates_mut(grant())
            .unwrap()
            .take_frame_stream_event()
            .unwrap();
        assert!(
            matches!(event.record, ContentFrameStreamRecord::Refused { candidate_generation, reason: actual, .. }
            if candidate_generation == generation && actual == reason)
        );
    }
}

fn frame(generation: u64, copying: bool) -> ContentFrameCommit {
    let rows = scene_record(request(if copying { 3 } else { 2 }).resource);
    ContentFrameCommit {
        transaction: tx(11),
        scene: ContentCandidate {
            grant: grant(),
            candidate_generation: generation,
            output: output(),
            facts_generation: 1,
            pacing_permit: 1,
            interaction_generation: 1,
            surfaces: rows.surfaces,
            placements: rows.placements,
            targets: rows.targets,
        },
        image: copying.then(|| request(3)),
        retire: Some(ContentRetireOnPresent {
            retire_transaction: tx(50),
            resource: request(1).resource,
        }),
    }
}

fn job(result: ContentFrameAdmission) -> u64 {
    let ContentFrameAdmission::Accepted {
        copy_job: Some(job),
    } = result
    else {
        panic!("{result:?}")
    };
    job
}

#[test]
fn complete_input_waits_for_reply_room_and_rolls_back_intent_when_worker_refuses() {
    let mut f = Fixture::new();
    let mut input = Some(frame(1, true));
    let original = input.as_ref().unwrap().scene.placements.as_ptr();
    let before = f.registry.accounting();
    assert_eq!(f.admit(&mut input, 0, 1), ContentFrameAdmission::Deferred);
    assert_eq!(input.as_ref().unwrap().scene.placements.as_ptr(), original);
    assert_eq!(f.registry.accounting(), before);
    assert_eq!(f.worker.pending(), 0);
    assert_eq!(
        f.admit(&mut input, 3, 1),
        ContentFrameAdmission::Refused(ContentCandidateError::Budget)
    );
    assert!(input.is_none());
    f.refusal(1, ContentReason::Budget);
    let occupied = f.worker.reserve(4, deadline()).unwrap();
    let mut input = Some(frame(2, true));
    assert_eq!(
        f.admit(&mut input, 4, 1),
        ContentFrameAdmission::Refused(ContentCandidateError::Budget)
    );
    assert!(input.is_none());
    let resources = f.registry.resources(grant()).unwrap();
    assert_eq!(resources.reserved_retiring(), 0);
    assert!(resources.lease(grant(), request(1).resource).is_ok());
    assert_eq!(resources.usage().reserved_resident, 0);
    assert_eq!(f.jobs.pending(), 0);
    f.refusal(2, ContentReason::Budget);
    assert!(f.worker.release_reserved(occupied));
    let mut input = Some(frame(3, true));
    let _job = job(f.admit(&mut input, 4, 1));
    assert!(input.is_none());
    assert_eq!(f.worker.pending(), 1);
    assert_eq!(
        f.registry.resources(grant()).unwrap().reserved_retiring(),
        16000
    );
    assert!(matches!(
        f.registry
            .resources(grant())
            .unwrap()
            .lease(grant(), request(1).resource),
        Err(ContentStoreError::Stale)
    ));
    assert_eq!(
        f.ring.pending_release(0),
        None,
        "admission must not read the source"
    );
}

#[test]
fn full_scene_and_predecessor_survive_copy_close_and_native_handoff() {
    for close in [false, true] {
        let mut f = Fixture::new();
        let old = f
            .registry
            .resources(grant())
            .unwrap()
            .lease(grant(), request(1).resource)
            .unwrap();
        let mut input = frame(1, true);
        input
            .scene
            .placements
            .push(scene_record(request(2).resource).placements.remove(0));
        let mut input = Some(input);
        let job = job(f.admit(&mut input, 4, 1));
        assert_eq!(f.finish(1, 1, 1), Err(ContentCandidateError::Budget));
        let candidates = f.registry.active_candidates_mut(grant()).unwrap();
        assert_eq!(candidates.ready_frame_commit(), None);
        assert_eq!(
            candidates.chunk(tx(12), scene_record(request(2).resource), 1),
            Err(ContentCandidateError::Stale)
        );
        if close {
            candidates
                .close_frame_stream(
                    tx(40),
                    FrameStreamKey {
                        grant: grant(),
                        output: output(),
                        allocation: request(3).allocation,
                        facts_generation: 1,
                        stream_id: 1,
                    },
                    1,
                )
                .unwrap();
            candidates.take_frame_stream_event().unwrap();
        }
        f.copy(job);
        assert_eq!(f.ring.pending_release(0), Some(request(3).slot));
        assert_eq!(
            f.registry
                .active_candidates(grant())
                .unwrap()
                .ready_frame_commit(),
            Some((output(), 1))
        );
        let allocations = f.registry.allocations(grant()).unwrap().snapshots();
        let (resources, candidates) = f.registry.active_pair_mut(grant()).unwrap();
        assert_eq!(
            candidates.end_with_retire(
                tx(60),
                ContentCandidateEnd {
                    grant: grant(),
                    candidate_generation: 1,
                    surface_count: 1,
                    placement_count: 2,
                    target_count: 0,
                },
                ContentRetireOnPresent {
                    retire_transaction: tx(61),
                    resource: request(2).resource
                },
                ContentCandidateContext {
                    output: output(),
                    facts_generation: 1,
                    interaction_generation: 1,
                    allocations: &allocations
                },
                resources,
                2
            ),
            Err(ContentCandidateError::Stale)
        );
        assert_eq!(resources.reserved_retiring(), 16000);
        f.finish(1, 1, 2).unwrap();
        let (resources, candidates) = f.registry.active_pair_mut(grant()).unwrap();
        assert_eq!(candidates.ready_frame_commit(), None);
        let bundle = candidates.begin_submission(output(), 1, 3).unwrap();
        assert_eq!(bundle.placements.len(), 2);
        assert_eq!(bundle.placements[0].resource, request(3).resource);
        assert_eq!(bundle.placements[1].resource, request(2).resource);
        assert!(bundle.resource(request(1).resource).is_none());
        candidates.prepared(output(), 1, 1, 1, 3).unwrap();
        assert!(
            candidates
                .transfer_frame_event_at(resources, 3, |_| Ok(()))
                .unwrap()
        );
        candidates.presented_at(output(), 1, 1, 1, 1, 4).unwrap();
        assert_eq!(
            candidates
                .transfer_frame_event_at(resources, 5, |_| Err(ContentCandidateError::Budget)),
            Err(ContentCandidateError::Budget)
        );
        assert_eq!(resources.reserved_retiring(), 16000);
        assert_eq!(resources.usage().retiring, 0);
        assert!(matches!(
            resources.lease(grant(), request(3).resource),
            Err(ContentStoreError::Stale)
        ));
        assert!(
            candidates
                .transfer_frame_event_at(resources, 6, |event| {
                    assert_eq!(event.frame_credit.unwrap().expires_msec.is_none(), close);
                    Ok(())
                })
                .unwrap()
        );
        assert_eq!(resources.reserved_retiring(), 0);
        assert_eq!(resources.usage().retiring, 16000);
        assert!(resources.lease(grant(), request(3).resource).is_ok());
        assert!(resources.lease(grant(), request(2).resource).is_ok());
        assert_eq!(old.cpu_bytes().unwrap(), &[1; 16000]);
    }
}

#[test]
fn retained_only_scene_needs_no_ring_or_worker_slot() {
    let mut f = Fixture::new();
    f.worker = IsbValidationWorker::new(4, 64000).unwrap();
    let occupied = [3, 4, 5, 6].map(|id| {
        f.jobs
            .reserve(
                &mut f.registry,
                &mut f.worker,
                &f.ring,
                request(id),
                1,
                deadline(),
            )
            .unwrap()
    });
    let mut input = Some(frame(1, false));
    let before = f.registry.resources(grant()).unwrap().usage();
    assert_eq!(
        f.jobs
            .admit_frame_commit(
                &mut f.registry,
                &mut f.worker,
                None,
                &mut input,
                2,
                1,
                deadline()
            )
            .unwrap(),
        ContentFrameAdmission::Accepted { copy_job: None }
    );
    assert!(input.is_none());
    assert_eq!(f.worker.pending(), 4);
    assert_eq!(f.jobs.pending(), 4);
    assert_eq!(f.registry.resources(grant()).unwrap().usage(), before);
    assert_eq!(
        f.registry
            .active_candidates(grant())
            .unwrap()
            .ready_frame_commit(),
        Some((output(), 1))
    );
    f.finish(1, 1, 2).unwrap();
    let bundle = f
        .registry
        .active_candidates_mut(grant())
        .unwrap()
        .begin_submission(output(), 1, 3)
        .unwrap();
    assert_eq!(bundle.placements.len(), 1);
    assert!(bundle.resource(request(2).resource).is_some());
    for job in occupied {
        f.jobs
            .withdraw(
                &mut f.registry,
                &mut f.worker,
                job,
                IsbImageWithdrawal::Cancelled,
                3,
            )
            .unwrap();
        f.jobs.reconcile(&mut f.registry, 3).unwrap();
    }
    assert_eq!(f.worker.pending(), 0);
    assert_eq!(f.jobs.pending(), 0);
}

#[test]
fn shape_refusals_happen_before_copy_or_predecessor_reservation() {
    for invalid in 0..5 {
        let mut f = Fixture::new();
        let mut frame = frame(1, true);
        match invalid {
            0 => frame.scene.placements[0].resource = request(2).resource,
            1 => frame.scene.surfaces[0].role = 0,
            2 => frame.scene.surfaces[0].margins.top = i16::MAX,
            3 => frame
                .scene
                .placements
                .resize(2000, frame.scene.placements[0].clone()),
            _ => frame.retire.as_mut().unwrap().resource = request(3).resource,
        }
        let mut input = Some(frame);
        let error = if invalid == 4 {
            ContentCandidateError::Stale
        } else {
            ContentCandidateError::Malformed
        };
        assert_eq!(
            f.admit(&mut input, 4, 1),
            ContentFrameAdmission::Refused(error)
        );
        assert!(input.is_none());
        assert_eq!(f.worker.pending(), 0);
        assert_eq!(f.jobs.pending(), 0);
        let resources = f.registry.resources(grant()).unwrap();
        assert_eq!(resources.usage().staging, 0);
        assert_eq!(resources.reserved_retiring(), 0);
        assert!(resources.lease(grant(), request(1).resource).is_ok());
        f.registry
            .active_candidates_mut(grant())
            .unwrap()
            .take_frame_stream_event()
            .unwrap();
        let mut same_generation = Some(super::commits::frame(1, true));
        assert_eq!(
            f.admit(&mut same_generation, 4, 1),
            ContentFrameAdmission::Refused(ContentCandidateError::Stale)
        );
    }
}

#[test]
fn complete_scene_obeys_the_existing_candidate_byte_boundary() {
    for bytes in [135, 136] {
        let mut f = Fixture::with_candidate_bytes(bytes);
        let mut input = Some(frame(1, true));
        let result = f.admit(&mut input, 4, 1);
        if bytes == 135 {
            assert_eq!(
                result,
                ContentFrameAdmission::Refused(ContentCandidateError::Malformed)
            );
            assert_eq!(f.worker.pending(), 0);
            assert_eq!(
                f.registry.resources(grant()).unwrap().reserved_retiring(),
                0
            );
        } else {
            let _job = job(result);
            assert_eq!(f.worker.pending(), 1);
        }
        assert!(input.is_none());
    }
}

#[test]
fn missing_or_foreign_copy_source_leaves_input_owned_and_stale_tuple_does_not_spend_identity() {
    let mut f = Fixture::new();
    let mut foreign = image_registry();
    let (_, foreign_ring) = ring(&mut foreign);
    let before = f.registry.accounting();
    for source in [None, Some(&foreign_ring)] {
        let mut input = Some(frame(1, true));
        assert_eq!(
            f.jobs.admit_frame_commit(
                &mut f.registry,
                &mut f.worker,
                source,
                &mut input,
                4,
                1,
                deadline()
            ),
            Err(ContentCandidateError::Stale)
        );
        assert!(input.is_some());
        assert_eq!(f.registry.accounting(), before);
        assert_eq!(f.worker.pending(), 0);
    }
    let mut input = frame(1, true);
    input.image.as_mut().unwrap().allocation.generation += 1;
    let mut input = Some(input);
    assert_eq!(
        f.admit(&mut input, 4, 1),
        ContentFrameAdmission::Refused(ContentCandidateError::Stale)
    );
    f.refusal(1, ContentReason::Stale);
    let mut input = Some(frame(1, true));
    let _job = job(f.admit(&mut input, 4, 1));
}

#[test]
fn conflicting_predecessor_intent_refuses_before_copy_and_keeps_its_existing_owner() {
    let mut f = Fixture::new();
    let token = ContentRetireToken::default();
    f.registry
        .resources_mut(grant())
        .unwrap()
        .reserve_retire(
            ContentRetireOwner {
                grant: grant(),
                output: ContentOutputId {
                    id: 3,
                    generation: 1,
                },
                candidate_generation: 9,
            },
            ContentRetireOnPresent {
                retire_transaction: tx(70),
                resource: request(1).resource,
            },
            &token,
        )
        .unwrap();
    let mut input = Some(frame(1, true));
    assert_eq!(
        f.admit(&mut input, 4, 1),
        ContentFrameAdmission::Refused(ContentCandidateError::Stale)
    );
    assert!(input.is_none());
    assert_eq!(f.worker.pending(), 0);
    assert_eq!(
        f.registry.resources(grant()).unwrap().reserved_retiring(),
        16000
    );
    f.refusal(1, ContentReason::Stale);
    drop(token);
    assert_eq!(
        f.registry.resources(grant()).unwrap().reserved_retiring(),
        0
    );
    let mut input = Some(frame(2, true));
    let _job = job(f.admit(&mut input, 4, 1));
}

#[test]
fn expiry_cancels_the_intent_but_retains_the_copy_and_terminal_debt() {
    let mut f = Fixture::new();
    let mut input = Some(frame(1, true));
    let job = job(f.admit(&mut input, 4, 1));
    let now = 10000;
    let candidates = f.registry.active_candidates_mut(grant()).unwrap();
    candidates.expire(now).unwrap();
    assert_eq!(candidates.pending_event_count(), 0);
    assert_eq!(f.worker.pending(), 1);
    assert_eq!(
        f.registry.resources(grant()).unwrap().reserved_retiring(),
        0
    );
    assert!(
        f.registry
            .resources(grant())
            .unwrap()
            .lease(grant(), request(1).resource)
            .is_ok()
    );
    f.jobs
        .withdraw(
            &mut f.registry,
            &mut f.worker,
            job,
            IsbImageWithdrawal::Cancelled,
            now,
        )
        .unwrap();
    let mut settlement = f.jobs.reconcile(&mut f.registry, now).unwrap();
    settlement.finish_frame_copy(&mut f.registry, now).unwrap();
    let (resources, candidates) = f.registry.active_pair_mut(grant()).unwrap();
    assert!(
        candidates
            .transfer_frame_event_at(resources, now, |event| {
                let ShellContentRecord::CandidateOutcome(outcome) = &event.record else {
                    panic!()
                };
                assert_eq!(outcome.reason, ContentReason::Timeout as u16);
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(resources.usage().resident, 32000);
    assert_eq!(resources.usage().reserved_resident, 0);
}

#[test]
fn changed_facts_at_scene_handoff_cancel_retirement_and_abandon_the_new_image() {
    let mut f = Fixture::new();
    let mut input = Some(frame(1, true));
    let job = job(f.admit(&mut input, 4, 1));
    f.copy(job);
    assert_eq!(f.finish(1, 2, 3), Err(ContentCandidateError::Stale));
    let (resources, candidates) = f.registry.active_pair_mut(grant()).unwrap();
    assert_eq!(resources.reserved_retiring(), 0);
    assert!(resources.lease(grant(), request(1).resource).is_ok());
    assert_eq!(candidates.pending_candidate_count(), 0);
    while resources.take_event().is_some() {}
    assert!(
        candidates
            .transfer_frame_event_at(resources, 4, |_| Ok(()))
            .unwrap()
    );
    assert!(
        matches!(resources.take_event().unwrap().record, ShellContentRecord::ResourceReleased(value) if value.resource == request(3).resource)
    );
    resources.confirm_isb_release(request(3).resource).unwrap();
    assert_eq!(resources.usage().resident, 32000);
    assert_eq!(resources.usage().retiring, 0);
}
