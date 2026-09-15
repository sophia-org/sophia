#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]

use std::fs::File;
use std::os::fd::OwnedFd;

use sophia_backend_live::{
    LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888, LiveCpuComposedFrame, LivePresentBufferDisposition,
    LivePresentFeedbackError, LivePresentProtocolFeedback, LivePresentationResourceSession,
    LivePresentationSubmission, LiveProductionPresentFeedbackCoordinator,
    LiveResourceReleaseStatus, LiveRetainedRendererImageLayer, compose_full_state_mixed_frame,
    try_clone_mixed_frame,
};
use sophia_engine::{
    CompositorDisplayList, CompositorRgb8, HeadlessEngine, HeadlessOutput,
    ProductionSessionCoordinator, output_frame_damage_snapshot,
};
use sophia_protocol::{
    AuthorityKind, BufferHandle, BufferSource, CommittedSurfaceState, DRM_FORMAT_MOD_INVALID,
    DmaBufDescriptor, DmaBufPlaneDescriptor, FenceHandle, Rect, Region, Size, SurfaceId,
    SurfaceTransaction, SurfaceTransactionReadiness, TransactionId, TransactionOutcome,
};
use sophia_renderer_live::{
    LiveBufferState, LiveCompositionPlacement, LiveOwnedMixedCompositionFrame,
    LiveOwnedMixedCompositionLayer, LiveOwnedMultiPlaneDmaBufFrame, LiveRendererImageId,
    LiveSharedCpuBufferSource,
};

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

#[test]
fn backend_session_builds_mixed_cpu_gpu_frame_and_retires_exactly_once() {
    let handle = BufferHandle::from_raw(7);
    let transaction = TransactionId::from_raw(8);
    let mut session = LivePresentationResourceSession::default();
    session
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    session
        .begin(LivePresentationSubmission {
            transaction,
            buffer: handle,
            acquire_fence: None,
            idle_fence: None,
        })
        .unwrap();
    let cpu = LiveCpuComposedFrame {
        size: Size {
            width: 128,
            height: 96,
        },
        stride: 512,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        bytes: vec![1; 128 * 96 * 4].into(),
    };

    let frame = session
        .build_mixed_frame(
            transaction,
            Some(cpu),
            Rect {
                x: 20,
                y: 10,
                width: 64,
                height: 48,
            },
            None,
            1.0,
        )
        .unwrap();
    assert_eq!(frame.layers.len(), 2);
    session.mark_submitted(transaction).unwrap();
    assert_eq!(
        session.release_source(handle),
        LiveResourceReleaseStatus::Deferred
    );
    let retired = session.retire_page_flip(transaction).unwrap();
    assert_eq!(retired.source, BufferSource::DmaBuf { handle: 7 });
    assert!(retired.released_source);
    assert!(session.retire_page_flip(transaction).is_none());
    assert_eq!(session.source_count(), 0);
    assert_eq!(session.presentation_count(), 0);
}

#[test]
fn dma_buf_surface_resize_preserves_pixels_and_clips_without_scaling() {
    let handle = BufferHandle::from_raw(17);
    let transaction = TransactionId::from_raw(18);
    let mut session = LivePresentationResourceSession::default();
    session
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    session
        .begin(LivePresentationSubmission {
            transaction,
            buffer: handle,
            acquire_fence: None,
            idle_fence: None,
        })
        .unwrap();
    let surface = Rect {
        x: 64,
        y: 12,
        width: 32,
        height: 80,
    };

    let frame = session
        .build_mixed_frame(transaction, None, surface, None, 1.0)
        .unwrap();
    let LiveOwnedMixedCompositionLayer::DmaBuf {
        frame, placement, ..
    } = &frame.layers[0]
    else {
        panic!("expected a DMA-BUF layer");
    };
    assert_eq!(
        placement.target,
        Rect {
            width: 64,
            height: 48,
            ..surface
        }
    );
    assert_eq!(
        placement.clip,
        Some(Rect {
            height: 48,
            ..surface
        })
    );
    let retained = LiveRetainedRendererImageLayer {
        image_id: LiveRendererImageId::from_raw(transaction.raw()),
        size: Size {
            width: i32::try_from(frame.width).unwrap(),
            height: i32::try_from(frame.height).unwrap(),
        },
        format: frame.format,
        placement: *placement,
    };
    assert!(retained.has_unit_scale());
}

