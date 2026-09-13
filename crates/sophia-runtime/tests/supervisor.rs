use std::time::Duration;

use sophia_protocol::{
    BrokerHealthState, BrokerKind, SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN, TransactionOutcome,
};
use sophia_runtime::{
    MAX_SESSION_RUNTIME_OBSERVATION_BATCH, ProcessLaunchSpec, ProcessSupervisor,
    ProcessSupervisorError, ProtectionDomainRole, ProtectionDomainSpec,
    ProtectionFilesystemManifest, ProtectionPath, RestartPolicy, RuntimeAuthorityHealth,
    RuntimeAuthoritySupervisor, RuntimeBrokerHealth, RuntimeBrokerSupervisors, RuntimeScanoutState,
    SessionRuntimeCommand, SessionRuntimeEvent, SessionRuntimeEventBatch, SessionRuntimeLoop,
    SessionRuntimeObservation, SessionRuntimeObservationError, SessionRuntimePhase,
    SessionRuntimeState, SupervisedProcessKind, SupervisorCommand, SupervisorEvent,
    SupervisorState, update_session_runtime, update_supervisor,
};

#[path = "supervisor/supervisor_process.rs"]
mod supervisor_process;

#[test]
fn session_runtime_reducer_runs_one_continuous_tick() {
    let state = SessionRuntimeState::default();

    let (state, command) = update_session_runtime(state, SessionRuntimeEvent::TickStarted);
    assert_eq!(state.phase, SessionRuntimePhase::PollingX);
    assert_eq!(command, SessionRuntimeCommand::PollXEvents);

    let (state, command) =
        update_session_runtime(state, SessionRuntimeEvent::XEventsPolled { count: 3 });
    assert_eq!(state.x_events_polled, 3);
    assert_eq!(state.phase, SessionRuntimePhase::ApplyingWmPolicy);
    assert_eq!(command, SessionRuntimeCommand::RequestWmLayout);

    let (state, command) = update_session_runtime(state, SessionRuntimeEvent::WmLayoutReady);
    assert_eq!(state.phase, SessionRuntimePhase::WaitingForFrame);
    assert_eq!(command, SessionRuntimeCommand::ScheduleFrame);

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::FrameScheduled { frame_serial: 9 },
    );
    assert_eq!(state.phase, SessionRuntimePhase::Rendering);
    assert_eq!(
        command,
        SessionRuntimeCommand::RenderFrame { frame_serial: 9 }
    );

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::FrameRendered { frame_serial: 9 },
    );
    assert_eq!(state.frames_rendered, 1);
    assert_eq!(state.last_frame_serial, Some(9));
    assert_eq!(state.phase, SessionRuntimePhase::SubmittingScanout);
    assert_eq!(
        command,
        SessionRuntimeCommand::SubmitScanout { frame_serial: 9 }
    );

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::ScanoutStateChanged {
            state: RuntimeScanoutState::Submitted,
            frame_serial: Some(9),
        },
    );
    assert_eq!(state.scanout_submissions, 1);
    assert_eq!(state.in_flight_scanouts, 1);
    assert_eq!(state.last_scanout_frame_serial, Some(9));
    assert_eq!(
        state.last_scanout_state,
        Some(RuntimeScanoutState::Submitted)
    );
    assert_eq!(state.phase, SessionRuntimePhase::DrainingPortals);
    assert_eq!(command, SessionRuntimeCommand::DrainPortalCommands);

    let (state, command) =
        update_session_runtime(state, SessionRuntimeEvent::PortalCommandsReady { count: 2 });
    assert_eq!(state.portal_commands_drained, 2);
    assert_eq!(state.phase, SessionRuntimePhase::PresentingChrome);
    assert_eq!(command, SessionRuntimeCommand::PresentChrome);

    let (state, command) =
        update_session_runtime(state, SessionRuntimeEvent::ChromeCommandsReady { count: 1 });
    assert_eq!(state.chrome_commands_presented, 1);
    assert_eq!(state.phase, SessionRuntimePhase::Idle);
    assert_eq!(command, SessionRuntimeCommand::None);
}

#[test]
fn session_runtime_reducer_skips_wm_policy_when_no_x_events_arrive() {
    let state = SessionRuntimeState::default();
    let (state, _command) = update_session_runtime(state, SessionRuntimeEvent::TickStarted);

    let (state, command) =
        update_session_runtime(state, SessionRuntimeEvent::XEventsPolled { count: 0 });

    assert_eq!(state.x_events_polled, 0);
    assert_eq!(state.phase, SessionRuntimePhase::WaitingForFrame);
    assert_eq!(command, SessionRuntimeCommand::ScheduleFrame);
}

