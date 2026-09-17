//! Real channel controls for the production publisher and shutdown entry.
use super::*;
use crate::live_session::shutdown::{
    AuthorityIngressState, begin_frontend_quiescence, observe_authority_ingress,
};
use crate::live_session::{
    SessionQuiescence, SessionQuiescenceDecision, SessionQuiescenceSnapshot,
};
use sophia_x_authority::XWindowAllocationUpdate;

fn update(
    receiver: &Receiver<XServerFrontendServiceCommand>,
) -> SyncSender<XWindowAllocationUpdate> {
    let XServerFrontendServiceCommand::UpdateWindowAllocationPreferences {
        acknowledgement, ..
    } = receiver.recv().unwrap()
    else {
        panic!("expected allocation update")
    };
    acknowledgement
}

#[test]
fn allocation_shutdown_stops_discovery_and_send_after_real_frontend_eof() {
    let now = Instant::now();
    let mut publisher = LiveWindowAllocationPublisher::default();
    let (sender, receiver) = sync_channel(2);
    let mut stopped = false;
    begin_frontend_quiescence(&mut publisher, &sender, &mut stopped).unwrap();
    assert!(publisher.quiescing);
    assert!(matches!(
        receiver.recv().unwrap(),
        XServerFrontendServiceCommand::DrainAndDisconnect
    ));
    drop(receiver);
    for _ in 0..3 {
        publisher
            .poll_snapshot(now, 1, &sender, || panic!("discovery after shutdown"))
            .unwrap();
        begin_frontend_quiescence(&mut publisher, &sender, &mut stopped).unwrap();
    }
    assert_eq!(publisher.generation, 0);
    assert!(publisher.pending.is_none());
    // Frontend EOF does not settle the owner's accepted work.
    let mut quiescence = Some(SessionQuiescence::new(
        "logout_complete",
        now,
        Duration::from_secs(2),
    ));
    observe_authority_ingress(AuthorityIngressState::Disconnected, &mut quiescence, now).unwrap();
    let state = quiescence.unwrap();
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
            cpu_update_pending: true,
            ..Default::default()
        },
        SessionQuiescenceSnapshot {
            native_work_pending: true,
            ..Default::default()
        },
    ] {
        assert_eq!(
            state.decision(now, snapshot),
            SessionQuiescenceDecision::Pending
        );
    }
    assert_eq!(
        state.decision(now, Default::default()),
        SessionQuiescenceDecision::Complete
    );
}

#[test]
fn allocation_shutdown_cancels_pending_ack_without_promoting_or_losing_applied_metadata() {
    for late_ack in [false, true] {
        let now = Instant::now();
        let mut publisher = LiveWindowAllocationPublisher::default();
        let (sender, receiver) = sync_channel(2);
        publisher.poll_snapshot(now, 1, &sender, Vec::new).unwrap();
        update(&receiver)
            .send(XWindowAllocationUpdate::Applied)
            .unwrap();
        publisher
            .poll_snapshot(now, 1, &sender, || panic!("cadence must hold"))
            .unwrap();
        assert_eq!(publisher.applied.as_ref().unwrap().generation, 1);
        publisher
            .poll_snapshot(now + Duration::from_secs(1), 2, &sender, Vec::new)
            .unwrap();
        let ack = update(&receiver);
        let ack = if late_ack {
            ack.send(XWindowAllocationUpdate::Applied).unwrap();
            Some(ack)
        } else {
            drop(ack);
            None
        };

        let mut stopped = false;
        begin_frontend_quiescence(&mut publisher, &sender, &mut stopped).unwrap();
        assert!(matches!(
            receiver.recv().unwrap(),
            XServerFrontendServiceCommand::DrainAndDisconnect
        ));
        drop(receiver);
        if let Some(ack) = ack {
            assert!(ack.send(XWindowAllocationUpdate::Applied).is_err());
        }
        publisher
            .poll_snapshot(now + Duration::from_secs(2), 3, &sender, || {
                panic!("closed publisher queried source")
            })
            .unwrap();
        assert!(publisher.pending.is_none());
        assert_eq!(publisher.generation, 2);
        assert_eq!(publisher.applied.as_ref().unwrap().generation, 1);
        assert_eq!(publisher.applied.as_ref().unwrap().topology_generation, 1);
    }
}

#[test]
fn allocation_shutdown_preserves_active_disconnect_failures_and_failed_stop() {
    let now = Instant::now();
    let mut publisher = LiveWindowAllocationPublisher::default();
    let (sender, receiver) = sync_channel(1);
    drop(receiver);
    assert_eq!(
        publisher
            .poll_snapshot(now, 1, &sender, Vec::new)
            .unwrap_err()
            .to_string(),
        "window allocation frontend disconnected"
    );
    let mut stopped = false;
    assert!(begin_frontend_quiescence(&mut publisher, &sender, &mut stopped).is_err());
    assert!(!stopped);
    assert!(publisher.quiescing);
    let mut publisher = LiveWindowAllocationPublisher::default();
    let (sender, receiver) = sync_channel(1);
    publisher.poll_snapshot(now, 1, &sender, Vec::new).unwrap();
    drop(update(&receiver));
    assert_eq!(
        publisher
            .poll_snapshot(now, 1, &sender, Vec::new)
            .unwrap_err()
            .to_string(),
        "window allocation acknowledgement disconnected"
    );
}

#[test]
fn allocation_shutdown_active_full_queue_retries_without_consuming_generation() {
    let now = Instant::now();
    let mut publisher = LiveWindowAllocationPublisher::default();
    let (sender, receiver) = sync_channel(1);
    sender
        .send(XServerFrontendServiceCommand::StopAccepting)
        .unwrap();
    publisher.poll_snapshot(now, 1, &sender, Vec::new).unwrap();
    assert_eq!(publisher.generation, 0);
    assert!(publisher.pending.is_none());
    receiver.recv().unwrap();
    publisher
        .poll_snapshot(now + Duration::from_secs(1), 1, &sender, Vec::new)
        .unwrap();
    update(&receiver)
        .send(XWindowAllocationUpdate::Applied)
        .unwrap();
    publisher
        .poll_snapshot(now + Duration::from_secs(1), 1, &sender, Vec::new)
        .unwrap();
    assert_eq!(publisher.applied.as_ref().unwrap().generation, 1);
}