#[test]
fn full_state_composition_keeps_retained_surface_before_current_damage() {
    let placement = |x| LiveCompositionPlacement {
        target: Rect {
            x,
            y: 0,
            width: 64,
            height: 48,
        },
        clip: None,
        transform: sophia_protocol::Transform::IDENTITY,
        alpha: 1.0,
        sampling: sophia_engine::HeadSamplingClass::Exact,
    };
    let frame = || LiveOwnedMultiPlaneDmaBufFrame {
        width: 64,
        height: 48,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        modifier: DRM_FORMAT_MOD_INVALID,
        plane_count: 1,
        planes: [
            Some(sophia_renderer_live::LiveOwnedDmaBufPlane {
                fd: fd(),
                offset: 0,
                stride: 256,
            }),
            None,
            None,
            None,
        ],
    };
    let current = sophia_renderer_live::LiveOwnedMixedCompositionFrame {
        layers: vec![LiveOwnedMixedCompositionLayer::DmaBuf {
            image_id: LiveRendererImageId::from_raw(1),
            frame: frame(),
            placement: placement(64),
        }],
        output_damage_snapshot: None,
        trace: None,
        direct_scanout: Default::default(),
    };

    let composed = compose_full_state_mixed_frame(
        current,
        vec![LiveRetainedRendererImageLayer {
            image_id: LiveRendererImageId::from_raw(2),
            size: Size {
                width: 64,
                height: 48,
            },
            format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            placement: placement(0),
        }],
    );

    assert_eq!(composed.layers.len(), 2);
    assert!(matches!(
        &composed.layers[0],
        LiveOwnedMixedCompositionLayer::RendererImage { image_id, .. }
            if *image_id == LiveRendererImageId::from_raw(2)
    ));
    assert!(matches!(
        &composed.layers[1],
        LiveOwnedMixedCompositionLayer::DmaBuf { image_id, .. }
            if *image_id == LiveRendererImageId::from_raw(1)
    ));
    let targets = composed
        .layers
        .iter()
        .map(|layer| match layer {
            LiveOwnedMixedCompositionLayer::DmaBuf { placement, .. }
            | LiveOwnedMixedCompositionLayer::RendererImage { placement, .. }
            | LiveOwnedMixedCompositionLayer::Cpu { placement, .. } => placement.target.x,
            LiveOwnedMixedCompositionLayer::Solid { geometry, .. } => geometry.x,
        })
        .collect::<Vec<_>>();
    assert_eq!(targets, [0, 64]);
}

#[test]
fn mixed_frame_clone_preserves_compositor_solid_rectangles() {
    let output = HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(1),
        size: Size {
            width: 64,
            height: 48,
        },
        scale: 1,
    };
    let snapshot =
        output_frame_damage_snapshot(output, CompositorDisplayList::empty(output.id), &[], None)
            .unwrap();
    let frame = LiveOwnedMixedCompositionFrame {
        layers: vec![LiveOwnedMixedCompositionLayer::Solid {
            geometry: Rect {
                x: 4,
                y: 5,
                width: 6,
                height: 7,
            },
            color: CompositorRgb8 {
                red: 0x70,
                green: 0xb7,
                blue: 0xff,
            },
        }],
        output_damage_snapshot: Some(snapshot.clone()),
        trace: None,
        direct_scanout: Default::default(),
    };

    let cloned = try_clone_mixed_frame(&frame).unwrap();
    assert_eq!(
        cloned.output_damage_snapshot.as_ref(),
        Some(&snapshot),
        "output damage identity must remain attached to cloned pixels"
    );
    let LiveOwnedMixedCompositionLayer::Solid { geometry, color } = cloned.layers[0] else {
        panic!("solid rectangle changed representation");
    };
    assert_eq!(
        geometry,
        Rect {
            x: 4,
            y: 5,
            width: 6,
            height: 7,
        }
    );
    assert_eq!(
        color,
        CompositorRgb8 {
            red: 0x70,
            green: 0xb7,
            blue: 0xff,
        }
    );
}

