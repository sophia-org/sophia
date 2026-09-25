use sophia_protocol::*;
use sophia_runtime::*;

fn tx(value: u64) -> TransactionId {
    TransactionId::from_raw(value)
}

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    }
}

fn output() -> ContentOutputId {
    ContentOutputId {
        id: 2,
        generation: 1,
    }
}

fn facts(height: u32) -> ContentOutputFactsEntry {
    ContentOutputFactsEntry {
        output: output(),
        local_width: 200,
        local_height: height,
        scale_numerator: 1,
        scale_denominator: 1,
        scale_generation: 1,
    }
}

fn panel_request(id: u64, operation: u16, prior: ContentAllocationId) -> ContentAllocationRequest {
    ContentAllocationRequest {
        grant: grant(),
        output: output(),
        allocation_request_id: id,
        operation,
        role: 1,
        edge: 1,
        prior,
        parent: ContentAllocationId::default(),
        parent_presentation_epoch: 0,
        anchor_parent_rect: ContentPixelRect::default(),
        desired_width: if operation == 3 { 0 } else { 200 },
        desired_height: if operation == 3 { 0 } else { 20 },
        margins: ContentMargins::default(),
    }
}

fn panel(allocation: ContentAllocationId) -> ContentAllocationSnapshot {
    ContentAllocationSnapshot {
        native_opening: None,
        output: output(),
        allocation,
        scale_generation: 1,
        scale_numerator: 1,
        scale_denominator: 1,
        role: 1,
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 200,
            height: 20,
        },
        pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 200,
            height: 20,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: 20,
    }
}

fn store() -> ContentAllocationStore {
    let mut store = ContentAllocationStore::new(ContentLimits::prototype(grant())).unwrap();
    store.publish_outputs(tx(1), 1, vec![facts(100)]).unwrap();
    store.take_event().unwrap();
    store
}

#[test]
fn output_facts_and_an_exact_engine_resolution_create_one_allocation() {
    let mut store = store();
    let request = panel_request(1, 1, ContentAllocationId::default());
    store.request(tx(2), request.clone(), &[], 0).unwrap();
    assert_eq!(store.pending_request(), Some((tx(2), request)));
    let allocation = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    store.grant(1, panel(allocation), &[]).unwrap();
    assert_eq!(store.snapshots(), vec![panel(allocation)]);
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::AllocationResult(ContentAllocationResult {
            status: 1,
            allocation: actual,
            ..
        }) if actual == allocation
    ));
}

#[test]
fn unknown_outputs_and_inexact_engine_resolutions_are_refused_without_losing_the_request() {
    let mut store = store();
    let mut request = panel_request(1, 1, ContentAllocationId::default());
    request.output.id = 9;
    assert_eq!(
        store.request(tx(2), request, &[], 0),
        Err(ContentAllocationError::OutputLost)
    );
    let request = panel_request(2, 1, ContentAllocationId::default());
    store.request(tx(3), request.clone(), &[], 0).unwrap();
    let mut wrong = panel(ContentAllocationId {
        id: 1,
        generation: 1,
    });
    wrong.logical.height = 21;
    assert_eq!(
        store.grant(2, wrong, &[]),
        Err(ContentAllocationError::Malformed)
    );
    assert_eq!(store.pending_request(), Some((tx(3), request)));
}

#[test]
fn coverage_and_role_limits_are_checked_on_resolved_allocations() {
    let mut store = store();
    let mut request = panel_request(1, 1, ContentAllocationId::default());
    request.desired_height = 60;
    store.request(tx(2), request, &[], 0).unwrap();
    let mut over_coverage = panel(ContentAllocationId {
        id: 1,
        generation: 1,
    });
    over_coverage.logical.height = 60;
    over_coverage.pixel.height = 60;
    assert_eq!(
        store.grant(1, over_coverage, &[]),
        Err(ContentAllocationError::Budget)
    );
}

#[test]
fn a_panel_cannot_authorize_more_reservation_than_its_thickness() {
    let mut store = store();
    store
        .request(
            tx(2),
            panel_request(1, 1, ContentAllocationId::default()),
            &[],
            0,
        )
        .unwrap();
    let mut over_reservation = panel(ContentAllocationId {
        id: 1,
        generation: 1,
    });
    over_reservation.allowed_reservation_extent = 21;
    assert_eq!(
        store.grant(1, over_reservation, &[]),
        Err(ContentAllocationError::Budget)
    );
}

