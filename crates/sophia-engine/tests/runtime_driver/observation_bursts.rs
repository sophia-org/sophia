struct BurstObservations(Vec<SessionRuntimeObservation>);

impl sophia_engine::RuntimeDriverAdapter for BurstObservations {
    fn poll_x_events(&mut self) -> Result<SessionRuntimeObservation, EngineError> {
        unreachable!("the driver must use the complete observation intake")
    }

    fn poll_x_observations(&mut self) -> Result<Vec<SessionRuntimeObservation>, EngineError> {
        Ok(std::mem::take(&mut self.0))
    }

    fn request_wm_layout(&mut self) -> Result<SessionRuntimeObservation, EngineError> {
        panic!("restart must precede the later layout command")
    }

    fn render_frame(
        &mut self,
        _: &HeadlessEngine,
        _: OutputId,
        _: u64,
        _: &mut LastCommittedLayout,
    ) -> Result<sophia_engine::SessionTickReport, EngineError> {
        panic!("this intake does not authorize rendering")
    }

    fn drain_portal_commands(&mut self) -> Result<SessionRuntimeObservation, EngineError> {
        unreachable!()
    }

    fn present_chrome(&mut self) -> Result<SessionRuntimeObservation, EngineError> {
        unreachable!()
    }
}

#[test]
fn observation_chunks_preserve_command_order_before_effects() {
    let engine = HeadlessEngine::default();
    let output = engine.output();
    let mut driver = HeadlessSessionDriver::new(engine);
    let mut records = vec![SessionRuntimeObservation::WmRestartRequested];
    records.extend(std::iter::repeat_n(
        SessionRuntimeObservation::AuthorityTransactionObserved {
            outcome: TransactionOutcome::Committed,
            applied_surface_count: 1,
        },
        64,
    ));
    records.push(SessionRuntimeObservation::XEventsPolled { count: 1 });
    let report = driver
        .run_with_adapter(output.id, 1, &mut BurstObservations(records))
        .unwrap();
    assert_eq!(report.runtime_state.authority_transactions_committed, 64);
    assert_eq!(report.runtime_state.wm_restart_requests, 1);
    assert_eq!(report.runtime_state.x_events_polled, 1);
    assert_eq!(
        report.runtime_commands,
        vec![
            SessionRuntimeCommand::PollXEvents,
            SessionRuntimeCommand::RestartWindowManager,
            SessionRuntimeCommand::RequestWmLayout,
        ]
    );
    assert!(report.session_tick.is_none());
}

#[test]
fn invalid_late_observation_does_not_partially_reduce_intake() {
    let engine = HeadlessEngine::default();
    let output = engine.output();
    let mut driver = HeadlessSessionDriver::new(engine);
    let mut records = vec![
        SessionRuntimeObservation::AuthorityTransactionObserved {
            outcome: TransactionOutcome::Committed,
            applied_surface_count: 1,
        };
        65
    ];
    records.push(SessionRuntimeObservation::BrokerHealthChanged {
        broker: sophia_protocol::BrokerKind::Portal,
        state: sophia_protocol::BrokerHealthState::Ready,
        generation: 1,
        status_message_len: usize::MAX,
    });
    let error = driver
        .run_with_adapter(output.id, 1, &mut BurstObservations(records))
        .unwrap_err();
    assert!(matches!(
        error,
        EngineError::RuntimeObservation(
            sophia_runtime::SessionRuntimeObservationError::BrokerStatusMessageTooLong { .. }
        )
    ));
    assert_eq!(driver.runtime_state().authority_transactions_committed, 0);
    assert_eq!(driver.runtime_state().authority_surfaces_applied, 0);
    assert_eq!(driver.runtime_state().phase, SessionRuntimePhase::PollingX);
}