#[test]
fn session_runtime_reducer_tracks_scanout_retirement_and_rejection() {
    let state = SessionRuntimeState::default();

    let (state, _command) = update_session_runtime(
        state,
        SessionRuntimeEvent::ScanoutStateChanged {
            state: RuntimeScanoutState::Submitted,
            frame_serial: Some(12),
        },
    );
    assert_eq!(state.scanout_submissions, 1);
    assert_eq!(state.in_flight_scanouts, 1);

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::ScanoutStateChanged {
            state: RuntimeScanoutState::Deferred,
            frame_serial: Some(13),
        },
    );
    assert_eq!(state.scanout_submissions, 1);
    assert_eq!(state.scanout_retirements, 0);
    assert_eq!(state.scanout_rejections, 0);
    assert_eq!(state.in_flight_scanouts, 1);
    assert_eq!(
        state.last_scanout_state,
        Some(RuntimeScanoutState::Deferred)
    );
    assert_eq!(command, SessionRuntimeCommand::None);

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::ScanoutStateChanged {
            state: RuntimeScanoutState::Retired,
            frame_serial: Some(12),
        },
    );
    assert_eq!(state.scanout_retirements, 1);
    assert_eq!(state.in_flight_scanouts, 0);
    assert_eq!(state.last_scanout_state, Some(RuntimeScanoutState::Retired));
    assert_eq!(state.phase, SessionRuntimePhase::Idle);
    assert_eq!(command, SessionRuntimeCommand::None);

    let (state, _command) = update_session_runtime(
        state,
        SessionRuntimeEvent::ScanoutStateChanged {
            state: RuntimeScanoutState::Submitted,
            frame_serial: Some(13),
        },
    );
    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::ScanoutStateChanged {
            state: RuntimeScanoutState::Rejected,
            frame_serial: Some(13),
        },
    );
    assert_eq!(state.scanout_rejections, 1);
    assert_eq!(state.in_flight_scanouts, 0);
    assert_eq!(
        state.last_scanout_state,
        Some(RuntimeScanoutState::Rejected)
    );
    assert_eq!(state.phase, SessionRuntimePhase::Idle);
    assert_eq!(command, SessionRuntimeCommand::None);
}

#[test]
fn session_runtime_reducer_scanout_submit_response_continues_the_render_pipeline() {
    let state = SessionRuntimeState {
        phase: SessionRuntimePhase::SubmittingScanout,
        ..SessionRuntimeState::default()
    };

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::ScanoutStateChanged {
            state: RuntimeScanoutState::Deferred,
            frame_serial: Some(21),
        },
    );

    assert_eq!(state.scanout_submissions, 0);
    assert_eq!(state.scanout_retirements, 0);
    assert_eq!(state.scanout_rejections, 0);
    assert_eq!(state.in_flight_scanouts, 0);
    assert_eq!(
        state.last_scanout_state,
        Some(RuntimeScanoutState::Deferred)
    );
    assert_eq!(state.phase, SessionRuntimePhase::DrainingPortals);
    assert_eq!(command, SessionRuntimeCommand::DrainPortalCommands);
}

#[test]
fn session_runtime_reducer_requests_wm_restart_without_rendering() {
    let state = SessionRuntimeState::default();

    let (state, command) = update_session_runtime(state, SessionRuntimeEvent::WmRestartRequested);

    assert_eq!(state.wm_restart_requests, 1);
    assert_eq!(state.frames_rendered, 0);
    assert_eq!(state.phase, SessionRuntimePhase::ApplyingWmPolicy);
    assert_eq!(command, SessionRuntimeCommand::RestartWindowManager);
}

