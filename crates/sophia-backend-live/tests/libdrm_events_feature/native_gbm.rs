#[cfg(feature = "gbm-probe")]
#[test]
fn pending_renderer_work_can_be_discarded_only_before_worker_ownership() {
    let mut exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingRenderDevice);
    exporter.set_pending_mixed_frame(sophia_renderer_live::LiveOwnedMixedCompositionFrame {
        image_reads: Default::default(),
        layers: Vec::new(),
        output_damage_snapshot: None,
        trace: None,
        direct_scanout: Default::default(),
    });

    assert!(exporter.pending_frame());
    assert!(exporter.discard_pending_frame());
    assert!(!exporter.pending_frame());
    assert!(!exporter.discard_pending_frame());
}

#[cfg(feature = "gbm-probe")]
#[test]
fn live_runtime_tick_native_gbm_rendered_scanout_fails_closed_when_render_device_is_unavailable() {
    let root = ready_drm_sysfs_fixture("runtime-native-gbm-rendered-primary-plane-unavailable");
    let report = discover_live_backend(&LiveBackendConfig::new(&root));
    let mut assembly = report
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .expect("ready backend should seed live assembly");
    let device = full_primary_plane_scanout_device();
    let mut exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingRenderDevice);
    let frame = LiveCpuComposedFrame {
        size: Size {
            width: 1920,
            height: 1080,
        },
        stride: 1920 * 4,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        bytes: vec![7; 1920 * 1080 * 4].into(),
    };
    exporter.set_pending_cpu_frame(frame);
    assert!(exporter.pending_cpu_frame());

    let tick = assembly
        .run_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_with(
            CompositorBackendTickInput::default(),
            &device,
            &mut exporter,
        )
        .expect("native GBM rendered scanout path should fail closed through runtime state");

    assert_eq!(
        tick.rendered_primary_plane_scanout_submit
            .expect("active scanout submit should be reported")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::ScanoutExportFailed
    );
    assert_eq!(
        tick.rendered_primary_plane_scanout_submit
            .unwrap()
            .export_detail,
        Some(LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable),
        "the typed exporter failure must survive preparation, submission and tracking"
    );
    assert_eq!(
        tick.engine.runtime.runtime_state.last_scanout_state,
        Some(RuntimeScanoutState::Rejected)
    );
    assert_eq!(tick.engine.runtime.runtime_state.scanout_rejections, 1);
    assert!(!assembly.rendered_primary_plane_scanout_in_flight());
    assert_eq!(exporter.export_attempts(), 1);
    assert_eq!(exporter.context_open_attempts(), 1);
    assert_eq!(
        exporter.context_status(),
        Some(NativeGbmRenderedScanoutContextStatus::Unavailable)
    );
    assert!(!exporter.context_ready());
    assert_eq!(exporter.cpu_frame_export_attempts(), 0);
    assert_eq!(exporter.last_cpu_frame_checksum(), None);
    assert!(exporter.pending_cpu_frame());
    assert_eq!(
        exporter.last_export_status(),
        Some(LiveRendererScanoutBufferExportStatus::Unavailable)
    );

    let second_tick = assembly
        .run_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_with(
            CompositorBackendTickInput::default(),
            &device,
            &mut exporter,
        )
        .expect("reusable native GBM exporter should survive another runtime tick");

    assert_eq!(
        second_tick
            .rendered_primary_plane_scanout_submit
            .expect("active scanout submit should be reported")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::ScanoutExportFailed
    );
    assert_eq!(exporter.export_attempts(), 2);
    assert_eq!(exporter.context_open_attempts(), 2);
    assert_eq!(
        exporter.context_status(),
        Some(NativeGbmRenderedScanoutContextStatus::Unavailable)
    );
    assert!(!exporter.context_ready());
    assert_eq!(
        exporter.last_export_status(),
        Some(LiveRendererScanoutBufferExportStatus::Unavailable)
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "gbm-probe")]
#[test]
fn live_runtime_tick_native_gbm_rendered_scanout_reads_native_page_flips_before_persistent_export()
{
    let root = ready_drm_sysfs_fixture("runtime-native-gbm-rendered-scanout-native-page-flip");
    let report = discover_live_backend(&LiveBackendConfig::new(&root));
    let (sender, receiver) = mpsc::sync_channel(2);
    let mut assembly = report
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .expect("ready backend should seed live assembly")
        .with_page_flip_callback_queue(LivePageFlipCallbackQueue::new(receiver, 2));
    let device = full_primary_plane_scanout_device();
    let mut initial_exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });
    let submitted = assembly
        .submit_and_track_rendered_primary_plane_scanout_with(&device, &mut initial_exporter);
    assert_eq!(
        submitted.status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );

    let slot = LibdrmNativeOutputSlot::new(1).expect("slot one should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(
        LibdrmBackendFdAuthority::new(34).expect("nonzero authority should mint"),
    );
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(1),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut reader =
        FakeLibdrmNativePageFlipReader::new([LibdrmNativePageFlipCallback::new(slot, 100)]);
    let mut exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingRenderDevice);

    let report = assembly
        .run_tick_with_native_gbm_rendered_primary_plane_scanout_exporter_and_native_page_flip_events_with(
            CompositorBackendTickInput::default(),
            &device,
            &mut exporter,
            &mut reader,
            &mut poller,
            &sender,
            4,
            4,
        )
        .expect("native page-flip intake should run before persistent GBM export");

    assert_eq!(
        report.native_page_flip.read_loop,
        LibdrmNativeReadLoopReport::callback_decoded(1)
            .expect("one callback should produce read evidence")
    );
    assert_eq!(
        report.native_page_flip.poll.status,
        LibdrmPageFlipEventPollStatus::Emitted
    );
    assert_eq!(
        report
            .tick
            .rendered_primary_plane_scanout_retire
            .expect("native page flip should retire in-flight scanout")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip
    );
    assert_eq!(
        report
            .tick
            .rendered_primary_plane_scanout_submit
            .expect("persistent native GBM export should be attempted")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::ScanoutExportFailed
    );
    assert_eq!(
        report.tick.libdrm_poller,
        LiveLibdrmPollerDiagnostics {
            status: LiveLibdrmPollerDiagnosticsStatus::CallbackDecoded,
            route_count: 1,
            pending_callbacks: 0,
            decoded_callbacks: 1,
            rejected_callbacks: 0,
        }
    );
    assert_eq!(
        report.tick.engine.runtime.runtime_state.scanout_retirements,
        1
    );
    assert_eq!(
        report.tick.engine.runtime.runtime_state.scanout_rejections,
        1
    );
    assert_eq!(
        report.tick.engine.runtime.runtime_state.last_scanout_state,
        Some(RuntimeScanoutState::Rejected)
    );
    assert!(!assembly.rendered_primary_plane_scanout_in_flight());
    assert_eq!(exporter.export_attempts(), 1);
    assert_eq!(exporter.context_open_attempts(), 1);
    assert_eq!(
        exporter.context_status(),
        Some(NativeGbmRenderedScanoutContextStatus::Unavailable)
    );
    assert_eq!(
        exporter.last_export_status(),
        Some(LiveRendererScanoutBufferExportStatus::Unavailable)
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "gbm-probe")]
#[test]
fn native_gbm_rendered_scanout_exporter_rejects_invalid_target_before_device_open() {
    let mut exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingRenderDevice);
    let target = LiveGbmEglFrameTargetRecord::new(Size {
        width: 0,
        height: 720,
    });

    let export = exporter.export_rendered_scanout_buffer(target);

    assert_eq!(
        export.status,
        LiveRendererScanoutBufferExportStatus::InvalidTarget
    );
    assert_eq!(exporter.export_attempts(), 1);
    assert_eq!(exporter.context_open_attempts(), 0);
    assert_eq!(exporter.context_status(), None);
    assert!(!exporter.context_ready());
    assert_eq!(
        exporter.last_export_status(),
        Some(LiveRendererScanoutBufferExportStatus::InvalidTarget)
    );
    assert_eq!(exporter.last_target(), Some(target));
    assert_eq!(
        exporter.last_target_lifecycle(),
        Some(LiveGbmEglFrameTargetLifecycleReport {
            status: LiveGbmEglFrameTargetLifecycleStatus::Invalidated,
            target,
        })
    );
}

