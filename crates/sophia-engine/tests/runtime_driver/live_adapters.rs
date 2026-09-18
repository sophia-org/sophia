#[test]
fn live_runtime_driver_adapter_reports_rejected_scanout_submit() {
    let engine = HeadlessEngine::default();
    let output = engine.output();
    let mut driver = HeadlessSessionDriver::new(engine);
    let mut adapter = LiveRuntimeDriverAdapter::from_intake(LiveRuntimeDriverIntake {
        x_event_count: 0,
        authority_commits: Vec::new(),
        authority_batches: Vec::new(),
        wm_update: None,
        portal_commands: Vec::new(),
        chrome_command_count: 0,
        layers: vec![test_layer(1, 0, 0, Region::empty())],
        committed_surfaces: Vec::new(),
        scanout_submit_state: Some(RuntimeScanoutState::Rejected),
        scanout_lifecycle_states: Vec::new(),
    });

    let report = driver
        .run_with_adapter(output.id, 96, &mut adapter)
        .expect("rejected scanout submit should stay reduced");

    assert_eq!(report.runtime_state.scanout_submissions, 0);
    assert_eq!(report.runtime_state.scanout_rejections, 1);
    assert_eq!(report.runtime_state.in_flight_scanouts, 0);
    assert_eq!(
        report.runtime_state.last_scanout_state,
        Some(RuntimeScanoutState::Rejected)
    );
}

#[test]
fn live_runtime_driver_adapter_records_async_scanout_retirement_before_submit() {
    let engine = HeadlessEngine::default();
    let output = engine.output();
    let mut driver = HeadlessSessionDriver::new(engine);
    let mut adapter = LiveRuntimeDriverAdapter::from_intake(LiveRuntimeDriverIntake {
        x_event_count: 0,
        authority_commits: Vec::new(),
        authority_batches: Vec::new(),
        wm_update: None,
        portal_commands: Vec::new(),
        chrome_command_count: 0,
        layers: vec![test_layer(1, 0, 0, Region::empty())],
        committed_surfaces: Vec::new(),
        scanout_submit_state: Some(RuntimeScanoutState::Submitted),
        scanout_lifecycle_states: vec![RuntimeScanoutState::Retired],
    });

    let report = driver
        .run_with_adapter(output.id, 97, &mut adapter)
        .expect("async scanout retirement should not disrupt frame scheduling");

    assert_eq!(report.runtime_state.scanout_retirements, 1);
    assert_eq!(report.runtime_state.scanout_submissions, 1);
    assert_eq!(report.runtime_state.in_flight_scanouts, 1);
    assert_eq!(report.runtime_state.phase, SessionRuntimePhase::Idle);
    assert_eq!(
        report.runtime_state.last_scanout_state,
        Some(RuntimeScanoutState::Submitted)
    );
}

#[test]
fn live_runtime_driver_adapter_records_authority_transaction_commits() {
    let engine = HeadlessEngine::default();
    let output = engine.output();
    let mut driver = HeadlessSessionDriver::new(engine);
    let mut adapter = LiveRuntimeDriverAdapter::from_intake(LiveRuntimeDriverIntake {
        x_event_count: 1,
        authority_commits: vec![TransactionCommit {
            transaction: TransactionId::from_raw(84),
            outcome: TransactionOutcome::Committed,
            applied_surfaces: vec![SurfaceId::new(7, 1)],
        }],
        authority_batches: Vec::new(),
        wm_update: None,
        portal_commands: Vec::new(),
        chrome_command_count: 0,
        layers: vec![test_layer(7, 0, 0, Region::empty())],
        committed_surfaces: Vec::new(),
        scanout_submit_state: None,
        scanout_lifecycle_states: Vec::new(),
    });

    let report = driver
        .run_with_adapter(output.id, 94, &mut adapter)
        .expect("authority transaction observations should feed runtime state");

    assert_eq!(report.runtime_state.x_events_polled, 1);
    assert_eq!(report.runtime_state.authority_transactions_committed, 1);
    assert_eq!(report.runtime_state.authority_surfaces_applied, 1);
}

