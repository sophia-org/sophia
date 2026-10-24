//! Supplied storage fixtures; memfd is NOT an immutable GPU capture proof.
use sophia_protocol::*;
use sophia_renderer_image::{
    OwnedDmaBufPlane, OwnedSnapshotData, RendererImageId, RendererImageSnapshot, SnapshotBudget,
    SnapshotEpoch,
};
use sophia_runtime::{ContentAllocationSnapshot, ContentCapturedImage, ContentEpochRegistry};
use std::sync::Arc;

pub(crate) struct Storage {
    pub image: Arc<ContentCapturedImage>,
    pub budget: Arc<SnapshotBudget>,
    pub epoch: Arc<SnapshotEpoch>,
}
pub(crate) fn storage(id: u64) -> Storage {
    let fd = rustix::fs::memfd_create(
        c"supplied-captured-storage",
        rustix::fs::MemfdFlags::CLOEXEC,
    )
    .unwrap();
    rustix::fs::ftruncate(&fd, 64).unwrap();
    let budget = SnapshotBudget::new(1, 64);
    let epoch = SnapshotEpoch::new(1, 2, 3);
    let image = budget
        .retain(
            RendererImageSnapshot::uncharged(
                RendererImageId::from_raw(id),
                OwnedSnapshotData {
                    width: 4,
                    height: 4,
                    format: DRM_FORMAT_XRGB8888,
                    modifier: 0,
                    plane_count: 1,
                    planes: [
                        Some(OwnedDmaBufPlane {
                            fd,
                            offset: 0,
                            stride: 16,
                        }),
                        None,
                        None,
                        None,
                    ],
                },
            ),
            epoch.clone(),
        )
        .unwrap();
    Storage {
        image: Arc::new(image.into()),
        budget,
        epoch,
    }
}

pub(crate) fn metadata(grant: ContentGrant) -> shell_files::ShellImageMetadata {
    shell_files::ShellImageMetadata {
        offer_id: 1,
        resource: 1,
        resource_generation: 1,
        admission_epoch: grant.content_grant_epoch,
        output_id: 1,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: 2,
        source_format: DRM_FORMAT_XRGB8888,
        width: 4,
        height: 4,
        stride: 0,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 64,
        modifier: 0,
    }
}

pub(crate) fn allocation_registry(grant: ContentGrant) -> ContentEpochRegistry {
    let mut registry = ContentEpochRegistry::new(64 << 20).unwrap();
    add_allocation_epoch(&mut registry, grant);
    registry
}

pub(crate) fn add_allocation_epoch(registry: &mut ContentEpochRegistry, grant: ContentGrant) {
    let mut limits = ContentLimits::prototype(grant);
    limits.max_staging_bytes = 4 << 20;
    limits.max_resident_bytes = 4 << 20;
    limits.max_retiring_bytes = 4 << 20;
    registry.admit(limits).unwrap();
    let store = registry.allocations_mut(grant).unwrap();
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    store
        .publish_outputs(
            TransactionId::from_raw(1),
            1,
            vec![ContentOutputFactsEntry {
                output,
                local_width: 200,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    store
        .request(
            TransactionId::from_raw(2),
            ContentAllocationRequest {
                grant,
                output,
                allocation_request_id: 1,
                operation: 1,
                role: 1,
                edge: 1,
                prior: ContentAllocationId::default(),
                parent: ContentAllocationId::default(),
                parent_presentation_epoch: 0,
                anchor_parent_rect: ContentPixelRect::default(),
                desired_width: 200,
                desired_height: 20,
                margins: ContentMargins::default(),
            },
            &[],
            0,
        )
        .unwrap();
    store
        .grant(
            1,
            ContentAllocationSnapshot {
                native_opening: None,
                output,
                allocation: ContentAllocationId {
                    id: 1,
                    generation: 1,
                },
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
            },
            &[],
        )
        .unwrap();
    while store.take_event().is_some() {}
}
