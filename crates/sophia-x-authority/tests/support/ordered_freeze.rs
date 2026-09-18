fn freeze_grab(owner: u64, pointer_mode: u8, keyboard_mode: u8) -> XActiveInputGrab {
    XActiveInputGrab {
        pointer_mode,
        keyboard_mode,
        ..implicit(owner)
    }
}

fn freeze_witness(observed: OrderedFreezeObservation) -> OrderedFreezeWitness {
    let OrderedFreezeObservation::Frozen(witness) = observed else {
        panic!("expected exact native freeze contributors, got {observed:?}");
    };
    witness
}

#[test]
fn freeze_witness_waits_for_both_exact_contributors_and_both_devices() {
    let namespace = NamespaceId::from_raw(101);
    let mut authority = XInputAuthorityState::default();
    authority
        .grab_pointer(namespace, freeze_grab(10, 0, 0))
        .unwrap();
    authority
        .grab_keyboard(namespace, freeze_grab(20, 0, 0))
        .unwrap();
    let pointer = freeze_witness(authority.ordered_pointer_freeze(namespace));
    let keyboard = freeze_witness(authority.ordered_keyboard_freeze(namespace));
    authority.allow_events(namespace, 99, 6).unwrap();
    assert_eq!(
        authority.check_ordered_freeze(&pointer),
        OrderedFreezeProgress::Frozen
    );
    authority.allow_events(namespace, 10, 6).unwrap();
    assert!(authority.pointer_frozen(namespace));
    assert!(authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.check_ordered_freeze(&pointer),
        OrderedFreezeProgress::Frozen
    );
    assert_eq!(
        authority.check_ordered_freeze(&keyboard),
        OrderedFreezeProgress::Frozen
    );
    authority.allow_events(namespace, 20, 0).unwrap();
    assert!(!authority.pointer_frozen(namespace));
    assert!(authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.check_ordered_freeze(&pointer),
        OrderedFreezeProgress::Thawed
    );
    assert_eq!(
        authority.check_ordered_freeze(&keyboard),
        OrderedFreezeProgress::Frozen
    );
    authority.allow_events(namespace, 20, 3).unwrap();
    assert!(!authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.check_ordered_freeze(&keyboard),
        OrderedFreezeProgress::Thawed
    );
}

#[test]
fn async_replacement_does_not_clear_a_sibling_grabs_freeze() {
    let namespace = NamespaceId::from_raw(102);
    let mut authority = XInputAuthorityState::default();
    authority
        .grab_pointer(namespace, freeze_grab(10, 0, 0))
        .unwrap();
    authority
        .grab_keyboard(namespace, freeze_grab(20, 0, 0))
        .unwrap();
    let old = freeze_witness(authority.ordered_pointer_freeze(namespace));
    authority
        .grab_pointer(namespace, freeze_grab(10, 1, 1))
        .unwrap();
    assert!(authority.pointer_frozen(namespace));
    assert!(authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.check_ordered_freeze(&old),
        OrderedFreezeProgress::Invalidated
    );
    let current = freeze_witness(authority.ordered_pointer_freeze(namespace));
    authority.allow_events(namespace, 10, 6).unwrap();
    assert_eq!(
        authority.check_ordered_freeze(&current),
        OrderedFreezeProgress::Frozen
    );
    authority.allow_events(namespace, 20, 6).unwrap();
    assert_eq!(
        authority.check_ordered_freeze(&current),
        OrderedFreezeProgress::Thawed
    );
    assert_eq!(
        authority.check_ordered_freeze(&old),
        OrderedFreezeProgress::Invalidated
    );
}

#[test]
fn ungrab_removes_only_its_own_cross_device_contributions() {
    let namespace = NamespaceId::from_raw(103);
    let mut authority = XInputAuthorityState::default();
    authority
        .grab_pointer(namespace, freeze_grab(10, 0, 0))
        .unwrap();
    authority
        .grab_keyboard(namespace, freeze_grab(20, 0, 0))
        .unwrap();
    let old = freeze_witness(authority.ordered_keyboard_freeze(namespace));
    authority.ungrab_pointer(namespace, 10);
    assert!(authority.pointer_frozen(namespace));
    assert!(authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.check_ordered_freeze(&old),
        OrderedFreezeProgress::Invalidated
    );
    let current = freeze_witness(authority.ordered_keyboard_freeze(namespace));
    authority.ungrab_keyboard(namespace, 20);
    assert!(!authority.pointer_frozen(namespace));
    assert!(!authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.check_ordered_freeze(&current),
        OrderedFreezeProgress::Invalidated
    );
}

