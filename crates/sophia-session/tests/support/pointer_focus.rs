use super::*;
use crate::live_session::*;

fn hover(output: u64, target: Option<SurfaceId>) -> PhysicalPolicyInput {
    PhysicalPolicyInput::Hover(PresentedPointerFocus {
        output: OutputId::from_raw(output),
        target,
    })
}

#[test]
fn hover_focus_orders_launch_shortcuts_behind_settlement_and_coalesces_only_motion() {
    let mut queue = PhysicalPolicyInputQueue::default();
    queue.synchronize(Some(1));
    let launch = PhysicalPolicyInput::Action(WmActionId::from_raw(7));
    queue.push(hover(1, None), true);
    queue.push(hover(2, None), true);
    queue.push(launch, true);
    queue.push(hover(1, None), true);
    assert_eq!(queue.next(false), Some(hover(2, None)));
    assert_eq!(
        queue.next(true),
        None,
        "launch must await terminal focus outcome"
    );
    assert_eq!(queue.next(false), Some(launch));
    assert_eq!(queue.next(false), Some(hover(1, None)));
    assert_eq!(queue.next(false), None);
    queue.push(hover(1, None), true);
    assert_eq!(
        queue.next(false),
        Some(hover(1, None)),
        "fresh motion may retry a rejected focus; committed focus is checked by the owner"
    );
    queue.push(launch, true);
    queue.next(false);
    queue.push(hover(1, None), true);
    assert_eq!(
        queue.next(false),
        Some(hover(1, None)),
        "motion after keyboard action can refocus the same window"
    );
}

#[test]
fn hover_disabled_and_replacement_do_not_replay_pointer_focus_or_shortcuts() {
    let mut queue = PhysicalPolicyInputQueue::default();
    queue.synchronize(Some(1));
    queue.push(hover(2, None), false);
    assert_eq!(queue.next(false), None);
    queue.push(hover(2, None), true);
    queue.push(PhysicalPolicyInput::Action(WmActionId::from_raw(7)), true);
    queue.synchronize(Some(2));
    assert_eq!(queue.next(false), None);
    // Only fresh movement may request focus under the replacement profile.
    queue.push(hover(2, None), true);
    assert_eq!(queue.next(false), Some(hover(2, None)));
}

fn observe_hover(
    epoch: u64,
    target: Option<SurfaceId>,
    role: sophia_protocol::SurfacePresentationRole,
    captured: bool,
    routing: PhysicalInputRoutingMode,
) -> PhysicalInputRouteReport {
    observe_hover_with_shortcut(epoch, target, role, captured, routing, None)
}

fn observe_hover_with_shortcut(
    epoch: u64,
    target: Option<SurfaceId>,
    role: sophia_protocol::SurfacePresentationRole,
    captured: bool,
    routing: PhysicalInputRoutingMode,
    shortcut_before_motion: Option<bool>,
) -> PhysicalInputRouteReport {
    let (sender, _receiver) = sync_channel(8);
    let (mut repeat, keymap) = test_key_repeat_parts();
    let mut pointer = SessionPointerPlacement::default();
    let geometry = Rect {
        x: 2600,
        y: 40,
        width: 600,
        height: 700,
    };
    let layers = target
        .into_iter()
        .map(|surface| LayerSnapshot {
            resize_sync: ResizeSyncCapability::default(),
            input_region: None,
            translation: None,
            output: Some(OutputId::from_raw(2)),
            surface,
            authority_local_id: None,
            namespace: None,
            stack_rank: 0,
            geometry,
            source_size: Size {
                width: 600,
                height: 700,
            },
            source: BufferSource::CpuBuffer { handle: 1 },
            damage: Region::empty(),
            opacity: 1.0,
            crop: None,
            transform: Transform::IDENTITY,
            generation: 1,
        })
        .collect::<Vec<_>>();
    let roles = target.map(|s| (s, role)).into_iter().collect();
    let event = InputEventPacket {
        serial: 1,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(1),
        time_msec: 1,
        kind: InputEventKind::PointerMotion,
        global_position: Some(Point {
            x: 2800.0,
            y: 200.0,
        }),
        target_surface: None,
        local_position: None,
    };
    let registry = WmShortcutRegistry::new(
        &[WmBindingRegistration {
            action: WmActionId::from_raw(7),
            keycode: 28,
            modifiers: WmModifierMask {
                bits: WmModifierMask::SUPER,
            },
        }],
        WmCapabilities::all_supported(),
        1,
        sophia_protocol::WmChromePolicy::default(),
    )
    .unwrap();
    let mut shortcuts = WmShortcutRouter::new(registry);
    let mut events = vec![event];
    if let Some(before) = shortcut_before_motion {
        let keys = [125, 28]
            .into_iter()
            .enumerate()
            .map(|(i, keycode)| InputEventPacket {
                serial: i as u64 + 2,
                seat: SeatId::from_raw(1),
                device: DeviceId::from_raw(1),
                time_msec: 2,
                kind: InputEventKind::Key {
                    keycode,
                    pressed: true,
                },
                global_position: None,
                target_surface: None,
                local_position: None,
            })
            .collect::<Vec<_>>();
        if before {
            events.splice(0..0, keys);
        } else {
            events.extend(keys);
        }
    }
    for (index, event) in events.iter_mut().enumerate() {
        event.serial = index as u64 + 1;
    }
    let mut capture = sophia_engine::ReferenceSheetCapture::default();
    if captured {
        capture.present(Some((OutputId::from_raw(2), epoch)));
    }
    route_input_events_with_pointer_focus(
        events,
        &InputFocusState::new(),
        &[],
        &layers,
        &roles,
        &XAuthorityClientSurfaceRoutes::default(),
        &sender,
        &mut XCoreKeyboardMapper::new(),
        &mut repeat,
        &keymap,
        &mut SessionClientKeyState::default(),
        &mut EmergencyChordState::awaiting_arm(),
        &mut VirtualTerminalChordState::default(),
        &mut PhysicalKeyboardCoverage::default(),
        Some(&mut shortcuts),
        &mut pointer,
        true,
        false,
        false,
        routing,
        &mut 1,
        1,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(OutputId::from_raw(2)),
        epoch,
        None,
        None,
        Some(&mut capture),
    )
    .unwrap()
}

