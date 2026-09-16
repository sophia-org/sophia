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
        continuity: sophia_engine::ContentTargetContinuity::mint(),
        scale_generation: 1,
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
        transform: sophia_engine::PresentedContentTransform {
            viewport: sophia_protocol::Rect {
                x: 0,
                y: 0,
                width: 2000,
                height: 1000,
            },
            layout_generation: 1,
        },
        authority_current: true,
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

fn click_part(
    state: &mut ContentCaptureState,
    binding: &PresentedContentBinding,
    point: Point,
    pressed: bool,
) -> ContentPointerDisposition {
    resolve_content_pointer_event(
        state,
        SeatId::from_raw(1),
        DeviceId::from_raw(2),
        button(0x110, pressed),
        Some(point),
        Some(binding),
        false,
    )
}

#[test]
fn content_coordinates_translate_once_for_nonzero_and_negative_output_origins() {
    // The fixture allocation is 200 logical / 300 physical: the target at
    // pixel30 starts at logical120. Translation must not scale a second time.
    for (x, y) in [(2560, 0), (-1920, -700), (0, 0)] {
        let mut b = binding(target(10, 11));
        b.transform.viewport.x = x;
        b.transform.viewport.y = y;
        let point = Point {
            x: f64::from(x) + 130.0,
            y: f64::from(y) + 30.0,
        };
        let mut state = ContentCaptureState::default();
        assert_eq!(
            click_part(&mut state, &b, point, true),
            ContentPointerDisposition::Captured
        );
        assert_eq!(
            click_part(&mut state, &b, point, false),
            ContentPointerDisposition::Activated(b.targets[0].clone())
        );
    }
}

#[test]
fn topology_only_change_and_cross_output_release_cancel_without_clickthrough() {
    for cross_output in [false, true] {
        let original = binding(target(10, 11));
        let point = Point { x: 130.0, y: 30.0 };
        let mut state = ContentCaptureState::default();
        assert_eq!(
            click_part(&mut state, &original, point, true),
            ContentPointerDisposition::Captured
        );
        let mut changed = original.clone();
        if cross_output {
            changed.output.id += 1;
            changed.targets[0].output = changed.output;
        } else {
            changed.transform.layout_generation += 1;
        }
        assert_eq!(
            click_part(&mut state, &changed, point, false),
            ContentPointerDisposition::Cancelled
        );
    }
}

#[test]
fn stale_known_shell_consumes_new_sequence_even_after_projection_disappears() {
    let mut b = binding(target(10, 11));
    b.authority_current = false;
    let point = Point { x: 130.0, y: 30.0 };
    let mut state = ContentCaptureState::default();
    assert_eq!(
        click_part(&mut state, &b, point, true),
        ContentPointerDisposition::Consumed
    );
    assert_eq!(
        resolve_content_pointer_event(
            &mut state,
            SeatId::from_raw(1),
            DeviceId::from_raw(2),
            button(0x110, false),
            Some(point),
            None,
            false
        ),
        ContentPointerDisposition::Consumed
    );
}

#[test]
fn nonfinite_or_other_output_points_cannot_mint_content_capture() {
    let b = binding(target(10, 11));
    for x in [f64::NAN, f64::INFINITY, -1.0, 2560.0] {
        assert_eq!(
            click_part(
                &mut ContentCaptureState::default(),
                &b,
                Point { x, y: 30.0 },
                true
            ),
            ContentPointerDisposition::Pass
        );
    }
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

#[test]
fn equivalent_refresh_activates_current_frame_not_pressed_frame() {
    let mut b = binding(target(10, 11));
    let mut state = ContentCaptureState::default();
    let point = Point { x: 130.0, y: 30.0 };
    assert_eq!(
        click_part(&mut state, &b, point, true),
        ContentPointerDisposition::Captured
    );
    for generation in 12..20 {
        let mut next = b.clone();
        next.candidate_generation = generation;
        next.presentation_epoch = generation + 1;
        next.targets[0].candidate_generation = generation;
        next.targets[0].presentation_epoch = generation + 1;
        sophia_engine::reconcile_content_continuity(Some(&b), &mut next);
        assert_eq!(next.targets[0].continuity, b.targets[0].continuity);
        b = next;
    }
    assert_eq!(
        click_part(&mut state, &b, point, false),
        ContentPointerDisposition::Activated(b.targets[0].clone())
    );
}

#[test]
fn presentation_transitions_break_continuity_even_without_intervening_input() {
    let point = Point { x: 130.0, y: 30.0 };
    for change in 0..12 {
        let b = binding(target(10, 11));
        let mut state = ContentCaptureState::default();
        assert_eq!(
            click_part(&mut state, &b, point, true),
            ContentPointerDisposition::Captured
        );
        let mut changed = b.clone();
        match change {
            0 => changed.targets.clear(),
            1 => changed.authority_current = false,
            2 => changed.targets[0].action_id += 1,
            3 => changed.targets[0].target_generation += 1,
            4 => changed.targets[0].bounds_px.x += 1,
            5 => changed.targets[0].scale_generation += 1,
            6 => changed.targets[0].allocation.generation += 1,
            7 => changed.targets[0].grant.content_grant_epoch += 1,
            8 => changed.transform.layout_generation += 1,
            9 => changed.targets[0].interaction_generation += 1,
            10 => changed.targets[0].allocation_logical.width += 1,
            11 => changed.targets[0].output.generation += 1,
            _ => unreachable!(),
        }
        sophia_engine::reconcile_content_continuity(Some(&b), &mut changed);
        let mut restored = b.clone();
        sophia_engine::reconcile_content_continuity(Some(&changed), &mut restored);
        assert_ne!(
            restored.targets[0].continuity, b.targets[0].continuity,
            "change {change}"
        );
        assert_eq!(
            click_part(&mut state, &restored, point, false),
            ContentPointerDisposition::Cancelled,
            "change {change}"
        );
    }
}
