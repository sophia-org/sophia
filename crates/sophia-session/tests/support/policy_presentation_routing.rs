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
    projections[0].presented_keyboard = sophia_engine::PresentedKeyboardScope::Modal;
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
    route_with_shortcuts(public, projections, kinds, launcher, None)
}

fn route_with_shortcuts(
    public: &mut LivePublicPolicyState,
    projections: &[sophia_backend_live::LivePresentedInputProjection],
    kinds: &[InputEventKind],
    launcher: Option<(
        &mut sophia_engine::LauncherCapture,
        &mut sophia_engine::LauncherKeyboard,
    )>,
    shortcuts: Option<&mut WmShortcutRouter>,
) -> PhysicalInputRouteReport {
    let keyed = kinds
        .iter()
        .map(|kind| (sophia_protocol::DeviceId::from_raw(1), *kind))
        .collect::<Vec<_>>();
    route_on_devices(public, projections, &keyed, launcher, shortcuts)
}

fn route_on_devices(
    public: &mut LivePublicPolicyState,
    projections: &[sophia_backend_live::LivePresentedInputProjection],
    kinds: &[(sophia_protocol::DeviceId, InputEventKind)],
    launcher: Option<(
        &mut sophia_engine::LauncherCapture,
        &mut sophia_engine::LauncherKeyboard,
    )>,
    shortcuts: Option<&mut WmShortcutRouter>,
) -> PhysicalInputRouteReport {
    route_captured(public, projections, kinds, launcher, None, shortcuts)
}

/// As `route_on_devices`, with an optional reference-sheet capture too.
fn route_captured(
    public: &mut LivePublicPolicyState,
    projections: &[sophia_backend_live::LivePresentedInputProjection],
    kinds: &[(sophia_protocol::DeviceId, InputEventKind)],
    launcher: Option<(
        &mut sophia_engine::LauncherCapture,
        &mut sophia_engine::LauncherKeyboard,
    )>,
    reference: Option<&mut sophia_engine::ReferenceSheetCapture>,
    shortcuts: Option<&mut WmShortcutRouter>,
) -> PhysicalInputRouteReport {
    let (report, ingress) = route_with_client_keys(
        public,
        projections,
        kinds,
        launcher,
        reference,
        shortcuts,
        &mut SessionClientKeyState::default(),
    );
    assert_eq!(ingress, 0, "modal input must not escape to application ingress");
    report
}

