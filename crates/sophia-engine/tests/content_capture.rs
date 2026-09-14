use sophia_engine::{
    ContentCaptureState, ContentPointerDisposition, PresentedContentBinding,
    PresentedContentTarget, resolve_content_pointer_event,
};
use sophia_protocol::{
    ContentAllocationId, ContentGrant, ContentLogicalRect, ContentOutputId, ContentPixelRect,
    DeviceId, InputEventKind, Point, SeatId,
};

fn target(candidate: u64, presentation: u64) -> PresentedContentTarget {
    PresentedContentTarget {
        grant: ContentGrant {
            connection_epoch: 3,
            content_grant_epoch: 4,
        },
        output: ContentOutputId {
            id: 1,
            generation: 2,
        },
        candidate_generation: candidate,
        presentation_epoch: presentation,
        interaction_generation: 1,
        allocation: ContentAllocationId {
            id: 5,
            generation: 6,
        },
        allocation_logical: ContentLogicalRect {
            x: 100,
            y: 20,
            width: 200,
            height: 40,
        },
        allocation_pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 300,
            height: 60,
        },
        target_id: 7,
        target_generation: 8,
        action_id: 9,
        bounds_px: ContentPixelRect {
            x: 30,
            y: 0,
            width: 60,
            height: 60,
        },
    }
}

fn binding(target: PresentedContentTarget) -> PresentedContentBinding {
    PresentedContentBinding {
        output: target.output,
        candidate_generation: target.candidate_generation,
        presentation_epoch: target.presentation_epoch,
        interaction_generation: target.interaction_generation,
        allocations: vec![(
            target.allocation,
            target.allocation_logical,
            target.allocation_pixel,
        )],
        targets: vec![target],
    }
}

fn button(button: u32, pressed: bool) -> InputEventKind {
    InputEventKind::PointerButton { button, pressed }
}

#[test]
fn exact_presented_target_activates_only_after_its_matching_release() {
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(2);
    let target = target(10, 11);
    let binding = binding(target.clone());
    let position = Some(Point { x: 130.0, y: 30.0 });
    let mut state = ContentCaptureState::default();
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            seat,
            device,
            button(0x110, true),
            position,
            Some(&binding),
            false,
        ),
        ContentPointerDisposition::Captured
    );
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            seat,
            device,
            button(0x110, false),
            position,
            Some(&binding),
            false,
        ),
        ContentPointerDisposition::Activated(target)
    );
}

#[test]
fn replacement_or_revocation_cancels_without_click_through() {
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(2);
    let old = binding(target(10, 11));
    let replacement = binding(target(12, 13));
    let position = Some(Point { x: 130.0, y: 30.0 });
    for binding in [Some(&replacement), None] {
        let mut state = ContentCaptureState::default();
        assert_eq!(
            resolve_content_pointer_event(
                &mut state,
                seat,
                device,
                button(0x110, true),
                position,
                Some(&old),
                false,
            ),
            ContentPointerDisposition::Captured
        );
        if binding.is_none() {
            state.revoke_targets();
        }
        assert_eq!(
            resolve_content_pointer_event(
                &mut state,
                seat,
                device,
                button(0x110, false),
                position,
                binding,
                false,
            ),
            ContentPointerDisposition::Cancelled
        );
    }
}

#[test]
fn authorized_application_capture_keeps_precedence_over_content() {
    let mut state = ContentCaptureState::default();
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            SeatId::from_raw(1),
            DeviceId::from_raw(2),
            button(0x110, true),
            Some(Point { x: 130.0, y: 30.0 }),
            Some(&binding(target(10, 11))),
            true,
        ),
        ContentPointerDisposition::Pass
    );
}

#[test]
fn later_application_capture_cannot_steal_a_content_owned_release() {
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(2);
    let target = target(10, 11);
    let binding = binding(target.clone());
    let position = Some(Point { x: 130.0, y: 30.0 });
    let mut state = ContentCaptureState::default();
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            seat,
            device,
            button(0x110, true),
            position,
            Some(&binding),
            false,
        ),
        ContentPointerDisposition::Captured
    );
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            seat,
            device,
            button(0x110, false),
            position,
            Some(&binding),
            true,
        ),
        ContentPointerDisposition::Activated(target)
    );
}

#[test]
fn suppression_overflow_quarantines_the_device_until_all_buttons_are_up() {
    let seat = SeatId::from_raw(1);
    let device = DeviceId::from_raw(2);
    let binding = binding(target(10, 11));
    let position = Some(Point { x: 130.0, y: 30.0 });
    let mut state = ContentCaptureState::default();
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            seat,
            device,
            button(0x110, true),
            position,
            Some(&binding),
            false,
        ),
        ContentPointerDisposition::Captured
    );
    for code in 0x120..=0x141 {
        assert_eq!(
            resolve_content_pointer_event(
                &mut state,
                seat,
                device,
                button(code, true),
                position,
                Some(&binding),
                false,
            ),
            ContentPointerDisposition::Consumed
        );
    }
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            seat,
            device,
            InputEventKind::PointerMotion,
            None,
            None,
            false,
        ),
        ContentPointerDisposition::Consumed
    );
    for code in 0x120..=0x141 {
        let _ = resolve_content_pointer_event(
            &mut state,
            seat,
            device,
            button(code, false),
            None,
            None,
            false,
        );
    }
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            seat,
            device,
            button(0x110, false),
            None,
            None,
            false,
        ),
        ContentPointerDisposition::Cancelled
    );
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            seat,
            device,
            InputEventKind::PointerMotion,
            None,
            None,
            false,
        ),
        ContentPointerDisposition::Pass
    );
}
