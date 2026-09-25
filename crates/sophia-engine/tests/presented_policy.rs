use sophia_engine::{PolicyInputCapture, PolicyInputDisposition, PolicyPointerHit};
use sophia_engine::{PolicyPresentationCompletion, PresentedPolicyState};
use sophia_protocol::*;

fn publication(generation: u64) -> PolicyPresentation {
    PolicyPresentation {
        generation,
        keyboard_output: Some(OutputId::from_raw(1)),
        outputs: [1, 2]
            .map(|id| PolicyPresentationOutput {
                output: OutputId::from_raw(id),
                generation: 7,
                coverage: Rect {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                mode: PolicyPresentationMode::ReplaceApplications,
            })
            .to_vec(),
        instances: vec![],
        regions: vec![],
        bindings: vec![],
    }
}

fn completion(output: u64, generation: u64) -> PolicyPresentationCompletion {
    PolicyPresentationCompletion {
        owner_epoch: 3,
        publication_generation: generation,
        output: OutputId::from_raw(output),
        output_generation: 7,
    }
}

#[test]
fn modal_scope_waits_for_every_output_and_existing_capture() {
    let mut state = PresentedPolicyState::default();
    state.admit(3, publication(1)).unwrap();
    assert!(!state.modal_ready(false));
    assert!(state.complete(completion(1, 1)).is_some());
    assert!(!state.modal_ready(false));
    assert!(state.complete(completion(2, 1)).is_some());
    assert!(state.modal_ready(false));
    assert!(!state.modal_ready(true));
}

#[test]
fn repaint_retains_identity_but_new_publication_waits_for_completion() {
    let mut state = PresentedPolicyState::default();
    state.admit(3, publication(1)).unwrap();
    let receipt = state.complete(completion(1, 1)).unwrap();
    assert!(state.complete(completion(1, 1)).is_none());
    assert_eq!(state.output_receipt(OutputId::from_raw(1)), Some(receipt));
    state.admit(3, publication(2)).unwrap();
    assert!(state.complete(completion(1, 1)).is_none());
    assert!(state.output_receipt(OutputId::from_raw(1)).is_none());
    let replacement = state.complete(completion(1, 2)).unwrap();
    assert!(replacement.presentation_epoch > receipt.presentation_epoch);
}

#[test]
fn revocation_needs_no_credit_and_late_completion_cannot_restore_input() {
    let mut state = PresentedPolicyState::default();
    state.admit(3, publication(1)).unwrap();
    let presented = state.complete(completion(1, 1)).unwrap();
    let receipts = state.revoke();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].presentation_epoch, presented.presentation_epoch);
    assert_eq!(receipts[0].outcome, PolicyPresentationOutcome::Revoked);
    assert!(state.complete(completion(2, 1)).is_none());
    assert!(state.admit(3, publication(1)).is_err());
    assert!(state.revoke().is_empty());
    assert!(!state.modal_ready(false));
}

#[test]
fn unpresented_revocation_does_not_invent_a_receipt() {
    let mut state = PresentedPolicyState::default();
    state.admit(3, publication(1)).unwrap();
    assert!(state.revoke().is_empty());
    assert!(state.admit(2, publication(2)).is_err());
    state.admit(4, publication(1)).unwrap();
    assert!(state.complete(completion(1, 1)).is_none());
}

#[test]
fn catalog_removal_revokes_completed_keyboard_identity() {
    let mut state = PresentedPolicyState::default();
    let mut p = publication(1);
    let action = WmActionId::from_raw(9);
    let modifiers = WmModifierMask { bits: 0 };
    p.bindings.push(PolicyPresentationBinding {
        action,
        keycode: 28,
        modifiers,
    });
    state.admit(3, p).unwrap();
    state.complete(completion(1, 1));
    state.complete(completion(2, 1));
    let (_, identity) = state.keyboard_action(28, modifiers, false).unwrap();
    assert_eq!((identity.target_id, identity.target_generation), (0, 0));
    assert!(state.action_is_current(3, action, identity));
    assert!(!state.action_is_current(4, action, identity));
    let mut forged = identity;
    forged.presentation_epoch += 1;
    assert!(!state.action_is_current(3, action, forged));
    assert!(state.revalidate_actions(&[action]).is_empty());
    assert_eq!(state.revalidate_actions(&[]).len(), 2);
    assert!(!state.action_is_current(3, action, identity));
    assert!(state.keyboard_action(28, modifiers, false).is_none());
}

fn clickable() -> PresentedPolicyState {
    let mut state = PresentedPolicyState::default();
    let mut p = publication(1);
    p.regions.push(PolicyPresentationRegion {
        id: 4,
        generation: 2,
        output: OutputId::from_raw(1),
        geometry: Rect {
            x: 0,
            y: 0,
            width: 50,
            height: 50,
        },
        clip: Rect {
            x: 5,
            y: 5,
            width: 40,
            height: 40,
        },
        z_index: 1,
        role: PolicyPresentationRegionRole::Emphasis,
        action: Some(WmActionId::from_raw(9)),
    });
    state.admit(3, p).unwrap();
    state.complete(completion(1, 1));
    state.complete(completion(2, 1));
    state
}