/// As `route_captured`, with the client-key ledger supplied, so keys an
/// application already holds are part of the picture. Returns the report and
/// how many routed inputs reached application ingress.
fn route_with_client_keys(
    public: &mut LivePublicPolicyState,
    projections: &[sophia_backend_live::LivePresentedInputProjection],
    kinds: &[(sophia_protocol::DeviceId, InputEventKind)],
    launcher: Option<(
        &mut sophia_engine::LauncherCapture,
        &mut sophia_engine::LauncherKeyboard,
    )>,
    reference: Option<&mut sophia_engine::ReferenceSheetCapture>,
    shortcuts: Option<&mut WmShortcutRouter>,
    client_keys: &mut SessionClientKeyState,
) -> (PhysicalInputRouteReport, usize) {
    let events = kinds
        .iter()
        .copied()
        .enumerate()
        .map(|(index, (device, kind))| InputEventPacket {
            serial: index as u64 + 1,
            seat: SeatId::from_raw(1),
            device,
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
        client_keys,
        &mut EmergencyChordState::awaiting_arm(),
        &mut VirtualTerminalChordState::default(),
        &mut PhysicalKeyboardCoverage::default(),
        shortcuts,
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
        reference,
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
    let ingress = receiver.try_iter().count();
    (report, ingress)
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

/// Enter fires CHORD, a chord the WM follows; the modal capture also binds
/// Enter, to action 77.
fn followed_enter() -> WmShortcutRouter {
    let mut router = WmShortcutRouter::new(
        sophia_engine::WmShortcutRegistry::new(
            &[sophia_protocol::WmBindingRegistration {
                action: WmActionId::from_raw(186),
                keycode: 28,
                modifiers: sophia_protocol::WmModifierMask { bits: 0 },
            }],
            sophia_protocol::WmCapabilities::all_supported(),
            1,
            sophia_protocol::WmChromePolicy::default(),
        )
        .unwrap(),
    );
    router.set_action_lifecycles(&[sophia_protocol::PolicyActionLifecycleInterest {
        action: WmActionId::from_raw(186),
        held_ms: 0,
    }]);
    router
}

/// A followed chord's activation is protected: the capture leaves it alone,
/// so its Held and Ended mean something.
#[test]
fn a_followed_chord_bypasses_the_modal_capture() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let mut router = followed_enter();
    let report = route_with_shortcuts(public, &projections, &[key(28, true)], None, Some(&mut router));
    assert!(actions(&report).is_empty());
    assert!(matches!(
        report.policy_inputs[..],
        [PhysicalPolicyInput::ChordAction(chorded)] if chorded.action == WmActionId::from_raw(186)
    ));
}

/// Control for the bypass above: a shortcut the WM does not follow, and that
/// is not protected, is the modal capture's when the capture binds it. The
/// router's proposal is declined, so only the presentation action is queued
/// and no chord or Action is left behind.
#[test]
fn an_unfollowed_shortcut_the_capture_binds_is_the_captures() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let mut router = followed_enter();
    router.set_action_lifecycles(&[]);
    let report = route_with_shortcuts(public, &projections, &[key(28, true)], None, Some(&mut router));
    assert_eq!(actions(&report).len(), 1);
    assert!(
        report
            .policy_inputs
            .iter()
            .all(|input| matches!(input, PhysicalPolicyInput::PresentedAction(_))),
        "{:?}",
        report.policy_inputs
    );
    assert!(router.take_outputs().is_empty());
}

/// A press the router consumes but fires nothing for, because every credit is
/// owed, is the router's: it never becomes a presentation activation.
#[test]
fn a_shortcut_refused_for_credit_is_no_presentation_activation() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let mut router = followed_enter();
    for _ in 0..sophia_engine::WM_CHORD_CREDITS {
        route_test_key(&mut router, SeatId::from_raw(1), sophia_protocol::DeviceId::from_raw(1), 28, true, 0);
        route_test_key(&mut router, SeatId::from_raw(1), sophia_protocol::DeviceId::from_raw(1), 28, false, 0);
    }
    assert_eq!(router.chord_credits_free(), 0);
    let report = route_with_shortcuts(public, &projections, &[key(28, true)], None, Some(&mut router));
    assert!(actions(&report).is_empty());
    assert!(report.policy_inputs.iter().all(|input| matches!(input, PhysicalPolicyInput::Chord(_))));
}

/// With the seat's modifiers unknown nothing matches: the capture swallows the
/// press without activating its binding and still settles its release.
#[test]
fn an_uncertain_seat_matches_no_presentation_binding() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let mut router = followed_enter();
    let seat = SeatId::from_raw(1);
    for device in 100..100 + sophia_engine::WM_MAX_SHORTCUT_DEVICES as u64 {
        route_test_key(&mut router, seat, sophia_protocol::DeviceId::from_raw(device), 30, true, 0);
    }
    route_test_key(&mut router, seat, sophia_protocol::DeviceId::from_raw(99), 56, true, 0);
    assert!(router.seat_uncertain(seat));
    // An unbound key for the router, bound in the capture only.
    let report = route_with_shortcuts(
        public,
        &projections,
        &[key(28, true), key(28, false)],
        None,
        Some(&mut router),
    );
    assert!(actions(&report).is_empty());
    assert!(report.policy_inputs.is_empty());
    assert_eq!(report.shortcut_uncertain_presses, 1);
}

/// Review R2: protection follows the activation, not the current declaration.
/// An open chord keeps its frozen eligibility, so a join from another keyboard
/// after the row is removed is still the chord's, never the capture's.
#[test]
fn a_join_keeps_its_protection_after_its_declaration_is_removed() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let mut router = followed_enter();
    let first = sophia_protocol::DeviceId::from_raw(1);
    let second = sophia_protocol::DeviceId::from_raw(2);
    let report = route_on_devices(public, &projections, &[(first, key(28, true))], None, Some(&mut router));
    let Some(PhysicalPolicyInput::ChordAction(opener)) = report.policy_inputs.first().copied() else {
        panic!("the opener is the chord's: {:?}", report.policy_inputs);
    };
    router.set_action_lifecycles(&[]);
    let report = route_on_devices(public, &projections, &[(second, key(28, true))], None, Some(&mut router));
    assert!(actions(&report).is_empty());
    assert!(matches!(
        report.policy_inputs[..],
        [PhysicalPolicyInput::ChordAction(join)]
            if join.chord.token == opener.chord.token && !join.chord.opens
    ));
}