#[cfg(feature = "gbm-probe")]
#[test]
fn native_gbm_renderer_worker_refuses_a_missing_device_before_spawning() {
    let result = NativeGbmRenderedScanoutBufferDiscoveryExporter::new_worker(MissingRenderDevice);
    assert!(
        result.is_err(),
        "a worker cannot reserve an unidentified device"
    );
}

#[cfg(feature = "gbm-probe")]
#[test]
fn native_gbm_renderer_image_owner_stays_absent_when_worker_admission_fails() {
    let mut exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingRenderDevice);
    assert!(!exporter.renderer_image_owner_initialized());
    assert!(exporter.enable_worker().is_err());
    assert!(!exporter.renderer_image_owner_initialized());
}

#[cfg(feature = "gbm-probe")]
#[test]
fn direct_cpu_bootstrap_failure_never_opens_an_inline_egl_context() {
    let mut exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingRenderDevice);
    exporter.set_pending_cpu_frame(LiveCpuComposedFrame {
        size: Size {
            width: 64,
            height: 48,
        },
        stride: 64 * 4,
        format: LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
        bytes: vec![0x33; 64 * 48 * 4].into(),
    });
    exporter
        .arm_direct_cpu_bootstrap()
        .expect("fresh exporter should admit one direct bootstrap");

    let export = exporter.export_rendered_scanout_buffer(LiveGbmEglFrameTargetRecord::new(Size {
        width: 64,
        height: 48,
    }));

    assert_eq!(
        export.status,
        LiveRendererScanoutBufferExportStatus::Unavailable
    );
    assert_eq!(exporter.direct_cpu_bootstrap_attempts(), 1);
    assert_eq!(exporter.direct_cpu_bootstrap_exports(), 0);
    assert_eq!(exporter.context_open_attempts(), 0);
    assert_eq!(exporter.context_status(), None);
    assert!(!exporter.context_ready());
    assert!(!exporter.renderer_image_owner_initialized());
}

