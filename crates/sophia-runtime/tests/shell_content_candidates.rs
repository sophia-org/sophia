use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/content_shutdown.rs"]
mod shutdown;

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    }
}

fn tx(value: u64) -> TransactionId {
    TransactionId::from_raw(value)
}

fn output() -> ContentOutputId {
    ContentOutputId {
        id: 2,
        generation: 3,
    }
}

fn second_output() -> ContentOutputId {
    ContentOutputId {
        id: 3,
        generation: 4,
    }
}

fn allocation_id(id: u64) -> ContentAllocationId {
    ContentAllocationId { id, generation: 1 }
}

fn resource_id(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

fn resource_begin(id: u64) -> ContentResourceBegin {
    ContentResourceBegin {
        grant: grant(),
        resource: resource_id(id),
        width_px: 2,
        height_px: 1,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 8,
    }
}

fn upload(store: &mut ContentResourceStore, id: u64) {
    let begin = resource_begin(id);
    store.begin(tx(1), begin.clone(), 0).unwrap();
    store
        .chunk(
            tx(2),
            &ContentResourceChunk {
                grant: grant(),
                resource: begin.resource,
                ordinal: 0,
                offset: 0,
                bytes: vec![0, 0, 255, 255, 0, 128, 0, 128],
            },
            0,
        )
        .unwrap();
    store
        .end(
            tx(3),
            &ContentResourceEnd {
                grant: grant(),
                resource: begin.resource,
                total_bytes: 8,
                chunk_count: 1,
            },
            0,
        )
        .unwrap();
    while store.take_event().is_some() {}
}

fn allocation() -> ContentAllocationSnapshot {
    ContentAllocationSnapshot {
        output: output(),
        allocation: allocation_id(1),
        scale_generation: 4,
        scale_numerator: 1,
        scale_denominator: 1,
        role: 1,
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 64,
            height: 32,
        },
        pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 64,
            height: 32,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: 32,
    }
}

fn begin(generation: u64) -> ContentCandidateBegin {
    ContentCandidateBegin {
        grant: grant(),
        candidate_generation: generation,
        output: output(),
        facts_generation: 6,
        pacing_permit: generation,
        interaction_generation: 8,
        surface_count: 1,
        placement_count: 1,
        target_count: 1,
    }
}

#[test]
fn candidate_generations_are_grant_scoped_across_outputs() {
    let (mut candidates, _) = stores();
    candidates.grant_permit(tx(10), output(), 1, 1, 0).unwrap();
    candidates.begin(tx(11), begin(1), 1).unwrap();

    candidates
        .grant_permit(tx(12), second_output(), 2, 2, 2)
        .unwrap();
    let mut repeated = begin(1);
    repeated.output = second_output();
    repeated.pacing_permit = 2;
    assert_eq!(
        candidates.begin(tx(13), repeated, 3),
        Err(ContentCandidateError::Stale)
    );
    assert!(
        std::iter::from_fn(|| candidates.take_event()).any(|event| matches!(
            event.record,
            ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
                candidate_generation: 1,
                output,
                kind: 3,
                reason,
                ..
            }) if output == second_output() && reason == ContentReason::Stale as u16
        ))
    );
}

fn chunk(generation: u64, resource: ContentResourceId) -> ContentCandidateChunk {
    ContentCandidateChunk {
        grant: grant(),
        candidate_generation: generation,
        chunk_ordinal: 0,
        surfaces: vec![ContentSurface {
            allocation: allocation_id(1),
            scale_generation: 4,
            role: 1,
            edge: 1,
            margins: ContentMargins::default(),
            reservation_extent: 24,
            parent_surface_index: u16::MAX,
            anchor_parent_rect: ContentPixelRect::default(),
        }],
        placements: vec![ContentPlacement {
            resource,
            surface_index: 0,
            destination_x_px: 3,
            destination_y_px: 4,
        }],
        targets: vec![ContentTarget {
            surface_index: 0,
            action_kind: 1,
            target_id: 1,
            target_generation: 1,
            action_id: 1,
            bounds_px: ContentPixelRect {
                x: 3,
                y: 4,
                width: 2,
                height: 1,
            },
        }],
    }
}

