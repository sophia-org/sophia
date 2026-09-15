#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]

use sophia_backend_live::{
    LivePresentBufferDisposition, LivePresentProtocolFeedback, LiveProductionAuthorityBatch,
    LiveProductionAuthorityGroup, LiveProductionCpuBufferUpdate, LiveProductionCursorPresentation,
    LiveProductionCycleRequest, LiveProductionDmaBufRegistration, LiveProductionFenceRegistration,
    LiveProductionNativeFrameId, LiveProductionNativeRetirementOwner,
    LiveProductionNativeSubmissionOwner, LiveProductionPresentDisposition,
    LiveProductionPresentSubmission, LiveProductionScanoutContent,
    LiveProductionSoftwarePresentFrameObservation, LiveProductionSoftwarePresentFramePhase,
    LiveProductionSoftwarePresentFrameTransition, LiveProductionSoftwarePresentSubmission,
    LiveProductionVisualRuntime, reduce_live_production_native_retirement_owner,
    reduce_live_production_native_submission_owner, reduce_software_present_frame_observation,
};
use sophia_engine::{HeadlessOutput, SurfaceChromeStyle, SurfaceFrameStyle};
use sophia_protocol::{
    AuthorityKind, BufferHandle, BufferSource, DRM_FORMAT_MOD_INVALID, DmaBufDescriptor,
    DmaBufPlaneDescriptor, FenceHandle, LayerSnapshot, OutputId, Rect, Region,
    ResizeSyncCapability, Size, SurfaceId, SurfaceTransaction, SurfaceTransactionReadiness,
    TransactionId, Transform,
};
use sophia_renderer_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuBufferPatch, LiveCpuBufferSource,
    LiveCpuBufferUpdate, LiveProductionCpuScene,
};
use std::fs::File;
use std::os::fd::OwnedFd;
use std::sync::Arc;

#[test]
fn software_present_feedback_requires_its_own_native_frame() {
    let owned = LiveProductionNativeFrameId::from_raw(41);
    let unrelated = LiveProductionNativeFrameId::from_raw(42);

    assert_eq!(
        reduce_software_present_frame_observation(
            owned,
            LiveProductionSoftwarePresentFramePhase::Pending,
            LiveProductionSoftwarePresentFrameObservation::NativeSubmitted(unrelated),
        ),
        LiveProductionSoftwarePresentFrameTransition::Unrelated
    );
    assert_eq!(
        reduce_software_present_frame_observation(
            owned,
            LiveProductionSoftwarePresentFramePhase::Pending,
            LiveProductionSoftwarePresentFrameObservation::NativeRetired(unrelated),
        ),
        LiveProductionSoftwarePresentFrameTransition::Unrelated
    );
    assert_eq!(
        reduce_software_present_frame_observation(
            owned,
            LiveProductionSoftwarePresentFramePhase::Pending,
            LiveProductionSoftwarePresentFrameObservation::NativeRetired(owned),
        ),
        LiveProductionSoftwarePresentFrameTransition::InvalidRetirement
    );
    assert_eq!(
        reduce_software_present_frame_observation(
            owned,
            LiveProductionSoftwarePresentFramePhase::Pending,
            LiveProductionSoftwarePresentFrameObservation::NativeSubmitted(owned),
        ),
        LiveProductionSoftwarePresentFrameTransition::Submitted
    );
    assert_eq!(
        reduce_software_present_frame_observation(
            owned,
            LiveProductionSoftwarePresentFramePhase::Submitted,
            LiveProductionSoftwarePresentFrameObservation::NativeSubmitted(owned),
        ),
        LiveProductionSoftwarePresentFrameTransition::AlreadySubmitted
    );
    assert_eq!(
        reduce_software_present_frame_observation(
            owned,
            LiveProductionSoftwarePresentFramePhase::Submitted,
            LiveProductionSoftwarePresentFrameObservation::NativeRetired(owned),
        ),
        LiveProductionSoftwarePresentFrameTransition::Retired
    );
}