#[test]
fn scaled_output_bounds_and_reservation_use_resolved_physical_pixels() {
    let mut store = ContentAllocationStore::new(ContentLimits::prototype(grant())).unwrap();
    let mut scaled_facts = facts(100);
    scaled_facts.scale_numerator = 2;
    store.publish_outputs(tx(1), 1, vec![scaled_facts]).unwrap();
    store.take_event().unwrap();
    let request = panel_request(1, 1, ContentAllocationId::default());
    store.request(tx(2), request, &[], 0).unwrap();
    let mut scaled = panel(ContentAllocationId {
        id: 1,
        generation: 1,
    });
    scaled.scale_numerator = 2;
    scaled.pixel.width = 400;
    scaled.pixel.height = 40;
    scaled.allowed_reservation_extent = 40;
    store.grant(1, scaled.clone(), &[]).unwrap();
    assert_eq!(store.snapshots(), vec![scaled]);
}

#[test]
fn replace_advances_the_exact_allocation_generation_and_release_is_terminal() {
    let mut store = store();
    let first = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    store
        .request(
            tx(2),
            panel_request(1, 1, ContentAllocationId::default()),
            &[],
            0,
        )
        .unwrap();
    store.grant(1, panel(first), &[]).unwrap();
    store.take_event().unwrap();

    store
        .request(tx(3), panel_request(2, 2, first), &[], 1)
        .unwrap();
    let replacement = ContentAllocationId {
        id: 1,
        generation: 2,
    };
    store.grant(2, panel(replacement), &[]).unwrap();
    assert_eq!(store.snapshots(), vec![panel(replacement)]);
    store.take_event().unwrap();

    store
        .request(tx(4), panel_request(3, 3, replacement), &[], 2)
        .unwrap();
    store.release(3).unwrap();
    assert!(store.snapshots().is_empty());
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::AllocationResult(ContentAllocationResult {
            status: 3,
            allocation,
            ..
        }) if allocation == replacement
    ));
}

#[test]
fn a_popout_requires_the_exact_presented_parent_epoch() {
    let mut store = store();
    let parent = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    store
        .request(
            tx(2),
            panel_request(1, 1, ContentAllocationId::default()),
            &[],
            0,
        )
        .unwrap();
    store.grant(1, panel(parent), &[]).unwrap();
    store.take_event().unwrap();
    let request = ContentAllocationRequest {
        grant: grant(),
        output: output(),
        allocation_request_id: 2,
        operation: 1,
        role: 2,
        edge: 1,
        prior: ContentAllocationId::default(),
        parent,
        parent_presentation_epoch: 7,
        anchor_parent_rect: ContentPixelRect {
            x: 10,
            y: 0,
            width: 20,
            height: 20,
        },
        desired_width: 50,
        desired_height: 40,
        margins: ContentMargins::default(),
    };
    assert_eq!(
        store.request(tx(3), request.clone(), &[(parent, 6)], 1),
        Err(ContentAllocationError::Stale)
    );
    let mut retry = request;
    retry.allocation_request_id = 3;
    store
        .request(tx(4), retry.clone(), &[(parent, 7)], 1)
        .unwrap();
    let popout = ContentAllocationSnapshot {
        native_opening: None,
        output: output(),
        allocation: ContentAllocationId {
            id: 2,
            generation: 1,
        },
        scale_generation: 1,
        scale_numerator: 1,
        scale_denominator: 1,
        role: 2,
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 10,
            y: 20,
            width: 50,
            height: 40,
        },
        pixel: ContentPixelRect {
            x: 10,
            y: 20,
            width: 50,
            height: 40,
        },
        parent,
        anchor_parent_rect: retry.anchor_parent_rect,
        allowed_reservation_extent: 0,
    };
    assert_eq!(
        store.grant(3, popout.clone(), &[]),
        Err(ContentAllocationError::Stale)
    );
    store.grant(3, popout, &[(parent, 7)]).unwrap();
    assert_eq!(
        store.request(tx(5), panel_request(4, 3, parent), &[], 2),
        Err(ContentAllocationError::Stale)
    );
}

