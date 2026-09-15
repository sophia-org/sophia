use std::sync::Arc;

use sophia_engine::{
    CompositorDisplayCommand, CompositorDisplayList, CompositorRgb8, CursorAsset, HeadlessOutput,
    x11_core_left_ptr_cursor,
};
use sophia_protocol::{
    BufferSource, CommittedSurfaceState, OutputId, Point, Rect, Region, Size, SurfaceId,
};
use sophia_renderer_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuBufferPatch, LiveCpuBufferSource,
    LiveCpuBufferSourceRef, LiveCpuBufferUpdate, LiveCpuCompositionElementRef,
    LiveCpuCompositionLayer, LiveCpuCompositionLayerRef, LiveCpuFrameMetricsMode,
    LiveProductionCpuScene, LiveSharedCpuBufferSource, compose_live_cpu_display_list_frame,
    compose_live_cpu_display_list_frame_with_metrics_reusing,
    compose_live_cpu_display_list_frame_with_metrics_reusing_damage, compose_live_cpu_frame,
    compose_live_cpu_frame_ref, compose_live_cpu_frame_ref_with_cursor,
    compose_live_cpu_frame_ref_with_cursor_asset, project_live_cpu_composed_frame,
};

#[test]
fn shared_cpu_source_moves_pixels_once_and_clones_the_arc() {
    let bytes = Arc::new(vec![0x5a; 16]);
    let allocation = bytes.as_ptr();
    let source: LiveSharedCpuBufferSource = LiveCpuBufferSource {
        handle: 7,
        size: Size {
            width: 2,
            height: 2,
        },
        stride: 8,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        generation: 11,
        bytes,
    }
    .into();

    assert_eq!(source.bytes.as_ptr(), allocation);
    let cloned = source.clone();
    assert!(std::ptr::eq(
        source.bytes.as_slice(),
        cloned.bytes.as_slice()
    ));
    assert_eq!(source.handle, cloned.handle);
    assert_eq!(source.generation, cloned.generation);
}

#[test]
fn cpu_mirror_projection_scales_pixels_and_leaves_bars_black() {
    let source = sophia_renderer_live::LiveCpuComposedFrame {
        size: Size {
            width: 2,
            height: 1,
        },
        stride: 8,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        bytes: Arc::new(vec![1, 2, 3, 0xff, 4, 5, 6, 0xff]),
    };
    let projected = project_live_cpu_composed_frame(
        &source,
        Size {
            width: 4,
            height: 3,
        },
        Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
        },
    )
    .unwrap();

    assert_eq!(&projected.bytes[0..4], &[1, 2, 3, 0xff]);
    assert_eq!(&projected.bytes[4..8], &[1, 2, 3, 0xff]);
    assert_eq!(&projected.bytes[8..12], &[4, 5, 6, 0xff]);
    assert_eq!(&projected.bytes[12..16], &[4, 5, 6, 0xff]);
    assert!(projected.bytes[32..48].iter().all(|byte| *byte == 0));
}

#[test]
fn production_scene_discards_late_patch_but_reports_missing_committed_base() {
    let size = Size {
        width: 2,
        height: 1,
    };
    let surface = SurfaceId::new(1, 1);
    let committed = [CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 72 },
            sophia_protocol::Size {
                width: 2,
                height: 1,
            },
        ),
        damage: Region::empty(),
    }];
    let mut scene = LiveProductionCpuScene::new(size);

    scene
        .apply_production_updates([LiveCpuBufferUpdate::Patch(LiveCpuBufferPatch {
            handle: 72,
            size,
            stride: 8,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 2,
            rect: Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            bytes: vec![1, 2, 3, 4],
        })])
        .unwrap();

    assert_eq!(scene.resident_buffer_count(), 0);
    assert_eq!(scene.missing_committed_buffer_count(&committed), 1);
}

