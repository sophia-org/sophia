//! Ledger -> real transport FIFO -> private peer. No WM/native acceptance.
use super::*;
#[path = "catalog_tests.rs"]
mod catalog_tests;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::{ContentEpochRegistry, ContentStoreProfile};
#[path = "action_files.rs"]
mod files;
use files::Peer;

fn transport_peer() -> (Peer, ContentEpochRegistry, ContentLimits) {
    let mut epochs = files::empty();
    let mut peer = Peer::new(&mut epochs, ContentStoreProfile::Legacy);
    peer.negotiate(
        &mut epochs,
        ShellV1ClientHello {
            minimum_revision: 5,
            maximum_revision: 6,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT,
        },
        files::granted(),
    )
    .unwrap();
    (peer, epochs, files::limits())
}

fn read_action(peer: &mut Peer, epochs: &mut ContentEpochRegistry) -> ContentAction {
    let ShellFileTransactionRecord {
        record: ShellContentRecord::Action(action),
        ..
    } = decode_shell_file_transaction(&peer.read(epochs), ShellFileKind::Action).unwrap()
    else {
        panic!("action");
    };
    action
}

#[test]
fn expired_action_keeps_its_real_fifo_cancel_credit_until_exact_transfer() {
    let (mut peer, mut epochs, limits) = transport_peer();
    let directory = peer.transport.socket_path().parent().unwrap().to_path_buf();
    let mut target = super::tests::target();
    target.grant = limits.grant;
    let mut ledger = ContentActionLedger::default();
    let event = ledger
        .issue(
            target,
            0,
            &limits,
            TransactionId::from_raw(1),
            &mut peer.transport.connection(&mut epochs),
        )
        .unwrap()
        .unwrap();
    peer.transport.poll_io(&mut epochs).unwrap();
    let action = read_action(&mut peer, &mut epochs);
    assert_eq!((action.kind, action.event_id), (ACTION_ACTIVATE, event));
    // No acknowledgement. The actual ACK service must not discard the credit
    // at the deadline before the cancellation producer gets its turn.
    let now = u64::from(limits.action_ack_timeout_ms) + 1;
    assert_eq!(
        ledger
            .service_acks(&mut peer.transport.connection(&mut epochs), now, 64)
            .unwrap(),
        0
    );
    let index = ledger
        .next_cancellation(&[], now)
        .expect("deadline retains cancel obligation");
    ledger
        .queue_cancellation(
            index,
            TransactionId::from_raw(2),
            &mut peer.transport.connection(&mut epochs),
        )
        .unwrap();
    assert_eq!(ledger.next_cancellation(&[], now), None);
    peer.transport.poll_io(&mut epochs).unwrap();
    let cancel = read_action(&mut peer, &mut epochs);
    let mut expected = action;
    expected.kind = ACTION_CANCEL;
    assert_eq!(cancel, expected);
    assert!(ledger.live.is_empty());
    // Cancel has no ACK and cannot be emitted again on a later service turn.
    ledger
        .service_acks(&mut peer.transport.connection(&mut epochs), now + 1, 64)
        .unwrap();
    assert_eq!(ledger.next_cancellation(&[], now + 1), None);
    peer.assert_no_event(&mut epochs);
    peer.transport.disconnect(&mut epochs).unwrap();
    drop(peer);
    assert!(
        !directory.exists(),
        "transport teardown removes its private endpoint"
    );
}

