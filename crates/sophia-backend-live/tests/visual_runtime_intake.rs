#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]

use sophia_backend_live::{
    LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL, LiveProductionCpuFrameQueueStatus,
    LiveProductionMixedLayerSource, LiveProductionNativeSuspendError,
    LiveProductionNativeSuspendOutcome, LiveProductionNativeSuspendReport,
    LiveProductionPageFlipWatchdogStatus, LiveProductionRetainedFrameQueueRequirement,
    LiveProductionRetainedSceneQueueStatus, LiveProductionScanoutContent,
    LiveProductionVisualRuntime, finish_live_production_native_suspend,
    live_production_mixed_layer_order, live_production_projection_requires_gpu_scanout,
    live_production_retained_frame_requirement, live_production_retained_projection_admitted,
    live_production_retained_surface_order, live_production_should_preserve_gpu_output,
    live_production_transactions_require_gpu_scanout,
    reduce_live_production_abandoned_scanout_count, reduce_live_production_cpu_frame_queue,
    reduce_live_production_frame_defer, reduce_live_production_page_flip_watchdog,
    reduce_live_production_retained_frame_queue, reduce_live_production_retained_scene_queue,
};
use sophia_engine::HeadlessOutput;
use sophia_protocol::{
    AuthorityKind, BufferSource, OutputId, Rect, Region, Size, SurfaceId, SurfaceTransaction,
    SurfaceTransactionReadiness, TransactionId, TransactionOutcome,
};
use std::sync::Arc;
use std::{cell::Cell, io, time::Duration};

fn output() -> HeadlessOutput {
    HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 640,
            height: 480,
        },
        scale: 1,
    }
}

fn initial_transaction(previous_committed_generation: u64) -> SurfaceTransaction {
    SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(1),
        authority: AuthorityKind::SophiaX,
        surface: SurfaceId::new(1, 1),
        namespace: None,
        target_geometry: Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        },
        presentation_extent: sophia_protocol::Size {
            width: 640,
            height: 480,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            sophia_protocol::Size {
                width: 640,
                height: 480,
            },
        ),

        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        }),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation,
    }
}

#[test]
fn mixed_layer_order_preserves_cpu_overlay_above_gpu_clients() {
    let lower_gpu = SurfaceId::new(1, 1);
    let current_gpu = SurfaceId::new(2, 1);
    let upper_cpu = SurfaceId::new(3, 1);

    assert_eq!(
        live_production_mixed_layer_order(
            &[lower_gpu, current_gpu, upper_cpu],
            current_gpu,
            &[upper_cpu],
            &[lower_gpu],
        ),
        vec![
            LiveProductionMixedLayerSource::RetainedDmaBuf(lower_gpu),
            LiveProductionMixedLayerSource::CurrentDmaBuf,
            LiveProductionMixedLayerSource::Cpu(upper_cpu),
        ]
    );
}

#[test]
fn mixed_layer_order_preserves_cpu_client_below_current_gpu() {
    let lower_cpu = SurfaceId::new(1, 1);
    let current_gpu = SurfaceId::new(2, 1);

    assert_eq!(
        live_production_mixed_layer_order(
            &[lower_cpu, current_gpu],
            current_gpu,
            &[lower_cpu],
            &[],
        ),
        vec![
            LiveProductionMixedLayerSource::Cpu(lower_cpu),
            LiveProductionMixedLayerSource::CurrentDmaBuf,
        ]
    );
}

#[test]
fn initial_surface_enters_visual_state_only_through_engine_commit() {
    let mut runtime = LiveProductionVisualRuntime::new(&[output()], None).expect("runtime");

    assert!(runtime.committed_surfaces().is_empty());
    let prepared = runtime
        .prepare_authority_transactions(TransactionId::from_raw(1), &[initial_transaction(0)], &[])
        .expect("prepare initial authority transaction");

    assert_eq!(prepared.authority_commits.len(), 1);
    assert_eq!(
        prepared.authority_commits[0].outcome,
        TransactionOutcome::Committed
    );
    assert_eq!(runtime.committed_surfaces().len(), 1);
    assert_eq!(runtime.committed_surfaces()[0].committed_generation, 1);
}

