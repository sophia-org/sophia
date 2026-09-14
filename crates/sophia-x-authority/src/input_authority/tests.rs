#![cfg(test)]

use super::*;

fn implicit(owner: u64) -> XActiveInputGrab {
    XActiveInputGrab {
        owner,
        window: XResourceId::new(owner, 1),
        owner_events: false,
        pointer_mode: 1,
        keyboard_mode: 1,
        event_mask: u16::MAX,
        xi_event_mask: [0; 8],
        xi_event_mask_words: 0,
        route_lease: None,
    }
}

#[test]
fn a_refused_binding_does_not_activate_the_selected_passive_grab() {
    let namespace = NamespaceId::from_raw(1);
    let mut state = XInputAuthorityState::default();
    state.prepare_ordered_namespace(namespace);
    state
        .namespaces
        .get_mut(&namespace)
        .unwrap()
        .buttons
        .push(XPassiveInputGrab {
            owner: 20,
            window: XResourceId::new(20, 1),
            detail: 1,
            modifiers: X_ANY_MODIFIER,
            owner_events: false,
            pointer_mode: 0,
            keyboard_mode: 0,
            event_mask: 4,
        });
    {
        let prepared = state
            .prepare_pointer_press(namespace, 1, 0, implicit(10))
            .unwrap();
        assert_eq!(prepared.recipient().owner, 20);
        // Binding refused: no call to commit.
    }
    assert!(state.pointer_grab(namespace).is_none());
    assert!(!state.pointer_frozen(namespace));
    assert!(!state.keyboard_frozen(namespace));
    let committed = state
        .prepare_pointer_press(namespace, 1, 0, implicit(10))
        .unwrap()
        .commit();
    assert_eq!(state.pointer_grab(namespace), Some(committed));
    assert_eq!(committed.owner, 20);
    assert!(state.pointer_frozen(namespace));
    assert!(state.keyboard_frozen(namespace));
}

#[test]
fn preparing_an_uninitialized_namespace_allocates_no_namespace() {
    let mut state = XInputAuthorityState::default();
    assert!(
        state
            .prepare_pointer_press(NamespaceId::from_raw(1), 1, 0, implicit(1))
            .is_none()
    );
    assert!(state.namespaces.is_empty());
}

#[test]
fn an_existing_grab_survives_another_button_and_implicit_release() {
    let namespace = NamespaceId::from_raw(1);
    let mut state = XInputAuthorityState::default();
    let explicit = implicit(30);
    state.grab_pointer(namespace, explicit).unwrap();
    let selected = state
        .prepare_pointer_press(namespace, 1, 0, implicit(10))
        .unwrap()
        .commit();
    assert_eq!(selected, explicit);
    state.release_button(namespace, 1, true);
    assert_eq!(state.pointer_grab(namespace), Some(explicit));
}

#[test]
fn side_buttons_keep_native_implicit_grab_debt_after_core_state_is_clear() {
    let mut pointer = crate::XCorePointerMapper::new();
    pointer.map_evdev_button(275, true).unwrap();
    pointer.map_evdev_button(272, true).unwrap();
    assert_eq!(pointer.map_evdev_button(272, false), Some((1, 1 << 8)));
    assert_eq!(pointer.state(), 0);
    assert!(pointer.button_is_pressed(8));
    assert!(!pointer.all_buttons_released());
    pointer.map_evdev_button(275, false).unwrap();
    assert!(pointer.all_buttons_released());
}