#[test]
fn mixed_frame_clone_shares_immutable_cpu_pixels() {
    let pixels = std::sync::Arc::new(vec![0x7f; 64]);
    let frame = LiveOwnedMixedCompositionFrame {
        layers: vec![LiveOwnedMixedCompositionLayer::Cpu {
            buffer: LiveSharedCpuBufferSource {
                handle: 17,
                size: Size {
                    width: 4,
                    height: 4,
                },
                stride: 16,
                format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: 23,
                bytes: std::sync::Arc::clone(&pixels).into(),
            },
            placement: LiveCompositionPlacement {
                target: Rect {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 4,
                },
                clip: None,
                transform: sophia_protocol::Transform::IDENTITY,
                alpha: 1.0,
                sampling: sophia_engine::HeadSamplingClass::Exact,
            },
        }],
        output_damage_snapshot: None,
        trace: None,
        direct_scanout: Default::default(),
    };

    let cloned = try_clone_mixed_frame(&frame).unwrap();
    let LiveOwnedMixedCompositionLayer::Cpu {
        buffer: original, ..
    } = &frame.layers[0]
    else {
        panic!("CPU layer changed representation");
    };
    let LiveOwnedMixedCompositionLayer::Cpu { buffer: cloned, .. } = &cloned.layers[0] else {
        panic!("cloned CPU layer changed representation");
    };

    assert!(std::ptr::eq(
        original.bytes.as_slice(),
        cloned.bytes.as_slice()
    ));
    assert_eq!(original.handle, cloned.handle);
    assert_eq!(original.generation, cloned.generation);
}

#[test]
fn retained_multi_plane_frame_clone_preserves_metadata_planes() {
    let original = LiveOwnedMultiPlaneDmaBufFrame {
        width: 64,
        height: 48,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        modifier: 7,
        plane_count: 3,
        planes: [
            Some(sophia_renderer_live::LiveOwnedDmaBufPlane {
                fd: fd(),
                offset: 0,
                stride: 256,
            }),
            Some(sophia_renderer_live::LiveOwnedDmaBufPlane {
                fd: fd(),
                offset: 12_288,
                stride: 64,
            }),
            Some(sophia_renderer_live::LiveOwnedDmaBufPlane {
                fd: fd(),
                offset: 15_360,
                stride: 64,
            }),
            None,
        ],
    };

    let cloned = original.try_clone().unwrap();

    assert_eq!(cloned.plane_count, 3);
    assert_eq!(cloned.modifier, 7);
    assert_eq!(cloned.planes[1].as_ref().unwrap().offset, 12_288);
    assert_eq!(cloned.planes[2].as_ref().unwrap().stride, 64);
}

#[test]
fn retained_renderer_image_preserves_pixel_aligned_placement() {
    let target = Rect {
        x: 64,
        y: 0,
        width: 128,
        height: 48,
    };
    let layer = LiveRetainedRendererImageLayer {
        image_id: LiveRendererImageId::from_raw(3),
        size: Size {
            width: 128,
            height: 48,
        },
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        placement: LiveCompositionPlacement {
            target,
            clip: None,
            transform: sophia_protocol::Transform::IDENTITY,
            alpha: 1.0,
            sampling: sophia_engine::HeadSamplingClass::Exact,
        },
    };

    assert!(layer.has_unit_scale());
    let cloned = layer;

    assert_eq!(cloned.image_id, LiveRendererImageId::from_raw(3));
    assert_eq!(cloned.size, layer.size);
    assert_eq!(cloned.placement.target, layer.placement.target);
    assert_eq!(cloned.placement.clip, layer.placement.clip);
    assert_eq!(cloned.placement.transform, layer.placement.transform);
    assert_eq!(cloned.placement.alpha, layer.placement.alpha);
}

#[test]
fn production_feedback_retires_resources_before_complete_and_idle() {
    let handle = BufferHandle::from_raw(17);
    let transaction = TransactionId::from_raw(18);
    let mut coordinator = LiveProductionPresentFeedbackCoordinator::default();
    coordinator
        .resources_mut()
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    coordinator
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction,
            buffer: handle,
            acquire_fence: None,
            idle_fence: None,
        })
        .unwrap();
    coordinator
        .resources_mut()
        .mark_submitted(transaction)
        .unwrap();
    assert_eq!(
        coordinator.resources_mut().release_source(handle),
        LiveResourceReleaseStatus::Deferred
    );

    let outcome = coordinator.complete_copy(transaction, 22, 33).unwrap();
    assert_eq!(
        outcome.feedback,
        [
            LivePresentProtocolFeedback::Idle { transaction },
            LivePresentProtocolFeedback::Complete {
                transaction,
                ust: 22,
                msc: 33,
                disposition: LivePresentBufferDisposition::Copied,
            },
        ]
    );
    assert!(!outcome.idle_fence_triggered);
    assert_eq!(coordinator.resources().source_count(), 0);
    assert_eq!(coordinator.resources().presentation_count(), 0);
    assert_eq!(
        coordinator.complete_copy(transaction, 44, 55),
        Err(LivePresentFeedbackError::UnknownPresentation { transaction })
    );
}

