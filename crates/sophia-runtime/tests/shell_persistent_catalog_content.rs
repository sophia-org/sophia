//! Actual candidate/resource custody with supplied catalog, allocation and
//! renderer completion. No negotiation, process launch or GPU execution.
use sophia_protocol::*;
use sophia_runtime::*;
#[allow(dead_code)]
#[path = "support/native_launcher_content.rs"]
mod fixture;
use fixture::{GRANT, OUTPUT, RESOURCE, catalog, context, end, limits, tx};

fn registry() -> ContentEpochRegistry {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    registry
        .admit_with_profile(limits(), ContentStoreProfile::PersistentCatalog)
        .unwrap();
    fixture::resources(&mut registry);
    registry
}
fn allocation() -> ContentAllocationSnapshot {
    let mut allocation = fixture::allocation();
    allocation.native_opening = None;
    allocation.role = 1;
    allocation
}

#[test]
fn persistent_allocations_use_edge_custody_without_popout_or_opening_authority() {
    let mut registry = registry();
    let store = registry.allocations_mut(GRANT).unwrap();
    store
        .publish_outputs(tx(1), 5, vec![fixture::facts()])
        .unwrap();
    store.take_event().unwrap();
    let request = ContentAllocationRequest {
        grant: GRANT,
        output: OUTPUT,
        allocation_request_id: 1,
        operation: 1,
        role: 1,
        edge: 1,
        prior: ContentAllocationId::default(),
        parent: ContentAllocationId::default(),
        parent_presentation_epoch: 0,
        anchor_parent_rect: ContentPixelRect::default(),
        desired_width: 64,
        desired_height: 32,
        margins: ContentMargins::default(),
    };
    for role in [2, 3] {
        let mut wrong = request.clone();
        wrong.role = role;
        assert!(store.request(tx(2), wrong, &[], 0).is_err());
        assert!(store.pending_request().is_none());
        assert!(store.pending_event().is_none());
    }
    assert!(
        store
            .request_native_launcher(tx(2), fixture::request(1), fixture::opening(), 0)
            .is_err()
    );
    store.request(tx(2), request.clone(), &[], 0).unwrap();
    assert_eq!(store.pending_native_opening(1), None);
    let mut wrong = allocation();
    wrong.native_opening = Some(7);
    assert!(store.grant(1, wrong, &[]).is_err());
    assert_eq!(store.pending_request().unwrap().1, request);
    store.grant(1, allocation(), &[]).unwrap();
    assert!(
        matches!(store.take_event().unwrap().record, ShellContentRecord::AllocationResult(v) if v.status == 1)
    );
    assert_eq!(store.snapshots(), vec![allocation()]);
    let mut release = request;
    release.allocation_request_id = 2;
    release.operation = 3;
    release.prior = allocation().allocation;
    release.desired_width = 0;
    release.desired_height = 0;
    store.request(tx(3), release, &[], 0).unwrap();
    // A release proposal is not disposition: the actual owner remains until
    // Session removes its content and explicitly completes this request.
    assert_eq!(store.snapshots(), vec![allocation()]);
    store.release(2).unwrap();
    assert!(store.snapshots().is_empty());
    assert!(store.pending_request().is_none());
    assert!(
        matches!(store.take_event().unwrap().record, ShellContentRecord::AllocationResult(v) if v.status == 3)
    );
    assert!(store.take_event().is_none());
}
fn begin() -> CatalogCandidateBegin {
    CatalogCandidateBegin {
        content: fixture::begin().content,
        catalog_generation: 8,
    }
}
fn chunk() -> ContentCandidateChunk {
    let mut chunk = fixture::chunk();
    chunk.surfaces[0].role = 1;
    for target in &mut chunk.targets {
        target.action_kind = 3;
    }
    chunk
}
fn assemble(registry: &mut ContentEpochRegistry) {
    let allocations = [allocation()];
    let (resources, candidates) = registry.active_parts_mut(GRANT).unwrap();
    candidates.grant_permit(tx(4), OUTPUT, 1, 1, 0).unwrap();
    candidates.take_event().unwrap();
    candidates
        .begin_persistent_catalog(tx(5), begin(), &catalog(), 0)
        .unwrap();
    candidates
        .chunk_persistent_catalog(tx(6), chunk(), 0)
        .unwrap();
    candidates
        .end_persistent_catalog(
            tx(7),
            end(),
            context(&allocations),
            &catalog(),
            resources,
            0,
        )
        .unwrap();
}

