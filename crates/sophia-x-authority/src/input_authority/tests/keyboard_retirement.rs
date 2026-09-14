#![cfg(test)]

use super::*;
use KeyboardActivationRetirement as R;

fn namespace() -> NamespaceId {
    NamespaceId::from_raw(19)
}

fn passive(key: u8) -> XPassiveInputGrab {
    XPassiveInputGrab {
        owner: 10,
        window: XResourceId::new(10, 1),
        detail: key,
        modifiers: X_ANY_MODIFIER,
        owner_events: false,
        pointer_mode: 1,
        keyboard_mode: 1,
        event_mask: 3,
    }
}

fn fixture(
    key: u8,
) -> (
    XInputAuthorityState,
    crate::XkbKeyboardState,
    KeyboardActivation,
) {
    let mut authority = XInputAuthorityState::default();
    authority.grab_key(namespace(), passive(key)).unwrap();
    // Actual native activation producer, not a constructed stamp or a common
    // admitted request. These controls cover the retirement component only.
    authority.activate_key(namespace(), key, 0).unwrap();
    let activation = authority.keyboard_activation(namespace()).unwrap().unwrap();
    let mut keyboard = crate::XkbKeyboardState::default();
    keyboard.map_evdev_key(u32::from(key) - 8, true).unwrap();
    (authority, keyboard, activation)
}

#[test]
fn passive_retirement_requires_the_actual_trigger_release_and_happens_once() {
    let (mut authority, mut keyboard, activation) = fixture(38);
    assert_eq!(keyboard.modifier_mask(), 0);
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
        R::StillRequiredByTrigger
    );
    assert_eq!(
        authority.keyboard_activation(namespace()),
        Ok(Some(activation))
    );
    keyboard.map_evdev_key(30, false).unwrap();
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
        R::Retired
    );
    assert_eq!(authority.keyboard_activation(namespace()), Ok(None));
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
        R::AlreadyAbsent
    );
}

#[test]
fn sibling_release_cannot_retire_the_trigger_even_after_the_trigger_is_up() {
    let (mut authority, mut keyboard, activation) = fixture(38);
    keyboard.map_evdev_key(31, true).unwrap();
    keyboard.map_evdev_key(30, false).unwrap();
    keyboard.map_evdev_key(31, false).unwrap();
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 39, &keyboard),
        R::StillRequiredByTrigger
    );
    assert_eq!(
        authority.keyboard_activation(namespace()),
        Ok(Some(activation))
    );
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
        R::Retired
    );
}

#[test]
fn retiring_a_released_lock_key_preserves_the_lock_modifier_and_pointer_grab() {
    let (mut authority, mut keyboard, activation) = fixture(66);
    let pointer = XActiveInputGrab {
        owner: 20,
        ..activation.recipient()
    };
    authority.grab_pointer(namespace(), pointer).unwrap();
    let pointer_stamp = authority.namespaces[&namespace()].pointer_activation;
    keyboard.map_evdev_key(58, false).unwrap();
    assert_eq!(keyboard.modifier_mask(), 2);
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 66, &keyboard),
        R::Retired
    );
    assert_eq!(keyboard.modifier_mask(), 2);
    assert_eq!(authority.pointer_grab(namespace()), Some(pointer));
    assert_eq!(
        authority.namespaces[&namespace()].pointer_activation,
        pointer_stamp
    );
}

#[test]
fn identical_replacement_and_foreign_namespace_cannot_answer_an_older_stamp() {
    let (mut authority, mut keyboard, activation) = fixture(38);
    keyboard.map_evdev_key(30, false).unwrap();
    authority.activate_key(namespace(), 38, 0).unwrap();
    let replacement = authority.keyboard_activation(namespace()).unwrap().unwrap();
    assert_eq!(replacement.recipient(), activation.recipient());
    assert_ne!(replacement.stamp(), activation.stamp());
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
        R::Replaced
    );
    let foreign = NamespaceId::from_raw(20);
    authority.grab_key(foreign, passive(38)).unwrap();
    authority.activate_key(foreign, 38, 0).unwrap();
    let foreign_activation = authority.keyboard_activation(foreign).unwrap();
    assert_eq!(
        authority.retire_keyboard_activation(foreign, replacement.stamp(), 38, &keyboard),
        R::Replaced
    );
    assert_eq!(
        authority.keyboard_activation(foreign),
        Ok(foreign_activation)
    );
    assert_eq!(
        authority.keyboard_activation(namespace()),
        Ok(Some(replacement))
    );
}