fn exhaust_credits(router: &mut WmShortcutRouter) {
    for _ in 0..sophia_engine::WM_CHORD_CREDITS {
        let seat = SeatId::from_raw(1);
        let device = sophia_protocol::DeviceId::from_raw(1);
        route_test_key(router, seat, device, 28, true, 0);
        route_test_key(router, seat, device, 28, false, 0);
    }
    assert_eq!(router.chord_credits_free(), 0);
}

fn active_launcher() -> (sophia_engine::LauncherCapture, sophia_engine::LauncherKeyboard) {
    let mut capture = sophia_engine::LauncherCapture::default();
    capture.present(
        Some((sophia_protocol::OutputId::from_raw(1), 7)),
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
    let keyboard = sophia_engine::LauncherKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap();
    (capture, keyboard)
}

fn active_reference() -> sophia_engine::ReferenceSheetCapture {
    let mut reference = sophia_engine::ReferenceSheetCapture::default();
    reference.present(Some((sophia_protocol::OutputId::from_raw(1), 3)));
    reference
}

/// D3 (addendum A2): a press that would open a chord with no credit free is
/// the router's before any capture, so neither the launcher nor the reference
/// sheet reads it.
#[test]
fn a_shortcut_refused_for_credit_reaches_no_launcher_or_reference_capture() {
    for with_reference in [false, true] {
        let mut fixture = ReloadFixture::new();
        let public = fixture.wm.public.as_mut().unwrap();
        let projections = presented(public);
        let mut router = followed_enter();
        exhaust_credits(&mut router);
        let (mut launcher, mut keyboard) = active_launcher();
        let mut reference = active_reference();
        let report = route_captured(
            public,
            &projections,
            &[(sophia_protocol::DeviceId::from_raw(1), key(28, true))],
            (!with_reference).then_some((&mut launcher, &mut keyboard)),
            with_reference.then_some(&mut reference),
            Some(&mut router),
        );
        assert!(report.launcher_events.is_empty(), "{with_reference}");
        assert!(report.reference_operations.is_empty(), "{with_reference}");
        assert!(report.policy_inputs.is_empty(), "{with_reference}");
        // Control: with a credit free the same press is the capture's.
        let mut router = followed_enter();
        let report = route_captured(
            public,
            &projections,
            &[(sophia_protocol::DeviceId::from_raw(1), key(28, true))],
            (!with_reference).then_some((&mut launcher, &mut keyboard)),
            with_reference.then_some(&mut active_reference()),
            Some(&mut router),
        );
        assert!(
            !report.launcher_events.is_empty() || !report.reference_operations.is_empty(),
            "{with_reference}"
        );
        assert!(report.policy_inputs.is_empty(), "{with_reference}");
        assert_eq!(router.chord_credits_free(), sophia_engine::WM_CHORD_CREDITS);
    }
}

/// D2 review: a capture declines the router's proposal, and the router is
/// left free. Cancellation, an epoch reset and fresh routing all proceed,
/// each with its outputs in order.
#[test]
fn a_captured_press_leaves_the_router_free_for_cancellation_and_reset() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let mut router = followed_enter();
    let first = sophia_protocol::DeviceId::from_raw(1);
    let second = sophia_protocol::DeviceId::from_raw(2);
    let opened = route_on_devices(public, &projections, &[(first, key(28, true))], None, Some(&mut router));
    let token = match opened.policy_inputs[..] {
        [PhysicalPolicyInput::ChordAction(chorded)] if chorded.chord.opens => chorded.chord.token,
        ref other => panic!("expected the opener, got {other:?}"),
    };
    // The reference sheet takes a join on the second keyboard: declined, so
    // nothing joins and nothing is queued.
    let mut reference = active_reference();
    let captured = route_captured(
        public,
        &projections,
        &[(second, key(28, true))],
        None,
        Some(&mut reference),
        Some(&mut router),
    );
    assert_eq!(captured.reference_operations.len(), 1);
    assert!(captured.policy_inputs.is_empty());
    router.cancel_all_chords();
    assert_eq!(
        router.take_outputs(),
        [sophia_engine::WmShortcutOutput::Chord(
            sophia_engine::WmChordEvent::Ended {
                token,
                end: sophia_protocol::PolicyChordEnd::Cancelled,
            }
        )]
    );
    router.reset_chords();
    router.set_action_lifecycles(&[sophia_protocol::PolicyActionLifecycleInterest {
        action: WmActionId::from_raw(186),
        held_ms: 0,
    }]);
    // Both releases stay paired, and a fresh press opens a new chord.
    route_test_key(&mut router, SeatId::from_raw(1), first, 28, false, 1);
    route_test_key(&mut router, SeatId::from_raw(1), second, 28, false, 1);
    let reopened = route_on_devices(public, &projections, &[(first, key(28, true))], None, Some(&mut router));
    assert!(matches!(
        reopened.policy_inputs[..],
        [PhysicalPolicyInput::ChordAction(chorded)] if chorded.chord.opens && chorded.chord.token != token
    ));
}