#[test]
fn live_runtime_driver_adapter_commits_authority_batches_before_rendering() {
    let engine = HeadlessEngine::default();
    let output = engine.output();
    let surface = SurfaceId::new(9, 1);
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(86),
        authority: AuthorityKind::SophiaX,
        surface,
        namespace: Some(NamespaceId::from_raw(3)),
        target_geometry: Rect {
            x: 20,
            y: 30,
            width: 140,
            height: 90,
        },
        presentation_extent: Size {
            width: 140,
            height: 90,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle: 700 }, sophia_protocol::Size {
            width: 140,
            height: 90,
        }),

        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: 140,
            height: 90,
        }),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 0,
    };
    let mut driver = HeadlessSessionDriver::new(engine.clone());
    let mut adapter = LiveRuntimeDriverAdapter::from_authority_batches(
        &engine,
        LiveRuntimeDriverIntake {
            x_event_count: 1,
            authority_commits: Vec::new(),
            authority_batches: vec![AuthorityTransactionIntake::new(
                TransactionId::from_raw(86),
                vec![transaction],
            )],
            wm_update: None,
            portal_commands: Vec::new(),
            chrome_command_count: 0,
            layers: vec![test_layer(9, 0, 0, Region::empty())],
            committed_surfaces: Vec::new(),
            scanout_submit_state: None,
            scanout_lifecycle_states: Vec::new(),
        },
    );

    let report = driver
        .run_with_adapter(output.id, 95, &mut adapter)
        .expect("authority batches should commit before frame projection");

    assert_eq!(report.runtime_state.authority_transactions_committed, 1);
    assert_eq!(report.runtime_state.authority_surfaces_applied, 1);
    let frame = &report.session_tick.unwrap().frame;
    assert_eq!(frame.layers[0].geometry.x, 20);
    assert_eq!(
        frame.layers[0].source,
        BufferSource::CpuBuffer { handle: 700 }
    );
}

#[test]
fn live_x_runtime_adapter_emits_bounded_event_count_observation() {
    let adapter = LiveXRuntimeAdapter {
        pending_event_count: 12,
        authority_commits: Vec::new(),
    };

    assert_eq!(
        adapter.poll_observation(),
        SessionRuntimeObservation::XEventsPolled { count: 12 }
    );
}

#[test]
fn live_x_runtime_adapter_emits_authority_commit_observations() {
    let adapter = LiveXRuntimeAdapter {
        pending_event_count: 2,
        authority_commits: vec![TransactionCommit {
            transaction: TransactionId::from_raw(85),
            outcome: TransactionOutcome::Committed,
            applied_surfaces: vec![SurfaceId::new(8, 1)],
        }],
    };

    assert_eq!(
        adapter.poll_observations(),
        vec![
            SessionRuntimeObservation::XEventsPolled { count: 2 },
            SessionRuntimeObservation::AuthorityTransactionObserved {
                outcome: TransactionOutcome::Committed,
                applied_surface_count: 1,
            },
        ]
    );
}

#[test]
fn live_wm_runtime_adapter_maps_update_to_ready_observation() {
    let adapter = LiveWmRuntimeAdapter {
        update: Some(WmTransactionUpdate {
            commit: TransactionCommit {
                transaction: TransactionId::from_raw(82),
                outcome: TransactionOutcome::TimedOut,
                applied_surfaces: Vec::new(),
            },
        }),
    };

    assert_eq!(
        adapter.layout_observation(),
        SessionRuntimeObservation::WmLayoutReady
    );
}

#[test]
fn live_broker_runtime_adapter_routes_health_without_message_payload() {
    let packet = BrokerHealthPacket::new(
        BrokerKind::Portal,
        BrokerHealthState::Ready,
        44,
        Some("ready".to_owned()),
    )
    .unwrap();

    assert_eq!(
        LiveBrokerRuntimeAdapter::from_health_packet(&packet),
        SessionRuntimeObservation::BrokerHealthChanged {
            broker: BrokerKind::Portal,
            state: BrokerHealthState::Ready,
            generation: 44,
            status_message_len: 5,
        }
    );
}

#[test]
fn live_portal_chrome_and_renderer_adapters_emit_counts_and_frame_serials() {
    let portal = LivePortalRuntimeAdapter::from_commands(vec![
        PortalCommand::DropNotification {
            transfer: PortalTransferId::from_raw(1),
        },
        PortalCommand::DeliverNotification {
            transfer: PortalTransferId::from_raw(2),
        },
    ]);
    let notification_updates = [
        NotificationChromeUpdate::Staged {
            transfer: PortalTransferId::from_raw(1),
        },
        NotificationChromeUpdate::Presented {
            transfer: PortalTransferId::from_raw(1),
        },
        NotificationChromeUpdate::Dismissed {
            transfer: PortalTransferId::from_raw(1),
        },
    ];
    let chrome = LiveChromeRuntimeAdapter::from_notification_updates(&notification_updates);
    let engine = HeadlessEngine::default();
    let output = engine.output();
    let mut last_committed = LastCommittedLayout::default();
    let mut renderer =
        LiveRendererRuntimeAdapter::from_layers(vec![test_layer(1, 0, 0, Region::empty())]);

    let report = renderer
        .render_frame(&engine, output.id, 94, &mut last_committed)
        .unwrap();

    assert_eq!(
        portal.drain_observation(),
        SessionRuntimeObservation::PortalCommandsReady { count: 2 }
    );
    assert_eq!(
        chrome.present_observation(),
        SessionRuntimeObservation::ChromeCommandsReady { count: 2 }
    );
    assert_eq!(
        LiveRendererRuntimeAdapter::rendered_observation(&report),
        SessionRuntimeObservation::FrameRendered { frame_serial: 94 }
    );
    assert_eq!(
        LiveRendererRuntimeAdapter::from_render_frame_report(
            &engine.render_frame(&report.frame).unwrap()
        ),
        SessionRuntimeObservation::FrameRendered { frame_serial: 94 }
    );
}