#[test]
fn persistent_binding_and_real_pixels_survive_until_exact_consumer_drops() {
    let mut registry = registry();
    assemble(&mut registry);
    let allocations = [allocation()];
    let candidates = registry.active_candidates_mut(GRANT).unwrap();
    let bundle = candidates
        .begin_persistent_catalog_submission(1, context(&allocations), &catalog(), 0)
        .unwrap();
    assert_eq!(bundle.persistent_catalog.unwrap().catalog_generation, 8);
    assert!(bundle.native_launcher.is_none());
    assert_eq!(bundle.resource(RESOURCE).unwrap().bytes().len(), 8);
    candidates.prepared(OUTPUT, 1, 1, 1, 0).unwrap();
    candidates.presented(OUTPUT, 1, 9, 1, 1).unwrap();
    let outcomes: Vec<_> = std::iter::from_fn(|| candidates.take_event()).collect();
    assert_eq!(outcomes.len(), 2);
    for (event, kind) in outcomes.iter().zip([1, 2]) {
        assert_eq!(event.transaction, tx(5));
        assert!(matches!(&event.record, ShellContentRecord::CandidateOutcome(v) if v.kind == kind));
    }
    assert!(registry.disconnect(GRANT));
    registry.collect();
    assert_eq!(registry.accounting().retired_epochs, 1);
    assert_eq!(registry.accounting().memory.resident, 8);
    assert_eq!(bundle.resource(RESOURCE).unwrap().bytes().len(), 8);
    drop(bundle);
    registry.collect();
    assert_eq!(registry.accounting().retired_epochs, 0);
    assert_eq!(registry.reserved_bytes(), 0);
}

#[test]
fn changed_authority_before_submission_keeps_the_pending_owner() {
    let mut registry = registry();
    assemble(&mut registry);
    let candidates = registry.active_candidates_mut(GRANT).unwrap();
    for mode in 0..6 {
        let mut current = catalog();
        let mut allocations = [allocation()];
        match mode {
            0 => current.generation += 1,
            1 => current.connection_epoch += 1,
            2 => current.entries[0].available = false,
            3 => allocations[0].allocation.generation += 1,
            4 => allocations[0].native_opening = Some(1),
            _ => allocations[0].scale_generation += 1,
        }
        assert!(
            candidates
                .begin_persistent_catalog_submission(1, context(&allocations), &current, 0)
                .is_err()
        );
        assert_eq!(candidates.pending_candidate_count(), 1);
        assert_eq!(candidates.submitted_candidate_count(), 0);
        assert!(candidates.take_event().is_none());
    }
    assert!(
        candidates
            .begin_persistent_catalog_submission(1, context(&[allocation()]), &catalog(), 0)
            .is_ok()
    );
}

#[test]
fn stale_begin_and_invalid_target_settle_only_their_terminal_debt() {
    for mode in 0..5 {
        let mut registry = registry();
        let (resources, candidates) = registry.active_parts_mut(GRANT).unwrap();
        candidates.grant_permit(tx(4), OUTPUT, 1, 1, 0).unwrap();
        candidates.take_event().unwrap();
        let mut current = catalog();
        if mode == 0 {
            current.generation += 1;
        }
        let result = candidates.begin_persistent_catalog(tx(5), begin(), &current, 0);
        if mode == 0 {
            assert_eq!(result, Err(ContentCandidateError::Stale));
        } else {
            result.unwrap();
            candidates
                .chunk_persistent_catalog(tx(6), chunk(), 0)
                .unwrap();
            match mode {
                1 => {
                    current.entries.pop();
                }
                2 => current.entries[0].available = false,
                3 => current.entries.push(current.entries[0].clone()),
                _ => current.generation += 1,
            }
            assert!(
                candidates
                    .end_persistent_catalog(
                        tx(7),
                        end(),
                        context(&[allocation()]),
                        &current,
                        resources,
                        0
                    )
                    .is_err()
            );
        }
        let terminal = candidates.take_event().unwrap();
        assert_eq!(terminal.transaction, tx(5));
        assert!(matches!(terminal.record, ShellContentRecord::CandidateOutcome(v) if v.kind == 3));
        assert!(candidates.take_event().is_none());
        assert_eq!(candidates.pending_candidate_count(), 0);
        assert_eq!(candidates.submitted_candidate_count(), 0);
    }
}

