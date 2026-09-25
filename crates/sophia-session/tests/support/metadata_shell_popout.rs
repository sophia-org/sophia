use super::*;

fn fixture() -> (
    LiveContentSession,
    ContentAllocationRequest,
    ContentAllocationSnapshot,
    HeadlessOutput,
) {
    let mut parent = allocation();
    parent.pixel = ContentPixelRect {
        x: 0,
        y: 0,
        width: 200,
        height: 200,
    };
    parent.logical = ContentLogicalRect {
        x: 0,
        y: 0,
        width: 200,
        height: 200,
    };
    let request = ContentAllocationRequest {
        grant: GRANT,
        output: OUTPUT,
        allocation_request_id: 1,
        operation: 1,
        role: 2,
        edge: 1,
        prior: ContentAllocationId::default(),
        parent: parent.allocation,
        parent_presentation_epoch: 8,
        anchor_parent_rect: ContentPixelRect {
            x: 90,
            y: 90,
            width: 20,
            height: 20,
        },
        desired_width: 20,
        desired_height: 10,
        margins: ContentMargins::default(),
    };
    (
        LiveContentSession::new(true, false, Some(32)),
        request,
        parent,
        HeadlessOutput {
            id: OutputId::from_raw(2),
            size: Size {
                width: 200,
                height: 200,
            },
            scale: 1,
        },
    )
}

#[test]
fn equal_room_places_away_from_each_panel_edge() {
    let (mut session, mut request, parent, output) = fixture();
    for (edge, x, y) in [(1, 90, 110), (2, 70, 90), (3, 90, 80), (4, 110, 90)] {
        request.edge = edge;
        let resolved = session
            .resolve_allocation(&request, &[output], std::slice::from_ref(&parent))
            .unwrap();
        assert_eq!(
            (resolved.pixel.x, resolved.pixel.y),
            (x, y),
            "panel edge {edge}"
        );
        assert_eq!(resolved.anchor_parent_rect, request.anchor_parent_rect);
        assert_eq!(resolved.allowed_reservation_extent, 0);
    }
}

#[test]
fn negative_margins_are_scaled_without_rounding_the_physical_anchor() {
    let (mut session, mut request, mut parent, mut output) = fixture();
    output.scale = 2;
    parent.scale_numerator = 2;
    parent.logical.width = 100;
    parent.logical.height = 100;
    request.anchor_parent_rect.x = 89;
    request.anchor_parent_rect.width = 22;
    request.margins = ContentMargins {
        top: -1,
        right: -1,
        bottom: -1,
        left: -1,
    };
    let resolved = session
        .resolve_allocation(&request, &[output], &[parent])
        .unwrap();
    assert_eq!(
        resolved.pixel,
        ContentPixelRect {
            x: 87,
            y: 108,
            width: 40,
            height: 20
        }
    );
    assert_eq!(resolved.logical.x, 43);
}

#[test]
fn refused_popouts_do_not_clip_or_consume_an_allocation_identity() {
    use sophia_runtime::ContentAllocationError;
    let (mut session, request, parent, output) = fixture();
    let initial_id = session.next_allocation_id;
    let mut oversized = request.clone();
    oversized.desired_width = 201;
    oversized.desired_height = 201;
    assert_eq!(
        session.resolve_allocation(&oversized, &[output], std::slice::from_ref(&parent)),
        Err(ContentAllocationError::Budget)
    );
    let mut outside_anchor = request.clone();
    outside_anchor.anchor_parent_rect.x = 199;
    assert_eq!(
        session.resolve_allocation(&outside_anchor, &[output], std::slice::from_ref(&parent)),
        Err(ContentAllocationError::Malformed)
    );
    assert_eq!(
        session.resolve_allocation(&request, &[output], &[]),
        Err(ContentAllocationError::AllocationLost)
    );
    let mut foreign_parent = parent.clone();
    foreign_parent.output.generation += 1;
    assert_eq!(
        session.resolve_allocation(&request, &[output], &[foreign_parent]),
        Err(ContentAllocationError::AllocationLost)
    );
    assert_eq!(session.next_allocation_id, initial_id);
    let admitted = session
        .resolve_allocation(&request, &[output], &[parent])
        .unwrap();
    assert_eq!(admitted.allocation.id, initial_id);
}

#[test]
fn fractional_negative_margins_use_the_acknowledged_parent_scale() {
    for (numerator, denominator, width, height, logical_x) in
        [(3, 2, 30, 15, 58), (7, 4, 35, 18, 49)]
    {
        let (mut session, mut request, mut parent, output) = fixture();
        parent.scale_numerator = numerator;
        parent.scale_denominator = denominator;
        request.anchor_parent_rect.x = 89;
        request.anchor_parent_rect.width = 22;
        request.margins = ContentMargins {
            top: -1,
            right: -1,
            bottom: -1,
            left: -1,
        };
        let resolved = session
            .resolve_allocation(&request, &[output], &[parent])
            .unwrap();
        assert_eq!(
            resolved.pixel,
            ContentPixelRect {
                x: 87,
                y: 108,
                width,
                height
            }
        );
        assert_eq!(resolved.logical.x, logical_x);
        assert_eq!(
            (resolved.scale_numerator, resolved.scale_denominator),
            (numerator, denominator)
        );
    }
}

#[test]
fn fractional_owner_placement_is_accepted_by_the_allocation_store() {
    use sophia_runtime::ContentAllocationStore;
    let (mut session, mut request, mut parent, output) = fixture();
    parent.scale_numerator = 5;
    parent.scale_denominator = 4;
    parent.logical = ContentLogicalRect {
        x: 0,
        y: 0,
        width: 160,
        height: 20,
    };
    parent.pixel = ContentPixelRect {
        x: 0,
        y: 0,
        width: 200,
        height: 25,
    };
    parent.allowed_reservation_extent = 25;
    request.allocation_request_id = 2;
    request.desired_width = 2;
    request.desired_height = 2;
    request.anchor_parent_rect = ContentPixelRect {
        x: 4,
        y: 0,
        width: 1,
        height: 1,
    };
    session.next_allocation_id = 2;
    let resolved = session
        .resolve_allocation(&request, &[output], std::slice::from_ref(&parent))
        .unwrap();
    assert_eq!(
        resolved.pixel,
        ContentPixelRect {
            x: 4,
            y: 1,
            width: 3,
            height: 3
        }
    );
    assert_eq!(resolved.logical.x, 3);

    let mut store = ContentAllocationStore::new(ContentLimits::prototype(GRANT)).unwrap();
    store
        .publish_outputs(
            tx(1),
            1,
            vec![ContentOutputFactsEntry {
                output: OUTPUT,
                local_width: 160,
                local_height: 160,
                scale_numerator: 5,
                scale_denominator: 4,
                scale_generation: parent.scale_generation,
            }],
        )
        .unwrap();
    let mut parent_request = request.clone();
    parent_request.allocation_request_id = 1;
    parent_request.role = 1;
    parent_request.parent = ContentAllocationId::default();
    parent_request.parent_presentation_epoch = 0;
    parent_request.anchor_parent_rect = ContentPixelRect::default();
    parent_request.desired_width = 160;
    parent_request.desired_height = 20;
    store.request(tx(2), parent_request, &[], 0).unwrap();
    store.grant(1, parent, &[]).unwrap();
    store
        .request(tx(3), request, &[(ALLOCATION, 8)], 1)
        .unwrap();
    store.grant(2, resolved, &[(ALLOCATION, 8)]).unwrap();
}