#[test]
fn a_full_action_queue_cannot_erase_the_outside_dismissal_deadline() {
    let (mut peer, mut epochs, limits) = transport_peer();
    let mut target = super::tests::target();
    target.grant = limits.grant;
    let mut ledger = ContentActionLedger::default();
    for i in 0..limits.max_pending_actions {
        assert!(
            ledger
                .issue(
                    target.clone(),
                    1,
                    &limits,
                    TransactionId::from_raw(u64::from(i) + 1),
                    &mut peer.transport.connection(&mut epochs)
                )
                .unwrap()
                .is_some()
        );
    }
    let popout = sophia_engine::PresentedContentDismissal {
        grant: target.grant,
        output: target.output,
        candidate_generation: target.candidate_generation,
        presentation_epoch: target.presentation_epoch,
        interaction_generation: target.interaction_generation,
        allocation: target.allocation,
    };
    let deadline = 10 + u64::from(limits.action_ack_timeout_ms);
    for now in [10, 20] {
        assert_eq!(
            ledger
                .issue_dismissal(
                    popout.clone(),
                    now,
                    &limits,
                    TransactionId::from_raw(100),
                    &mut peer.transport.connection(&mut epochs)
                )
                .unwrap(),
            None
        );
    }
    assert!(ledger.dismissal_expired(popout.allocation, deadline));
    assert!(!ledger.dismissal_expired(popout.allocation, deadline - 1));
    assert_eq!(ledger.dismissals.len(), 1);
    assert!(!ledger.dismissals[0].notification_sent);
    assert_eq!(
        ledger.next_event_id,
        u64::from(limits.max_pending_actions) + 1
    );
    peer.transport.poll_io(&mut epochs).unwrap();
    for _ in 0..limits.max_pending_actions {
        let action = read_action(&mut peer, &mut epochs);
        assert_eq!(action.kind, 1);
    }
    peer.assert_no_event(&mut epochs);
}

#[test]
fn dismissal_wire_identity_has_no_coordinates_and_ack_cannot_renew_its_deadline() {
    let (mut peer, mut epochs, limits) = transport_peer();
    let target = super::tests::target();
    let popout = sophia_engine::PresentedContentDismissal {
        grant: limits.grant,
        output: target.output,
        candidate_generation: target.candidate_generation,
        presentation_epoch: target.presentation_epoch,
        interaction_generation: target.interaction_generation,
        allocation: target.allocation,
    };
    let mut ledger = ContentActionLedger::default();
    let id = ledger
        .issue_dismissal(
            popout.clone(),
            10,
            &limits,
            TransactionId::from_raw(1),
            &mut peer.transport.connection(&mut epochs),
        )
        .unwrap()
        .unwrap();
    peer.transport.poll_io(&mut epochs).unwrap();
    let action = read_action(&mut peer, &mut epochs);
    assert_eq!(
        (
            action.kind,
            action.target_id,
            action.target_generation,
            action.action_id
        ),
        (2, 0, 0, 0)
    );
    assert_eq!(
        (
            action.grant,
            action.output,
            action.allocation,
            action.presentation_epoch
        ),
        (
            popout.grant,
            popout.output,
            popout.allocation,
            popout.presentation_epoch
        )
    );
    let deadline = 10 + u64::from(limits.action_ack_timeout_ms);
    assert_eq!(
        ledger
            .issue_dismissal(
                popout,
                20,
                &limits,
                TransactionId::from_raw(2),
                &mut peer.transport.connection(&mut epochs)
            )
            .unwrap(),
        Some(id)
    );
    assert_eq!(ledger.dismissals.len(), 1);
    let mut ack = ContentActionAck {
        grant: action.grant,
        output: action.output,
        candidate_generation: action.candidate_generation,
        presentation_epoch: action.presentation_epoch,
        interaction_generation: action.interaction_generation,
        allocation: action.allocation,
        target_id: 0,
        target_generation: 0,
        action_id: 0,
        event_id: id,
        disposition: 1,
    };
    ack.presentation_epoch += 1;
    ledger.acknowledge(&ack, 21).unwrap();
    assert!(!ledger.dismissals[0].acknowledged);
    ack.presentation_epoch -= 1;
    peer.send_content(
        &mut epochs,
        TransactionId::from_raw(3),
        ShellContentRecord::ActionAck(ack),
    );
    assert_eq!(
        ledger
            .service_acks(&mut peer.transport.connection(&mut epochs), 22, 1)
            .unwrap(),
        1
    );
    assert!(ledger.dismissals[0].acknowledged);
    assert!(!ledger.dismissal_expired(action.allocation, deadline - 1));
    assert!(ledger.dismissal_expired(action.allocation, deadline));
    assert_eq!(ledger.dismissals[0].deadline_msec, deadline);
}
