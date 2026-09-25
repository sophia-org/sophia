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
