// The neutral shell content records, one per record kind, with their grant.
// Shared by the socket corpus fixtures and the no-IPC value tests.
use sophia_protocol::*;

pub fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 11,
        content_grant_epoch: 3,
    }
}

pub fn fixtures() -> Vec<ShellContentRecord> {
    let g = grant();
    let o = ContentOutputId {
        id: 2,
        generation: 7,
    };
    let a = ContentAllocationId {
        id: 1,
        generation: 2,
    };
    let r = ContentResourceId {
        id: 9,
        generation: 1,
    };
    let pixel = ContentPixelRect {
        x: 0,
        y: 0,
        width: 2,
        height: 1,
    };
    let logical = ContentLogicalRect {
        x: 0,
        y: 0,
        width: 2,
        height: 1,
    };
    let margins = ContentMargins::default();
    let none = ContentAllocationId::default();
    vec![
        ShellContentRecord::AdmissionRefused(ContentAdmissionRefused {
            reason: 1,
            denied_capabilities: 128,
        }),
        ShellContentRecord::Limits(ContentLimits::prototype(g)),
        ShellContentRecord::OutputFacts(ContentOutputFacts {
            grant: g,
            facts_generation: 1,
            outputs: vec![ContentOutputFactsEntry {
                output: o,
                local_width: 1920,
                local_height: 1080,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        }),
        ShellContentRecord::AllocationRequest(ContentAllocationRequest {
            grant: g,
            output: o,
            allocation_request_id: 1,
            operation: 1,
            role: 1,
            edge: 1,
            prior: none,
            parent: none,
            parent_presentation_epoch: 0,
            anchor_parent_rect: ContentPixelRect::default(),
            desired_width: 2,
            desired_height: 1,
            margins,
        }),
        ShellContentRecord::AllocationResult(ContentAllocationResult {
            grant: g,
            output: o,
            allocation_request_id: 1,
            status: 1,
            reason: 0,
            allocation: a,
            parent: none,
            scale_generation: 1,
            logical,
            pixel,
            scale_numerator: 1,
            scale_denominator: 1,
            allowed_reservation_extent: 1,
            margins,
            acknowledged_anchor: ContentPixelRect::default(),
        }),
        ShellContentRecord::ResourceBegin(ContentResourceBegin {
            grant: g,
            resource: r,
            width_px: 2,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 8,
        }),
        ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: g,
            resource: r,
            status: 1,
            reason: 0,
            next_ordinal: 0,
            admitted_bytes: 8,
        }),
        ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant: g,
            resource: r,
            ordinal: 0,
            offset: 0,
            bytes: vec![0, 0, 255, 255, 0, 128, 0, 128],
        }),
        ShellContentRecord::ResourceEnd(ContentResourceEnd {
            grant: g,
            resource: r,
            total_bytes: 8,
            chunk_count: 1,
        }),
        ShellContentRecord::ResourceCancel(ContentResourceCancel {
            grant: g,
            resource: r,
        }),
        ShellContentRecord::ResourceRetire(ContentResourceRetire {
            grant: g,
            resource: r,
        }),
        ShellContentRecord::ResourceReleased(ContentResourceReleased {
            grant: g,
            resource: r,
            reason: 0,
        }),
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant: g,
            output: o,
            candidate_generation: 4,
            facts_generation: 1,
            pacing_permit: 2,
            interaction_generation: 4,
            surface_count: 1,
            placement_count: 1,
            target_count: 1,
        }),
        ShellContentRecord::CandidateChunk(ContentCandidateChunk {
            grant: g,
            candidate_generation: 4,
            chunk_ordinal: 0,
            surfaces: vec![ContentSurface {
                allocation: a,
                scale_generation: 1,
                role: 1,
                edge: 1,
                margins,
                reservation_extent: 1,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: ContentPixelRect::default(),
            }],
            placements: vec![ContentPlacement {
                resource: r,
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
            targets: vec![ContentTarget {
                surface_index: 0,
                action_kind: 1,
                target_id: 1,
                target_generation: 2,
                action_id: 3,
                bounds_px: pixel,
            }],
        }),
        ShellContentRecord::CandidateEnd(ContentCandidateEnd {
            grant: g,
            candidate_generation: 4,
            surface_count: 1,
            placement_count: 1,
            target_count: 1,
        }),
        ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
            grant: g,
            output: o,
            candidate_generation: 4,
            kind: 2,
            reason: 0,
            presentation_epoch: 5,
            work_area_generation: 6,
            wm_commit_generation: 7,
        }),
        ShellContentRecord::FrameDemand(ContentFrameDemand {
            grant: g,
            output: o,
            allocation: none,
            demand_id: 1,
            reason: 1,
        }),
        ShellContentRecord::FramePermit(ContentFramePermit {
            grant: g,
            output: o,
            demand_id: 1,
            permit_id: 2,
            state: 1,
            reason: 0,
            ttl_ms: 250,
            max_candidate_bytes: 8192,
        }),
        ShellContentRecord::FrameDemandCancel(ContentFrameDemandCancel {
            grant: g,
            output: o,
            demand_id: 1,
            permit_id: 2,
        }),
        ShellContentRecord::Action(ContentAction {
            grant: g,
            output: o,
            candidate_generation: 4,
            presentation_epoch: 5,
            interaction_generation: 4,
            allocation: a,
            target_id: 1,
            target_generation: 2,
            action_id: 3,
            event_id: 8,
            kind: 1,
            reason: 0,
        }),
        ShellContentRecord::ActionAck(ContentActionAck {
            grant: g,
            output: o,
            candidate_generation: 4,
            presentation_epoch: 5,
            interaction_generation: 4,
            allocation: a,
            target_id: 1,
            target_generation: 2,
            action_id: 3,
            event_id: 8,
            disposition: 1,
        }),
    ]
}