#[test]
fn cpu_composition_blits_clipped_xrgb_layers() {
    let source = LiveCpuBufferSource {
        handle: 1,
        size: Size {
            width: 2,
            height: 2,
        },
        stride: 8,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        generation: 1,
        bytes: Arc::new(vec![0xff; 16]),
    };
    let report = compose_live_cpu_frame(
        Size {
            width: 3,
            height: 3,
        },
        &[LiveCpuCompositionLayer {
            geometry: Rect {
                x: 2,
                y: 2,
                width: 2,
                height: 2,
            },
            buffer: source,
        }],
    )
    .unwrap();

    assert_eq!(report.layers_input, 1);
    assert_eq!(report.layers_composed, 1);
    assert_eq!(report.nonzero_pixel_bytes, 4);
    assert_ne!(report.checksum, 0);
    assert_eq!(&report.frame.bytes[32..36], &[0xff; 4]);
}

#[test]
fn display_list_composition_reuses_uniquely_owned_frame_storage() {
    let size = Size {
        width: 4,
        height: 4,
    };
    let reusable = Arc::new(vec![0xaa; 64]);
    let allocation = reusable.as_ptr();
    let report = compose_live_cpu_display_list_frame_with_metrics_reusing(
        size,
        &[LiveCpuCompositionElementRef::Solid {
            opacity: 255,
            geometry: Rect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
            color: CompositorRgb8 {
                red: 1,
                green: 2,
                blue: 3,
            },
        }],
        None,
        LiveCpuFrameMetricsMode::DamageScopedEvidence,
        Some(reusable),
    )
    .unwrap();

    assert_eq!(report.frame.bytes.as_ptr(), allocation);
    assert!(report.frame.bytes[..16].iter().all(|byte| *byte == 0));
}

#[test]
fn display_list_content_identity_is_stable_across_metric_modes() {
    let size = Size {
        width: 4,
        height: 4,
    };
    let elements = [LiveCpuCompositionElementRef::Solid {
        opacity: 255,
        geometry: Rect {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        },
        color: CompositorRgb8 {
            red: 1,
            green: 2,
            blue: 3,
        },
    }];
    let exact = compose_live_cpu_display_list_frame_with_metrics_reusing(
        size,
        &elements,
        None,
        LiveCpuFrameMetricsMode::ExactPixels,
        None,
    )
    .unwrap();
    let damage_scoped = compose_live_cpu_display_list_frame_with_metrics_reusing(
        size,
        &elements,
        None,
        LiveCpuFrameMetricsMode::DamageScopedEvidence,
        None,
    )
    .unwrap();

    assert_eq!(exact.checksum, damage_scoped.checksum);
    assert_eq!(exact.nonzero_pixel_bytes, 16);
    assert_eq!(damage_scoped.nonzero_pixel_bytes, 1);
}

#[test]
fn cpu_display_list_preserves_solid_order_and_clips_to_output() {
    let size = Size {
        width: 3,
        height: 2,
    };
    let pixels = vec![0x11; 3 * 2 * 4];
    let report = compose_live_cpu_display_list_frame(
        size,
        &[
            LiveCpuCompositionElementRef::Layer(LiveCpuCompositionLayerRef {
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 3,
                    height: 2,
                },
                buffer: LiveCpuBufferSourceRef {
                    handle: 21,
                    size,
                    stride: 12,
                    format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                    generation: 1,
                    bytes: &pixels,
                },
            }),
            LiveCpuCompositionElementRef::Solid {
                opacity: 255,
                geometry: Rect {
                    x: 2,
                    y: -1,
                    width: 3,
                    height: 3,
                },
                color: CompositorRgb8 {
                    red: 0x70,
                    green: 0xb7,
                    blue: 0xff,
                },
            },
        ],
        None,
    )
    .unwrap();

    assert_eq!(report.layers_input, 2);
    assert_eq!(report.layers_composed, 2);
    assert_eq!(&report.frame.bytes[0..4], &[0x11; 4]);
    assert_eq!(&report.frame.bytes[8..12], &[0xff, 0xb7, 0x70, 0xff]);
    assert_eq!(&report.frame.bytes[20..24], &[0xff, 0xb7, 0x70, 0xff]);
}

