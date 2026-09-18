#![cfg(test)]

use super::*;

fn namespace() -> NamespaceId {
    NamespaceId::from_raw(19)
}

fn active(owner: u64) -> XActiveInputGrab {
    XActiveInputGrab {
        owner,
        window: XResourceId::new(owner, 1),
        owner_events: false,
        pointer_mode: 1,
        keyboard_mode: 1,
        event_mask: 3,
        xi_event_mask: [0; 8],
        xi_event_mask_words: 0,
        route_lease: None,
    }
}

fn passive(owner: u64) -> XPassiveInputGrab {
    XPassiveInputGrab {
        owner,
        window: XResourceId::new(owner, 1),
        detail: 38,
        modifiers: X_ANY_MODIFIER,
        owner_events: false,
        pointer_mode: 1,
        keyboard_mode: 1,
        event_mask: 3,
    }
}

fn observed(state: &XInputAuthorityState) -> KeyboardActivation {
    state.keyboard_activation(namespace()).unwrap().unwrap()
}

#[test]
fn identical_keyboard_replacement_has_a_new_activation_name() {
    let mut state = XInputAuthorityState::default();
    state.grab_keyboard(namespace(), active(10)).unwrap();
    let first = observed(&state);
    assert_eq!(first.recipient(), active(10));
    assert_eq!(first.trigger(), None);
    state.grab_keyboard(namespace(), active(10)).unwrap();
    let second = observed(&state);
    assert_eq!(second.recipient(), first.recipient());
    assert_ne!(second.stamp(), first.stamp());
    state.ungrab_keyboard(namespace(), 10);
    assert_eq!(state.keyboard_activation(namespace()), Ok(None));
    state.grab_keyboard(namespace(), active(10)).unwrap();
    assert_ne!(observed(&state).stamp(), second.stamp());
}

#[test]
fn passive_keyboard_activation_records_the_actual_trigger_and_replacement() {
    let mut state = XInputAuthorityState::default();
    state.grab_key(namespace(), passive(10)).unwrap();
    assert_eq!(state.keyboard_activation(namespace()), Ok(None));
    assert_eq!(state.activate_key(namespace(), 39, 0), None);
    state.activate_key(namespace(), 38, 0).unwrap();
    let first = observed(&state);
    assert_eq!(first.trigger(), Some(38));
    assert_eq!(first.recipient(), active(10));
    state.release_key(namespace(), 39);
    assert_eq!(observed(&state), first);
    state.release_key(namespace(), 38);
    assert_eq!(state.keyboard_activation(namespace()), Ok(None));
    state.activate_key(namespace(), 38, 0).unwrap();
    let second = observed(&state);
    assert_ne!(first.stamp(), second.stamp());
    // Record what the existing ordinary producer actually does. This does
    // not bless it as private passive selection or change its routing choice.
    state.activate_key(namespace(), 38, 0).unwrap();
    assert_ne!(second.stamp(), observed(&state).stamp());
}

#[test]
fn removing_passive_policy_does_not_relabel_its_active_keyboard_grab() {
    let mut state = XInputAuthorityState::default();
    let policy = passive(10);
    state.grab_key(namespace(), policy).unwrap();
    state.activate_key(namespace(), 38, 0).unwrap();
    let first = observed(&state);
    state.ungrab_key(
        namespace(),
        10,
        policy.window,
        policy.detail,
        policy.modifiers,
    );
    assert_eq!(observed(&state), first);
    state.release_key(namespace(), 38);
    assert_eq!(state.keyboard_activation(namespace()), Ok(None));
    assert_eq!(state.activate_key(namespace(), 38, 0), None);
}

#[test]
fn namespace_recreation_cannot_revive_a_keyboard_activation_name() {
    let mut state = XInputAuthorityState::default();
    state.grab_keyboard(namespace(), active(10)).unwrap();
    let first = observed(&state);
    state.cleanup_owner(10);
    assert_eq!(
        state.keyboard_activation(namespace()),
        Err(KeyboardActivationRefusal::NamespaceUnprepared)
    );
    state.grab_keyboard(namespace(), active(10)).unwrap();
    assert_ne!(observed(&state).stamp(), first.stamp());
    state.advance_security_epoch();
    assert_eq!(state.keyboard_activation(namespace()), Ok(None));
    state.grab_keyboard(namespace(), active(10)).unwrap();
    assert_ne!(observed(&state).stamp(), first.stamp());
}

