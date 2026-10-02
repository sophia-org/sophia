use super::*;
use sophia_engine::{WmChordEvent, WmShortcutRegistry, WmShortcutRouter};
use sophia_protocol::{
    InputEventKind, InputEventPacket, PolicyActionLifecycleInterest, PolicyChordEnd,
    WmBindingRegistration, WmCapabilities, WmModifierMask,
};

// Key routing's half of the chord lifecycle (t277): lifecycle events join the
// policy inputs at the boundary of the event that caused them, and the paths
// that end chords outside ordinary matching end them cancelled.

const NEXT: WmActionId = WmActionId::from_raw(186);
const PLAIN: WmActionId = WmActionId::from_raw(187);
const SEAT: SeatId = SeatId::from_raw(1);
const KEYBOARD: DeviceId = DeviceId::from_raw(1);
const OTHER_KEYBOARD: DeviceId = DeviceId::from_raw(3);
const CTRL: u32 = 29;
const ALT: u32 = 56;
const TAB: u32 = 15;
const F2: u32 = 60;
const F9: u32 = 67;

fn shortcuts() -> WmShortcutRouter {
    let bindings =
        [(NEXT, TAB, WmModifierMask::ALT), (PLAIN, F9, 0)].map(|(action, keycode, bits)| {
            WmBindingRegistration {
                action,
                keycode,
                modifiers: WmModifierMask { bits },
            }
        });
    let mut router = WmShortcutRouter::new(
        WmShortcutRegistry::new(
            &bindings,
            WmCapabilities::all_supported(),
            1,
            sophia_protocol::WmChromePolicy::default(),
        )
        .unwrap(),
    );
    router.set_action_lifecycles(&[PolicyActionLifecycleInterest {
        action: NEXT,
        held_ms: 150,
    }]);
    router
}

fn key(device: DeviceId, keycode: u32, pressed: bool) -> InputEventPacket {
    event(device, InputEventKind::Key { keycode, pressed })
}

fn event(device: DeviceId, kind: InputEventKind) -> InputEventPacket {
    InputEventPacket {
        serial: 1,
        seat: SEAT,
        device,
        time_msec: 1,
        kind,
        global_position: None,
        target_surface: None,
        local_position: None,
    }
}

/// Route one batch the way physical input does, with no captures present.
fn route(
    router: &mut WmShortcutRouter,
    virtual_terminal: &mut crate::session_keyboard::VirtualTerminalChordState,
    events: Vec<InputEventPacket>,
    mode: PhysicalInputRoutingMode,
) -> Vec<PhysicalPolicyInput> {
    let (input_sender, _input_receiver) = sync_channel(64);
    let mut modifiers = XCoreKeyboardMapper::new();
    let (mut key_repeat, key_repeat_map) = super::test_key_repeat_parts();
    let mut client_keys = SessionClientKeyState::default();
    let mut emergency = super::super::EmergencyChordState::awaiting_arm();
    let mut keyboard_coverage = PhysicalKeyboardCoverage::default();
    let mut pointer = SessionPointerPlacement::default();
    let mut next_delivery = 1;
    route_input_events(
        events,
        &InputFocusState::new(),
        &[],
        &[],
        &XAuthorityClientSurfaceRoutes::default(),
        &input_sender,
        &mut modifiers,
        &mut key_repeat,
        &key_repeat_map,
        &mut client_keys,
        &mut emergency,
        virtual_terminal,
        &mut keyboard_coverage,
        Some(router),
        &mut pointer,
        false,
        false,
        false,
        mode,
        &mut next_delivery,
        0,
        None,
        None,
        None,
    )
    .unwrap()
    .policy_inputs
}

fn chord_action(inputs: &[PhysicalPolicyInput]) -> sophia_engine::WmChordToken {
    match inputs.first() {
        Some(PhysicalPolicyInput::ChordAction(chorded)) => {
            assert_eq!(chorded.action, NEXT);
            assert!(chorded.chord.opens);
            chorded.chord.token
        }
        other => panic!("expected the chord's opening action, got {other:?}"),
    }
}

fn ended(token: sophia_engine::WmChordToken, end: PolicyChordEnd) -> PhysicalPolicyInput {
    PhysicalPolicyInput::Chord(WmChordEvent::Ended { token, end })
}