#[test]
fn hover_observations_require_presented_input_and_follow_root_geometry_on_second_output() {
    let surface = SurfaceId::new(7, 1);
    for target in [None, Some(surface)] {
        let report = observe_hover(
            9,
            target,
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
            false,
            PhysicalInputRoutingMode::Full,
        );
        assert_eq!(report.policy_inputs, vec![hover(2, target)]);
    }
    let report = observe_hover(
        0,
        Some(surface),
        sophia_protocol::SurfacePresentationRole::PolicyManaged,
        false,
        PhysicalInputRoutingMode::Full,
    );
    assert!(report.policy_inputs.is_empty());
}

#[test]
fn hover_does_not_focus_through_modal_capture_popups_or_suppressed_input() {
    for (role, captured, mode) in [
        (
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
            true,
            PhysicalInputRoutingMode::Full,
        ),
        (
            sophia_protocol::SurfacePresentationRole::ClientPositioned,
            false,
            PhysicalInputRoutingMode::Full,
        ),
        (
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
            false,
            PhysicalInputRoutingMode::CursorOnly,
        ),
        (
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
            false,
            PhysicalInputRoutingMode::Suppressed,
        ),
    ] {
        assert!(
            observe_hover(9, Some(SurfaceId::new(7, 1)), role, captured, mode)
                .policy_inputs
                .is_empty()
        );
    }
}

#[test]
fn an_unpresented_second_output_never_borrows_primary_input_authority() {
    let outputs = [
        sophia_engine::HeadlessOutput::deterministic(),
        sophia_engine::HeadlessOutput {
            id: OutputId::from_raw(2),
            ..sophia_engine::HeadlessOutput::deterministic()
        },
    ];
    let projection = sophia_backend_live::LivePresentedInputProjection {
        output: outputs[0].id,
        epoch: 99,
        layers: vec![],
        chrome_targets: vec![],
        chrome_occlusion: None,
        descriptor_targets: vec![],
        descriptor_occlusion: None,
        content: None,
        descriptor_projection: None,
        tab_occlusions: vec![],
    };
    let projections = [projection];
    let result = input_projection_for_pointer(
        Some(&projections),
        Some(&outputs),
        Some(1),
        &[],
        Some(outputs[0].id),
        99,
    );
    assert_eq!(result.6, Some(outputs[1].id));
    assert_eq!(result.7, 0);
}

#[test]
fn physical_motion_and_launch_shortcuts_keep_their_ingress_order() {
    for before in [false, true] {
        let report = observe_hover_with_shortcut(
            9,
            None,
            sophia_protocol::SurfacePresentationRole::PolicyManaged,
            false,
            PhysicalInputRoutingMode::Full,
            Some(before),
        );
        let action = PhysicalPolicyInput::Action(WmActionId::from_raw(7));
        assert_eq!(
            report.policy_inputs,
            if before {
                vec![action, hover(2, None)]
            } else {
                vec![hover(2, None), action]
            }
        );
    }
}

#[test]
fn pointer_focus_queue_is_bounded_without_replacing_ordered_actions() {
    let mut queue = PhysicalPolicyInputQueue::default();
    for value in 1..=256 {
        assert!(queue.push(
            PhysicalPolicyInput::Action(WmActionId::from_raw(value)),
            true
        ));
    }
    assert!(!queue.push(hover(2, None), true));
    assert!(!queue.push(PhysicalPolicyInput::Action(WmActionId::from_raw(257)), true));
    for value in 1..=256 {
        assert_eq!(
            queue.next(false),
            Some(PhysicalPolicyInput::Action(WmActionId::from_raw(value)))
        );
    }
    assert_eq!(queue.next(false), None);
}
