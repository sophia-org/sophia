use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant};

use sophia_protocol::{
    MetadataDisclosure, MetadataDisclosureRule, SurfaceId, TransactionId, TrustLevel,
};
use sophia_session::session_control::{
    SESSION_CONTROL_ACKNOWLEDGEMENT_TIMEOUT, SESSION_CONTROL_CAPACITY,
    SESSION_CONTROL_QUEUE_TIMEOUT, SessionControlFailure, SessionControlMetrics,
    SessionControlQueue,
};
use sophia_x_authority::{
    XAuthorityClientControlAck, XAuthorityClientControlCommand, XAuthorityControlAck,
    XAuthorityControlCommand, XAuthorityControlKind, XAuthorityControlOutcome,
    XServerFrontendClientId,
};

fn surface(index: u32) -> SurfaceId {
    SurfaceId::new(index, 1)
}

fn control(
    client: u64,
    transaction: u64,
    surface: SurfaceId,
    kind: XAuthorityControlKind,
) -> XAuthorityClientControlCommand {
    let transaction = TransactionId::from_raw(transaction);
    let command = match kind {
        XAuthorityControlKind::PublishMetadataRule => {
            XAuthorityControlCommand::PublishMetadataRule {
                transaction,
                surface,
                rule: MetadataDisclosureRule {
                    surface,
                    disclosure: MetadataDisclosure::ClassOnly,
                    trust_level: TrustLevel::Unknown,
                    icon: None,
                    generation: 1,
                },
            }
        }
        XAuthorityControlKind::AdmitSurface => XAuthorityControlCommand::AdmitSurface {
            transaction,
            surface,
            geometry: sophia_protocol::Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
        },
        XAuthorityControlKind::ConfigureSurface => XAuthorityControlCommand::ConfigureSurface {
            transaction,
            surface,
            geometry: sophia_protocol::Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
        },
        XAuthorityControlKind::SetPresentationState => {
            XAuthorityControlCommand::SetPresentationState {
                transaction,
                surface,
                state: sophia_protocol::PolicyPresentationState::default(),
            }
        }
        XAuthorityControlKind::RestorePresentationState => {
            XAuthorityControlCommand::RestorePresentationState {
                transaction,
                surface,
                state: sophia_protocol::PolicyPresentationState::default(),
            }
        }
        XAuthorityControlKind::FocusSurface => XAuthorityControlCommand::FocusSurface {
            transaction,
            surface,
        },
        XAuthorityControlKind::ClearFocus => XAuthorityControlCommand::ClearFocus {
            transaction,
            surface,
        },
        XAuthorityControlKind::CloseSurface => XAuthorityControlCommand::CloseSurface {
            transaction,
            surface,
        },
        XAuthorityControlKind::WithdrawSurface => XAuthorityControlCommand::WithdrawSurface {
            transaction,
            surface,
        },
    };
    XAuthorityClientControlCommand {
        client: XServerFrontendClientId::from_raw(client),
        command,
    }
}

fn acknowledgement(command: XAuthorityClientControlCommand) -> XAuthorityClientControlAck {
    XAuthorityClientControlAck {
        client: command.client,
        acknowledgement: XAuthorityControlAck {
            kind: command.command.kind(),
            transaction: command.command.transaction(),
            surface: command.command.surface(),
            outcome: XAuthorityControlOutcome::Delivered,
        },
    }
}

#[test]
fn configure_and_close_controls_can_be_in_flight_together() {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let mut queue = SessionControlQueue::default();
    queue
        .enqueue(
            control(1, 1, surface(1), XAuthorityControlKind::ConfigureSurface),
            now,
        )
        .unwrap();
    queue
        .enqueue(
            control(1, 2, surface(1), XAuthorityControlKind::CloseSurface),
            now,
        )
        .unwrap();
    let mut completions = Vec::new();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    let first = commands.try_recv().unwrap();
    let second = commands.try_recv().unwrap();
    acknowledgements.send(acknowledgement(second)).unwrap();
    acknowledgements.send(acknowledgement(first)).unwrap();
    queue
        .service(
            &sender,
            &receiver,
            now + Duration::from_millis(2),
            &mut completions,
        )
        .unwrap();
    assert_eq!(completions.len(), 2);
    assert_eq!(queue.pending_len(), 0);
    assert_eq!(queue.metrics().delivered, 2);
}

