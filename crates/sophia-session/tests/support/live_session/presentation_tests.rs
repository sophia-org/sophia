use super::*;
use crate::live_session::{
    LiveAdmissionAuthorityGroup, PersistentLiveLayout, production_authority_batch,
    wm_update_coordinator_batch,
};
use sophia_protocol::TransactionId;
use std::sync::Arc;

#[path = "../feedback_progress.rs"]
mod feedback_progress;

fn layer_snapshots_from_committed(
    committed_surfaces: &[CommittedSurfaceState],
) -> Vec<LayerSnapshot> {
    committed_surfaces
        .iter()
        .enumerate()
        .map(|(stack_rank, surface)| LayerSnapshot {
            input_region: None,
            translation: None,
            output: None,
            surface: surface.surface,
            authority_local_id: None,
            namespace: None,
            stack_rank: u32::try_from(stack_rank).unwrap_or(u32::MAX),
            geometry: surface.geometry,
            source_size: Size {
                width: (surface.geometry).width,
                height: (surface.geometry).height,
            },
            source: surface.buffer(),
            damage: surface.damage.clone(),
            opacity: 1.0,
            crop: None,
            transform: Transform::IDENTITY,
            generation: surface.committed_generation,
            resize_sync: ResizeSyncCapability::ImplicitOnly,
        })
        .collect()
}

#[test]
fn compatibility_surface_is_centered_without_resizing() {
    let geometry = center_geometry_without_scaling(
        Rect {
            x: 19,
            y: 27,
            width: 800,
            height: 600,
        },
        Size {
            width: 1280,
            height: 720,
        },
    );
    assert_eq!(geometry.x, 240);
    assert_eq!(geometry.y, 60);
    assert_eq!(geometry.width, 800);
    assert_eq!(geometry.height, 600);
}

#[test]
fn oversized_compatibility_surface_keeps_size_and_anchors_at_origin() {
    let geometry = center_geometry_without_scaling(
        Rect {
            x: 19,
            y: 27,
            width: 1920,
            height: 1080,
        },
        Size {
            width: 1280,
            height: 720,
        },
    );
    assert_eq!(geometry.x, 0);
    assert_eq!(geometry.y, 0);
    assert_eq!(geometry.width, 1920);
    assert_eq!(geometry.height, 1080);
}

#[test]
fn terminal_readiness_is_scoped_to_the_focused_surface() {
    let focused = SurfaceId::new(21, 1);
    let secondary = SurfaceId::new(22, 1);
    let mut scene = LiveProductionCpuScene::new(Size {
        width: 4,
        height: 1,
    });
    let committed = vec![
        test_committed_cpu_surface(
            focused,
            Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
            1,
        ),
        test_committed_cpu_surface(
            secondary,
            Rect {
                x: 2,
                y: 0,
                width: 2,
                height: 1,
            },
            2,
        ),
    ];
    scene
        .apply_updates([
            sophia_backend_live::LiveCpuBufferUpdate::Replace(test_cpu_buffer(1, [0xff; 8])),
            sophia_backend_live::LiveCpuBufferUpdate::Replace(test_cpu_buffer(
                2,
                [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0xff],
            )),
        ])
        .unwrap();
    scene.reconcile_buffer_residency(&[1, 2]);

    assert!(!scene.surface_has_visual_detail(&committed, focused));
    assert!(scene.surface_has_visual_detail(&committed, secondary));

    scene
        .apply_updates([sophia_backend_live::LiveCpuBufferUpdate::Replace(
            test_cpu_buffer(1, [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0xff]),
        )])
        .unwrap();
    assert!(scene.surface_has_visual_detail(&committed, focused));
}