#[test]
fn older_software_frame_may_retire_after_next_dma_frame_submits() {
    let retired = LiveProductionNativeFrameId::from_raw(30);
    let successor = LiveProductionNativeFrameId::from_raw(31);
    let software = LiveProductionScanoutContent::RetainedMixed {
        logical_content_checksum: None,
        requires_retirement: false,
        frame: retired,
        nonzero_rgb_pixels: 985,
    };

    assert_eq!(
        reduce_live_production_native_retirement_owner(retired, software, Some(successor), false),
        LiveProductionNativeRetirementOwner::IndependentFrame
    );

    let present = LiveProductionScanoutContent::MixedPresent {
        frame: retired,
        transaction: TransactionId::from_raw(699),
        nonzero_rgb_pixels: 985,
    };

    // A frame this session gave the kernel, displaced by a later present
    // before the kernel reported it. The kernel retires what it scanned out,
    // so this is ordinary and must not end the session. See
    // `PresentMixedOwnership`, whose scheduler-only control is this case.
    assert_eq!(
        reduce_live_production_native_retirement_owner(retired, present, Some(successor), true),
        LiveProductionNativeRetirementOwner::SupersededDmaPresent
    );

    // The same shape for a frame this session never submitted stays fatal.
    // That is the invariant the check exists to defend: a retirement naming a
    // buffer we never gave the kernel means we do not know what is on glass.
    assert_eq!(
        reduce_live_production_native_retirement_owner(retired, present, Some(successor), false),
        LiveProductionNativeRetirementOwner::InvalidDmaOwnership
    );

    // Nothing in flight at all, which the crashed session also showed, is
    // still a supersession when the frame was ours.
    assert_eq!(
        reduce_live_production_native_retirement_owner(retired, present, None, true),
        LiveProductionNativeRetirementOwner::SupersededDmaPresent
    );
}

/// The shape a live session actually produced, sixty-five seconds in, when a
/// Firefox extension popup recomposed while a frame was already in flight.
///
/// A submit pass that finds a frame already in flight records nothing, so the
/// cohort never marks that output submitted and `submitted_frame` answers
/// `None` for a frame the kernel is about to retire. The reservation is the
/// session's only remaining claim on it, which is why ownership asks
/// `owns_frame` rather than which frame is current.
#[test]
fn a_frame_reserved_but_never_recorded_is_still_ours() {
    let retired = LiveProductionNativeFrameId::from_raw(2608);
    let present = LiveProductionScanoutContent::MixedPresent {
        frame: retired,
        transaction: TransactionId::from_raw(12795),
        nonzero_rgb_pixels: 985,
    };

    assert_eq!(
        reduce_live_production_native_retirement_owner(retired, present, None, true),
        LiveProductionNativeRetirementOwner::SupersededDmaPresent,
        "a reserved frame the cohort never recorded must not end the session"
    );

    use sophia_backend_live::LiveProductionNativeOwnershipMismatch;
    let observed = LiveProductionNativeOwnershipMismatch {
        retired_frame: retired,
        content_frame: retired,
        content_transaction: Some(TransactionId::from_raw(12795)),
        submitted_frame: None,
        in_flight_frame: Some(retired),
        in_flight_transaction: Some(TransactionId::from_raw(12795)),
    };
    assert_eq!(
        observed.kind(),
        "reserved_but_not_submitted",
        "the diagnostic that found this must keep naming it"
    );
}

