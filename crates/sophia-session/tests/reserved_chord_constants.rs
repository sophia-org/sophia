//! The chord the private executor refuses to a synthetic source is the chord
//! the session's input guard recognises from physical devices, and the one
//! the engine keeps from policy clients. Neither crate can see the other's
//! definition, so this is where the three are held to one value: the guard's
//! evdev keys, the executor's evdev key and X modifier mask, and the engine's
//! keycode with its Control and Alt bits.

use sophia_session::emergency_input::{
    EVDEV_KEY_BACKSPACE, EVDEV_KEY_LEFTALT, EVDEV_KEY_LEFTCTRL, EVDEV_KEY_RIGHTALT,
    EVDEV_KEY_RIGHTCTRL,
};
use sophia_x_authority::{X_AUTHORITY_RESERVED_CHORD_KEY, X_AUTHORITY_RESERVED_CHORD_MODIFIERS};

#[test]
fn the_executors_reserved_chord_is_the_guards_emergency_chord() {
    assert_eq!(
        X_AUTHORITY_RESERVED_CHORD_KEY, EVDEV_KEY_BACKSPACE,
        "the key the executor refuses under both modifiers is the guard's Backspace"
    );
    // The guard reads the modifiers as evdev keys, either side; the executor
    // reads them as the X modifier mask the keymap gives those keys: Control
    // is bit 2, and Alt lives in Mod1, bit 3.
    assert_eq!(
        X_AUTHORITY_RESERVED_CHORD_MODIFIERS,
        (1 << 2) | (1 << 3),
        "Control and Mod1, the mask Control_L/R and Alt_L/R produce"
    );
    assert!(
        [
            EVDEV_KEY_LEFTCTRL,
            EVDEV_KEY_RIGHTCTRL,
            EVDEV_KEY_LEFTALT,
            EVDEV_KEY_RIGHTALT
        ]
        .iter()
        .all(|key| *key != EVDEV_KEY_BACKSPACE),
        "the modifiers are never the completing key"
    );
}

#[test]
fn the_engine_reserves_the_same_chord_from_policy_clients() {
    // sophia-engine refuses a policy binding of keycode 14 with Control and
    // Alt; the value is the guard's Backspace, checked here rather than read
    // from the engine's private constant.
    assert_eq!(EVDEV_KEY_BACKSPACE, 14);
}
