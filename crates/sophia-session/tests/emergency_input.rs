use sophia_session::emergency_input::{
    EVDEV_KEY_BACKSPACE, EVDEV_KEY_LEFTALT, EVDEV_KEY_LEFTCTRL, EVDEV_KEY_RIGHTALT,
    EVDEV_KEY_RIGHTCTRL, EmergencyChordAction, EmergencyChordState,
};

fn press(state: &mut EmergencyChordState, keycode: u32) -> EmergencyChordAction {
    state.observe(keycode, true)
}

fn release(state: &mut EmergencyChordState, keycode: u32) -> EmergencyChordAction {
    state.observe(keycode, false)
}

#[test]
fn first_complete_chord_arms_only_after_full_release_and_second_triggers() {
    let mut state = EmergencyChordState::awaiting_arm();

    assert_eq!(
        press(&mut state, EVDEV_KEY_LEFTCTRL),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_LEFTALT),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
    assert!(!state.is_armed());

    assert_eq!(
        release(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
    assert_eq!(
        release(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
    assert_eq!(
        release(&mut state, EVDEV_KEY_LEFTALT),
        EmergencyChordAction::None
    );
    assert_eq!(
        release(&mut state, EVDEV_KEY_LEFTCTRL),
        EmergencyChordAction::Armed
    );
    assert!(state.is_armed());

    assert_eq!(
        press(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_RIGHTALT),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_RIGHTCTRL),
        EmergencyChordAction::Triggered
    );
}

#[test]
fn armed_state_triggers_in_any_press_order_and_ignores_unrelated_keys() {
    let mut state = EmergencyChordState::armed();

    assert_eq!(press(&mut state, 30), EmergencyChordAction::None);
    assert_eq!(
        press(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_RIGHTCTRL),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_LEFTALT),
        EmergencyChordAction::Triggered
    );
}

#[test]
fn partial_chords_and_repeats_do_not_trigger() {
    let mut state = EmergencyChordState::armed();

    assert_eq!(
        press(&mut state, EVDEV_KEY_LEFTCTRL),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
    assert_eq!(
        release(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
    assert_eq!(
        release(&mut state, EVDEV_KEY_LEFTCTRL),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_LEFTALT),
        EmergencyChordAction::None
    );
    assert_eq!(
        press(&mut state, EVDEV_KEY_BACKSPACE),
        EmergencyChordAction::None
    );
}

// Two keyboards on one seat. A chord is one device's, so keys held on two
// cannot be assembled into it, and a device that leaves takes its keys.

fn a() -> sophia_protocol::DeviceId {
    sophia_protocol::DeviceId::from_raw(257)
}

fn b() -> sophia_protocol::DeviceId {
    sophia_protocol::DeviceId::from_raw(258)
}

#[test]
fn a_chord_split_across_two_devices_is_not_a_chord() {
    let mut state = EmergencyChordState::armed();

    assert_eq!(
        state.observe_at_device(a(), EVDEV_KEY_LEFTCTRL, true),
        EmergencyChordAction::None
    );
    assert_eq!(
        state.observe_at_device(a(), EVDEV_KEY_LEFTALT, true),
        EmergencyChordAction::None
    );
    assert_eq!(
        state.observe_at_device(b(), EVDEV_KEY_BACKSPACE, true),
        EmergencyChordAction::None
    );
    assert_eq!(
        state.observe_at_device(b(), EVDEV_KEY_BACKSPACE, false),
        EmergencyChordAction::None
    );
    assert!(state.is_armed());
}

#[test]
fn a_whole_chord_on_one_device_triggers_while_another_holds_a_modifier() {
    let mut state = EmergencyChordState::armed();
    let _ = state.observe_at_device(a(), EVDEV_KEY_LEFTCTRL, true);

    assert_eq!(
        state.observe_at_device(b(), EVDEV_KEY_RIGHTCTRL, true),
        EmergencyChordAction::None
    );
    assert_eq!(
        state.observe_at_device(b(), EVDEV_KEY_RIGHTALT, true),
        EmergencyChordAction::None
    );
    assert_eq!(
        state.observe_at_device(b(), EVDEV_KEY_BACKSPACE, true),
        EmergencyChordAction::Triggered
    );
}

#[test]
fn a_device_that_leaves_takes_its_keys_with_it() {
    let mut state = EmergencyChordState::armed();
    let _ = state.observe_at_device(a(), EVDEV_KEY_LEFTCTRL, true);
    let _ = state.observe_at_device(a(), EVDEV_KEY_LEFTALT, true);

    assert_eq!(state.forget_device(a()), EmergencyChordAction::None);
    assert_eq!(
        state.observe_at_device(b(), EVDEV_KEY_BACKSPACE, true),
        EmergencyChordAction::None
    );
    // Its keys are gone, not merely hidden: the same keys pressed again on
    // a returned device start from nothing.
    assert_eq!(
        state.observe_at_device(a(), EVDEV_KEY_BACKSPACE, true),
        EmergencyChordAction::None
    );
}

#[test]
fn arming_waits_for_every_device_to_let_go() {
    let mut state = EmergencyChordState::awaiting_arm();
    let _ = state.observe_at_device(b(), EVDEV_KEY_RIGHTCTRL, true);
    let _ = state.observe_at_device(a(), EVDEV_KEY_LEFTCTRL, true);
    let _ = state.observe_at_device(a(), EVDEV_KEY_LEFTALT, true);
    assert_eq!(
        state.observe_at_device(a(), EVDEV_KEY_BACKSPACE, true),
        EmergencyChordAction::None
    );

    let _ = state.observe_at_device(a(), EVDEV_KEY_BACKSPACE, false);
    let _ = state.observe_at_device(a(), EVDEV_KEY_LEFTALT, false);
    assert_eq!(
        state.observe_at_device(a(), EVDEV_KEY_LEFTCTRL, false),
        EmergencyChordAction::None,
        "the other device still holds a chord key"
    );
    assert!(!state.is_armed());
    assert_eq!(
        state.observe_at_device(b(), EVDEV_KEY_RIGHTCTRL, false),
        EmergencyChordAction::Armed
    );
    assert!(state.is_armed());
}

#[test]
fn a_departure_is_the_release_that_completes_arming() {
    let mut state = EmergencyChordState::awaiting_arm();
    let _ = state.observe_at_device(a(), EVDEV_KEY_LEFTCTRL, true);
    let _ = state.observe_at_device(a(), EVDEV_KEY_LEFTALT, true);
    let _ = state.observe_at_device(a(), EVDEV_KEY_BACKSPACE, true);

    assert_eq!(state.forget_device(a()), EmergencyChordAction::Armed);
    assert!(state.is_armed());
}