/// Action1, End1, Action2 stay in that order: the End joins the inputs at the
/// release that caused it, not after the next shortcut.
#[test]
fn lifecycle_events_join_the_inputs_at_their_event_boundary() {
    let mut router = shortcuts();
    let mut vt = Default::default();
    let inputs = route(
        &mut router,
        &mut vt,
        vec![
            key(KEYBOARD, ALT, true),
            key(KEYBOARD, TAB, true),
            key(KEYBOARD, TAB, false),
            key(KEYBOARD, ALT, false),
            key(KEYBOARD, F9, true),
        ],
        PhysicalInputRoutingMode::Full,
    );
    let token = chord_action(&inputs);
    assert_eq!(
        inputs[1..],
        [
            ended(token, PolicyChordEnd::Released),
            PhysicalPolicyInput::Action(PLAIN),
        ]
    );
}

/// Removing a keyboard ends the seat's chords even when that keyboard holds
/// nothing: an earlier trigger of its may belong to a modifier-held chord.
#[test]
fn removing_an_idle_keyboard_cancels_its_seats_chords() {
    let mut router = shortcuts();
    let mut vt = Default::default();
    let inputs = route(
        &mut router,
        &mut vt,
        vec![key(KEYBOARD, ALT, true), key(KEYBOARD, TAB, true)],
        PhysicalInputRoutingMode::Full,
    );
    let token = chord_action(&inputs);
    let inputs = route(
        &mut router,
        &mut vt,
        vec![event(OTHER_KEYBOARD, InputEventKind::DeviceRemoved)],
        PhysicalInputRoutingMode::Full,
    );
    assert_eq!(inputs, [ended(token, PolicyChordEnd::Cancelled)]);
}

/// CursorOnly matches nothing, but records every key:
/// back in Full, a modifier pressed meanwhile is known, a duplicate press is
/// no new action, and a consumed press keeps its consumed release.
#[test]
fn cursor_only_observes_keys_without_matching_them() {
    let mut router = shortcuts();
    let mut vt = Default::default();
    let inputs = route(
        &mut router,
        &mut vt,
        vec![key(KEYBOARD, F9, true)],
        PhysicalInputRoutingMode::Full,
    );
    assert_eq!(inputs, [PhysicalPolicyInput::Action(PLAIN)]);
    let inputs = route(
        &mut router,
        &mut vt,
        vec![
            key(KEYBOARD, ALT, true),
            key(KEYBOARD, TAB, true),
            key(KEYBOARD, F9, true),
        ],
        PhysicalInputRoutingMode::CursorOnly,
    );
    assert!(inputs.is_empty(), "{inputs:?}");
    assert_eq!(router.modifier_mask(SEAT).bits, WmModifierMask::ALT);
    let inputs = route(
        &mut router,
        &mut vt,
        vec![
            key(KEYBOARD, TAB, false),
            key(KEYBOARD, TAB, true),
            key(KEYBOARD, F9, false),
        ],
        PhysicalInputRoutingMode::Full,
    );
    // Alt was pressed during CursorOnly and is still down, so Tab is Alt+Tab.
    let token = chord_action(&inputs);
    assert_eq!(inputs.len(), 1);
    // Key routing in CursorOnly only observes: the chord's cancellation on
    // entering it belongs to the routing transition, tested with the session.
    let inputs = route(
        &mut router,
        &mut vt,
        vec![key(KEYBOARD, TAB, false)],
        PhysicalInputRoutingMode::CursorOnly,
    );
    assert!(inputs.is_empty(), "{inputs:?}");
    let _ = token;
}

/// A VT switch ends the seat's chords cancelled before its synthetic
/// modifier releases could end them released.
#[test]
fn a_virtual_terminal_switch_cancels_before_releasing_modifiers() {
    let mut router = shortcuts();
    let mut vt = crate::session_keyboard::VirtualTerminalChordState::default();
    let inputs = route(
        &mut router,
        &mut vt,
        vec![key(KEYBOARD, ALT, true), key(KEYBOARD, TAB, true)],
        PhysicalInputRoutingMode::Full,
    );
    let token = chord_action(&inputs);
    let inputs = route(
        &mut router,
        &mut vt,
        vec![key(KEYBOARD, CTRL, true), key(KEYBOARD, F2, true)],
        PhysicalInputRoutingMode::Full,
    );
    assert_eq!(inputs, [ended(token, PolicyChordEnd::Cancelled)]);
}