const SWITCHER: WmActionId = WmActionId::from_raw(u64::MAX);
const HELP: WmActionId = WmActionId::from_raw(u64::MAX - 1);
const SUPER_KEY: u32 = 125;

/// The shell switcher and help in every shape: immediate Super+P and Super+H,
/// a Super+X hold, Super+W K and Super+W J L, and a Super tap.
fn shell_shapes() -> WmShortcutRouter {
    use sophia_engine::{
        WmHoldBinding, WmKeyStep, WmModifierTapBinding, WmSequenceBinding, WmShortcutPlan,
    };
    use sophia_protocol::WmModifierMask;
    let step = |keycode, modifiers| WmKeyStep { keycode, modifiers };
    let plan = WmShortcutPlan {
        immediate: [(SWITCHER, 25), (HELP, 35)]
            .map(|(action, keycode)| sophia_protocol::WmBindingRegistration {
                action,
                keycode,
                modifiers: WmModifierMask {
                    bits: WmModifierMask::SUPER,
                },
            })
            .to_vec(),
        holds: vec![WmHoldBinding {
            step: step(45, WmModifierMask::SUPER),
            hold_ms: 500,
            action: SWITCHER,
        }],
        taps: vec![WmModifierTapBinding {
            modifier: WmModifierMask::SUPER,
            action: SWITCHER,
        }],
        sequences: vec![
            WmSequenceBinding {
                steps: vec![step(17, WmModifierMask::SUPER), step(37, 0)],
                action: SWITCHER,
            },
            WmSequenceBinding {
                steps: vec![step(17, WmModifierMask::SUPER), step(36, 0), step(38, 0)],
                action: HELP,
            },
        ],
        ..WmShortcutPlan::default()
    };
    WmShortcutRouter::new(
        sophia_engine::WmShortcutRegistry::from_plan(
            &plan,
            sophia_protocol::WmCapabilities::all_supported(),
            1,
            sophia_protocol::WmChromePolicy::default(),
        )
        .unwrap(),
    )
}

#[derive(Clone, Copy, Debug)]
enum ShellCapture {
    Launcher,
    Reference,
}

