use sophia_engine::{
    ContentCaptureState, ContentPointerDisposition as Outcome, PresentedContentBinding,
    PresentedContentTarget, PresentedContentTransform, reconcile_content_continuity,
    resolve_content_pointer_stack,
};
use sophia_protocol::{
    ContentAllocationId, ContentGrant, ContentLogicalRect, ContentOutputId, ContentPixelRect,
    DeviceId, InputEventKind, Point, Rect, SeatId,
};

fn component(epoch: u64, x: i32) -> PresentedContentBinding {
    let grant = ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    };
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    let allocation = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    let logical = ContentLogicalRect {
        x,
        y: 0,
        width: 100,
        height: 40,
    };
    let pixel = ContentPixelRect {
        x: 0,
        y: 0,
        width: 200,
        height: 80,
    };
    let mut binding = PresentedContentBinding {
        grant,
        output,
        candidate_generation: 1,
        presentation_epoch: 1,
        interaction_generation: 1,
        transform: PresentedContentTransform {
            viewport: Rect {
                x: -500,
                y: 80,
                width: 500,
                height: 300,
            },
            layout_generation: 1,
        },
        authority_current: true,
        targets: vec![PresentedContentTarget {
            continuity: None,
            scale_generation: 1,
            grant,
            output,
            candidate_generation: 1,
            presentation_epoch: 1,
            interaction_generation: 1,
            allocation,
            allocation_logical: logical,
            allocation_pixel: pixel,
            target_id: 1,
            target_generation: 1,
            action_id: 1,
            bounds_px: pixel,
        }],
        allocations: vec![(allocation, logical, pixel)],
    };
    reconcile_content_continuity(None, &mut binding);
    binding
}

fn pointer(
    state: &mut ContentCaptureState,
    stack: &[PresentedContentBinding],
    x: f64,
    pressed: bool,
) -> Outcome {
    resolve_content_pointer_stack(
        state,
        SeatId::from_raw(1),
        DeviceId::from_raw(1),
        InputEventKind::PointerButton {
            button: 0x110,
            pressed,
        },
        Some(Point {
            x: x - 500.0,
            y: 90.0,
        }),
        stack,
        false,
    )
}

#[test]
fn overlapping_components_select_topmost_not_numeric_epoch_or_target_id() {
    let stack = [component(100, 0), component(2, 20)];
    let mut state = ContentCaptureState::default();
    assert_eq!(pointer(&mut state, &stack, 50.0, true), Outcome::Captured);
    let Outcome::Activated(target) = pointer(&mut state, &stack, 50.0, false) else {
        panic!("no activation")
    };
    assert_eq!(target.grant, stack[1].grant);
    assert_eq!(pointer(&mut state, &stack, 10.0, true), Outcome::Captured);
    let Outcome::Activated(target) = pointer(&mut state, &stack, 10.0, false) else {
        panic!("no bar activation")
    };
    assert_eq!(target.grant, stack[0].grant);
}

#[test]
fn occluding_component_cancels_lower_capture_without_activating_either() {
    let panel = component(100, 0);
    let mut state = ContentCaptureState::default();
    assert_eq!(
        pointer(&mut state, std::slice::from_ref(&panel), 50.0, true),
        Outcome::Captured
    );
    let stack = [panel, component(2, 20)];
    assert_eq!(pointer(&mut state, &stack, 50.0, false), Outcome::Cancelled);
    assert_eq!(pointer(&mut state, &stack, 50.0, false), Outcome::Consumed);
}

#[test]
fn removed_component_cannot_release_into_reused_ids_of_underlying_panel() {
    let panel = component(100, 0);
    let stack = [panel.clone(), component(2, 20)];
    let mut state = ContentCaptureState::default();
    assert_eq!(pointer(&mut state, &stack, 50.0, true), Outcome::Captured);
    assert_eq!(
        pointer(&mut state, &[panel], 50.0, false),
        Outcome::Cancelled
    );
}

#[test]
fn inert_and_stale_pixels_occlude_without_targets_and_suppress_release() {
    for stale in [false, true] {
        let mut overlay = component(2, 20);
        overlay.targets.clear();
        overlay.authority_current = !stale;
        if stale {
            overlay.allocations.clear();
        }
        let panel = component(100, 0);
        let stack = [panel.clone(), overlay];
        let mut state = ContentCaptureState::default();
        assert_eq!(pointer(&mut state, &stack, 50.0, true), Outcome::Consumed);
        // Even removing the overlay cannot hand its release to an application.
        assert_eq!(
            pointer(&mut state, &[panel], 50.0, false),
            Outcome::Consumed
        );
    }
}

#[test]
fn unrelated_component_repaint_preserves_exact_capture() {
    let panel = component(100, 0);
    let mut overlay = component(2, 200);
    let mut state = ContentCaptureState::default();
    assert_eq!(
        pointer(&mut state, &[panel.clone(), overlay.clone()], 50.0, true),
        Outcome::Captured
    );
    overlay.candidate_generation = 2;
    overlay.presentation_epoch = 2;
    let Outcome::Activated(target) = pointer(&mut state, &[panel.clone(), overlay], 50.0, false)
    else {
        panic!("capture lost")
    };
    assert_eq!(target.grant, panel.grant);
}

#[test]
fn contradictory_binding_cannot_lend_another_grants_target() {
    let mut binding = component(2, 0);
    binding.targets[0].grant = component(100, 0).grant;
    let mut state = ContentCaptureState::default();
    assert_eq!(
        pointer(&mut state, &[binding], 50.0, true),
        Outcome::Consumed
    );
}

#[test]
fn stale_transform_cannot_open_a_clickthrough_hole_on_a_relocated_output() {
    let mut old = component(2, 20);
    old.authority_current = false;
    // Session selected this output using its new topology. The retained pixels
    // still carry their old transform, so no new current target may be inferred.
    let mut state = ContentCaptureState::default();
    assert_eq!(pointer(&mut state, &[old], 600.0, true), Outcome::Consumed);
    assert_eq!(pointer(&mut state, &[], 600.0, false), Outcome::Consumed);
}