#[test]
fn missing_interrupted_or_inconsistent_activation_is_not_a_retirement() {
    let (mut authority, mut keyboard, activation) = fixture(38);
    keyboard.map_evdev_key(30, false).unwrap();
    authority
        .namespaces
        .get_mut(&namespace())
        .unwrap()
        .keyboard_activation = KeyboardActivationState::Changing;
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
        R::Unavailable
    );
    authority
        .namespaces
        .get_mut(&namespace())
        .unwrap()
        .keyboard_activation = KeyboardActivationState::Absent;
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
        R::Unavailable
    );
    authority.namespaces.remove(&namespace());
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
        R::Unavailable
    );
    assert!(authority.namespaces.is_empty());
}

#[test]
fn explicit_keyboard_grab_is_not_owned_by_a_key_release() {
    let (mut authority, mut keyboard, activation) = fixture(38);
    authority
        .grab_keyboard(namespace(), activation.recipient())
        .unwrap();
    let explicit = authority.keyboard_activation(namespace()).unwrap().unwrap();
    assert_eq!(explicit.trigger(), None);
    keyboard.map_evdev_key(30, false).unwrap();
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), explicit.stamp(), 38, &keyboard),
        R::Explicit
    );
    assert_eq!(
        authority.keyboard_activation(namespace()),
        Ok(Some(explicit))
    );
}

#[test]
fn frozen_or_leased_activation_requires_its_own_reconciliation() {
    let (mut authority, mut keyboard, activation) = fixture(38);
    keyboard.map_evdev_key(30, false).unwrap();
    for (pointer_frozen, keyboard_frozen) in [(true, false), (false, true)] {
        let state = authority.namespaces.get_mut(&namespace()).unwrap();
        state.pointer_frozen = pointer_frozen;
        state.keyboard_frozen = keyboard_frozen;
        assert_eq!(
            authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
            R::SynchronousUnproved
        );
        assert_eq!(
            authority.keyboard_activation(namespace()),
            Ok(Some(activation))
        );
    }
    for (pointer_mode, keyboard_mode) in [(0, 1), (1, 0)] {
        let state = authority.namespaces.get_mut(&namespace()).unwrap();
        state.pointer_frozen = false;
        state.keyboard_frozen = false;
        let grab = state.keyboard.as_mut().unwrap();
        grab.pointer_mode = pointer_mode;
        grab.keyboard_mode = keyboard_mode;
        assert_eq!(
            authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
            R::SynchronousUnproved
        );
    }
    let state = authority.namespaces.get_mut(&namespace()).unwrap();
    state.pointer_frozen = false;
    state.keyboard_frozen = false;
    state.keyboard.as_mut().unwrap().pointer_mode = 1;
    state.keyboard.as_mut().unwrap().keyboard_mode = 1;
    // A route lease can be present on a retained active grab. Retirement may
    // not silently clear it merely because this component can clear XKB.
    state.keyboard.as_mut().unwrap().route_lease =
        Some(sophia_protocol::ApplicationRouteLeaseIdentity {
            id: sophia_protocol::ApplicationRouteLeaseId::from_raw(3),
            seat: sophia_protocol::SeatId::from_raw(1),
            frontend_sequence: 4,
            control_epoch: 2,
        });
    assert_eq!(
        authority.retire_keyboard_activation(namespace(), activation.stamp(), 38, &keyboard),
        R::LeaseUnproved
    );
    assert!(
        authority
            .keyboard_grab(namespace())
            .unwrap()
            .route_lease
            .is_some()
    );
}
