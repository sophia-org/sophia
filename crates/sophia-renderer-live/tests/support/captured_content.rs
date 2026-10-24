//! Supplied retained storage: no context, import, readback or GPU execution.
use super::*;
use sophia_engine::CompositorContentMetadata;
use sophia_protocol::DRM_FORMAT_XRGB8888;
use std::sync::Arc;
#[path = "../../../sophia-runtime/tests/support/captured_resource_fixture.rs"]
mod fixture;

#[test]
fn gpu_content_lowers_to_the_same_snapshot_and_frames_retain_resource_credit() {
    let grant = ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    };
    let mut registry = fixture::allocation_registry(grant);
    let metadata = fixture::metadata(grant);
    let supplied = fixture::storage(71);
    let mut ticket = Some(registry.reserve_capture_image(grant, &metadata, 0).unwrap());
    registry
        .accept_capture_image(&mut ticket, &mut Some(supplied.image.clone()), 0)
        .unwrap();
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let lease = registry
        .resources(grant)
        .unwrap()
        .lease(grant, resource)
        .unwrap();
    assert!(lease.cpu_bytes().is_none());
    assert!(sophia_renderer_live::LiveCpuPixelStorage::try_from(lease.clone()).is_err());
    let mut plan = plan();
    plan.layers.clear();
    plan.compositor.clear();
    plan.cursor = None;
    let geometry = Rect {
        x: 15,
        y: 9,
        width: 4,
        height: 4,
    };
    let image = CompositorContentImage {
        node: CompositorNodeId::ShellContent {
            grant,
            output: plan.output,
            candidate: 17,
            surface: 0,
            placement: 0,
        },
        generation: 1,
        output_size_px: plan.native_size,
        geometry_px: geometry,
        size_px: Size {
            width: 4,
            height: 4,
        },
        stride: 16,
        format: DRM_FORMAT_XRGB8888,
        resource: lease.into(),
    };
    assert_eq!(image.content_identity().source_bytes, 64);
    assert!(image.resource.cpu_bytes().is_none());
    plan.compositor.push(HeadCompositorCommand::ContentImage(
        sophia_engine::HeadCompositorContentImage {
            image,
            geometry,
            clip: geometry,
        },
    ));
    let lowered = lower_head_composition_plan(&plan, &[]).unwrap();
    let LiveOwnedMixedCompositionLayer::Snapshot {
        snapshot,
        placement,
    } = &lowered.layers[0]
    else {
        panic!("captured content must remain GPU-backed")
    };
    let sophia_runtime::ContentCapturedImage::Exported(exported) = supplied.image.as_ref() else {
        panic!("fixture must be exported");
    };
    assert!(Arc::ptr_eq(snapshot.storage(), exported));
    assert_eq!(placement.target, geometry);
    assert_eq!(placement.clip, Some(geometry));
    assert_eq!(supplied.budget.usage(), (1, 64));
    let queued = snapshot.clone();
    supplied.epoch.invalidate();
    assert!(matches!(
        lower_head_composition_plan(&plan, &[]),
        Err(LiveHeadCompositionLoweringError::UnavailableContentImage)
    ));
    drop(plan);
    drop(lowered);
    drop(supplied.image);
    let store = registry.resources_mut(grant).unwrap();
    while store.take_event().is_some() {}
    store
        .retire(
            TransactionId::from_raw(4),
            &ContentResourceRetire { grant, resource },
        )
        .unwrap();
    let mut calls = 0;
    registry
        .retire_capture_imports(&mut 1, &mut |_| {
            calls += 1;
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(calls, 1);
    let store = registry.resources_mut(grant).unwrap();
    assert!(store.take_event().is_none());
    assert_eq!(store.usage().retiring, 64);
    drop(queued);
    store.collect();
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::ResourceReleased(_)
    ));
    assert_eq!(supplied.budget.usage(), (1, 64));
    store.confirm_isb_release(resource).unwrap();
    assert_eq!(supplied.budget.usage(), (0, 0));
}