/// D3a review R1: the launcher's switcher and help exceptions, and the
/// reference sheet's switcher exception, follow only an action the event
/// selects: an immediate chord's, or a modifier tap's at its release. An
/// undecided hold, a sequence prefix, an internal or leaf continuation with
/// the capture opened after the prefix, all leading to the switcher or help,
/// stay the capture's: declined, nothing armed, nothing fired later. Neither
/// capture takes a modifier key, so an arming Super press passes both, and
/// its release fires through the selected-action exception.
#[test]
fn only_a_selected_action_skips_the_launcher_or_reference_capture() {
    // (name, keys before the capture opens, keys while it is open, keys
    // after it closes, whether it took the first open key, actions fired)
    type Keys = &'static [(u32, bool)];
    type Row = (&'static str, Keys, Keys, Keys, bool, &'static [WmActionId]);
    let rows: [Row; 7] = [
        ("immediate switcher", &[(SUPER_KEY, true)], &[(25, true)], &[], false, &[SWITCHER]),
        ("undecided hold", &[(SUPER_KEY, true)], &[(45, true)], &[(45, false)], true, &[]),
        ("sequence prefix", &[(SUPER_KEY, true)], &[(17, true)], &[(17, false), (37, true)], true, &[]),
        (
            "internal continuation",
            &[(SUPER_KEY, true), (17, true), (17, false), (SUPER_KEY, false)],
            &[(36, true)],
            &[(36, false), (38, true)],
            true,
            &[],
        ),
        (
            "leaf continuation",
            &[(SUPER_KEY, true), (17, true), (17, false), (SUPER_KEY, false)],
            &[(37, true)],
            &[],
            true,
            &[],
        ),
        ("modifier tap", &[], &[(SUPER_KEY, true), (SUPER_KEY, false)], &[], false, &[SWITCHER]),
        // Help is the launcher's exception only; the reference sheet takes it.
        ("immediate help", &[(SUPER_KEY, true)], &[(35, true)], &[], false, &[HELP]),
    ];
    for capture in [ShellCapture::Launcher, ShellCapture::Reference] {
        for (name, before, during, after, taken, fired) in rows {
            let (taken, fired) = match (capture, name) {
                (ShellCapture::Reference, "immediate help") => (true, &[][..]),
                _ => (taken, fired),
            };
            let mut fixture = ReloadFixture::new();
            let public = fixture.wm.public.as_mut().unwrap();
            let projections = presented(public);
            let mut router = shell_shapes();
            let device = sophia_protocol::DeviceId::from_raw(1);
            let mut actions = Vec::new();
            let collect = |report: &PhysicalInputRouteReport, actions: &mut Vec<WmActionId>| {
                actions.extend(report.policy_inputs.iter().filter_map(|input| match input {
                    PhysicalPolicyInput::Action(action) => Some(*action),
                    PhysicalPolicyInput::ChordAction(chorded) => Some(chorded.action),
                    _ => None,
                }));
            };
            for &(keycode, pressed) in before {
                let report = route_captured(public, &projections, &[(device, key(keycode, pressed))], None, None, Some(&mut router));
                collect(&report, &mut actions);
            }
            let (mut launcher, mut keyboard) = active_launcher();
            let mut reference = active_reference();
            for (index, &(keycode, pressed)) in during.iter().enumerate() {
                let report = route_captured(
                    public,
                    &projections,
                    &[(device, key(keycode, pressed))],
                    matches!(capture, ShellCapture::Launcher).then_some((&mut launcher, &mut keyboard)),
                    matches!(capture, ShellCapture::Reference).then_some(&mut reference),
                    Some(&mut router),
                );
                if index == 0 {
                    let took = !report.launcher_events.is_empty() || !report.reference_operations.is_empty();
                    assert_eq!(took, taken, "{name} with {capture:?}");
                }
                collect(&report, &mut actions);
            }
            for &(keycode, pressed) in after {
                let report = route_captured(public, &projections, &[(device, key(keycode, pressed))], None, None, Some(&mut router));
                collect(&report, &mut actions);
            }
            router.poll_shortcuts(10_000);
            actions.extend(router.take_outputs().into_iter().filter_map(|output| match output {
                sophia_engine::WmShortcutOutput::Activation(activation) => Some(activation.action),
                sophia_engine::WmShortcutOutput::Chord(_) => None,
            }));
            assert_eq!(actions, fired, "{name} with {capture:?}");
        }
    }
}

/// An Overlay publication with a keyboard scope: a held capture. Escape with
/// Alt is bound to action 77. `completed` false leaves the frame unfinished,
/// so the capture shields instead of matching.
/// The held publication: an Overlay keyboard scope binding Escape with Alt
/// to action 77, at `generation`.
fn held_publication(public: &LivePublicPolicyState, generation: u64) -> PolicyPresentation {
    let mut p = publication(public);
    p.generation = generation;
    let output = p.outputs[0];
    assert_eq!(output.mode, PolicyPresentationMode::Overlay);
    p.regions[0].action = None;
    p.keyboard_output = Some(output.output);
    p.bindings.push(sophia_protocol::PolicyPresentationBinding {
        keycode: 1,
        modifiers: sophia_protocol::WmModifierMask {
            bits: sophia_protocol::WmModifierMask::ALT,
        },
        action: WmActionId::from_raw(77),
    });
    sophia_protocol::validate_policy_presentation_shape(&p).unwrap();
    p
}

/// A modal publication over replaced applications at `generation`.
fn modal_publication(public: &LivePublicPolicyState, generation: u64) -> PolicyPresentation {
    let mut p = held_publication(public, generation);
    p.outputs[0].mode = PolicyPresentationMode::ReplaceApplications;
    sophia_protocol::validate_policy_presentation_shape(&p).unwrap();
    p
}