#[test]
fn session_runtime_records_broker_health_without_status_payload() {
    let state = SessionRuntimeState::default();

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::BrokerHealthChanged {
            broker: BrokerKind::Portal,
            state: BrokerHealthState::Ready,
            generation: 7,
            status_message_len: 17,
        },
    );

    assert_eq!(command, SessionRuntimeCommand::None);
    assert_eq!(
        state.portal_broker_health,
        Some(RuntimeBrokerHealth {
            state: BrokerHealthState::Ready,
            generation: 7,
            status_message_len: 17,
        })
    );
    assert_eq!(state.metadata_broker_health, None);

    let (state, _command) = update_session_runtime(
        state,
        SessionRuntimeEvent::BrokerHealthChanged {
            broker: BrokerKind::Portal,
            state: BrokerHealthState::Stopped,
            generation: 6,
            status_message_len: 0,
        },
    );

    assert_eq!(
        state.portal_broker_health,
        Some(RuntimeBrokerHealth {
            state: BrokerHealthState::Ready,
            generation: 7,
            status_message_len: 17,
        })
    );

    let (state, _command) = update_session_runtime(
        state,
        SessionRuntimeEvent::BrokerHealthChanged {
            broker: BrokerKind::Metadata,
            state: BrokerHealthState::Degraded,
            generation: 2,
            status_message_len: 8,
        },
    );

    assert_eq!(
        state.metadata_broker_health,
        Some(RuntimeBrokerHealth {
            state: BrokerHealthState::Degraded,
            generation: 2,
            status_message_len: 8,
        })
    );
}

#[test]
fn session_runtime_records_authority_health_without_resource_identity() {
    let state = SessionRuntimeState::default();

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::AuthorityProcessHealthChanged {
            process: SupervisedProcessKind::SophiaXAuthority,
            state: BrokerHealthState::Ready,
            generation: 11,
            status_message_len: 9,
        },
    );

    assert_eq!(command, SessionRuntimeCommand::None);
    assert_eq!(
        state.x_authority_health,
        Some(RuntimeAuthorityHealth {
            process: SupervisedProcessKind::SophiaXAuthority,
            state: BrokerHealthState::Ready,
            generation: 11,
            status_message_len: 9,
        })
    );

    let (state, _command) = update_session_runtime(
        state,
        SessionRuntimeEvent::AuthorityProcessHealthChanged {
            process: SupervisedProcessKind::SophiaXAuthority,
            state: BrokerHealthState::Stopped,
            generation: 10,
            status_message_len: 0,
        },
    );

    assert_eq!(
        state.x_authority_health,
        Some(RuntimeAuthorityHealth {
            process: SupervisedProcessKind::SophiaXAuthority,
            state: BrokerHealthState::Ready,
            generation: 11,
            status_message_len: 9,
        })
    );
}

#[test]
fn authority_health_observation_rejects_unbounded_status_lengths() {
    assert_eq!(
        SessionRuntimeEventBatch::from_observations([
            SessionRuntimeObservation::AuthorityProcessHealthChanged {
                process: SupervisedProcessKind::SophiaXAuthority,
                state: BrokerHealthState::Degraded,
                generation: 12,
                status_message_len: 1025,
            }
        ]),
        Err(SessionRuntimeObservationError::BrokerStatusMessageTooLong {
            len: 1025,
            max: SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN,
        })
    );
}

#[test]
fn session_runtime_records_authority_transaction_outcomes_without_ids() {
    let state = SessionRuntimeState::default();

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::AuthorityTransactionObserved {
            outcome: TransactionOutcome::Committed,
            applied_surface_count: 2,
        },
    );

    assert_eq!(command, SessionRuntimeCommand::None);
    assert_eq!(state.authority_transactions_committed, 1);
    assert_eq!(state.authority_transactions_rejected, 0);
    assert_eq!(state.authority_transactions_timed_out, 0);
    assert_eq!(state.authority_surfaces_applied, 2);

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::AuthorityTransactionObserved {
            outcome: TransactionOutcome::RejectedInvalidSurface,
            applied_surface_count: 0,
        },
    );

    assert_eq!(command, SessionRuntimeCommand::None);
    assert_eq!(state.authority_transactions_committed, 1);
    assert_eq!(state.authority_transactions_rejected, 1);
    assert_eq!(state.authority_transactions_timed_out, 0);
    assert_eq!(state.authority_surfaces_applied, 2);

    let (state, command) = update_session_runtime(
        state,
        SessionRuntimeEvent::AuthorityTransactionObserved {
            outcome: TransactionOutcome::TimedOut,
            applied_surface_count: 0,
        },
    );

    assert_eq!(command, SessionRuntimeCommand::None);
    assert_eq!(state.authority_transactions_committed, 1);
    assert_eq!(state.authority_transactions_rejected, 1);
    assert_eq!(state.authority_transactions_timed_out, 1);
    assert_eq!(state.authority_surfaces_applied, 2);
}

