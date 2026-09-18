#![cfg(test)]

use super::*;
use XkbPhysicalKeyState as K;

#[test]
fn ordinary_key_down_is_not_a_modifier_and_lock_release_can_keep_modifiers() {
    let mut keyboard = XkbKeyboardState::default();
    assert_eq!(keyboard.physical_key_state(38), K::Released);
    assert_eq!(keyboard.map_evdev_key(30, true), Some((38, 0)));
    assert_eq!(keyboard.modifier_mask(), 0);
    assert_eq!(keyboard.physical_key_state(38), K::Held);
    assert_eq!(keyboard.map_evdev_key(30, false), Some((38, 0)));
    assert_eq!(keyboard.physical_key_state(38), K::Released);

    assert_eq!(keyboard.map_evdev_key(58, true), Some((66, 0)));
    assert_ne!(keyboard.modifier_mask() & 2, 0);
    assert_eq!(keyboard.map_evdev_key(58, false), Some((66, 2)));
    assert_eq!(keyboard.physical_key_state(66), K::Released);
    assert_ne!(keyboard.modifier_mask() & 2, 0);
}

#[test]
fn each_shift_contribution_survives_until_its_own_release() {
    let mut keyboard = XkbKeyboardState::default();
    assert_eq!(keyboard.map_evdev_key(42, true), Some((50, 0)));
    assert_eq!(keyboard.map_evdev_key(54, true), Some((62, 1)));
    assert_eq!(keyboard.map_evdev_key(42, false), Some((50, 1)));
    assert_eq!(keyboard.physical_key_state(50), K::Released);
    assert_eq!(keyboard.physical_key_state(62), K::Held);
    assert_eq!(keyboard.modifier_mask(), 1);
    assert_eq!(keyboard.map_evdev_key(54, false), Some((62, 1)));
    assert_eq!(keyboard.physical_key_state(62), K::Released);
    assert_eq!(keyboard.modifier_mask(), 0);
}

#[test]
fn physical_bitmap_covers_all_core_keycodes_without_aliasing() {
    let mut keyboard = XkbKeyboardState::default();
    for key in [8u8, 63, 64, 127, 128, 191, 192, 255] {
        assert!(keyboard.map_evdev_key(u32::from(key) - 8, true).is_some());
        assert_eq!(keyboard.physical_key_state(key), K::Held);
    }
    for key in [63u8, 127, 191, 255] {
        assert!(keyboard.map_evdev_key(u32::from(key) - 8, false).is_some());
        assert_eq!(keyboard.physical_key_state(key), K::Released);
    }
    for key in [8, 64, 128, 192] {
        assert_eq!(keyboard.physical_key_state(key), K::Held);
    }
}

#[test]
fn invalid_input_does_not_change_known_history() {
    let mut keyboard = XkbKeyboardState::default();
    keyboard.map_evdev_key(42, true).unwrap();
    for invalid in [248, u32::MAX] {
        assert_eq!(keyboard.map_evdev_key(invalid, false), None);
        assert_eq!(keyboard.physical_key_state(50), K::Held);
        assert_eq!(keyboard.modifier_mask(), 1);
    }
    for invalid in 0..8 {
        assert_eq!(keyboard.physical_key_state(invalid), K::InvalidKey);
    }
}

#[test]
fn unbalanced_xkb_edges_cannot_be_certified_by_the_last_edge() {
    let mut keyboard = XkbKeyboardState::default();
    keyboard.map_evdev_key(42, true).unwrap();
    keyboard.map_evdev_key(42, true).unwrap();
    keyboard.map_evdev_key(42, false).unwrap();
    assert_eq!(keyboard.physical_key_state(50), K::Unavailable);
    // Even balancing the earlier repeat cannot retroactively establish what
    // a private hold owned. Only the continuing exact history may answer it.
    keyboard.map_evdev_key(42, false).unwrap();
    assert_eq!(keyboard.physical_key_state(50), K::Unavailable);
    let mut unheld = XkbKeyboardState::default();
    unheld.map_evdev_key(30, false).unwrap();
    assert_eq!(unheld.physical_key_state(38), K::Unavailable);
}

#[test]
fn a_later_update_cannot_repair_an_unknown_xkb_transition() {
    let mut keyboard = XkbKeyboardState::default();
    keyboard.map_evdev_key(42, true).unwrap();
    // Staged refusal/latching control. The write-ahead placement is checked
    // by a separate disposable-source interruption after the C state update.
    keyboard.physical_known = false;
    assert_eq!(keyboard.physical_key_state(50), K::Unavailable);
    keyboard.map_evdev_key(42, false).unwrap();
    keyboard.map_evdev_key(30, true).unwrap();
    assert_eq!(keyboard.modifier_mask(), 0);
    assert_eq!(keyboard.physical_key_state(50), K::Unavailable);
    assert_eq!(keyboard.physical_key_state(38), K::Unavailable);
}

#[test]
fn unavailable_keyboard_cannot_retire_its_native_activation() {
    let mut authority = crate::XInputAuthorityState::default();
    let namespace = sophia_protocol::NamespaceId::from_raw(19);
    authority
        .grab_key(
            namespace,
            crate::XPassiveInputGrab {
                owner: 10,
                window: crate::XResourceId::new(10, 1),
                detail: 38,
                modifiers: crate::X_ANY_MODIFIER,
                owner_events: false,
                pointer_mode: 1,
                keyboard_mode: 1,
                event_mask: 3,
            },
        )
        .unwrap();
    authority.activate_key(namespace, 38, 0).unwrap();
    let activation = authority.keyboard_activation(namespace).unwrap().unwrap();
    let mut keyboard = XkbKeyboardState::default();
    keyboard.map_evdev_key(30, true).unwrap();
    keyboard.physical_known = false;
    keyboard.map_evdev_key(30, false).unwrap();
    assert_eq!(keyboard.modifier_mask(), 0);
    assert_eq!(
        authority.retire_keyboard_activation(namespace, activation.stamp(), 38, &keyboard),
        crate::KeyboardActivationRetirement::KeyboardUnavailable
    );
    assert_eq!(
        authority.keyboard_activation(namespace),
        Ok(Some(activation))
    );
}
