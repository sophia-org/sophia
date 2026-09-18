#![cfg(test)]

use super::*;

include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/support/ordered_freeze.rs"
));

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
            .is_err_and(|error| error == PointerPreparationRefusal::NamespaceUnprepared)
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

#[test]
fn stamped_retirement_does_not_retire_an_identical_replacement() {
    let ns = NamespaceId::from_raw(1);
    let mut state = XInputAuthorityState::default();
    state.prepare_ordered_namespace(ns);
    let first = state
        .prepare_pointer_press(ns, 1, 0, implicit(10))
        .unwrap()
        .commit_stamped();
    assert!(first.automatic());
    state.ungrab_pointer(ns, 10);
    let second = state
        .prepare_pointer_press(ns, 1, 0, implicit(10))
        .unwrap()
        .commit_stamped();
    assert_eq!(first.recipient(), second.recipient());
    assert_ne!(first.stamp(), second.stamp());
    let pointer = crate::XCorePointerMapper::new();
    assert_eq!(
        state.retire_pointer_activation(ns, first.stamp(), 1, &pointer),
        PointerActivationRetirement::Replaced
    );
    assert_eq!(state.pointer_grab(ns), Some(second.recipient()));
    assert_eq!(
        state.retire_pointer_activation(ns, second.stamp(), 1, &pointer),
        PointerActivationRetirement::Retired
    );
    assert_eq!(
        state.retire_pointer_activation(ns, second.stamp(), 1, &pointer),
        PointerActivationRetirement::AlreadyAbsent
    );
}

#[test]
fn namespace_recreation_and_ordinary_activation_cannot_reuse_a_stamp() {
    let ns = NamespaceId::from_raw(1);
    let mut state = XInputAuthorityState::default();
    state.prepare_ordered_namespace(ns);
    let first = state
        .prepare_pointer_press(ns, 1, 0, implicit(10))
        .unwrap()
        .commit_stamped();
    state.cleanup_owner(10);
    assert!(!state.has_ordered_namespace(ns));
    state.activate_button(ns, 1, 0, implicit(10));
    let replacement = state
        .prepare_pointer_press(ns, 2, 0, implicit(20))
        .unwrap()
        .commit_stamped();
    assert_ne!(first.stamp(), replacement.stamp());
    assert_eq!(
        state.retire_pointer_activation(ns, first.stamp(), 1, &crate::XCorePointerMapper::new()),
        PointerActivationRetirement::Replaced
    );
    assert_eq!(state.pointer_grab(ns), Some(implicit(10)));
}

#[test]
fn one_implicit_activation_spans_buttons_and_waits_for_side_buttons() {
    let ns = NamespaceId::from_raw(1);
    let mut state = XInputAuthorityState::default();
    state.prepare_ordered_namespace(ns);
    let mut pointer = crate::XCorePointerMapper::new();
    pointer.map_evdev_button(272, true).unwrap();
    let first = state
        .prepare_pointer_press(ns, 1, 0, implicit(10))
        .unwrap()
        .commit_stamped();
    pointer.map_evdev_button(275, true).unwrap();
    let side = state
        .prepare_pointer_press(ns, 8, 0, implicit(20))
        .unwrap()
        .commit_stamped();
    assert_eq!(first.stamp(), side.stamp());
    pointer.map_evdev_button(272, false).unwrap();
    assert_eq!(pointer.state(), 0);
    assert_eq!(
        state.retire_pointer_activation(ns, first.stamp(), 1, &pointer),
        PointerActivationRetirement::StillRequiredByOtherButtons
    );
    pointer.map_evdev_button(275, false).unwrap();
    assert_eq!(
        state.retire_pointer_activation(ns, side.stamp(), 8, &pointer),
        PointerActivationRetirement::Retired
    );
}

#[test]
fn a_stamped_explicit_grab_is_not_an_automatic_cleanup_obligation() {
    let ns = NamespaceId::from_raw(1);
    let mut state = XInputAuthorityState::default();
    state.grab_pointer(ns, implicit(10)).unwrap();
    let reached = state
        .prepare_pointer_press(ns, 1, 0, implicit(20))
        .unwrap()
        .commit_stamped();
    assert!(!reached.automatic());
    assert_eq!(
        state.retire_pointer_activation(ns, reached.stamp(), 1, &crate::XCorePointerMapper::new()),
        PointerActivationRetirement::Explicit
    );
    assert_eq!(state.pointer_grab(ns), Some(implicit(10)));
}

#[test]
fn identity_exhaustion_refuses_before_an_automatic_grab_can_begin() {
    let ns = NamespaceId::from_raw(1);
    let mut state = XInputAuthorityState::default();
    state.prepare_ordered_namespace(ns);
    state.pointer_activation_high_water = u64::MAX;
    assert!(
        state
            .prepare_pointer_press(ns, 1, 0, implicit(10))
            .is_err_and(|error| error == PointerPreparationRefusal::IdentityExhausted)
    );
    assert_eq!(state.pointer_grab(ns), None);
    assert_eq!(state.pointer_activation_high_water, u64::MAX);
}

#[test]
fn interrupted_activation_is_unavailable_even_if_its_grab_fields_agree() {
    let ns = NamespaceId::from_raw(1);
    let mut state = XInputAuthorityState::default();
    state.prepare_ordered_namespace(ns);
    let reached = state
        .prepare_pointer_press(ns, 1, 0, implicit(10))
        .unwrap()
        .commit_stamped();
    // Staged interruption: the write-ahead marker, not a completed mutation.
    state.namespaces.get_mut(&ns).unwrap().pointer_activation = PointerActivationState::Changing;
    assert!(
        state
            .prepare_pointer_press(ns, 1, 0, implicit(10))
            .is_err_and(|error| error == PointerPreparationRefusal::ProvenanceUnavailable)
    );
    assert_eq!(
        state.retire_pointer_activation(ns, reached.stamp(), 1, &crate::XCorePointerMapper::new()),
        PointerActivationRetirement::Unavailable
    );
    assert_eq!(state.pointer_grab(ns), Some(reached.recipient()));
}