#[test]
fn authority_transaction_observation_roundtrips_through_batch_loop() {
    let mut runtime = SessionRuntimeLoop::default();

    let report = runtime
        .step_observations([
            SessionRuntimeObservation::AuthorityTransactionObserved {
                outcome: TransactionOutcome::Committed,
                applied_surface_count: 3,
            },
            SessionRuntimeObservation::AuthorityTransactionObserved {
                outcome: TransactionOutcome::RejectedStaleSurface,
                applied_surface_count: 0,
            },
        ])
        .expect("authority transaction observations should be accepted");

    assert_eq!(report.events_processed, 2);
    assert!(report.commands.is_empty());
    assert_eq!(runtime.state().authority_transactions_committed, 1);
    assert_eq!(runtime.state().authority_transactions_rejected, 1);
    assert_eq!(runtime.state().authority_transactions_timed_out, 0);
    assert_eq!(runtime.state().authority_surfaces_applied, 3);
}

#[test]
fn slow_client_visual_observation_records_only_aggregate_counts() {
    let mut runtime = SessionRuntimeLoop::default();

    let report = runtime
        .step_observations([
            SessionRuntimeObservation::SlowClientVisualDecisionsObserved {
                timeout_count: 3,
                preserved_count: 2,
                degraded_count: 1,
            },
            SessionRuntimeObservation::SlowClientVisualDecisionsObserved {
                timeout_count: 1,
                preserved_count: 1,
                degraded_count: 0,
            },
        ])
        .expect("slow-client observations should be accepted");

    assert_eq!(report.events_processed, 2);
    assert!(report.commands.is_empty());
    assert_eq!(runtime.state().slow_client_timeouts, 4);
    assert_eq!(runtime.state().slow_client_preserved, 3);
    assert_eq!(runtime.state().slow_client_degraded, 1);
}

#[test]
fn session_runtime_loop_processes_event_batches_without_side_effects() {
    let mut runtime = SessionRuntimeLoop::default();

    let report = runtime.step([
        SessionRuntimeEvent::TickStarted,
        SessionRuntimeEvent::XEventsPolled { count: 4 },
        SessionRuntimeEvent::WmLayoutReady,
        SessionRuntimeEvent::FrameScheduled { frame_serial: 11 },
        SessionRuntimeEvent::FrameRendered { frame_serial: 11 },
        SessionRuntimeEvent::ScanoutStateChanged {
            state: RuntimeScanoutState::Submitted,
            frame_serial: Some(11),
        },
        SessionRuntimeEvent::PortalCommandsReady { count: 2 },
        SessionRuntimeEvent::ChromeCommandsReady { count: 1 },
    ]);

    assert_eq!(report.events_processed, 8);
    assert_eq!(
        report.commands,
        vec![
            SessionRuntimeCommand::PollXEvents,
            SessionRuntimeCommand::RequestWmLayout,
            SessionRuntimeCommand::ScheduleFrame,
            SessionRuntimeCommand::RenderFrame { frame_serial: 11 },
            SessionRuntimeCommand::SubmitScanout { frame_serial: 11 },
            SessionRuntimeCommand::DrainPortalCommands,
            SessionRuntimeCommand::PresentChrome,
        ]
    );
    assert_eq!(runtime.state().phase, SessionRuntimePhase::Idle);
    assert_eq!(runtime.state().x_events_polled, 4);
    assert_eq!(runtime.state().frames_rendered, 1);
    assert_eq!(runtime.state().last_frame_serial, Some(11));
    assert_eq!(runtime.state().scanout_submissions, 1);
    assert_eq!(runtime.state().in_flight_scanouts, 1);
    assert_eq!(runtime.state().portal_commands_drained, 2);
    assert_eq!(runtime.state().chrome_commands_presented, 1);
}

#[test]
fn session_runtime_loop_resumes_from_previous_state() {
    let mut runtime = SessionRuntimeLoop::new(SessionRuntimeState::default());

    let first = runtime.step([
        SessionRuntimeEvent::TickStarted,
        SessionRuntimeEvent::XEventsPolled { count: 0 },
    ]);

    assert_eq!(
        first.commands,
        vec![
            SessionRuntimeCommand::PollXEvents,
            SessionRuntimeCommand::ScheduleFrame,
        ]
    );
    assert_eq!(runtime.state().phase, SessionRuntimePhase::WaitingForFrame);

    let second = runtime.step([
        SessionRuntimeEvent::FrameScheduled { frame_serial: 2 },
        SessionRuntimeEvent::FrameRendered { frame_serial: 2 },
        SessionRuntimeEvent::ScanoutStateChanged {
            state: RuntimeScanoutState::Submitted,
            frame_serial: Some(2),
        },
        SessionRuntimeEvent::PortalCommandsReady { count: 0 },
        SessionRuntimeEvent::ChromeCommandsReady { count: 0 },
    ]);

    assert_eq!(second.events_processed, 5);
    assert_eq!(
        second.commands,
        vec![
            SessionRuntimeCommand::RenderFrame { frame_serial: 2 },
            SessionRuntimeCommand::SubmitScanout { frame_serial: 2 },
            SessionRuntimeCommand::DrainPortalCommands,
            SessionRuntimeCommand::PresentChrome,
        ]
    );
    assert_eq!(runtime.into_state().phase, SessionRuntimePhase::Idle);
}

