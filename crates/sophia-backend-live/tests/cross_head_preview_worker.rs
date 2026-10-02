#![cfg(all(feature = "gbm-probe", feature = "libdrm-events"))]

use sophia_backend_live::{
    LiveRenderedScanoutBufferExport, LiveRenderedScanoutBufferExporter,
    LiveRendererScanoutBufferExportDetail as Detail,
    LiveRendererScanoutBufferExportStatus as Status, LiveRendererWorkerOutputKey,
    NativeGbmRenderedScanoutBufferDiscoveryExporter as Exporter, NativeGbmRenderedScanoutOwner,
    NativeGbmRendererWorkerCore, RenderDeviceDiscoveryBackend,
};
use sophia_protocol::{Rect, Size, Transform};
use sophia_renderer_live::{
    LiveCompositionPlacement, LiveGbmEglFrameTargetRecord, LiveOwnedDmaBufPlane,
    LiveOwnedMixedCompositionFrame as Frame, LiveOwnedMixedCompositionLayer as Layer,
    LiveOwnedMultiPlaneDmaBufFrame, LiveRendererImageId,
};
use std::{
    fs::{File, OpenOptions},
    path::PathBuf,
    time::{Duration, Instant},
};

struct Workers(Vec<std::sync::Arc<NativeGbmRendererWorkerCore>>);
impl Drop for Workers {
    fn drop(&mut self) {
        for core in &self.0 {
            core.request_shutdown();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        for core in &self.0 {
            while !core.poll_shutdown().expect("worker shutdown") {
                assert!(Instant::now() < deadline, "renderer cleanup did not finish");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

#[derive(Clone)]
struct Node(PathBuf);
impl RenderDeviceDiscoveryBackend for Node {
    type Device = File;
    fn open_render_device(&self) -> std::io::Result<File> {
        OpenOptions::new().read(true).write(true).open(&self.0)
    }
}
fn placement(x: i32, width: i32) -> LiveCompositionPlacement {
    LiveCompositionPlacement {
        target: Rect {
            x,
            y: 0,
            width,
            height: 16,
        },
        clip: Some(Rect {
            x: 0,
            y: 0,
            width: 32,
            height: 16,
        }),
        transform: Transform::IDENTITY,
        alpha: 1.0,
        sampling: sophia_engine::HeadSamplingClass::Exact,
    }
}
fn retained(id: u64, x: i32, width: i32) -> Layer {
    Layer::RendererImage {
        size: Size {
            width: 32,
            height: 16,
        },
        format: 0x34325258,
        image_id: LiveRendererImageId::from_raw(id),
        placement: placement(x, width),
    }
}
fn frame(layers: Vec<Layer>) -> Frame {
    Frame {
        image_reads: Default::default(),
        layers,
        trace: None,
        output_damage_snapshot: None,
        direct_scanout: Default::default(),
    }
}
fn render(
    exporter: &mut Exporter<Node>,
    frame: Frame,
) -> LiveRenderedScanoutBufferExport<NativeGbmRenderedScanoutOwner> {
    exporter.set_pending_mixed_frame(frame);
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let result =
            exporter.export_rendered_scanout_buffer(LiveGbmEglFrameTargetRecord::new(Size {
                width: 32,
                height: 16,
            }));
        if result.status != Status::Pending {
            return result;
        }
        assert!(Instant::now() < end, "worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn capture_staged(
    node: &Node,
    exporter: &mut Exporter<Node>,
    id: u64,
    color: sophia_engine::CompositorRgb8,
) {
    let size = Size {
        width: 32,
        height: 16,
    };
    let mut context =
        sophia_renderer_live::NativeGbmRenderedScanoutContext::from_backend_device_result(
            node.open_render_device(),
        )
        .context
        .unwrap();
    let source = context
        .export_owned_mixed_frame_with_modifiers(
            LiveGbmEglFrameTargetRecord::new(size),
            &frame(vec![Layer::Solid {
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 32,
                    height: 16,
                },
                color,
            }]),
            &[0],
        )
        .unwrap();
    let source = source
        .buffer
        .unwrap_or_else(|| panic!("source buffer: {:?}", source.detail));
    let descriptor = source.descriptor();
    let mut fds = source
        .export_scanout_dma_buf_fds()
        .unwrap()
        .into_plane_fds();
    let result = render(
        exporter,
        frame(vec![Layer::DmaBuf {
            image_id: LiveRendererImageId::from_raw(id),
            frame: LiveOwnedMultiPlaneDmaBufFrame {
                width: 32,
                height: 16,
                format: descriptor.format,
                modifier: descriptor.modifier.unwrap(),
                plane_count: descriptor.plane_count,
                planes: std::array::from_fn(|i| {
                    fds[i].take().map(|fd| LiveOwnedDmaBufPlane {
                        fd,
                        offset: descriptor.plane_offsets[i],
                        stride: descriptor.plane_pitches[i],
                    })
                }),
            },
            placement: placement(0, 32),
        }]),
    );
    assert_eq!(
        result.status,
        Status::Exported,
        "capture: {:?}",
        result.detail
    );
    drop(result);
}
fn capture(node: &Node, exporter: &mut Exporter<Node>, id: u64) {
    capture_staged(
        node,
        exporter,
        id,
        sophia_engine::CompositorRgb8 {
            red: 255,
            green: 255,
            blue: 255,
        },
    );
    assert!(
        exporter
            .promote_renderer_image(LiveRendererImageId::from_raw(id))
            .unwrap()
    );
}
fn overlay() -> Frame {
    frame(vec![
        retained(2, 0, 32),
        Layer::Solid {
            geometry: Rect {
                x: 0,
                y: 0,
                width: 32,
                height: 16,
            },
            color: sophia_engine::CompositorRgb8 {
                red: 0,
                green: 0,
                blue: 0,
            },
        },
        retained(2, -1, 8),
        retained(1, 16, 8),
    ])
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; private off-screen workers, no KMS"]
fn a_staged_restore_collision_can_promote_or_rollback_then_restore() {
    let node = Node(PathBuf::from(
        std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("render node"),
    ));
    let workers = Workers(vec![
        NativeGbmRendererWorkerCore::spawn(node.open_render_device()).unwrap(),
        NativeGbmRendererWorkerCore::spawn(node.open_render_device()).unwrap(),
    ]);
    let mut donor = Exporter::new(node.clone());
    donor.set_output(LiveRendererWorkerOutputKey::from_raw(1));
    donor.attach_shared_worker(&workers.0[0]);
    let mut target = Exporter::new(node.clone());
    target.set_output(LiveRendererWorkerOutputKey::from_raw(2));
    target.attach_shared_worker(&workers.0[1]);
    for (raw, rollback) in [(701, false), (702, true)] {
        let image = LiveRendererImageId::from_raw(raw);
        capture(&node, &mut donor, raw);
        capture_staged(
            &node,
            &mut target,
            raw,
            sophia_engine::CompositorRgb8 {
                red: 255,
                green: 255,
                blue: 255,
            },
        );
        let snapshot = donor
            .try_export_promoted_renderer_image(image)
            .unwrap()
            .unwrap();
        assert!(!target.restore_promoted_renderer_image(snapshot).unwrap());
        assert!(
            target
                .try_export_promoted_renderer_image(image)
                .unwrap()
                .is_none()
        );
        if rollback {
            assert!(target.rollback_renderer_image(image).unwrap());
            let snapshot = donor
                .try_export_promoted_renderer_image(image)
                .unwrap()
                .unwrap();
            assert!(target.restore_promoted_renderer_image(snapshot).unwrap());
        } else {
            assert!(target.promote_renderer_image(image).unwrap());
        }
        assert!(
            target
                .try_export_promoted_renderer_image(image)
                .unwrap()
                .is_some()
        );
        let displayed = render(&mut target, frame(vec![retained(raw, 0, 32)]));
        assert_eq!(displayed.status, Status::Exported, "{:?}", displayed.detail);
        drop(displayed);
        target.evict_renderer_image(image).unwrap();
        donor.evict_renderer_image(image).unwrap();
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; private off-screen workers, no KMS"]
fn private_head_workers_refuse_cross_head_preview_until_image_is_transferred() {
    let node = Node(PathBuf::from(
        std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("render node"),
    ));
    let workers = Workers(vec![
        NativeGbmRendererWorkerCore::spawn(node.open_render_device()).unwrap(),
        NativeGbmRendererWorkerCore::spawn(node.open_render_device()).unwrap(),
    ]);
    let mut first = Exporter::new(node.clone());
    first.set_output(LiveRendererWorkerOutputKey::from_raw(1));
    first.attach_shared_worker(&workers.0[0]);
    let mut second = Exporter::new(node.clone());
    second.set_output(LiveRendererWorkerOutputKey::from_raw(2));
    second.attach_shared_worker(&workers.0[1]);
    capture(&node, &mut first, 1);
    capture(&node, &mut second, 2);
    let same_head = render(
        &mut second,
        frame(vec![retained(2, 0, 32), retained(2, -1, 8)]),
    );
    assert_eq!(
        same_head.status,
        Status::Exported,
        "same-head duplicate: {:?}",
        same_head.detail
    );
    drop(same_head);
    let missing = render(&mut second, overlay());
    eprintln!(
        "private worker cross-head: {:?} {:?}",
        missing.status, missing.detail
    );
    assert_eq!(missing.status, Status::Degraded);
    assert_eq!(missing.detail, Detail::InvalidRendererImageId);
    let snapshot = first
        .export_promoted_renderer_image(LiveRendererImageId::from_raw(1))
        .unwrap()
        .unwrap();
    first
        .evict_renderer_image(LiveRendererImageId::from_raw(1))
        .unwrap();
    let budget = sophia_renderer_live::LiveRendererSnapshotBudget::new(1, 8 * 1024 * 1024);
    let epoch = sophia_renderer_live::LiveRendererSnapshotEpoch::new(1, 1, 1);
    let snapshot = snapshot.retain(&budget, epoch.clone()).unwrap();
    assert_eq!(budget.usage().0, 1);
    let mut queued = overlay();
    queued.layers[3] = Layer::Snapshot {
        snapshot: snapshot.clone(),
        placement: placement(16, 8),
    };
    let before = second.persistent_render_stats().snapshot_captures;
    let restored = render(&mut second, queued);
    assert_eq!(
        restored.status,
        Status::Exported,
        "transferred after donor eviction: {:?}",
        restored.detail
    );
    assert_eq!(
        second.persistent_render_stats().snapshot_captures,
        before,
        "foreign immutable pixels must be imported directly, never recaptured"
    );
    drop(restored);
    drop(snapshot);
    assert_eq!(budget.usage().0, 1, "the EGL import still pins its charge");
    second
        .evict_renderer_image(LiveRendererImageId::from_raw(1))
        .unwrap();
    assert_eq!(
        budget.usage(),
        (0, 0),
        "eviction must release foreign import custody"
    );
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; shared off-screen worker, no KMS"]
fn shared_store_control_already_owns_both_preview_sources() {
    let node = Node(PathBuf::from(
        std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("render node"),
    ));
    let workers = Workers(vec![
        NativeGbmRendererWorkerCore::spawn(node.open_render_device()).unwrap(),
    ]);
    let core = &workers.0[0];
    let mut first = Exporter::new(node.clone());
    first.set_output(LiveRendererWorkerOutputKey::from_raw(1));
    first.attach_shared_worker(core);
    let mut second = Exporter::new(node.clone());
    second.set_output(LiveRendererWorkerOutputKey::from_raw(2));
    second.attach_shared_worker(core);
    capture(&node, &mut first, 1);
    capture(&node, &mut second, 2);
    let result = render(&mut second, overlay());
    assert_eq!(
        result.status,
        Status::Exported,
        "shared-store control: {:?}",
        result.detail
    );
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; off-screen worker, no KMS"]
fn removed_local_images_survive_queued_readers_and_release_after_render() {
    let node = Node(PathBuf::from(
        std::env::var_os("SOPHIA_TEST_RENDER_NODE").unwrap(),
    ));
    let workers = Workers(vec![
        NativeGbmRendererWorkerCore::spawn(node.open_render_device()).unwrap(),
    ]);
    let mut exporter = Exporter::new(node.clone());
    exporter.set_output(LiveRendererWorkerOutputKey::from_raw(1));
    exporter.attach_shared_worker(&workers.0[0]);
    let reads = sophia_renderer_live::LiveRendererImageReads::default();
    for raw in 200..208 {
        capture(&node, &mut exporter, raw);
        let image = LiveRendererImageId::from_raw(raw);
        let mut queued = frame(vec![retained(raw, 0, 32)]);
        queued.image_reads = vec![reads.acquire(image)].into_boxed_slice();
        let sibling = sophia_backend_live::try_clone_mixed_frame(&queued).unwrap();
        let frozen_retry = reads.acquire(image);
        // Native eviction uses exactly this guard before its broadcast. The
        // removed window has no displayed owner; only queued/frozen users do.
        assert!(!reads.request_eviction(image));
        let first = render(&mut exporter, queued);
        assert_eq!(first.status, Status::Exported);
        assert!(reads.ready_evictions().is_empty());
        drop(frozen_retry);
        assert!(
            reads.ready_evictions().is_empty(),
            "mirror sibling still reads"
        );
        let second = render(&mut exporter, sibling);
        assert_eq!(second.status, Status::Exported);
        // Output buffers can still be held for scanout. They contain their own
        // pixels and must not retain these local input-image guards.
        assert_eq!(reads.ready_evictions(), vec![image]);
        assert!(reads.request_eviction(image));
        assert!(exporter.evict_renderer_image(image).unwrap());
        assert!(reads.ready_evictions().is_empty());
        assert!(
            exporter
                .export_promoted_renderer_image(image)
                .unwrap()
                .is_none()
        );
        // Maintenance returns its result, but render metrics are refreshed by
        // render completion. Observe a source-free render before reading them.
        let barrier = render(
            &mut exporter,
            frame(vec![Layer::Solid {
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 32,
                    height: 16,
                },
                color: sophia_engine::CompositorRgb8 {
                    red: 0,
                    green: 0,
                    blue: 0,
                },
            }]),
        );
        assert_eq!(barrier.status, Status::Exported);
        assert_eq!(exporter.persistent_render_stats().snapshot_live_entries, 0);
        drop((first, second, barrier));
        reads.prune();
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; private off-screen workers, no KMS"]
fn promotion_exports_once_and_retired_queued_snapshots_do_not_resurrect_imports() {
    let node = Node(PathBuf::from(
        std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("render node"),
    ));
    let workers = Workers(vec![
        NativeGbmRendererWorkerCore::spawn(node.open_render_device()).unwrap(),
        NativeGbmRendererWorkerCore::spawn(node.open_render_device()).unwrap(),
    ]);
    let mut first = Exporter::new(node.clone());
    first.set_output(LiveRendererWorkerOutputKey::from_raw(1));
    first.attach_shared_worker(&workers.0[0]);
    let mut second = Exporter::new(node.clone());
    second.set_output(LiveRendererWorkerOutputKey::from_raw(2));
    second.attach_shared_worker(&workers.0[1]);
    let budget = sophia_renderer_live::LiveRendererSnapshotBudget::new(1, 8 * 1024 * 1024);
    let epoch = sophia_renderer_live::LiveRendererSnapshotEpoch::new(1, 1, 1);
    for id in 10..18 {
        let image = LiveRendererImageId::from_raw(id);
        capture_staged(
            &node,
            &mut first,
            id,
            sophia_engine::CompositorRgb8 {
                red: id as u8,
                green: 255,
                blue: 0,
            },
        );
        let promotion = first.promote_and_export_renderer_image(image).unwrap();
        assert!(promotion.promoted);
        let snapshot = promotion
            .snapshot
            .unwrap()
            .unwrap()
            .retain(&budget, epoch.clone())
            .unwrap();
        let repeat = first.promote_and_export_renderer_image(image).unwrap();
        assert!(
            !repeat.promoted,
            "promotion must not re-export an already promoted id"
        );
        assert!(repeat.snapshot.unwrap().is_none());
        let mut queued = frame(vec![Layer::Snapshot {
            snapshot: snapshot.clone(),
            placement: placement(0, 32),
        }]);
        if id == 17 {
            // Saturation refuses a *new* attachment, never the source promotion.
            capture_staged(
                &node,
                &mut first,
                18,
                sophia_engine::CompositorRgb8 {
                    red: 1,
                    green: 2,
                    blue: 3,
                },
            );
            let full = first
                .promote_and_export_renderer_image(LiveRendererImageId::from_raw(18))
                .unwrap();
            assert!(full.promoted);
            assert!(matches!(
                full.snapshot
                    .unwrap()
                    .unwrap()
                    .retain(&budget, epoch.clone()),
                Err(Detail::RendererImageStoreFull)
            ));
            first
                .evict_renderer_image(LiveRendererImageId::from_raw(18))
                .unwrap();
        }
        snapshot.retire_import_cache();
        first.evict_renderer_image(image).unwrap();
        second.evict_renderer_image(image).unwrap();
        // Both stores have seen the broadcast eviction *before* this frame
        // reaches the recipient. Its own immutable FDs still suffice.
        let before = second.persistent_render_stats().snapshot_captures;
        let result = render(
            &mut second,
            std::mem::replace(&mut queued, frame(Vec::new())),
        );
        assert_eq!(
            result.status,
            Status::Exported,
            "revision {id}: {:?}",
            result.detail
        );
        assert_eq!(second.persistent_render_stats().snapshot_captures, before);
        drop(snapshot);
        assert_eq!(
            budget.usage().0,
            1,
            "submitted buffer still owns the source"
        );
        drop(result);
        // Worker leases release asynchronously; an ordered maintenance visit
        // puts the release ahead of this observation without reading a stream.
        second
            .evict_renderer_image(LiveRendererImageId::from_raw(9999))
            .unwrap();
        assert_eq!(budget.usage(), (0, 0));
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; three off-screen stores, no KMS"]
fn recipient_replaces_a_donor_copy_and_drops_an_invalid_epoch_before_render() {
    let node = Node(PathBuf::from(
        std::env::var_os("SOPHIA_TEST_RENDER_NODE").unwrap(),
    ));
    let workers = Workers(
        (0..3)
            .map(|_| NativeGbmRendererWorkerCore::spawn(node.open_render_device()).unwrap())
            .collect(),
    );
    let mut exporters = (0..3)
        .map(|i| {
            let mut exporter = Exporter::new(node.clone());
            exporter.set_output(LiveRendererWorkerOutputKey::from_raw(i as u64 + 1));
            exporter.attach_shared_worker(&workers.0[i]);
            exporter
        })
        .collect::<Vec<_>>();
    // Same immutable pixels and image identity, independent donor allocations.
    capture(&node, &mut exporters[0], 500);
    capture(&node, &mut exporters[1], 500);
    let image = LiveRendererImageId::from_raw(500);
    let budget = sophia_renderer_live::LiveRendererSnapshotBudget::new(2, 16 * 1024 * 1024);
    let epochs = [
        sophia_renderer_live::LiveRendererSnapshotEpoch::new(1, 1, 1),
        sophia_renderer_live::LiveRendererSnapshotEpoch::new(1, 2, 2),
    ];
    let mut snapshots = Vec::new();
    for i in 0..2 {
        let snapshot = exporters[i]
            .export_promoted_renderer_image(image)
            .unwrap()
            .unwrap()
            .retain(&budget, epochs[i].clone())
            .unwrap();
        let result = render(
            &mut exporters[2],
            frame(vec![Layer::Snapshot {
                snapshot: snapshot.clone(),
                placement: placement(0, 32),
            }]),
        );
        assert_eq!(
            result.status,
            Status::Exported,
            "donor {i}: {:?}",
            result.detail
        );
        drop(result);
        // Reuse the released target slot; otherwise this check can leave an
        // independent, still-valid import in a different frame slot.
        exporters[2]
            .evict_renderer_image(LiveRendererImageId::from_raw(9999))
            .unwrap();
        snapshots.push(snapshot);
    }
    assert_eq!(exporters[2].persistent_render_stats().snapshot_captures, 0);
    // Workers keep separate import caches per frame slot. Production retires
    // the old donor lease and explicitly evicts its imports in every slot.
    epochs[0].invalidate();
    exporters[2].evict_renderer_image_imports(image).unwrap();
    drop(snapshots.remove(0));
    exporters[2]
        .evict_renderer_image(LiveRendererImageId::from_raw(9999))
        .unwrap();
    assert_eq!(budget.usage().0, 1);
    let current = render(
        &mut exporters[2],
        frame(vec![Layer::Snapshot {
            snapshot: snapshots[0].clone(),
            placement: placement(0, 32),
        }]),
    );
    assert_eq!(current.status, Status::Exported);
    drop(current);
    exporters[2]
        .evict_renderer_image(LiveRendererImageId::from_raw(9999))
        .unwrap();
    epochs[1].invalidate();
    let refused = render(
        &mut exporters[2],
        frame(vec![Layer::Snapshot {
            snapshot: snapshots.pop().unwrap(),
            placement: placement(0, 32),
        }]),
    );
    assert_eq!(refused.detail, Detail::InvalidRendererImageId);
    assert_eq!(refused.status, Status::Degraded);
    drop(refused);
    // A maintenance barrier places worker disposal before budget observation.
    exporters[2]
        .evict_renderer_image(LiveRendererImageId::from_raw(9999))
        .unwrap();
    assert_eq!(
        budget.usage(),
        (0, 0),
        "early refusal must clear stale imports"
    );
}
