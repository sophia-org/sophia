//! Physical input while the session is locked (t292, t293 review finding 2):
//! keys reach the lock's secret and nothing else, and no report, metric or
//! coverage record counts or times them.

use super::*;

fn packet(serial: u64, kind: InputEventKind) -> InputEventPacket {
    InputEventPacket {
        serial,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(1),
        time_msec: serial,
        kind,
        global_position: None,
        target_surface: None,
        local_position: None,
    }
}

#[test]
fn locked_keys_reach_the_secret_and_are_never_counted() {
    let keyboard = sophia_engine::SessionLockKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap();
    let mut lock = crate::session_lock_input::SessionLockInput::new(keyboard).unwrap();
    // Shift+A, then b: a shifted printable key, which coverage would record.
    let mut events: Vec<_> = [
        (42, true),
        (30, true),
        (30, false),
        (42, false),
        (48, true),
        (48, false),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (keycode, pressed))| {
        packet(
            u64::try_from(index + 1).unwrap(),
            InputEventKind::Key { keycode, pressed },
        )
    })
    .collect();
    events.push(packet(7, InputEventKind::PointerMotion));
    let (input_sender, input_receiver) = sync_channel(4);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, _) = test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = crate::emergency_input::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut next_delivery = 1;

    let report = crate::live_session::locked::route_locked_input(
        events,
        &mut lock,
        &mut client_keys,
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &mut emergency,
        &mut virtual_terminal,
        &mut keyboard_coverage,
        &mut next_delivery,
        0,
    )
    .unwrap();

    assert_eq!(
        lock.take_edits().len(),
        2,
        "both characters reached the secret"
    );
    assert_eq!(report.keys_observed, 0);
    assert_eq!(report.events, 1, "only the pointer event is counted");
    assert_eq!(report.pointer_events, 1);
    assert!(report.routed_key_presses.is_empty());
    assert!(report.deferred_key_presses.is_empty());
    assert_eq!(keyboard_coverage.snapshot().shifted_positions, 0);
    assert!(input_receiver.try_recv().is_err(), "nothing reached X");
}