#[test]
fn cleanup_freeze_receipt_answers_only_exact_removed_contributor() {
    let namespace = NamespaceId::from_raw(104);
    let mut authority = XInputAuthorityState::default();
    authority
        .grab_pointer(namespace, freeze_grab(10, 0, 0))
        .unwrap();
    authority
        .grab_keyboard(namespace, freeze_grab(20, 0, 0))
        .unwrap();
    let pointer = authority
        .prepare_pointer_press(namespace, 1, 0, implicit(10))
        .unwrap()
        .commit_stamped()
        .stamp();
    let keyboard = authority
        .keyboard_activation(namespace)
        .unwrap()
        .unwrap()
        .stamp();
    let foreign = authority.cleanup_ordered_freeze_owner(namespace, 99);
    assert!(!foreign.answers_pointer(pointer));
    assert!(!foreign.answers_keyboard(keyboard));
    let removed = authority.cleanup_ordered_freeze_owner(namespace, 10);
    assert!(removed.answers_pointer(pointer));
    assert!(!removed.answers_keyboard(keyboard));
    assert!(authority.pointer_frozen(namespace));
    assert!(authority.keyboard_frozen(namespace));
    authority.cleanup_owner(10);
    assert!(authority.pointer_frozen(namespace));
    assert!(authority.keyboard_frozen(namespace));
    assert!(
        !authority
            .cleanup_ordered_freeze_owner(namespace, 10)
            .answers_pointer(pointer)
    );
    let removed_keyboard = authority.cleanup_ordered_freeze_owner(namespace, 20);
    assert!(removed_keyboard.answers_keyboard(keyboard));
    assert!(!removed_keyboard.answers_pointer(pointer));
    assert!(!authority.pointer_frozen(namespace));
    assert!(!authority.keyboard_frozen(namespace));
    authority.cleanup_owner(20);
    assert!(
        !authority
            .cleanup_ordered_freeze_owner(namespace, 20)
            .answers_keyboard(keyboard)
    );
    authority
        .grab_pointer(namespace, freeze_grab(10, 0, 0))
        .unwrap();
    let replacement = authority
        .prepare_pointer_press(namespace, 1, 0, implicit(10))
        .unwrap()
        .commit_stamped()
        .stamp();
    assert!(!removed.answers_pointer(replacement));
    assert!(
        !authority
            .cleanup_ordered_freeze_owner(namespace, 10)
            .answers_pointer(pointer)
    );
}

#[test]
fn replay_and_single_event_modes_do_not_issue_persistent_ordered_thaw() {
    for mode in [1, 2, 4, 5, 7] {
        let namespace = NamespaceId::from_raw(105);
        let mut authority = XInputAuthorityState::default();
        authority
            .grab_pointer(namespace, freeze_grab(10, 0, 0))
            .unwrap();
        let witness = freeze_witness(if mode <= 2 {
            authority.ordered_pointer_freeze(namespace)
        } else {
            authority.ordered_keyboard_freeze(namespace)
        });
        authority.allow_events(namespace, 10, mode).unwrap();
        assert_eq!(
            authority.check_ordered_freeze(&witness),
            OrderedFreezeProgress::Invalidated
        );
        // An additional async operation cannot restamp an already unsupported
        // transition into a proof that the original event may now execute.
        authority.allow_events(namespace, 10, 6).unwrap();
        assert_eq!(
            authority.check_ordered_freeze(&witness),
            OrderedFreezeProgress::Invalidated
        );
    }
}

