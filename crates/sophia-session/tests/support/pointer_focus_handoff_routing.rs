// Whether a left press opens an ordered focus handoff, and what happens to the
// press when nothing can close one.
//
// WHY THIS EXISTS. A press that opens a handoff is withheld until the handoff
// answers, so the click lands after the focus change rather than before it.
// Only a WM session answers. A session may legitimately run without one -- the
// QEMU pointer proof does, and so do the standalone, native and fallback
// profiles -- and there the handoff expired and took the button with it: the
// press was observed, never routed, and suppressed under neither recorded
// reason. These two controls pin both directions, because the fix must not be
// allowed to cost the WM case its ordering.
//
// WHAT THESE DO NOT COVER, SAID PLAINLY. They call the router directly and
// choose the handoff themselves, so they pin the mechanism the session selects
// between -- offered means withheld, withheld means routed -- and not the
// selecting. Both still pass with that selection reverted; it was checked.
// The selection lives in how `PhysicalInputRoutingContext` is built from
// `wm_session.is_some()`, nothing constructs that context outside
// `owner_loop/physical_input_phase.rs`, and reaching it needs a real input
// poller. Until that has a seam, the plumbing is covered by the QEMU gate and
// by these two only insofar as they keep the two ends honest.

use crate::live_session::*;
use sophia_protocol::{
    ClientAdmissionContext, InputEventKind, InputEventPacket, NamespaceCapabilities,
    NamespaceProfile, OutputId, Point,
};

