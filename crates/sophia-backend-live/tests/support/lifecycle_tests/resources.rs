use super::*;

pub(super) fn upload(
    store: &mut sophia_runtime::ContentResourceStore,
    grant: ContentGrant,
    resource: ContentResourceId,
) -> sophia_runtime::ContentResourceLease {
    store
        .begin(
            TransactionId::from_raw(1),
            ContentResourceBegin {
                grant,
                resource,
                width_px: 4,
                height_px: 2,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 32,
            },
            0,
        )
        .unwrap();
    let usage = store.usage();
    assert_eq!((usage.staging, usage.reserved_resident), (32, 32));
    assert!(usage.staging + usage.resident + usage.retiring <= 128);
    assert!(usage.backing <= 128);
    store
        .chunk(
            TransactionId::from_raw(2),
            &ContentResourceChunk {
                grant,
                resource,
                ordinal: 0,
                offset: 0,
                bytes: vec![0xff; 32],
            },
            0,
        )
        .unwrap();
    store
        .end(
            TransactionId::from_raw(3),
            &ContentResourceEnd {
                grant,
                resource,
                chunk_count: 1,
                total_bytes: 32,
            },
            0,
        )
        .unwrap();
    while store.take_event().is_some() {}
    store.lease(grant, resource).unwrap()
}

pub(super) fn shell_frame(
    output: HeadlessOutput,
    candidate: u64,
    lease: sophia_runtime::ContentResourceLease,
) -> LiveShellContentFrame {
    let grant = lease.description().grant;
    LiveShellContentFrame {
        output: output.id,
        content_output: ContentOutputId {
            id: output.id.raw(),
            generation: 1,
        },
        grant,
        candidate_generation: candidate,
        interaction_generation: candidate,
        images: vec![CompositorContentImage {
            node: CompositorNodeId::ShellContent {
                output: output.id,
                candidate,
                surface: 0,
                placement: 0,
            },
            generation: lease.description().resource.generation,
            output_size_px: output.size,
            geometry_px: Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            size_px: Size {
                width: 4,
                height: 2,
            },
            stride: 16,
            format: u32::from_le_bytes(*b"AR24"),
            resource: lease,
        }],
        targets: vec![PresentedContentTarget {
            grant,
            output: ContentOutputId {
                id: output.id.raw(),
                generation: 1,
            },
            candidate_generation: candidate,
            presentation_epoch: 0,
            interaction_generation: candidate,
            allocation: ContentAllocationId {
                id: output.id.raw(),
                generation: 1,
            },
            allocation_logical: ContentLogicalRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            allocation_pixel: ContentPixelRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
            target_id: 1,
            target_generation: candidate,
            action_id: candidate,
            bounds_px: ContentPixelRect {
                x: 0,
                y: 0,
                width: 4,
                height: 2,
            },
        }],
        allocations: vec![],
    }
}

pub(super) fn retire(
    store: &mut sophia_runtime::ContentResourceStore,
    grant: ContentGrant,
    resource: ContentResourceId,
) {
    store
        .retire(
            TransactionId::from_raw(4),
            &ContentResourceRetire { grant, resource },
        )
        .unwrap();
}

pub(super) fn released(store: &mut sophia_runtime::ContentResourceStore) -> Vec<ContentResourceId> {
    store.collect();
    let mut result = vec![];
    while let Some(event) = store.take_event() {
        match event.record {
            ShellContentRecord::ResourceReleased(value) => result.push(value.resource),
            other => panic!("unexpected retirement event: {other:?}"),
        }
    }
    result
}