#[cfg(feature = "gbm-probe")]
#[test]
fn native_gbm_rendered_scanout_exporter_rejects_forged_ready_target_before_device_open() {
    let mut exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingRenderDevice);
    let target = LiveGbmEglFrameTargetRecord {
        status: LiveGbmEglFrameTargetStatus::Ready,
        size: Size {
            width: -1,
            height: 720,
        },
    };

    let export = exporter.export_rendered_scanout_buffer(target);

    assert_eq!(
        export.status,
        LiveRendererScanoutBufferExportStatus::InvalidTarget
    );
    assert_eq!(exporter.export_attempts(), 1);
    assert_eq!(exporter.context_open_attempts(), 0);
    assert_eq!(exporter.context_status(), None);
    assert!(!exporter.context_ready());
    assert_eq!(
        exporter.last_export_status(),
        Some(LiveRendererScanoutBufferExportStatus::InvalidTarget)
    );
    assert_eq!(exporter.last_target(), Some(target));
    assert_eq!(
        exporter.last_target_lifecycle(),
        Some(LiveGbmEglFrameTargetLifecycleReport {
            status: LiveGbmEglFrameTargetLifecycleStatus::Created,
            target,
        })
    );
}

#[cfg(feature = "gbm-probe")]
#[test]
fn native_gbm_rendered_scanout_exporter_tracks_reduced_target_reuse_and_resize() {
    let mut exporter = NativeGbmRenderedScanoutBufferDiscoveryExporter::new(MissingRenderDevice);
    let first = LiveGbmEglFrameTargetRecord::new(Size {
        width: 1280,
        height: 720,
    });
    let resized = LiveGbmEglFrameTargetRecord::new(Size {
        width: 1920,
        height: 1080,
    });

    let first_export = exporter.export_rendered_scanout_buffer(first);
    assert_eq!(
        first_export.status,
        LiveRendererScanoutBufferExportStatus::Unavailable
    );
    assert_eq!(
        exporter.last_target_lifecycle(),
        Some(LiveGbmEglFrameTargetLifecycleReport {
            status: LiveGbmEglFrameTargetLifecycleStatus::Created,
            target: first,
        })
    );

    let retained_export = exporter.export_rendered_scanout_buffer(first);
    assert_eq!(
        retained_export.status,
        LiveRendererScanoutBufferExportStatus::Unavailable
    );
    assert_eq!(
        exporter.last_target_lifecycle(),
        Some(LiveGbmEglFrameTargetLifecycleReport {
            status: LiveGbmEglFrameTargetLifecycleStatus::Retained,
            target: first,
        })
    );

    let resized_export = exporter.export_rendered_scanout_buffer(resized);
    assert_eq!(
        resized_export.status,
        LiveRendererScanoutBufferExportStatus::Unavailable
    );
    assert_eq!(
        exporter.last_target_lifecycle(),
        Some(LiveGbmEglFrameTargetLifecycleReport {
            status: LiveGbmEglFrameTargetLifecycleStatus::Resized,
            target: resized,
        })
    );
    assert_eq!(exporter.last_target(), Some(resized));
    assert_eq!(exporter.export_attempts(), 3);
    assert_eq!(exporter.context_open_attempts(), 3);
}

