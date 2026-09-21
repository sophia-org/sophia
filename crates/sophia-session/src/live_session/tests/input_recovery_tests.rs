use super::super::*;
use crate::session_control::{SESSION_CONTROL_CAPACITY, SessionControlQueue};
use sophia_protocol::RoutedInputRequest;

fn release_route(id: u64) -> XAuthorityRoutedInput {
    XAuthorityRoutedInput {
        request: RoutedInputRequest {
            serial: id,
            seat: SeatId::from_raw(1),
            device: sophia_protocol::DeviceId::from_raw(1),
            time_msec: 0,
            target_surface: SurfaceId::new(11, 3),
            global_position: sophia_protocol::Point::default(),
            local_position: sophia_protocol::Point::default(),
            kind: sophia_protocol::InputEventKind::Key {
                keycode: 30,
                pressed: false,
            },
        },
        route_lease: None,
        delivery: Some(XAuthorityInputDeliveryId::from_raw(id)),
        mode: sophia_x_authority::XAuthorityRoutedInputMode::Deliver,
        origin: sophia_x_authority::XAuthorityRoutedInputOrigin::Physical,
    }
}

#[test]
fn input_recovery_deadline_releases_the_owner_control_barrier() {
    for close in [false, true] {
        let (ack_sender, acks) = sync_channel(SESSION_CONTROL_CAPACITY);
        let (delivery_sender, receipts) = std::sync::mpsc::channel();
        let broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
            NonZeroUsize::new(4).unwrap(),
            ack_sender,
            delivery_sender,
        );
        let input = broker.routed_input_sender();
        let release = release_route(1);
        let id = release.delivery.unwrap();
        input.send(release).unwrap();
        let now = input.delivery_ticket(id).unwrap().admitted_at;
        let mut state = InputDeliveryState {
            fail_on_client_error: false,
            events_expected: 1,
            ..Default::default()
        };
        state.track(&input, [id], true).unwrap();
        let mut barrier = BTreeSet::from([id]);
        let mut proof = None;
        let mut deadline = None;
        let mut queue = SessionControlQueue::default();
        let (control_sender, commands) = sync_channel(4);
        let transaction = TransactionId::from_raw(11);
        let surface = SurfaceId::new(22, 5);
        let command = sophia_x_authority::XAuthorityClientControlCommand {
            client: sophia_x_authority::XServerFrontendClientId::from_raw(2),
            command: if close {
                XAuthorityControlCommand::CloseSurface {
                    transaction,
                    surface,
                }
            } else {
                XAuthorityControlCommand::FocusSurface {
                    transaction,
                    surface,
                }
            },
        };
        queue.enqueue(command, now).unwrap();
        let mut completions = Vec::new();
        // The original missing watchdog leaves this exact queue indefinitely
        // held. Prove the specimen still exists before exercising the repair.
        queue
            .service_when(
                &control_sender,
                &acks,
                now + Duration::from_secs(120),
                &mut completions,
                barrier.is_empty(),
            )
            .unwrap();
        assert!(commands.try_recv().is_err());
        assert!(completions.is_empty());
        // Start a fresh queue for the repaired, monotonic-clock execution.
        let mut queue = SessionControlQueue::default();
        queue.enqueue(command, now).unwrap();
        for elapsed in [5999, 6000] {
            let at = now + Duration::from_millis(elapsed);
            InputDeliveryPhase {
                sender: Some(&input),
                receiver: &receipts,
                state: &mut state,
                client_key_release_barrier: &mut barrier,
                proof_started_at: &mut proof,
                post_input_deadline: &mut deadline,
            }
            .drain_at(at)
            .unwrap();
            queue
                .service_when(
                    &control_sender,
                    &acks,
                    at,
                    &mut completions,
                    barrier.is_empty(),
                )
                .unwrap();
            if elapsed == 5999 {
                assert!(commands.try_recv().is_err());
            }
        }
        assert!(barrier.is_empty());
        assert!(state.pending.is_empty());
        assert_eq!(state.events_flushed, 0);
        assert_eq!(commands.try_recv().unwrap(), command);
        assert!(completions.is_empty());
    }
}

#[test]
fn input_recovery_seat_revoke_cancels_unresolved_key_and_pointer_before_handoff() {
    let (acks, _ack_receiver) = sync_channel(4);
    let (delivery_sender, receipts) = std::sync::mpsc::channel();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        acks,
        delivery_sender,
    );
    let input = broker.routed_input_sender();
    let key = release_route(1);
    let mut pointer = release_route(2);
    pointer.request.kind = sophia_protocol::InputEventKind::PointerMotion;
    input.send(key).unwrap();
    input.send(pointer).unwrap();
    let ids = [
        XAuthorityInputDeliveryId::from_raw(1),
        XAuthorityInputDeliveryId::from_raw(2),
    ];
    let now = input.delivery_ticket(ids[0]).unwrap().admitted_at;
    let mut state = InputDeliveryState {
        fail_on_client_error: false,
        events_expected: 2,
        ..Default::default()
    };
    state.track(&input, ids, false).unwrap();
    let mut barrier = BTreeSet::from([ids[0]]);
    assert_eq!(
        input
            .recover_input_deliveries(now + Duration::from_millis(500), true)
            .unwrap()
            .len(),
        2
    );
    let mut proof = None;
    let mut deadline = None;
    InputDeliveryPhase {
        sender: Some(&input),
        receiver: &receipts,
        state: &mut state,
        client_key_release_barrier: &mut barrier,
        proof_started_at: &mut proof,
        post_input_deadline: &mut deadline,
    }
    .drain_at(now + Duration::from_millis(500))
    .unwrap();
    assert!(state.pending.is_empty());
    assert!(barrier.is_empty());
    assert!(input.advance_control_epoch(2));
    broker.route_pending().unwrap();
    assert!(receipts.try_recv().is_err());
    assert!(ids.iter().all(|id| input.delivery_ticket(*id).is_none()));
    input.send(release_route(3)).unwrap();
    assert_eq!(
        input
            .delivery_ticket(XAuthorityInputDeliveryId::from_raw(3))
            .unwrap()
            .control_epoch,
        2
    );
}
