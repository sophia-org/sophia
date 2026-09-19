use super::*;
use crate::live_session::*;
use sophia_engine::{LauncherCapture, LauncherInput, LauncherKeyboard};

fn route_overlay_input(
    events: Vec<InputEventPacket>,
    capture: &mut LauncherCapture,
    keyboard: &mut LauncherKeyboard,
    pointer: &mut SessionPointerPlacement,
) -> PhysicalInputRouteReport {
    let (sender, receiver) = sync_channel(8);
    let (mut repeat, keymap) = test_key_repeat_parts();
    let report = route_input_events_with_launcher(
        events,
        &InputFocusState::new(),
        &[],
        &[],
        &Default::default(),
        &Default::default(),
        &sender,
        &mut XCoreKeyboardMapper::new(),
        &mut repeat,
        &keymap,
        &mut SessionClientKeyState::default(),
        &mut EmergencyChordState::awaiting_arm(),
        &mut VirtualTerminalChordState::default(),
        &mut PhysicalKeyboardCoverage::default(),
        None,
        pointer,
        true,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut 1,
        10,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(OutputId::from_raw(1)),
        7,
        None,
        None,
        None,
        Some((capture, keyboard)),
        None,
        &mut sophia_engine::RoutedInputCoalescer::new(),
        true,
        &mut None,
        std::time::Duration::ZERO,
    )
    .unwrap();
    assert_eq!(receiver.try_iter().count(), 0);
    report
}

#[test]
fn launcher_capture_preserves_cursor_motion_accounting_and_click_activation() {
    let mut capture = LauncherCapture::default();
    capture.present(
        Some((OutputId::from_raw(1), 7)),
        1,
        &[(
            1,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
        )],
        false,
    );
    let mut keyboard = LauncherKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap();
    let events = [
        InputEventKind::PointerMotion,
        InputEventKind::PointerButton {
            button: 272,
            pressed: true,
        },
        InputEventKind::PointerButton {
            button: 272,
            pressed: false,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(index, kind)| InputEventPacket {
        serial: index as u64 + 1,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(1),
        time_msec: 1,
        kind,
        global_position: Some(Point { x: 10.0, y: 20.0 }),
        target_surface: None,
        local_position: None,
    })
    .collect();
    let mut pointer = SessionPointerPlacement::default();
    let report = route_overlay_input(events, &mut capture, &mut keyboard, &mut pointer);
    assert_eq!(pointer.position(), Some(Point { x: 10.0, y: 20.0 }));
    assert_eq!(report.launcher_events.len(), 1);
    assert_eq!(report.launcher_events[0].input, LauncherInput::Activate(1));
    assert_eq!(report.pointer_events, 3);
    assert_eq!(report.pointer_buttons_observed, 2);
    assert_eq!(report.pointer_axes_observed, 0);
    // The owner uses observed motion to schedule the visible cursor update;
    // modal capture must still prevent delivery to the application underneath.
    assert_eq!(report.pointer_routed, 0);
    assert!(
        report.policy_inputs.is_empty(),
        "launcher capture must suppress hover even with a presented output epoch"
    );
}

#[test]
fn virtual_terminal_handoff_releases_launcher_modifiers_before_input_returns() {
    for capture_active in [false, true] {
        check_virtual_terminal_handoff(capture_active);
    }
}

fn check_virtual_terminal_handoff(capture_active: bool) {
    let mut keyboard = LauncherKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap();
    let mut capture = LauncherCapture::default();
    if capture_active {
        capture.present(Some((OutputId::from_raw(1), 7)), 1, &[], false);
    }
    let mut pointer = SessionPointerPlacement::default();
    let events = [29, 56, 60]
        .into_iter()
        .enumerate()
        .map(|(index, keycode)| InputEventPacket {
            serial: index as u64 + 1,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(1),
            time_msec: 1,
            kind: InputEventKind::Key {
                keycode,
                pressed: true,
            },
            global_position: None,
            target_surface: None,
            local_position: None,
        })
        .collect();
    let report = route_overlay_input(events, &mut capture, &mut keyboard, &mut pointer);
    assert_eq!(report.virtual_terminal, Some(2));
    // Physical releases can occur on the other VT. The same synthetic release
    // used by application and shortcut state must reach the launcher keyboard.
    assert!(!keyboard.command_modifier_active());
    for keycode in [29, 56] {
        keyboard.observe(keycode, true, false);
        keyboard.observe(keycode, false, false);
    }
    assert!(!keyboard.command_modifier_active());
    assert_eq!(keyboard.observe(30, true, true).0, Some("a".to_owned()));
}

#[test]
fn native_capture_uses_real_text_composition_and_retains_modal_pointer_accounting() {
    use sophia_engine::NativeLauncherCommand;
    use sophia_protocol::{
        ContentAllocationId, ContentGrant, ContentOutputId, NativeLauncherBinding,
        NativeLauncherInputKind,
    };
    let binding = NativeLauncherBinding {
        grant: ContentGrant {
            connection_epoch: 4,
            content_grant_epoch: 9,
        },
        opening: 3,
        output: ContentOutputId {
            id: 1,
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
    };
    let mut capture = LauncherCapture::default();
    capture.present_native(Some(binding)); // supplied retirement/focus; no native renderer
    let mut keyboard = LauncherKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap();
    let kinds = [
        InputEventKind::Key {
            keycode: 30,
            pressed: true,
        },
        InputEventKind::Key {
            keycode: 30,
            pressed: false,
        },
        InputEventKind::Key {
            keycode: 28,
            pressed: true,
        },
        InputEventKind::Key {
            keycode: 28,
            pressed: false,
        },
        InputEventKind::PointerMotion,
        InputEventKind::PointerButton {
            button: 272,
            pressed: true,
        },
        InputEventKind::PointerButton {
            button: 272,
            pressed: false,
        },
        InputEventKind::PointerAxis {
            horizontal_v120: 0,
            vertical_v120: 120,
        },
    ];
    let events = kinds
        .into_iter()
        .enumerate()
        .map(|(index, kind)| InputEventPacket {
            serial: index as u64 + 1,
            seat: SeatId::from_raw(1),
            device: DeviceId::from_raw(1),
            time_msec: 1,
            kind,
            global_position: Some(Point { x: 10.0, y: 20.0 }),
            target_surface: None,
            local_position: None,
        })
        .collect();
    let mut pointer = SessionPointerPlacement::default();
    let report = route_overlay_input(events, &mut capture, &mut keyboard, &mut pointer);
    assert_eq!(pointer.position(), Some(Point { x: 10.0, y: 20.0 }));
    assert_eq!(report.launcher_events.len(), 3);
    for (record, (kind, text)) in report.launcher_events.iter().zip([
        (NativeLauncherInputKind::Text, "a"),
        (NativeLauncherInputKind::Accept, ""),
        (NativeLauncherInputKind::Next, ""),
    ]) {
        assert_eq!(
            record.input,
            LauncherInput::Native {
                binding,
                command: NativeLauncherCommand::Input {
                    kind,
                    text: text.into()
                }
            }
        );
    }
    assert_eq!(report.pointer_events, 4);
    assert_eq!(report.pointer_buttons_observed, 2);
    assert_eq!(report.pointer_axes_observed, 1);
    assert_eq!(report.chrome_events_consumed, 3); // release suppressed at first capture visit
    assert!(report.policy_inputs.is_empty());
    assert!(report.content_activations.is_empty());
    assert_eq!(report.pointer_routed, 0);
    assert_eq!(report.keys_routed, 0);
}
