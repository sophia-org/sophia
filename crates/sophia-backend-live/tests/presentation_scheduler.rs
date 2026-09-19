#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]

use std::fs::File;
use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use sophia_backend_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LivePresentationResourceSession,
    LiveProductionAuthorityBatch, LiveProductionAuthorityGroup,
    LiveProductionFirstVisibilityReason, LiveProductionNativeFrameId,
    LiveProductionPageFlipRetirement, LiveProductionPresentDisposition, LiveProductionPresentGate,
    LiveProductionPresentScheduler, LiveProductionPresentSubmission,
    LiveProductionSubmittedPresent, LiveRetainedRendererImageLayer,
};
use sophia_engine::{HeadlessEngine, ProductionSessionCoordinator};
use sophia_protocol::{
    AuthorityKind, BufferHandle, BufferSource, CommittedSurfaceState, DRM_FORMAT_MOD_INVALID,
    DmaBufDescriptor, DmaBufPlaneDescriptor, OutputId, Rect, Region, Size, SurfaceId,
    SurfaceTransaction, SurfaceTransactionReadiness, TransactionId, Transform,
};
use sophia_renderer_live::{LiveCompositionPlacement, LiveRendererImageId};

#[path = "support/first_present_visibility.rs"]
mod first_present_visibility;

#[path = "support/present_frame_tick.rs"]
mod present_frame_tick;

fn fd() -> OwnedFd {
    File::open("/dev/null").unwrap().into()
}

fn descriptor(handle: BufferHandle) -> DmaBufDescriptor {
    DmaBufDescriptor {
        handle,
        size: Size {
            width: 64,
            height: 48,
        },
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        modifier: DRM_FORMAT_MOD_INVALID,
        plane_count: 1,
        planes: [
            Some(DmaBufPlaneDescriptor {
                offset: 0,
                stride: 256,
            }),
            None,
            None,
            None,
        ],
    }
}

fn scheduler_batch(
    transaction: TransactionId,
    surface: SurfaceId,
    handle: BufferHandle,
) -> LiveProductionAuthorityBatch {
    LiveProductionAuthorityBatch {
        groups: vec![LiveProductionAuthorityGroup {
            transaction,
            transactions: vec![SurfaceTransaction {
                input_region: None,
                transaction,
                authority: AuthorityKind::SophiaX,
                surface,
                namespace: None,
                target_geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 48,
                },
                presentation_extent: sophia_protocol::Size {
                    width: 64,
                    height: 48,
                },
                content: sophia_protocol::SurfaceContentSet::singleton(
                    BufferSource::DmaBuf {
                        handle: handle.raw(),
                    },
                    sophia_protocol::Size {
                        width: 64,
                        height: 48,
                    },
                ),

                damage: Region::single(Rect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 48,
                }),
                readiness: SurfaceTransactionReadiness::Ready,
                timeout_msec: 250,
                previous_committed_generation: 0,
            }],
            cpu_buffer_updates: Vec::new(),
            removed_surfaces: Vec::new(),
            present_submissions: vec![LiveProductionPresentSubmission {
                transaction,
                surface,
                buffer: handle,
                x_offset: 0,
                y_offset: 0,
                acquire_fence: None,
                idle_fence: None,
                layout_disposition: LiveProductionPresentDisposition::Immediate,
            }],
            software_present_submissions: Vec::new(),
        }],
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    }
}

fn scheduler_batch_with_disposition(
    transaction: TransactionId,
    surface: SurfaceId,
    handle: BufferHandle,
    disposition: LiveProductionPresentDisposition,
) -> LiveProductionAuthorityBatch {
    let mut batch = scheduler_batch(transaction, surface, handle);
    batch.groups[0].present_submissions[0].layout_disposition = disposition;
    batch
}

fn in_flight_present(
    transaction: TransactionId,
    surface: SurfaceId,
) -> LiveProductionSubmittedPresent {
    in_flight_present_for_outputs(transaction, surface, [OutputId::from_raw(1)])
}

