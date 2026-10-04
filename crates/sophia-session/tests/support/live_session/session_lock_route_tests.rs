//! Physical input while the session is locked (t292, t293 review finding 2):
//! keys reach the lock's secret and nothing else, and no report, metric or
//! coverage record counts or times them.

use super::*;
use sophia_engine::{LauncherKeyboard, WmShortcutOutput, WmShortcutRegistry, WmShortcutRouter};

fn desktop_shortcuts() -> WmShortcutRouter {
    let bindings =
        [(1, 1), (2, 16)].map(|(action, keycode)| sophia_protocol::WmBindingRegistration {
            action: WmActionId::from_raw(action),
            keycode,
            modifiers: WmModifierMask {
                bits: WmModifierMask::SUPER,
            },
        });
    WmShortcutRouter::new(
        WmShortcutRegistry::new(
            &bindings,
            sophia_protocol::WmCapabilities::all_supported(),
            1,
            sophia_protocol::WmChromePolicy::default(),
        )
        .unwrap(),
    )
}

fn launcher_keyboard() -> LauncherKeyboard {
    LauncherKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap()
}

#[test]
fn lock_boundary_forgets_shortcuts_but_keeps_cancelled_terminals() {
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(1);
    let mut shortcuts = desktop_shortcuts();
    shortcuts.set_action_lifecycles(&[sophia_protocol::PolicyActionLifecycleInterest {
        action: WmActionId::from_raw(1),
        held_ms: 150,
    }]);
    let mut launcher = launcher_keyboard();
    let mut modifiers = XCoreKeyboardMapper::new();
    assert!(
        shortcuts
            .key_event(seat, device, 125, true, 1)
            .accept()
            .is_empty()
    );
    let opening = shortcuts.key_event(seat, device, 1, true, 2).accept();
    assert!(!opening.is_empty());
    launcher.observe(125, true, false);
    shortcuts.cancel_all_chords();
    crate::live_session::locked::reset_desktop_keyboard_for_lock(
        seat,
        Some(&mut shortcuts),
        &mut modifiers,
        &mut launcher,
    );
    let terminal = shortcuts.take_outputs();
    assert!(matches!(
        terminal.as_slice(),
        [WmShortcutOutput::Chord(
            sophia_engine::WmChordEvent::Ended {
                end: sophia_protocol::PolicyChordEnd::Cancelled,
                ..
            }
        )]
    ));
    assert_eq!(shortcuts.modifier_mask(seat).bits, 0);
    assert!(!shortcuts.seat_uncertain(seat));
    for keycode in [1, 125] {
        assert!(
            shortcuts
                .key_event(seat, device, keycode, false, 3)
                .accept()
                .is_empty()
        );
    }
    assert!(
        shortcuts
            .key_event(seat, device, 16, true, 4)
            .accept()
            .is_empty()
    );
    assert!(
        shortcuts
            .key_event(seat, device, 16, false, 5)
            .accept()
            .is_empty()
    );
    assert!(
        shortcuts
            .key_event(seat, device, 125, true, 6)
            .accept()
            .is_empty()
    );
    assert!(
        !shortcuts
            .key_event(seat, device, 16, true, 7)
            .accept()
            .is_empty()
    );
}

#[test]
fn lock_boundary_clears_all_desktop_modifier_views_but_preserves_toggles() {
    let seat = SeatId::from_raw(1);
    let mut shortcuts = desktop_shortcuts();
    let mut launcher = launcher_keyboard();
    let mut modifiers = XCoreKeyboardMapper::with_locks(true, true);
    for (i, keycode) in [42, 54, 29, 97, 56, 100, 125, 126].into_iter().enumerate() {
        assert!(
            shortcuts
                .key_event(seat, DeviceId::from_raw(1 + i as u64 % 2), keycode, true, 1)
                .accept()
                .is_empty()
        );
        modifiers.map_evdev_key(keycode, true);
        launcher.observe(keycode, true, false);
    }
    crate::live_session::locked::reset_desktop_keyboard_for_lock(
        seat,
        Some(&mut shortcuts),
        &mut modifiers,
        &mut launcher,
    );
    assert_eq!(shortcuts.modifier_mask(seat).bits, 0);
    assert_eq!(modifiers.modifier_mask(), 2 | 16);
    assert!(!launcher.command_modifier_active());
    assert_eq!(launcher.observe(30, true, true).0.as_deref(), Some("a"));
}

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