#[test]
fn initial_surface_cannot_seed_a_forged_generation() {
    let mut runtime = LiveProductionVisualRuntime::new(&[output()], None).expect("runtime");

    let prepared = runtime
        .prepare_authority_transactions(TransactionId::from_raw(1), &[initial_transaction(7)], &[])
        .expect("prepare malformed initial authority transaction");

    assert_eq!(
        prepared.authority_commits[0].outcome,
        TransactionOutcome::RejectedStaleSurface
    );
    assert!(runtime.committed_surfaces().is_empty());
}

#[test]
fn retained_order_omits_policy_surfaces_without_a_committed_engine_state() {
    let committed_surface = SurfaceId::new(1, 1);
    let pending_surface = SurfaceId::new(2, 1);
    let mut committed = initial_transaction(0);
    committed.surface = committed_surface;
    let mut runtime = LiveProductionVisualRuntime::new(&[output()], None).expect("runtime");
    runtime
        .prepare_authority_transactions(TransactionId::from_raw(1), &[committed], &[])
        .expect("commit retained surface");

    assert_eq!(
        live_production_retained_surface_order(
            &[pending_surface, committed_surface],
            runtime.committed_surfaces(),
        ),
        vec![committed_surface]
    );
}

#[test]
fn native_head_frame_admission_rejects_duplicate_and_incomplete_output_batches() {
    let mut second = output();
    second.id = OutputId::from_raw(2);

    for (frames, expected) in [
        (
            vec![(output().id, Vec::new()), (output().id, Vec::new())],
            "native head composition named a logical output more than once",
        ),
        (
            vec![(output().id, Vec::new())],
            "native head composition did not cover every logical output",
        ),
    ] {
        let mut runtime =
            LiveProductionVisualRuntime::new(&[output(), second], None).expect("runtime");
        let prepared = runtime
            .prepare_authority_transactions(TransactionId::from_raw(1), &[], &[])
            .expect("prepare empty authority cycle");
        let error = runtime
            .run_prepared_authority_transactions(prepared, 0, None, Some(frames), None)
            .expect_err("malformed native output batch must fail closed");
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn provisional_extended_topology_composes_each_head_from_one_committed_scene() {
    use sophia_backend_live::{
        LibdrmNativeOutputTiming, LiveOutputAuthorityHeadTarget,
        LiveOutputAuthorityLogicalViewport, LiveResolvedOutputTopology, NativeMirrorGrouping,
    };
    use sophia_engine::RenderHeadId;
    use sophia_protocol::{OutputHeadMapping, OutputTransform, OutputVrrPolicy};
    use sophia_renderer_live::{
        LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuBufferSource, LiveCpuBufferUpdate,
        LiveProductionCpuScene,
    };

    let mut runtime = LiveProductionVisualRuntime::new(&[output()], None).expect("runtime");
    runtime
        .prepare_authority_transactions(TransactionId::from_raw(1), &[initial_transaction(0)], &[])
        .expect("commit one spanning surface");
    let mut scene = LiveProductionCpuScene::new(output().size);
    scene
        .apply_updates([LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 1,
            size: output().size,
            stride: 640 * 4,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![0x7f; 640 * 480 * 4]),
        })])
        .unwrap();

    let left = OutputId::from_raw(1);
    let right = OutputId::from_raw(2);
    let half = Size {
        width: 320,
        height: 480,
    };
    let resolved = LiveResolvedOutputTopology {
        primary_output: left,
        primary_heads: [
            (left, RenderHeadId::from_raw(11)),
            (right, RenderHeadId::from_raw(12)),
        ]
        .into_iter()
        .collect(),
        outputs: vec![
            HeadlessOutput {
                id: left,
                size: half,
                scale: 1,
            },
            HeadlessOutput {
                id: right,
                size: half,
                scale: 1,
            },
        ],
        logical_viewports: vec![
            LiveOutputAuthorityLogicalViewport {
                output: left,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 320,
                    height: 480,
                },
            },
            LiveOutputAuthorityLogicalViewport {
                output: right,
                logical: Rect {
                    x: 320,
                    y: 0,
                    width: 320,
                    height: 480,
                },
            },
        ],
        disabled_heads: Vec::new(),
        targets: vec![
            LiveOutputAuthorityHeadTarget {
                head: RenderHeadId::from_raw(11),
                target_generation: 2,
                output: left,
                timing: LibdrmNativeOutputTiming::new(320, 480, 60_000),
                native_size: half,
                transform: OutputTransform::Normal,
                mapping: OutputHeadMapping::Exact,
                vrr: OutputVrrPolicy::Disabled,
            },
            LiveOutputAuthorityHeadTarget {
                head: RenderHeadId::from_raw(12),
                target_generation: 2,
                output: right,
                timing: LibdrmNativeOutputTiming::new(320, 480, 60_000),
                native_size: half,
                transform: OutputTransform::Normal,
                mapping: OutputHeadMapping::Exact,
                vrr: OutputVrrPolicy::Disabled,
            },
        ],
        mirror_grouping: NativeMirrorGrouping::none(),
    };
    let frames = runtime
        .compose_output_topology_head_frames(&scene, &resolved, 9)
        .expect("both extended viewports should lower independently");
    assert_eq!(frames.len(), 2);
    assert_eq!(
        frames.iter().map(|frame| frame.head).collect::<Vec<_>>(),
        vec![RenderHeadId::from_raw(11), RenderHeadId::from_raw(12)]
    );
    assert_eq!(
        frames[0]
            .frame
            .output_damage_snapshot
            .as_ref()
            .unwrap()
            .output,
        resolved.outputs[0]
    );
    assert_eq!(
        frames[1]
            .frame
            .output_damage_snapshot
            .as_ref()
            .unwrap()
            .output,
        resolved.outputs[1]
    );
}