#[test]
fn focused_surface_is_composed_above_an_overlapping_client() {
    let focused = SurfaceId::new(31, 1);
    let secondary = SurfaceId::new(32, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 2,
        height: 1,
    };
    let mut scene = LiveProductionCpuScene::new(Size {
        width: 2,
        height: 1,
    });
    let committed = vec![
        test_committed_cpu_surface(focused, geometry, 1),
        test_committed_cpu_surface(secondary, geometry, 2),
    ];
    let focused_pixels = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
    let secondary_pixels = [0, 0, 0, 0xff, 0, 0, 0, 0xff];
    scene
        .apply_updates([
            sophia_backend_live::LiveCpuBufferUpdate::Replace(test_cpu_buffer(1, focused_pixels)),
            sophia_backend_live::LiveCpuBufferUpdate::Replace(test_cpu_buffer(2, secondary_pixels)),
        ])
        .unwrap();
    scene.reconcile_buffer_residency(&[1, 2]);

    assert_eq!(
        scene.compose(&committed, None, None).unwrap().frame.bytes,
        secondary_pixels.to_vec().into()
    );
    assert_eq!(
        scene
            .compose(&committed, Some(focused), None)
            .unwrap()
            .frame
            .bytes,
        focused_pixels.to_vec().into()
    );
}

fn test_committed_cpu_surface(
    surface: SurfaceId,
    geometry: Rect,
    handle: u64,
) -> CommittedSurfaceState {
    CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle },
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::single(geometry),
    }
}

fn test_cpu_buffer(handle: u64, bytes: [u8; 8]) -> sophia_backend_live::LiveCpuBufferSource {
    sophia_backend_live::LiveCpuBufferSource {
        handle,
        size: Size {
            width: 2,
            height: 1,
        },
        stride: 8,
        format: X_AUTHORITY_CPU_BUFFER_FORMAT_XRGB8888,
        generation: 1,
        bytes: Arc::new(bytes.to_vec()),
    }
}

#[test]
fn committed_snapshot_preserves_surface_generation_in_render_layers() {
    let layers = layer_snapshots_from_committed(&[CommittedSurfaceState {
        surface: sophia_protocol::SurfaceId::new(9, 1),
        committed_generation: 4,
        geometry: Rect {
            x: 10,
            y: 20,
            width: 300,
            height: 200,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 99 },
            sophia_protocol::Size {
                width: 300,
                height: 200,
            },
        ),
        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: 300,
            height: 200,
        }),
    }]);

    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].generation, 4);
    assert_eq!(layers[0].source, BufferSource::CpuBuffer { handle: 99 });
}

#[test]
fn authority_batch_commits_once_and_fans_out_one_snapshot() {
    let outputs = [17u64, 18]
        .into_iter()
        .map(|id| sophia_engine::HeadlessOutput {
            id: sophia_protocol::OutputId::from_raw(id),
            size: Size {
                width: 640,
                height: 480,
            },
            scale: 1,
        })
        .collect::<Vec<_>>();
    let surface = sophia_protocol::SurfaceId::new(17, 1);
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: sophia_protocol::TransactionId::from_raw(90),
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: Rect {
            x: 4,
            y: 8,
            width: 632,
            height: 464,
        },
        presentation_extent: Size {
            width: 632,
            height: 464,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 18 },
            sophia_protocol::Size {
                width: 632,
                height: 464,
            },
        ),

        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: 632,
            height: 464,
        }),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };

    let group = sophia_backend_live::LiveProductionAuthorityGroup {
        transaction: sophia_protocol::TransactionId::from_raw(90),
        transactions: vec![transaction],
        cpu_buffer_updates: Vec::new(),
        removed_surfaces: Vec::new(),
        present_submissions: Vec::new(),
        software_present_submissions: Vec::new(),
    };
    let report = runtime
        .run_authority_transactions(sophia_backend_live::LiveAuthorityTransactionRun {
            groups: std::slice::from_ref(&group),
            event_count: 1,
            native_scanout: None,
            native_head_frames: None,
            wm_update: None,
        })
        .unwrap();

    assert_eq!(
        report
            .engine
            .runtime
            .runtime_state
            .authority_transactions_committed,
        1
    );
    assert_eq!(runtime.committed_surfaces().len(), 1);
    assert_eq!(runtime.committed_surfaces()[0].committed_generation, 1);
    for index in 0..runtime.output_count() {
        let committed = runtime.output_committed(index).unwrap();
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].committed_generation, 1);
    }
}