fn in_flight_present_for_outputs(
    transaction: TransactionId,
    surface: SurfaceId,
    outputs: impl IntoIterator<Item = OutputId>,
) -> LiveProductionSubmittedPresent {
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 64,
        height: 48,
    };
    let buffer = BufferSource::DmaBuf { handle: 900 };
    let production = ProductionSessionCoordinator::new(HeadlessEngine::default())
        .with_committed_surfaces(vec![CommittedSurfaceState::with_source(
            surface,
            1,
            geometry,
            buffer,
            Size {
                width: geometry.width,
                height: geometry.height,
            },
            Region::empty(),
        )]);
    let prepared = production.prepare_present_transaction(&SurfaceTransaction {
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
        content: sophia_protocol::SurfaceContentSet::new(
            sophia_protocol::Size {
                width: geometry.width,
                height: geometry.height,
            },
            vec![
                sophia_protocol::SurfaceContentVariant {
                    variant: 1,
                    source: buffer,
                    pixel_size: sophia_protocol::Size {
                        width: geometry.width,
                        height: geometry.height,
                    },
                    density_millis: 1_000,
                    transform: sophia_protocol::SurfaceRasterTransform::Normal,
                    fidelity: sophia_protocol::SurfaceContentFidelity::AuthorityRaster,
                    damage: Region::empty(),
                },
                sophia_protocol::SurfaceContentVariant {
                    variant: 2,
                    source: BufferSource::CpuBuffer { handle: 4 },
                    pixel_size: sophia_protocol::Size {
                        width: geometry.width / 2,
                        height: geometry.height / 2,
                    },
                    density_millis: 500,
                    transform: sophia_protocol::SurfaceRasterTransform::Normal,
                    fidelity: sophia_protocol::SurfaceContentFidelity::AuthorityRaster,
                    damage: Region::empty(),
                },
            ],
        )
        .unwrap(),

        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    });
    let frames = outputs
        .into_iter()
        .enumerate()
        .map(|(index, output)| {
            (
                output,
                sophia_backend_live::LiveProductionNativeFrameId::from_raw(
                    u64::try_from(index).unwrap() + 1,
                ),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let clock_output = *frames.keys().next().unwrap();
    LiveProductionSubmittedPresent::new(
        frames,
        clock_output,
        sophia_protocol::SurfaceTransactionKey {
            transaction,
            surface,
            target_buffer: buffer,
        },
        transaction,
        surface,
        prepared,
        LiveRetainedRendererImageLayer {
            image_id: LiveRendererImageId::from_raw(901),
            size: Size {
                width: 64,
                height: 48,
            },
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            placement: LiveCompositionPlacement {
                target: geometry,
                clip: None,
                transform: Transform::IDENTITY,
                alpha: 1.0,
                sampling: sophia_engine::HeadSamplingClass::Exact,
            },
        },
    )
    .unwrap()
}

#[test]
fn in_flight_candidate_retains_its_cpu_content_variants() {
    let transaction = TransactionId::from_raw(879);
    let surface = SurfaceId::new(880, 1);
    let mut scheduler = LiveProductionPresentScheduler::default();
    scheduler.mark_rendering(in_flight_present(transaction, surface));

    assert_eq!(
        scheduler.retained_cpu_buffer_handles().collect::<Vec<_>>(),
        vec![4]
    );
}

#[test]
fn submitted_present_joins_outputs_after_independent_submission_and_retirement() {
    let transaction = TransactionId::from_raw(880);
    let surface = SurfaceId::new(881, 1);
    let output_a = OutputId::from_raw(3);
    let output_b = OutputId::from_raw(9);
    let mut scheduler = LiveProductionPresentScheduler::default();
    scheduler.mark_rendering(in_flight_present_for_outputs(
        transaction,
        surface,
        [output_a, output_b],
    ));

    assert_eq!(
        scheduler.in_flight_frame(output_a),
        Some(LiveProductionNativeFrameId::from_raw(1))
    );
    assert_eq!(
        scheduler.in_flight_frame(output_b),
        Some(LiveProductionNativeFrameId::from_raw(2))
    );
    assert_eq!(scheduler.in_flight_frame(OutputId::from_raw(99)), None);
    assert_eq!(scheduler.mark_output_submitted(output_a).unwrap(), None);
    assert!(scheduler.submitted_frame(output_a).is_some());
    assert_eq!(scheduler.submitted_frame(output_b), None);
    assert_eq!(scheduler.unsubmitted_frame(output_a), None);
    assert_eq!(
        scheduler.unsubmitted_frame(output_b),
        Some(LiveProductionNativeFrameId::from_raw(2))
    );
    assert_eq!(
        scheduler
            .mark_output_retired(LiveProductionPageFlipRetirement {
                output: output_a,
                ust: 900,
                msc: 1_113_395,
            })
            .unwrap(),
        None
    );
    assert_eq!(scheduler.unsubmitted_frame(output_a), None);
    assert_eq!(
        scheduler.unsubmitted_frame(output_b),
        Some(LiveProductionNativeFrameId::from_raw(2))
    );
    assert_eq!(
        scheduler.mark_output_submitted(output_b).unwrap(),
        Some(transaction)
    );
    assert!(scheduler.has_submitted());
    assert_eq!(
        scheduler
            .mark_output_retired(LiveProductionPageFlipRetirement {
                output: output_b,
                ust: 1_200,
                msc: 4_202,
            })
            .unwrap(),
        Some(sophia_engine::TransactionPresentationTerminal::Presented {
            logical_sequence: transaction.raw(),
            ust_usec: 1_200,
        })
    );
    let submitted = scheduler.take_submitted().unwrap();
    assert_eq!(submitted.transaction, transaction);
    assert_eq!(
        submitted.presentation_clock(),
        Some(LiveProductionPageFlipRetirement {
            output: output_a,
            ust: 900,
            msc: 1_113_395,
        })
    );
}

#[test]
fn asynchronous_present_retains_one_transaction_until_scanout_submission() {
    let transaction = TransactionId::from_raw(902);
    let surface = SurfaceId::new(903, 1);
    let mut scheduler = LiveProductionPresentScheduler::default();
    let mut resources = LivePresentationResourceSession::default();

    scheduler.mark_rendering(in_flight_present(transaction, surface));

    assert!(scheduler.has_rendering());
    assert!(scheduler.has_in_flight());
    assert_eq!(
        scheduler.poll_gate(&mut resources, Instant::now()).unwrap(),
        LiveProductionPresentGate::SubmittedInFlight
    );
    assert_eq!(
        scheduler.promote_rendering_to_submitted(),
        Some(transaction)
    );
    assert!(!scheduler.has_rendering());
    assert!(scheduler.has_submitted());
    assert_eq!(
        scheduler
            .take_submitted()
            .map(|present| present.transaction),
        Some(transaction)
    );
    assert!(!scheduler.has_in_flight());
}

#[test]
fn queued_present_is_not_runnable_while_an_earlier_present_is_rendering() {
    let first_transaction = TransactionId::from_raw(40);
    let second_transaction = TransactionId::from_raw(41);
    let surface = SurfaceId::new(42, 1);
    let second_handle = BufferHandle::from_raw(43);
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(second_handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();

    scheduler.mark_rendering(in_flight_present(first_transaction, surface));
    scheduler
        .enqueue_group(
            &scheduler_batch(second_transaction, surface, second_handle).groups[0],
            &[],
            &mut resources,
            Instant::now(),
        )
        .unwrap();

    assert!(scheduler.has_queued());
    assert!(scheduler.has_eligible());
    assert!(scheduler.has_rendering());
    assert!(!scheduler.has_runnable_queued());

    assert_eq!(
        scheduler
            .take_rendering()
            .map(|present| present.transaction),
        Some(first_transaction)
    );
    assert!(scheduler.has_runnable_queued());
}

#[test]
fn production_present_scheduler_owns_delay_and_controlled_rejection_gates() {
    let handle = BufferHandle::from_raw(37);
    let transaction = TransactionId::from_raw(38);
    let surface = SurfaceId::new(39, 1);
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default().with_controls(
        Some(Duration::from_millis(50)),
        true,
        false,
    );
    let now = Instant::now();
    scheduler
        .enqueue_group(
            &scheduler_batch(transaction, surface, handle).groups[0],
            &[],
            &mut resources,
            now,
        )
        .unwrap();
    assert_eq!(
        scheduler.front().map(|queued| queued.surface),
        Some(surface)
    );

    assert_eq!(
        scheduler.poll_gate(&mut resources, now).unwrap(),
        LiveProductionPresentGate::WaitingAcquire
    );
    assert_eq!(scheduler.acquire_waits(), 1);
    assert_eq!(
        scheduler
            .poll_gate(&mut resources, now + Duration::from_millis(50))
            .unwrap(),
        LiveProductionPresentGate::Reject(transaction)
    );
    assert_eq!(scheduler.controlled_rejections(), 1);
    assert!(!scheduler.has_queued());
}

#[test]
fn queued_present_rebases_offset_and_clip_to_atomic_layout() {
    let handle = BufferHandle::from_raw(47);
    let transaction = TransactionId::from_raw(48);
    let surface = SurfaceId::new(49, 1);
    let mut batch = scheduler_batch(transaction, surface, handle);
    batch.groups[0].present_submissions[0].x_offset = 3;
    batch.groups[0].present_submissions[0].y_offset = -4;
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();
    scheduler
        .enqueue_group(&batch.groups[0], &[], &mut resources, Instant::now())
        .unwrap();
    let geometry = Rect {
        x: 1280,
        y: 720,
        width: 1280,
        height: 720,
    };

    scheduler.reproject_surface(surface, geometry);

    let queued = scheduler.front().unwrap();
    assert_eq!(queued.surface_clip, geometry);
    assert_eq!(queued.candidate.target_geometry, geometry);
    assert_eq!(
        queued.target,
        Rect {
            x: 1283,
            y: 716,
            ..geometry
        }
    );
}

#[test]
fn queued_present_owns_only_its_exact_surface_transaction() {
    let kitty_handle = BufferHandle::from_raw(402);
    let kitty_transaction = TransactionId::from_raw(403);
    let kitty_surface = SurfaceId::new(404, 1);
    let mut batch = scheduler_batch(kitty_transaction, kitty_surface, kitty_handle);
    batch.groups.insert(
        0,
        LiveProductionAuthorityGroup {
            transaction: TransactionId::from_raw(198),
            transactions: vec![SurfaceTransaction {
                input_region: None,
                transaction: TransactionId::from_raw(198),
                authority: AuthorityKind::SophiaX,
                surface: SurfaceId::new(405, 1),
                namespace: None,
                target_geometry: Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 14,
                },
                presentation_extent: sophia_protocol::Size {
                    width: 2560,
                    height: 14,
                },
                content: sophia_protocol::SurfaceContentSet::singleton(
                    BufferSource::CpuBuffer { handle: 406 },
                    sophia_protocol::Size {
                        width: 2560,
                        height: 14,
                    },
                ),

                damage: Region::empty(),
                readiness: SurfaceTransactionReadiness::Ready,
                timeout_msec: 250,
                previous_committed_generation: 1,
            }],
            cpu_buffer_updates: Vec::new(),
            removed_surfaces: Vec::new(),
            present_submissions: Vec::new(),
            software_present_submissions: Vec::new(),
        },
    );
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(kitty_handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();

    let rejected = scheduler
        .enqueue_group(&batch.groups[1], &[], &mut resources, Instant::now())
        .unwrap();

    assert!(rejected.is_empty());
    let queued = scheduler.front().expect("Kitty Present should be queued");
    assert_eq!(queued.candidate.transaction, kitty_transaction);
    assert_eq!(queued.candidate.surface, kitty_surface);
}

#[test]
fn dma_buf_authority_group_without_matching_present_fails_closed() {
    let transaction = TransactionId::from_raw(501);
    let surface = SurfaceId::new(502, 1);
    let handle = BufferHandle::from_raw(503);
    let mut batch = scheduler_batch(transaction, surface, handle);
    batch.groups[0].present_submissions.clear();

    assert_eq!(
        batch.validate(),
        Err("production DMA-BUF transactions and Presents are not exact pairs")
    );
}

#[test]
fn mismatched_present_candidate_is_a_controlled_rejection() {
    let handle = BufferHandle::from_raw(407);
    let transaction = TransactionId::from_raw(408);
    let surface = SurfaceId::new(409, 1);
    let mut batch = scheduler_batch(transaction, surface, handle);
    batch.groups[0].transactions[0].transaction = TransactionId::from_raw(410);
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();

    let rejected = scheduler.enqueue_group(&batch.groups[0], &[], &mut resources, Instant::now());

    assert!(rejected.is_err());
    assert!(!scheduler.has_queued());
    assert_eq!(scheduler.controlled_rejections(), 0);
    assert_eq!(resources.presentation_count(), 0);
}

#[test]
fn newly_queued_present_uses_the_committed_presentation_layout() {
    let handle = BufferHandle::from_raw(50);
    let transaction = TransactionId::from_raw(51);
    let surface = SurfaceId::new(52, 1);
    let mut batch = scheduler_batch(transaction, surface, handle);
    batch.groups[0].transactions[0].target_geometry = Rect {
        x: 80,
        y: 60,
        width: 1280,
        height: 1426,
    };
    let geometry = Rect {
        x: 0,
        y: 14,
        width: 1280,
        height: 1426,
    };
    let layout = [sophia_protocol::LayerSnapshot {
        input_region: None,
        translation: None,
        output: None,
        surface,
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry,
        source_size: Size {
            width: geometry.width,
            height: geometry.height,
        },
        source: batch.groups[0].transactions[0].target_buffer(),
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: sophia_protocol::Transform::IDENTITY,
        generation: 1,
        resize_sync: sophia_protocol::ResizeSyncCapability::ImplicitOnly,
    }];
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();

    scheduler
        .enqueue_group(&batch.groups[0], &layout, &mut resources, Instant::now())
        .unwrap();

    let queued = scheduler.front().unwrap();
    assert_eq!(queued.target, geometry);
    assert_eq!(queued.surface_clip, geometry);
}

#[test]
fn committed_epoch_present_waits_until_surface_is_visible() {
    let handle = BufferHandle::from_raw(90);
    let transaction = TransactionId::from_raw(91);
    let surface = SurfaceId::new(92, 1);
    let epoch = TransactionId::from_raw(93);
    let batch = scheduler_batch_with_disposition(
        transaction,
        surface,
        handle,
        LiveProductionPresentDisposition::StageLayout { epoch },
    );
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();
    scheduler
        .enqueue_group(&batch.groups[0], &[], &mut resources, Instant::now())
        .unwrap();

    assert_eq!(scheduler.commit_layout_epoch(epoch), 1);
    assert!(scheduler.has_layout_deferred());
    assert!(!scheduler.has_eligible());
    assert_eq!(
        scheduler
            .release_layout_deferred_for_surfaces(&[], &[])
            .released,
        0
    );
    assert!(!scheduler.has_eligible());
    let committed = [CommittedSurfaceState {
        surface,
        committed_generation: 7,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 64,
            height: 48,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 98 },
            sophia_protocol::Size {
                width: 64,
                height: 48,
            },
        ),
        damage: Region::empty(),
    }];
    assert_eq!(
        scheduler
            .release_layout_deferred_for_surfaces(&[surface], &committed)
            .released,
        1
    );
    assert!(scheduler.has_eligible());
    assert_eq!(
        scheduler
            .front()
            .map(|queued| queued.submission.transaction),
        Some(transaction)
    );
    assert_eq!(
        scheduler
            .front()
            .map(|queued| queued.candidate.previous_committed_generation),
        Some(7)
    );
}

#[test]
fn aborted_epoch_rejects_its_staged_present() {
    let handle = BufferHandle::from_raw(94);
    let transaction = TransactionId::from_raw(95);
    let surface = SurfaceId::new(96, 1);
    let epoch = TransactionId::from_raw(97);
    let batch = scheduler_batch_with_disposition(
        transaction,
        surface,
        handle,
        LiveProductionPresentDisposition::StageLayout { epoch },
    );
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();
    scheduler
        .enqueue_group(&batch.groups[0], &[], &mut resources, Instant::now())
        .unwrap();

    let report = scheduler.abort_layout_epoch(epoch);

    assert_eq!(report.rejected, [transaction]);
    assert!(!scheduler.has_queued());
}

#[test]
fn present_released_after_its_layout_epoch_aborted_is_rejected() {
    let handle = BufferHandle::from_raw(104);
    let transaction = TransactionId::from_raw(105);
    let surface = SurfaceId::new(106, 1);
    let epoch = TransactionId::from_raw(107);
    let batch = scheduler_batch_with_disposition(
        transaction,
        surface,
        handle,
        LiveProductionPresentDisposition::StageLayout { epoch },
    );
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();

    assert!(scheduler.abort_layout_epoch(epoch).rejected.is_empty());
    let rejected = scheduler
        .enqueue_group(&batch.groups[0], &[], &mut resources, Instant::now())
        .unwrap();

    assert_eq!(rejected, [transaction]);
    assert!(!scheduler.has_queued());
    assert_eq!(scheduler.controlled_rejections(), 1);
}

#[test]
fn present_released_after_commit_runs_when_its_surface_is_visible() {
    let handle = BufferHandle::from_raw(108);
    let transaction = TransactionId::from_raw(109);
    let surface = SurfaceId::new(110, 1);
    let epoch = TransactionId::from_raw(111);
    let batch = scheduler_batch_with_disposition(
        transaction,
        surface,
        handle,
        LiveProductionPresentDisposition::StageLayout { epoch },
    );
    let layout = [sophia_protocol::LayerSnapshot {
        input_region: None,
        translation: None,
        output: None,
        surface,
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry: batch.groups[0].transactions[0].target_geometry,
        source_size: Size {
            width: (batch.groups[0].transactions[0].target_geometry).width,
            height: (batch.groups[0].transactions[0].target_geometry).height,
        },
        source: batch.groups[0].transactions[0].target_buffer(),
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: sophia_protocol::ResizeSyncCapability::ImplicitOnly,
    }];
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();

    assert_eq!(scheduler.commit_layout_epoch(epoch), 0);
    let rejected = scheduler
        .enqueue_group(&batch.groups[0], &layout, &mut resources, Instant::now())
        .unwrap();

    assert!(rejected.is_empty());
    assert!(scheduler.has_eligible());
    assert_eq!(
        scheduler.poll_gate(&mut resources, Instant::now()).unwrap(),
        LiveProductionPresentGate::Ready(transaction)
    );
}

#[test]
fn aborting_one_epoch_does_not_settle_another_epoch() {
    let first_epoch = TransactionId::from_raw(980);
    let second_epoch = TransactionId::from_raw(981);
    let first_transaction = TransactionId::from_raw(982);
    let second_transaction = TransactionId::from_raw(983);
    let first_surface = SurfaceId::new(984, 1);
    let second_surface = SurfaceId::new(985, 1);
    let first_handle = BufferHandle::from_raw(986);
    let second_handle = BufferHandle::from_raw(987);
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(first_handle), vec![fd()])
        .unwrap();
    resources
        .register_source(descriptor(second_handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();
    for batch in [
        scheduler_batch_with_disposition(
            first_transaction,
            first_surface,
            first_handle,
            LiveProductionPresentDisposition::StageLayout { epoch: first_epoch },
        ),
        scheduler_batch_with_disposition(
            second_transaction,
            second_surface,
            second_handle,
            LiveProductionPresentDisposition::StageLayout {
                epoch: second_epoch,
            },
        ),
    ] {
        scheduler
            .enqueue_group(&batch.groups[0], &[], &mut resources, Instant::now())
            .unwrap();
    }

    assert_eq!(
        scheduler.abort_layout_epoch(first_epoch).rejected,
        [first_transaction]
    );
    assert!(scheduler.has_layout_deferred());
    assert_eq!(scheduler.commit_layout_epoch(second_epoch), 1);
}

#[test]
fn unrelated_present_remains_eligible_while_layout_surface_is_staged() {
    let staged_handle = BufferHandle::from_raw(62);
    let immediate_handle = BufferHandle::from_raw(63);
    let staged_transaction = TransactionId::from_raw(64);
    let immediate_transaction = TransactionId::from_raw(65);
    let staged_surface = SurfaceId::new(66, 1);
    let immediate_surface = SurfaceId::new(67, 1);
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(staged_handle), vec![fd()])
        .unwrap();
    resources
        .register_source(descriptor(immediate_handle), vec![fd()])
        .unwrap();
    let staged = scheduler_batch_with_disposition(
        staged_transaction,
        staged_surface,
        staged_handle,
        LiveProductionPresentDisposition::StageLayout {
            epoch: TransactionId::from_raw(1),
        },
    );
    let immediate = scheduler_batch(immediate_transaction, immediate_surface, immediate_handle);
    let mut scheduler = LiveProductionPresentScheduler::default();
    scheduler
        .enqueue_group(&staged.groups[0], &[], &mut resources, Instant::now())
        .unwrap();
    scheduler
        .enqueue_group(&immediate.groups[0], &[], &mut resources, Instant::now())
        .unwrap();

    assert!(scheduler.has_layout_deferred());
    assert!(scheduler.has_eligible());
    assert_eq!(
        scheduler.poll_gate(&mut resources, Instant::now()).unwrap(),
        LiveProductionPresentGate::Ready(immediate_transaction)
    );
    assert_eq!(
        scheduler
            .front()
            .map(|queued| queued.submission.transaction),
        Some(immediate_transaction)
    );
}

#[test]
fn layout_epoch_keeps_only_the_newest_present_per_surface() {
    let first_handle = BufferHandle::from_raw(67);
    let second_handle = BufferHandle::from_raw(68);
    let first_transaction = TransactionId::from_raw(69);
    let second_transaction = TransactionId::from_raw(70);
    let surface = SurfaceId::new(71, 1);
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(first_handle), vec![fd()])
        .unwrap();
    resources
        .register_source(descriptor(second_handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let epoch = TransactionId::from_raw(1);
    let first_batch = scheduler_batch_with_disposition(
        first_transaction,
        surface,
        first_handle,
        LiveProductionPresentDisposition::StageLayout { epoch },
    );
    let second_batch = scheduler_batch_with_disposition(
        second_transaction,
        surface,
        second_handle,
        LiveProductionPresentDisposition::StageLayout { epoch },
    );
    let first_superseded = scheduler
        .enqueue_group(&first_batch.groups[0], &[], &mut resources, Instant::now())
        .unwrap();
    let second_superseded = scheduler
        .enqueue_group(&second_batch.groups[0], &[], &mut resources, Instant::now())
        .unwrap();

    assert!(first_superseded.is_empty());
    assert_eq!(second_superseded, [first_transaction]);
    assert_eq!(
        scheduler
            .front()
            .map(|queued| queued.submission.transaction),
        Some(second_transaction)
    );
    assert_eq!(scheduler.commit_layout_epoch(epoch), 1);
}

#[test]
fn immediate_overload_keeps_only_the_newest_pending_present() {
    let surface = SurfaceId::new(701, 1);
    let first_transaction = TransactionId::from_raw(702);
    let second_transaction = TransactionId::from_raw(703);
    let first_handle = BufferHandle::from_raw(704);
    let second_handle = BufferHandle::from_raw(705);
    let mut resources = LivePresentationResourceSession::default();
    for handle in [first_handle, second_handle] {
        resources
            .register_source(descriptor(handle), vec![fd()])
            .unwrap();
    }
    let mut scheduler = LiveProductionPresentScheduler::default();

    let first_superseded = scheduler
        .enqueue_group(
            &scheduler_batch(first_transaction, surface, first_handle).groups[0],
            &[],
            &mut resources,
            Instant::now(),
        )
        .unwrap();
    let second_superseded = scheduler
        .enqueue_group(
            &scheduler_batch(second_transaction, surface, second_handle).groups[0],
            &[],
            &mut resources,
            Instant::now(),
        )
        .unwrap();

    assert!(first_superseded.is_empty());
    assert_eq!(second_superseded, [first_transaction]);
    assert_eq!(
        scheduler
            .front()
            .map(|queued| queued.submission.transaction),
        Some(second_transaction)
    );
    assert_eq!(scheduler.pending_supersessions(), 1);
    assert_eq!(scheduler.max_pending_queued(), 1);
    assert_eq!(scheduler.max_total_queued(), 1);
}

#[test]
fn sustained_overload_keeps_queue_and_presentation_ownership_bounded() {
    let surface = SurfaceId::new(721, 1);
    let handle = BufferHandle::from_raw(722);
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();

    for raw in 1..=512 {
        let transaction = TransactionId::from_raw(800 + raw);
        let superseded = scheduler
            .enqueue_group(
                &scheduler_batch(transaction, surface, handle).groups[0],
                &[],
                &mut resources,
                Instant::now(),
            )
            .unwrap();
        for transaction in superseded {
            resources.reject(transaction).unwrap();
        }
        assert_eq!(resources.presentation_count(), 1);
    }

    assert_eq!(scheduler.pending_supersessions(), 511);
    assert_eq!(scheduler.max_pending_queued(), 1);
    assert_eq!(scheduler.max_total_queued(), 1);
    assert_eq!(resources.max_presentation_count(), 2);
}

#[test]
fn newly_visible_layout_work_preserves_one_present_per_surface() {
    let epoch = TransactionId::from_raw(711);
    let first_transaction = TransactionId::from_raw(712);
    let second_transaction = TransactionId::from_raw(713);
    let first_surface = SurfaceId::new(714, 1);
    let second_surface = SurfaceId::new(715, 1);
    let first_handle = BufferHandle::from_raw(716);
    let second_handle = BufferHandle::from_raw(717);
    let mut resources = LivePresentationResourceSession::default();
    for handle in [first_handle, second_handle] {
        resources
            .register_source(descriptor(handle), vec![fd()])
            .unwrap();
    }
    let mut scheduler = LiveProductionPresentScheduler::default();
    for (transaction, surface, handle) in [
        (first_transaction, first_surface, first_handle),
        (second_transaction, second_surface, second_handle),
    ] {
        scheduler
            .enqueue_group(
                &scheduler_batch_with_disposition(
                    transaction,
                    surface,
                    handle,
                    LiveProductionPresentDisposition::StageLayout { epoch },
                )
                .groups[0],
                &[],
                &mut resources,
                Instant::now(),
            )
            .unwrap();
    }
    assert_eq!(scheduler.commit_layout_epoch(epoch), 2);
    let committed = [
        CommittedSurfaceState {
            surface: first_surface,
            committed_generation: 1,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 64,
                height: 48,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 718 },
                sophia_protocol::Size {
                    width: 64,
                    height: 48,
                },
            ),
            damage: Region::empty(),
        },
        CommittedSurfaceState {
            surface: second_surface,
            committed_generation: 1,
            geometry: Rect {
                x: 64,
                y: 0,
                width: 64,
                height: 48,
            },
            content: sophia_protocol::SurfaceContentSet::singleton(
                BufferSource::CpuBuffer { handle: 719 },
                sophia_protocol::Size {
                    width: 64,
                    height: 48,
                },
            ),
            damage: Region::empty(),
        },
    ];

    let report = scheduler
        .release_layout_deferred_for_surfaces(&[first_surface, second_surface], &committed);

    assert_eq!(report.released, 2);
    assert!(report.superseded.is_empty());
    assert_eq!(
        scheduler
            .front()
            .map(|queued| queued.submission.transaction),
        Some(first_transaction)
    );
    assert_eq!(
        scheduler
            .pop_front()
            .map(|queued| queued.submission.transaction),
        Some(first_transaction)
    );
    assert_eq!(
        scheduler
            .front()
            .map(|queued| queued.submission.transaction),
        Some(second_transaction)
    );
    assert_eq!(scheduler.pending_supersessions(), 0);
    assert_eq!(scheduler.max_pending_queued(), 2);
    assert_eq!(scheduler.max_total_queued(), 2);
}