fn end(generation: u64) -> ContentCandidateEnd {
    ContentCandidateEnd {
        grant: grant(),
        candidate_generation: generation,
        surface_count: 1,
        placement_count: 1,
        target_count: 1,
    }
}

fn context(allocations: &[ContentAllocationSnapshot]) -> ContentCandidateContext<'_> {
    ContentCandidateContext {
        output: output(),
        facts_generation: 6,
        interaction_generation: 8,
        allocations,
    }
}

fn stores() -> (ContentCandidateStore, ContentResourceStore) {
    let limits = ContentLimits::prototype(grant());
    (
        ContentCandidateStore::new(limits.clone()).unwrap(),
        ContentResourceStore::new(limits).unwrap(),
    )
}

fn assemble(
    candidates: &mut ContentCandidateStore,
    resources: &ContentResourceStore,
    generation: u64,
    allocations: &[ContentAllocationSnapshot],
) {
    candidates
        .grant_permit(tx(10), output(), generation, generation, 0)
        .unwrap();
    candidates.begin(tx(11), begin(generation), 1).unwrap();
    candidates
        .chunk(tx(12), chunk(generation, resource_id(1)), 2)
        .unwrap();
    candidates
        .end(tx(13), end(generation), context(allocations), resources, 3)
        .unwrap();
}

#[test]
fn complete_candidate_holds_pixels_through_native_retirement() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    let allocations = [allocation()];
    assemble(&mut candidates, &resources, 1, &allocations);
    assert_eq!(candidates.pending_candidate_count(), 1);

    let render = candidates.begin_submission(output(), 1, 4).unwrap();
    assert_eq!(render.resource(resource_id(1)).unwrap().bytes().len(), 8);
    assert_eq!(candidates.submitted_candidate_count(), 1);
    candidates.prepared(output(), 1, 20, 30, 5).unwrap();

    resources
        .retire(
            tx(20),
            &ContentResourceRetire {
                grant: grant(),
                resource: resource_id(1),
            },
        )
        .unwrap();
    drop(render);
    resources.collect();
    assert_eq!(resources.usage().retiring, 8);

    candidates.presented(output(), 1, 40, 20, 30).unwrap();
    resources.collect();
    assert_eq!(resources.usage().retiring, 0);
    let events: Vec<_> = std::iter::from_fn(|| candidates.take_event()).collect();
    assert!(events.iter().any(|event| matches!(
        &event.record,
        ShellContentRecord::CandidateOutcome(outcome)
            if outcome.kind == 1 && outcome.presentation_epoch == 0
    )));
    assert!(events.iter().any(|event| matches!(
        &event.record,
        ShellContentRecord::CandidateOutcome(outcome)
            if outcome.kind == 2 && outcome.presentation_epoch == 40
    )));
}

#[test]
fn overlapping_targets_on_one_surface_are_rejected() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    candidates.grant_permit(tx(10), output(), 1, 1, 0).unwrap();
    let mut begin = begin(1);
    begin.target_count = 2;
    candidates.begin(tx(11), begin, 1).unwrap();
    let mut chunk = chunk(1, resource_id(1));
    let mut overlap = chunk.targets[0].clone();
    overlap.target_id = 2;
    overlap.action_id = 2;
    overlap.bounds_px.x = 4;
    chunk.targets.push(overlap);
    candidates.chunk(tx(12), chunk, 2).unwrap();
    let mut end = end(1);
    end.target_count = 2;
    assert_eq!(
        candidates.end(tx(13), end, context(&[allocation()]), &resources, 3),
        Err(ContentCandidateError::Malformed)
    );
}

#[test]
fn incomplete_end_pays_the_terminal_outcome_debt() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    candidates.grant_permit(tx(10), output(), 1, 1, 0).unwrap();
    candidates.begin(tx(11), begin(1), 1).unwrap();
    let allocations = [allocation()];
    assert_eq!(
        candidates.end(tx(12), end(1), context(&allocations), &resources, 2),
        Err(ContentCandidateError::Incomplete)
    );
    let events: Vec<_> = std::iter::from_fn(|| candidates.take_event()).collect();
    assert!(events.iter().any(|event| matches!(
        &event.record,
        ShellContentRecord::CandidateOutcome(outcome)
            if outcome.kind == 3 && outcome.reason == ContentReason::Incomplete as u16
    )));
}