fn register_action_77(public: &mut LivePublicPolicyState) {
    public
        .actions
        .push(sophia_protocol::PolicyActionRegistration {
            action: WmActionId::from_raw(77),
            name: "opaque-action".into(),
            session_operation_slot: None,
        });
}

fn held_presented(
    public: &mut LivePublicPolicyState,
    completed: bool,
) -> Vec<sophia_backend_live::LivePresentedInputProjection> {
    let p = held_publication(public, 1);
    let output = p.outputs[0];
    register_action_77(public);
    public.presentation_input.admit(1, p).unwrap();
    assert!(public.presentation_input.held_capture());
    let runtime = LiveProductionVisualRuntime::new(&public.outputs, None).unwrap();
    let mut projections = runtime.input_projections().to_vec();
    projections[0].frame_completed = completed;
    projections[0].policy_visible = true;
    projections[0].policy_publication = Some(sophia_backend_live::LivePresentedPolicyPublication {
        owner_epoch: 1,
        generation: 1,
        output: output.output,
        output_generation: output.generation,
        instances: vec![],
        regions: vec![(1, 1)],
    });
    projections[0].presented_keyboard = sophia_engine::PresentedKeyboardScope::Held;
    if completed {
        public.observe_presented_policy(&projections);
        assert!(public.presentation_input.modal_ready(false));
    }
    projections
}

/// Alt+Tab, followed: further Tabs while Alt is held are chord joins.
fn followed_alt_tab() -> WmShortcutRouter {
    let mut router = WmShortcutRouter::new(
        sophia_engine::WmShortcutRegistry::new(
            &[sophia_protocol::WmBindingRegistration {
                action: WmActionId::from_raw(186),
                keycode: 15,
                modifiers: sophia_protocol::WmModifierMask {
                    bits: sophia_protocol::WmModifierMask::ALT,
                },
            }],
            sophia_protocol::WmCapabilities::all_supported(),
            1,
            sophia_protocol::WmChromePolicy::default(),
        )
        .unwrap(),
    );
    router.set_action_lifecycles(&[sophia_protocol::PolicyActionLifecycleInterest {
        action: WmActionId::from_raw(186),
        held_ms: 0,
    }]);
    router
}

/// A key an application already holds, delivered before any capture existed.
fn held_by_application(keycode: u32) -> SessionClientKeyState {
    let mut keys = SessionClientKeyState::default();
    keys.record_routed(
        SessionClientPressedKey {
            surface: SurfaceId::new(201, 1),
            seat: SeatId::from_raw(1),
            device: sophia_protocol::DeviceId::from_raw(1),
            keycode,
        },
        true,
    );
    keys
}

fn keyed(kinds: &[InputEventKind]) -> Vec<(sophia_protocol::DeviceId, InputEventKind)> {
    kinds
        .iter()
        .map(|kind| (sophia_protocol::DeviceId::from_raw(1), *kind))
        .collect()
}

/// Escape while Alt is held is the held capture's; the application keeps Alt.
/// Alt's press and release both reach client routing, and the capture owes
/// nothing for them, while Escape and its release never do.
#[test]
fn a_held_capture_takes_escape_and_leaves_alt_to_the_application() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = held_presented(public, true);
    let mut router = followed_alt_tab();
    let mut keys = held_by_application(56);
    let (report, _) = route_with_client_keys(
        public,
        &projections,
        &keyed(&[key(56, true), key(1, true), key(1, false), key(56, false)]),
        None,
        None,
        Some(&mut router),
        &mut keys,
    );
    assert_eq!(actions(&report).len(), 1, "{:?}", report.policy_inputs);
    assert_eq!(actions(&report)[0].action, WmActionId::from_raw(77));
    assert_eq!(
        report.keys_suppressed_no_focus, 2,
        "both of Alt's edges went on to client routing; Escape's did not"
    );
}

/// Control: a modal capture over replaced applications still takes Alt.
#[test]
fn a_modal_capture_still_takes_modifiers() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = presented(public);
    let mut router = followed_alt_tab();
    let report = route_with_shortcuts(
        public,
        &projections,
        &[key(56, true), key(56, false)],
        None,
        Some(&mut router),
    );
    assert_eq!(report.keys_suppressed_no_focus, 0, "the modal capture swallowed Alt");
}