#[test]
fn revoked_native_suspend_is_idempotent_without_active_scanout() {
    let output = output();
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).expect("headless runtime");

    let first = runtime
        .suspend_revoked_native_scanout(&[output])
        .expect("first revoked suspension");
    let second = runtime
        .suspend_revoked_native_scanout(&[output])
        .expect("duplicate revoked suspension");

    assert_eq!(first.abandoned_scanouts, 0);
    assert_eq!(
        first.outcome,
        LiveProductionNativeSuspendOutcome::ForcedDetachRevoked
    );
    assert_eq!(first.skipped_present, None);
    assert_eq!(second, first);
    assert_eq!(runtime.output_count(), 1);
}

#[test]
fn forced_detach_counts_logical_and_physical_head_owners() {
    assert_eq!(reduce_live_production_abandoned_scanout_count(1, 2), 3);
    assert_eq!(
        reduce_live_production_abandoned_scanout_count(usize::MAX, 1),
        usize::MAX
    );
}

fn native_drain_failure() -> Result<bool, Box<dyn std::error::Error>> {
    Err(io::Error::other("original callback drain failure").into())
}

#[test]
fn drain_failure_forces_detach_before_returning_original_error() {
    let detached = Cell::new(None);

    let error = finish_live_production_native_suspend(native_drain_failure(), |outcome| {
        detached.set(Some(outcome));
        Ok(LiveProductionNativeSuspendReport {
            outcome,
            abandoned_scanouts: 2,
            skipped_present: None,
        })
    })
    .expect_err("drain failure must remain fatal");

    assert_eq!(
        detached.get(),
        Some(LiveProductionNativeSuspendOutcome::ForcedDetachDrainError)
    );
    let structured = error
        .downcast_ref::<LiveProductionNativeSuspendError>()
        .expect("drain failure should retain structured detach evidence");
    assert!(structured.forced_detach_established());
    assert_eq!(structured.detach_report.unwrap().abandoned_scanouts, 2);
    assert_eq!(
        structured
            .drain_error
            .downcast_ref::<io::Error>()
            .expect("original drain error type must be retained")
            .kind(),
        io::ErrorKind::Other
    );
    assert!(
        error
            .to_string()
            .contains("original callback drain failure")
    );
    assert!(error.to_string().contains("abandoned_scanouts=2"));
}

#[test]
fn drain_and_detach_failures_are_aggregated_without_losing_original_error() {
    let detached = Cell::new(false);

    let error = finish_live_production_native_suspend(native_drain_failure(), |_| {
        detached.set(true);
        Err(io::Error::other("physical owner detach failure").into())
    })
    .expect_err("both failures must remain fatal");

    assert!(detached.get());
    let structured = error
        .downcast_ref::<LiveProductionNativeSuspendError>()
        .expect("drain failure should retain structured detach evidence");
    assert!(!structured.forced_detach_established());
    assert_eq!(structured.detach_report, None);
    assert!(
        structured
            .detach_error
            .as_deref()
            .and_then(|error| error.downcast_ref::<io::Error>())
            .is_some(),
        "detach failure type must be retained"
    );
    assert!(
        error
            .to_string()
            .contains("original callback drain failure")
    );
    assert!(error.to_string().contains("physical owner detach failure"));
}