#[test]
fn assembly_and_permit_deadlines_are_visible_recovery_events() {
    let (mut candidates, _) = stores();
    candidates.grant_permit(tx(10), output(), 1, 1, 0).unwrap();
    candidates.expire(250).unwrap();
    assert!(matches!(
        candidates.take_event().unwrap().record,
        ShellContentRecord::FramePermit(ContentFramePermit { state: 1, .. })
    ));
    assert!(matches!(
        candidates.take_event().unwrap().record,
        ShellContentRecord::FramePermit(ContentFramePermit {
            state: 2,
            reason,
            ..
        }) if reason == ContentReason::Timeout as u16
    ));

    candidates
        .grant_permit(tx(20), output(), 2, 2, 251)
        .unwrap();
    candidates.begin(tx(21), begin(2), 252).unwrap();
    candidates.expire(1252).unwrap();
    assert!(matches!(
        candidates.take_event().unwrap().record,
        ShellContentRecord::FramePermit(ContentFramePermit { state: 1, .. })
    ));
    assert!(matches!(
        candidates.take_event().unwrap().record,
        ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
            kind: 3,
            reason,
            ..
        }) if reason == ContentReason::Timeout as u16
    ));
}

#[test]
fn stale_facts_and_allocation_identity_never_pin_pixels() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    candidates.grant_permit(tx(10), output(), 1, 1, 0).unwrap();
    candidates.begin(tx(11), begin(1), 1).unwrap();
    candidates
        .chunk(tx(12), chunk(1, resource_id(1)), 2)
        .unwrap();
    let allocations = [allocation()];
    let mut stale = context(&allocations);
    stale.facts_generation += 1;
    assert_eq!(
        candidates.end(tx(13), end(1), stale, &resources, 3),
        Err(ContentCandidateError::Stale)
    );
    assert_eq!(resources.usage().resident, 8);

    candidates.grant_permit(tx(20), output(), 2, 2, 4).unwrap();
    candidates.begin(tx(21), begin(2), 5).unwrap();
    candidates
        .chunk(tx(22), chunk(2, resource_id(1)), 6)
        .unwrap();
    let mut changed = allocation();
    changed.allocation.generation = 2;
    assert_eq!(
        candidates.end(tx(23), end(2), context(&[changed]), &resources, 7),
        Err(ContentCandidateError::AllocationLost)
    );
}

#[test]
fn placement_scale_and_bounds_are_checked_before_acceptance() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    candidates.grant_permit(tx(10), output(), 1, 1, 0).unwrap();
    candidates.begin(tx(11), begin(1), 1).unwrap();
    let mut invalid = chunk(1, resource_id(1));
    invalid.placements[0].destination_x_px = 63;
    candidates.chunk(tx(12), invalid, 2).unwrap();
    assert_eq!(
        candidates.end(tx(13), end(1), context(&[allocation()]), &resources, 3),
        Err(ContentCandidateError::Malformed)
    );

    candidates.grant_permit(tx(20), output(), 2, 2, 4).unwrap();
    candidates.begin(tx(21), begin(2), 5).unwrap();
    candidates
        .chunk(tx(22), chunk(2, resource_id(1)), 6)
        .unwrap();
    let mut scaled = allocation();
    scaled.scale_numerator = 2;
    assert_eq!(
        candidates.end(tx(23), end(2), context(&[scaled]), &resources, 7),
        Err(ContentCandidateError::Malformed)
    );
}

#[test]
fn submitted_candidate_is_never_superseded_or_timed_out() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    assemble(&mut candidates, &resources, 1, &[allocation()]);
    let render = candidates.begin_submission(output(), 1, 4).unwrap();
    assert_eq!(candidates.grant_permit(tx(20), output(), 2, 2, 4), Ok(()));
    candidates.expire(10_000).unwrap();
    assert_eq!(candidates.submitted_candidate_count(), 1);
    assert_eq!(render.resource(resource_id(1)).unwrap().bytes().len(), 8);
}