#[test]
fn unlock_keeps_lock_time_keys_private_and_routes_plain_keys_normally() {
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(1);
    let mut shortcuts = desktop_shortcuts();
    let mut launcher = launcher_keyboard();
    let mut modifiers = XCoreKeyboardMapper::new();
    // The lock trigger has reached the desktop's two keyboard views.
    for keycode in [125, 1] {
        let _ = shortcuts.key_event(seat, device, keycode, true, 1).accept();
        launcher.observe(keycode, true, false);
    }
    shortcuts.cancel_all_chords();
    crate::live_session::locked::reset_desktop_keyboard_for_lock(
        seat,
        Some(&mut shortcuts),
        &mut modifiers,
        &mut launcher,
    );
    assert_eq!(shortcuts.modifier_mask(seat).bits, 0);
    assert!(!launcher.command_modifier_active());

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
    let (input_sender, input_receiver) = sync_channel(16);
    let (mut key_repeat, key_repeat_map) = test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = crate::emergency_input::EmergencyChordState::awaiting_arm();
    let mut virtual_terminal = crate::session_keyboard::VirtualTerminalChordState::default();
    let mut coverage = PhysicalKeyboardCoverage::default();
    let mut next_delivery = 1;
    // Trigger releases and a lock-toggle change belong only to the lock.
    // Q remains down when authentication finishes.
    let events = [
        (125, false),
        (1, false),
        (58, true),
        (58, false),
        (16, true),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (keycode, pressed))| packet(i as u64 + 2, InputEventKind::Key { keycode, pressed }))
    .collect();
    let report = crate::live_session::locked::route_locked_input(
        events,
        &mut lock,
        &mut client_keys,
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &mut emergency,
        &mut virtual_terminal,
        &mut coverage,
        &mut next_delivery,
        10,
    )
    .unwrap();
    assert_eq!(report.keys_observed, 0);
    assert_eq!(report.events, 0);
    assert!(input_receiver.try_recv().is_err());
    crate::live_session::locked::reset_desktop_keyboard_for_lock(
        seat,
        Some(&mut shortcuts),
        &mut modifiers,
        &mut launcher,
    );
    assert_eq!(
        modifiers.modifier_mask(),
        0,
        "private CapsLock was not imported"
    );
    assert_eq!(launcher.observe(16, true, true).0.as_deref(), Some("q"));

    let surface = SurfaceId::new(41, 1);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    };
    let committed = [CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry,
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            Size {
                width: 640,
                height: 480,
            },
        ),
        damage: Region::single(geometry),
    }];
    let mut focus = InputFocusState::new();
    focus.focus_surface(seat, surface, &committed);
    let events = [
        (16, false),
        (125, false),
        (1, false),
        (16, true),
        (16, false),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (keycode, pressed))| packet(i as u64 + 20, InputEventKind::Key { keycode, pressed }))
    .collect();
    let report = route_input_events(
        events,
        &focus,
        &committed,
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        &mut virtual_terminal,
        &mut coverage,
        Some(&mut shortcuts),
        &mut SessionPointerPlacement::default(),
        false,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut next_delivery,
        30,
        None,
        None,
        None,
    )
    .unwrap();
    assert!(
        report.policy_inputs.is_empty(),
        "plain Q must not match Super+Q"
    );
    let delivered: Vec<_> = input_receiver
        .try_iter()
        .map(|input| input.request.kind)
        .collect();
    assert_eq!(
        delivered,
        vec![
            InputEventKind::Key {
                keycode: 16,
                pressed: true
            },
            InputEventKind::Key {
                keycode: 16,
                pressed: false
            },
        ],
        "only fresh desktop presses and their paired releases reach the client"
    );
    assert_eq!(modifiers.modifier_mask(), 0);
}
