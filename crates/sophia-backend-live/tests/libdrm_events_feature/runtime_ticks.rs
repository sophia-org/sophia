#[test]
fn live_runtime_tick_retries_pending_rendered_scanout_cleanup_before_submit() {
    let root = ready_drm_sysfs_fixture("runtime-rendered-primary-plane-cleanup-auto-retry");
    let report = discover_live_backend(&LiveBackendConfig::new(&root));
    let mut assembly = report
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .expect("ready backend should seed live assembly");
    let failing_device = FakeNativePrimaryPlaneScanoutDevice {
        resources: FakeNativePrimaryPlaneResourceDevice {
            destroy_framebuffer: Err(io::Error::other("test framebuffer destroy failed")),
            ..full_primary_plane_resource_device()
        },
        ..full_primary_plane_scanout_device()
    };
    let retry_device = full_primary_plane_scanout_device();
    let mut exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });

    let submitted = assembly
        .submit_and_track_rendered_primary_plane_scanout_with(&failing_device, &mut exporter);
    assert_eq!(
        submitted.status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    let accepted = LivePageFlipCallbackReport {
        decision: LivePageFlipCallbackDecision::Accepted,
        event: LivePageFlipEvent {
            status: LivePageFlipEventStatus::Presented,
            frame_serial: Some(55),
        },
    };
    let retired = assembly
        .retire_tracked_rendered_primary_plane_scanout_after_page_flip(&failing_device, &accepted);
    assert_eq!(
        retired.status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::ResourceRetireFailed
    );
    assert!(assembly.rendered_primary_plane_scanout_cleanup_pending());

    let mut next_exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });
    let tick = assembly
        .run_tick_with_rendered_primary_plane_scanout_with(
            CompositorBackendTickInput::default(),
            &retry_device,
            &mut next_exporter,
        )
        .expect("device-backed tick should retry pending cleanup and submit next scanout");

    assert_eq!(
        tick.rendered_primary_plane_scanout_cleanup_retry
            .expect("pending cleanup should be retried")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::CleanedUp
    );
    assert!(!tick.rendered_primary_plane_scanout_cleanup_pending);
    assert!(!assembly.rendered_primary_plane_scanout_cleanup_pending());
    assert_eq!(
        tick.rendered_primary_plane_scanout_submit
            .expect("runtime should still submit the next scanout")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_runtime_tick_reports_failed_rendered_scanout_cleanup_retry() {
    let root = ready_drm_sysfs_fixture("runtime-rendered-primary-plane-cleanup-auto-retry-fail");
    let report = discover_live_backend(&LiveBackendConfig::new(&root));
    let mut assembly = report
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .expect("ready backend should seed live assembly");
    let failing_device = FakeNativePrimaryPlaneScanoutDevice {
        resources: FakeNativePrimaryPlaneResourceDevice {
            destroy_framebuffer: Err(io::Error::other("test framebuffer destroy failed")),
            ..full_primary_plane_resource_device()
        },
        ..full_primary_plane_scanout_device()
    };
    let mut exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });

    let submitted = assembly
        .submit_and_track_rendered_primary_plane_scanout_with(&failing_device, &mut exporter);
    assert_eq!(
        submitted.status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    let accepted = LivePageFlipCallbackReport {
        decision: LivePageFlipCallbackDecision::Accepted,
        event: LivePageFlipEvent {
            status: LivePageFlipEventStatus::Presented,
            frame_serial: Some(55),
        },
    };
    let retired = assembly
        .retire_tracked_rendered_primary_plane_scanout_after_page_flip(&failing_device, &accepted);
    assert_eq!(
        retired.status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::ResourceRetireFailed
    );
    assert!(assembly.rendered_primary_plane_scanout_cleanup_pending());

    let mut next_exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });
    let tick = assembly
        .run_tick_with_rendered_primary_plane_scanout_with(
            CompositorBackendTickInput::default(),
            &failing_device,
            &mut next_exporter,
        )
        .expect("device-backed tick should report failed cleanup retry");

    assert_eq!(
        tick.rendered_primary_plane_scanout_cleanup_retry
            .expect("pending cleanup should be retried")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutCleanupStatus::CleanupFailed
    );
    assert_eq!(
        tick.rendered_primary_plane_scanout_cleanup_retry
            .expect("pending cleanup should be retried")
            .reduced_log_line(),
        "sophia_runtime_rendered_scanout_cleanup schema=1 status=CleanupFailed destroy=FramebufferDestroyFailed cleanup_pending=true"
    );
    assert!(tick.rendered_primary_plane_scanout_cleanup_pending);
    assert!(assembly.rendered_primary_plane_scanout_cleanup_pending());
    assert_eq!(
        tick.rendered_primary_plane_scanout_submit
            .expect("failed cleanup retry should defer the next submit")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::CleanupPending
    );
    assert_eq!(
        tick.engine.runtime.runtime_state.last_scanout_state,
        Some(RuntimeScanoutState::Deferred)
    );
    assert!(!assembly.rendered_primary_plane_scanout_in_flight());

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_runtime_tick_submits_rendered_scanout_when_runtime_requests_scanout() {
    let root = ready_drm_sysfs_fixture("runtime-rendered-primary-plane-submit-command");
    let report = discover_live_backend(&LiveBackendConfig::new(&root));
    let (sender, receiver) = mpsc::sync_channel(2);
    let mut assembly = report
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .expect("ready backend should seed live assembly")
        .with_page_flip_callback_queue(LivePageFlipCallbackQueue::new(receiver, 2));
    let device = full_primary_plane_scanout_device();
    let mut exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });

    let tick = assembly
        .run_tick_with_rendered_primary_plane_scanout_with(
            CompositorBackendTickInput::default(),
            &device,
            &mut exporter,
        )
        .expect("runtime scanout command should use rendered primary-plane submit");

    let submit = tick
        .rendered_primary_plane_scanout_submit
        .expect("active scanout submit should be reported");
    assert_eq!(
        submit.status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert_eq!(
        submit.properties,
        Some(LibdrmNativePrimaryPlanePropertyDiscoveryStatus::Discovered)
    );
    assert_eq!(
        submit.resources,
        Some(LibdrmNativePrimaryPlaneResourceCreateStatus::Created)
    );
    assert_eq!(
        submit.request,
        Some(LibdrmNativeAtomicRequestBuildStatus::Built)
    );
    assert_eq!(
        submit.commit_submit,
        Some(LibdrmNativeAtomicCommitSubmitStatus::Submitted)
    );
    assert_eq!(
        submit.reduced_log_line(),
        "sophia_runtime_rendered_scanout_submit schema=6 status=SubmittedWaitingForPageFlip scanout_target=Ready output_size=1280x720 target=Ready target_size=1280x720 export=Exported scanout_buffer=Ready buffer_format=Xrgb8888 buffer_modifier=Implicit buffer_planes=Single properties=Discovered format_table=Present resources=Created framebuffer=CreatedWithAddFb2 request=Built submit=SubmittedWaitingForPageFlip request_scope=PageFlip commit_page_flip_event=true commit_nonblocking=true commit_allow_modeset=false commit_test_only=false commit_submit=Submitted runtime_scanout_state=Submitted in_flight=true in_flight_ticks=0 cleanup_pending=false"
    );
    assert_eq!(tick.engine.runtime.runtime_state.scanout_submissions, 1);
    assert_eq!(
        tick.engine.runtime.runtime_state.last_scanout_state,
        Some(RuntimeScanoutState::Submitted)
    );
    assert_eq!(
        tick.engine.runtime.runtime_state.last_scanout_frame_serial,
        Some(tick.engine.tick.frame_serial)
    );
    assert!(assembly.rendered_primary_plane_scanout_in_flight());
    assert_eq!(assembly.rendered_primary_plane_scanout_in_flight_ticks(), 0);
    assert_eq!(assembly.pending_runtime_scanout_state_count(), 0);
    assert_eq!(tick.rendered_primary_plane_scanout_in_flight_ticks, 0);
    assert_eq!(
        tick.rendered_primary_plane_scanout_backpressure,
        LiveRenderedPrimaryPlaneScanoutBackpressureReport {
            status: LiveRenderedPrimaryPlaneScanoutBackpressureStatus::WaitingForPageFlip,
            in_flight: true,
            in_flight_ticks: 0,
            threshold_ticks: LIVE_RENDERED_PRIMARY_PLANE_SCANOUT_STALL_THRESHOLD_TICKS,
        }
    );

    let deferred_tick = assembly
        .run_tick_with_rendered_primary_plane_scanout_with(
            CompositorBackendTickInput::default(),
            &device,
            &mut exporter,
        )
        .expect("runtime scanout command should defer while previous submit is in flight");

    assert_eq!(
        deferred_tick
            .rendered_primary_plane_scanout_submit
            .expect("active scanout submit should be reported")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::AlreadyInFlight
    );
    assert_eq!(
        deferred_tick.rendered_primary_plane_scanout_in_flight_ticks,
        1
    );
    assert_eq!(
        deferred_tick
            .rendered_primary_plane_scanout_submit
            .expect("active scanout submit should be reported")
            .in_flight_ticks,
        1
    );
    assert_eq!(
        deferred_tick
            .engine
            .runtime
            .runtime_state
            .scanout_submissions,
        1
    );
    assert_eq!(
        deferred_tick
            .engine
            .runtime
            .runtime_state
            .scanout_rejections,
        0
    );
    assert_eq!(
        deferred_tick
            .engine
            .runtime
            .runtime_state
            .in_flight_scanouts,
        1
    );
    assert_eq!(
        deferred_tick
            .engine
            .runtime
            .runtime_state
            .last_scanout_state,
        Some(RuntimeScanoutState::Deferred)
    );
    assert!(assembly.rendered_primary_plane_scanout_in_flight());
    assert_eq!(assembly.rendered_primary_plane_scanout_in_flight_ticks(), 1);
    assert_eq!(
        deferred_tick.rendered_primary_plane_scanout_backpressure,
        LiveRenderedPrimaryPlaneScanoutBackpressureReport {
            status: LiveRenderedPrimaryPlaneScanoutBackpressureStatus::WaitingForPageFlip,
            in_flight: true,
            in_flight_ticks: 1,
            threshold_ticks: LIVE_RENDERED_PRIMARY_PLANE_SCANOUT_STALL_THRESHOLD_TICKS,
        }
    );

    sender
        .try_send(LivePageFlipCallback {
            output: OutputId::from_raw(1),
            head: sophia_engine::RenderHeadId::from_raw(1),
            frame_serial: 99,
        })
        .expect("test channel should accept page-flip callback");
    let mut next_exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });
    let retire_and_submit_tick = assembly
        .run_tick_with_rendered_primary_plane_scanout_with(
            CompositorBackendTickInput::default(),
            &device,
            &mut next_exporter,
        )
        .expect("accepted page flip should retire previous submit and allow next submit");

    assert_eq!(
        retire_and_submit_tick
            .rendered_primary_plane_scanout_retire
            .expect("accepted page flip should retire in-flight scanout")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutRetireStatus::RetiredAfterPageFlip
    );
    assert_eq!(
        retire_and_submit_tick
            .rendered_primary_plane_scanout_retire
            .expect("accepted page flip should retire in-flight scanout")
            .destroy,
        Some(LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed)
    );
    assert_eq!(
        retire_and_submit_tick
            .rendered_primary_plane_scanout_retire
            .expect("accepted page flip should retire in-flight scanout")
            .in_flight_ticks,
        0
    );
    assert_eq!(
        retire_and_submit_tick.runtime_scanout_states,
        vec![RuntimeScanoutState::Retired]
    );
    assert_eq!(
        retire_and_submit_tick
            .rendered_primary_plane_scanout_submit
            .expect("runtime should submit the next rendered scanout")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert_eq!(
        retire_and_submit_tick
            .engine
            .runtime
            .runtime_state
            .scanout_retirements,
        1
    );
    assert_eq!(
        retire_and_submit_tick
            .engine
            .runtime
            .runtime_state
            .scanout_submissions,
        2
    );
    assert_eq!(
        retire_and_submit_tick
            .engine
            .runtime
            .runtime_state
            .in_flight_scanouts,
        1
    );
    assert!(assembly.rendered_primary_plane_scanout_in_flight());
    assert_eq!(assembly.rendered_primary_plane_scanout_in_flight_ticks(), 0);
    assert_eq!(
        retire_and_submit_tick.rendered_primary_plane_scanout_in_flight_ticks,
        0
    );
    assert_eq!(
        retire_and_submit_tick.rendered_primary_plane_scanout_backpressure,
        LiveRenderedPrimaryPlaneScanoutBackpressureReport {
            status: LiveRenderedPrimaryPlaneScanoutBackpressureStatus::WaitingForPageFlip,
            in_flight: true,
            in_flight_ticks: 0,
            threshold_ticks: LIVE_RENDERED_PRIMARY_PLANE_SCANOUT_STALL_THRESHOLD_TICKS,
        }
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn live_runtime_tick_reads_native_page_flip_events_before_rendered_scanout() {
    let root = ready_drm_sysfs_fixture("runtime-rendered-primary-plane-native-page-flip");
    let report = discover_live_backend(&LiveBackendConfig::new(&root));
    let (sender, receiver) = mpsc::sync_channel(2);
    let mut assembly = report
        .into_live_runtime_assembly(QueuedInputPoller::default())
        .expect("ready backend should seed live assembly")
        .with_page_flip_callback_queue(LivePageFlipCallbackQueue::new(receiver, 2));
    let device = full_primary_plane_scanout_device();
    let mut exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });
    let submitted =
        assembly.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(
        submitted.status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );

    let slot = LibdrmNativeOutputSlot::new(1).expect("slot one should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(
        LibdrmBackendFdAuthority::new(33).expect("nonzero authority should mint"),
    );
    let mut poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(1),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut reader =
        FakeLibdrmNativePageFlipReader::new([LibdrmNativePageFlipCallback::new(slot, 99)]);
    let mut next_exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });

    let report = assembly
        .run_tick_with_rendered_primary_plane_scanout_and_native_page_flip_events_with(
            CompositorBackendTickInput::default(),
            &device,
            &mut next_exporter,
            &mut reader,
            &mut poller,
            &sender,
            4,
            4,
        )
        .expect("native page-flip tick should retire and submit");

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
            .expect("runtime should submit the next scanout")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
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
    assert!(assembly.rendered_primary_plane_scanout_in_flight());
    assert_eq!(assembly.rendered_primary_plane_scanout_in_flight_ticks(), 0);

    std::fs::remove_dir_all(root).unwrap();
}
#[cfg(feature = "libinput-events")]
#[test]
fn live_runtime_tick_polls_libinput_shaped_input_while_retiring_and_submitting_scanout() {
    let root = ready_drm_sysfs_fixture("runtime-input-native-page-flip-rendered-scanout");
    let config = LiveBackendConfig::new(&root).with_input_device(LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    });
    let report = discover_live_backend(&config);
    let (sender, receiver) = mpsc::sync_channel(2);
    let poller = NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([libinput_motion_event(1, 42.0, 24.0)]),
        4,
    );
    let mut assembly = report
        .into_live_runtime_assembly(poller)
        .expect("ready backend should seed live assembly")
        .with_page_flip_callback_queue(LivePageFlipCallbackQueue::new(receiver, 2));
    let device = full_primary_plane_scanout_device();
    let mut exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });
    let submitted =
        assembly.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(
        submitted.status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );

    let slot = LibdrmNativeOutputSlot::new(1).expect("slot one should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(
        LibdrmBackendFdAuthority::new(35).expect("nonzero authority should mint"),
    );
    let mut page_flip_poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(1),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut reader =
        FakeLibdrmNativePageFlipReader::new([LibdrmNativePageFlipCallback::new(slot, 101)]);
    let mut next_exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });

    let report = assembly
        .run_tick_with_rendered_primary_plane_scanout_and_native_page_flip_events_with(
            CompositorBackendTickInput::default(),
            &device,
            &mut next_exporter,
            &mut reader,
            &mut page_flip_poller,
            &sender,
            4,
            4,
        )
        .expect("input, page-flip retirement, and scanout submit should share one tick");

    assert_eq!(report.tick.engine.input_poll.polled, 1);
    assert_eq!(report.tick.engine.input_poll.accepted, 1);
    assert!(report.tick.engine.input_poll.rejected.is_empty());
    assert_eq!(assembly.assembly().input().source().pending_len(), 1);
    assert_eq!(
        report.native_page_flip.read_loop,
        LibdrmNativeReadLoopReport::callback_decoded(1)
            .expect("one callback should produce read evidence")
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
            .expect("runtime should submit the next scanout")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert!(assembly.rendered_primary_plane_scanout_in_flight());

    std::fs::remove_dir_all(root).unwrap();
}
#[cfg(feature = "libinput-events")]
#[test]
fn live_session_loop_tick_leaves_input_idle_until_reduced_readiness() {
    let root = ready_drm_sysfs_fixture("session-loop-idle-input-rendered-scanout");
    let config = LiveBackendConfig::new(&root).with_input_device(LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    });
    let report = discover_live_backend(&config);
    let (sender, receiver) = mpsc::sync_channel(2);
    let poller = LiveInputReadinessGatedPoller::new(NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([libinput_motion_event(1, 42.0, 24.0)]),
        4,
    ));
    let mut assembly = report
        .into_live_runtime_assembly(poller)
        .expect("ready backend should seed live assembly")
        .with_page_flip_callback_queue(LivePageFlipCallbackQueue::new(receiver, 2));
    let device = full_primary_plane_scanout_device();
    let mut exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });
    let slot = LibdrmNativeOutputSlot::new(1).expect("slot one should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(
        LibdrmBackendFdAuthority::new(36).expect("nonzero authority should mint"),
    );
    let page_flip_poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(1),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut session_loop = LiveBackendSessionLoop::new(
        page_flip_poller,
        LiveBackendSessionLoopPageFlipBudget::new(4, 4),
    );
    let mut reader = FakeLibdrmNativePageFlipReader::new([]);

    let report = session_loop
        .run_tick_with_rendered_primary_plane_scanout_and_native_page_flip_events_with(
            &mut assembly,
            CompositorBackendTickInput::default(),
            LiveBackendSessionLoopReadiness::idle(),
            &device,
            &mut exporter,
            &mut reader,
            &sender,
        )
        .expect("session loop tick should keep moving when input is idle");

    assert_eq!(report.input_gate.status, LiveInputReadinessGateStatus::Idle);
    assert_eq!(report.tick.engine.input_poll.polled, 0);
    assert_eq!(assembly.assembly().input().source().pending_len(), 0);
    assert_eq!(
        assembly
            .assembly()
            .input()
            .poller()
            .inner()
            .reader()
            .queued_len(),
        1
    );
    assert_eq!(
        report
            .tick
            .rendered_primary_plane_scanout_submit
            .expect("idle input must not block scanout")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );
    assert_eq!(
        report.native_page_flip.read_loop.status,
        LibdrmNativeReadLoopStatus::Idle
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "libinput-events")]
#[test]
fn live_readiness_collector_drains_reduced_readiness_without_identity() {
    let mut collector = LiveBackendReadinessCollector::new();

    assert_eq!(
        collector.snapshot(),
        LiveBackendSessionLoopReadiness::idle()
    );

    collector.observe_input_ready();
    assert_eq!(
        collector.snapshot(),
        LiveBackendSessionLoopReadiness::input_ready()
    );

    collector.observe_page_flip_ready();
    assert_eq!(
        collector.drain(),
        LiveBackendSessionLoopReadiness::all_ready()
    );
    assert_eq!(
        collector.snapshot(),
        LiveBackendSessionLoopReadiness::idle()
    );
}