#[test]
fn later_epoch_present_does_not_supersede_another_surface() {
    let epoch = TransactionId::from_raw(730);
    let first_transaction = TransactionId::from_raw(731);
    let second_transaction = TransactionId::from_raw(732);
    let first_surface = SurfaceId::new(733, 1);
    let second_surface = SurfaceId::new(734, 1);
    let first_handle = BufferHandle::from_raw(735);
    let second_handle = BufferHandle::from_raw(736);
    let mut resources = LivePresentationResourceSession::default();
    for handle in [first_handle, second_handle] {
        resources
            .register_source(descriptor(handle), vec![fd()])
            .unwrap();
    }
    let mut scheduler = LiveProductionPresentScheduler::default();
    scheduler
        .enqueue_group(
            &scheduler_batch_with_disposition(
                first_transaction,
                first_surface,
                first_handle,
                LiveProductionPresentDisposition::StageLayout { epoch },
            )
            .groups[0],
            &[],
            &mut resources,
            Instant::now(),
        )
        .unwrap();
    assert_eq!(scheduler.commit_layout_epoch(epoch), 1);
    assert_eq!(
        scheduler
            .release_layout_deferred_for_surfaces(&[first_surface], &[])
            .released,
        1
    );

    let geometry = Rect {
        x: 64,
        y: 0,
        width: 64,
        height: 48,
    };
    let layout = [sophia_protocol::LayerSnapshot {
        input_region: None,
        translation: None,
        output: None,
        surface: second_surface,
        authority_local_id: None,
        namespace: None,
        stack_rank: 1,
        geometry,
        source_size: Size {
            width: geometry.width,
            height: geometry.height,
        },
        source: BufferSource::DmaBuf {
            handle: second_handle.raw(),
        },
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: sophia_protocol::ResizeSyncCapability::ImplicitOnly,
    }];
    let superseded = scheduler
        .enqueue_group(
            &scheduler_batch_with_disposition(
                second_transaction,
                second_surface,
                second_handle,
                LiveProductionPresentDisposition::StageLayout { epoch },
            )
            .groups[0],
            &layout,
            &mut resources,
            Instant::now(),
        )
        .unwrap();

    assert!(superseded.is_empty());
    assert_eq!(
        scheduler
            .pop_front()
            .map(|queued| queued.submission.transaction),
        Some(first_transaction)
    );
    assert_eq!(
        scheduler
            .front()
            .map(|queued| queued.submission.transaction),
        Some(second_transaction)
    );
    assert_eq!(scheduler.pending_supersessions(), 0);
}

