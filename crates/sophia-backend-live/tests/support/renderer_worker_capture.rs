fn capture_identity(frame: u64) -> crate::LiveNativeFrameIdentity {
    crate::NativeFrameOwner::new().frame(
        sophia_protocol::OutputId::from_raw(7),
        sophia_engine::RenderHeadId::from_raw(17),
        3,
        frame,
    )
}

fn capture_job(identity: crate::LiveNativeFrameIdentity, red: u8) -> super::PendingRenderedFrame {
    let frame = sophia_renderer_live::LiveOwnedMixedCompositionFrame {
        layers: vec![
            sophia_renderer_live::LiveOwnedMixedCompositionLayer::Solid {
                geometry: sophia_protocol::Rect {
                    x: 0,
                    y: 0,
                    width: 16,
                    height: 16,
                },
                color: sophia_engine::CompositorRgb8 {
                    red,
                    green: 23,
                    blue: 45,
                },
            },
        ],
        ..Default::default()
    };
    super::PendingRenderedFrame::Mixed(frame, Some(identity))
}

#[test]
fn capture_request_cannot_follow_a_superseding_frame() {
    let (core, commands, resume) = inventory_test_core(32);
    resume.send(()).unwrap();
    let mut exporter =
        crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingWorkerDevice);
    exporter.attach_shared_worker(&core);
    let WorkerCommand::Register { .. } = commands.recv().unwrap() else {
        panic!("register")
    };
    let expected = capture_identity(1);
    let other = capture_identity(2);
    let super::PendingRenderedFrame::Mixed(frame, _) = capture_job(expected, 17) else {
        unreachable!()
    };
    exporter.set_pending_identified_mixed_frame(frame, Some(expected));
    assert!(!exporter.request_pending_frame_capture(other, sophia_renderer_live::LiveSnapshotReservation::for_image(16, 16).unwrap()));
    assert!(exporter.request_pending_frame_capture(expected, sophia_renderer_live::LiveSnapshotReservation::for_image(16, 16).unwrap()));
    let super::PendingRenderedFrame::Mixed(frame, _) = capture_job(other, 18) else {
        unreachable!()
    };
    exporter.set_pending_identified_mixed_frame(frame, Some(other));
    assert!(exporter.capture_requested.is_none());
    assert!(!exporter.request_pending_frame_capture(expected, sophia_renderer_live::LiveSnapshotReservation::for_image(16, 16).unwrap()));
}

#[test]
fn requested_capture_bypasses_an_otherwise_valid_direct_candidate() {
    use sophia_renderer_live::*;
    for capture in [false, true] {
        let (core, commands, resume) = inventory_test_core(32);
        resume.send(()).unwrap();
        let mut exporter =
            crate::NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingWorkerDevice);
        exporter.attach_shared_worker(&core);
        let WorkerCommand::Register { .. } = commands.recv().unwrap() else {
            panic!("register")
        };
        exporter.set_direct_scanout_enabled(true);
        let identity = capture_identity(1);
        let frame = LiveOwnedMixedCompositionFrame {
            direct_scanout: sophia_engine::DirectScanoutVerdict::Eligible,
            layers: vec![LiveOwnedMixedCompositionLayer::DmaBuf {
                image_id: LiveRendererImageId::from_raw(9),
                frame: LiveOwnedMultiPlaneDmaBufFrame {
                    width: 16,
                    height: 16,
                    format: sophia_protocol::DRM_FORMAT_XRGB8888,
                    modifier: 0,
                    plane_count: 1,
                    planes: [
                        Some(LiveOwnedDmaBufPlane {
                            fd: std::fs::File::open("/dev/null").unwrap().into(),
                            offset: 0,
                            stride: 64,
                        }),
                        None,
                        None,
                        None,
                    ],
                },
                placement: LiveCompositionPlacement {
                    target: sophia_protocol::Rect {
                        x: 0,
                        y: 0,
                        width: 16,
                        height: 16,
                    },
                    clip: None,
                    transform: sophia_protocol::Transform::IDENTITY,
                    alpha: 1.0,
                    sampling: sophia_engine::HeadSamplingClass::Exact,
                },
            }],
            ..Default::default()
        };
        exporter.set_pending_identified_mixed_frame(frame, Some(identity));
        if capture {
            assert!(exporter.request_pending_frame_capture(identity, sophia_renderer_live::LiveSnapshotReservation::for_image(16, 16).unwrap()));
        }
        let direct = exporter.try_direct_scanout_export(
            LiveGbmEglFrameTargetRecord::new(Size {
                width: 16,
                height: 16,
            }),
            false,
        );
        assert_eq!(direct.is_some(), !capture);
        if capture {
            assert!(exporter.pending_frame.is_some());
        }
    }
}