#[test]
fn a_popout_origin_is_exact_physical_even_when_logical_coordinates_cannot_name_it() {
    for (physical_x, logical_x, width, height, pixel_width, pixel_height) in
        [(1, 0, 40, 24, 50, 30), (4, 3, 2, 2, 3, 3)]
    {
        let mut store = ContentAllocationStore::new(ContentLimits::prototype(grant())).unwrap();
        store
            .publish_outputs(
                tx(1),
                1,
                vec![ContentOutputFactsEntry {
                    output: output(),
                    local_width: 250,
                    local_height: 125,
                    scale_numerator: 5,
                    scale_denominator: 4,
                    scale_generation: 1,
                }],
            )
            .unwrap();
        store.take_event();
        let parent = ContentAllocationId {
            id: 1,
            generation: 1,
        };
        let mut panel_request = panel_request(1, 1, ContentAllocationId::default());
        panel_request.desired_width = 200;
        panel_request.desired_height = 20;
        store.request(tx(2), panel_request, &[], 0).unwrap();
        let mut panel = panel(parent);
        panel.logical.width = 200;
        panel.logical.height = 20;
        panel.pixel.width = 250;
        panel.pixel.height = 25;
        panel.scale_numerator = 5;
        panel.scale_denominator = 4;
        store.grant(1, panel, &[]).unwrap();
        store.take_event();

        let request = ContentAllocationRequest {
            grant: grant(),
            output: output(),
            allocation_request_id: 2,
            operation: 1,
            role: 2,
            edge: 1,
            prior: ContentAllocationId::default(),
            parent,
            parent_presentation_epoch: 7,
            anchor_parent_rect: ContentPixelRect {
                x: physical_x,
                y: 0,
                width: 1,
                height: 1,
            },
            desired_width: width,
            desired_height: height,
            margins: ContentMargins::default(),
        };
        store
            .request(tx(3), request.clone(), &[(parent, 7)], 1)
            .unwrap();
        let popout = ContentAllocationSnapshot {
            native_opening: None,
            output: output(),
            allocation: ContentAllocationId {
                id: 2,
                generation: 1,
            },
            scale_generation: 1,
            scale_numerator: 5,
            scale_denominator: 4,
            role: 2,
            edge: 1,
            margins: ContentMargins::default(),
            // The exact physical origin need not have an integer logical representation.
            logical: ContentLogicalRect {
                x: logical_x,
                y: 0,
                width,
                height,
            },
            pixel: ContentPixelRect {
                x: physical_x,
                y: 1,
                width: pixel_width,
                height: pixel_height,
            },
            parent,
            anchor_parent_rect: request.anchor_parent_rect,
            allowed_reservation_extent: 0,
        };
        store.grant(2, popout, &[(parent, 7)]).unwrap();
    }
}

#[test]
fn allocation_pixels_use_outward_endpoint_quantization() {
    let mut store = ContentAllocationStore::new(ContentLimits::prototype(grant())).unwrap();
    store
        .publish_outputs(
            tx(1),
            1,
            vec![ContentOutputFactsEntry {
                output: output(),
                local_width: 100,
                local_height: 100,
                scale_numerator: 5,
                scale_denominator: 4,
                scale_generation: 2,
            }],
        )
        .unwrap();
    store.take_event().unwrap();
    let mut request = panel_request(1, 1, ContentAllocationId::default());
    request.desired_width = 4;
    request.desired_height = 1;
    store.request(tx(2), request, &[], 0).unwrap();
    let mut resolved = panel(ContentAllocationId {
        id: 1,
        generation: 1,
    });
    resolved.scale_generation = 2;
    resolved.scale_numerator = 5;
    resolved.scale_denominator = 4;
    resolved.logical = ContentLogicalRect {
        x: 1,
        y: 0,
        width: 4,
        height: 1,
    };
    resolved.pixel = ContentPixelRect {
        x: 1,
        y: 0,
        width: 6,
        height: 2,
    };
    resolved.allowed_reservation_extent = 1;
    store.grant(1, resolved, &[]).unwrap();
}

#[test]
fn allocation_timeout_and_topology_replacement_are_explicit() {
    let mut store = store();
    store
        .request(
            tx(2),
            panel_request(1, 1, ContentAllocationId::default()),
            &[],
            0,
        )
        .unwrap();
    assert_eq!(
        store.publish_outputs(tx(3), 2, vec![facts(120)]),
        Err(ContentAllocationError::AllocationLost)
    );
    store.expire(1_000).unwrap();
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::AllocationResult(ContentAllocationResult {
            status: 2,
            reason,
            ..
        }) if reason == ContentReason::Timeout as u16
    ));
    store.publish_outputs(tx(4), 2, vec![facts(120)]).unwrap();
}

#[test]
fn topology_change_requires_a_visible_allocation_invalidation_first() {
    let mut store = store();
    let allocation = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    store
        .request(
            tx(2),
            panel_request(1, 1, ContentAllocationId::default()),
            &[],
            0,
        )
        .unwrap();
    store.grant(1, panel(allocation), &[]).unwrap();
    store.take_event().unwrap();
    assert_eq!(
        store.publish_outputs(tx(3), 2, vec![facts(120)]),
        Err(ContentAllocationError::AllocationLost)
    );
    store
        .invalidate(tx(4), allocation, ContentReason::OutputLost)
        .unwrap();
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::AllocationResult(ContentAllocationResult {
            allocation_request_id: 0,
            status: 4,
            allocation: actual,
            reason,
            ..
        }) if actual == allocation && reason == ContentReason::OutputLost as u16
    ));
    store.publish_outputs(tx(5), 2, vec![facts(120)]).unwrap();
}