#[test]
fn borrowed_fullscreen_composition_preserves_pixels_and_metrics() {
    let pixels = vec![0x5a; 1280 * 720 * 4];
    let report = compose_live_cpu_frame_ref(
        Size {
            width: 1280,
            height: 720,
        },
        &[LiveCpuCompositionLayerRef {
            geometry: Rect {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            },
            buffer: LiveCpuBufferSourceRef {
                handle: 7,
                size: Size {
                    width: 1280,
                    height: 720,
                },
                stride: 1280 * 4,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 1,
                bytes: &pixels,
            },
        }],
    )
    .unwrap();
    assert_eq!(report.frame.bytes.as_ref(), &pixels);
    assert_eq!(report.nonzero_pixel_bytes, 1280 * 720 * 4);
    assert_eq!(report.layers_composed, 1);
}

#[test]
fn borrowed_composition_clips_negative_geometry_by_rows() {
    let pixels = vec![0x33; 4 * 4 * 4];
    let report = compose_live_cpu_frame_ref(
        Size {
            width: 4,
            height: 4,
        },
        &[LiveCpuCompositionLayerRef {
            geometry: Rect {
                x: -2,
                y: -1,
                width: 4,
                height: 4,
            },
            buffer: LiveCpuBufferSourceRef {
                handle: 9,
                size: Size {
                    width: 4,
                    height: 4,
                },
                stride: 16,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 2,
                bytes: &pixels,
            },
        }],
    )
    .unwrap();
    assert_eq!(report.nonzero_pixel_bytes, 2 * 3 * 4);
    assert_eq!(&report.frame.bytes[..8], &[0x33; 8]);
    assert!(report.frame.bytes[8..16].iter().all(|byte| *byte == 0));
}

#[test]
fn cpu_composition_identity_changes_with_an_immutable_generation() {
    let size = Size {
        width: 2,
        height: 1,
    };
    let baseline = [0x5a; 8];
    let mut changed = baseline;
    changed[7] ^= 0xff;
    let first = compose_live_cpu_frame_ref(
        size,
        &[LiveCpuCompositionLayerRef {
            geometry: Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
            buffer: LiveCpuBufferSourceRef {
                handle: 13,
                size,
                stride: 8,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 2,
                bytes: &baseline,
            },
        }],
    )
    .unwrap();
    let second = compose_live_cpu_frame_ref(
        size,
        &[LiveCpuCompositionLayerRef {
            geometry: Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
            buffer: LiveCpuBufferSourceRef {
                handle: 13,
                size,
                stride: 8,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 1,
                bytes: &changed,
            },
        }],
    )
    .unwrap();
    assert_eq!(first.nonzero_pixel_bytes, 8);
    assert_eq!(second.nonzero_pixel_bytes, 8);
    assert_ne!(first.checksum, second.checksum);
}

#[test]
fn borrowed_composition_draws_a_high_contrast_software_cursor() {
    let size = Size {
        width: 16,
        height: 20,
    };
    let pixels = vec![0x22; 16 * 20 * 4];
    let layer = LiveCpuCompositionLayerRef {
        geometry: Rect {
            x: 0,
            y: 0,
            width: 16,
            height: 20,
        },
        buffer: LiveCpuBufferSourceRef {
            handle: 17,
            size,
            stride: 16 * 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: &pixels,
        },
    };
    let baseline = compose_live_cpu_frame_ref(size, &[layer]).unwrap();
    let report =
        compose_live_cpu_frame_ref_with_cursor(size, &[layer], Some(Point { x: 2.8, y: 3.2 }))
            .unwrap();

    assert!(
        report
            .frame
            .bytes
            .chunks_exact(4)
            .any(|pixel| pixel == [0xff; 4])
    );
    assert!(
        report
            .frame
            .bytes
            .chunks_exact(4)
            .any(|pixel| pixel == [0, 0, 0, 0xff])
    );
    assert_ne!(report.checksum, baseline.checksum);
}