#[test]
fn superseded_present_is_rejected_without_evicting_matching_candidate() {
    let matching_handle = BufferHandle::from_raw(77);
    let rejected_handle = BufferHandle::from_raw(78);
    let matching_transaction = TransactionId::from_raw(79);
    let rejected_transaction = TransactionId::from_raw(80);
    let surface = SurfaceId::new(81, 1);
    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(matching_handle), vec![fd()])
        .unwrap();
    resources
        .register_source(descriptor(rejected_handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();
    let epoch = TransactionId::from_raw(2);
    let matching_batch = scheduler_batch_with_disposition(
        matching_transaction,
        surface,
        matching_handle,
        LiveProductionPresentDisposition::StageLayout { epoch },
    );
    scheduler
        .enqueue_group(
            &matching_batch.groups[0],
            &[],
            &mut resources,
            Instant::now(),
        )
        .unwrap();

    let rejected_batch = scheduler_batch_with_disposition(
        rejected_transaction,
        surface,
        rejected_handle,
        LiveProductionPresentDisposition::RejectSuperseded,
    );
    let rejected = scheduler
        .enqueue_group(
            &rejected_batch.groups[0],
            &[],
            &mut resources,
            Instant::now(),
        )
        .unwrap();

    assert_eq!(rejected, [rejected_transaction]);
    assert_eq!(
        scheduler
            .front()
            .map(|queued| queued.submission.transaction),
        Some(matching_transaction)
    );
}

/// Forcing quiescence for a topology change skips what could run now, and
/// leaves alone what a layout epoch is still holding.
///
/// A staged present belongs to its epoch, which commits or aborts it on its
/// own schedule. Mass-skipping it here would settle work the layout still
/// intends to present; what blocks quiescence is the runnable set alone.
#[test]
fn drain_runnable_transactions_leaves_layout_deferred() {
    let runnable_handle = BufferHandle::from_raw(220);
    let staged_handle = BufferHandle::from_raw(221);
    let runnable_transaction = TransactionId::from_raw(222);
    let staged_transaction = TransactionId::from_raw(223);
    let surface = SurfaceId::new(224, 1);
    let epoch = TransactionId::from_raw(225);

    let mut resources = LivePresentationResourceSession::default();
    resources
        .register_source(descriptor(runnable_handle), vec![fd()])
        .unwrap();
    resources
        .register_source(descriptor(staged_handle), vec![fd()])
        .unwrap();
    let mut scheduler = LiveProductionPresentScheduler::default();

    scheduler
        .enqueue_group(
            &scheduler_batch(runnable_transaction, surface, runnable_handle).groups[0],
            &[],
            &mut resources,
            Instant::now(),
        )
        .unwrap();
    scheduler
        .enqueue_group(
            &scheduler_batch_with_disposition(
                staged_transaction,
                surface,
                staged_handle,
                LiveProductionPresentDisposition::StageLayout { epoch },
            )
            .groups[0],
            &[],
            &mut resources,
            Instant::now(),
        )
        .unwrap();
    assert!(scheduler.has_runnable_queued());

    assert_eq!(
        scheduler.drain_runnable_transactions(),
        [runnable_transaction]
    );

    // The runnable one is gone, so quiescence is reachable; the staged one is
    // still owned by its epoch.
    assert!(!scheduler.has_runnable_queued());
    assert!(scheduler.has_queued());
    assert_eq!(
        scheduler.abort_layout_epoch(epoch).rejected,
        [staged_transaction]
    );
    assert!(!scheduler.has_queued());
}