/// A non-modifier key an application still holds keeps its sequence: the
/// held capture passes everything until it is released.
#[test]
fn a_held_capture_waits_for_a_held_application_key() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = held_presented(public, true);
    let mut router = followed_alt_tab();
    let mut keys = held_by_application(30);
    let (report, _) = route_with_client_keys(
        public,
        &projections,
        &keyed(&[key(56, true), key(1, true), key(1, false)]),
        None,
        None,
        Some(&mut router),
        &mut keys,
    );
    assert!(actions(&report).is_empty(), "{:?}", report.policy_inputs);
    assert_eq!(report.keys_suppressed_no_focus, 3, "Alt and Escape went to client routing");
}

/// A further Tab of the held Alt+Tab is still the router's join, the capture
/// takes Escape in between, and Alt's release ends the chord.
#[test]
fn a_held_capture_leaves_the_chord_join_and_its_end_to_the_router() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = held_presented(public, true);
    let mut router = followed_alt_tab();
    let mut keys = SessionClientKeyState::default();
    let (report, _) = route_with_client_keys(
        public,
        &projections,
        &keyed(&[
            key(56, true),
            key(15, true),
            key(15, false),
            key(1, true),
            key(1, false),
            key(56, false),
        ]),
        None,
        None,
        Some(&mut router),
        &mut keys,
    );
    assert!(
        matches!(
            report.policy_inputs[..],
            [
                PhysicalPolicyInput::ChordAction(join),
                PhysicalPolicyInput::PresentedAction(escape),
                PhysicalPolicyInput::Chord(sophia_engine::WmChordEvent::Ended {
                    end: sophia_protocol::PolicyChordEnd::Released,
                    ..
                }),
            ] if join.action == WmActionId::from_raw(186) && escape.action == WmActionId::from_raw(77)
        ),
        "{:?}",
        report.policy_inputs
    );
}

/// Before its frame completes a held capture shields, swallowing new keys,
/// but it still never takes a modifier.
#[test]
fn a_shielded_held_capture_still_passes_modifiers() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = held_presented(public, false);
    let mut router = followed_alt_tab();
    let mut keys = SessionClientKeyState::default();
    let (report, _) = route_with_client_keys(
        public,
        &projections,
        &keyed(&[key(56, true), key(1, true), key(1, false), key(56, false)]),
        None,
        None,
        Some(&mut router),
        &mut keys,
    );
    assert!(actions(&report).is_empty(), "{:?}", report.policy_inputs);
    assert_eq!(report.keys_suppressed_no_focus, 2, "Alt's edges reached client routing; Escape did not");
}

/// Alt down, Escape down and up, Alt up, on the Alt+Tab router.
fn alt_escape() -> Vec<(sophia_protocol::DeviceId, InputEventKind)> {
    keyed(&[key(56, true), key(1, true), key(1, false), key(56, false)])
}

fn route_alt_escape(
    public: &mut LivePublicPolicyState,
    projections: &[sophia_backend_live::LivePresentedInputProjection],
) -> PhysicalInputRouteReport {
    let mut router = followed_alt_tab();
    route_with_client_keys(
        public,
        projections,
        &alt_escape(),
        None,
        None,
        Some(&mut router),
        &mut SessionClientKeyState::default(),
    )
    .0
}

/// Revoked while the held strip is still on screen: the shield keeps the
/// presented Held rule, so Alt passes and Escape is shielded. Once every head
/// retires the withdrawal, nothing is shielded.
#[test]
fn a_revoked_held_strip_still_on_screen_keeps_passing_modifiers() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let mut projections = held_presented(public, true);
    public.presentation_input.revoke();
    assert!(!public.presentation_input.held_capture());
    let report = route_alt_escape(public, &projections);
    assert!(actions(&report).is_empty(), "{:?}", report.policy_inputs);
    assert_eq!(report.keys_suppressed_no_focus, 2, "Alt's edges pass; Escape is shielded");
    projections[0].policy_visible = false;
    projections[0].policy_publication = None;
    projections[0].presented_keyboard = sophia_engine::PresentedKeyboardScope::None;
    let report = route_alt_escape(public, &projections);
    assert_eq!(report.keys_suppressed_no_focus, 4, "withdrawn: nothing is shielded");
}

/// A new owner epoch's publication is admitted but not yet presented: the
/// old Held pixels keep their rule.
#[test]
fn a_new_owner_epoch_keeps_the_presented_held_rule() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let projections = held_presented(public, true);
    let next = held_publication(public, 2);
    public.presentation_input.admit(2, next).unwrap();
    let report = route_alt_escape(public, &projections);
    assert!(actions(&report).is_empty(), "{:?}", report.policy_inputs);
    assert_eq!(report.keys_suppressed_no_focus, 2);
}