#[test]
fn cpu_frame_queue_suppresses_only_matching_cpu_content() {
    let checksum = 42;
    let cpu = Some(LiveProductionScanoutContent::Cpu {
        frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(1),
        checksum,
    });
    let mixed = Some(LiveProductionScanoutContent::MixedPresent {
        frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(2),
        transaction: TransactionId::from_raw(9),
        nonzero_rgb_pixels: 1,
    });

    assert_eq!(
        reduce_live_production_cpu_frame_queue(cpu, None, None, false, false, checksum),
        LiveProductionCpuFrameQueueStatus::UnchangedPending
    );
    assert_eq!(
        reduce_live_production_cpu_frame_queue(None, cpu, None, false, false, checksum),
        LiveProductionCpuFrameQueueStatus::UnchangedSubmitted
    );
    assert_eq!(
        reduce_live_production_cpu_frame_queue(None, None, cpu, false, true, checksum),
        LiveProductionCpuFrameQueueStatus::UnchangedPresented
    );
    assert_eq!(
        reduce_live_production_cpu_frame_queue(None, None, mixed, false, false, checksum),
        LiveProductionCpuFrameQueueStatus::Queued
    );
    assert_eq!(
        reduce_live_production_cpu_frame_queue(None, None, cpu, false, false, checksum + 1),
        LiveProductionCpuFrameQueueStatus::Queued
    );
    assert_eq!(
        reduce_live_production_cpu_frame_queue(None, None, mixed, true, false, checksum),
        LiveProductionCpuFrameQueueStatus::GpuFrameOwned
    );
    assert_eq!(
        reduce_live_production_cpu_frame_queue(mixed, None, None, false, false, checksum),
        LiveProductionCpuFrameQueueStatus::GpuFrameOwned
    );
    assert_eq!(
        reduce_live_production_cpu_frame_queue(None, mixed, None, false, false, checksum),
        LiveProductionCpuFrameQueueStatus::GpuFrameOwned
    );
}

#[test]
fn retained_scene_queue_suppresses_the_matching_newest_owned_frame() {
    let matching = Some(LiveProductionScanoutContent::HeadComposition {
        frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(1),
        logical_content_checksum: 42,
        nonzero_rgb_pixels: 1,
    });
    let different = Some(LiveProductionScanoutContent::HeadComposition {
        frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(2),
        logical_content_checksum: 43,
        nonzero_rgb_pixels: 1,
    });
    let present = Some(LiveProductionScanoutContent::MixedPresent {
        frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(3),
        transaction: TransactionId::from_raw(9),
        nonzero_rgb_pixels: 1,
    });

    for (owned, expected) in [
        (
            [matching, None, None, None],
            LiveProductionRetainedSceneQueueStatus::UnchangedPending,
        ),
        (
            [None, matching, None, None],
            LiveProductionRetainedSceneQueueStatus::UnchangedRendering,
        ),
        (
            [None, None, matching, None],
            LiveProductionRetainedSceneQueueStatus::UnchangedSubmitted,
        ),
        (
            [None, None, None, matching],
            LiveProductionRetainedSceneQueueStatus::UnchangedPresented,
        ),
    ] {
        assert_eq!(
            reduce_live_production_retained_scene_queue(owned[0], owned[1], owned[2], owned[3], 42,),
            expected
        );
    }
    assert_eq!(
        reduce_live_production_retained_scene_queue(different, None, None, matching, 42,),
        LiveProductionRetainedSceneQueueStatus::Queue,
        "a different newer frame must not be hidden by matching displayed pixels"
    );
    assert_eq!(
        reduce_live_production_retained_scene_queue(present, None, None, matching, 42,),
        LiveProductionRetainedSceneQueueStatus::Queue,
        "present-owned pixels have no interchangeable logical-scene checksum"
    );
    assert_eq!(
        reduce_live_production_retained_scene_queue(None, None, None, None, 42),
        LiveProductionRetainedSceneQueueStatus::Queue
    );
}

