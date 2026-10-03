use std::num::NonZeroUsize;
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

use sophia_protocol::{Rect, SurfaceId, TransactionId};
use sophia_session::session_control::{
    SESSION_CONTROL_ACKNOWLEDGEMENT_TIMEOUT, SessionControlFailure, SessionControlQueue,
};
use sophia_x_authority::{
    XAuthorityClientControlAck, XAuthorityClientControlCommand, XAuthorityControlAck,
    XAuthorityControlCommand, XAuthorityControlOutcome, XServerFrontendClientId,
    XServerFrontendRouteBroker,
};

use sophia_session::session_shutdown::{
    SessionLogoutDrainDecision, SessionLogoutDrainState, SessionQuiescence,
    SessionQuiescenceDecision, SessionQuiescenceSnapshot, session_logout_drain_decision,
};

fn drained_logout() -> SessionLogoutDrainState {
    SessionLogoutDrainState {
        requested: true,
        pending_input_deliveries: 0,
        pending_key_release_barriers: 0,
        pending_controls: 0,
        pending_wm_update: false,
    }
}

#[test]
fn logout_waits_for_the_committed_wm_update_to_enter_engine() {
    let mut state = drained_logout();
    state.pending_wm_update = true;
    assert_eq!(
        session_logout_drain_decision(state),
        SessionLogoutDrainDecision::Draining
    );

    state.pending_wm_update = false;
    assert_eq!(
        session_logout_drain_decision(state),
        SessionLogoutDrainDecision::Complete
    );
}

#[test]
fn logout_waits_for_every_delivery_boundary() {
    for state in [
        SessionLogoutDrainState {
            pending_input_deliveries: 1,
            ..drained_logout()
        },
        SessionLogoutDrainState {
            pending_key_release_barriers: 1,
            ..drained_logout()
        },
        SessionLogoutDrainState {
            pending_controls: 1,
            ..drained_logout()
        },
    ] {
        assert_eq!(
            session_logout_drain_decision(state),
            SessionLogoutDrainDecision::Draining
        );
    }
}

#[test]
fn idle_session_does_not_exit_without_logout() {
    assert_eq!(
        session_logout_drain_decision(SessionLogoutDrainState {
            requested: false,
            ..drained_logout()
        }),
        SessionLogoutDrainDecision::Running
    );
}

#[test]
fn session_quiescence_requires_frontend_authority_cpu_and_native_drain() {
    let started = Instant::now();
    let mut quiescence = SessionQuiescence::new("test", started, Duration::from_millis(20));
    let drained = SessionQuiescenceSnapshot::default();

    assert_eq!(
        quiescence.decision(started + Duration::from_millis(1), drained),
        SessionQuiescenceDecision::Pending
    );
    quiescence.mark_frontend_authority_drained();
    assert_eq!(
        quiescence.decision(
            started + Duration::from_millis(2),
            SessionQuiescenceSnapshot {
                pending_authority_batches: 1,
                pending_coordinator_work: 0,
                pending_controls: 0,
                cpu_update_pending: true,
                native_work_pending: true,
            },
        ),
        SessionQuiescenceDecision::Pending
    );
    assert_eq!(
        quiescence.decision(started + Duration::from_millis(20), drained),
        SessionQuiescenceDecision::Complete,
        "settlement at the deadline must beat cancellation"
    );

    let mut blocked = SessionQuiescence::new("test", started, Duration::from_millis(20));
    blocked.mark_frontend_authority_drained();
    assert_eq!(
        blocked.decision(
            started + Duration::from_millis(20),
            SessionQuiescenceSnapshot {
                cpu_update_pending: true,
                ..SessionQuiescenceSnapshot::default()
            },
        ),
        SessionQuiescenceDecision::TimedOut
    );
}

