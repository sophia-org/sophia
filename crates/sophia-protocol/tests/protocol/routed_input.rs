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