#[test]
fn default_cursor_asset_has_stable_dimensions_and_hotspot() {
    let cursor = x11_core_left_ptr_cursor(1);
    assert_eq!((cursor.width(), cursor.height()), (10, 16));
    assert_eq!(cursor.hotspot(), (1, 1));
    assert_eq!(cursor.pixels().len(), 10 * 16 * 4);
}

#[test]
fn software_cursor_uses_the_resolved_asset_hotspot() {
    let mut pixels = vec![0; 2 * 2 * 4];
    pixels[12..16].copy_from_slice(&[0x10, 0x20, 0x30, 0xff]);
    let cursor = CursorAsset::new(2, 2, 1, 1, 1, pixels).unwrap();
    let report = compose_live_cpu_frame_ref_with_cursor_asset(
        Size {
            width: 5,
            height: 5,
        },
        &[],
        Some(Point { x: 3.0, y: 3.0 }),
        &cursor,
    )
    .unwrap();
    let offset = (3 * 5 + 3) * 4;

    assert_eq!(
        &report.frame.bytes[offset..offset + 4],
        &[0x10, 0x20, 0x30, 0xff]
    );
}

#[test]
fn production_scene_composes_only_the_visible_surface_order() {
    let size = Size {
        width: 2,
        height: 2,
    };
    let surface = SurfaceId::new(1, 1);
    let committed = CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            sophia_protocol::Size {
                width: 2,
                height: 2,
            },
        ),
        damage: Region::empty(),
    };
    let mut scene = LiveProductionCpuScene::new(size);
    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 1,
            size,
            stride: 8,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![0x55; 16]),
        })])
        .unwrap();
    scene.reconcile_buffer_residency(&[1]);

    let hidden = scene
        .compose_visible(std::slice::from_ref(&committed), &[], None, None)
        .unwrap();
    assert_eq!(hidden.layers_composed, 0);
    assert_eq!(hidden.nonzero_pixel_bytes, 0);

    let visible = scene
        .compose_visible(
            std::slice::from_ref(&committed),
            std::slice::from_ref(&surface),
            None,
            None,
        )
        .unwrap();
    assert_eq!(visible.layers_composed, 1);
    assert_eq!(visible.nonzero_pixel_bytes, 16);
}

#[test]
fn production_scene_keeps_display_list_attached_to_composed_primary_pixels() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 2,
            height: 2,
        },
        scale: 1,
    };
    let display_list = CompositorDisplayList::empty(output.id);
    let mut scene = LiveProductionCpuScene::new(output.size);

    scene
        .compose_display_list(output, &[], &display_list, None)
        .unwrap();
    let frames = scene.frames_for_outputs(&[output]).unwrap();

    assert_eq!(frames.len(), 1);
    assert_eq!(
        frames[0]
            .output_damage_snapshot
            .as_ref()
            .map(|snapshot| &snapshot.compositor_display_list),
        Some(&sophia_engine::CompositorDamageList::from(
            display_list.clone()
        )),
    );
}