#[test]
fn asynchronous_skip_retains_the_last_display_clock_sample() {
    let displayed_handle = BufferHandle::from_raw(40);
    let skipped_handle = BufferHandle::from_raw(41);
    let displayed = TransactionId::from_raw(42);
    let skipped = TransactionId::from_raw(43);
    let mut coordinator = LiveProductionPresentFeedbackCoordinator::default();
    for handle in [displayed_handle, skipped_handle] {
        coordinator
            .resources_mut()
            .register_source(descriptor(handle), vec![fd()])
            .unwrap();
    }
    for (transaction, buffer) in [(displayed, displayed_handle), (skipped, skipped_handle)] {
        coordinator
            .resources_mut()
            .begin(LivePresentationSubmission {
                transaction,
                buffer,
                acquire_fence: None,
                idle_fence: None,
            })
            .unwrap();
    }
    coordinator
        .resources_mut()
        .mark_submitted(displayed)
        .unwrap();
    coordinator.complete_copy(displayed, 44_000, 45).unwrap();

    let outcome = coordinator
        .reject_skip_at_last_display(skipped)
        .expect("a policy skip after a displayed frame must retain its display timeline");

    assert_eq!(
        outcome.feedback,
        [
            LivePresentProtocolFeedback::Complete {
                transaction: skipped,
                ust: 44_000,
                msc: 45,
                disposition: LivePresentBufferDisposition::Skipped,
            },
            LivePresentProtocolFeedback::Idle {
                transaction: skipped,
            },
        ]
    );
    assert_eq!(coordinator.resources().presentation_count(), 0);
}

#[test]
fn displayed_feedback_delays_idle_until_surface_buffer_replacement() {
    let handle = BufferHandle::from_raw(19);
    let transaction = TransactionId::from_raw(20);
    let mut coordinator = LiveProductionPresentFeedbackCoordinator::default();
    coordinator
        .resources_mut()
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    coordinator
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction,
            buffer: handle,
            acquire_fence: None,
            idle_fence: None,
        })
        .unwrap();
    coordinator
        .resources_mut()
        .mark_submitted(transaction)
        .unwrap();

    let completed = coordinator
        .complete_retained_without_idle(transaction, 22, 23)
        .unwrap();
    assert_eq!(
        completed.feedback,
        [LivePresentProtocolFeedback::Complete {
            transaction,
            ust: 22,
            msc: 23,
            disposition: LivePresentBufferDisposition::Retained,
        }]
    );
    assert_eq!(coordinator.resources().presentation_count(), 1);

    let idle = coordinator.idle_displayed(transaction).unwrap();
    assert_eq!(
        idle.feedback,
        [LivePresentProtocolFeedback::Idle { transaction }]
    );
    assert_eq!(coordinator.resources().presentation_count(), 0);
}

#[test]
fn composited_successor_can_release_completed_source_before_its_own_flip() {
    let first_handle = BufferHandle::from_raw(21);
    let second_handle = BufferHandle::from_raw(22);
    let first = TransactionId::from_raw(23);
    let second = TransactionId::from_raw(24);
    let idle_handle = FenceHandle::from_raw(25);
    let idle_fence = sophia_xshmfence::allocate().unwrap();
    let idle_query = idle_fence.try_clone().unwrap();
    let mut coordinator = LiveProductionPresentFeedbackCoordinator::default();
    for handle in [first_handle, second_handle] {
        coordinator
            .resources_mut()
            .register_source(descriptor(handle), vec![fd()])
            .unwrap();
    }
    coordinator
        .resources_mut()
        .register_fence(idle_handle, false, idle_fence)
        .unwrap();
    coordinator
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction: first,
            buffer: first_handle,
            acquire_fence: None,
            idle_fence: Some(idle_handle),
        })
        .unwrap();
    coordinator.resources_mut().mark_submitted(first).unwrap();
    coordinator
        .complete_retained_without_idle(first, 1, 2)
        .unwrap();
    coordinator
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction: second,
            buffer: second_handle,
            acquire_fence: None,
            idle_fence: None,
        })
        .unwrap();

    let released = coordinator.idle_displayed(first).unwrap();

    assert_eq!(
        released.feedback,
        [LivePresentProtocolFeedback::Idle { transaction: first }]
    );
    assert!(released.idle_fence_triggered);
    assert!(sophia_xshmfence::query(&idle_query).unwrap());
    assert_eq!(
        coordinator.resources().state(second),
        Some(LiveBufferState::Ready)
    );
    assert_eq!(coordinator.resources().presentation_count(), 1);
}