#[test]
fn persistent_role_cannot_use_legacy_native_or_wrong_kind_routes() {
    for profile in [
        ContentStoreProfile::Legacy,
        ContentStoreProfile::NativeLauncher,
    ] {
        let mut store = ContentCandidateStore::with_profile(limits(), profile).unwrap();
        assert_eq!(
            store.begin_persistent_catalog(tx(1), begin(), &catalog(), 0),
            Err(ContentCandidateError::Malformed)
        );
    }
    for kind in [1, 2] {
        let mut registry = registry();
        let store = registry.active_candidates_mut(GRANT).unwrap();
        assert_eq!(
            store.begin(tx(1), begin().content, 0),
            Err(ContentCandidateError::Malformed)
        );
        assert_eq!(
            store.begin_native_launcher(tx(1), fixture::begin(), fixture::native(&catalog()), 0),
            Err(ContentCandidateError::Malformed)
        );
        store.grant_permit(tx(4), OUTPUT, 1, 1, 0).unwrap();
        store.take_event().unwrap();
        store
            .begin_persistent_catalog(tx(5), begin(), &catalog(), 0)
            .unwrap();
        let mut wrong = chunk();
        wrong.targets[0].action_kind = kind;
        assert_eq!(
            store.chunk_persistent_catalog(tx(6), wrong, 0),
            Err(ContentCandidateError::Malformed)
        );
        assert!(store.take_event().is_some());
        assert!(store.take_event().is_none());
    }
}

#[test]
fn candidate_generation_is_still_grant_wide_across_outputs() {
    let mut registry = registry();
    assemble(&mut registry);
    let store = registry.active_candidates_mut(GRANT).unwrap();
    let mut second = begin();
    second.content.output.id += 1;
    second.content.pacing_permit = 2;
    store
        .grant_permit(tx(8), second.content.output, 2, 2, 0)
        .unwrap();
    store.take_event().unwrap();
    assert_eq!(
        store.begin_persistent_catalog(tx(9), second, &catalog(), 0),
        Err(ContentCandidateError::Stale)
    );
    assert_eq!(store.pending_candidate_count(), 1);
    assert_eq!(store.submitted_candidate_count(), 0);
}

#[test]
fn refused_begin_consumes_its_generation_as_well_as_its_permit() {
    let mut registry = registry();
    let store = registry.active_candidates_mut(GRANT).unwrap();
    store.grant_permit(tx(1), OUTPUT, 1, 1, 0).unwrap();
    store.take_event().unwrap();
    let mut stale = catalog();
    stale.generation += 1;
    assert_eq!(
        store.begin_persistent_catalog(tx(2), begin(), &stale, 0),
        Err(ContentCandidateError::Stale)
    );
    store.take_event().unwrap();
    store.grant_permit(tx(3), OUTPUT, 2, 2, 0).unwrap();
    store.take_event().unwrap();
    let mut retry = begin();
    retry.content.pacing_permit = 2;
    assert_eq!(
        store.begin_persistent_catalog(tx(4), retry, &catalog(), 0),
        Err(ContentCandidateError::Stale)
    );
    store.take_event().unwrap();
    store.grant_permit(tx(5), OUTPUT, 3, 3, 0).unwrap();
    store.take_event().unwrap();
    let mut fresh = begin();
    fresh.content.pacing_permit = 3;
    fresh.content.candidate_generation = 2;
    assert!(
        store
            .begin_persistent_catalog(tx(6), fresh, &catalog(), 0)
            .is_ok()
    );
}
