use super::*;
use sophia_protocol::{InputEventKind, InputEventPacket};
use sophia_x_authority::XkbRmlvoConfig;

fn key(keycode: u32, pressed: bool) -> InputEventKind {
    InputEventKind::Key { keycode, pressed }
}

fn button(pressed: bool) -> InputEventKind {
    InputEventKind::PointerButton {
        button: 272,
        pressed,
    }
}

fn presented(
    public: &mut LivePublicPolicyState,
) -> Vec<sophia_backend_live::LivePresentedInputProjection> {
    let mut p = publication(public);
    let output = p.outputs[0];
    p.outputs[0].mode = PolicyPresentationMode::ReplaceApplications;
    let mut target = p.regions[0];
    p.regions[0].action = None;
    target.id = 2;
    target.z_index = 1;
    p.regions.push(target);
    p.keyboard_output = Some(output.output);
    p.bindings.push(sophia_protocol::PolicyPresentationBinding {
        keycode: 28,
        modifiers: sophia_protocol::WmModifierMask { bits: 0 },
        action: WmActionId::from_raw(77),
    });
    sophia_protocol::validate_policy_presentation_shape(&p).unwrap();
    public
        .actions
        .push(sophia_protocol::PolicyActionRegistration {
            action: WmActionId::from_raw(77),
            name: "opaque-action".into(),
            session_operation_slot: None,
        });
    public.presentation_input.admit(1, p).unwrap();
    let runtime = LiveProductionVisualRuntime::new(&public.outputs, None).unwrap();
    let mut projections = runtime.input_projections().to_vec();
    projections[0].frame_completed = true;
    projections[0].policy_visible = true;
    projections[0].policy_publication = Some(sophia_backend_live::LivePresentedPolicyPublication {
        owner_epoch: 1,
        generation: 1,
        output: output.output,
        output_generation: output.generation,
        instances: vec![],
        regions: vec![(1, 1), (2, 1)],
    });
    public.observe_presented_policy(&projections);
    assert!(public.presentation_input.modal_ready(false));
    projections
}

fn route(
    public: &mut LivePublicPolicyState,
    projections: &[sophia_backend_live::LivePresentedInputProjection],
    kinds: &[InputEventKind],
    launcher: Option<(
        &mut sophia_engine::LauncherCapture,
        &mut sophia_engine::LauncherKeyboard,
    )>,
) -> PhysicalInputRouteReport {
    let events = kinds
        .iter()
        .copied()
        .enumerate()
        .map(|(index, kind)| InputEventPacket {
            serial: index as u64 + 1,
            seat: SeatId::from_raw(1),
            device: sophia_protocol::DeviceId::from_raw(1),
            time_msec: index as u64 + 1,
            kind,
            global_position: Some(Point { x: 10.0, y: 10.0 }),
            target_surface: None,
            local_position: None,
        })
        .collect();
    let (sender, receiver) = sync_channel(8);
    let mut repeat = KeyRepeatState::new(KeyRepeatConfig::new(600, 25).unwrap());
    let keymap = XkbKeymapSnapshot::new(&XkbRmlvoConfig::default()).unwrap();
    let mut pointer = SessionPointerPlacement::default();
    pointer.center_on_primary_output(Size {
        width: 20,
        height: 20,
    });
    let report = route_input_events_with_launcher(
        events,
        &InputFocusState::new(),
        &[],
        &[],
        &Default::default(),
        &Default::default(),
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
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(projections[0].output),
        projections[0].epoch,
        Some(projections),
        None,
        None,
        launcher,
        None,
        &mut sophia_engine::RoutedInputCoalescer::new(),
        true,
        &mut None,
        std::time::Duration::ZERO,
        Some(PolicyPresentedInputRouting {
            state: &public.presentation_input,
            capture: &mut public.presentation_capture,
            protected_actions: vec![],
        }),
    )
    .unwrap();
    assert_eq!(
        receiver.try_iter().count(),
        0,
        "modal input must not escape to application ingress"
    );
    report
}

fn actions(report: &PhysicalInputRouteReport) -> Vec<sophia_engine::PresentedPolicyAction> {
    report
        .policy_inputs
        .iter()
        .filter_map(|input| match input {
            PhysicalPolicyInput::PresentedAction(action) => Some(*action),
            _ => None,
        })
        .collect()
}