#[test]
fn ordinary_head_composition_submission_does_not_advance_a_present_cohort() {
    let frame = LiveProductionNativeFrameId::from_raw(43);
    let transaction = TransactionId::from_raw(700);
    let ordinary = LiveProductionScanoutContent::HeadComposition {
        frame,
        logical_content_checksum: 91,
        nonzero_rgb_pixels: 1_024,
    };
    let present = LiveProductionScanoutContent::MixedPresent {
        frame,
        transaction,
        nonzero_rgb_pixels: 1_024,
    };

    assert_eq!(
        reduce_live_production_native_submission_owner(ordinary, None),
        LiveProductionNativeSubmissionOwner::IndependentFrame
    );
    assert_eq!(
        reduce_live_production_native_submission_owner(present, Some((frame, transaction))),
        LiveProductionNativeSubmissionOwner::SubmittedDmaPresent
    );
    // Scene-driven content while a present is pending is the plane moving
    // on, not corruption. This exact shape was fatal, and what it cost was a
    // session ending seven seconds in when a browser popup recomposed while
    // present 1231 was still waiting for a frame that had already retired.
    assert_eq!(
        reduce_live_production_native_submission_owner(ordinary, Some((frame, transaction))),
        LiveProductionNativeSubmissionOwner::OvertookPendingPresent
    );
    // Present content the cohort no longer names is stale but still ours.
    assert_eq!(
        reduce_live_production_native_submission_owner(present, None),
        LiveProductionNativeSubmissionOwner::StalePresentContent
    );
    // Half-matching identity is the shape that stays fatal: content and
    // cohort agree on the frame or the transaction but not both, which is
    // bookkeeping that has split rather than a race that ordered itself
    // badly.
    let other_frame = LiveProductionNativeFrameId::from_raw(frame.raw() + 1);
    let other_transaction = TransactionId::from_raw(transaction.raw() + 1);
    assert_eq!(
        reduce_live_production_native_submission_owner(present, Some((frame, other_transaction))),
        LiveProductionNativeSubmissionOwner::InvalidDmaOwnership
    );
    assert_eq!(
        reduce_live_production_native_submission_owner(present, Some((other_frame, transaction))),
        LiveProductionNativeSubmissionOwner::InvalidDmaOwnership
    );
    // A fully different expectation means the cohort moved on: stale, owned.
    assert_eq!(
        reduce_live_production_native_submission_owner(
            present,
            Some((other_frame, other_transaction))
        ),
        LiveProductionNativeSubmissionOwner::StalePresentContent
    );
}

#[test]
fn recent_cpu_update_residency_bridges_patch_gaps_and_remains_bounded() {
    let size = Size {
        width: 2,
        height: 1,
    };
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size,
        scale: 1,
    };
    let mut scene = LiveProductionCpuScene::new(size);
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).unwrap();
    let run_update = |runtime: &mut LiveProductionVisualRuntime,
                      scene: &mut LiveProductionCpuScene,
                      updates: Vec<LiveCpuBufferUpdate>| {
        let groups = if let Some(update) = updates.first() {
            let transaction = TransactionId::from_raw(
                update
                    .handle()
                    .saturating_add(update.generation())
                    .saturating_add(1),
            );
            let surface = SurfaceId::new(update.handle() as u32, 1);
            vec![LiveProductionAuthorityGroup {
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
                        width: size.width,
                        height: size.height,
                    },
                    presentation_extent: sophia_protocol::Size {
                        width: size.width,
                        height: size.height,
                    },
                    content: sophia_protocol::SurfaceContentSet::singleton(
                        BufferSource::CpuBuffer {
                            handle: update.handle(),
                        },
                        size,
                    ),

                    damage: Region::empty(),
                    readiness: SurfaceTransactionReadiness::Ready,
                    timeout_msec: 250,
                    previous_committed_generation: 0,
                }],
                cpu_buffer_updates: updates
                    .into_iter()
                    .map(|update| LiveProductionCpuBufferUpdate::new(transaction, surface, update))
                    .collect(),
                removed_surfaces: vec![surface],
                present_submissions: Vec::new(),
                software_present_submissions: Vec::new(),
            }]
        } else {
            Vec::new()
        };
        let batch = LiveProductionAuthorityBatch {
            groups,
            dma_buf_registrations: Vec::new(),
            fence_registrations: Vec::new(),
            released_dma_bufs: Vec::new(),
            released_fences: Vec::new(),
        };
        runtime.run_cpu_production_cycle(LiveProductionCycleRequest {
            batch: &batch,
            scene,
            raised_surface: None,
            focused_surface: None,
            cursor_presentation: LiveProductionCursorPresentation::Software(None),
            defer_frame: false,
            output_descriptors: &[output],
            native_scanout: None,
            wm_update: None,
            presentation_layout: &[],
            geometry_routed_surfaces: &[],
            chrome_surfaces: &[],
            indicator_publication: None,
            staged_cpu_buffer_handles: &[],
        })
    };

    run_update(
        &mut runtime,
        &mut scene,
        vec![LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
            handle: 72,
            size,
            stride: 8,
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            generation: 1,
            bytes: Arc::new(vec![0; 8]),
        })],
    )
    .unwrap();
    assert_eq!(scene.resident_buffer_count(), 1);

    run_update(
        &mut runtime,
        &mut scene,
        vec![LiveCpuBufferUpdate::Patch(LiveCpuBufferPatch {
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
        })],
    )
    .unwrap();
    assert_eq!(scene.resident_buffer_count(), 1);

    run_update(&mut runtime, &mut scene, Vec::new()).unwrap();
    assert_eq!(scene.resident_buffer_count(), 1);

    for handle in 100..116 {
        run_update(
            &mut runtime,
            &mut scene,
            vec![LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                handle,
                size,
                stride: 8,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 1,
                bytes: Arc::new(vec![0; 8]),
            })],
        )
        .unwrap();
    }
    assert_eq!(scene.resident_buffer_count(), 16);

    run_update(&mut runtime, &mut scene, Vec::new()).unwrap();
    assert_eq!(scene.resident_buffer_count(), 16);
}

