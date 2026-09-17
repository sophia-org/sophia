use sophia_protocol::*;

pub fn binding() -> NativeLauncherBinding {
    NativeLauncherBinding {
        grant: ContentGrant {
            connection_epoch: 2,
            content_grant_epoch: 3,
        },
        opening: 4,
        output: ContentOutputId {
            id: 5,
            generation: 6,
        },
        allocation: ContentAllocationId {
            id: 7,
            generation: 8,
        },
        catalog_generation: 9,
        candidate_generation: 10,
        presentation_epoch: 11,
        interaction_generation: 12,
        state_revision: 13,
        focus_lease: 14,
    }
}

pub fn fixtures() -> Vec<ShellNativeLauncherRecord> {
    use ShellNativeLauncherRecord::*;
    let b = binding();
    let event = NativeLauncherEvent {
        binding: b,
        event_id: 15,
        state_revision: b.state_revision,
    };
    let activation = NativeLauncherActivation {
        event,
        cause: 1,
        slot: 2,
    };
    vec![
        Opening(NativeLauncherOpening {
            grant: b.grant,
            opening: b.opening,
            output: b.output,
            catalog_generation: b.catalog_generation,
            state_revision: 1,
        }),
        AllocationRequest(NativeLauncherAllocationRequest {
            grant: b.grant,
            opening: b.opening,
            output: b.output,
            request_id: 16,
            prior: ContentAllocationId::default(),
            operation: 1,
            edge: 1,
            desired_width: 640,
            desired_height: 240,
            margins: ContentMargins::default(),
        }),
        CandidateBegin(NativeLauncherCandidateBegin {
            content: ContentCandidateBegin {
                grant: b.grant,
                candidate_generation: b.candidate_generation,
                output: b.output,
                facts_generation: 17,
                pacing_permit: 18,
                interaction_generation: b.interaction_generation,
                surface_count: 1,
                placement_count: 1,
                target_count: 1,
            },
            opening: b.opening,
            catalog_generation: b.catalog_generation,
            state_revision: b.state_revision,
            selected: 2,
            rows: vec![2],
        }),
        CandidateChunk(ContentCandidateChunk {
            grant: b.grant,
            candidate_generation: b.candidate_generation,
            chunk_ordinal: 0,
            surfaces: vec![ContentSurface {
                allocation: b.allocation,
                scale_generation: 19,
                role: 3,
                edge: 1,
                margins: ContentMargins::default(),
                reservation_extent: 0,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: ContentPixelRect::default(),
            }],
            placements: vec![ContentPlacement {
                resource: ContentResourceId {
                    id: 20,
                    generation: 21,
                },
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
            targets: vec![ContentTarget {
                surface_index: 0,
                action_kind: 2,
                target_id: 22,
                target_generation: 23,
                action_id: 2,
                bounds_px: ContentPixelRect {
                    x: 0,
                    y: 32,
                    width: 640,
                    height: 24,
                },
            }],
        }),
        Focus(b),
        FocusRevoked(NativeLauncherFocusRevoked {
            binding: b,
            reason: ContentReason::Revoked as u16,
        }),
        Input(NativeLauncherInput {
            event: NativeLauncherEvent {
                state_revision: 14,
                ..event
            },
            issued_mono_usec: 24,
            kind: NativeLauncherInputKind::Text,
            text: "café 東京".to_owned(),
        }),
        InputAck(NativeLauncherInputAck {
            event: NativeLauncherEvent {
                state_revision: 14,
                ..event
            },
            disposition: 1,
        }),
        Activate(activation),
        ActivationOutcome(NativeLauncherActivationOutcome {
            activation,
            status: 1,
            reason: 0,
        }),
        Closed(NativeLauncherClosed {
            grant: b.grant,
            opening: b.opening,
            reason: ContentReason::Cancelled as u16,
        }),
    ]
}