#[test]
fn software_present_queue_requires_fresh_retirement_for_identical_pixels() {
    let matching = Some(LiveProductionScanoutContent::HeadComposition {
        frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(1),
        logical_content_checksum: 42,
        nonzero_rgb_pixels: 1,
    });

    for owned in [
        [matching, None, None, None],
        [None, matching, None, None],
        [None, None, matching, None],
        [None, None, None, matching],
    ] {
        assert_eq!(
            reduce_live_production_retained_frame_queue(
                LiveProductionRetainedFrameQueueRequirement::FreshRetirement,
                owned[0],
                owned[1],
                owned[2],
                owned[3],
                42,
            ),
            LiveProductionRetainedSceneQueueStatus::Queue,
            "Present feedback cannot reuse an identical owned scene"
        );
    }
}

#[test]
fn shell_content_retirement_is_output_local_and_cannot_reuse_identical_pixels() {
    let presented = Some(LiveProductionScanoutContent::HeadComposition {
        frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(1),
        logical_content_checksum: 42,
        nonzero_rgb_pixels: 1,
    });
    let required = live_production_retained_frame_requirement(true);
    let ordinary = live_production_retained_frame_requirement(false);

    assert_eq!(
        required,
        LiveProductionRetainedFrameQueueRequirement::FreshRetirement
    );
    assert_eq!(
        ordinary,
        LiveProductionRetainedFrameQueueRequirement::LatestScene
    );
    assert_eq!(
        reduce_live_production_retained_frame_queue(required, None, None, None, presented, 42,),
        LiveProductionRetainedSceneQueueStatus::Queue,
    );
    assert_eq!(
        reduce_live_production_retained_frame_queue(ordinary, None, None, None, presented, 42,),
        LiveProductionRetainedSceneQueueStatus::UnchangedPresented,
    );
}

#[test]
fn mirror_content_identity_tracks_logical_pixels_not_head_local_metrics() {
    let frame = sophia_backend_live::LiveProductionNativeFrameId::from_raw(7);
    let cpu = LiveProductionScanoutContent::Cpu {
        frame,
        checksum: 99,
    };
    assert_eq!(cpu.cpu_checksum(), Some(99));
    assert_eq!(cpu.source_label(), "cpu");
    assert!(cpu.same_logical_identity(cpu));
    assert!(
        !cpu.same_logical_identity(LiveProductionScanoutContent::Cpu {
            frame,
            checksum: 100,
        })
    );

    let retained = LiveProductionScanoutContent::RetainedMixed {
        logical_content_checksum: None,
        requires_retirement: false,
        frame,
        nonzero_rgb_pixels: 10,
    };
    assert!(
        retained.same_logical_identity(LiveProductionScanoutContent::RetainedMixed {
            logical_content_checksum: None,
            requires_retirement: false,
            frame,
            nonzero_rgb_pixels: 20,
        })
    );
    assert_eq!(retained.cpu_checksum(), None);
    assert_eq!(retained.source_label(), "retained_mixed");
    assert!(!retained.same_logical_identity(cpu));
}

#[test]
fn unchanged_initial_modeset_frame_requires_one_event_bearing_submission() {
    let checksum = 42;
    let cpu = Some(LiveProductionScanoutContent::Cpu {
        frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(1),
        checksum,
    });

    assert_eq!(
        reduce_live_production_cpu_frame_queue(None, None, cpu, false, false, checksum),
        LiveProductionCpuFrameQueueStatus::BaselineRequired
    );
    assert_eq!(
        reduce_live_production_cpu_frame_queue(None, None, cpu, false, true, checksum),
        LiveProductionCpuFrameQueueStatus::UnchangedPresented
    );
}

#[test]
fn page_flip_watchdog_fails_closed_after_its_hard_boundary() {
    assert_eq!(
        reduce_live_production_page_flip_watchdog(None, LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL),
        LiveProductionPageFlipWatchdogStatus::Idle
    );
    assert_eq!(
        reduce_live_production_page_flip_watchdog(
            Some(LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL - Duration::from_millis(1)),
            LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL,
        ),
        LiveProductionPageFlipWatchdogStatus::Healthy
    );
    assert_eq!(
        reduce_live_production_page_flip_watchdog(
            Some(LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL),
            LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL,
        ),
        LiveProductionPageFlipWatchdogStatus::HardStall
    );
}