#[test]
fn software_present_applies_grouped_pixels_and_routes_feedback() {
    software_present_during_seat_lifetime(false);
}

#[test]
fn suspended_native_owner_drains_software_present_without_presentation_or_input() {
    software_present_during_seat_lifetime(true);
}

fn software_present_during_seat_lifetime(suspended: bool) {
    let size = Size {
        width: 2,
        height: 1,
    };
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size,
        scale: 1,
    };
    let transaction = TransactionId::from_raw(70);
    let surface = SurfaceId::new(71, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 2,
        height: 1,
    };
    let surface_transaction = SurfaceTransaction {
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
            BufferSource::CpuBuffer { handle: 72 },
            size,
        ),

        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let batch = LiveProductionAuthorityBatch {
        groups: vec![LiveProductionAuthorityGroup {
            transaction,
            transactions: vec![surface_transaction.clone()],
            cpu_buffer_updates: vec![LiveProductionCpuBufferUpdate::new(
                transaction,
                surface,
                LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                    handle: 72,
                    size,
                    stride: 8,
                    format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                    generation: 1,
                    bytes: Arc::new(vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0xff]),
                }),
            )],
            removed_surfaces: Vec::new(),
            present_submissions: Vec::new(),
            software_present_submissions: vec![LiveProductionSoftwarePresentSubmission {
                candidate: surface_transaction.key(),
                source_size: size,
                transaction,
                surface,
                acquire_fence: None,
                idle_fence: None,
            }],
        }],
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    };
    let layout = [LayerSnapshot {
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
        source: surface_transaction.target_buffer(),
        damage: surface_transaction.damage.clone(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    }];
    let mut scene = LiveProductionCpuScene::new(size);
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).unwrap();
    if suspended {
        runtime.suspend_revoked_native_scanout(&[output]).unwrap();
    }
    let (submission, committed, progress) = runtime
        .run_cpu_production_cycle(LiveProductionCycleRequest {
            batch: &batch,
            scene: &mut scene,
            raised_surface: None,
            focused_surface: Some(surface),
            cursor_presentation: LiveProductionCursorPresentation::Software(None),
            defer_frame: false,
            output_descriptors: &[output],
            native_scanout: None,
            wm_update: None,
            presentation_layout: &layout,
            geometry_routed_surfaces: &[],
            chrome_surfaces: &[surface],
            indicator_publication: None,
            staged_cpu_buffer_handles: &[],
        })
        .unwrap();

    assert!(submission.composed);
    assert_eq!(committed.len(), 1);
    let owner = progress.latest_update.expect("accepted update owner");
    assert_eq!(owner.transaction, transaction);
    assert_eq!(owner.surface, surface);
    assert_eq!(owner.handle, 72);
    assert_eq!(owner.generation, 1);
    assert_eq!(progress.primary_logical_target, None);
    assert_eq!(progress.accepted_updates, 1);
    assert!(scene.surface_has_visual_detail(&committed, surface));
    let mut outcomes = Vec::new();
    runtime.drain_present_feedback_into(&mut outcomes).unwrap();
    assert_eq!(outcomes.len(), 1);
    let complete = LivePresentProtocolFeedback::Complete {
        transaction,
        ust: 0,
        msc: 0,
        disposition: if suspended {
            LivePresentBufferDisposition::Skipped
        } else {
            LivePresentBufferDisposition::Copied
        },
    };
    let idle = LivePresentProtocolFeedback::Idle { transaction };
    assert_eq!(
        outcomes[0].feedback,
        if suspended {
            vec![complete, idle]
        } else {
            vec![idle, complete]
        }
    );
    assert_eq!(runtime.diagnostics().live_presentations, 0);
    let mut retired = Vec::new();
    runtime
        .drain_retired_software_presents_into(&mut retired)
        .unwrap();
    if suspended {
        assert!(retired.is_empty());
        assert!(runtime.input_layers().is_empty());
        return;
    }
    assert_eq!(
        retired,
        [sophia_backend_live::LiveProductionRetiredSoftwarePresent {
            candidate: surface_transaction.key(),
            source_size: size,
            frame: sophia_backend_live::LiveProductionNativeFrameId::from_raw(0),
            native_submission: 0,
            ust_usec: 0,
            msc: 0,
        }]
    );
}