#[test]
fn same_iteration_software_admission_release_replaces_original_observation() {
    let transaction = TransactionId::from_raw(190);
    let surface = SurfaceId::new(191, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 500,
        height: 500,
    };
    let pixels = SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: Size {
            width: (geometry).width,
            height: (geometry).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 192 },
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),

        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let software_present = sophia_x_authority::XAuthoritySoftwarePresentSubmission {
        transaction,
        surface,
        acquire_fence: None,
        idle_fence: None,
    };
    let mut observed = wm_update_coordinator_batch(transaction);
    observed.transactions.push(pixels.clone());
    observed.software_present_submissions.push(software_present);
    let cpu_update = sophia_x_authority::XAuthorityCpuBufferUpdate::Replace(
        sophia_x_authority::XAuthorityCpuBufferSnapshot {
            handle: 192,
            drawable: sophia_x_authority::XResourceId::new(192, 1),
            size: Size {
                width: geometry.width,
                height: geometry.height,
            },
            stride: u32::try_from(geometry.width * 4).unwrap(),
            format: sophia_backend_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![
                0;
                usize::try_from(geometry.width * geometry.height * 4)
                    .unwrap()
            ]),
        },
    );
    observed.cpu_buffer_updates.push(cpu_update.clone());
    let mut layout = PersistentLiveLayout::default();
    layout.cpu_buffer_sizes.insert(
        192,
        Size {
            width: geometry.width,
            height: geometry.height,
        },
    );
    layout
        .released_admission_groups
        .push_back(LiveAdmissionAuthorityGroup {
            transaction,
            transactions: vec![pixels],
            cpu_buffer_updates: vec![cpu_update],
            present_submissions: Vec::new(),
            software_present_submissions: vec![software_present],
            superseded: false,
        });

    let (projected, released) = layout.projected_batch(&observed);
    let production = production_authority_batch(&projected, &released, &layout).unwrap();

    assert!(projected.transactions.is_empty());
    assert!(projected.cpu_buffer_updates.is_empty());
    assert!(projected.software_present_submissions.is_empty());
    assert_eq!(released.len(), 1);
    assert_eq!(production.groups.len(), 1);
    assert_eq!(production.groups[0].transactions.len(), 1);
    assert_eq!(production.groups[0].cpu_buffer_updates.len(), 1);
    assert_eq!(production.groups[0].software_present_submissions.len(), 1);
    production.validate().unwrap();
}

#[test]
fn duplicate_software_present_fails_before_renderer_registration() {
    let transaction = TransactionId::from_raw(193);
    let surface = SurfaceId::new(194, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 2,
        height: 1,
    };
    let pixels = SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: Size {
            width: (geometry).width,
            height: (geometry).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 195 },
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),

        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let submission = sophia_backend_live::LiveProductionSoftwarePresentSubmission {
        candidate: pixels.key(),
        source_size: Size {
            width: geometry.width,
            height: geometry.height,
        },
        transaction,
        surface,
        acquire_fence: None,
        idle_fence: None,
    };
    let batch = sophia_backend_live::LiveProductionAuthorityBatch {
        groups: vec![sophia_backend_live::LiveProductionAuthorityGroup {
            transaction,
            transactions: vec![pixels],
            cpu_buffer_updates: Vec::new(),
            removed_surfaces: Vec::new(),
            present_submissions: Vec::new(),
            software_present_submissions: vec![submission, submission],
        }],
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    };

    assert_eq!(
        batch.validate(),
        Err("production authority group contains a duplicate software Present")
    );
}