#[test]
fn revocation_keeps_submitted_storage_and_emits_no_dead_peer_outcome() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    assemble(&mut candidates, &resources, 1, &[allocation()]);
    let render = candidates.begin_submission(output(), 1, 4).unwrap();
    while candidates.take_event().is_some() {}
    candidates.revoke();
    candidates.prepared(output(), 1, 2, 3, 5).unwrap();
    candidates.presented(output(), 1, 4, 2, 3).unwrap();
    assert!(candidates.take_event().is_none());
    resources
        .retire(
            tx(30),
            &ContentResourceRetire {
                grant: grant(),
                resource: resource_id(1),
            },
        )
        .unwrap();
    resources.collect();
    assert_eq!(resources.usage().retiring, 8);
    drop(render);
    resources.collect();
    assert_eq!(resources.usage().retiring, 0);
}

#[test]
fn only_accepted_pending_work_can_be_superseded() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    assemble(&mut candidates, &resources, 1, &[allocation()]);
    candidates.grant_permit(tx(20), output(), 2, 2, 4).unwrap();
    assert_eq!(candidates.pending_candidate_count(), 0);
    let events: Vec<_> = std::iter::from_fn(|| candidates.take_event()).collect();
    assert!(events.iter().any(|event| matches!(
        &event.record,
        ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
            candidate_generation: 1,
            kind: 4,
            reason,
            ..
        }) if *reason == ContentReason::Superseded as u16
    )));

    candidates.begin(tx(21), begin(2), 5).unwrap();
    candidates
        .chunk(tx(22), chunk(2, resource_id(1)), 6)
        .unwrap();
    candidates
        .end(tx(23), end(2), context(&[allocation()]), &resources, 7)
        .unwrap();
    let render = candidates.begin_submission(output(), 2, 8).unwrap();
    assert!(matches!(
        candidates.begin_submission(output(), 2, 9),
        Err(ContentCandidateError::Budget)
    ));
    assert_eq!(render.resource(resource_id(1)).unwrap().bytes().len(), 8);
}

#[test]
fn resource_retired_during_assembly_cannot_acquire_a_new_candidate_pin() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    candidates.grant_permit(tx(10), output(), 1, 1, 0).unwrap();
    candidates.begin(tx(11), begin(1), 1).unwrap();
    candidates
        .chunk(tx(12), chunk(1, resource_id(1)), 2)
        .unwrap();
    resources
        .retire(
            tx(13),
            &ContentResourceRetire {
                grant: grant(),
                resource: resource_id(1),
            },
        )
        .unwrap();
    assert_eq!(
        candidates.end(tx(14), end(1), context(&[allocation()]), &resources, 3),
        Err(ContentCandidateError::Stale)
    );
    resources.collect();
    assert_eq!(resources.usage().retiring, 0);
}

#[test]
fn direct_malformed_rows_are_rejected_before_resource_lookup() {
    let (mut candidates, _) = stores();
    candidates.grant_permit(tx(10), output(), 1, 1, 0).unwrap();
    candidates.begin(tx(11), begin(1), 1).unwrap();
    let mut malformed = chunk(1, resource_id(1));
    malformed.targets[0].action_kind = 2;
    assert_eq!(
        candidates.chunk(tx(12), malformed, 2),
        Err(ContentCandidateError::Malformed)
    );
    assert!(matches!(
        candidates.take_event().unwrap().record,
        ShellContentRecord::FramePermit(ContentFramePermit { state: 1, .. })
    ));
    assert!(matches!(
        candidates.take_event().unwrap().record,
        ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
            kind: 3,
            reason,
            ..
        }) if reason == ContentReason::Malformed as u16
    ));
}