#[test]
fn gpu_owner_batch_registers_its_separate_software_present_group() {
    let size = Size {
        width: 64,
        height: 48,
    };
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size,
        scale: 1,
    };
    let cpu_transaction = TransactionId::from_raw(80);
    let cpu_surface = SurfaceId::new(81, 1);
    let cpu_geometry = Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    };
    let cpu_candidate = SurfaceTransaction {
        input_region: None,
        transaction: cpu_transaction,
        authority: AuthorityKind::SophiaX,
        surface: cpu_surface,
        namespace: None,
        target_geometry: cpu_geometry,
        presentation_extent: Size {
            width: (cpu_geometry).width,
            height: (cpu_geometry).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 82 },
            size,
        ),

        damage: Region::single(cpu_geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let dma_transaction = TransactionId::from_raw(90);
    let dma_surface = SurfaceId::new(91, 1);
    let dma_handle = BufferHandle::from_raw(92);
    let dma_candidate = SurfaceTransaction {
        input_region: None,
        transaction: dma_transaction,
        authority: AuthorityKind::SophiaX,
        surface: dma_surface,
        namespace: None,
        target_geometry: cpu_geometry,
        presentation_extent: Size {
            width: (cpu_geometry).width,
            height: (cpu_geometry).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: dma_handle.raw(),
            },
            size,
        ),

        damage: Region::single(cpu_geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let batch = LiveProductionAuthorityBatch {
        groups: vec![
            LiveProductionAuthorityGroup {
                transaction: cpu_transaction,
                transactions: vec![cpu_candidate.clone()],
                cpu_buffer_updates: Vec::new(),
                removed_surfaces: Vec::new(),
                present_submissions: Vec::new(),
                software_present_submissions: vec![LiveProductionSoftwarePresentSubmission {
                    candidate: cpu_candidate.key(),
                    source_size: size,
                    transaction: cpu_transaction,
                    surface: cpu_surface,
                    acquire_fence: None,
                    idle_fence: None,
                }],
            },
            LiveProductionAuthorityGroup {
                transaction: dma_transaction,
                transactions: vec![dma_candidate],
                cpu_buffer_updates: Vec::new(),
                removed_surfaces: Vec::new(),
                present_submissions: vec![LiveProductionPresentSubmission {
                    transaction: dma_transaction,
                    surface: dma_surface,
                    buffer: dma_handle,
                    x_offset: 0,
                    y_offset: 0,
                    acquire_fence: None,
                    idle_fence: None,
                    layout_disposition: LiveProductionPresentDisposition::Immediate,
                }],
                software_present_submissions: Vec::new(),
            },
        ],
        dma_buf_registrations: vec![LiveProductionDmaBufRegistration {
            descriptor: DmaBufDescriptor {
                handle: dma_handle,
                size,
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
            },
            plane_fds: vec![Arc::new(OwnedFd::from(
                File::open("/dev/null").expect("DMA-BUF fixture FD"),
            ))],
        }],
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    };
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).unwrap();
    let scene = LiveProductionCpuScene::new(size);

    runtime
        .run_batch(&batch, &[], None, None, &scene, Vec::new(), None)
        .unwrap();

    let diagnostics = runtime.diagnostics();
    assert_eq!(diagnostics.software_present_frames_waiting, 1);
    assert_eq!(diagnostics.software_present_frames_submitted, 0);
    assert_eq!(diagnostics.live_presentations, 1);
    runtime.shutdown_presentations().unwrap();
    assert_eq!(runtime.diagnostics().live_presentations, 0);
}

