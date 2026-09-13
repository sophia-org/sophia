#![cfg(test)]

use super::*;
use sophia_protocol::*;
use sophia_runtime::{ContentCandidateContext, ContentCandidateStore, ContentResourceStore};

const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 7,
    content_grant_epoch: 9,
};
const OUTPUT: ContentOutputId = ContentOutputId {
    id: 2,
    generation: 3,
};
const ALLOCATION: ContentAllocationId = ContentAllocationId {
    id: 1,
    generation: 1,
};
const RESOURCE: ContentResourceId = ContentResourceId {
    id: 1,
    generation: 1,
};

fn tx(raw: u64) -> TransactionId {
    TransactionId::from_raw(raw)
}

#[test]
fn output_facts_publish_logical_extents_and_refuse_lossy_scale_conversion() {
    let facts = output_facts_entry(HeadlessOutput {
        id: OutputId::from_raw(8),
        size: Size {
            width: 2560,
            height: 1440,
        },
        scale: 2,
    })
    .unwrap();
    assert_eq!((facts.local_width, facts.local_height), (1280, 720));
    assert_eq!((facts.scale_numerator, facts.scale_denominator), (2, 1));
    assert!(
        output_facts_entry(HeadlessOutput {
            id: OutputId::from_raw(8),
            size: Size {
                width: 2559,
                height: 1440,
            },
            scale: 2,
        })
        .is_err()
    );
}

#[test]
fn panel_allowance_is_enforced_against_resolved_physical_thickness() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(2),
        size: Size {
            width: 200,
            height: 100,
        },
        scale: 2,
    };
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
        desired_width: 100,
        desired_height: 16,
        margins: ContentMargins::default(),
    };
    let mut session = LiveContentSession::new(true, true, Some(32));
    let resolved = session
        .resolve_allocation(&request, &[output], &[])
        .unwrap();
    assert_eq!(resolved.pixel.height, 32);
    assert_eq!(resolved.allowed_reservation_extent, 32);

    let mut oversized = request;
    oversized.desired_height = 17;
    assert_eq!(
        session.resolve_allocation(&oversized, &[output], &[]),
        Err(sophia_runtime::ContentAllocationError::Budget)
    );
}

fn allocation() -> ContentAllocationSnapshot {
    ContentAllocationSnapshot {
        output: OUTPUT,
        allocation: ALLOCATION,
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
            x: 10,
            y: 20,
            width: 64,
            height: 32,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: 32,
    }
}

fn render_bundle() -> ContentRenderBundle {
    let limits = ContentLimits::prototype(GRANT);
    let mut resources = ContentResourceStore::new(limits.clone()).unwrap();
    resources
        .begin(
            tx(1),
            ContentResourceBegin {
                grant: GRANT,
                resource: RESOURCE,
                width_px: 2,
                height_px: 1,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 8,
            },
            0,
        )
        .unwrap();
    resources
        .chunk(
            tx(2),
            &ContentResourceChunk {
                grant: GRANT,
                resource: RESOURCE,
                ordinal: 0,
                offset: 0,
                bytes: vec![0, 0, 255, 255, 0, 128, 0, 128],
            },
            0,
        )
        .unwrap();
    resources
        .end(
            tx(3),
            &ContentResourceEnd {
                grant: GRANT,
                resource: RESOURCE,
                total_bytes: 8,
                chunk_count: 1,
            },
            0,
        )
        .unwrap();
    let mut candidates = ContentCandidateStore::new(limits).unwrap();
    candidates.grant_permit(tx(4), OUTPUT, 1, 1, 0).unwrap();
    candidates
        .begin(
            tx(5),
            ContentCandidateBegin {
                grant: GRANT,
                candidate_generation: 1,
                output: OUTPUT,
                facts_generation: 6,
                pacing_permit: 1,
                interaction_generation: 8,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            },
            1,
        )
        .unwrap();
    candidates
        .chunk(
            tx(6),
            ContentCandidateChunk {
                grant: GRANT,
                candidate_generation: 1,
                chunk_ordinal: 0,
                surfaces: vec![ContentSurface {
                    allocation: ALLOCATION,
                    scale_generation: 4,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    reservation_extent: 24,
                    parent_surface_index: u16::MAX,
                    anchor_parent_rect: ContentPixelRect::default(),
                }],
                placements: vec![ContentPlacement {
                    resource: RESOURCE,
                    surface_index: 0,
                    destination_x_px: 3,
                    destination_y_px: 4,
                }],
                targets: vec![],
            },
            2,
        )
        .unwrap();
    let allocation = allocation();
    candidates
        .end(
            tx(7),
            ContentCandidateEnd {
                grant: GRANT,
                candidate_generation: 1,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            },
            ContentCandidateContext {
                output: OUTPUT,
                facts_generation: 6,
                interaction_generation: 8,
                allocations: std::slice::from_ref(&allocation),
            },
            &resources,
            3,
        )
        .unwrap();
    candidates.begin_submission(OUTPUT, 1, 4).unwrap()
}

#[test]
fn projection_keeps_exact_pixels_and_allocation_local_placement() {
    let bundle = render_bundle();
    let projected = project_render_bundle(
        &bundle,
        HeadlessOutput {
            id: OutputId::from_raw(2),
            size: Size {
                width: 1920,
                height: 1080,
            },
            scale: 1,
        },
        OUTPUT,
        &[allocation()],
    )
    .unwrap();
    assert_eq!(projected.candidate_generation, 1);
    assert_eq!(projected.images.len(), 1);
    assert_eq!(
        projected.images[0].geometry_px,
        Rect {
            x: 13,
            y: 24,
            width: 2,
            height: 1,
        }
    );
    assert_eq!(projected.images[0].resource.bytes().len(), 8);
}

#[test]
fn a_scaled_popout_keeps_the_exact_physical_anchor_origin() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(2),
        size: Size {
            width: 200,
            height: 100,
        },
        scale: 2,
    };
    let parent_id = ContentAllocationId {
        id: 5,
        generation: 1,
    };
    let parent = ContentAllocationSnapshot {
        output: OUTPUT,
        allocation: parent_id,
        scale_generation: 1,
        scale_numerator: 2,
        scale_denominator: 1,
        role: 1,
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 100,
            height: 16,
        },
        pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 200,
            height: 32,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: 16,
    };
    let request = ContentAllocationRequest {
        grant: GRANT,
        output: OUTPUT,
        allocation_request_id: 1,
        operation: 1,
        role: 2,
        edge: 1,
        prior: ContentAllocationId::default(),
        parent: parent_id,
        parent_presentation_epoch: 8,
        anchor_parent_rect: ContentPixelRect {
            x: 11,
            y: 0,
            width: 10,
            height: 10,
        },
        desired_width: 20,
        desired_height: 10,
        margins: ContentMargins::default(),
    };
    let mut session = LiveContentSession::new(true, true, Some(32));
    let resolved = session
        .resolve_allocation(&request, &[output], &[parent])
        .unwrap();

    assert_eq!(resolved.pixel.x, 21);
    assert_eq!(resolved.pixel.y, 0);
    assert_eq!(resolved.pixel.width, 40);
    assert_eq!(resolved.pixel.height, 20);
    assert_eq!(resolved.logical.x, 10);
    assert_eq!(resolved.allowed_reservation_extent, 0);
}
