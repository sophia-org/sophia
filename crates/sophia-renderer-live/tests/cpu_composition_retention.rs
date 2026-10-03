use std::sync::Arc;

use sophia_engine::{
    CompositorContentImage, CompositorDisplayCommand, CompositorDisplayList, CompositorNodeId,
    HeadlessOutput,
};
use sophia_protocol::{
    BufferSource, CommittedSurfaceState, ContentGrant, ContentLimits, ContentResourceBegin,
    ContentResourceChunk, ContentResourceEnd, ContentResourceId, ContentResourceRetire, OutputId,
    Rect, Region, ShellContentRecord, Size, SurfaceId, TransactionId,
};
use sophia_renderer_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuBufferSource, LiveCpuBufferUpdate,
    LiveProductionCpuScene,
};

#[test]
fn production_scene_reuses_a_retired_frame_while_latest_pixels_are_shared() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 4,
            height: 1,
        },
        scale: 1,
    };
    let surface = SurfaceId::new(1, 1);
    let mut committed = [CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            sophia_protocol::Size {
                width: 1,
                height: 1,
            },
        ),
        damage: Region::empty(),
    }];
    let display_list = CompositorDisplayList {
        output: output.id,
        commands: vec![CompositorDisplayCommand::Surface { surface }],
    };
    let mut scene = LiveProductionCpuScene::new(output.size);
    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 1,
            size: Size {
                width: 1,
                height: 1,
            },
            stride: 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![0x11; 4]),
        })])
        .unwrap();
    let first = scene
        .compose_display_list(output, &committed, &display_list, None)
        .unwrap()
        .frame
        .bytes
        .clone();
    let first_allocation = first.as_ptr();

    committed[0].committed_generation = 2;
    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 1,
            size: Size {
                width: 1,
                height: 1,
            },
            stride: 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 2,
            bytes: Arc::new(vec![0x22; 4]),
        })])
        .unwrap();
    let second = scene
        .compose_display_list(output, &committed, &display_list, None)
        .unwrap()
        .frame
        .bytes
        .clone();
    assert_ne!(second.as_ptr(), first_allocation);
    drop(first);

    committed[0].committed_generation = 3;
    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 1,
            size: Size {
                width: 1,
                height: 1,
            },
            stride: 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 3,
            bytes: Arc::new(vec![0x33; 4]),
        })])
        .unwrap();
    let third = scene
        .compose_display_list(output, &committed, &display_list, None)
        .unwrap();

    assert_eq!(third.frame.bytes.as_ptr(), first_allocation);
    assert_eq!(&third.frame.bytes[..4], &[0x33; 4]);
    assert_eq!(&third.frame.bytes[4..], &[0; 12]);
    assert_eq!(second.as_ref()[..4], [0x22; 4]);
}

#[test]
fn production_scene_reuses_shared_latest_pixels_for_an_unchanged_snapshot() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 4,
            height: 1,
        },
        scale: 1,
    };
    let display_list = CompositorDisplayList::empty(output.id);
    let mut scene = LiveProductionCpuScene::new(output.size);
    let observer = scene
        .compose_display_list(output, &[], &display_list, None)
        .unwrap()
        .frame
        .bytes
        .clone();

    let unchanged = scene
        .compose_display_list(output, &[], &display_list, None)
        .unwrap();

    assert!(Arc::ptr_eq(&observer, &unchanged.frame.bytes));
}

fn upload_content_resource(
    store: &mut sophia_runtime::ContentResourceStore,
    grant: ContentGrant,
    id: u64,
    transaction: u64,
    byte: u8,
) -> sophia_runtime::ContentResourceLease {
    let resource = ContentResourceId { id, generation: 1 };
    let pixels = vec![byte, byte, byte, 0xff];
    store
        .begin(
            TransactionId::from_raw(transaction),
            ContentResourceBegin {
                grant,
                resource,
                width_px: 1,
                height_px: 1,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 4,
            },
            0,
        )
        .unwrap();
    store
        .chunk(
            TransactionId::from_raw(transaction + 1),
            &ContentResourceChunk {
                grant,
                resource,
                ordinal: 0,
                offset: 0,
                bytes: pixels,
            },
            0,
        )
        .unwrap();
    store
        .end(
            TransactionId::from_raw(transaction + 2),
            &ContentResourceEnd {
                grant,
                resource,
                total_bytes: 4,
                chunk_count: 1,
            },
            0,
        )
        .unwrap();
    store.lease(grant, resource).unwrap()
}

fn content_display_list(
    output: HeadlessOutput,
    candidate: u64,
    lease: sophia_runtime::ContentResourceLease,
) -> CompositorDisplayList {
    let generation = lease.description().resource.generation;
    CompositorDisplayList {
        output: output.id,
        commands: vec![CompositorDisplayCommand::ContentImage(
            CompositorContentImage {
                node: CompositorNodeId::ShellContent {
                    grant: lease.description().grant,
                    output: output.id,
                    candidate,
                    surface: 0,
                    placement: 0,
                },
                generation,
                output_size_px: output.size,
                geometry_px: Rect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                size_px: Size {
                    width: 1,
                    height: 1,
                },
                stride: 4,
                format: u32::from_le_bytes(*b"AR24"),
                resource: lease.into(),
            },
        )],
    }
}