#[test]
fn deferred_successor_present_retains_resources_until_stream_admission() {
    let size = Size {
        width: 64,
        height: 48,
    };
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size,
        scale: 1,
    };
    let surface = SurfaceId::new(101, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    };
    let layout = [LayerSnapshot {
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
        source: BufferSource::None,
        damage: Region::single(geometry),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 0,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    }];
    let first_transaction = TransactionId::from_raw(102);
    let first_handle = BufferHandle::from_raw(103);
    let acquire_handle = FenceHandle::from_raw(104);
    let acquire_fence = sophia_xshmfence::allocate().unwrap();
    let present_batch =
        |transaction: TransactionId, handle: BufferHandle, acquire_fence: Option<FenceHandle>| {
            let candidate = SurfaceTransaction {
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
                    BufferSource::DmaBuf {
                        handle: handle.raw(),
                    },
                    size,
                ),

                damage: Region::single(geometry),
                readiness: SurfaceTransactionReadiness::Ready,
                timeout_msec: 250,
                previous_committed_generation: 0,
            };
            LiveProductionAuthorityGroup {
                transaction,
                transactions: vec![candidate],
                cpu_buffer_updates: Vec::new(),
                removed_surfaces: Vec::new(),
                present_submissions: vec![LiveProductionPresentSubmission {
                    transaction,
                    surface,
                    buffer: handle,
                    x_offset: 0,
                    y_offset: 0,
                    acquire_fence,
                    idle_fence: None,
                    layout_disposition: LiveProductionPresentDisposition::Immediate,
                }],
                software_present_submissions: Vec::new(),
            }
        };
    let registration = |handle| LiveProductionDmaBufRegistration {
        descriptor: DmaBufDescriptor {
            handle,
            size,
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
        },
        plane_fds: vec![Arc::new(OwnedFd::from(
            File::open("/dev/null").expect("DMA-BUF fixture FD"),
        ))],
    };
    let first = LiveProductionAuthorityBatch {
        groups: vec![present_batch(
            first_transaction,
            first_handle,
            Some(acquire_handle),
        )],
        dma_buf_registrations: vec![registration(first_handle)],
        fence_registrations: vec![LiveProductionFenceRegistration {
            handle: acquire_handle,
            initially_triggered: false,
            fd: Arc::new(acquire_fence),
        }],
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    };
    let successor_transaction = TransactionId::from_raw(105);
    let successor_handle = BufferHandle::from_raw(106);
    let successor = LiveProductionAuthorityBatch {
        groups: vec![present_batch(successor_transaction, successor_handle, None)],
        dma_buf_registrations: vec![registration(successor_handle)],
        fence_registrations: Vec::new(),
        released_dma_bufs: vec![successor_handle],
        released_fences: Vec::new(),
    };
    let mut scene = LiveProductionCpuScene::new(size);
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).unwrap();
    for batch in [&first, &successor] {
        runtime
            .run_gpu_production_cycle(LiveProductionCycleRequest {
                batch,
                scene: &mut scene,
                raised_surface: None,
                focused_surface: None,
                cursor_presentation: LiveProductionCursorPresentation::Software(None),
                defer_frame: false,
                output_descriptors: &[output],
                native_scanout: None,
                wm_update: None,
                presentation_layout: &layout,
                geometry_routed_surfaces: &[],
                chrome_surfaces: &[],
                indicator_publication: None,
                staged_cpu_buffer_handles: &[],
            })
            .unwrap();
    }

    let diagnostics = runtime.diagnostics();
    assert_eq!(diagnostics.live_presentations, 1);
    assert_eq!(diagnostics.live_sources, 2);
    runtime.shutdown_presentations().unwrap();
    assert_eq!(runtime.diagnostics().live_sources, 0);
}