#[test]
fn live_runtime_assembly_keeps_rendered_scanout_owner_until_page_flip_is_accepted() {
    let root = ready_drm_sysfs_fixture("runtime-rendered-primary-plane-wait");
    let report = discover_live_backend(&LiveBackendConfig::new(&root));
    let mut assembly = report
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .expect("ready backend should seed live assembly");
    let device = full_primary_plane_scanout_device();
    let mut exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });
    let mut submitted = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);
    let submission = submitted
        .submission
        .take()
        .expect("rendered scanout submit should retain both owners");
    let rejected = LivePageFlipCallbackReport {
        decision: LivePageFlipCallbackDecision::RejectedStaleFrameSerial,
        event: LivePageFlipEvent {
            status: LivePageFlipEventStatus::Rejected,
            frame_serial: Some(54),
        },
    };

    let waiting =
        retire_rendered_primary_plane_scanout_after_page_flip(&device, submission, &rejected);

    assert_eq!(
        waiting.status,
        LibdrmNativePrimaryPlaneScanoutRetireStatus::WaitingForAcceptedPageFlip
    );
    assert_eq!(waiting.runtime_scanout_state(), None);
    assert!(waiting.destroy.is_none());
    let owner = waiting
        .submission
        .expect("waiting retirement must keep rendered scanout owner")
        .into_scanout_buffer();
    assert_eq!(owner, FakeRenderedScanoutOwner { raw: 7 });

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_runtime_assembly_fails_rendered_scanout_submit_before_kms_on_export_failure() {
    let root = ready_drm_sysfs_fixture("runtime-rendered-primary-plane-export-fail");
    let report = discover_live_backend(&LiveBackendConfig::new(&root));
    let mut assembly = report
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .expect("ready backend should seed live assembly");
    let device = full_primary_plane_scanout_device();
    let mut exporter = FakeRenderedScanoutExporter::unavailable();

    let submitted = assembly.submit_rendered_primary_plane_scanout_with(&device, &mut exporter);

    assert_eq!(
        submitted.status,
        LiveRenderedPrimaryPlaneScanoutSubmitStatus::ScanoutExportFailed
    );
    assert_eq!(
        submitted.runtime_scanout_state(),
        RuntimeScanoutState::Rejected
    );
    assert_eq!(
        submitted.export,
        Some(LiveRendererScanoutBufferExportStatus::Unavailable)
    );
    assert!(submitted.submit.is_none());
    assert!(submitted.submission.is_none());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mirror_preview_withdrawal_never_poisons_or_waives_a_submitted_owner() {
    let output = sophia_protocol::OutputId::from_raw(1);
    let a = sophia_engine::RenderHeadId::from_raw(1);
    let b = sophia_engine::RenderHeadId::from_raw(2);
    let frame = |n| sophia_backend_live::LiveProductionNativeFrameId::from_raw(n);
    let mut group =
        sophia_backend_live::LiveProductionMirrorGroupLifecycle::new(output, [a, b]).unwrap();
    group.mark_initialized(a);
    group.mark_initialized(b);
    assert_eq!(
        group.begin(frame(1)),
        sophia_backend_live::LiveProductionMirrorGroupBegin::Started
    );
    assert!(group.withdraw_unsubmitted(frame(1)));
    assert!(!group.failed());
    assert!(group.active_age().is_none());
    assert_eq!(
        group.begin(frame(2)),
        sophia_backend_live::LiveProductionMirrorGroupBegin::Started
    );
    group.mark_submitted(a, frame(2));
    assert!(!group.withdraw_unsubmitted(frame(2)));
    assert!(!group.withdraw_unsubmitted(frame(1)));
    assert!(!group.failed());
    assert!(group.awaiting_flips());
}

#[test]
fn withdrawing_a_new_mirror_frame_keeps_the_older_flip_watchdog() {
    let output = sophia_protocol::OutputId::from_raw(1);
    let a = sophia_engine::RenderHeadId::from_raw(1);
    let b = sophia_engine::RenderHeadId::from_raw(2);
    let frame = |n| sophia_backend_live::LiveProductionNativeFrameId::from_raw(n);
    let mut group =
        sophia_backend_live::LiveProductionMirrorGroupLifecycle::new(output, [a, b]).unwrap();
    group.mark_initialized(a);
    group.mark_initialized(b);
    group.begin(frame(1));
    group.mark_submitted(b, frame(1));
    group.begin(frame(2));
    assert!(group.withdraw_unsubmitted(frame(2)));
    assert!(group.awaiting_flips());
    assert!(group.active_generation_hard_stalled(std::time::Duration::ZERO));
    assert!(!group.failed());
}