#[test]
fn presentation_requires_one_prepared_transition() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    assemble(&mut candidates, &resources, 1, &[allocation()]);
    let _render = candidates.begin_submission(output(), 1, 4).unwrap();
    assert_eq!(
        candidates.presented(output(), 1, 5, 6, 7),
        Err(ContentCandidateError::Stale)
    );
    candidates.prepared(output(), 1, 6, 7, 5).unwrap();
    assert_eq!(
        candidates.prepared(output(), 1, 6, 7, 6),
        Err(ContentCandidateError::Stale)
    );
    candidates.presented(output(), 1, 8, 6, 7).unwrap();
}

#[test]
fn disconnected_epoch_retains_submitted_candidate_until_renderer_retirement() {
    let limits = ContentLimits::prototype(grant());
    let mut epochs = ContentEpochPool::new(limits.max_session_retiring_bytes).unwrap();
    epochs.admit(limits).unwrap();
    upload(epochs.active_mut().unwrap(), 1);
    let allocations = [allocation()];
    let (resources, candidates) = epochs.active_parts_mut().unwrap();
    assemble(candidates, resources, 1, &allocations);
    let render = candidates.begin_submission(output(), 1, 4).unwrap();

    epochs.disconnect();
    assert_eq!(epochs.retired_bytes(), 8);
    assert_eq!(epochs.accounting().candidates, 1);
    assert!(!epochs.accounting().quiescent());
    let retired = epochs.candidates_mut(grant()).unwrap();
    retired.prepared(output(), 1, 2, 3, 5).unwrap();
    retired.presented(output(), 1, 4, 2, 3).unwrap();
    epochs.collect();
    assert_eq!(epochs.accounting().candidates, 0);
    assert_eq!(epochs.retired_bytes(), 8);

    drop(render);
    epochs.collect();
    assert_eq!(epochs.retired_bytes(), 0);
    assert_eq!(epochs.reserved_bytes(), 0);
    assert!(epochs.accounting().quiescent());
}

#[test]
fn ending_the_copy_consumer_does_not_hide_an_unsettled_submitted_candidate() {
    let limits = ContentLimits::prototype(grant());
    let mut epochs = ContentEpochPool::new(limits.max_session_retiring_bytes).unwrap();
    epochs.admit(limits).unwrap();
    upload(epochs.active_mut().unwrap(), 1);
    let (resources, candidates) = epochs.active_parts_mut().unwrap();
    assemble(candidates, resources, 1, &[allocation()]);
    let render = candidates.begin_submission(output(), 1, 4).unwrap();
    epochs.disconnect();
    drop(render);
    epochs.collect();
    let pending = epochs.accounting();
    assert_eq!(pending.candidates, 1);
    assert_eq!(pending.resources, 1);
    assert_eq!(pending.reserved_bytes, 8);
    assert!(!pending.quiescent());
    epochs
        .candidates_mut(grant())
        .unwrap()
        .renderer_failed(output(), 1)
        .unwrap();
    epochs.collect();
    assert!(epochs.accounting().quiescent());
}

#[test]
fn demands_coalesce_but_withdrawal_keeps_priority_until_permitted() {
    let (mut candidates, _) = stores();
    let allocation = allocation();
    let demand = |id, reason| ContentFrameDemand {
        grant: grant(),
        output: output(),
        allocation: allocation.allocation,
        demand_id: id,
        reason,
    };
    candidates
        .demand(
            tx(1),
            demand(1, 1),
            &[output()],
            std::slice::from_ref(&allocation),
        )
        .unwrap();
    candidates
        .demand(
            tx(2),
            demand(2, 2),
            &[output()],
            std::slice::from_ref(&allocation),
        )
        .unwrap();
    assert_eq!(candidates.next_demand().unwrap().1.demand_id, 2);
    candidates
        .demand(
            tx(3),
            demand(3, 3),
            &[output()],
            std::slice::from_ref(&allocation),
        )
        .unwrap();
    assert_eq!(
        candidates.demand(
            tx(4),
            demand(4, 1),
            &[output()],
            std::slice::from_ref(&allocation),
        ),
        Err(ContentCandidateError::Stale)
    );
    assert_eq!(candidates.next_demand().unwrap().1.demand_id, 3);
    candidates.grant_demand(tx(5), output(), 1, 0).unwrap();
    assert!(candidates.next_demand().is_none());
}