#[test]
fn cleanup_only_ends_the_keyboard_activation_owned_by_that_client() {
    let mut state = XInputAuthorityState::default();
    state.grab_key(namespace(), passive(20)).unwrap();
    state.grab_keyboard(namespace(), active(10)).unwrap();
    let first = observed(&state);
    state.cleanup_owner(30);
    assert_eq!(observed(&state), first);
    state.cleanup_owner(10);
    // Other retained passive policy keeps this namespace present.
    assert_eq!(state.keyboard_activation(namespace()), Ok(None));
    state.activate_key(namespace(), 38, 0).unwrap();
    assert_eq!(observed(&state).recipient().owner, 20);
    assert_ne!(observed(&state).stamp(), first.stamp());
}

#[test]
fn refused_keyboard_grab_preserves_its_existing_activation_name() {
    let mut state = XInputAuthorityState::default();
    state.grab_keyboard(namespace(), active(10)).unwrap();
    let first = observed(&state);
    let counter = state.keyboard_activation_high_water;
    assert_eq!(
        state.grab_keyboard(namespace(), active(20)),
        Err(XInputGrabError::AlreadyGrabbed)
    );
    let mut invalid = active(10);
    invalid.keyboard_mode = 2;
    assert_eq!(
        state.grab_keyboard(namespace(), invalid),
        Err(XInputGrabError::InvalidMode)
    );
    state.ungrab_keyboard(namespace(), 20);
    assert_eq!(observed(&state), first);
    assert_eq!(state.keyboard_activation_high_water, counter);
}

#[test]
fn exhausted_keyboard_names_preserve_ordinary_behavior_but_refuse_private_provenance() {
    let mut state = XInputAuthorityState {
        keyboard_activation_high_water: u64::MAX - 1,
        ..XInputAuthorityState::default()
    };
    state.grab_keyboard(namespace(), active(10)).unwrap();
    assert_eq!(observed(&state).stamp().serial, u64::MAX);
    state.grab_keyboard(namespace(), active(10)).unwrap();
    assert_eq!(state.keyboard_grab(namespace()), Some(active(10)));
    assert_eq!(state.keyboard_activation_high_water, u64::MAX);
    assert_eq!(
        state.keyboard_activation(namespace()),
        Err(KeyboardActivationRefusal::ProvenanceUnavailable)
    );
    state.ungrab_keyboard(namespace(), 10);
    assert_eq!(state.keyboard_activation(namespace()), Ok(None));
    state.grab_key(namespace(), passive(20)).unwrap();
    assert_eq!(state.activate_key(namespace(), 38, 0), Some(active(20)));
    assert_eq!(
        state.keyboard_activation(namespace()),
        Err(KeyboardActivationRefusal::ProvenanceUnavailable)
    );
}

#[test]
fn interrupted_keyboard_publication_is_unavailable_even_when_grab_fields_match() {
    let mut state = XInputAuthorityState::default();
    assert_eq!(
        state.keyboard_activation(namespace()),
        Err(KeyboardActivationRefusal::NamespaceUnprepared)
    );
    assert!(state.namespaces.is_empty());
    state.grab_keyboard(namespace(), active(10)).unwrap();
    // Staged state validates refusal, not the actual write-ahead placement.
    state
        .namespaces
        .get_mut(&namespace())
        .unwrap()
        .keyboard_activation = KeyboardActivationState::Changing;
    assert_eq!(state.keyboard_grab(namespace()), Some(active(10)));
    assert_eq!(
        state.keyboard_activation(namespace()),
        Err(KeyboardActivationRefusal::ProvenanceUnavailable)
    );
    state.advance_security_epoch();
    assert_eq!(state.keyboard_activation(namespace()), Ok(None));
}