#[test]
fn busy_composed_pixels_do_not_pin_a_replaced_shell_resource() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 4,
            height: 1,
        },
        scale: 1,
    };
    let grant = ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    };
    let old_resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap();
    let old = content_display_list(
        output,
        1,
        upload_content_resource(&mut store, grant, 1, 1, 0x20),
    );
    let next = content_display_list(
        output,
        2,
        upload_content_resource(&mut store, grant, 2, 4, 0x40),
    );
    while store.take_event().is_some() {}

    let mut scene = LiveProductionCpuScene::new(output.size);
    let scanout_pixels = scene
        .compose_display_list(output, &[], &old, None)
        .unwrap()
        .frame
        .bytes
        .clone();
    scene
        .compose_display_list(output, &[], &next, None)
        .unwrap();
    assert_eq!(
        Arc::strong_count(&scanout_pixels),
        2,
        "the detached framebuffer remains in the bounded reuse pool"
    );
    drop(old);

    store
        .retire(
            TransactionId::from_raw(7),
            &ContentResourceRetire {
                grant,
                resource: old_resource,
            },
        )
        .unwrap();
    store.collect();
    assert!(matches!(
        store.take_event().map(|event| event.record),
        Some(ShellContentRecord::ResourceReleased(released))
            if released.resource == old_resource
    ));
    assert_eq!(
        scanout_pixels.len(),
        16,
        "the old framebuffer is still in flight"
    );
}

#[test]
fn gpu_only_updates_reuse_cpu_pixels_but_source_transitions_repaint() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 4,
            height: 2,
        },
        scale: 1,
    };
    let surface = SurfaceId::new(1, 1);
    let size = Size {
        width: 2,
        height: 1,
    };
    let mut state = CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry: Rect {
            x: 1,
            y: 0,
            width: 2,
            height: 1,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf { handle: 1 },
            size,
        ),
        damage: Region::empty(),
    };
    let list = CompositorDisplayList {
        output: output.id,
        commands: vec![CompositorDisplayCommand::Surface { surface }],
    };
    let mut scene = LiveProductionCpuScene::new(output.size);
    let first = scene
        .compose_display_list(output, std::slice::from_ref(&state), &list, None)
        .unwrap()
        .frame
        .bytes
        .clone();
    state.committed_generation = 2;
    state.content =
        sophia_protocol::SurfaceContentSet::singleton(BufferSource::DmaBuf { handle: 2 }, size);
    let second = scene
        .compose_display_list(output, std::slice::from_ref(&state), &list, None)
        .unwrap()
        .frame
        .bytes
        .clone();
    assert!(
        Arc::ptr_eq(&first, &second),
        "native image generations must not copy CPU backing"
    );
    assert_eq!(scene.cpu_raster_count(), 1);
    assert_eq!(scene.cpu_raster_reuse_count(), 1);

    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 3,
            size,
            stride: 8,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 3,
            bytes: Arc::new(vec![0x55; 8]),
        })])
        .unwrap();
    state.committed_generation = 3;
    state.content =
        sophia_protocol::SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle: 3 }, size);
    let cpu = scene
        .compose_display_list(output, std::slice::from_ref(&state), &list, None)
        .unwrap()
        .frame
        .bytes
        .clone();
    assert_eq!(&cpu[4..12], &[0x55; 8]);
    assert!(!Arc::ptr_eq(&first, &cpu));
    // An upload can advance the CPU registry independently of placement and
    // authority generation. The backing must still reflect its new bytes.
    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 3,
            size,
            stride: 8,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 4,
            bytes: Arc::new(vec![0x77; 8]),
        })])
        .unwrap();
    let uploaded = scene
        .compose_display_list(output, std::slice::from_ref(&state), &list, None)
        .unwrap()
        .frame
        .bytes
        .clone();
    assert_eq!(&uploaded[4..12], &[0x77; 8]);
    assert_eq!(
        &cpu[4..12],
        &[0x55; 8],
        "old CPU generation remains immutable"
    );
    state.committed_generation = 4;
    state.content =
        sophia_protocol::SurfaceContentSet::singleton(BufferSource::DmaBuf { handle: 4 }, size);
    let gpu = scene
        .compose_display_list(output, &[state], &list, None)
        .unwrap();
    assert!(
        gpu.frame.bytes.iter().all(|byte| *byte == 0),
        "CPU pixels under the new GPU source must be cleared"
    );
    assert_eq!(scene.cpu_raster_count(), 4);
    assert!(
        first.iter().all(|byte| *byte == 0),
        "an in-flight CPU backing stays immutable"
    );
    assert_eq!(&cpu[4..12], &[0x55; 8]);
}
