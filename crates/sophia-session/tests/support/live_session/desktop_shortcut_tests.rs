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

/// Profile shapes whose matching is not wired yet are refused by name, never
/// installed as their immediate bindings alone (t277 D1).
#[test]
fn chording_shapes_are_refused_until_their_matching_exists() {
    let configuration = sophia_protocol::PolicyConfiguration {
        action_lifecycles: Vec::new(),
        connection_epoch: 1,
        generation: 1,
        actions: vec![sophia_protocol::PolicyActionRegistration {
            action: WmActionId::from_raw(1),
            name: "focus-next".to_owned(),
            session_operation_slot: None,
        }],
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
    let mut candidate = shortcut_candidate(vec![key_shortcut("j", target.clone())]);
    // The default timing, written out, is no shape at all.
    candidate.timing = sophia_config::DesktopShortcutTiming {
        tap_ms: 400,
        sequence_ms: 1000,
    };
    assert_eq!(resolve(&candidate), Ok(1));

    let mut sequence = candidate.clone();
    sequence.bindings[0]
        .steps
        .push(key_shortcut("k", target.clone()).chord);
    assert_eq!(
        resolve(&sequence),
        Err("shortcut sequences are not yet supported")
    );

    let mut hold = candidate.clone();
    hold.bindings[0].hold_ms = Some(500);
    assert_eq!(resolve(&hold), Err("hold shortcuts are not yet supported"));

    let mut tap = candidate.clone();
    tap.bindings[0].chord.modifiers = sophia_config::DesktopShortcutModifiers::NONE;
    tap.bindings[0].chord.trigger = "super".to_owned();
    assert_eq!(
        resolve(&tap),
        Err("modifier tap shortcuts are not yet supported")
    );

    let mut leader = sequence.clone();
    leader.bindings[0].steps.clear();
    leader.leaders.push(sophia_config::DesktopShortcutLeader {
        chord: key_shortcut("w", target.clone()).chord,
        steps: Vec::new(),
        action: "focus-next".to_owned(),
        label: None,
        group: None,
    });
    assert_eq!(
        resolve(&leader),
        Err("shortcut leaders are not yet supported")
    );

    let mut timing = candidate;
    timing.timing.tap_ms = 300;
    assert_eq!(
        resolve(&timing),
        Err("shortcut timing is not yet supported")
    );
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
