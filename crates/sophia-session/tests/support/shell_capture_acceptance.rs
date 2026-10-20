//! Owner accounting with supplied completed storage; no fake native receipts.
use super::*;
use captured_fixture::{metadata, storage};
use sophia_renderer_live::LiveRendererImageId;

#[test]
fn bounded_eviction_retries_active_and_retired_epochs_without_skipping_obligations() {
    let mut registry = ContentEpochRegistry::with_active_capacity(64 << 20, 3).unwrap();
    let mut budgets = Vec::new();
    for i in 1..=3 {
        let b = binding(i, 1);
        captured_fixture::add_allocation_epoch(&mut registry, b.grant);
        let mut ticket = Some(
            registry
                .reserve_capture_image(b.grant, &b.metadata, 0)
                .unwrap(),
        );
        let supplied = storage(i);
        accept_proven_snapshot(
            b,
            LiveRendererImageId::from_raw(i),
            false,
            &mut ticket,
            &supplied.image,
            &mut registry,
            0,
        )
        .unwrap();
        budgets.push(supplied.budget);
        if i == 3 {
            assert!(registry.disconnect(b.grant));
        } else {
            registry
                .resources_mut(b.grant)
                .unwrap()
                .retire(
                    TransactionId::from_raw(8),
                    &ContentResourceRetire {
                        grant: b.grant,
                        resource: b.resource(),
                    },
                )
                .unwrap();
        }
    }
    let mut visited = Vec::new();
    registry
        .retire_capture_imports(&mut 0, &mut |_| -> Result<(), ()> { panic!("zero budget") })
        .unwrap();
    assert!(registry.capture_retirement_pending());
    registry
        .retire_capture_imports(&mut 1, &mut |image| {
            visited.push(image.image_id().raw());
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(visited, vec![1]);
    visited.clear();
    assert_eq!(
        registry.retire_capture_imports(&mut 2, &mut |image| {
            visited.push(image.image_id().raw());
            Err("retry")
        }),
        Err("retry")
    );
    assert_eq!(visited, vec![2]);
    assert!(budgets.iter().all(|b| b.usage() == (1, 64)));
    visited.clear();
    registry
        .retire_capture_imports(&mut 2, &mut |image| {
            visited.push(image.image_id().raw());
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(visited, vec![2, 3]);
    assert!(!registry.capture_retirement_pending());
    assert_eq!(budgets[2].usage(), (0, 0));
    for i in 1..=2 {
        let b = binding(i, 1);
        let store = registry.resources_mut(b.grant).unwrap();
        let mut released = 0;
        while let Some(event) = store.take_event() {
            if matches!(event.record, ShellContentRecord::ResourceReleased(_)) {
                released += 1;
            }
        }
        assert_eq!(released, 1);
        assert_eq!(budgets[i as usize - 1].usage(), (1, 64));
        store.confirm_isb_release(b.resource()).unwrap();
        assert_eq!(budgets[i as usize - 1].usage(), (0, 0));
    }
}

#[test]
fn exact_completed_storage_accepts_without_releasing_source_or_duplicating_charge() {
    let b = binding(1, 1);
    assert_eq!(b.metadata, metadata(b.grant));
    let mut registry = allocation_registry(b.grant);
    let mut ticket = Some(
        registry
            .reserve_capture_image(b.grant, &b.metadata, 0)
            .unwrap(),
    );
    let supplied = storage(17);
    accept_proven_snapshot(
        b,
        LiveRendererImageId::from_raw(17),
        false,
        &mut ticket,
        &supplied.image,
        &mut registry,
        0,
    )
    .unwrap();
    assert!(ticket.is_none());
    let store = registry.resources_mut(b.grant).unwrap();
    assert_eq!(store.usage().resident, 64);
    let lease = store.lease(b.grant, b.resource()).unwrap();
    assert!(Arc::ptr_eq(
        lease.captured_image().unwrap(),
        &supplied.image
    ));
    assert!(lease.cpu_bytes().is_none());
    assert!(lease.into_cpu().is_err());
    assert_eq!(supplied.budget.usage(), (1, 64));
    assert!(supplied.epoch.is_valid());
    assert!(
        matches!(store.take_event().unwrap().record,ShellContentRecord::ResourceStatus(s) if s.status==2)
    );
    assert!(store.take_event().is_none()); // no fabricated SourceReleased or ResourceReleased
}

#[test]
fn image_identity_connection_clock_and_withdrawal_refuse_without_consuming_completion_credit() {
    for case in 0..6 {
        let mut b = binding(1, 1);
        let mut registry = allocation_registry(b.grant);
        let mut ticket = Some(
            registry
                .reserve_capture_image(b.grant, &b.metadata, 10)
                .unwrap(),
        );
        let supplied = storage(17);
        match case {
            1 => supplied.epoch.invalidate(),
            2 => b.grant.connection_epoch += 1,
            4 => b.metadata.topology_generation += 1,
            _ => {}
        }
        let expected = if case == 0 { 18 } else { 17 };
        let result = accept_proven_snapshot(
            b,
            LiveRendererImageId::from_raw(expected),
            case == 3,
            &mut ticket,
            &supplied.image,
            &mut registry,
            if case == 5 { 9 } else { 10 },
        );
        assert!(result.is_err());
        assert!(ticket.is_some());
        assert_eq!(supplied.budget.usage(), (1, 64));
        assert_eq!(supplied.image.is_current(), case != 1);
        registry
            .fail_capture(&mut ticket, ContentCaptureFailure::Renderer, 10)
            .unwrap();
        assert!(ticket.is_none());
        assert_eq!(supplied.budget.usage(), (1, 64));
    }
}

#[test]
fn disconnected_or_changed_topology_rejects_publication_but_proven_failure_can_settle() {
    for disconnected in [false, true] {
        let b = binding(1, 1);
        let mut registry = allocation_registry(b.grant);
        let mut ticket = Some(
            registry
                .reserve_capture_image(b.grant, &b.metadata, 0)
                .unwrap(),
        );
        let supplied = storage(17);
        if disconnected {
            assert!(registry.disconnect(b.grant));
        } else {
            registry
                .allocations_mut(b.grant)
                .unwrap()
                .publish_outputs(
                    TransactionId::from_raw(20),
                    2,
                    vec![ContentOutputFactsEntry {
                        output: ContentOutputId {
                            id: 1,
                            generation: 1,
                        },
                        local_width: 200,
                        local_height: 100,
                        scale_numerator: 1,
                        scale_denominator: 1,
                        scale_generation: 1,
                    }],
                )
                .unwrap();
        }
        assert!(
            accept_proven_snapshot(
                b,
                LiveRendererImageId::from_raw(17),
                false,
                &mut ticket,
                &supplied.image,
                &mut registry,
                0
            )
            .is_err()
        );
        assert!(ticket.is_some());
        registry
            .fail_capture(&mut ticket, ContentCaptureFailure::Withdrawn, 0)
            .unwrap();
        assert!(ticket.is_none());
        assert_eq!(supplied.budget.usage(), (1, 64));
    }
}

#[test]
fn a_real_pre_use_return_cannot_enter_successful_acceptance() {
    let mut f = Fixture::new();
    let b = binding(1, 1);
    f.submit(b);
    drop(f.executor.take());
    let mut batch = f.owner.take_completed(f.now).unwrap();
    let mut receipt = batch.completed.pop().unwrap();
    assert_eq!(
        receipt.accept_completed(&mut f.registries[0], 0),
        Err(ContentStoreError::Incomplete)
    );
    assert!(receipt.reservation.is_some());
    receipt.fail_unpublished(&mut f.registries[0], 0).unwrap();
    assert!(receipt.reservation.is_none());
}

#[test]
fn same_allocation_number_and_dimensions_do_not_rebind_an_old_capture() {
    let b = binding(1, 1);
    let mut registry = allocation_registry(b.grant);
    let mut ticket = Some(
        registry
            .reserve_capture_image(b.grant, &b.metadata, 0)
            .unwrap(),
    );
    let supplied = storage(17);
    let allocations = registry.allocations_mut(b.grant).unwrap();
    let mut current = allocations.snapshots().pop().unwrap();
    allocations
        .request(
            TransactionId::from_raw(22),
            ContentAllocationRequest {
                grant: b.grant,
                output: current.output,
                allocation_request_id: 2,
                operation: 2,
                role: current.role,
                edge: current.edge,
                prior: current.allocation,
                parent: current.parent,
                parent_presentation_epoch: 0,
                anchor_parent_rect: current.anchor_parent_rect,
                desired_width: 200,
                desired_height: 20,
                margins: current.margins,
            },
            &[],
            0,
        )
        .unwrap();
    current.allocation.generation += 1;
    allocations.grant(2, current, &[]).unwrap();
    assert_eq!(
        accept_proven_snapshot(
            b,
            LiveRendererImageId::from_raw(17),
            false,
            &mut ticket,
            &supplied.image,
            &mut registry,
            0
        ),
        Err(ContentStoreError::Stale)
    );
    assert!(ticket.is_some());
    registry
        .fail_capture(&mut ticket, ContentCaptureFailure::Withdrawn, 0)
        .unwrap();
    assert!(ticket.is_none());
    assert_eq!(supplied.budget.usage(), (1, 64));
}