/// The three ways ownership can disagree are three different bugs, and the
/// message has to tell them apart. Reporting all of them with one sentence
/// naming no frame is what made this cost a session to diagnose.
#[test]
fn an_ownership_mismatch_names_which_disagreement_it_is() {
    use sophia_backend_live::LiveProductionNativeOwnershipMismatch;

    let retired = LiveProductionNativeFrameId::from_raw(4346);
    let successor = LiveProductionNativeFrameId::from_raw(4347);
    let transaction = TransactionId::from_raw(38244);

    let base = LiveProductionNativeOwnershipMismatch {
        retired_frame: retired,
        content_frame: retired,
        content_transaction: Some(transaction),
        submitted_frame: None,
        in_flight_frame: None,
        in_flight_transaction: None,
    };

    // Nothing in flight at all: the scheduler finished or dropped the present
    // before its retirement arrived.
    assert_eq!(base.kind(), "no_present_in_flight");

    // A later present holds the scheduler. This is the shape the crashed
    // session showed, with a newer frame already submitted.
    let superseded = LiveProductionNativeOwnershipMismatch {
        submitted_frame: Some(successor),
        in_flight_frame: Some(successor),
        in_flight_transaction: Some(TransactionId::from_raw(38245)),
        ..base
    };
    assert_eq!(superseded.kind(), "superseded_by_later_present");

    // Reserved for this output but never submitted: an ownership question,
    // not a presentation one, which is why in_flight_frame is consulted.
    let reserved = LiveProductionNativeOwnershipMismatch {
        submitted_frame: None,
        in_flight_frame: Some(retired),
        ..base
    };
    assert_eq!(reserved.kind(), "reserved_but_not_submitted");

    // The content disagrees with the frame it retired under, which is a
    // corrupt record rather than a scheduling race.
    let mismatched = LiveProductionNativeOwnershipMismatch {
        content_frame: successor,
        ..base
    };
    assert_eq!(mismatched.kind(), "content_names_another_frame");

    // Every fact reaches the message; a kind alone would not locate it.
    let rendered = superseded.to_string();
    for fragment in [
        "kind=superseded_by_later_present",
        "retired_frame=4346",
        "content_transaction=Some(38244)",
        "submitted_frame=Some(4347)",
        "in_flight_transaction=Some(38245)",
    ] {
        assert!(
            rendered.contains(fragment),
            "missing {fragment} in {rendered}"
        );
    }
}