#[test]
fn production_feedback_emits_nothing_when_skip_has_no_live_presentation() {
    let transaction = TransactionId::from_raw(28);
    let mut coordinator = LiveProductionPresentFeedbackCoordinator::default();

    assert_eq!(
        coordinator.reject_skip(transaction, 0, 0),
        Err(LivePresentFeedbackError::UnknownPresentation { transaction })
    );
}

#[test]
fn stale_prepared_page_flip_settles_as_skip_and_retires_resources_exactly_once() {
    let handle = BufferHandle::from_raw(29);
    let transaction = TransactionId::from_raw(30);
    let surface = SurfaceId::new(31, 1);
    let committed = CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
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
        damage: Region::empty(),
    };
    let mut production = ProductionSessionCoordinator::new(HeadlessEngine::default())
        .with_committed_surfaces(vec![committed.clone()]);
    let prepared = production.prepare_present_transaction(&SurfaceTransaction {
        input_region: None,
        transaction,
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: None,
        target_geometry: committed.geometry,
        presentation_extent: Size {
            width: (committed.geometry).width,
            height: (committed.geometry).height,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            committed.buffer(),
            sophia_protocol::Size {
                width: committed.geometry.width,
                height: committed.geometry.height,
            },
        ),

        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    });
    production.replace_committed_surfaces(Vec::new());

    let mut coordinator = LiveProductionPresentFeedbackCoordinator::default();
    coordinator
        .resources_mut()
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    coordinator
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction,
            buffer: handle,
            acquire_fence: None,
            idle_fence: None,
        })
        .unwrap();
    coordinator
        .resources_mut()
        .mark_submitted(transaction)
        .unwrap();
    assert_eq!(
        coordinator.resources_mut().release_source(handle),
        LiveResourceReleaseStatus::Deferred
    );

    let report = production
        .settle_prepared_retirement(prepared, |commit| match commit.outcome {
            TransactionOutcome::Committed => coordinator.complete_copy(transaction, 41, 42),
            TransactionOutcome::RejectedStaleSurface
            | TransactionOutcome::RejectedInvalidSurface
            | TransactionOutcome::TimedOut => coordinator.reject_skip(transaction, 41, 42),
        })
        .expect("stale page flip should settle through controlled rejection");

    assert_eq!(
        report.commit.outcome,
        TransactionOutcome::RejectedStaleSurface
    );
    assert!(report.committed_surfaces.is_empty());
    assert_eq!(
        report.evidence.feedback,
        [
            LivePresentProtocolFeedback::Complete {
                transaction,
                ust: 41,
                msc: 42,
                disposition: LivePresentBufferDisposition::Skipped,
            },
            LivePresentProtocolFeedback::Idle { transaction },
        ]
    );
    assert_eq!(coordinator.resources().source_count(), 0);
    assert_eq!(coordinator.resources().presentation_count(), 0);
    assert_eq!(
        coordinator.reject_skip(transaction, 41, 42),
        Err(LivePresentFeedbackError::UnknownPresentation { transaction })
    );
}

/// A directly scanned frame is on glass, not copied. Its Present completes as
/// `Flipped` and the buffer stays owed to the client until a successor flip
/// takes the plane. Releasing it any earlier lets the client draw into pixels
/// the display is reading.
///
/// Model: `validation/tla/PresentFlipOwnership.tla`,
/// `DisplayedClientBufferIsNeverReleased` and `ReleasedOnlyBySuccessor`.
#[test]
fn a_directly_flipped_buffer_is_released_only_after_its_successor() {
    let displayed_handle = BufferHandle::from_raw(31);
    let successor_handle = BufferHandle::from_raw(32);
    let displayed = TransactionId::from_raw(33);
    let successor = TransactionId::from_raw(34);
    let idle_handle = FenceHandle::from_raw(35);
    let idle_fence = sophia_xshmfence::allocate().unwrap();
    let idle_query = idle_fence.try_clone().unwrap();
    let mut coordinator = LiveProductionPresentFeedbackCoordinator::default();
    for handle in [displayed_handle, successor_handle] {
        coordinator
            .resources_mut()
            .register_source(descriptor(handle), vec![fd()])
            .unwrap();
    }
    coordinator
        .resources_mut()
        .register_fence(idle_handle, false, idle_fence)
        .unwrap();
    coordinator
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction: displayed,
            buffer: displayed_handle,
            acquire_fence: None,
            idle_fence: Some(idle_handle),
        })
        .unwrap();
    coordinator
        .resources_mut()
        .mark_submitted(displayed)
        .unwrap();

    let completed = coordinator
        .complete_flip_without_idle(displayed, 11, 12)
        .unwrap();

    // The client learns its frame is on screen, and learns it as a flip.
    assert_eq!(
        completed.feedback,
        [LivePresentProtocolFeedback::Complete {
            transaction: displayed,
            ust: 11,
            msc: 12,
            disposition: LivePresentBufferDisposition::Flipped,
        }]
    );
    // And does not get its buffer back, because the screen is scanning it.
    assert!(!completed.idle_fence_triggered);
    assert!(!sophia_xshmfence::query(&idle_query).unwrap());
    assert_eq!(
        coordinator.resources().state(displayed),
        Some(LiveBufferState::Submitted)
    );

    // A successor takes the plane. Only now is the release lawful.
    coordinator
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction: successor,
            buffer: successor_handle,
            acquire_fence: None,
            idle_fence: None,
        })
        .unwrap();
    let released = coordinator.idle_displayed(displayed).unwrap();

    assert_eq!(
        released.feedback,
        [LivePresentProtocolFeedback::Idle {
            transaction: displayed
        }]
    );
    assert!(released.idle_fence_triggered);
    assert!(sophia_xshmfence::query(&idle_query).unwrap());
}