#[test]
fn only_configure_controls_may_yield_priority_to_native_frames() {
    let now = Instant::now();
    let mut queue = SessionControlQueue::default();
    queue
        .enqueue(
            control(1, 1, surface(1), XAuthorityControlKind::ConfigureSurface),
            now,
        )
        .unwrap();
    assert!(!queue.has_non_configure_pending());

    queue
        .enqueue(
            control(1, 2, surface(1), XAuthorityControlKind::FocusSurface),
            now,
        )
        .unwrap();
    assert!(queue.has_non_configure_pending());
}

#[test]
fn focus_controls_are_globally_serialized() {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let mut queue = SessionControlQueue::default();
    let first = control(1, 1, surface(1), XAuthorityControlKind::FocusSurface);
    let second = control(2, 2, surface(2), XAuthorityControlKind::ClearFocus);
    queue.enqueue(first, now).unwrap();
    queue.enqueue(second, now).unwrap();
    let mut completions = Vec::new();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    assert_eq!(commands.try_recv().unwrap(), first);
    assert!(commands.try_recv().is_err());
    acknowledgements.send(acknowledgement(first)).unwrap();
    queue
        .service(
            &sender,
            &receiver,
            now + Duration::from_millis(1),
            &mut completions,
        )
        .unwrap();
    assert_eq!(commands.try_recv().unwrap(), second);
}

#[test]
fn acknowledgement_kind_is_part_of_exact_correlation() {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let mut queue = SessionControlQueue::default();
    let command = control(1, 7, surface(1), XAuthorityControlKind::FocusSurface);
    queue.enqueue(command, now).unwrap();
    let mut completions = Vec::new();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    let _ = commands.recv().unwrap();
    let mut wrong = acknowledgement(command);
    wrong.acknowledgement.kind = XAuthorityControlKind::ClearFocus;
    acknowledgements.send(wrong).unwrap();
    assert_eq!(
        queue
            .service(&sender, &receiver, now, &mut completions)
            .unwrap_err(),
        SessionControlFailure::UnexpectedAcknowledgement
    );
}

#[test]
fn undispatched_controls_expire_at_the_total_deadline() {
    let (sender, _commands) = sync_channel(0);
    let (_acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let mut queue = SessionControlQueue::default();
    queue
        .enqueue(
            control(1, 1, surface(1), XAuthorityControlKind::CloseSurface),
            now,
        )
        .unwrap();
    let mut completions = Vec::new();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    queue
        .service(
            &sender,
            &receiver,
            now + SESSION_CONTROL_QUEUE_TIMEOUT,
            &mut completions,
        )
        .unwrap();
    assert_eq!(
        completions[0].failure,
        Some(SessionControlFailure::TimedOut)
    );
    assert_eq!(queue.metrics().timed_out, 1);
}

#[test]
fn dispatched_control_gets_an_independent_acknowledgement_deadline() {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let command = control(1, 1, surface(1), XAuthorityControlKind::FocusSurface);
    let mut queue = SessionControlQueue::default();
    queue.enqueue(command, now).unwrap();
    let mut completions = Vec::new();

    let dispatched_at = now + SESSION_CONTROL_QUEUE_TIMEOUT - Duration::from_millis(1);
    queue
        .service(&sender, &receiver, dispatched_at, &mut completions)
        .unwrap();
    assert_eq!(commands.recv().unwrap(), command);
    queue
        .service(
            &sender,
            &receiver,
            dispatched_at + SESSION_CONTROL_ACKNOWLEDGEMENT_TIMEOUT - Duration::from_millis(1),
            &mut completions,
        )
        .unwrap();
    assert!(completions.is_empty());

    acknowledgements.send(acknowledgement(command)).unwrap();
    queue
        .service(
            &sender,
            &receiver,
            dispatched_at + SESSION_CONTROL_ACKNOWLEDGEMENT_TIMEOUT,
            &mut completions,
        )
        .unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].failure, None);
    assert_eq!(queue.metrics().timed_out, 0);
}