#[test]
fn physical_policy_routing_emits_exact_targets_and_swallows_unbound_modal_keys() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let receipt = public
        .presentation_input
        .output_receipt(projections[0].output)
        .unwrap();
    let report = route(
        public,
        &projections,
        &[
            key(28, true),
            key(28, false),
            key(30, true),
            key(30, false),
            button(true),
            button(false),
        ],
        None,
    );
    let actions = actions(&report);
    assert_eq!(actions.len(), 2);
    for action in &actions {
        assert_eq!(action.action, WmActionId::from_raw(77));
        assert_eq!(action.connection_epoch, 1);
        assert_eq!(
            action.identity.presentation_epoch,
            receipt.presentation_epoch
        );
        assert_eq!(action.identity.output, projections[0].output);
    }
    assert_eq!(
        (
            actions[0].identity.target_id,
            actions[0].identity.target_generation
        ),
        (0, 0)
    );
    assert_eq!(
        (
            actions[1].identity.target_id,
            actions[1].identity.target_generation
        ),
        (2, 1)
    );
    assert_eq!(
        report.keys_suppressed_no_focus, 0,
        "unbound key was swallowed before application routing"
    );
    assert_eq!(report.pointer_routed, 0);
}

#[test]
fn physical_policy_routing_keeps_release_debt_when_action_queue_forces_revocation() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let report = route(public, &projections, &[button(true), key(28, true)], None);
    let action = actions(&report)[0];
    public.queue.clear();
    for _ in 0..WM_OWNER_REQUEST_CAPACITY {
        assert_eq!(
            public.queue_cause(LivePublicPolicyCause {
                source: LiveWmProposalSource::Action(action.action),
                cause: sophia_protocol::PolicyRequestCause::SceneChanged,
                affected_outputs: vec![projections[0].output],
            }),
            LiveWmRequestAdmission::Admitted
        );
    }
    fixture.wm.enqueue_presented_action(action).unwrap();
    let public = fixture.wm.public.as_mut().unwrap();
    assert!(public.presentation_input.publication().is_none());
    assert!(public.presentation_withdrawal_pending);
    let mut withdrawn = projections;
    withdrawn[0].policy_visible = false;
    withdrawn[0].policy_publication = None;
    let report = route(public, &withdrawn, &[button(false), key(28, false)], None);
    assert!(actions(&report).is_empty());
    assert_eq!(
        report.keys_suppressed_no_focus, 0,
        "release debt survives without visible shielding"
    );
    assert_eq!(report.pointer_buttons_suppressed_no_target, 0);
    assert_eq!(public.queue.len(), WM_OWNER_REQUEST_CAPACITY);
}

#[test]
fn physical_policy_routing_yields_to_launcher_and_virtual_terminal() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let mut capture = sophia_engine::LauncherCapture::default();
    capture.present(
        Some((projections[0].output, 7)),
        1,
        &[(
            1,
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
        )],
        false,
    );
    let mut keyboard = sophia_engine::LauncherKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap();
    let report = route(
        public,
        &projections,
        &[button(true), button(false)],
        Some((&mut capture, &mut keyboard)),
    );
    assert!(actions(&report).is_empty());
    assert_eq!(report.launcher_events.len(), 1);
    let report = route(
        public,
        &projections,
        &[key(29, true), key(56, true), key(60, true)],
        None,
    );
    assert_eq!(report.virtual_terminal, Some(2));
    assert!(actions(&report).is_empty());
}

#[test]
fn physical_policy_routing_revokes_keys_when_a_completed_frame_loses_its_stamp() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let mut projections = presented(public);
    projections[0].policy_publication = None;
    projections[0].policy_visible = false;
    public.observe_presented_policy(&projections);
    assert!(public.presentation_input.publication().is_none());
    assert!(!public.presentation_input.modal_ready(false));
    let report = route(public, &projections, &[key(28, true), key(28, false)], None);
    assert!(actions(&report).is_empty());
    assert_eq!(report.keys_suppressed_no_focus, 2);
    assert!(
        public
            .presentation_receipts
            .iter()
            .any(|receipt| receipt.outcome == PolicyPresentationOutcome::Revoked)
    );
}