/// Old modal pixels still on screen with a held publication admitted: a new
/// modifier is still the modal scope's. Once the heads show Held, the Alt the
/// modal scope consumed keeps its consumed release (debts first), and only
/// fresh Alt edges pass.
#[test]
fn old_modal_pixels_keep_modal_rules_until_the_held_strip_is_presented() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let mut projections = presented(public);
    let mut router = followed_alt_tab();
    let mut keys = SessionClientKeyState::default();
    let mut route = |public: &mut LivePublicPolicyState,
                     projections: &[sophia_backend_live::LivePresentedInputProjection],
                     kinds: &[InputEventKind]| {
        route_with_client_keys(
            public,
            projections,
            &keyed(kinds),
            None,
            None,
            Some(&mut router),
            &mut keys,
        )
        .0
        .keys_suppressed_no_focus
    };
    assert_eq!(route(public, &projections, &[key(56, true)]), 0, "the modal scope took Alt");
    let next = held_publication(public, 2);
    public.presentation_input.admit(1, next).unwrap();
    assert!(public.presentation_input.held_capture());
    assert_eq!(
        route(public, &projections, &[key(42, true), key(42, false), key(1, true), key(1, false)]),
        0,
        "old modal pixels: a new Shift and Escape are shielded"
    );
    projections[0].presented_keyboard = sophia_engine::PresentedKeyboardScope::Held;
    assert_eq!(
        route(public, &projections, &[key(56, false)]),
        0,
        "the modal scope's Alt keeps its consumed release"
    );
    assert_eq!(
        route(public, &projections, &[key(56, true), key(56, false)]),
        2,
        "fresh Alt edges pass the presented held strip"
    );
}

/// Old Held pixels with a modal publication admitted pass Alt until any head
/// shows the modal frame.
#[test]
fn old_held_pixels_pass_modifiers_until_a_head_shows_modal() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let mut projections = held_presented(public, true);
    let next = modal_publication(public, 2);
    public.presentation_input.admit(1, next).unwrap();
    assert!(!public.presentation_input.held_capture());
    let report = route_alt_escape(public, &projections);
    assert_eq!(report.keys_suppressed_no_focus, 2);
    projections[0].presented_keyboard = sophia_engine::PresentedKeyboardScope::Modal;
    let report = route_alt_escape(public, &projections);
    assert_eq!(report.keys_suppressed_no_focus, 0, "a head shows the modal frame");
}

/// A visible output whose head mode is unknown aggregates to Modal and
/// swallows, as before the held capture.
#[test]
fn an_unknown_presented_head_shields_as_modal() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let mut projections = held_presented(public, true);
    projections[0].policy_publication = None;
    projections[0].presented_keyboard = sophia_engine::PresentedKeyboardScope::Modal;
    let report = route_alt_escape(public, &projections);
    assert_eq!(report.keys_suppressed_no_focus, 0);
}

/// A held publication admitted before any frame presents it shields nothing:
/// modifiers and keys pass, and no action is taken.
#[test]
fn an_admitted_held_capture_with_no_presented_frame_passes_everything() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let mut projections = held_presented(public, false);
    projections[0].policy_visible = false;
    projections[0].policy_publication = None;
    projections[0].presented_keyboard = sophia_engine::PresentedKeyboardScope::None;
    let report = route_alt_escape(public, &projections);
    assert!(actions(&report).is_empty(), "{:?}", report.policy_inputs);
    assert_eq!(report.keys_suppressed_no_focus, 4);
}

/// Two shielded outputs, one still presenting modal pixels: Modal outranks
/// Held across outputs too, so Alt is swallowed.
#[test]
fn a_modal_output_outranks_a_held_one_while_shielding() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let mut projections = held_presented(public, true);
    public.presentation_input.revoke();
    let mut modal = projections[0].clone();
    modal.output = sophia_protocol::OutputId::from_raw(999);
    modal.presented_keyboard = sophia_engine::PresentedKeyboardScope::Modal;
    projections.push(modal);
    let report = route_alt_escape(public, &projections);
    assert_eq!(report.keys_suppressed_no_focus, 0, "the modal output keeps Alt");
}
