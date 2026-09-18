#![cfg(feature = "native-session")]
//! Real socket/store intake and the shared Session admission sequence. Renderer
//! completion/protection are supplied; no application is spawned by these tests.
use sophia_engine::PresentedContentTarget;
use sophia_protocol::*;
use sophia_runtime::ShellTransportError;
use sophia_session::session_actions::SessionLaunchQueue;
use sophia_session::shell_native_launcher::NativeLauncherActionService;
#[path = "support/content_actions/native_launcher_fixture.rs"]
mod fixture;
use fixture::*;

#[test]
fn keyboard_admission_is_once_and_independent_of_ack_order_or_disposition() {
    for ack_first in [false, true] {
        for disposition in [1, 2] {
            let mut h = Harness::new();
            let activation = h.accept();
            if ack_first {
                assert!(h.acknowledge(activation, disposition));
            }
            assert_eq!(h.activate(activation, 0).status, 1);
            if !ack_first {
                assert!(h.acknowledge(activation, disposition));
            }
            assert_eq!(h.queue.pending_len(), 1);
            assert!(!h.service(0));
            assert_eq!(h.activate(activation, 0).status, 2);
            assert_eq!(h.queue.pending_len(), 1);
            let payload = h.dispatch();
            assert_eq!(payload.activation, activation);
            assert!(h.queue.native_catalog_admission(&payload));
            assert_ne!(payload.transaction, tx(20)); // client request transaction
            h.queue.cancel_catalog(payload.transaction);
            assert!(h.queue.native_catalog_admission(&payload));
            h.queue.cancel_native_catalog(&payload);
            assert!(!h.queue.native_catalog_admission(&payload));
        }
    }
}

#[test]
fn presented_catalog_and_issued_accept_are_both_required_before_queue_insertion() {
    for fault in 0..6 {
        let mut h = Harness::new();
        let mut activation = h.accept();
        match fault {
            0 => activation.event.binding.focus_lease += 1,
            1 => activation.event.event_id += 1,
            2 => activation.slot = 1, // visible, but keyboard selected slot is 2
            3 => activation.event.binding.catalog_generation += 1,
            4 => activation.event.binding.presentation_epoch += 1,
            5 => activation.event.binding.opening += 1,
            _ => unreachable!(),
        }
        assert_ne!(h.activate(activation, 0).status, 1);
        assert_eq!(h.queue.pending_len(), 0);
    }
}

#[test]
fn capacity_refusal_does_not_make_the_same_accept_replayable() {
    let mut h = Harness::new();
    let activation = h.accept();
    assert_eq!(
        h.activate(
            activation,
            sophia_session::session_actions::SESSION_ACTION_APPLICATION_CAPACITY
        )
        .status,
        5
    );
    assert_eq!(h.queue.pending_len(), 0);
    assert_eq!(h.activate(activation, 0).status, 2);
    assert_eq!(h.queue.pending_len(), 0);
}

#[test]
fn pointer_requires_actual_ledger_event_and_can_choose_an_unselected_visible_row() {
    for ack_first in [false, true] {
        let mut h = Harness::new();
        let event_id = h
            .service
            .issue(
                &mut h.peer.transport.connection(&mut h.epochs),
                h.target.clone(),
                tx(30),
                1,
            )
            .unwrap()
            .unwrap();
        h.peer.transport.poll_io(&mut h.epochs).unwrap();
        let (_, ShellContentRecord::Action(action)) =
            decode_shell_content_frame(&h.peer.read()).unwrap()
        else {
            panic!();
        };
        let activation = NativeLauncherActivation {
            event: NativeLauncherEvent {
                binding: h.focus,
                event_id,
                state_revision: 1,
            },
            cause: 2,
            slot: 1,
        };
        if ack_first {
            h.peer
                .send_content(ShellContentRecord::ActionAck(ContentActionAck {
                    grant: action.grant,
                    output: action.output,
                    candidate_generation: action.candidate_generation,
                    presentation_epoch: action.presentation_epoch,
                    interaction_generation: action.interaction_generation,
                    allocation: action.allocation,
                    target_id: action.target_id,
                    target_generation: action.target_generation,
                    action_id: action.action_id,
                    event_id: action.event_id,
                    disposition: 2,
                }));
            assert_eq!(
                h.service
                    .service_acks(&mut h.peer.transport.connection(&mut h.epochs), 1, 1)
                    .unwrap(),
                1
            );
        }
        let mut wrong = activation;
        wrong.event.event_id += 1;
        assert_eq!(h.activate(wrong, 0).status, 2);
        assert_eq!(h.queue.pending_len(), 0);
        assert_eq!(h.activate(activation, 0).status, 1);
        assert_eq!(h.queue.pending_len(), 1);
        assert_eq!(h.activate(activation, 0).status, 2);
    }
}

#[test]
fn revoked_exact_grant_cancels_native_queue_but_not_a_retained_worker_payload() {
    let mut h = Harness::new();
    let activation = h.accept();
    assert_eq!(h.activate(activation, 0).status, 1);
    let payload = h.dispatch();
    let mut wrong = GRANT;
    wrong.content_grant_epoch += 1;
    assert_eq!(h.queue.revoke_native_catalog_grant(wrong), 0);
    assert!(h.queue.native_catalog_admission(&payload));
    assert_eq!(h.queue.revoke_native_catalog_grant(GRANT), 1);
    assert!(!h.queue.native_catalog_admission(&payload));
    assert!(payload.entry.command.is_some());
    assert_eq!(h.queue.revoke_native_catalog_grant(GRANT), 0);
}

#[test]
fn saturated_socket_defers_admission_then_delivers_one_exact_queue_outcome() {
    let mut h = Harness::new();
    let activation = h.accept();
    let frame = encode_shell_content_frame(
        tx(40),
        &ShellContentRecord::OutputFacts(ContentOutputFacts {
            grant: GRANT,
            facts_generation: 5,
            outputs: vec![facts()],
        }),
    )
    .unwrap();
    let mut sent = 0;
    let mut full = false;
    for _ in 0..4096 {
        match h.peer.transport.send_async(&mut h.epochs, frame.clone()) {
            Ok(()) => sent += 1,
            Err(ShellTransportError::ActivationQueueSaturated) => {
                full = true;
                break;
            }
            Err(error) => panic!("unexpected backpressure: {error}"),
        }
    }
    assert!(full);
    h.peer.send(ShellNativeLauncherRecord::Activate(activation));
    assert!(!h.service(0));
    assert_eq!(h.queue.pending_len(), 0);
    for _ in 0..sent {
        h.peer.transport.poll_io(&mut h.epochs).unwrap();
        assert_eq!(h.peer.read(), frame);
    }
    assert!(h.service(0));
    assert_eq!(h.queue.pending_len(), 1);
    assert_eq!(h.outcome().status, 1);
    assert_eq!(h.activate(activation, 0).status, 2);
    assert_eq!(h.queue.pending_len(), 1);
}
