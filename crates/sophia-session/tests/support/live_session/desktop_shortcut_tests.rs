use super::*;
use crate::live_session::{SessionApplicationConfig, SessionCommandRegistry};

fn shortcut_candidate(
    bindings: Vec<sophia_config::DesktopShortcutBinding>,
) -> sophia_config::DesktopShortcutCandidate {
    sophia_config::DesktopShortcutCandidate {
        generation: sophia_config::ConfigGeneration::INITIAL,
        digest: sophia_config::ConfigDigest::new([7; 32]),
        profile: "test".to_owned(),
        bindings,
        leaders: Vec::new(),
        timing: sophia_config::DesktopShortcutTiming::default(),
    }
}

fn key_shortcut(
    trigger: &str,
    target: sophia_config::DesktopShortcutTarget,
) -> sophia_config::DesktopShortcutBinding {
    sophia_config::DesktopShortcutBinding {
        label: None,
        group: None,
        chord: sophia_config::DesktopShortcutChord {
            kind: sophia_config::DesktopShortcutBindingKind::Key,
            modifiers: sophia_config::DesktopShortcutModifiers::SUPER,
            trigger: trigger.to_owned(),
        },
        steps: Vec::new(),
        hold_ms: None,
        target,
    }
}

#[test]
fn desktop_shortcuts_resolve_against_the_policy_action_catalog() {
    let target = sophia_config::DesktopShortcutTarget::PolicyAction("focus-next".to_owned());
    let candidate = shortcut_candidate(vec![
        key_shortcut("j", target.clone()),
        key_shortcut("l", target),
        key_shortcut(
            "return",
            sophia_config::DesktopShortcutTarget::Session(
                sophia_config::DesktopSessionShortcut::LaunchTerminal,
            ),
        ),
    ]);
    let configuration = sophia_protocol::PolicyConfiguration {
        action_lifecycles: Vec::new(),
        connection_epoch: 1,
        generation: 1,
        actions: vec![
            sophia_protocol::PolicyActionRegistration {
                action: WmActionId::from_raw(1),
                name: "focus-next".to_owned(),
                session_operation_slot: None,
            },
            sophia_protocol::PolicyActionRegistration {
                action: WmActionId::from_raw(2),
                name: "spawn-terminal".to_owned(),
                session_operation_slot: Some(1),
            },
        ],
        chrome: sophia_protocol::WmChromePolicy::default(),
    };

    let mut registry = resolve_public_shortcuts(
        &candidate,
        &configuration,
        candidate.generation.raw(),
        &SessionCommandRegistry::prepare(1, &SessionApplicationConfig::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(registry.binding_count(), 3);
    assert_eq!(
        registry.handle_key(
            36,
            WmModifierMask {
                bits: WmModifierMask::SUPER,
            },
            true,
        ),
        sophia_engine::WmShortcutDecision {
            action: Some(WmActionId::from_raw(1)),
            consumed: true,
            chord: None,
        }
    );
}

/// Every chording shape installs (t277 D3b), each as one registry entry;
/// D1 refused them by name until their matching existed.
#[test]
fn chording_shapes_install_as_their_own_shapes() {
    let registration = |raw, name: &str| sophia_protocol::PolicyActionRegistration {
        action: WmActionId::from_raw(raw),
        name: name.to_owned(),
        session_operation_slot: None,
    };
    let configuration = sophia_protocol::PolicyConfiguration {
        action_lifecycles: Vec::new(),
        connection_epoch: 1,
        generation: 1,
        actions: vec![registration(1, "focus-next"), registration(2, "hint")],
        chrome: sophia_protocol::WmChromePolicy::default(),
    };
    let commands =
        SessionCommandRegistry::prepare(1, &SessionApplicationConfig::default()).unwrap();
    let target = sophia_config::DesktopShortcutTarget::PolicyAction("focus-next".to_owned());
    let resolve = |candidate: &sophia_config::DesktopShortcutCandidate| {
        resolve_public_shortcuts(
            candidate,
            &configuration,
            candidate.generation.raw(),
            &commands,
        )
        .map(|registry| registry.binding_count())
    };
    let candidate = shortcut_candidate(vec![key_shortcut("j", target.clone())]);
    assert_eq!(resolve(&candidate), Ok(1));

    let mut sequence = candidate.clone();
    sequence.bindings[0]
        .steps
        .push(key_shortcut("k", target.clone()).chord);
    assert_eq!(resolve(&sequence), Ok(1));

    let mut hold = candidate.clone();
    hold.bindings[0].hold_ms = Some(500);
    assert_eq!(resolve(&hold), Ok(1));

    let mut tap = candidate.clone();
    tap.bindings[0].chord.modifiers = sophia_config::DesktopShortcutModifiers::NONE;
    tap.bindings[0].chord.trigger = "super".to_owned();
    assert_eq!(resolve(&tap), Ok(1));

    let mut leader = sequence.clone();
    leader.leaders.push(sophia_config::DesktopShortcutLeader {
        chord: key_shortcut("j", target.clone()).chord,
        steps: Vec::new(),
        action: "hint".to_owned(),
        label: None,
        group: None,
    });
    assert_eq!(resolve(&leader), Ok(2));
    leader.leaders[0].action = "unknown".to_owned();
    assert_eq!(
        resolve(&leader),
        Err("shortcut leader names an unregistered policy action")
    );

    let mut timing = candidate;
    timing.timing.tap_ms = 300;
    assert_eq!(resolve(&timing), Ok(1));
}

#[test]
fn desktop_shortcuts_reject_unregistered_policy_semantics() {
    let candidate = shortcut_candidate(vec![key_shortcut(
        "j",
        sophia_config::DesktopShortcutTarget::PolicyAction("unknown".to_owned()),
    )]);
    let configuration = sophia_protocol::PolicyConfiguration {
        action_lifecycles: Vec::new(),
        connection_epoch: 1,
        generation: 1,
        actions: Vec::new(),
        chrome: sophia_protocol::WmChromePolicy::default(),
    };

    assert_eq!(
        resolve_public_shortcuts(
            &candidate,
            &configuration,
            candidate.generation.raw(),
            &SessionCommandRegistry::prepare(1, &SessionApplicationConfig::default()).unwrap()
        ),
        Err("shortcut names an unregistered policy action")
    );
}

#[test]
fn descriptor_switcher_shortcut_is_session_owned() {
    let candidate = shortcut_candidate(vec![key_shortcut(
        "p",
        sophia_config::DesktopShortcutTarget::Session(
            sophia_config::DesktopSessionShortcut::WindowSwitcher,
        ),
    )]);
    let configuration = sophia_protocol::PolicyConfiguration {
        action_lifecycles: Vec::new(),
        connection_epoch: 1,
        generation: 1,
        actions: Vec::new(),
        chrome: sophia_protocol::WmChromePolicy::default(),
    };

    let mut registry = resolve_public_shortcuts(
        &candidate,
        &configuration,
        candidate.generation.raw(),
        &SessionCommandRegistry::prepare(1, &SessionApplicationConfig::default()).unwrap(),
    )
    .unwrap();
    let decision = registry.handle_key(
        25,
        WmModifierMask {
            bits: WmModifierMask::SUPER,
        },
        true,
    );
    assert!(decision.consumed);
    assert!(decision.action.is_some_and(is_shell_switcher_shortcut));
}

#[test]
fn policy_cannot_claim_the_descriptor_switcher_action_identity() {
    let candidate = shortcut_candidate(Vec::new());
    let configuration = sophia_protocol::PolicyConfiguration {
        action_lifecycles: Vec::new(),
        connection_epoch: 1,
        generation: 1,
        actions: vec![sophia_protocol::PolicyActionRegistration {
            action: SHELL_SWITCHER_SHORTCUT_ACTION,
            name: "collision".to_owned(),
            session_operation_slot: None,
        }],
        chrome: sophia_protocol::WmChromePolicy::default(),
    };

    assert_eq!(
        resolve_public_shortcuts(
            &candidate,
            &configuration,
            candidate.generation.raw(),
            &SessionCommandRegistry::prepare(1, &SessionApplicationConfig::default()).unwrap()
        ),
        Err("policy action collides with a reserved session shortcut")
    );
}

fn chord(
    modifiers: sophia_config::DesktopShortcutModifiers,
    trigger: &str,
) -> sophia_config::DesktopShortcutChord {
    sophia_config::DesktopShortcutChord {
        kind: sophia_config::DesktopShortcutBindingKind::Key,
        modifiers,
        trigger: trigger.to_owned(),
    }
}

/// D3b: every key shape resolves to the engine plan with the same target
/// rules as immediate chords; leaders resolve only as policy actions, and
/// timing carries over.
#[test]
fn every_shortcut_shape_resolves_into_the_engine_plan() {
    use sophia_config::{DesktopShortcutModifiers as M, DesktopShortcutTarget as T};
    let policy = |name: &str| T::PolicyAction(name.to_owned());
    let mut tap = key_shortcut("super", policy("launcher"));
    tap.chord = chord(M::NONE, "super");
    let mut hold = key_shortcut(
        "q",
        T::Session(sophia_config::DesktopSessionShortcut::Logout),
    );
    hold.hold_ms = Some(500);
    let mut sequence = key_shortcut(
        "w",
        T::Session(sophia_config::DesktopSessionShortcut::ShortcutHelp),
    );
    sequence.steps = vec![chord(M::NONE, "k")];
    let mut candidate = shortcut_candidate(vec![
        key_shortcut("q", policy("close")),
        hold,
        tap,
        sequence,
    ]);
    candidate
        .leaders
        .push(sophia_config::DesktopShortcutLeader {
            chord: chord(M::SUPER, "w"),
            steps: Vec::new(),
            action: "hint".to_owned(),
            label: None,
            group: None,
        });
    candidate.timing = sophia_config::DesktopShortcutTiming {
        tap_ms: 300,
        sequence_ms: 2000,
    };
    let action = WmActionId::from_raw;
    let policy_actions = BTreeMap::from([
        ("launcher", action(1)),
        ("close", action(2)),
        ("hint", action(3)),
    ]);
    let session_actions = BTreeMap::from([((4_u16, "logout"), action(4))]);
    let commands =
        SessionCommandRegistry::prepare(1, &SessionApplicationConfig::default()).unwrap();
    let plan = resolve_shortcut_plan(
        &candidate,
        &policy_actions,
        &session_actions,
        &commands,
        &[],
    )
    .unwrap();
    let step = |keycode, modifiers| sophia_engine::WmKeyStep { keycode, modifiers };
    let super_bit = u32::from(M::SUPER.bits());
    assert_eq!(
        plan.immediate,
        [sophia_protocol::WmBindingRegistration {
            action: action(2),
            keycode: 16,
            modifiers: WmModifierMask { bits: super_bit },
        }]
    );
    assert_eq!(
        plan.holds,
        [sophia_engine::WmHoldBinding {
            step: step(16, super_bit),
            hold_ms: 500,
            action: action(4),
        }]
    );
    assert_eq!(
        plan.taps,
        [sophia_engine::WmModifierTapBinding {
            modifier: WmModifierMask::SUPER,
            action: action(1),
        }]
    );
    assert_eq!(
        plan.sequences,
        [sophia_engine::WmSequenceBinding {
            steps: vec![step(17, super_bit), step(37, 0)],
            action: SHELL_HELP_SHORTCUT_ACTION,
        }]
    );
    assert_eq!(
        plan.leaders,
        [sophia_engine::WmSequenceLeader {
            steps: vec![step(17, super_bit)],
            action: action(3),
        }]
    );
    assert_eq!(
        plan.timing,
        sophia_engine::WmShortcutTiming {
            tap_ms: 300,
            sequence_ms: 2000,
        }
    );
    // A dropped compiled default leaves no shape behind.
    let dropped = [sophia_config::DesktopSessionShortcut::Logout];
    let plan = resolve_shortcut_plan(
        &candidate,
        &policy_actions,
        &session_actions,
        &commands,
        &dropped,
    )
    .unwrap();
    assert!(plan.holds.is_empty());
    // A leader is never a session capability or an unknown action.
    candidate.leaders[0].action = "logout".to_owned();
    assert_eq!(
        resolve_shortcut_plan(
            &candidate,
            &policy_actions,
            &session_actions,
            &commands,
            &[]
        ),
        Err("shortcut leader names an unregistered policy action")
    );
}

/// D3b: a profile carrying every shape installs a router that behaves as
/// those shapes. The modifier tap fires on a quick release only; a tap and
/// hold on one chord fire exactly one of the two; the leader opens and its
/// sequence completes; and the profile's own timing sets the tap window and
/// the sequence timeout.
#[test]
fn a_resolved_profile_routes_each_shape_as_its_shape() {
    use crate::live_session::tests::shortcut_keys::route_test_key;
    use sophia_config::{DesktopShortcutModifiers as M, DesktopShortcutTarget as T};
    use sophia_engine::{WmChordEvent, WmShortcutOutput, WmShortcutRouter};
    use sophia_protocol::PolicyChordEnd;
    let policy = |name: &str| T::PolicyAction(name.to_owned());
    let mut tap = key_shortcut("super", policy("launcher"));
    tap.chord = chord(M::NONE, "super");
    let mut hold = key_shortcut("q", policy("force-close"));
    hold.hold_ms = Some(500);
    let mut sequence = key_shortcut("w", policy("split"));
    sequence.steps = vec![chord(M::NONE, "k")];
    let mut candidate = shortcut_candidate(vec![
        tap,
        key_shortcut("q", policy("close")),
        hold,
        sequence,
    ]);
    candidate
        .leaders
        .push(sophia_config::DesktopShortcutLeader {
            chord: chord(M::SUPER, "w"),
            steps: Vec::new(),
            action: "hint".to_owned(),
            label: None,
            group: None,
        });
    candidate.timing = sophia_config::DesktopShortcutTiming {
        tap_ms: 300,
        sequence_ms: 2000,
    };
    let names = ["launcher", "close", "force-close", "split", "hint"];
    let configuration = sophia_protocol::PolicyConfiguration {
        // The WM follows the leader's chord, or the leader never fires.
        action_lifecycles: vec![sophia_protocol::PolicyActionLifecycleInterest {
            action: WmActionId::from_raw(5),
            held_ms: 0,
        }],
        connection_epoch: 1,
        generation: 1,
        actions: (1..)
            .zip(names)
            .map(|(raw, name)| sophia_protocol::PolicyActionRegistration {
                action: WmActionId::from_raw(raw),
                name: name.to_owned(),
                session_operation_slot: None,
            })
            .collect(),
        chrome: sophia_protocol::WmChromePolicy::default(),
    };
    let commands =
        SessionCommandRegistry::prepare(1, &SessionApplicationConfig::default()).unwrap();
    let registry = resolve_public_shortcuts(&candidate, &configuration, 1, &commands).unwrap();
    let mut router = WmShortcutRouter::new(registry);
    router.set_action_lifecycles(&configuration.action_lifecycles);
    let action = |name| {
        let raw = names.iter().position(|known| *known == name).unwrap() + 1;
        WmActionId::from_raw(u64::try_from(raw).unwrap())
    };
    let (seat, device) = (SeatId::from_raw(1), sophia_protocol::DeviceId::from_raw(1));
    let (super_key, q, w, k) = (125, 16, 17, 37);
    // Every activation from one key event, and whether it opened a chord.
    let key = |router: &mut WmShortcutRouter, keycode, pressed, now| {
        let (_, _, outputs) = route_test_key(router, seat, device, keycode, pressed, now);
        outputs
    };
    let activations = |outputs: &[WmShortcutOutput]| -> Vec<WmActionId> {
        outputs
            .iter()
            .filter_map(|output| match output {
                WmShortcutOutput::Activation(activation) => Some(activation.action),
                WmShortcutOutput::Chord(_) => None,
            })
            .collect()
    };
    let ends = |outputs: &[WmShortcutOutput]| -> Vec<PolicyChordEnd> {
        outputs
            .iter()
            .filter_map(|output| match output {
                WmShortcutOutput::Chord(WmChordEvent::Ended { end, .. }) => Some(*end),
                _ => None,
            })
            .collect()
    };

    // Modifier tap: within the profile's 300 ms, then past it. Clients still
    // see both edges of the modifier.
    let (consumed, _, armed) = route_test_key(&mut router, seat, device, super_key, true, 0);
    assert!(!consumed && armed.is_empty());
    let (consumed, _, tapped) = route_test_key(&mut router, seat, device, super_key, false, 299);
    assert!(!consumed);
    assert_eq!(activations(&tapped), [action("launcher")]);
    assert!(key(&mut router, super_key, true, 1_000).is_empty());
    assert!(activations(&key(&mut router, super_key, false, 1_301)).is_empty());

    // Tap and hold on Super+q: a quick release is the tap, and the modifier
    // tap it interrupted stays silent.
    key(&mut router, super_key, true, 2_000);
    assert!(key(&mut router, q, true, 2_010).is_empty());
    assert_eq!(
        activations(&key(&mut router, q, false, 2_100)),
        [action("close")]
    );
    assert!(activations(&key(&mut router, super_key, false, 2_200)).is_empty());
    // Held to its deadline, it is the hold, and the release adds nothing.
    key(&mut router, super_key, true, 3_000);
    key(&mut router, q, true, 3_010);
    router.poll_shortcuts(3_509);
    assert!(router.take_outputs().is_empty());
    router.poll_shortcuts(3_510);
    assert_eq!(activations(&router.take_outputs()), [action("force-close")]);
    assert!(key(&mut router, q, false, 3_600).is_empty());
    assert!(activations(&key(&mut router, super_key, false, 3_700)).is_empty());

    // Leader then its sequence.
    key(&mut router, super_key, true, 4_000);
    assert_eq!(
        activations(&key(&mut router, w, true, 4_010)),
        [action("hint")]
    );
    key(&mut router, w, false, 4_020);
    key(&mut router, super_key, false, 4_030);
    let completed = key(&mut router, k, true, 4_100);
    assert_eq!(activations(&completed), [action("split")]);
    assert_eq!(ends(&completed), [PolicyChordEnd::Completed]);
    key(&mut router, k, false, 4_110);

    // The profile's 2000 ms sequence timeout, and k alone afterwards is not
    // a step.
    key(&mut router, super_key, true, 5_000);
    assert_eq!(
        activations(&key(&mut router, w, true, 5_010)),
        [action("hint")]
    );
    key(&mut router, w, false, 5_020);
    key(&mut router, super_key, false, 5_030);
    router.poll_shortcuts(7_009);
    assert!(router.take_outputs().is_empty());
    router.poll_shortcuts(7_010);
    assert_eq!(ends(&router.take_outputs()), [PolicyChordEnd::TimedOut]);
    let (consumed, fired, _) = route_test_key(&mut router, seat, device, k, true, 7_100);
    assert!(!consumed && fired.is_none());
}