#[test]
fn production_scene_reuses_unchanged_secondary_output_frames() {
    let primary = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 4,
            height: 3,
        },
        scale: 1,
    };
    let secondary = HeadlessOutput {
        id: OutputId::from_raw(2),
        size: Size {
            width: 3,
            height: 2,
        },
        scale: 1,
    };
    let display_list = CompositorDisplayList::empty(primary.id);
    let mut scene = LiveProductionCpuScene::new(primary.size);

    scene
        .compose_display_list(primary, &[], &display_list, None)
        .unwrap();
    let first = scene.frames_for_outputs(&[primary, secondary]).unwrap();
    scene
        .compose_display_list(primary, &[], &display_list, None)
        .unwrap();
    let second = scene.frames_for_outputs(&[primary, secondary]).unwrap();

    assert!(Arc::ptr_eq(&first[1].frame.bytes, &second[1].frame.bytes));
    assert_eq!(first[1].checksum, second[1].checksum);

    let resized_secondary = HeadlessOutput {
        size: Size {
            width: 2,
            height: 2,
        },
        ..secondary
    };
    let resized = scene
        .frames_for_outputs(&[primary, resized_secondary])
        .unwrap();
    assert!(!Arc::ptr_eq(
        &second[1].frame.bytes,
        &resized[1].frame.bytes
    ));
    assert_eq!(resized[1].frame.size, resized_secondary.size);
}

#[test]
fn production_scene_reconfiguration_preserves_buffers_and_invalidates_frames() {
    let original = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 2,
            height: 2,
        },
        scale: 1,
    };
    let replacement = HeadlessOutput {
        size: Size {
            width: 3,
            height: 2,
        },
        ..original
    };
    let mut scene = LiveProductionCpuScene::new(original.size);
    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 41,
            size: Size {
                width: 1,
                height: 1,
            },
            stride: 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![0xff; 4]),
        })])
        .unwrap();
    scene
        .compose_display_list(
            original,
            &[],
            &CompositorDisplayList::empty(original.id),
            None,
        )
        .unwrap();

    assert!(scene.reconfigure_output_size(replacement.size).unwrap());
    assert!(scene.contains_buffer(41));
    assert!(scene.frames_for_outputs(&[replacement]).is_err());

    scene
        .compose_display_list(
            replacement,
            &[],
            &CompositorDisplayList::empty(replacement.id),
            None,
        )
        .unwrap();
    assert_eq!(
        scene.frames_for_outputs(&[replacement]).unwrap()[0]
            .frame
            .size,
        replacement.size
    );
    assert!(!scene.reconfigure_output_size(replacement.size).unwrap());
}

#[test]
fn production_scene_metric_warmup_does_not_schedule_unchanged_content() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 2,
            height: 2,
        },
        scale: 1,
    };
    let display_list = CompositorDisplayList::empty(output.id);
    let mut scene = LiveProductionCpuScene::new(output.size);
    let mut checksums = Vec::new();

    for _ in 0..4 {
        checksums.push(
            scene
                .compose_display_list(output, &[], &display_list, None)
                .unwrap()
                .checksum,
        );
    }

    assert!(checksums.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(scene.exact_pixel_metric_frames(), 3);
    assert_eq!(scene.damage_scoped_metric_frames(), 1);
}

#[test]
fn damage_scoped_composition_preserves_pixels_outside_clipped_damage() {
    let size = Size {
        width: 4,
        height: 1,
    };
    let reusable = Arc::new(vec![0x7a; 16]);
    let report = compose_live_cpu_display_list_frame_with_metrics_reusing_damage(
        size,
        &[LiveCpuCompositionElementRef::Solid {
            opacity: 255,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 1,
            },
            color: CompositorRgb8 {
                red: 0x11,
                green: 0x22,
                blue: 0x33,
            },
        }],
        None,
        LiveCpuFrameMetricsMode::ExactPixels,
        Some(reusable),
        Some(&Region::single(Rect {
            x: -2,
            y: 0,
            width: 4,
            height: 1,
        })),
    )
    .unwrap();

    assert_eq!(
        &report.frame.bytes[..8],
        &[0x33, 0x22, 0x11, 0xff, 0x33, 0x22, 0x11, 0xff]
    );
    assert_eq!(&report.frame.bytes[8..], &[0x7a; 8]);
}

