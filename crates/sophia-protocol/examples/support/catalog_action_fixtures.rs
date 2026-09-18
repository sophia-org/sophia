use sophia_protocol::*;

pub fn action() -> ContentAction {
    ContentAction {
        grant: ContentGrant {
            connection_epoch: 2,
            content_grant_epoch: 3,
        },
        output: ContentOutputId {
            id: 1,
            generation: 2,
        },
        candidate_generation: 4,
        presentation_epoch: 5,
        interaction_generation: 6,
        allocation: ContentAllocationId {
            id: 7,
            generation: 1,
        },
        target_id: 8,
        target_generation: 1,
        action_id: 9,
        event_id: 10,
        kind: 1,
        reason: 0,
    }
}
pub fn records() -> Vec<ShellCatalogActionRecord> {
    let action = action();
    vec![
        ShellCatalogActionRecord::Identity(ShellCatalogIdentity {
            connection_epoch: 2,
            catalog_generation: 11,
            slot: 9,
            identity: "registered:terminal".into(),
        }),
        ShellCatalogActionRecord::CandidateBegin(CatalogCandidateBegin {
            content: ContentCandidateBegin {
                grant: action.grant,
                candidate_generation: 4,
                output: action.output,
                facts_generation: 1,
                pacing_permit: 2,
                interaction_generation: 6,
                surface_count: 1,
                placement_count: 0,
                target_count: 1,
            },
            catalog_generation: 11,
        }),
        ShellCatalogActionRecord::CandidateChunk(ContentCandidateChunk {
            chunk_ordinal: 0,
            grant: action.grant,
            candidate_generation: 4,
            surfaces: vec![ContentSurface {
                allocation: action.allocation,
                scale_generation: 1,
                role: 1,
                edge: 3,
                margins: ContentMargins::default(),
                reservation_extent: 64,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: ContentPixelRect::default(),
            }],
            placements: vec![],
            targets: vec![ContentTarget {
                surface_index: 0,
                action_kind: 3,
                target_id: 8,
                target_generation: 1,
                action_id: 9,
                bounds_px: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: 48,
                    height: 48,
                },
            }],
        }),
        ShellCatalogActionRecord::Activate(CatalogActivation {
            action: action.clone(),
            catalog_generation: 11,
        }),
        ShellCatalogActionRecord::ActivationOutcome(CatalogActivationOutcome {
            activation: CatalogActivation {
                action,
                catalog_generation: 11,
            },
            status: 1,
            reason: 0,
        }),
    ]
}