#[test]
fn gpu_scanout_preservation_follows_post_batch_active_transactions() {
    let mut gpu = initial_transaction(0);
    gpu.content = sophia_protocol::SurfaceContentSet::singleton(
        BufferSource::DmaBuf { handle: 7 },
        gpu.raster_extent(),
    );
    let cpu = initial_transaction(0);

    assert!(live_production_transactions_require_gpu_scanout(
        std::slice::from_ref(&gpu)
    ));
    assert!(!live_production_transactions_require_gpu_scanout(&[cpu]));
    assert!(!live_production_transactions_require_gpu_scanout(&[]));
    assert!(live_production_projection_requires_gpu_scanout(
        std::slice::from_ref(&gpu),
        std::slice::from_ref(&gpu.surface),
    ));
    assert!(!live_production_projection_requires_gpu_scanout(
        std::slice::from_ref(&gpu),
        &[],
    ));
}

#[test]
fn visibility_change_forces_a_frame_unless_a_retained_gpu_projection_is_queued() {
    assert!(!reduce_live_production_frame_defer(true, true, false));
    assert!(reduce_live_production_frame_defer(true, true, true));
    assert!(reduce_live_production_frame_defer(true, false, false));
    assert!(!reduce_live_production_frame_defer(false, false, false));
}

#[test]
fn current_cpu_pixels_bypass_the_stale_retained_projection_shortcut() {
    assert!(live_production_retained_projection_admitted(
        true, false, false
    ));
    assert!(!live_production_retained_projection_admitted(
        true, true, false
    ));
    assert!(live_production_retained_projection_admitted(
        true, true, true
    ));
    assert!(!live_production_retained_projection_admitted(
        false, false, true
    ));
}

#[test]
fn submitted_gpu_present_blocks_a_cpu_frame_from_superseding_it() {
    assert!(live_production_should_preserve_gpu_output(
        true, true, false, false, false,
    ));
    assert!(live_production_should_preserve_gpu_output(
        true, true, false, true, false,
    ));
    assert!(!live_production_should_preserve_gpu_output(
        false, true, false, false, false,
    ));
    assert!(!live_production_should_preserve_gpu_output(
        true, false, false, false, false,
    ));
}

#[test]
fn only_logical_scene_content_can_own_a_cpu_progress_target() {
    let frame = sophia_backend_live::LiveProductionNativeFrameId::from_raw(91);
    assert_eq!(
        LiveProductionScanoutContent::Cpu {
            frame,
            checksum: 41,
        }
        .logical_checksum(),
        Some(41),
    );
    assert_eq!(
        LiveProductionScanoutContent::HeadComposition {
            frame,
            logical_content_checksum: 42,
            nonzero_rgb_pixels: 1,
        }
        .logical_checksum(),
        Some(42),
    );
    assert_eq!(
        LiveProductionScanoutContent::MixedPresent {
            frame,
            transaction: TransactionId::from_raw(92),
            nonzero_rgb_pixels: 1,
        }
        .logical_checksum(),
        None,
    );
    assert_eq!(
        LiveProductionScanoutContent::RetainedMixed {
            logical_content_checksum: None,
            requires_retirement: false,
            frame,
            nonzero_rgb_pixels: 1,
        }
        .logical_checksum(),
        None,
    );
}

#[test]
fn deduplicated_gpu_projection_survives_a_changed_presentation_order() {
    // No submission is currently in flight, and the matching newest scene
    // already has a native owner: deduplication legitimately queues no frame.
    let preserve = live_production_should_preserve_gpu_output(true, false, false, true, true);
    assert!(preserve, "an unchanged owned GPU frame is not a CPU source");
    assert!(reduce_live_production_frame_defer(false, true, preserve));
}

#[test]
fn removing_the_last_gpu_surface_still_admits_cpu_composition() {
    let preserve = live_production_should_preserve_gpu_output(true, false, false, true, false);
    assert!(!preserve);
    assert!(!reduce_live_production_frame_defer(false, true, preserve));
}