#[test]
fn freeze_provenance_exhaustion_and_interrupted_activation_fail_closed() {
    let namespace = NamespaceId::from_raw(106);
    let mut authority = XInputAuthorityState::default();
    assert_eq!(
        authority.ordered_pointer_freeze(namespace),
        OrderedFreezeObservation::Unavailable
    );
    authority.pointer_activation_high_water = u64::MAX;
    authority
        .grab_pointer(namespace, freeze_grab(10, 0, 0))
        .unwrap();
    assert!(authority.pointer_frozen(namespace));
    assert!(authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.ordered_pointer_freeze(namespace),
        OrderedFreezeObservation::Unavailable
    );
    assert_eq!(
        authority.ordered_keyboard_freeze(namespace),
        OrderedFreezeObservation::Unavailable
    );
    authority.ungrab_pointer(namespace, 10);
    assert_eq!(
        authority.ordered_pointer_freeze(namespace),
        OrderedFreezeObservation::Ready
    );
    authority
        .grab_keyboard(namespace, freeze_grab(20, 0, 0))
        .unwrap();
    let witness = freeze_witness(authority.ordered_keyboard_freeze(namespace));
    authority
        .namespaces
        .get_mut(&namespace)
        .unwrap()
        .keyboard_activation = KeyboardActivationState::Changing;
    assert_eq!(
        authority.check_ordered_freeze(&witness),
        OrderedFreezeProgress::Invalidated
    );
}

#[test]
fn newly_frozen_sibling_and_security_epoch_cannot_thaw_old_witness() {
    let namespace = NamespaceId::from_raw(107);
    let mut authority = XInputAuthorityState::default();
    authority
        .grab_pointer(namespace, freeze_grab(10, 0, 0))
        .unwrap();
    let witness = freeze_witness(authority.ordered_keyboard_freeze(namespace));
    authority.allow_events(namespace, 10, 6).unwrap();
    assert_eq!(
        authority.check_ordered_freeze(&witness),
        OrderedFreezeProgress::Thawed
    );
    authority
        .grab_keyboard(namespace, freeze_grab(20, 0, 0))
        .unwrap();
    assert_eq!(
        authority.check_ordered_freeze(&witness),
        OrderedFreezeProgress::Invalidated
    );
    authority.advance_security_epoch();
    assert!(!authority.pointer_frozen(namespace));
    assert!(!authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.check_ordered_freeze(&witness),
        OrderedFreezeProgress::Invalidated
    );
}

#[test]
fn identical_replacement_cannot_supply_thaw_for_an_earlier_activation() {
    let namespace = NamespaceId::from_raw(109);
    let mut authority = XInputAuthorityState::default();
    let grab = freeze_grab(10, 0, 0);
    authority.grab_pointer(namespace, grab).unwrap();
    let pointer = freeze_witness(authority.ordered_pointer_freeze(namespace));
    authority.grab_pointer(namespace, grab).unwrap();
    authority.allow_events(namespace, 10, 6).unwrap();
    assert_eq!(
        authority.check_ordered_freeze(&pointer),
        OrderedFreezeProgress::Invalidated
    );
    authority.grab_keyboard(namespace, grab).unwrap();
    let keyboard = freeze_witness(authority.ordered_keyboard_freeze(namespace));
    authority.grab_keyboard(namespace, grab).unwrap();
    authority.allow_events(namespace, 10, 6).unwrap();
    assert_eq!(
        authority.check_ordered_freeze(&keyboard),
        OrderedFreezeProgress::Invalidated
    );
}

#[test]
fn passive_release_removes_its_cross_device_freeze_without_answering_thaw() {
    let namespace = NamespaceId::from_raw(108);
    let mut authority = XInputAuthorityState::default();
    authority
        .grab_key(
            namespace,
            XPassiveInputGrab {
                owner: 10,
                window: XResourceId::new(10, 1),
                detail: 38,
                modifiers: 0,
                owner_events: false,
                pointer_mode: 0,
                keyboard_mode: 0,
                event_mask: 1,
            },
        )
        .unwrap();
    authority.activate_key(namespace, 38, 0).unwrap();
    let witness = freeze_witness(authority.ordered_pointer_freeze(namespace));
    authority.release_key(namespace, 38);
    assert!(!authority.pointer_frozen(namespace));
    assert!(!authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.check_ordered_freeze(&witness),
        OrderedFreezeProgress::Invalidated
    );
    authority.activate_button(namespace, 1, 0, freeze_grab(10, 0, 0));
    let witness = freeze_witness(authority.ordered_keyboard_freeze(namespace));
    authority.release_button(namespace, 1, true);
    assert!(!authority.pointer_frozen(namespace));
    assert!(!authority.keyboard_frozen(namespace));
    assert_eq!(
        authority.check_ordered_freeze(&witness),
        OrderedFreezeProgress::Invalidated
    );
}