#[test]
fn quiescence_waits_for_each_accepted_work_domain_without_extending_deadline() {
    let now = Instant::now();
    let mut quiescence = SessionQuiescence::new("test", now, Duration::from_millis(20));
    quiescence.mark_frontend_authority_drained();
    for snapshot in [
        SessionQuiescenceSnapshot {
            pending_authority_batches: 1,
            ..Default::default()
        },
        SessionQuiescenceSnapshot {
            pending_coordinator_work: 1,
            ..Default::default()
        },
        SessionQuiescenceSnapshot {
            pending_controls: 1,
            ..Default::default()
        },
        SessionQuiescenceSnapshot {
            cpu_update_pending: true,
            ..Default::default()
        },
        SessionQuiescenceSnapshot {
            native_work_pending: true,
            ..Default::default()
        },
    ] {
        assert_eq!(
            quiescence.decision(now + Duration::from_millis(1), snapshot),
            SessionQuiescenceDecision::Pending
        );
        assert_eq!(
            quiescence.decision(now + Duration::from_millis(20), snapshot),
            SessionQuiescenceDecision::TimedOut
        );
    }
    assert_eq!(
        quiescence.decision(
            now + Duration::from_millis(20),
            SessionQuiescenceSnapshot::default()
        ),
        SessionQuiescenceDecision::Complete
    );
}

fn configure(sequence: u64) -> XAuthorityClientControlCommand {
    XAuthorityClientControlCommand {
        client: XServerFrontendClientId::from_raw(1),
        command: XAuthorityControlCommand::ConfigureSurface {
            transaction: TransactionId::from_raw(sequence),
            surface: SurfaceId::new(7, 1),
            geometry: Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
        },
    }
}

fn acknowledge(
    command: XAuthorityClientControlCommand,
    outcome: XAuthorityControlOutcome,
) -> XAuthorityClientControlAck {
    XAuthorityClientControlAck {
        client: command.client,
        acknowledgement: XAuthorityControlAck {
            transaction: command.command.transaction(),
            surface: command.command.surface(),
            kind: command.command.kind(),
            outcome,
        },
    }
}

fn quiescing(now: Instant) -> SessionQuiescence {
    let mut state = SessionQuiescence::new("runtime_deadline", now, Duration::from_secs(2));
    state.mark_frontend_authority_drained();
    state
}

fn with_controls(queue: &SessionControlQueue) -> SessionQuiescenceSnapshot {
    SessionQuiescenceSnapshot {
        pending_controls: queue.pending_len(),
        ..Default::default()
    }
}

#[test]
fn immediate_departed_client_ack_requires_another_owner_turn_before_shutdown() {
    let now = Instant::now();
    let state = quiescing(now);
    let (acks, receiver) = sync_channel(4);
    let broker =
        XServerFrontendRouteBroker::with_control_ack_sender(NonZeroUsize::new(4).unwrap(), acks);
    let router = broker.control_router();
    let mut queue = SessionControlQueue::default();
    let mut completions = Vec::new();
    queue.enqueue(configure(1), now).unwrap();
    // No client is registered: the real router queues ClientGone while dispatch
    // runs, after this turn has already consumed available acknowledgements.
    queue
        .service(&router, &receiver, now, &mut completions)
        .unwrap();
    assert_eq!(queue.metrics().dispatched, 1);
    assert_eq!(queue.pending_len(), 1);
    assert!(completions.is_empty());
    assert_eq!(
        state.decision(now, with_controls(&queue)),
        SessionQuiescenceDecision::Pending
    );

    queue
        .service(
            &router,
            &receiver,
            now + Duration::from_millis(1),
            &mut completions,
        )
        .unwrap();
    assert_eq!(queue.metrics().stale_targets_retired, 1);
    assert!(queue.metrics().is_drained(queue.pending_len()));
    assert_eq!(
        state.decision(now + Duration::from_millis(1), with_controls(&queue)),
        SessionQuiescenceDecision::Complete
    );
}