/// `Flipped` is not `Retained`. Both report X `Flip`, but only one of them
/// means a client's own buffer reached the plane uncomposed, and a session
/// with no direct scanout must not be able to report the number that proves
/// this row.
#[test]
fn a_retained_completion_is_not_a_direct_flip() {
    let handle = BufferHandle::from_raw(41);
    let transaction = TransactionId::from_raw(42);
    let mut coordinator = LiveProductionPresentFeedbackCoordinator::default();
    coordinator
        .resources_mut()
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    coordinator
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction,
            buffer: handle,
            acquire_fence: None,
            idle_fence: None,
        })
        .unwrap();
    coordinator
        .resources_mut()
        .mark_submitted(transaction)
        .unwrap();

    let retained = coordinator
        .complete_retained_without_idle(transaction, 1, 2)
        .unwrap();

    assert_eq!(
        retained.feedback,
        [LivePresentProtocolFeedback::Complete {
            transaction,
            ust: 1,
            msc: 2,
            disposition: LivePresentBufferDisposition::Retained,
        }]
    );
}

/// A directly flipped buffer is released when the session ends, because the
/// successor that would retire it is not coming.
///
/// The first frame that ever reached a plane directly could not be shut down
/// after: it was correctly held until a successor retired it, and at teardown
/// there is no successor, so presentation shutdown refused with retained
/// surface content ownership. The screen going away retires it as surely as a
/// flip does.
///
/// Model: `validation/tla/PresentFlipOwnership.tla`, `ReleasedOnlyBySuccessor`.
#[test]
fn a_directly_flipped_buffer_is_released_when_the_session_ends() {
    let handle = BufferHandle::from_raw(51);
    let transaction = TransactionId::from_raw(52);
    let idle_handle = FenceHandle::from_raw(53);
    let idle_fence = sophia_xshmfence::allocate().unwrap();
    let idle_query = idle_fence.try_clone().unwrap();
    let mut coordinator = LiveProductionPresentFeedbackCoordinator::default();
    coordinator
        .resources_mut()
        .register_source(descriptor(handle), vec![fd()])
        .unwrap();
    coordinator
        .resources_mut()
        .register_fence(idle_handle, false, idle_fence)
        .unwrap();
    coordinator
        .resources_mut()
        .begin(LivePresentationSubmission {
            transaction,
            buffer: handle,
            acquire_fence: None,
            idle_fence: Some(idle_handle),
        })
        .unwrap();
    coordinator
        .resources_mut()
        .mark_submitted(transaction)
        .unwrap();
    coordinator
        .complete_flip_without_idle(transaction, 7, 8)
        .unwrap();

    // Still on glass, still owed to the client.
    assert!(!sophia_xshmfence::query(&idle_query).unwrap());
    assert_eq!(
        coordinator.resources().state(transaction),
        Some(LiveBufferState::Submitted)
    );

    // The session ends. Nothing will flip after this, so this is the release.
    let released = coordinator.idle_displayed(transaction).unwrap();

    assert!(released.idle_fence_triggered);
    assert!(sophia_xshmfence::query(&idle_query).unwrap());
    assert_eq!(coordinator.resources().presentation_count(), 0);
}