#[test]
fn pointer_requires_same_device_target_and_live_release() {
    let mut state = clickable();
    let mut capture = PolicyInputCapture::default();
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(2);
    let hit = state.pointer_hit(OutputId::from_raw(1), Point { x: 10.0, y: 10.0 });
    assert!(matches!(hit, PolicyPointerHit::Action(_)));
    assert_eq!(
        capture.pointer(&state, seat, device, 1, true, hit, false),
        PolicyInputDisposition::Consumed
    );
    assert_eq!(
        capture.pointer(&state, seat, DeviceId::from_raw(3), 1, false, hit, false),
        PolicyInputDisposition::Pass
    );
    assert!(state.complete(completion(1, 1)).is_none());
    assert!(matches!(
        capture.pointer(&state, seat, device, 1, false, hit, false),
        PolicyInputDisposition::Action(_)
    ));
    capture.pointer(&state, seat, device, 1, true, hit, false);
    state.revoke();
    assert_eq!(
        capture.pointer(&state, seat, device, 1, false, hit, false),
        PolicyInputDisposition::Consumed
    );
}

#[test]
fn clipping_and_existing_application_capture_prevent_new_policy_capture() {
    let state = clickable();
    let mut capture = PolicyInputCapture::default();
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(2);
    assert_eq!(
        state.pointer_hit(OutputId::from_raw(1), Point { x: 1.0, y: 1.0 }),
        PolicyPointerHit::Blocked
    );
    let hit = state.pointer_hit(OutputId::from_raw(1), Point { x: 10.0, y: 10.0 });
    assert_eq!(
        capture.pointer(&state, seat, device, 1, true, hit, true),
        PolicyInputDisposition::Pass
    );
    assert_eq!(
        capture.pointer(&state, seat, device, 1, false, hit, false),
        PolicyInputDisposition::Pass
    );
}

#[test]
fn unbound_modal_key_release_remains_consumed_after_close() {
    let mut state = clickable();
    let mut capture = PolicyInputCapture::default();
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(2);
    let modifiers = WmModifierMask { bits: 0 };
    assert_eq!(
        capture.key(&state, seat, device, 30, true, modifiers, false),
        PolicyInputDisposition::Consumed
    );
    state.revoke();
    capture.revoke();
    assert_eq!(
        capture.key(&state, seat, device, 30, false, modifiers, false),
        PolicyInputDisposition::Consumed
    );
    assert_eq!(
        capture.key(&state, seat, device, 31, true, modifiers, false),
        PolicyInputDisposition::Pass
    );
}

#[test]
fn modal_pointer_waits_for_the_second_output_even_if_its_target_is_complete() {
    let ready = clickable();
    let (_, publication) = ready.publication().unwrap();
    let mut partial = PresentedPolicyState::default();
    partial.admit(3, publication.clone()).unwrap();
    partial.complete(completion(1, 1));
    let point = Point { x: 10.0, y: 10.0 };
    assert_eq!(
        partial.pointer_hit(OutputId::from_raw(1), point),
        PolicyPointerHit::Blocked
    );
    let PolicyPointerHit::Action(action) = ready.pointer_hit(OutputId::from_raw(1), point) else {
        panic!("ready target missing");
    };
    assert!(!partial.action_is_current(action.connection_epoch, action.action, action.identity));
    partial.complete(completion(2, 1));
    assert_eq!(
        partial.pointer_hit(OutputId::from_raw(1), point),
        PolicyPointerHit::Action(action)
    );
}

#[test]
fn protected_release_and_device_removal_cannot_leave_a_reusable_press() {
    let state = clickable();
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(2);
    let hit = state.pointer_hit(OutputId::from_raw(1), Point { x: 10.0, y: 10.0 });
    let mut capture = PolicyInputCapture::default();
    capture.pointer(&state, seat, device, 1, true, hit, false);
    capture.discard_release(
        seat,
        device,
        InputEventKind::PointerButton {
            button: 1,
            pressed: false,
        },
    );
    assert_eq!(
        capture.pointer(&state, seat, device, 1, false, hit, false),
        PolicyInputDisposition::Pass
    );
    capture.pointer(&state, seat, device, 1, true, hit, false);
    capture.remove_device(device);
    assert_eq!(
        capture.pointer(&state, seat, device, 1, false, hit, false),
        PolicyInputDisposition::Pass
    );
}

#[test]
fn reconnect_cannot_reuse_an_old_action_when_publication_and_target_ids_repeat() {
    let mut state = clickable();
    let point = Point { x: 10.0, y: 10.0 };
    let PolicyPointerHit::Action(old) = state.pointer_hit(OutputId::from_raw(1), point) else {
        panic!("target missing");
    };
    let publication = state.publication().unwrap().1.clone();
    state.revoke();
    state.admit(4, publication).unwrap();
    for output in [1, 2] {
        state.complete(PolicyPresentationCompletion {
            owner_epoch: 4,
            ..completion(output, 1)
        });
    }
    let PolicyPointerHit::Action(new) = state.pointer_hit(OutputId::from_raw(1), point) else {
        panic!("replacement target missing");
    };
    assert_eq!(
        new.identity.publication_generation,
        old.identity.publication_generation
    );
    assert_eq!(
        new.identity.target_generation,
        old.identity.target_generation
    );
    assert!(new.identity.presentation_epoch > old.identity.presentation_epoch);
    assert!(!state.action_is_current(3, old.action, old.identity));
    assert!(!state.action_is_current(4, old.action, old.identity));
    assert!(state.action_is_current(4, new.action, new.identity));
}
