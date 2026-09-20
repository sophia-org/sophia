// A device announcement is consumed on the session's physical turn and never
// becomes a client route. Should one reach the registry anyway, the sender
// is told nothing was delivered, and the client hears nothing at all.

fn announcement_to(
    surface: SurfaceId,
    delivery: XAuthorityInputDeliveryId,
    kind: InputEventKind,
) -> XAuthorityRoutedInput {
    XAuthorityRoutedInput {
        request: RoutedInputRequest {
            serial: 1,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(257),
            time_msec: 0,
            target_surface: surface,
            global_position: Point::default(),
            local_position: Point::default(),
            kind,
        },
        route_lease: None,
        delivery: Some(delivery),
        mode: XAuthorityRoutedInputMode::Deliver,
    }
}

#[test]
fn a_device_announcement_routed_to_a_client_is_rejected_and_delivers_nothing() {
    let namespace = NamespaceId::from_raw(26);
    let client = XServerFrontendClientId(22);
    let surface = SurfaceId::new(36, 1);
    let window = XResourceId::new(0x200060, 1);
    let (control_ack_sender, _control_ack_receiver) = sync_channel(4);
    let (delivery_sender, delivery_receiver) = channel();
    let mut broker = XServerFrontendRouteBroker::with_control_and_input_delivery_senders(
        NonZeroUsize::new(4).unwrap(),
        control_ack_sender,
        delivery_sender,
    );
    let (_registration, channels) = broker.registry.register_client(client).unwrap();
    broker
        .registry
        .register_surface(client, namespace, surface, window)
        .unwrap();
    let sender = broker.routed_input_sender();

    for (raw, kind) in [
        (
            61,
            InputEventKind::DeviceAdded {
                keyboard: true,
                pointer: false,
                touch: false,
                virtual_bus: false,
            },
        ),
        (62, InputEventKind::DeviceRemoved),
    ] {
        let delivery = XAuthorityInputDeliveryId::from_raw(raw);
        sender
            .send(announcement_to(surface, delivery, kind))
            .expect("an open broker to admit work");

        assert_eq!(broker.route_pending(), Ok(1));
        assert_eq!(
            delivery_receiver.recv().unwrap(),
            XAuthorityClientInputDelivery {
                client,
                delivery,
                outcome: XAuthorityInputDeliveryOutcome::RouteRejected,
            }
        );
        assert_eq!(channels.input.try_recv(), Err(TryRecvError::Empty));
    }
}