#[cfg(feature = "libinput-events")]
#[test]
fn live_session_loop_tick_skips_page_flip_read_until_reduced_ready() {
    let root = ready_drm_sysfs_fixture("session-loop-page-flip-readiness-gate");
    let config = LiveBackendConfig::new(&root).with_input_device(LibinputDeviceDescriptor {
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        kind: LibinputDeviceKind::Pointer,
    });
    let report = discover_live_backend(&config);
    let (sender, receiver) = mpsc::sync_channel(2);
    let poller = LiveInputReadinessGatedPoller::new(NativeLibinputEventPoller::new(
        FakeLiveLibinputEventReader::new([libinput_motion_event(1, 42.0, 24.0)]),
        4,
    ));
    let mut assembly = report
        .into_live_runtime_assembly(poller)
        .expect("ready backend should seed live assembly")
        .with_page_flip_callback_queue(LivePageFlipCallbackQueue::new(receiver, 2));
    let device = full_primary_plane_scanout_device();
    let mut exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });
    let submitted =
        assembly.submit_and_track_rendered_primary_plane_scanout_with(&device, &mut exporter);
    assert_eq!(
        submitted.status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::SubmittedWaitingForPageFlip
    );

    let slot = LibdrmNativeOutputSlot::new(1).expect("slot one should be valid");
    let source = LibdrmNativePageFlipSource::from_authority(
        LibdrmBackendFdAuthority::new(38).expect("nonzero authority should mint"),
    );
    let page_flip_poller =
        NativeLibdrmPageFlipEventPoller::new(source).with_routes([LibdrmNativeOutputRoute {
            slot,
            output: OutputId::from_raw(1),
            head: sophia_engine::RenderHeadId::from_raw(1),
        }]);
    let mut session_loop = LiveBackendSessionLoop::new(
        page_flip_poller,
        LiveBackendSessionLoopPageFlipBudget::new(4, 4),
    );
    let mut reader =
        FakeLibdrmNativePageFlipReader::new([LibdrmNativePageFlipCallback::new(slot, 104)]);
    let mut next_exporter = FakeRenderedScanoutExporter::exported(Size {
        width: 1280,
        height: 720,
    });

    let report = session_loop
        .run_tick_with_rendered_primary_plane_scanout_and_native_page_flip_events_with(
            &mut assembly,
            CompositorBackendTickInput::default(),
            LiveBackendSessionLoopReadiness::input_ready(),
            &device,
            &mut next_exporter,
            &mut reader,
            &sender,
        )
        .expect("session loop tick should skip native page-flip read without readiness");

    assert_eq!(
        report.input_gate.status,
        LiveInputReadinessGateStatus::Polled
    );
    assert_eq!(
        report.native_page_flip.read_loop.status,
        LibdrmNativeReadLoopStatus::Idle
    );
    assert_eq!(reader.queued_len(), 1);
    assert_eq!(
        report
            .tick
            .rendered_primary_plane_scanout_retire
            .map(|retire| retire.status),
        None
    );
    assert_eq!(
        report
            .tick
            .rendered_primary_plane_scanout_submit
            .expect("in-flight owner should defer next submit")
            .status,
        LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::AlreadyInFlight
    );
    assert!(assembly.rendered_primary_plane_scanout_in_flight());

    std::fs::remove_dir_all(root).unwrap();
}