#[test]
fn dispatch_prerequisite_does_not_consume_the_queue_deadline() {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let command = control(1, 1, surface(1), XAuthorityControlKind::ClearFocus);
    let mut queue = SessionControlQueue::default();
    queue.enqueue(command, now).unwrap();
    let mut completions = Vec::new();

    let prerequisite_completed_at = now + SESSION_CONTROL_QUEUE_TIMEOUT * 2;
    queue
        .service_when(
            &sender,
            &receiver,
            prerequisite_completed_at,
            &mut completions,
            false,
        )
        .unwrap();
    assert!(completions.is_empty());
    assert!(commands.try_recv().is_err());
    queue
        .service_when(
            &sender,
            &receiver,
            prerequisite_completed_at,
            &mut completions,
            true,
        )
        .unwrap();
    assert_eq!(commands.recv().unwrap(), command);

    acknowledgements.send(acknowledgement(command)).unwrap();
    queue
        .service(
            &sender,
            &receiver,
            prerequisite_completed_at + Duration::from_millis(1),
            &mut completions,
        )
        .unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].failure, None);
    assert!(completions[0].queue_dwell >= SESSION_CONTROL_QUEUE_TIMEOUT * 2);
}

#[test]
fn duplicate_and_over_capacity_controls_fail_closed() {
    let now = Instant::now();
    let mut queue = SessionControlQueue::default();
    let duplicate = control(1, 1, surface(1), XAuthorityControlKind::CloseSurface);
    queue.enqueue(duplicate, now).unwrap();
    assert_eq!(
        queue.enqueue(duplicate, now).unwrap_err(),
        SessionControlFailure::Duplicate
    );
    for index in 1..SESSION_CONTROL_CAPACITY {
        queue
            .enqueue(
                control(
                    1,
                    index as u64 + 1,
                    surface(index as u32 + 1),
                    XAuthorityControlKind::ConfigureSurface,
                ),
                now,
            )
            .unwrap();
    }
    assert_eq!(
        queue
            .enqueue(
                control(1, 99, surface(99), XAuthorityControlKind::CloseSurface),
                now,
            )
            .unwrap_err(),
        SessionControlFailure::Capacity
    );
}

#[test]
fn stale_target_acknowledgements_retire_without_rejection_debt() {
    for kind in [
        XAuthorityControlKind::CloseSurface,
        XAuthorityControlKind::PublishMetadataRule,
        XAuthorityControlKind::ClearFocus,
        XAuthorityControlKind::FocusSurface,
    ] {
        assert_stale_target_retires(kind);
    }
}

