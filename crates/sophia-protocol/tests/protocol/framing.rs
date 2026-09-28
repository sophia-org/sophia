#[test]
fn routed_input_request_is_protocol_neutral_and_surface_targeted() {
    let request = RoutedInputRequest {
        serial: 99,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        time_msec: 1_000,
        target_surface: SurfaceId::new(42, 1),
        global_position: Point { x: 20.0, y: 30.0 },
        local_position: Point { x: 12.5, y: 9.0 },
        kind: InputEventKind::PointerButton {
            button: 1,
            pressed: true,
        },
    };

    assert_eq!(request.serial, 99);
    assert_eq!(request.target_surface, SurfaceId::new(42, 1));
    assert_eq!(request.local_position.x, 12.5);
    assert_eq!(request.device, DeviceId::from_raw(2));
    assert_eq!(
        request.kind,
        InputEventKind::PointerButton {
            button: 1,
            pressed: true,
        }
    );
}

#[test]
fn pointer_axis_packet_uses_protocol_neutral_v120_units() {
    let kind = InputEventKind::PointerAxis {
        horizontal_v120: -120,
        vertical_v120: 240,
    };

    assert_eq!(
        kind,
        InputEventKind::PointerAxis {
            horizontal_v120: -120,
            vertical_v120: 240,
        }
    );
}

#[test]
fn routed_input_decision_carries_authority_rejection() {
    let decision = RoutedInputDecision {
        serial: 100,
        target_surface: SurfaceId::new(55, 3),
        outcome: RoutedInputOutcome::RejectedDeniedNamespace,
    };

    assert_eq!(decision.serial, 100);
    assert_eq!(
        decision.outcome,
        RoutedInputOutcome::RejectedDeniedNamespace
    );
}

 #[test]
fn broker_health_frame_roundtrips() {
    let packet = BrokerHealthPacket::new(
        BrokerKind::Portal,
        BrokerHealthState::Ready,
        12,
        Some("ready".to_owned()),
    )
    .unwrap();

    let frame = encode_broker_health_frame(&packet).unwrap();

    assert_eq!(decode_broker_health_frame(&frame), Ok(packet));
    assert_eq!(frame_payload_len(&frame), 12);
}

#[test]
fn portal_broker_and_payload_frames_roundtrip() {
    let transfer = PortalTransferId::from_raw(41);
    let request = PortalBrokerRequestPacket {
        request: PortalRequest {
            transfer: PortalTransfer {
                transfer,
                source_namespace: NamespaceId::from_raw(10),
                target_namespace: NamespaceId::from_raw(20),
                kind: PortalTransferKind::Clipboard,
                mime_type: Some("UTF8_STRING".to_owned()),
                byte_size: 6,
                decision: PortalDecision::Pending,
                generation: 7,
            },
            deadline_msec: 2_000,
        },
        source_may_publish: true,
        target_may_request: true,
    };
    let frame = encode_portal_broker_request_frame(&request).unwrap();
    assert_eq!(decode_portal_broker_request_frame(&frame), Ok(request));

    let response = PortalBrokerResponsePacket {
        transfer,
        decision: PortalBrokerResponseDecision::Allowed(PortalGrant {
            transfer,
            source_namespace: NamespaceId::from_raw(10),
            target_namespace: NamespaceId::from_raw(20),
            kind: PortalTransferKind::Clipboard,
            source_generation: 7,
            broker_generation: 3,
            deadline_msec: 2_000,
            state: PortalGrantState::Active,
        }),
    };
    let frame = encode_portal_broker_response_frame(&response).unwrap();
    assert_eq!(decode_portal_broker_response_frame(&frame), Ok(response));

    let frame = encode_portal_clipboard_payload_frame(transfer, b"sophia").unwrap();
    assert_eq!(
        decode_portal_clipboard_payload_frame(&frame),
        Ok((transfer, b"sophia".to_vec()))
    );
}

#[test]
fn metadata_broker_health_frame_roundtrips_without_message() {
    let packet =
        BrokerHealthPacket::new(BrokerKind::Metadata, BrokerHealthState::Stopped, 13, None)
            .unwrap();

    let frame = encode_broker_health_frame(&packet).unwrap();

    assert_eq!(decode_broker_health_frame(&frame), Ok(packet));
    assert_eq!(frame_payload_len(&frame), 5);
}

#[test]
fn broker_health_frame_rejects_oversized_status_message() {
    let mut payload = Vec::new();
    push_u16(&mut payload, 1);
    push_u16(&mut payload, 3);
    push_u8(&mut payload, 1);
    push_u16(
        &mut payload,
        (SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN as u16) + 1,
    );
    payload.extend(std::iter::repeat_n(
        b'x',
        SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN + 1,
    ));
    let frame = encode_frame(
        IpcMessageKind::BrokerHealth,
        TransactionId::from_raw(4),
        &payload,
    )
    .unwrap();

    assert_eq!(
        decode_broker_health_frame(&frame),
        Err(IpcCodecError::TextTooLarge {
            field: "broker_health_message",
            len: SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN + 1,
            max: SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN,
        })
    );
}

#[test]
fn oversized_payload_is_rejected_before_allocation() {
    let mut frame = Vec::new();
    push_u32(&mut frame, SOPHIA_IPC_MAGIC);
    push_u16(&mut frame, SOPHIA_IPC_VERSION);
    push_u16(&mut frame, IpcMessageKind::BrokerHealth as u16);
    push_u64(&mut frame, 1);
    push_u32(&mut frame, (SOPHIA_IPC_MAX_PAYLOAD_LEN as u32) + 1);
    push_u32(&mut frame, 0);

    assert_eq!(
        decode_frame(&frame),
        Err(IpcCodecError::PayloadTooLarge(
            SOPHIA_IPC_MAX_PAYLOAD_LEN + 1
        ))
    );
}

#[test]
fn malformed_frames_fail_closed() {
    assert_eq!(decode_frame(&[]), Err(IpcCodecError::Truncated));

    let packet = BrokerHealthPacket::new(
        BrokerKind::Portal,
        BrokerHealthState::Ready,
        1,
        None,
    )
    .unwrap();
    let mut frame = encode_broker_health_frame(&packet).unwrap();
    frame[0] = 0;
    assert_eq!(decode_frame(&frame), Err(IpcCodecError::BadMagic));

    let mut frame = encode_broker_health_frame(&packet).unwrap();
    frame.push(0);
    assert_eq!(decode_frame(&frame), Err(IpcCodecError::TrailingBytes(1)));
}

fn frame_payload_len(frame: &[u8]) -> usize {
    u32::from_le_bytes(frame[16..20].try_into().unwrap()) as usize
}

fn push_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