#[test]
fn damage_scoped_composition_clears_removed_pixels_and_restores_stacking() {
    let size = Size {
        width: 4,
        height: 1,
    };
    let background = CompositorRgb8 {
        red: 0x10,
        green: 0x20,
        blue: 0x30,
    };
    let old = compose_live_cpu_display_list_frame(
        size,
        &[
            LiveCpuCompositionElementRef::Solid {
                opacity: 255,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 1,
                },
                color: background,
            },
            LiveCpuCompositionElementRef::Solid {
                opacity: 255,
                geometry: Rect {
                    x: 1,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                color: CompositorRgb8 {
                    red: 0x90,
                    green: 0x80,
                    blue: 0x70,
                },
            },
        ],
        None,
    )
    .unwrap();
    let report = compose_live_cpu_display_list_frame_with_metrics_reusing_damage(
        size,
        &[
            LiveCpuCompositionElementRef::Solid {
                opacity: 255,
                geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 1,
                },
                color: background,
            },
            LiveCpuCompositionElementRef::Solid {
                opacity: 255,
                geometry: Rect {
                    x: 2,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                color: CompositorRgb8 {
                    red: 0xa0,
                    green: 0xb0,
                    blue: 0xc0,
                },
            },
        ],
        None,
        LiveCpuFrameMetricsMode::ExactPixels,
        Some(old.frame.bytes),
        Some(&Region::single(Rect {
            x: 1,
            y: 0,
            width: 2,
            height: 1,
        })),
    )
    .unwrap();

    assert_eq!(&report.frame.bytes[4..8], &[0x30, 0x20, 0x10, 0xff]);
    assert_eq!(&report.frame.bytes[8..12], &[0xc0, 0xb0, 0xa0, 0xff]);
    assert_eq!(report.layers_composed, 2);
}

#[test]
fn damage_scoped_composition_copies_a_shared_retained_frame() {
    let size = Size {
        width: 3,
        height: 1,
    };
    let reusable = Arc::new(vec![0xaa; 12]);
    let observer = reusable.clone();
    let report = compose_live_cpu_display_list_frame_with_metrics_reusing_damage(
        size,
        &[LiveCpuCompositionElementRef::Solid {
            opacity: 255,
            geometry: Rect {
                x: 1,
                y: 0,
                width: 1,
                height: 1,
            },
            color: CompositorRgb8 {
                red: 1,
                green: 2,
                blue: 3,
            },
        }],
        None,
        LiveCpuFrameMetricsMode::ExactPixels,
        Some(reusable),
        Some(&Region::single(Rect {
            x: 1,
            y: 0,
            width: 1,
            height: 1,
        })),
    )
    .unwrap();

    assert_eq!(observer.as_ref(), &[0xaa; 12]);
    assert!(!Arc::ptr_eq(&observer, &report.frame.bytes));
    assert_eq!(&report.frame.bytes[..4], &[0xaa; 4]);
    assert_eq!(&report.frame.bytes[4..8], &[3, 2, 1, 0xff]);
    assert_eq!(&report.frame.bytes[8..], &[0xaa; 4]);
}

#[test]
fn damage_scoped_composition_keeps_shared_storage_when_damage_is_empty() {
    let size = Size {
        width: 3,
        height: 1,
    };
    let reusable = Arc::new(vec![0xaa; 12]);
    let observer = reusable.clone();
    let report = compose_live_cpu_display_list_frame_with_metrics_reusing_damage(
        size,
        &[],
        None,
        LiveCpuFrameMetricsMode::DamageScopedEvidence,
        Some(reusable),
        Some(&Region::empty()),
    )
    .unwrap();

    assert!(Arc::ptr_eq(&observer, &report.frame.bytes));
    assert_eq!(report.frame.bytes.as_ref(), &[0xaa; 12]);
}

