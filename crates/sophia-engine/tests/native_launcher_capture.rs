use sophia_engine::{LauncherCapture, LauncherInput, NativeLauncherCommand};
use sophia_protocol::*;
fn binding() -> NativeLauncherBinding {
    NativeLauncherBinding {
        grant: ContentGrant {
            connection_epoch: 4,
            content_grant_epoch: 9,
        },
        opening: 3,
        output: ContentOutputId {
            id: 2,
            generation: 1,
        },
        allocation: ContentAllocationId {
            id: 5,
            generation: 2,
        },
        catalog_generation: 7,
        candidate_generation: 11,
        presentation_epoch: 13,
        interaction_generation: 1,
        state_revision: 8,
        focus_lease: 6,
    }
}
fn event(kind: InputEventKind) -> InputEventPacket {
    InputEventPacket {
        serial: 1,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        time_msec: 1,
        kind,
        global_position: None,
        target_surface: None,
        local_position: None,
    }
}
fn key(code: u32, pressed: bool) -> InputEventPacket {
    event(InputEventKind::Key {
        keycode: code,
        pressed,
    })
}
#[test]
fn semantic_text_navigation_and_accept_keep_exact_supplied_focus_without_slot_guess() {
    let mut capture = LauncherCapture::default();
    assert!(
        !capture
            .route(&key(30, true), Some("a"), None, false, false)
            .0
    );
    capture.present_native(Some(binding()));
    for (code, kind, text) in [
        (30, NativeLauncherInputKind::Text, "Привет λ"),
        (28, NativeLauncherInputKind::Accept, ""),
        (105, NativeLauncherInputKind::Left, ""),
        (106, NativeLauncherInputKind::Right, ""),
        (103, NativeLauncherInputKind::Previous, ""),
        (108, NativeLauncherInputKind::Next, ""),
        (14, NativeLauncherInputKind::Backspace, ""),
        (111, NativeLauncherInputKind::Delete, ""),
    ] {
        let (consumed, command) = capture.route(&key(code, true), Some(text), None, false, false);
        assert!(consumed);
        let command = command.unwrap();
        assert_eq!((command.output.raw(), command.presentation_epoch), (2, 13));
        assert_eq!(
            command.input,
            LauncherInput::Native {
                binding: binding(),
                command: NativeLauncherCommand::Input {
                    kind,
                    text: text.into()
                }
            }
        );
        assert!(capture.route(&key(code, false), None, None, false, false).0);
    }
    assert!(matches!(
        capture
            .route(&key(1, true), None, None, false, false)
            .1
            .unwrap()
            .input,
        LauncherInput::Native {
            command: NativeLauncherCommand::Dismiss,
            ..
        }
    ));
    assert!(
        capture
            .route(&key(22, true), None, None, true, true)
            .1
            .is_some()
    );
    assert!(
        capture
            .route(&key(46, true), Some("c"), None, false, true)
            .1
            .is_none()
    );
    assert!(
        capture
            .route(&key(31, true), Some("\0"), None, false, false)
            .1
            .is_none()
    );
}
#[test]
fn replacement_and_revocation_preserve_consumed_sequences() {
    let mut capture = LauncherCapture::default();
    capture.present_native(Some(binding()));
    assert!(
        capture
            .route(&key(30, true), Some("a"), None, false, false)
            .1
            .is_some()
    );
    let mut next = binding();
    next.grant.content_grant_epoch += 1;
    capture.present_native(Some(next));
    assert_eq!(
        capture.route(&key(30, true), Some("a"), None, false, false),
        (true, None)
    );
    capture.present_native(None);
    assert_eq!(
        capture.route(&key(30, false), None, None, false, false),
        (true, None)
    );
    assert_eq!(
        capture.route(&key(30, true), Some("a"), None, false, false),
        (false, None)
    );
}
#[test]
fn pointer_targets_precede_modal_fallback_and_wheel_does_not_cross_focus() {
    let mut capture = LauncherCapture::default();
    capture.present_native(Some(binding()));
    let down = event(InputEventKind::PointerButton {
        button: 272,
        pressed: true,
    });
    assert_eq!(
        capture.route(&down, None, None, false, false),
        (false, None)
    );
    assert_eq!(
        capture.route_native_pointer_fallback(&down).unwrap(),
        (true, None)
    );
    capture.present_native(None);
    let up = event(InputEventKind::PointerButton {
        button: 272,
        pressed: false,
    });
    assert_eq!(capture.route(&up, None, None, false, false), (true, None));
    capture.present_native(Some(binding()));
    let wheel = event(InputEventKind::PointerAxis {
        horizontal_v120: 0,
        vertical_v120: 60,
    });
    assert_eq!(
        capture.route(&wheel, None, None, false, false),
        (false, None)
    );
    assert!(
        capture
            .route_native_pointer_fallback(&wheel)
            .unwrap()
            .1
            .is_none()
    );
    let mut next = binding();
    next.focus_lease += 1;
    capture.present_native(Some(next));
    assert!(
        capture
            .route_native_pointer_fallback(&wheel)
            .unwrap()
            .1
            .is_none()
    );
    assert!(
        matches!(capture.route_native_pointer_fallback(&wheel).unwrap().1.unwrap().input,
        LauncherInput::Native { binding: actual, command: NativeLauncherCommand::Input {
            kind: NativeLauncherInputKind::Next, .. } } if actual == next)
    );
}
#[test]
fn bounded_sequence_inventory_reports_exhaustion_before_an_untracked_capture() {
    let mut capture = LauncherCapture::default();
    capture.present_native(Some(binding()));
    for id in 1..=1024 {
        let mut press = key(30, true);
        press.device = DeviceId::from_raw(id);
        assert!(matches!(
            capture
                .route(&press, Some("a"), None, false, false)
                .1
                .unwrap()
                .input,
            LauncherInput::Native { .. }
        ));
    }
    let mut excess = key(30, true);
    excess.device = DeviceId::from_raw(1025);
    assert_eq!(
        capture
            .route(&excess, Some("a"), None, false, false)
            .1
            .unwrap()
            .input,
        LauncherInput::CaptureCapacityExceeded
    );
    excess.kind = InputEventKind::PointerButton {
        button: 272,
        pressed: true,
    };
    assert!(capture.route_native_pointer_fallback(&excess).is_err());
}