fn admission(client: u64, namespace: u64) -> ClientAdmissionContext {
    ClientAdmissionContext::new(
        sophia_protocol::ClientAdmissionId::from_raw(client),
        sophia_protocol::NamespaceContext::new(
            sophia_protocol::NamespaceId::from_raw(namespace),
            NamespaceProfile::Confined,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
        sophia_protocol::ClientAuthProvenance::new(
            sophia_protocol::ClientAuthenticationMethod::PeerCredentials,
            1,
        )
        .unwrap(),
    )
    .unwrap()
}

/// A mapped, policy-managed toplevel at the origin, routable and hit-testable.
fn add_surface(
    layout: &mut PersistentLiveLayout,
    surface: SurfaceId,
    admission: ClientAdmissionContext,
) -> LayerSnapshot {
    let client = sophia_x_authority::XServerFrontendClientId::from_raw(admission.client_id.raw());
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
    };
    let mut batch = wm_update_coordinator_batch(TransactionId::from_raw(7));
    batch.client = Some(client);
    batch.admission = Some(admission);
    batch
        .surface_routes
        .push(sophia_x_authority::XAuthoritySurfaceRouteObservation {
            surface,
            client,
            admission: Some(admission),
        });
    batch.surface_presentations.push(
        sophia_x_authority::XAuthoritySurfacePresentationObservation {
            surface,
            role: sophia_protocol::SurfacePresentationRole::PolicyManaged,
            kind: sophia_protocol::LayoutNodeKind::Toplevel,
            placement_preference: sophia_protocol::SurfacePlacementPreference::Default,
            owner: None,
            stack_rank: 0,
            mapped: true,
            geometry,
            constraints: sophia_protocol::SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    );
    assert!(!layout.observe_authority_batch(&batch).client_route_invalid);
    LayerSnapshot {
        surface,
        authority_local_id: None,
        namespace: Some(admission.namespace.id),
        stack_rank: surface.index(),
        geometry,
        source_size: Size {
            width: 100,
            height: 100,
        },
        source: BufferSource::CpuBuffer { handle: 1 },
        damage: Region::empty(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: 1,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
        input_region: None,
        translation: None,
        output: None,
    }
}

fn event(serial: u64, kind: InputEventKind) -> InputEventPacket {
    InputEventPacket {
        serial,
        seat: SeatId::from_raw(1),
        device: sophia_protocol::DeviceId::from_raw(1),
        time_msec: serial,
        kind,
        global_position: Some(Point { x: 10.0, y: 10.0 }),
        target_surface: None,
        local_position: None,
    }
}

/// BTN_LEFT. The only button that opens a focus handoff.
const BTN_LEFT: u32 = 0x110;

/// Motion onto the surface, then a left press on it. The press is the subject;
/// the motion is what gives the pointer somewhere to be.
fn press_on_surface() -> Vec<InputEventPacket> {
    vec![
        event(1, InputEventKind::PointerMotion),
        event(
            2,
            InputEventKind::PointerButton {
                button: BTN_LEFT,
                pressed: true,
            },
        ),
    ]
}

/// Drives one press, with the focus handoff either offered or withheld, and
/// returns what routing decided. `applied_client_focus` is deliberately `None`
/// so the press is always a focus *change* -- the case that opens a handoff.
fn route_press(handoff: Option<&mut PointerFocusHandoffState>) -> PhysicalInputRouteReport {
    let mut layout = PersistentLiveLayout::default();
    let surface = SurfaceId::new(201, 1);
    let layer = add_surface(&mut layout, surface, admission(1, 4));
    let layers = vec![layer];
    let (sender, _receiver) = sync_channel(8);
    let (mut repeat, keymap) = super::test_key_repeat_parts();
    let mut pointer = SessionPointerPlacement::default();
    pointer.center_on_primary_output(Size {
        width: 100,
        height: 100,
    });

    route_input_events_with_pointer_focus(
        press_on_surface(),
        &InputFocusState::new(),
        &[],
        &layers,
        &layout.presentation_roles,
        &layout.client_routes,
        &sender,
        &mut XCoreKeyboardMapper::new(),
        &mut repeat,
        &keymap,
        &mut SessionClientKeyState::default(),
        &mut EmergencyChordState::awaiting_arm(),
        &mut VirtualTerminalChordState::default(),
        &mut PhysicalKeyboardCoverage::default(),
        None,
        &mut pointer,
        true,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut 1,
        10,
        None,
        None,
        handoff,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(OutputId::from_raw(1)),
        5,
        None,
        None,
        None,
    )
    .unwrap()
}

#[test]
fn a_left_press_routes_at_once_when_nothing_can_answer_a_focus_handoff() {
    let report = route_press(None);

    // THE REGRESSION THIS CONTROL EXISTS FOR. Withholding the press here
    // delivers it to nobody: the handoff that would release it is never
    // opened, so it expires and the button is lost. A session with no window
    // manager has no focus to change, so there is nothing to order the press
    // behind and it belongs with its client immediately.
    assert_eq!(
        report.pointer_buttons_routed, 1,
        "a press must reach its client when no focus handoff can be opened"
    );
    assert_eq!(
        report.pointer_buttons_suppressed_no_target, 0,
        "the press had a target; it must not be recorded as targetless"
    );
    assert!(
        report.pointer_focus_targets.is_empty(),
        "no handoff was opened, so nothing was handed off"
    );
    assert!(
        !report
            .policy_inputs
            .iter()
            .any(|input| matches!(input, PhysicalPolicyInput::ClickFocus(_))),
        "a focus request with nobody to receive it must not be raised at all"
    );
}

#[test]
fn a_left_press_still_opens_a_focus_handoff_when_one_can_be_answered() {
    let mut handoff = PointerFocusHandoffState::default();
    let report = route_press(Some(&mut handoff));

    // THE OTHER HALF, AND THE REASON THE FIX IS NARROW. Where a WM exists the
    // ordering is the point: the click must land after the focus it causes,
    // so it is held here and released when the handoff answers.
    assert_eq!(
        report.pointer_buttons_routed, 0,
        "a press that opens a handoff waits for it"
    );
    assert_eq!(
        report.pointer_focus_targets,
        vec![SurfaceId::new(201, 1)],
        "the press hands focus to the surface under the pointer"
    );
    assert!(
        report
            .policy_inputs
            .iter()
            .any(|input| matches!(input, PhysicalPolicyInput::ClickFocus(_))),
        "the WM is asked for the focus change the press implies"
    );
}

/// One batch with the handoff and a shortcut router, at owner time `now`,
/// with `applied` as the client focus the WM's answer has applied.
fn route_with_shortcuts(
    layout: &PersistentLiveLayout,
    layers: &[LayerSnapshot],
    events: Vec<InputEventPacket>,
    handoff: &mut PointerFocusHandoffState,
    shortcuts: &mut WmShortcutRouter,
    applied: Option<SurfaceId>,
    now: u64,
) -> PhysicalInputRouteReport {
    let (sender, _receiver) = sync_channel(8);
    let (mut repeat, keymap) = super::test_key_repeat_parts();
    let mut pointer = SessionPointerPlacement::default();
    pointer.center_on_primary_output(Size {
        width: 100,
        height: 100,
    });
    route_input_events_with_pointer_focus(
        events,
        &InputFocusState::new(),
        &[],
        layers,
        &layout.presentation_roles,
        &layout.client_routes,
        &sender,
        &mut XCoreKeyboardMapper::new(),
        &mut repeat,
        &keymap,
        &mut SessionClientKeyState::default(),
        &mut EmergencyChordState::awaiting_arm(),
        &mut VirtualTerminalChordState::default(),
        &mut PhysicalKeyboardCoverage::default(),
        Some(shortcuts),
        &mut pointer,
        true,
        false,
        false,
        PhysicalInputRoutingMode::Full,
        &mut 1,
        now,
        None,
        None,
        Some(handoff),
        applied,
        None,
        None,
        None,
        None,
        None,
        Some(OutputId::from_raw(1)),
        5,
        None,
        None,
        None,
    )
    .unwrap()
}

#[test]
fn a_click_on_a_hidden_surface_is_discarded_when_the_new_layout_presents() {
    let mut layout = PersistentLiveLayout::default();
    let surface = SurfaceId::new(201, 1);
    let layers = vec![add_surface(&mut layout, surface, admission(1, 4))];
    let mut handoff = PointerFocusHandoffState::default();
    let mut router = WmShortcutRouter::new(
        sophia_engine::WmShortcutRegistry::from_plan(
            &sophia_engine::WmShortcutPlan::default(),
            sophia_protocol::WmCapabilities::all_supported(),
            1,
            sophia_protocol::WmChromePolicy::default(),
        )
        .unwrap(),
    );
    let pressed = route_with_shortcuts(
        &layout,
        &layers,
        press_on_surface(),
        &mut handoff,
        &mut router,
        None,
        10,
    );
    assert_eq!(pressed.pointer_focus_targets, vec![surface]);
    assert_eq!(pressed.pointer_buttons_routed, 0);
    assert_eq!(handoff.target(), Some(surface));

    // The client remains live; only its workspace placement disappeared.
    // A refused focus request cannot replay the held click to a hidden client.
    let hidden = route_with_shortcuts(&layout, &[], vec![], &mut handoff, &mut router, None, 11);
    assert_eq!(hidden.pointer_focus_handoff_stale_drops, 1);
    assert_eq!(hidden.pointer_buttons_routed, 0);
    assert!(hidden.pointer_focus_handoff_released.is_none());
    assert_eq!(handoff.target(), None);

    let returned = route_with_shortcuts(
        &layout,
        &layers,
        vec![],
        &mut handoff,
        &mut router,
        Some(surface),
        12,
    );
    assert_eq!(returned.pointer_buttons_routed, 0);
    assert!(returned.pointer_focus_handoff_released.is_none());
}

/// D3b (D3a review): a button deferred by a focus handoff already abandoned
/// pending shortcut work when it arrived. Its replay, after the WM answers,
/// is that same button, so a sequence started meanwhile stays pending and
/// completes.
#[test]
fn replaying_a_deferred_button_keeps_a_sequence_started_after_it() {
    use sophia_engine::{WmKeyStep, WmSequenceBinding, WmSequenceLeader, WmShortcutPlan};
    let step = |keycode, modifiers| WmKeyStep { keycode, modifiers };
    let super_mask = sophia_protocol::WmModifierMask::SUPER;
    let (leaf, leader) = (WmActionId::from_raw(5), WmActionId::from_raw(6));
    let plan = WmShortcutPlan {
        sequences: vec![WmSequenceBinding {
            steps: vec![step(17, super_mask), step(37, 0)],
            action: leaf,
        }],
        leaders: vec![WmSequenceLeader {
            steps: vec![step(17, super_mask)],
            action: leader,
        }],
        ..WmShortcutPlan::default()
    };
    let mut router = WmShortcutRouter::new(
        sophia_engine::WmShortcutRegistry::from_plan(
            &plan,
            sophia_protocol::WmCapabilities::all_supported(),
            1,
            sophia_protocol::WmChromePolicy::default(),
        )
        .unwrap(),
    );
    router.set_action_lifecycles(&[sophia_protocol::PolicyActionLifecycleInterest {
        action: leader,
        held_ms: 0,
    }]);
    let mut layout = PersistentLiveLayout::default();
    let surface = SurfaceId::new(201, 1);
    let layers = vec![add_surface(&mut layout, surface, admission(1, 4))];
    let mut handoff = PointerFocusHandoffState::default();
    let key = |serial, keycode, pressed| event(serial, InputEventKind::Key { keycode, pressed });

    let pressed = route_with_shortcuts(
        &layout,
        &layers,
        press_on_surface(),
        &mut handoff,
        &mut router,
        None,
        10,
    );
    assert_eq!(
        pressed.pointer_buttons_routed, 0,
        "the press waits for the handoff"
    );
    assert_eq!(handoff.target(), Some(surface));

    let started = route_with_shortcuts(
        &layout,
        &layers,
        vec![key(3, 125, true), key(4, 17, true)],
        &mut handoff,
        &mut router,
        None,
        20,
    );
    assert!(matches!(
        started.policy_inputs[..],
        [PhysicalPolicyInput::ChordAction(chorded)] if chorded.action == leader && chorded.chord.opens
    ));

    let replayed = route_with_shortcuts(
        &layout,
        &layers,
        Vec::new(),
        &mut handoff,
        &mut router,
        Some(surface),
        30,
    );
    assert_eq!(
        replayed.pointer_buttons_routed, 1,
        "the answered handoff replays the press"
    );
    assert!(
        replayed.policy_inputs.is_empty(),
        "{:?}",
        replayed.policy_inputs
    );
    assert!(
        router.next_deadline().is_some(),
        "the sequence is still pending"
    );

    let completed = route_with_shortcuts(
        &layout,
        &layers,
        vec![key(5, 125, false), key(6, 37, true)],
        &mut handoff,
        &mut router,
        Some(surface),
        40,
    );
    assert!(
        matches!(
            completed.policy_inputs[..],
            [
                PhysicalPolicyInput::Action(action),
                PhysicalPolicyInput::Chord(sophia_engine::WmChordEvent::Ended {
                    end: sophia_protocol::PolicyChordEnd::Completed,
                    ..
                }),
            ] if action == leaf
        ),
        "{:?}",
        completed.policy_inputs
    );
}