/// Focus landing on a popup does not unfocus the window it belongs to.
///
/// Menus, tooltips and dropdowns take input focus and carry no chrome of their
/// own. The border colour is chosen by comparing each framed surface against
/// the focused one, so a focused popup used to match nothing and every window
/// repainted in the unfocused colour until the popup closed. In a measured
/// session that happened 43 times, for 6 to 574 milliseconds each: the window
/// border visibly flashed every time a menu opened.
#[test]
fn a_focused_popup_does_not_unfocus_the_window_it_belongs_to() {
    let size = Size {
        width: 8,
        height: 8,
    };
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size,
        scale: 1,
    };
    let transaction = TransactionId::from_raw(90);
    let window = SurfaceId::new(91, 1);
    // A popup the compositor draws no frame for, which is what every menu is.
    let popup = SurfaceId::new(92, 1);
    // Large enough that a one-pixel frame has somewhere to land.
    let geometry = Rect {
        x: 1,
        y: 1,
        width: 6,
        height: 6,
    };
    let surface_transaction = SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface: window,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: Size {
            width: geometry.width,
            height: geometry.height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 92 },
            Size {
                width: geometry.width,
                height: geometry.height,
            },
        ),
        damage: Region::single(geometry),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let batch = LiveProductionAuthorityBatch {
        groups: vec![LiveProductionAuthorityGroup {
            transaction,
            transactions: vec![surface_transaction.clone()],
            cpu_buffer_updates: vec![LiveProductionCpuBufferUpdate::new(
                transaction,
                window,
                LiveCpuBufferUpdate::Replace(LiveCpuBufferSource {
                    handle: 92,
                    size,
                    stride: 32,
                    format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                    generation: 1,
                    bytes: Arc::new(vec![0xff; 8 * 8 * 4]),
                }),
            )],
            removed_surfaces: Vec::new(),
            present_submissions: Vec::new(),
            software_present_submissions: vec![LiveProductionSoftwarePresentSubmission {
                candidate: surface_transaction.key(),
                source_size: Size {
                    width: geometry.width,
                    height: geometry.height,
                },
                transaction,
                surface: window,
                acquire_fence: None,
                idle_fence: None,
            }],
        }],
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    };
    let empty = LiveProductionAuthorityBatch {
        groups: Vec::new(),
        dma_buf_registrations: Vec::new(),
        fence_registrations: Vec::new(),
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
    };
    let layout = [LayerSnapshot {
        input_region: None,
        translation: None,
        // Chrome is only composed for a surface a head owns.
        output: Some(OutputId::from_raw(1)),
        surface: window,
        authority_local_id: None,
        namespace: None,
        stack_rank: 0,
        geometry,
        source_size: Size {
            width: geometry.width,
            height: geometry.height,
        },
        source: surface_transaction.target_buffer(),
        damage: surface_transaction.damage.clone(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    }];
    let mut scene = LiveProductionCpuScene::new(size);
    // The default frame width is zero, which draws no chrome at all.
    let style = SurfaceChromeStyle {
        frame: SurfaceFrameStyle {
            width: 1,
            ..SurfaceFrameStyle::default()
        },
        ..SurfaceChromeStyle::default()
    };
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None)
        .unwrap()
        .with_surface_chrome_style(style);

    let cycle = |runtime: &mut LiveProductionVisualRuntime,
                 scene: &mut LiveProductionCpuScene,
                 batch: &LiveProductionAuthorityBatch,
                 focused: Option<SurfaceId>| {
        runtime
            .run_cpu_production_cycle(LiveProductionCycleRequest {
                batch,
                scene,
                raised_surface: None,
                focused_surface: focused,
                cursor_presentation: LiveProductionCursorPresentation::Software(None),
                defer_frame: false,
                output_descriptors: &[output],
                native_scanout: None,
                wm_update: None,
                presentation_layout: &layout,
                geometry_routed_surfaces: &[],
                chrome_surfaces: &[window],
                indicator_publication: None,
                staged_cpu_buffer_handles: &[],
            })
            .unwrap();
        runtime.take_chrome_set_observation()
    };

    // The window is focused and framed, which is the ordinary case.
    let focused = cycle(&mut runtime, &mut scene, &batch, Some(window))
        .expect("the first cycle composes chrome");
    assert_eq!(focused.frames, 1);
    assert_eq!(
        focused.focused_frames, 1,
        "the focused window is framed focused"
    );

    // A menu opens and takes focus. It has no frame of its own, and the window
    // it belongs to must keep its focused border rather than going grey.
    let with_popup = cycle(&mut runtime, &mut scene, &empty, Some(popup));
    if let Some(chrome) = with_popup {
        assert_eq!(
            chrome.focused_frames, 1,
            "a focused popup must not repaint the window's border unfocused"
        );
        assert_eq!(chrome.unfocused_frames, 0);
    }

    // Focus genuinely going nowhere still unfocuses everything, so clicking
    // away from every window is not mistaken for a popup.
    let cleared =
        cycle(&mut runtime, &mut scene, &empty, None).expect("clearing focus recomposes chrome");
    assert_eq!(cleared.focused_frames, 0, "no focus means no focused frame");
    assert_eq!(cleared.unfocused_frames, 1);
}