fn assert_stale_target_retires(kind: XAuthorityControlKind) {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let command = control(1, 1, surface(1), kind);
    let mut queue = SessionControlQueue::default();
    queue.enqueue(command, now).unwrap();
    let mut completions = Vec::new();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    let _ = commands.recv().unwrap();
    let mut rejected = acknowledgement(command);
    rejected.acknowledgement.outcome = XAuthorityControlOutcome::UnknownSurface;
    acknowledgements.send(rejected).unwrap();
    queue
        .service(
            &sender,
            &receiver,
            now + Duration::from_millis(1),
            &mut completions,
        )
        .unwrap();
    assert_eq!(
        completions[0].failure,
        Some(SessionControlFailure::Rejected(
            XAuthorityControlOutcome::UnknownSurface
        ))
    );
    assert_eq!(queue.metrics().stale_targets_retired, 1);
    assert_eq!(queue.metrics().rejected, 0);
    assert!(
        completions[0]
            .failure
            .is_some_and(|failure| failure.is_stale_target_for(completions[0].key.kind))
    );
    assert!(completions[0].failure.is_some_and(|failure| {
        failure.is_stale_target_for(XAuthorityControlKind::PublishMetadataRule)
    }));
    assert!(completions[0].failure.is_some_and(|failure| {
        !failure.is_stale_target_for(XAuthorityControlKind::ConfigureSurface)
    }));

    let gone = control(1, 2, surface(2), XAuthorityControlKind::FocusSurface);
    queue.enqueue(gone, now).unwrap();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    let _ = commands.recv().unwrap();
    let mut disconnected = acknowledgement(gone);
    disconnected.acknowledgement.outcome = XAuthorityControlOutcome::ClientGone;
    acknowledgements.send(disconnected).unwrap();
    queue
        .service(
            &sender,
            &receiver,
            now + Duration::from_millis(1),
            &mut completions,
        )
        .unwrap();
    assert!(
        completions[1]
            .failure
            .is_some_and(|failure| failure.is_stale_target_for(completions[1].key.kind))
    );
    assert_eq!(queue.metrics().stale_targets_retired, 2);
    assert_eq!(queue.metrics().rejected, 0);
    assert!(queue.metrics().is_drained(queue.pending_len()));
}

#[test]
fn unexpected_target_rejection_remains_terminal_debt() {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let command = control(1, 1, surface(1), XAuthorityControlKind::ConfigureSurface);
    let mut queue = SessionControlQueue::default();
    queue.enqueue(command, now).unwrap();
    let mut completions = Vec::new();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    let _ = commands.recv().unwrap();
    let mut rejected = acknowledgement(command);
    rejected.acknowledgement.outcome = XAuthorityControlOutcome::UnknownSurface;
    acknowledgements.send(rejected).unwrap();
    queue
        .service(
            &sender,
            &receiver,
            now + Duration::from_millis(1),
            &mut completions,
        )
        .unwrap();

    assert_eq!(
        completions[0].failure,
        Some(SessionControlFailure::Rejected(
            XAuthorityControlOutcome::UnknownSurface
        ))
    );
    assert_eq!(queue.metrics().stale_targets_retired, 0);
    assert_eq!(queue.metrics().rejected, 1);
    assert!(!queue.metrics().is_drained(queue.pending_len()));
}

#[test]
fn terminal_accounting_accepts_the_physical_stale_metadata_shape() {
    let metrics = SessionControlMetrics {
        enqueued: 19,
        dispatched: 19,
        delivered: 18,
        stale_targets_retired: 1,
        ..SessionControlMetrics::default()
    };

    assert!(metrics.is_drained(0));
    assert!(
        !SessionControlMetrics {
            stale_targets_retired: 0,
            rejected: 1,
            ..metrics
        }
        .is_drained(0)
    );
}

