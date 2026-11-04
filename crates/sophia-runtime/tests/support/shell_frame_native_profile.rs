//! Native SFT eligibility is narrower than the reusable store and legacy image path.
use super::*;

pub(super) fn open(registry: &mut ContentEpochRegistry) -> FrameStreamKey {
    let allocations = registry.allocations(grant()).unwrap().snapshots();
    let key = FrameStreamKey {
        grant: grant(),
        output: allocations[0].output,
        allocation: allocations[0].allocation,
        facts_generation: 1,
        stream_id: 1,
    };
    let store = registry.active_candidates_mut(grant()).unwrap();
    store
        .open_native_frame_stream(
            tx(10),
            key,
            ContentCandidateContext {
                output: key.output,
                facts_generation: 1,
                interaction_generation: 1,
                allocations: &allocations,
            },
            0,
        )
        .unwrap();
    let event = store.take_frame_stream_event().unwrap();
    assert!(
        matches!(event.record, ContentFrameStreamRecord::Opened { credits: 1, key: actual, .. }
        if actual == key)
    );
    key
}

pub(super) fn ring(registry: &mut ContentEpochRegistry, slots: usize) -> (File, OwnedRing) {
    let geometry = OwnedRingGeometry::new(slots, 16000).unwrap();
    let file = File::from(
        rustix::fs::memfd_create("native-profile-test", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    file.set_len(geometry.source_bytes() as u64).unwrap();
    let reservation = registry
        .reserve_owned_ring_source(grant(), geometry)
        .unwrap();
    let ring = OwnedRing::admit(file.try_clone().unwrap(), reservation).unwrap();
    (file, ring)
}

#[test]
fn only_two_slot_copy_sources_enter_the_native_profile() {
    for slots in [1, 3, 4] {
        let mut f = Fixture::new();
        (f.file, f.ring) = ring(&mut f.registry, slots);
        let before = f.registry.accounting();
        let mut input = Some(frame(1, true));
        assert_eq!(
            f.admit(&mut input, 4, 1),
            ContentFrameAdmission::Refused(ContentCandidateError::Malformed)
        );
        assert!(input.is_none());
        f.refusal(1, ContentReason::Malformed);
        assert_eq!(f.registry.accounting(), before);
        assert_eq!((f.jobs.pending(), f.worker.pending()), (0, 0));
        assert_eq!(
            f.registry.resources(grant()).unwrap().reserved_retiring(),
            0
        );
        // Unsupported geometry does not disable retained-only input or legacy sources.
        let mut retained = Some(frame(2, false));
        assert_eq!(
            f.admit(&mut retained, 2, 1),
            ContentFrameAdmission::Accepted { copy_job: None }
        );
        assert!(f.ring.offer(request(3).slot, 16000).is_ok());
    }
}

#[test]
fn one_native_credit_stays_spent_through_prepared_and_blocked_terminal() {
    let mut f = Fixture::new();
    let mut input = Some(frame(1, false));
    assert_eq!(
        f.admit(&mut input, 2, 1),
        ContentFrameAdmission::Accepted { copy_job: None }
    );
    f.finish(1, 1, 1).unwrap();
    let store = f.registry.active_candidates_mut(grant()).unwrap();
    let _display = store.begin_submission(output(), 1, 1).unwrap();
    store.prepared(output(), 1, 1, 1, 1).unwrap();
    store.take_event().unwrap();
    let mut next = frame(2, false);
    next.retire = None;
    assert_eq!(
        f.admit(&mut Some(next), 2, 2),
        ContentFrameAdmission::Refused(ContentCandidateError::Budget)
    );
    f.refusal(2, ContentReason::Budget);
    let (resources, store) = f.registry.active_pair_mut(grant()).unwrap();
    store.presented_at(output(), 1, 1, 1, 1, 2).unwrap();
    assert_eq!(
        store.transfer_frame_event_at(resources, 2, |_| Err(ContentCandidateError::Budget)),
        Err(ContentCandidateError::Budget)
    );
    let mut next = frame(3, false);
    next.retire = None;
    assert_eq!(
        f.admit(&mut Some(next), 2, 2),
        ContentFrameAdmission::Refused(ContentCandidateError::Budget)
    );
    let (resources, store) = f.registry.active_pair_mut(grant()).unwrap();
    assert!(
        store
            .transfer_frame_event_at(resources, 2, |event| {
                assert!(event.frame_credit.unwrap().expires_msec.is_some());
                Ok(())
            })
            .unwrap()
    );
    f.refusal(3, ContentReason::Budget);
    let mut next = frame(4, false);
    next.retire = None;
    assert_eq!(
        f.admit(&mut Some(next), 2, 3),
        ContentFrameAdmission::Accepted { copy_job: None }
    );
}

#[cfg(feature = "experimental-shell-capture")]
#[path = "captured_resource_fixture.rs"]
mod captured;

#[cfg(feature = "experimental-shell-capture")]
#[test]
fn native_open_uses_the_same_exact_allocation_context_as_the_existing_owner() {
    let mut registry = captured::allocation_registry(grant());
    let key = open(&mut registry);
    let allocations = registry.allocations(grant()).unwrap().snapshots();
    let store = registry.active_candidates_mut(grant()).unwrap();
    store.close_frame_stream(tx(20), key, 1).unwrap();
    store.take_frame_stream_event().unwrap();
    let key = FrameStreamKey {
        stream_id: 2,
        ..key
    };
    let mut context = ContentCandidateContext {
        output: key.output,
        facts_generation: 2,
        interaction_generation: 1,
        allocations: &allocations,
    };
    assert_eq!(
        store.open_native_frame_stream(tx(21), key, context, 1),
        Err(ContentCandidateError::Stale)
    );
    context.facts_generation = 1;
    store
        .open_native_frame_stream(tx(21), key, context, 1)
        .unwrap();
    assert!(matches!(
        store.take_frame_stream_event().unwrap().record,
        ContentFrameStreamRecord::Opened { credits: 1, .. }
    ));
}

#[cfg(feature = "experimental-shell-capture")]
#[test]
fn gpu_retained_layer_refuses_before_copy_and_predecessor_reservation() {
    for (copying, retiring_gpu) in [(false, false), (true, false), (true, true)] {
        let mut f = Fixture::new();
        let storage = captured::storage(4);
        let mut metadata = captured::metadata(grant());
        metadata.offer_id = 4;
        metadata.resource = 4;
        metadata.output_id = output().id;
        let mut reservation = Some(
            f.registry
                .reserve_capture_image(grant(), &metadata, 0)
                .unwrap(),
        );
        let mut image = Some(storage.image);
        f.registry
            .accept_capture_image(&mut reservation, &mut image, 0)
            .unwrap();
        assert!(reservation.is_none() && image.is_none());
        // The fixture is storage/accounting evidence, not a physical GPU capture.
        drop((storage.budget, storage.epoch));
        let resources = f.registry.resources_mut(grant()).unwrap();
        while resources.take_event().is_some() {}
        let gpu = request(4).resource;
        assert!(resources.lease(grant(), gpu).unwrap().cpu_bytes().is_none());
        let before = f.registry.accounting();
        let mut input = frame(1, copying);
        if retiring_gpu {
            input.retire.as_mut().unwrap().resource = gpu;
        } else {
            input
                .scene
                .placements
                .push(scene_record(gpu).placements.remove(0));
        }
        let mut input = Some(input);
        assert_eq!(f.admit(&mut input, 0, 1), ContentFrameAdmission::Deferred);
        assert!(input.is_some());
        assert_eq!(
            f.admit(&mut input, 4, 1),
            ContentFrameAdmission::Refused(ContentCandidateError::Malformed)
        );
        assert!(input.is_none());
        f.refusal(1, ContentReason::Malformed);
        assert_eq!(f.registry.accounting(), before);
        assert_eq!((f.jobs.pending(), f.worker.pending()), (0, 0));
        let resources = f.registry.resources(grant()).unwrap();
        assert_eq!(resources.reserved_retiring(), 0);
        assert!(resources.lease(grant(), request(1).resource).is_ok());
        assert!(
            resources.lease(grant(), gpu).is_ok(),
            "legacy use stays valid"
        );
        // The ordinary candidate path must continue to compose the same resource.
        let allocations = f.registry.allocations(grant()).unwrap().snapshots();
        let resources = f.registry.resources(grant()).unwrap();
        let context = ContentCandidateContext {
            output: output(),
            facts_generation: 1,
            interaction_generation: 1,
            allocations: &allocations,
        };
        let mut stream = ContentCandidateStore::new(ContentLimits::prototype(grant())).unwrap();
        stream
            .open_native_frame_stream(
                tx(60),
                FrameStreamKey {
                    grant: grant(),
                    output: output(),
                    allocation: request(1).allocation,
                    facts_generation: 1,
                    stream_id: 1,
                },
                context,
                1,
            )
            .unwrap();
        stream.take_frame_stream_event().unwrap();
        stream.begin(tx(61), begin(1), 1).unwrap();
        stream.chunk(tx(62), scene_record(gpu), 1).unwrap();
        assert_eq!(
            stream.end(
                tx(63),
                ContentCandidateEnd {
                    grant: grant(),
                    candidate_generation: 1,
                    surface_count: 1,
                    placement_count: 1,
                    target_count: 0,
                },
                context,
                resources,
                1
            ),
            Err(ContentCandidateError::Malformed)
        );
        let mut legacy = ContentCandidateStore::new(ContentLimits::prototype(grant())).unwrap();
        legacy.grant_permit(tx(60), output(), 1, 1, 1).unwrap();
        legacy.take_event().unwrap();
        legacy.begin(tx(61), begin(1), 1).unwrap();
        legacy.chunk(tx(62), scene_record(gpu), 1).unwrap();
        legacy
            .end(
                tx(63),
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
                1,
            )
            .unwrap();
        let bundle = legacy.begin_submission(output(), 1, 1).unwrap();
        assert!(bundle.resource(gpu).unwrap().cpu_bytes().is_none());
    }
}