#[test]
fn delayed_ack_and_followup_layout_control_each_hold_quiescence_open() {
    let now = Instant::now();
    let state = quiescing(now);
    let (sender, commands) = sync_channel(4);
    let (acks, receiver) = sync_channel(4);
    let mut queue = SessionControlQueue::default();
    let mut completions = Vec::new();
    for sequence in 1..=2 {
        let turn = now + Duration::from_millis(sequence * 10);
        // The second control represents layout progress produced by the first
        // completion, after the acknowledgement service returned to its owner.
        queue.enqueue(configure(sequence), turn).unwrap();
        queue
            .service(&sender, &receiver, turn, &mut completions)
            .unwrap();
        assert_eq!(
            state.decision(turn, with_controls(&queue)),
            SessionQuiescenceDecision::Pending
        );
        let command = commands.try_recv().unwrap();
        queue
            .service(
                &sender,
                &receiver,
                turn + Duration::from_millis(2),
                &mut completions,
            )
            .unwrap();
        assert_eq!(
            state.decision(turn, with_controls(&queue)),
            SessionQuiescenceDecision::Pending
        );
        acks.send(acknowledge(command, XAuthorityControlOutcome::Delivered))
            .unwrap();
        queue
            .service(
                &sender,
                &receiver,
                turn + Duration::from_millis(3),
                &mut completions,
            )
            .unwrap();
    }
    assert_eq!(queue.metrics().delivered, 2);
    assert!(queue.metrics().is_drained(queue.pending_len()));
    assert_eq!(
        state.decision(now + Duration::from_millis(23), with_controls(&queue)),
        SessionQuiescenceDecision::Complete
    );
}

#[test]
fn input_release_barrier_blocks_shutdown_without_extending_quiescence_deadline() {
    let now = Instant::now();
    let state = quiescing(now);
    let (sender, commands) = sync_channel(4);
    let (_acks, receiver) = sync_channel(4);
    let mut queue = SessionControlQueue::default();
    let mut completions = Vec::new();
    queue.enqueue(configure(1), now).unwrap();
    queue
        .service_when(&sender, &receiver, now, &mut completions, false)
        .unwrap();
    assert!(commands.try_recv().is_err());
    assert_eq!(
        state.decision(now, with_controls(&queue)),
        SessionQuiescenceDecision::Pending
    );
    assert_eq!(
        state.decision(now + Duration::from_secs(2), with_controls(&queue)),
        SessionQuiescenceDecision::TimedOut
    );

    // A released input barrier permits dispatch, but does not settle the control.
    queue
        .service_when(
            &sender,
            &receiver,
            now + Duration::from_millis(1),
            &mut completions,
            true,
        )
        .unwrap();
    assert!(commands.try_recv().is_ok());
    assert_eq!(
        state.decision(now + Duration::from_millis(1), with_controls(&queue)),
        SessionQuiescenceDecision::Pending
    );
}

#[test]
fn missing_acknowledgements_and_rejections_remain_failed_terminal_accounting() {
    for outcome in [None, Some(XAuthorityControlOutcome::AuthorityRejected)] {
        let now = Instant::now();
        let (sender, commands) = sync_channel(4);
        let (acks, receiver) = sync_channel(4);
        let mut queue = SessionControlQueue::default();
        let mut completions = Vec::new();
        queue.enqueue(configure(1), now).unwrap();
        queue
            .service(&sender, &receiver, now, &mut completions)
            .unwrap();
        let command = commands.try_recv().unwrap();
        if let Some(outcome) = outcome {
            acks.send(acknowledge(command, outcome)).unwrap();
        }
        queue
            .service(
                &sender,
                &receiver,
                now + SESSION_CONTROL_ACKNOWLEDGEMENT_TIMEOUT,
                &mut completions,
            )
            .unwrap();
        assert_eq!(completions.len(), 1);
        let failure = completions[0].failure.unwrap();
        assert!(!failure.is_stale_target_for(command.command.kind()));
        assert!(!queue.metrics().is_drained(queue.pending_len()));
    }
}

#[test]
fn unexpected_acknowledgement_still_fails_service_during_shutdown() {
    let now = Instant::now();
    let (sender, _commands) = sync_channel(4);
    let (acks, receiver) = sync_channel(4);
    let mut queue = SessionControlQueue::default();
    let mut completions = Vec::new();
    acks.send(acknowledge(
        configure(99),
        XAuthorityControlOutcome::Delivered,
    ))
    .unwrap();
    assert_eq!(
        queue.service(&sender, &receiver, now, &mut completions),
        Err(SessionControlFailure::UnexpectedAcknowledgement)
    );
    assert!(!queue.metrics().is_drained(queue.pending_len()));
}