#[test]
fn dispatch_barrier_holds_control_until_input_release_is_acknowledged() {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (_acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let command = control(1, 1, surface(1), XAuthorityControlKind::CloseSurface);
    let mut queue = SessionControlQueue::default();
    queue.enqueue(command, now).unwrap();
    let mut completions = Vec::new();

    queue
        .service_when(&sender, &receiver, now, &mut completions, false)
        .unwrap();
    assert!(commands.try_recv().is_err());
    assert_eq!(queue.metrics().dispatched, 0);

    queue
        .service_when(
            &sender,
            &receiver,
            now + Duration::from_millis(1),
            &mut completions,
            true,
        )
        .unwrap();
    assert_eq!(commands.try_recv().unwrap(), command);
}

#[test]
fn disconnected_client_controls_settle_once_and_late_acks_cannot_restore_focus() {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let failed = control(1, 1, surface(1), XAuthorityControlKind::FocusSurface);
    let healthy = control(2, 2, surface(2), XAuthorityControlKind::CloseSurface);
    let mut queue = SessionControlQueue::default();
    queue.enqueue(failed, now).unwrap();
    queue.enqueue(healthy, now).unwrap();
    let mut completions = Vec::new();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    assert_eq!(commands.try_iter().count(), 2);
    // The owner calls this only after exact-connection socket shutdown.
    queue.revoke_client(failed.client, now, &mut completions);
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].failure,
        Some(SessionControlFailure::ClientDisconnected)
    );
    completions.clear();
    acknowledgements.send(acknowledgement(failed)).unwrap();
    acknowledgements.send(acknowledgement(healthy)).unwrap();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].key.client, healthy.client);
    assert_eq!(completions[0].failure, None);
    assert!(queue.metrics().is_settled(queue.pending_len()));
    assert!(!queue.metrics().is_drained(queue.pending_len()));
    assert_eq!(queue.metrics().delivered, 1);
    // Repeating a revoked ACK remains inert; repeating a healthy ACK retains
    // the existing invariant failure instead of hiding shared corruption.
    acknowledgements.send(acknowledgement(failed)).unwrap();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    acknowledgements.send(acknowledgement(healthy)).unwrap();
    assert_eq!(
        queue.service(&sender, &receiver, now, &mut completions),
        Err(SessionControlFailure::UnexpectedAcknowledgement)
    );
}

/// After a successful primary exit the frontend drains and answers nothing
/// (t187): what was pending is retired as `Quiescing`, what is enqueued
/// afterwards completes the same way without failing the enqueue, a late
/// acknowledgement is inert, and the metrics still settle and drain.
#[test]
fn quiescence_retires_pending_and_later_controls_without_failing_the_session() {
    let (sender, commands) = sync_channel(SESSION_CONTROL_CAPACITY);
    let (acknowledgements, receiver) = sync_channel(SESSION_CONTROL_CAPACITY);
    let now = Instant::now();
    let in_flight = control(1, 1, surface(1), XAuthorityControlKind::FocusSurface);
    let waiting = control(1, 2, surface(1), XAuthorityControlKind::ClearFocus);
    let mut queue = SessionControlQueue::default();
    queue.enqueue(in_flight, now).unwrap();
    queue.enqueue(waiting, now).unwrap();
    let mut completions = Vec::new();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    // One focus control in flight; the second focus-class control waits.
    assert_eq!(commands.try_iter().count(), 1);
    assert!(completions.is_empty());

    queue.begin_quiescence(now);
    assert!(queue.is_quiescing());
    assert_eq!(queue.pending_len(), 0);
    // Enqueued after quiescence began: accepted, retired at once.
    let late = control(2, 3, surface(2), XAuthorityControlKind::FocusSurface);
    queue.enqueue(late, now).unwrap();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    assert_eq!(
        completions
            .iter()
            .map(|completion| (completion.key.transaction.raw(), completion.failure))
            .collect::<Vec<_>>(),
        vec![
            (1, Some(SessionControlFailure::Quiescing)),
            (2, Some(SessionControlFailure::Quiescing)),
            (3, Some(SessionControlFailure::Quiescing)),
        ]
    );
    assert_eq!(
        commands.try_iter().count(),
        0,
        "nothing is dispatched while quiescing"
    );
    // The frontend answers the in-flight control on its way out: inert.
    completions.clear();
    acknowledgements.send(acknowledgement(in_flight)).unwrap();
    queue
        .service(&sender, &receiver, now, &mut completions)
        .unwrap();
    assert!(completions.is_empty());
    let metrics = queue.metrics();
    assert_eq!(metrics.unexpected, 0);
    assert_eq!(metrics.timed_out, 0);
    assert_eq!(metrics.quiesced_in_flight, 1);
    assert_eq!(metrics.quiesced_before_dispatch, 2);
    assert!(metrics.is_settled(queue.pending_len()));
    assert!(metrics.is_drained(queue.pending_len()));
}