#[test]
fn managed_content_lowers_to_the_same_lease_and_a_queued_layer_holds_it_through_retirement() {
    let grant = ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    };
    let mut registry = fixture::allocation_registry(grant);
    let metadata = fixture::metadata(grant);
    let epoch = sophia_renderer_image::SnapshotEpoch::new(1, 2, 3);
    let wake = sophia_wake::Wake::new().unwrap();
    let lease = sophia_renderer_image::ManagedCaptureLease::new(
        sophia_renderer_image::ManagedCaptureInfo {
            image_id: sophia_renderer_image::RendererImageId::from_raw(72),
            allocation_id: 1,
            generation: 1,
            width: 4,
            height: 4,
            bytes: 64,
        },
        epoch.clone(),
        wake.notifier(),
    )
    .unwrap();
    let image = Arc::new(sophia_runtime::ContentCapturedImage::Managed(lease.clone()));
    let mut ticket = Some(registry.reserve_capture_image(grant, &metadata, 0).unwrap());
    registry
        .accept_capture_image(&mut ticket, &mut Some(image.clone()), 0)
        .unwrap();
    drop(image);
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let content = registry
        .resources(grant)
        .unwrap()
        .lease(grant, resource)
        .unwrap();
    let mut plan = plan();
    plan.layers.clear();
    plan.compositor.clear();
    plan.cursor = None;
    let geometry = Rect {
        x: 15,
        y: 9,
        width: 4,
        height: 4,
    };
    plan.compositor.push(HeadCompositorCommand::ContentImage(
        sophia_engine::HeadCompositorContentImage {
            image: CompositorContentImage {
                node: CompositorNodeId::ShellContent {
                    grant,
                    output: plan.output,
                    candidate: 17,
                    surface: 0,
                    placement: 0,
                },
                generation: 1,
                output_size_px: plan.native_size,
                geometry_px: geometry,
                size_px: Size {
                    width: 4,
                    height: 4,
                },
                stride: 16,
                format: DRM_FORMAT_XRGB8888,
                resource: content.into(),
            },
            geometry,
            clip: geometry,
        },
    ));
    let lowered = lower_head_composition_plan(&plan, &[]).unwrap();
    let LiveOwnedMixedCompositionLayer::ManagedCapture {
        lease: queued,
        placement,
    } = &lowered.layers[0]
    else {
        panic!("managed content must lower to its lease")
    };
    assert!(Arc::ptr_eq(queued, &lease));
    assert_eq!(placement.target, geometry);
    let queued = queued.clone();
    drop(plan);
    drop(lowered);
    let store = registry.resources_mut(grant).unwrap();
    while store.take_event().is_some() {}
    store
        .retire(
            TransactionId::from_raw(4),
            &ContentResourceRetire { grant, resource },
        )
        .unwrap();
    let mut calls = 0;
    registry
        .retire_capture_imports(&mut 1, &mut |_| {
            calls += 1;
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(calls, 1);
    assert!(lease.is_retired());
    drop(lease);
    let store = registry.resources_mut(grant).unwrap();
    store.collect();
    assert!(
        store.take_event().is_none(),
        "the queued layer still reads the storage"
    );
    assert_eq!(store.usage().retiring, 64);
    drop(queued);
    store.collect();
    assert!(matches!(
        store.take_event().unwrap().record,
        ShellContentRecord::ResourceReleased(_)
    ));
    store.confirm_isb_release(resource).unwrap();
    // A lease whose epoch ended can no longer be lowered, queued or not.
    epoch.invalidate();
    let late = sophia_renderer_image::ManagedCaptureLease::new(
        sophia_renderer_image::ManagedCaptureInfo {
            image_id: sophia_renderer_image::RendererImageId::from_raw(73),
            allocation_id: 2,
            generation: 2,
            width: 4,
            height: 4,
            bytes: 64,
        },
        epoch,
        wake.notifier(),
    );
    assert!(late.is_err());
}