#[test]
fn demand_and_unconsumed_permit_cancellation_are_explicit_and_exact() {
    let (mut candidates, _) = stores();
    let request = ContentFrameDemand {
        grant: grant(),
        output: output(),
        allocation: ContentAllocationId::default(),
        demand_id: 1,
        reason: 1,
    };
    candidates.demand(tx(1), request, &[output()], &[]).unwrap();
    candidates.grant_demand(tx(2), output(), 4, 0).unwrap();
    assert_eq!(
        candidates.cancel_demand(
            tx(3),
            ContentFrameDemandCancel {
                grant: grant(),
                output: output(),
                demand_id: 1,
                permit_id: 5,
            }
        ),
        Err(ContentCandidateError::Stale)
    );
    candidates
        .cancel_demand(
            tx(4),
            ContentFrameDemandCancel {
                grant: grant(),
                output: output(),
                demand_id: 1,
                permit_id: 4,
            },
        )
        .unwrap();
    let events: Vec<_> = std::iter::from_fn(|| candidates.take_event()).collect();
    assert!(events.iter().any(|event| matches!(
        &event.record,
        ShellContentRecord::FramePermit(ContentFramePermit {
            permit_id: 4,
            state: 3,
            reason,
            ..
        }) if *reason == ContentReason::Cancelled as u16
    )));
}

#[test]
fn a_demand_cannot_invent_an_output_or_cancel_the_wrong_standing_request() {
    let (mut candidates, _) = stores();
    let request = ContentFrameDemand {
        grant: grant(),
        output: output(),
        allocation: ContentAllocationId::default(),
        demand_id: 1,
        reason: 1,
    };
    assert_eq!(
        candidates.demand(tx(1), request.clone(), &[], &[]),
        Err(ContentCandidateError::Stale)
    );
    candidates.demand(tx(2), request, &[output()], &[]).unwrap();
    assert_eq!(
        candidates.cancel_demand(
            tx(3),
            ContentFrameDemandCancel {
                grant: grant(),
                output: output(),
                demand_id: 2,
                permit_id: 0,
            }
        ),
        Err(ContentCandidateError::Stale)
    );
    assert_eq!(candidates.next_demand().unwrap().1.demand_id, 1);
    candidates
        .cancel_demand(
            tx(4),
            ContentFrameDemandCancel {
                grant: grant(),
                output: output(),
                demand_id: 1,
                permit_id: 0,
            },
        )
        .unwrap();
    assert!(candidates.next_demand().is_none());
}

#[test]
fn a_waiting_output_does_not_hide_another_outputs_complete_candidate() {
    let (mut candidates, mut resources) = stores();
    upload(&mut resources, 1);
    let allocations = [allocation()];
    assemble(&mut candidates, &resources, 1, &allocations);
    let mut other_allocation = allocation();
    other_allocation.output = second_output();
    candidates
        .grant_permit(tx(14), second_output(), 2, 2, 4)
        .unwrap();
    let mut other_begin = begin(2);
    other_begin.output = second_output();
    candidates.begin(tx(15), other_begin, 5).unwrap();
    candidates
        .chunk(tx(16), chunk(2, resource_id(1)), 6)
        .unwrap();
    candidates
        .end(
            tx(17),
            end(2),
            ContentCandidateContext {
                output: second_output(),
                facts_generation: 6,
                interaction_generation: 8,
                allocations: &[other_allocation],
            },
            &resources,
            7,
        )
        .unwrap();
    assert_eq!(candidates.next_pending_candidate(), Some((output(), 1)));
    assert_eq!(
        candidates.next_pending_candidate_for(|candidate| candidate != output()),
        Some((second_output(), 2))
    );
    assert_eq!(candidates.next_pending_candidate_for(|_| false), None);
    assert_eq!(candidates.pending_candidate_count(), 2);
    assert_eq!(
        candidates.next_pending_candidate(),
        Some((output(), 1)),
        "skipping did not consume the waiting owner"
    );
}