#[test]
fn live_renderer_runtime_adapter_projects_committed_state_before_frame_planning() {
    let engine = HeadlessEngine::default();
    let output = engine.output();
    let mut last_committed = LastCommittedLayout::default();
    let template = test_layer(1, 0, 0, Region::empty());
    let committed = CommittedSurfaceState {
        surface: template.surface,
        committed_generation: 3,
        geometry: Rect {
            x: 200,
            y: 220,
            width: 320,
            height: 240,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(BufferSource::DmaBuf { handle: 701 }, sophia_protocol::Size { width: 320, height: 240 }),
        damage: Region::single(Rect {
            x: 200,
            y: 220,
            width: 320,
            height: 240,
        }),
    };
    let mut renderer = LiveRendererRuntimeAdapter::from_committed_surface_states(
        vec![committed.clone()],
        vec![template],
    );

    let report = renderer
        .render_frame(&engine, output.id, 95, &mut last_committed)
        .unwrap();

    assert_eq!(report.frame.layers[0].geometry, committed.geometry);
    assert_eq!(
        report.frame.layers[0].source,
        BufferSource::DmaBuf { handle: 701 }
    );
    assert_eq!(report.frame.commands[0].target.rects[0], committed.geometry);
}

#[test]
fn live_chrome_runtime_adapter_counts_metadata_updates() {
    let updates = [
        MetadataChromeUpdate::Upserted {
            surface: SurfaceId::new(1, 1),
        },
        MetadataChromeUpdate::Removed {
            surface: SurfaceId::new(1, 2),
        },
        MetadataChromeUpdate::Rejected(MetadataChromeRejectReason::InvalidLabel),
    ];

    let chrome = LiveChromeRuntimeAdapter::from_metadata_updates(&updates);

    assert_eq!(
        chrome.present_observation(),
        SessionRuntimeObservation::ChromeCommandsReady { count: 2 }
    );
}
#[test]
fn live_runtime_driver_preserves_oversized_commit_and_retirement_bursts() {
    for count in [63, 64, 65, 193] {
        let engine = HeadlessEngine::default();
        let output = engine.output();
        let mut driver = HeadlessSessionDriver::new(engine);
        let mut adapter = LiveRuntimeDriverAdapter::from_intake(LiveRuntimeDriverIntake {
            x_event_count: 1,
            authority_commits: (0..count)
                .map(|index| TransactionCommit {
                    transaction: TransactionId::from_raw(index + 1),
                    outcome: TransactionOutcome::Committed,
                    applied_surfaces: vec![SurfaceId::new(7, 1)],
                })
                .collect(),
            authority_batches: Vec::new(),
            wm_update: None,
            portal_commands: Vec::new(),
            chrome_command_count: 0,
            layers: vec![test_layer(7, 0, 0, Region::empty())],
            committed_surfaces: Vec::new(),
            scanout_submit_state: Some(RuntimeScanoutState::Submitted),
            scanout_lifecycle_states: vec![RuntimeScanoutState::Retired; count as usize],
        });
        let report = driver.run_with_adapter(output.id, 94, &mut adapter).unwrap();
        assert_eq!(report.runtime_state.authority_transactions_committed, count);
        assert_eq!(report.runtime_state.authority_surfaces_applied, count);
        assert_eq!(report.runtime_state.scanout_retirements, count);
        assert_eq!(report.runtime_state.x_events_polled, 1);
        assert_eq!(report.runtime_state.frames_rendered, 1);
        assert_eq!(report.runtime_state.scanout_submissions, 1);
        assert_eq!(report.runtime_state.in_flight_scanouts, 1);
        assert_eq!(report.runtime_state.phase, SessionRuntimePhase::Idle);
    }
}