#[test]
fn session_runtime_observations_feed_the_batch_loop() {
    let mut runtime = SessionRuntimeLoop::default();

    let report = runtime
        .step_observations([
            SessionRuntimeObservation::TickStarted,
            SessionRuntimeObservation::XEventsPolled { count: 1 },
            SessionRuntimeObservation::WmLayoutReady,
            SessionRuntimeObservation::FrameScheduled { frame_serial: 15 },
            SessionRuntimeObservation::FrameRendered { frame_serial: 15 },
            SessionRuntimeObservation::ScanoutStateChanged {
                state: RuntimeScanoutState::Submitted,
                frame_serial: Some(15),
            },
            SessionRuntimeObservation::PortalCommandsReady { count: 3 },
            SessionRuntimeObservation::ChromeCommandsReady { count: 2 },
        ])
        .expect("observation batch should be accepted");

    assert_eq!(report.events_processed, 8);
    assert_eq!(
        report.commands,
        vec![
            SessionRuntimeCommand::PollXEvents,
            SessionRuntimeCommand::RequestWmLayout,
            SessionRuntimeCommand::ScheduleFrame,
            SessionRuntimeCommand::RenderFrame { frame_serial: 15 },
            SessionRuntimeCommand::SubmitScanout { frame_serial: 15 },
            SessionRuntimeCommand::DrainPortalCommands,
            SessionRuntimeCommand::PresentChrome,
        ]
    );
    assert_eq!(runtime.state().phase, SessionRuntimePhase::Idle);
    assert_eq!(runtime.state().x_events_polled, 1);
    assert_eq!(runtime.state().frames_rendered, 1);
    assert_eq!(runtime.state().scanout_submissions, 1);
    assert_eq!(runtime.state().portal_commands_drained, 3);
    assert_eq!(runtime.state().chrome_commands_presented, 2);
}

#[test]
fn session_runtime_observations_route_broker_health_without_payload_bytes() {
    let mut runtime = SessionRuntimeLoop::default();

    let report = runtime
        .step_observations([SessionRuntimeObservation::BrokerHealthChanged {
            broker: BrokerKind::Metadata,
            state: BrokerHealthState::Ready,
            generation: 9,
            status_message_len: 12,
        }])
        .expect("broker health observation should be accepted");

    assert_eq!(report.events_processed, 1);
    assert!(report.commands.is_empty());
    assert_eq!(
        runtime.state().metadata_broker_health,
        Some(RuntimeBrokerHealth {
            state: BrokerHealthState::Ready,
            generation: 9,
            status_message_len: 12,
        })
    );
}

#[test]
fn session_runtime_event_batch_rejects_unbounded_observations() {
    let observations =
        vec![SessionRuntimeObservation::TickCompleted; MAX_SESSION_RUNTIME_OBSERVATION_BATCH + 1];

    let error = SessionRuntimeEventBatch::from_observations(observations)
        .expect_err("oversized observation batch should be rejected");

    assert_eq!(
        error,
        SessionRuntimeObservationError::TooManyObservations {
            max: MAX_SESSION_RUNTIME_OBSERVATION_BATCH
        }
    );
}

#[test]
fn session_runtime_event_batch_rejects_oversized_broker_status_lengths() {
    let error = SessionRuntimeEventBatch::from_observations([
        SessionRuntimeObservation::BrokerHealthChanged {
            broker: BrokerKind::Portal,
            state: BrokerHealthState::Degraded,
            generation: 2,
            status_message_len: sophia_protocol::SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN + 1,
        },
    ])
    .expect_err("oversized broker status length should be rejected");

    assert_eq!(
        error,
        SessionRuntimeObservationError::BrokerStatusMessageTooLong {
            len: sophia_protocol::SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN + 1,
            max: sophia_protocol::SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN,
        }
    );
}