#[test]
fn damage_scoped_composition_clears_the_old_cursor_and_draws_the_new_cursor() {
    let size = Size {
        width: 32,
        height: 20,
    };
    let old =
        compose_live_cpu_display_list_frame(size, &[], Some(Point { x: 0.0, y: 0.0 })).unwrap();
    let damage = Region {
        rects: vec![
            Rect {
                x: 0,
                y: 0,
                width: 16,
                height: 16,
            },
            Rect {
                x: 16,
                y: 0,
                width: 16,
                height: 16,
            },
        ],
    };
    let report = compose_live_cpu_display_list_frame_with_metrics_reusing_damage(
        size,
        &[],
        Some(Point { x: 16.0, y: 0.0 }),
        LiveCpuFrameMetricsMode::ExactPixels,
        Some(old.frame.bytes),
        Some(&damage),
    )
    .unwrap();

    assert_eq!(&report.frame.bytes[..4], &[0; 4]);
    assert_eq!(&report.frame.bytes[16 * 4..17 * 4], &[0, 0, 0, 0xff]);
}

#[test]
fn damage_scoped_composition_falls_back_for_an_incompatible_baseline() {
    let size = Size {
        width: 2,
        height: 1,
    };
    let report = compose_live_cpu_display_list_frame_with_metrics_reusing_damage(
        size,
        &[LiveCpuCompositionElementRef::Solid {
            opacity: 255,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
            color: CompositorRgb8 {
                red: 0x11,
                green: 0x22,
                blue: 0x33,
            },
        }],
        None,
        LiveCpuFrameMetricsMode::ExactPixels,
        Some(Arc::new(vec![0xaa; 4])),
        Some(&Region::single(Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        })),
    )
    .unwrap();

    assert_eq!(
        report.frame.bytes.as_ref(),
        &[0x33, 0x22, 0x11, 0xff, 0x33, 0x22, 0x11, 0xff]
    );
}

#[test]
fn production_scene_uses_snapshot_damage_for_changed_surface_only() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 4,
            height: 1,
        },
        scale: 1,
    };
    let left = SurfaceId::new(1, 1);
    let right = SurfaceId::new(2, 1);
    let mut committed = [
        CommittedSurfaceState {
            surface: left,
            committed_generation: 1,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 1 },
                sophia_protocol::Size {
                    width: 2,
                    height: 1,
                },
            ),
            damage: Region::empty(),
        },
        CommittedSurfaceState {
            surface: right,
            committed_generation: 1,
            geometry: Rect {
                x: 2,
                y: 0,
                width: 2,
                height: 1,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 2 },
                sophia_protocol::Size {
                    width: 2,
                    height: 1,
                },
            ),
            damage: Region::empty(),
        },
    ];
    let display_list = CompositorDisplayList {
        output: output.id,
        commands: vec![
            CompositorDisplayCommand::Surface { surface: left },
            CompositorDisplayCommand::Surface { surface: right },
        ],
    };
    let mut scene = LiveProductionCpuScene::new(output.size);
    scene
        .apply_updates([
            LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                handle: 1,
                size: Size {
                    width: 2,
                    height: 1,
                },
                stride: 8,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 1,
                bytes: Arc::new(vec![0x11; 8]),
            }),
            LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                handle: 2,
                size: Size {
                    width: 2,
                    height: 1,
                },
                stride: 8,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 1,
                bytes: Arc::new(vec![0x22; 8]),
            }),
        ])
        .unwrap();
    scene
        .compose_display_list(output, &committed, &display_list, None)
        .unwrap();

    committed[0].committed_generation = 2;
    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 1,
            size: Size {
                width: 2,
                height: 1,
            },
            stride: 8,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 2,
            bytes: Arc::new(vec![0x33; 8]),
        })])
        .unwrap();
    let report = scene
        .compose_display_list(output, &committed, &display_list, None)
        .unwrap();

    assert_eq!(report.layers_composed, 1);
    assert_eq!(&report.frame.bytes[..8], &[0x33; 8]);
    assert_eq!(&report.frame.bytes[8..], &[0x22; 8]);
}
