use sophia_protocol::*;
use sophia_runtime::*;

pub const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 2,
    content_grant_epoch: 2,
};
pub const OUTPUT: ContentOutputId = ContentOutputId {
    id: 2,
    generation: 3,
};
pub const ALLOCATION: ContentAllocationId = ContentAllocationId {
    id: 1,
    generation: 1,
};
pub const RESOURCE: ContentResourceId = ContentResourceId {
    id: 1,
    generation: 1,
};
pub fn tx(n: u64) -> TransactionId {
    TransactionId::from_raw(n)
}
pub fn opening() -> NativeLauncherOpening {
    NativeLauncherOpening {
        grant: GRANT,
        opening: 7,
        output: OUTPUT,
        catalog_generation: 8,
        state_revision: 1,
    }
}
pub fn catalog() -> ShellApplicationCatalog {
    ShellApplicationCatalog {
        connection_epoch: GRANT.connection_epoch,
        generation: 8,
        entries: [1, 2]
            .into_iter()
            .map(|slot| ShellApplicationDescriptor {
                slot,
                available: true,
                label: format!("app{slot}"),
                keywords: String::new(),
            })
            .collect(),
    }
}
pub fn limits() -> ContentLimits {
    let mut limits = ContentLimits::prototype(GRANT);
    limits.max_staging_bytes = 4 * 1024 * 1024;
    limits.max_resident_bytes = 12 * 1024 * 1024;
    limits.max_retiring_bytes = 8 * 1024 * 1024;
    limits
}
pub fn facts() -> ContentOutputFactsEntry {
    ContentOutputFactsEntry {
        output: OUTPUT,
        local_width: 128,
        local_height: 64,
        scale_numerator: 1,
        scale_denominator: 1,
        scale_generation: 4,
    }
}
pub fn request(id: u64) -> NativeLauncherAllocationRequest {
    NativeLauncherAllocationRequest {
        grant: GRANT,
        opening: 7,
        output: OUTPUT,
        request_id: id,
        prior: ContentAllocationId::default(),
        operation: 1,
        edge: 1,
        desired_width: 64,
        desired_height: 32,
        margins: ContentMargins::default(),
    }
}
pub fn allocation() -> ContentAllocationSnapshot {
    ContentAllocationSnapshot {
        native_opening: Some(7),
        output: OUTPUT,
        allocation: ALLOCATION,
        scale_generation: 4,
        scale_numerator: 1,
        scale_denominator: 1,
        role: 3,
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 32,
            y: 0,
            width: 64,
            height: 32,
        },
        pixel: ContentPixelRect {
            x: 32,
            y: 0,
            width: 64,
            height: 32,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: 0,
    }
}
pub fn begin() -> NativeLauncherCandidateBegin {
    NativeLauncherCandidateBegin {
        content: ContentCandidateBegin {
            grant: GRANT,
            candidate_generation: 1,
            output: OUTPUT,
            facts_generation: 5,
            pacing_permit: 1,
            interaction_generation: 6,
            surface_count: 1,
            placement_count: 1,
            target_count: 2,
        },
        opening: 7,
        catalog_generation: 8,
        state_revision: 1,
        selected: 2,
        rows: vec![2, 1],
    }
}
pub fn chunk() -> ContentCandidateChunk {
    ContentCandidateChunk {
        grant: GRANT,
        candidate_generation: 1,
        chunk_ordinal: 0,
        surfaces: vec![ContentSurface {
            allocation: ALLOCATION,
            scale_generation: 4,
            role: 3,
            edge: 1,
            margins: ContentMargins::default(),
            reservation_extent: 0,
            parent_surface_index: u16::MAX,
            anchor_parent_rect: ContentPixelRect::default(),
        }],
        placements: vec![ContentPlacement {
            resource: RESOURCE,
            surface_index: 0,
            destination_x_px: 0,
            destination_y_px: 0,
        }],
        targets: [2, 1]
            .into_iter()
            .enumerate()
            .map(|(index, slot)| ContentTarget {
                surface_index: 0,
                action_kind: 2,
                target_id: index as u64 + 1,
                target_generation: 1,
                action_id: slot,
                bounds_px: ContentPixelRect {
                    x: index as i32,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            })
            .collect(),
    }
}
pub fn end() -> ContentCandidateEnd {
    ContentCandidateEnd {
        grant: GRANT,
        candidate_generation: 1,
        surface_count: 1,
        placement_count: 1,
        target_count: 2,
    }
}
pub fn context(allocations: &[ContentAllocationSnapshot]) -> ContentCandidateContext<'_> {
    ContentCandidateContext {
        output: OUTPUT,
        facts_generation: 5,
        interaction_generation: 6,
        allocations,
    }
}
pub fn native(catalog: &ShellApplicationCatalog) -> NativeLauncherCandidateContext<'_> {
    NativeLauncherCandidateContext {
        opening: opening(),
        state_revision: 1,
        catalog,
    }
}
pub fn resources(registry: &mut ContentEpochRegistry) {
    upload(registry, GRANT, 255);
}
pub fn upload(registry: &mut ContentEpochRegistry, grant: ContentGrant, value: u8) {
    let store = registry.resources_mut(grant).unwrap();
    store
        .begin(
            tx(1),
            ContentResourceBegin {
                grant,
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
    store
        .chunk(
            tx(2),
            &ContentResourceChunk {
                grant,
                resource: RESOURCE,
                ordinal: 0,
                offset: 0,
                bytes: vec![0, 0, value, 255, 0, 128, 0, 128],
            },
            0,
        )
        .unwrap();
    store
        .end(
            tx(3),
            &ContentResourceEnd {
                grant,
                resource: RESOURCE,
                total_bytes: 8,
                chunk_count: 1,
            },
            0,
        )
        .unwrap();
    while store.take_event().is_some() {}
}
pub fn registry() -> ContentEpochRegistry {
    registry_with_bar(false)
}
pub fn registry_with_bar(bar: bool) -> ContentEpochRegistry {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    if bar {
        registry
            .admit(ContentLimits::prototype(ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            }))
            .unwrap();
    }
    registry
        .admit_with_profile(limits(), ContentStoreProfile::NativeLauncher)
        .unwrap();
    let allocations = registry.allocations_mut(GRANT).unwrap();
    allocations
        .publish_outputs(tx(1), 5, vec![facts()])
        .unwrap();
    allocations.take_event().unwrap();
    resources(&mut registry);
    registry
}
pub fn grant_allocation(registry: &mut ContentEpochRegistry) -> Vec<ContentAllocationSnapshot> {
    let store = registry.allocations_mut(GRANT).unwrap();
    store
        .request_native_launcher(tx(4), request(1), opening(), 0)
        .unwrap();
    store.grant(1, allocation(), &[]).unwrap();
    store.take_event().unwrap();
    store.snapshots()
}
pub fn assemble(
    registry: &mut ContentEpochRegistry,
    allocations: &[ContentAllocationSnapshot],
    catalog: &ShellApplicationCatalog,
) {
    let (resources, store) = registry.active_parts_mut(GRANT).unwrap();
    store.grant_permit(tx(5), OUTPUT, 1, 1, 0).unwrap();
    store.take_event().unwrap();
    store
        .begin_native_launcher(tx(6), begin(), native(catalog), 0)
        .unwrap();
    store.chunk_native_launcher(tx(7), chunk(), 0).unwrap();
    store
        .end_native_launcher(
            tx(8),
            end(),
            context(allocations),
            native(catalog),
            resources,
            0,
        )
        .unwrap();
}