pub(crate) fn capture_lease(
    identity: crate::LiveNativeFrameIdentity,
) -> super::NativeGbmRendererWorkerScanoutLease {
    let (mut facade, commands, results) = correlated_facade();
    facade
        .submit_with_capture(
            LiveGbmEglFrameTargetRecord::new(Size {
                width: 16,
                height: 16,
            }),
            capture_job(identity, 17),
            vec![],
            None,
            true,
        )
        .unwrap();
    let WorkerCommand::Render {
        request_id,
        frame,
        capture,
        ..
    } = commands.recv().unwrap()
    else {
        panic!("render")
    };
    assert!(capture.is_some());
    let correlation = super::frame_correlation(&frame, Some(request_id));
    let mut outcome = exported_outcome();
    let WorkerOutcome::Exported { capture, .. } = &mut outcome else {
        unreachable!()
    };
    *capture = Some(super::super::LiveNativeFrameCapture::new(
        identity,
        sophia_renderer_live::LiveCompositionSnapshot::from_rgba(16, 16, vec![17; 1024]).unwrap(),
    ));
    results
        .send(correlated_result(correlation, outcome))
        .unwrap();
    let super::WorkerPoll::Exported(lease) = facade.poll() else {
        panic!("matching image")
    };
    lease
}

#[test]
fn capture_requires_exact_native_identity_and_requested_storage() {
    for bad in 0..3 {
        let (mut facade, commands, results) = correlated_facade();
        let expected = capture_identity(12);
        facade
            .submit_with_capture(
                LiveGbmEglFrameTargetRecord::new(Size {
                    width: 16,
                    height: 16,
                }),
                capture_job(expected, 17),
                vec![],
                None,
                true,
            )
            .unwrap();
        let WorkerCommand::Render {
            request_id, frame, ..
        } = commands.recv().unwrap()
        else {
            panic!("render")
        };
        let correlation = super::frame_correlation(&frame, Some(request_id));
        let mut outcome = exported_outcome();
        let WorkerOutcome::Exported { capture, .. } = &mut outcome else {
            unreachable!()
        };
        if bad != 0 {
            let (identity, size) = if bad == 1 {
                (capture_identity(12), 16)
            } else {
                (expected, 1)
            };
            *capture = Some(super::super::LiveNativeFrameCapture::new(
                identity,
                sophia_renderer_live::LiveCompositionSnapshot::from_rgba(
                    size,
                    size,
                    vec![1; (size * size * 4) as usize],
                )
                .unwrap(),
            ));
        }
        results
            .send(correlated_result(correlation, outcome))
            .unwrap();
        assert!(
            matches!(facade.poll(), super::WorkerPoll::Failed(_)),
            "bad image {bad}"
        );
    }
    let expected = capture_identity(13);
    let lease = capture_lease(expected);
    assert_eq!(
        lease.capture_for(expected).unwrap().image().rgba(),
        [17; 1024]
    );
    assert!(lease.capture_for(capture_identity(13)).is_none());
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; isolated worker readback"]
fn native_worker_capture_keeps_pixels_with_the_exact_exported_owner() {
    use sophia_renderer_live::*;
    let path = std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("explicit render-node grant");
    let open = || {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
    };
    let mut producer = NativeGbmRenderedScanoutContext::from_backend_device_result(open())
        .context
        .unwrap();
    let core = super::NativeGbmRendererWorkerCore::spawn(open()).unwrap();
    let mut facade = core.attach(LiveRendererWorkerOutputKey::from_raw(901));
    let mut retained = None;
    for (turn, capture) in [true, false, true].into_iter().enumerate() {
        let identity = capture_identity(turn as u64 + 1);
        let red = 17 + turn as u8;
        let target = LiveGbmEglFrameTargetRecord::new(Size {
            width: 16,
            height: 16,
        });
        let super::PendingRenderedFrame::Mixed(mut frame, _) = capture_job(identity, red) else {
            unreachable!()
        };
        let placement = LiveCompositionPlacement {
            target: sophia_protocol::Rect {
                x: 0,
                y: 0,
                width: 16,
                height: 16,
            },
            clip: None,
            transform: sophia_protocol::Transform::IDENTITY,
            alpha: 1.0,
            sampling: sophia_engine::HeadSamplingClass::Exact,
        };
        let gpu_owner = if turn == 2 {
            let buffer = producer
                .export_owned_mixed_frame(target, &frame, LiveCompositionOutputRequest::default())
                .unwrap()
                .buffer
                .unwrap();
            let descriptor = buffer.descriptor();
            let mut fds = buffer
                .export_scanout_dma_buf_fds()
                .unwrap()
                .into_plane_fds();
            frame.layers = vec![LiveOwnedMixedCompositionLayer::DmaBuf {
                image_id: LiveRendererImageId::from_raw(91),
                placement,
                frame: LiveOwnedMultiPlaneDmaBufFrame {
                    width: 16,
                    height: 16,
                    format: descriptor.format,
                    modifier: descriptor
                        .modifier
                        .unwrap_or(sophia_protocol::DRM_FORMAT_MOD_INVALID),
                    plane_count: descriptor.plane_count,
                    planes: std::array::from_fn(|i| {
                        fds[i].take().map(|fd| LiveOwnedDmaBufPlane {
                            fd,
                            offset: descriptor.plane_offsets[i],
                            stride: descriptor.plane_pitches[i],
                        })
                    }),
                },
            }];
            Some(buffer)
        } else {
            frame.layers = vec![LiveOwnedMixedCompositionLayer::Cpu {
                placement,
                buffer: LiveSharedCpuBufferSource {
                    handle: 9,
                    size: Size {
                        width: 16,
                        height: 16,
                    },
                    stride: 64,
                    format: sophia_protocol::DRM_FORMAT_XRGB8888,
                    generation: turn as u64 + 1,
                    bytes: std::sync::Arc::new([45, 23, red, 255].repeat(256)).into(),
                },
            }];
            None
        };
        facade
            .submit_with_capture(
                target,
                super::PendingRenderedFrame::Mixed(frame, Some(identity)),
                vec![],
                None,
                capture,
            )
            .unwrap();
        let start = std::time::Instant::now();
        let lease = loop {
            match facade.poll() {
                super::WorkerPoll::Exported(lease) => break lease,
                super::WorkerPoll::Pending { .. } => {
                    assert!(start.elapsed() < Duration::from_secs(10));
                    std::thread::sleep(Duration::from_millis(1));
                }
                super::WorkerPoll::Failed(detail) => {
                    panic!("native capture turn {turn} failed: {detail:?}")
                }
                _ => panic!("native capture turn {turn} did not export"),
            }
        };
        assert_eq!(lease.correlation().native, Some(identity));
        if capture {
            let image = lease.capture_for(identity).unwrap();
            assert!(
                image
                    .image()
                    .rgba()
                    .chunks_exact(4)
                    .all(|p| p == [red, 23, 45, 255])
            );
            if retained.is_none() {
                retained = Some(image);
            }
        } else {
            assert!(lease.capture_for(identity).is_none());
        }
        drop(lease);
        drop(gpu_owner);
    }
    // Worker destruction is asynchronous. Settle its exact thread before
    // destroying the independent producer or letting the test process exit.
    drop(facade);
    core.request_shutdown();
    let shutdown = std::time::Instant::now();
    while !core.poll_shutdown().unwrap() {
        assert!(shutdown.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(core);
    drop(producer);
    assert!(
        retained
            .unwrap()
            .image()
            .rgba()
            .chunks_exact(4)
            .all(|p| p == [17, 23, 45, 255])
    );
}
