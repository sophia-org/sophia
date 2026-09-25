use super::*;

#[test]
fn dock_only_profile_requires_catalog_and_input_before_endpoint_construction() {
    use std::os::unix::fs::PermissionsExt;
    let profile =
        std::env::temp_dir().join(format!("sophia-dock-profile-{}.kdl", std::process::id()));
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/fixtures/lom_panel_core.kdl");
    for (input, catalog) in [(false, false), (true, false), (false, true), (true, true)] {
        let source = format!(
            r#"schema 1
shell {{ enabled #true; content #true; content-input #{input}; }}
shortcut {{ profile "dock-test"; }}
session {{
    shell-component "dock" "dock" {{ executable "/absent/provlita"; reservation "bottom" 64; gpu "denied"; }}
    {}
    startup
}}
"#,
            if catalog {
                "application-catalog \"lom-panel-gate\""
            } else {
                ""
            }
        );
        std::fs::write(&profile, source).unwrap();
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
        let result = PersistentXtermSessionConfig::from_args(&[
            "--session-mode=normal".into(),
            "--wm-process=/absent/hagia".into(),
            format!("--config={}", core.display()),
            format!("--desktop-profile={}", profile.display()),
        ]);
        if input && catalog {
            let config = result.unwrap();
            assert!(config.shell_content_input_enabled);
            assert!(config.application_catalog.is_some());
        } else {
            let error = result.expect_err("incomplete dock policy must refuse");
            assert!(
                error
                    .to_string()
                    .contains("catalog shell components require"),
                "{error}"
            );
        }
    }
    std::fs::remove_file(profile).unwrap();
}

// Parsing fixtures must not inherit the operator's applications or shortcuts:
// discovered applications implicitly select normal mode, and desktop bindings
// may require capabilities unrelated to the behavior under test. Keep explicit
// source-selection and --no-config cases intact.
pub(super) fn isolated_session_config(
    args: &[String],
) -> Result<PersistentXtermSessionConfig, Box<dyn std::error::Error>> {
    let mut arguments = args.to_vec();
    if !arguments.iter().any(|argument| argument == "--no-config") {
        if !arguments
            .iter()
            .any(|argument| argument.starts_with("--config="))
        {
            arguments.push(isolated_core_config_argument());
        }
        if !arguments
            .iter()
            .any(|argument| argument.starts_with("--desktop-profile="))
        {
            arguments.push(isolated_desktop_profile_argument());
        }
    }
    PersistentXtermSessionConfig::from_args(&arguments)
}

/// The core configuration a test must name so it does not discover the
/// operator's own.
///
/// Without `--config` (or `--no-config`, which an explicit `--desktop-profile`
/// forbids) core discovery falls through to `~/.config/sophia/config.kdl`.
/// A test that lands there asserts against whatever desktop the machine
/// happens to run: two of these passed on a machine with no Sophia config and
/// failed on one with it.
fn isolated_core_config_argument() -> String {
    let core =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/config/sophia/core.kdl");
    format!("--config={}", core.display())
}

fn isolated_desktop_profile_argument() -> String {
    let profile = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tools/fixtures/mixed_output_probe.kdl");
    format!("--desktop-profile={}", profile.display())
}

#[test]
fn firefox_m10_kitty_proof_requires_only_retention_checkpoints() {
    let mut proof = FirefoxM10KittyProof::default();
    let expected = [
        (193, "a", "before"),
        (194, "b", "before"),
        (211, "a", "after_normal_close"),
        (212, "b", "after_normal_close"),
        (229, "a", "after_forced_close"),
        (230, "b", "after_forced_close"),
    ];

    for (index, (title_bytes, terminal, checkpoint)) in expected.iter().enumerate() {
        assert_eq!(
            proof.observe("_NET_WM_NAME", *title_bytes),
            Some((*terminal, *checkpoint)),
        );
        assert_eq!(proof.completed(), index + 1);
        assert_eq!(proof.observe("_NET_WM_NAME", *title_bytes), None);
    }
    assert!(proof.complete());
    assert!(proof.lifecycle_complete());
}

#[test]
fn firefox_promotion_stage_proof_skips_focused_selection_stages() {
    let mut proof = FirefoxM8StageProof::promotion();
    assert!(proof.observe("_NET_WM_NAME", 24).is_empty());
    assert_eq!(
        proof.observe("_NET_WM_NAME", 40),
        vec![("loaded", 0, 24), ("keyboard", 1, 40)]
    );
    assert!(proof.navigation_ready("_NET_WM_NAME", 73));
    for (title_bytes, stage, index) in [(88, "scroll", 2), (104, "layout", 3), (120, "refocus", 4)]
    {
        assert_eq!(
            proof.observe("_NET_WM_NAME", title_bytes),
            vec![(stage, index, title_bytes)]
        );
    }
    assert!(proof.dialog_ready("_NET_WM_NAME", 121));
    assert_eq!(proof.observe("_NET_WM_NAME", 136), vec![("dialog", 5, 136)]);
    assert!(proof.complete());
}

#[test]
fn firefox_full_stage_proof_retains_selection_stages() {
    let mut proof = FirefoxM8StageProof::default();
    assert!(proof.observe("_NET_WM_NAME", 24).is_empty());
    assert_eq!(
        proof.observe("_NET_WM_NAME", 40),
        vec![("loaded", 0, 24), ("keyboard", 1, 40)]
    );
    for (title_bytes, stage, index) in [
        (56, "clipboard", 2),
        (72, "primary", 3),
        (88, "scroll", 4),
        (104, "resize", 5),
        (120, "refocus", 6),
        (136, "dialog", 7),
    ] {
        assert_eq!(
            proof.observe("_NET_WM_NAME", title_bytes),
            vec![(stage, index, title_bytes)]
        );
    }
    assert!(proof.complete());
}

#[test]
fn focused_selection_kitty_proof_requires_all_three_checkpoints() {
    let mut proof = FirefoxM10SelectionKittyProof::default();
    for (title_bytes, checkpoint) in [
        (241, "before"),
        (242, "clipboard_peer"),
        (243, "primary_peer"),
    ] {
        assert_eq!(proof.observe("_NET_WM_NAME", title_bytes), Some(checkpoint));
    }
    assert_eq!(proof.completed(), 3);
    assert!(proof.complete());
}

#[test]
fn focused_dialog_proof_requires_ordered_unique_checkpoints() {
    let mut proof = FirefoxM10DialogProof::default();
    assert_eq!(proof.observe("_NET_WM_NAME", 246), None);
    for (title_bytes, checkpoint) in [
        (245, "page_ready"),
        (246, "modal_ready"),
        (247, "confirmed"),
    ] {
        assert_eq!(proof.observe("_NET_WM_NAME", title_bytes), Some(checkpoint));
    }
    assert!(proof.complete());
    assert_eq!(proof.observe("_NET_WM_NAME", 247), None);
}

#[test]
fn focused_primary_proof_requires_ordered_unique_checkpoints() {
    let mut proof = FirefoxM10PrimaryProof::default();
    assert_eq!(proof.observe("_NET_WM_NAME", 250), None);
    assert_eq!(proof.observe("_NET_WM_NAME", 253), None);
    for (title_bytes, checkpoint) in [
        (251, "source_armed"),
        (253, "kitty_received"),
        (252, "confirmed"),
    ] {
        assert_eq!(proof.observe("_NET_WM_NAME", title_bytes), Some(checkpoint));
    }
    assert!(proof.complete());
    assert_eq!(proof.observe("_NET_WM_NAME", 252), None);
}

#[test]
fn firefox_physical_slices_are_mutually_exclusive() {
    let base = [
        "--session-mode=normal".to_owned(),
        "--session-app=firefox=/usr/bin/firefox".to_owned(),
        "--session-action-app=browser=firefox".to_owned(),
        isolated_desktop_profile_argument(),
    ];
    for proof in [
        "--firefox-m10-rendering-proof",
        "--firefox-m10-dialog-proof",
        "--firefox-m10-primary-proof",
        "--firefox-m10-selection-proof",
        "--firefox-m10-lifecycle-proof",
    ] {
        let mut arguments = base.to_vec();
        arguments.push(proof.to_owned());
        let config = PersistentXtermSessionConfig::from_args(&arguments).unwrap();
        assert!(config.firefox_proof_requested());
        assert!(!config.firefox_full_proof_requested());
    }

    let mut conflicting = base.to_vec();
    conflicting.extend([
        "--firefox-m10-primary-proof".to_owned(),
        "--firefox-m10-selection-proof".to_owned(),
    ]);
    assert!(
        PersistentXtermSessionConfig::from_args(&conflicting)
            .unwrap_err()
            .to_string()
            .contains("select only one Firefox proof mode")
    );
}

#[test]
fn live_x_session_profiles_are_explicit_and_fail_closed() {
    let classic = isolated_session_config(&[]).unwrap();
    assert_eq!(classic.namespace_profile, NamespaceProfile::ClassicShared);
    assert_eq!(classic.namespace_capabilities, NamespaceCapabilities::NONE);

    let confined = isolated_session_config(&["--namespace-profile=confined".to_owned()]).unwrap();
    assert_eq!(confined.namespace_profile, NamespaceProfile::Confined);
    assert_eq!(confined.namespace_capabilities, NamespaceCapabilities::NONE);

    assert!(
        isolated_session_config(&["--namespace-profile=unknown".to_owned()])
            .unwrap_err()
            .to_string()
            .contains("expected classic or confined")
    );
}

#[test]
fn public_policy_profile_activation_is_mandatory() {
    isolated_session_config(&[
        "--wm-process=/usr/bin/true".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
    ])
    .unwrap();
    // Retain the old proof switch as a harmless compatibility argument. It no
    // longer controls whether the activation barrier runs.
    isolated_session_config(&[
        "--wm-process=/usr/bin/true".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
        "--wm-profile-activation".to_owned(),
    ])
    .unwrap();
}

#[test]
fn public_policy_child_executable_grant_is_explicit_and_read_only() {
    let config = isolated_session_config(&[
        "--wm-process=/usr/bin/true".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
        "--wm-process-executable-grant=/usr/bin/true".to_owned(),
    ])
    .unwrap();
    assert_eq!(
        config.wm_process_executable_grants,
        [std::path::PathBuf::from("/usr/bin/true")]
    );

    let spec = public_policy_launch_spec(
        &config,
        "/usr/bin/true",
        std::path::Path::new("/run/user/1000/sophia/policy/endpoint/wm.sock"),
        std::path::Path::new("/run/user/1000/sophia/policy/checkpoint/policy.checkpoint"),
        std::path::Path::new("/run/user/1000/sophia/policy/policy.profile.kdl"),
        false,
        None,
    )
    .unwrap();
    let domain = spec.protection_domain.as_ref().unwrap();
    assert_eq!(
        domain.paths().last(),
        Some(&sophia_runtime::ProtectionPath::read_only("/usr/bin/true"))
    );

    assert!(
        isolated_session_config(&[
            "--wm-process-executable-grant=/opt/sophia/native-wm".to_owned(),
        ])
        .unwrap_err()
        .to_string()
        .contains("requires --wm-process")
    );
    assert!(
        isolated_session_config(&[
            "--wm-process=/usr/bin/true".to_owned(),
            "--wm-process-executable-grant=relative/native-wm".to_owned(),
        ])
        .unwrap_err()
        .to_string()
        .contains("requires an absolute path")
    );
}

include!("session_config_tests/desktop_profile.rs");

#[test]
fn public_policy_session_operation_tokens_are_fresh_and_slot_stable() {
    let config = isolated_session_config(&[]).unwrap();
    let (first, _) = public_session_operations(&config);
    let (second, _) = public_session_operations(&config);

    assert_eq!(
        first
            .iter()
            .map(|operation| operation.slot)
            .collect::<Vec<_>>(),
        second
            .iter()
            .map(|operation| operation.slot)
            .collect::<Vec<_>>()
    );
    assert!(
        first
            .iter()
            .all(|left| { second.iter().all(|right| left.token != right.token) })
    );
}

#[test]
fn public_policy_output_reappearance_advances_its_generation() {
    let output = sophia_engine::HeadlessOutput::deterministic();
    let mut generations = std::collections::BTreeMap::new();
    let mut live = std::collections::BTreeSet::new();

    observe_public_output_generations(&mut generations, &mut live, &[output]).unwrap();
    assert_eq!(generations.get(&output.id), Some(&1));
    observe_public_output_generations(&mut generations, &mut live, &[]).unwrap();
    observe_public_output_generations(&mut generations, &mut live, &[output]).unwrap();

    assert_eq!(generations.get(&output.id), Some(&2));
}

#[test]
fn public_policy_complete_topology_admission_is_atomic_and_generation_aware() {
    let first = sophia_engine::HeadlessOutput::deterministic();
    let second = sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(2),
        ..first
    };
    let mut generations = std::collections::BTreeMap::new();
    let mut live = std::collections::BTreeSet::new();
    let mut active = first.id;

    assert!(
        observe_public_output_topology(&mut generations, &mut live, &mut active, &[first, second],)
            .unwrap()
    );
    assert_eq!(generations.get(&first.id), Some(&1));
    assert_eq!(generations.get(&second.id), Some(&1));

    assert!(
        observe_public_output_topology(&mut generations, &mut live, &mut active, &[second],)
            .unwrap()
    );
    assert_eq!(active, second.id);

    let before = (generations.clone(), live.clone(), active);
    assert!(
        observe_public_output_topology(
            &mut generations,
            &mut live,
            &mut active,
            &[second, second],
        )
        .is_err()
    );
    assert_eq!((generations.clone(), live.clone(), active), before);

    assert!(
        observe_public_output_topology(&mut generations, &mut live, &mut active, &[first, second],)
            .unwrap()
    );
    assert_eq!(generations.get(&first.id), Some(&2));
    assert_eq!(active, second.id);
}

#[test]
fn public_policy_restart_aborts_settlement_before_process_replacement() {
    for (restart, exited) in [(true, false), (false, true), (true, true)] {
        assert_eq!(
            public_policy_restart_decision(restart, exited, true),
            PublicPolicyRestartDecision::AbortSettlement,
        );
        assert_eq!(
            public_policy_restart_decision(restart, exited, false),
            PublicPolicyRestartDecision::Restart,
        );
    }
    assert_eq!(
        public_policy_restart_decision(false, false, true),
        PublicPolicyRestartDecision::Idle,
    );
}

#[test]
fn output_topology_effect_is_a_restart_settlement_barrier() {
    assert!(public_policy_restart_settlement_pending(false, true));
    assert!(public_policy_restart_settlement_pending(true, false));
    assert!(!public_policy_restart_settlement_pending(false, false));
}

#[test]
fn public_policy_checkpoint_parent_survives_peer_endpoint_replacement() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{SystemTime, UNIX_EPOCH};

    let path = std::env::temp_dir().join(format!(
        "sophia-policy-session-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let directory = PolicySessionDirectory::create(path.clone()).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    std::fs::write(directory.checkpoint_path(), b"private checkpoint").unwrap();
    let endpoint_path = directory.endpoint_path();
    let endpoint = sophia_runtime::PolicyWmSessionTransport::bind_for_supervised_uid(
        &endpoint_path,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    drop(endpoint);
    assert!(directory.checkpoint_path().is_file());
    assert!(!endpoint_path.exists());

    let replacement = sophia_runtime::PolicyWmSessionTransport::bind_for_supervised_uid(
        &endpoint_path,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    drop(replacement);
    drop(directory);
    assert!(!path.exists());
}

#[test]
fn public_policy_owner_fault_points_are_bounded_proof_controls() {
    for (value, expected) in [
        ("proposal_staged", PublicPolicyFaultPoint::ProposalStaged),
        ("frontend_pending", PublicPolicyFaultPoint::FrontendPending),
        ("prepared", PublicPolicyFaultPoint::Prepared),
        (
            "terminal_outcome_queued",
            PublicPolicyFaultPoint::TerminalOutcomeQueued,
        ),
    ] {
        let config = isolated_session_config(&[
            "--wm-process=/usr/bin/true".to_owned(),
            "--wm-interface=sophia_wm_v1".to_owned(),
            "--max-runtime-ms=1000".to_owned(),
            format!("--wm-proof-fault-after={value}"),
        ])
        .unwrap();
        assert_eq!(config.wm_public_fault_after, Some(expected));
    }

    assert!(
        isolated_session_config(&[
            "--wm-proof-fault-after=frontend_pending".to_owned(),
            "--max-runtime-ms=1000".to_owned(),
        ])
        .unwrap_err()
        .to_string()
        .contains("requires a configured sophia_wm_v1 --wm-process")
    );
    assert!(
        isolated_session_config(&[
            "--wm-process=/usr/bin/true".to_owned(),
            "--wm-interface=sophia_wm_v1".to_owned(),
            "--max-runtime-ms=1000".to_owned(),
            "--wm-proof-fault-after=unknown".to_owned(),
        ])
        .unwrap_err()
        .to_string()
        .contains("expects proposal_staged")
    );

    let restart = isolated_session_config(&[
        "--wm-process=/usr/bin/true".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
        "--max-runtime-ms=1000".to_owned(),
        "--wm-proof-restart-after-action=66".to_owned(),
    ])
    .unwrap();
    assert_eq!(
        restart.wm_public_restart_after_action,
        Some(WmActionId::from_raw(66))
    );

    for arguments in [
        vec![
            "--wm-process=/usr/bin/true".to_owned(),
            "--wm-interface=sophia_wm_v1".to_owned(),
            "--max-runtime-ms=1000".to_owned(),
            "--wm-proof-restart-after-action=0".to_owned(),
        ],
        vec![
            "--wm-process=/usr/bin/true".to_owned(),
            "--wm-interface=sophia_wm_v1".to_owned(),
            "--max-runtime-ms=1000".to_owned(),
            "--wm-proof-fault-after=prepared".to_owned(),
            "--wm-proof-restart-after-action=66".to_owned(),
        ],
    ] {
        assert!(isolated_session_config(&arguments).is_err());
    }
}

#[test]
fn checkpoint_restart_waits_for_an_atomic_replacement() {
    let first = PolicyCheckpointIdentity {
        device: 1,
        inode: 2,
    };
    let second = PolicyCheckpointIdentity {
        device: 1,
        inode: 3,
    };

    assert!(!policy_checkpoint_replaced(None, None));
    assert!(!policy_checkpoint_replaced(Some(first), None));
    assert!(!policy_checkpoint_replaced(Some(first), Some(first)));
    assert!(policy_checkpoint_replaced(None, Some(first)));
    assert!(policy_checkpoint_replaced(Some(first), Some(second)));
}

#[test]
fn normal_session_application_registry_is_bounded_and_explicit() {
    let config = isolated_session_config(&[
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/xterm".to_owned(),
        "--session-app-arg=terminal=-cm".to_owned(),
        "--session-start=terminal".to_owned(),
        "--session-action-app=terminal=terminal".to_owned(),
        isolated_desktop_profile_argument(),
    ])
    .unwrap();
    assert!(config.normal_session);
    assert_eq!(config.applications.startup, ["terminal"]);
    assert_eq!(
        config
            .application_for_action(WmSessionAction::LaunchApplication {
                application: super::super::TERMINAL_APPLICATION_ID,
            })
            .unwrap()
            .arguments,
        ["-cm"]
    );

    let blank = isolated_session_config(&[
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/kitty".to_owned(),
        "--session-action-app=terminal=terminal".to_owned(),
        isolated_desktop_profile_argument(),
    ])
    .unwrap();
    assert!(blank.applications.startup.is_empty());
    assert!(
        blank
            .application_for_action(WmSessionAction::LaunchApplication {
                application: super::super::TERMINAL_APPLICATION_ID,
            })
            .is_some()
    );

    let dual_terminal = isolated_session_config(&[
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/kitty".to_owned(),
        "--session-app=terminal-secondary=/usr/bin/kitty".to_owned(),
        "--session-start=terminal".to_owned(),
        "--session-start=terminal-secondary".to_owned(),
        "--session-action-app=terminal=terminal".to_owned(),
        isolated_desktop_profile_argument(),
    ])
    .unwrap();
    assert_eq!(
        dual_terminal.applications.startup,
        ["terminal", "terminal-secondary"]
    );
    assert!(!dual_terminal.secondary_terminal);

    for args in [
        vec![
            "--session-mode=normal".to_owned(),
            "--session-app=terminal=xterm".to_owned(),
            "--session-start=terminal".to_owned(),
        ],
        vec![
            "--session-mode=normal".to_owned(),
            "--session-app=terminal=/usr/bin/xterm".to_owned(),
            "--session-start=missing".to_owned(),
        ],
        vec![
            "--session-app=terminal=/usr/bin/xterm".to_owned(),
            "--session-start=terminal".to_owned(),
        ],
        vec![
            "--session-mode=normal".to_owned(),
            "--session-app=terminal=/usr/bin/xterm".to_owned(),
            "--session-app=terminal=/usr/bin/xterm".to_owned(),
            "--session-start=terminal".to_owned(),
        ],
    ] {
        assert!(isolated_session_config(&args).is_err());
    }
}

#[test]
fn mixed_output_gate_apps_satisfy_probe_profile() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let core = root.join("tools/config/sophia/core.kdl");
    let desktop = root.join("tools/fixtures/mixed_output_probe.kdl");
    let config = PersistentXtermSessionConfig::from_args(&[
        format!("--config={}", core.display()),
        format!("--desktop-profile={}", desktop.display()),
        "--session-mode=normal".to_owned(),
        "--session-app=mirror=/usr/bin/kitty".to_owned(),
        "--session-start=mirror".to_owned(),
        "--session-action-app=terminal=mirror".to_owned(),
        "--session-app=proof=/usr/bin/kitty".to_owned(),
        "--session-start=proof".to_owned(),
        "--session-action-app=browser=proof".to_owned(),
        "--wm-process=/usr/bin/true".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
        "--max-runtime-ms=30000".to_owned(),
    ])
    .unwrap();

    assert!(config.shortcut_profile_candidate.bindings.is_empty());
    assert_eq!(
        config
            .application_for_action(WmSessionAction::LaunchApplication {
                application: super::super::TERMINAL_APPLICATION_ID,
            })
            .unwrap()
            .id,
        "mirror"
    );
    assert_eq!(
        config
            .application_for_action(WmSessionAction::LaunchApplication {
                application: super::super::BROWSER_APPLICATION_ID,
            })
            .unwrap()
            .id,
        "proof"
    );
}

#[test]
fn frame_fed_output_gate_admits_hagias_complete_session_operation_catalog() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let core = root.join("tools/config/sophia/core.kdl");
    let desktop = root.join("tools/fixtures/frame_fed_output_proof.kdl");
    let config = PersistentXtermSessionConfig::from_args(&[
        format!("--config={}", core.display()),
        format!("--desktop-profile={}", desktop.display()),
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/kitty".to_owned(),
        "--session-start=terminal".to_owned(),
        "--session-action-app=terminal=terminal".to_owned(),
        "--session-action-app=browser=terminal".to_owned(),
        "--wm-process=/usr/bin/true".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
        "--max-runtime-ms=180000".to_owned(),
    ])
    .unwrap();

    let (operations, _) = public_session_operations(&config);
    // Slots 1-4 depend on what the session was configured to launch. Slots 5
    // and 6 -- reload the profile, replace the policy client -- do not: a
    // desktop whose configuration is wrong is the one that needs them, and a
    // catalog that offered them only when things were already working would
    // withhold them exactly when they matter.
    assert_eq!(
        operations
            .iter()
            .map(|operation| operation.slot)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn session_authority_preparation_is_deterministic_and_rejection_preserves_active_state() {
    let args = [
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/kitty".to_owned(),
        "--session-app-arg=terminal=--single-instance".to_owned(),
        "--session-start=terminal".to_owned(),
        "--session-action-app=terminal=terminal".to_owned(),
        isolated_desktop_profile_argument(),
    ];
    let first = PersistentXtermSessionConfig::from_args(&args).unwrap();
    let second = PersistentXtermSessionConfig::from_args(&args).unwrap();

    assert_eq!(first.applications, second.applications);
    assert_eq!(
        first.session_application_overrides,
        second.session_application_overrides
    );
    assert_eq!(first.session_profile, second.session_profile);
    assert_eq!(
        first.session_profile.slot().participant().phase(),
        sophia_config::DesktopProfileParticipantPhase::Prepared
    );
    assert_eq!(
        first.session_profile.slot().candidate(),
        second.session_profile.slot().candidate()
    );
    assert_eq!(
        first.input_profile.slot().participant().phase(),
        sophia_config::DesktopProfileParticipantPhase::Prepared
    );
    assert_eq!(
        first.output_profile.slot().participant().phase(),
        sophia_config::DesktopProfileParticipantPhase::Prepared
    );
    for (generation, digest) in [
        (
            first.input_profile.current().generation,
            first.input_profile.current().digest,
        ),
        (
            first.output_profile.current().generation,
            first.output_profile.current().digest,
        ),
        (
            first.shortcut_profile_candidate.generation,
            first.shortcut_profile_candidate.digest,
        ),
    ] {
        assert_eq!(generation, first.desktop_profile.generation);
        assert_eq!(digest, first.desktop_profile.digest);
    }
    let active_applications = first.applications.clone();
    let active_overrides = first.session_application_overrides.clone();

    let rejected = PersistentXtermSessionConfig::from_args(&[
        isolated_core_config_argument(),
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/kitty".to_owned(),
        "--session-start=missing".to_owned(),
    ]);
    assert!(rejected.is_err());
    assert_eq!(first.applications, active_applications);
    assert_eq!(first.session_application_overrides, active_overrides);
}

#[test]
fn normal_session_rejects_proof_only_options() {
    let result = PersistentXtermSessionConfig::from_args(&[
        isolated_core_config_argument(),
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/xterm".to_owned(),
        "--session-start=terminal".to_owned(),
        "--proof".to_owned(),
    ]);
    assert!(result.is_err());
}

#[test]
fn kitty_only_session_can_exit_with_its_single_startup_app() {
    let config = isolated_session_config(&[
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/kitty".to_owned(),
        "--session-start=terminal".to_owned(),
        "--exit-when-startup-exits".to_owned(),
        isolated_desktop_profile_argument(),
    ])
    .unwrap();
    assert!(config.exit_when_startup_exits);

    for args in [
        vec!["--exit-when-startup-exits".to_owned()],
        vec![
            "--session-mode=normal".to_owned(),
            "--session-app=terminal=/usr/bin/kitty".to_owned(),
            "--session-action-app=terminal=terminal".to_owned(),
            "--exit-when-startup-exits".to_owned(),
        ],
    ] {
        assert!(isolated_session_config(&args).is_err());
    }
}

#[test]
fn startup_readiness_timeout_is_bounded_and_requires_a_startup_app() {
    let config = isolated_session_config(&[
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/kitty".to_owned(),
        "--session-start=terminal".to_owned(),
        "--startup-ready-timeout-ms=8000".to_owned(),
        isolated_desktop_profile_argument(),
    ])
    .unwrap();
    assert_eq!(
        config.startup_ready_timeout,
        Some(Duration::from_millis(8_000))
    );

    for args in [
        vec!["--startup-ready-timeout-ms=8000".to_owned()],
        vec![
            "--session-mode=normal".to_owned(),
            "--session-app=terminal=/usr/bin/kitty".to_owned(),
            "--session-action-app=terminal=terminal".to_owned(),
            "--startup-ready-timeout-ms=8000".to_owned(),
        ],
        vec![
            "--session-mode=normal".to_owned(),
            "--session-app=terminal=/usr/bin/kitty".to_owned(),
            "--session-start=terminal".to_owned(),
            "--startup-ready-timeout-ms=99".to_owned(),
        ],
    ] {
        assert!(isolated_session_config(&args).is_err());
    }
}

#[test]
fn application_admission_outlives_a_policy_response() {
    const { assert!(SESSION_APP_ADMISSION_TIMEOUT_MSEC > SESSION_POLICY_RESPONSE_TIMEOUT_MSEC) };
}

#[test]
fn policy_deadlines_follow_response_and_admission_order() {
    const { assert!(SESSION_POLICY_RESPONSE_TIMEOUT_MSEC > 3_000) };
    const { assert!(SESSION_POLICY_RESPONSE_TIMEOUT_MSEC < SESSION_APP_ADMISSION_TIMEOUT_MSEC) };
}

#[test]
fn production_input_seat_and_explicit_paths_are_distinct_modes() {
    let seat =
        isolated_session_config(&["--input-seat=seat0".to_owned(), "--max-ticks=1".to_owned()])
            .unwrap();
    assert_eq!(seat.input_seat.as_deref(), Some("seat0"));
    assert!(seat.input_devices.is_empty());

    assert!(
        isolated_session_config(&[
            "--input-seat=seat0".to_owned(),
            "--input-devices=/dev/input/event0".to_owned(),
        ])
        .is_err()
    );
    assert!(isolated_session_config(&["--input-seat=../../seat0".to_owned()]).is_err());
}

#[test]
fn live_x_output_injection_is_bounded_and_explicit() {
    let config = isolated_session_config(&[
        "--inject-output-size=1600x900".to_owned(),
        "--inject-surface-resize=960x640".to_owned(),
    ])
    .unwrap();
    assert_eq!(
        config.inject_output_size,
        Some(Size {
            width: 1600,
            height: 900
        })
    );
    assert_eq!(
        config.inject_surface_resize,
        Some(Size {
            width: 960,
            height: 640
        })
    );
    assert!(config.inject_surface_resize_sequence.is_empty());
    let sequence = isolated_session_config(&[
        "--inject-surface-resize-sequence=960x640,800x600,1024x700".to_owned(),
    ])
    .unwrap();
    assert_eq!(
        sequence.inject_surface_resize_sequence,
        vec![
            Size {
                width: 960,
                height: 640,
            },
            Size {
                width: 800,
                height: 600,
            },
            Size {
                width: 1024,
                height: 700,
            },
        ]
    );
    assert!(sequence.surface_resize_requested());
    assert_eq!(
        sequence.surface_resize_targets(),
        sequence.inject_surface_resize_sequence
    );
    assert!(
        isolated_session_config(&[
            "--inject-surface-resize=960x640".to_owned(),
            "--inject-surface-resize-sequence=800x600,960x640".to_owned(),
        ])
        .is_err()
    );
    assert!(
        isolated_session_config(&["--inject-surface-resize-sequence=800x600".to_owned(),]).is_err()
    );
    assert!(
        isolated_session_config(&["--inject-surface-resize-sequence=800x600,800x600".to_owned(),])
            .is_err()
    );
    assert!(isolated_session_config(&["--inject-output-size=0x900".to_owned(),]).is_err());
    assert!(isolated_session_config(&["--inject-output-size=wide".to_owned(),]).is_err());
}

#[test]
fn live_x_application_client_contract_is_bounded_and_exclusive() {
    let config = isolated_session_config(&[
        "--client=zenity".to_owned(),
        "--client-arg=--entry".to_owned(),
        "--expect-client-stdout=sophia\n".to_owned(),
        "--require-client-normal-exit".to_owned(),
        "--expect-physical-text=sophia".to_owned(),
        "--expect-physical-pointer".to_owned(),
        "--input-devices=/dev/input/event0,/dev/input/event1".to_owned(),
        "--max-runtime-ms=30000".to_owned(),
        "--physical-sequence-timeout-ms=600000".to_owned(),
    ])
    .unwrap();
    assert_eq!(config.client.as_deref(), Some("zenity"));
    assert_eq!(config.client_args, ["--entry"]);
    assert_eq!(config.expect_client_stdout.as_deref(), Some("sophia\n"));
    assert!(config.require_client_normal_exit);
    assert_eq!(config.physical_sequence_timeout_msec, 600_000);

    assert!(
        isolated_session_config(&["--client=zenity".to_owned(), "--terminal=xterm".to_owned(),])
            .is_err()
    );
    assert!(isolated_session_config(&["--client-arg=--entry".to_owned(),]).is_err());
    assert!(
        isolated_session_config(&[
            "--physical-sequence-timeout-ms=600000".to_owned(),
            "--max-runtime-ms=660000".to_owned(),
        ])
        .unwrap_err()
        .to_string()
        .contains("requires --expect-physical-text")
    );
    assert!(
        isolated_session_config(&[
            "--expect-physical-text=sophia".to_owned(),
            "--input-seat=seat0".to_owned(),
            "--physical-sequence-timeout-ms=600001".to_owned(),
            "--max-runtime-ms=660000".to_owned(),
        ])
        .unwrap_err()
        .to_string()
        .contains("1000 through 600000")
    );
}

#[test]
fn live_xauthority_file_is_owner_only_valid_and_removed_on_drop() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn field<'a>(record: &'a [u8], offset: &mut usize) -> &'a [u8] {
        let len = usize::from(u16::from_be_bytes([record[*offset], record[*offset + 1]]));
        *offset += 2;
        let value = &record[*offset..*offset + len];
        *offset += len;
        value
    }

    let directory = std::env::temp_dir().join(format!(
        "sophia-live-xauthority-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let (authority, cookie) = LiveXAuthorityFile::create_in(&directory, 77).unwrap();
    let path = authority.path().to_owned();
    let metadata = std::fs::metadata(&path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);

    let record = std::fs::read(&path).unwrap();
    assert_eq!(u16::from_be_bytes([record[0], record[1]]), 256);
    let mut offset = 2;
    assert_eq!(
        field(&record, &mut offset),
        rustix::system::uname().nodename().to_bytes()
    );
    assert_eq!(field(&record, &mut offset), b"77");
    assert_eq!(field(&record, &mut offset), b"MIT-MAGIC-COOKIE-1");
    assert_eq!(field(&record, &mut offset), cookie);
    assert_eq!(offset, record.len());

    drop(authority);
    assert!(!path.exists());
    std::fs::remove_dir(directory).unwrap();
}

/// A session running one application starts under the compiled default
/// profile, and says which of its shortcuts it dropped.
///
/// The compiled profile binds spawn-terminal and spawn-browser. It is the
/// fallback loaded whenever no user profile is found -- `--no-config`, and any
/// machine with no `~/.config/sophia` -- so refusing on it made every
/// single-application session unstartable from 29b9424b until a physical run
/// tripped over it, nineteen days later.
#[test]
fn a_single_application_session_drops_default_shortcuts_it_cannot_perform() {
    let config = PersistentXtermSessionConfig::from_args(&[
        "--session-mode=normal".to_owned(),
        "--no-config".to_owned(),
        "--session-app=standalone=/usr/bin/true".to_owned(),
        "--session-start=standalone".to_owned(),
        "--exit-when-startup-exits".to_owned(),
    ])
    .expect("a single-application session starts under the compiled profile");

    // Named, not silently ignored: a session where Super+Return does nothing
    // should say so, and reporting it is what makes the drop reviewable.
    let dropped = config
        .dropped_shortcuts
        .iter()
        .map(|shortcut| shortcut.profile_name())
        .collect::<Vec<_>>();
    assert!(
        dropped.contains(&"spawn-terminal") && dropped.contains(&"spawn-browser"),
        "expected the unavailable spawn shortcuts to be named: {dropped:?}"
    );
    // Nothing else was dropped. Logout and close-window need no application,
    // and dropping them would mean the session could not be left.
    assert!(
        !dropped.contains(&"logout") && !dropped.contains(&"close-window"),
        "dropped a shortcut that needs no application: {dropped:?}"
    );
}

/// An author who wrote an unsatisfiable binding still hears about it.
///
/// This is the half of the old behaviour worth keeping: a profile someone
/// wrote by hand states intent, and a binding in it that can do nothing is a
/// mistake rather than a default that does not apply.
#[test]
fn an_explicit_profile_still_refuses_a_shortcut_the_session_cannot_perform() {
    let profile = std::env::temp_dir().join(format!(
        "sophia-explicit-shortcut-{}-{}.kdl",
        std::process::id(),
        line!()
    ));
    std::fs::write(
        &profile,
        concat!(
            "schema 1\n",
            "shell { enabled #false; }\n",
            "shortcut {\n",
            "  profile \"explicit\"\n",
            "  bind \"Super+Return\" \"session:spawn-terminal\"\n",
            "}\n",
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let refused = PersistentXtermSessionConfig::from_args(&[
        isolated_core_config_argument(),
        "--session-mode=normal".to_owned(),
        format!("--desktop-profile={}", profile.display()),
        "--session-app=standalone=/usr/bin/true".to_owned(),
        "--session-start=standalone".to_owned(),
        "--exit-when-startup-exits".to_owned(),
    ]);

    assert!(
        refused.is_err(),
        "an explicit profile naming an unavailable capability was accepted"
    );
    std::fs::remove_file(&profile).unwrap();
}

/// An explicit profile that enables a shell still refuses without one.
///
/// The compiled default's shell is turned off for a session that cannot run
/// it, because the default describes a desktop rather than this session. A
/// profile someone wrote is different: silently dropping its shell would take
/// the indicator strip and the switcher out of a Hagia session without saying
/// so, and the gate asserting they are visible would fail somewhere else
/// entirely.
#[test]
fn an_explicit_profile_enabling_a_shell_still_refuses_without_one() {
    let profile = std::env::temp_dir().join(format!(
        "sophia-explicit-shell-{}-{}.kdl",
        std::process::id(),
        line!()
    ));
    std::fs::write(
        &profile,
        concat!(
            "schema 1\n",
            "shell { enabled #true; }\n",
            "shortcut {\n",
            "  profile \"explicit-shell\"\n",
            "  bind \"Ctrl+Alt+Delete\" \"session:logout\"\n",
            "}\n",
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let refused = PersistentXtermSessionConfig::from_args(&[
        isolated_core_config_argument(),
        "--session-mode=normal".to_owned(),
        format!("--desktop-profile={}", profile.display()),
        "--session-app=standalone=/usr/bin/true".to_owned(),
        "--session-start=standalone".to_owned(),
        "--exit-when-startup-exits".to_owned(),
    ]);

    assert!(
        refused.is_err(),
        "an explicit profile enabling a shell started without one"
    );
    std::fs::remove_file(&profile).unwrap();
}

/// The argument vector the direct-scanout probe builds, with the fixtures it
/// ships rather than copies of them.
///
/// Three physical runs have now died in argument validation on things nothing
/// described. The last one dropped `--no-config` for a core configuration and
/// so began discovering the operator's own desktop profile, which enables a
/// shell and binds spawn-terminal -- neither of which a one-application proof
/// can provide. Both configurations are explicit now, and both are read from
/// the files the runner installs.
///
/// `--native-scanout` is deliberately absent: it is gated on an environment
/// variable the runner exports, and setting one from a test races every other
/// test in the process.
#[test]
fn the_standalone_single_application_argument_set_still_starts() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/fixtures");
    let staged = |name: &str| {
        let path = std::env::temp_dir().join(format!(
            "sophia-probe-{name}-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::copy(fixtures.join(name), &path).unwrap();
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    };
    let core = staged("direct_scanout_core.kdl");
    let desktop = staged("direct_scanout_desktop.kdl");

    let accepted = PersistentXtermSessionConfig::from_args(&[
        "--session-mode=normal".to_owned(),
        "--display=:77".to_owned(),
        "--startup-ready-timeout-ms=8000".to_owned(),
        format!("--config={}", core.display()),
        format!("--desktop-profile={}", desktop.display()),
        "--session-app=standalone=/usr/bin/true".to_owned(),
        "--session-start=standalone".to_owned(),
        "--exit-when-startup-exits".to_owned(),
        // The profile runs bounded, which is also what keeps its records
        // readable: `sophia` diverts an *ordinary* session's records to the
        // reduced per-session evidence log, where a cadence summary loses the
        // `mean_fps` and `p95_frame_msec` the benchmark report exists to read.
        // A bounded session keeps its full records on stdout, which is where
        // this profile's report looks.
        "--max-runtime-ms=55000".to_owned(),
        "--session-app-arg=standalone=--config".to_owned(),
        "--session-app-arg=standalone=NONE".to_owned(),
        "--session-app-arg=standalone=--override".to_owned(),
        "--session-app-arg=standalone=linux_display_server=x11".to_owned(),
        "--session-app-arg=standalone=--override".to_owned(),
        "--session-app-arg=standalone=background_opacity=1".to_owned(),
        "--session-app-arg=standalone=--override".to_owned(),
        "--session-app-arg=standalone=remember_window_size=no".to_owned(),
        "--session-app-arg=standalone=--override".to_owned(),
        "--session-app-arg=standalone=initial_window_width=2560".to_owned(),
        "--session-app-arg=standalone=--override".to_owned(),
        "--session-app-arg=standalone=initial_window_height=1440".to_owned(),
        "--session-app-arg=standalone=--override".to_owned(),
        "--session-app-arg=standalone=confirm_os_window_close=0".to_owned(),
        "--session-app-arg=standalone=sh".to_owned(),
        "--session-app-arg=standalone=-c".to_owned(),
        "--session-app-arg=standalone=sleep 20".to_owned(),
    ]);

    assert!(
        accepted
            .as_ref()
            .is_ok_and(|config| config.max_runtime.is_some()),
        "the standalone argument set must stay bounded so its records reach the report: {:?}",
        accepted.as_ref().err()
    );
    assert!(
        accepted.is_ok(),
        "the standalone argument set was refused: {:?}",
        accepted.err()
    );
    std::fs::remove_file(&core).unwrap();
    std::fs::remove_file(&desktop).unwrap();
}

/// Arguments a cursor-flag test can use.
///
/// `--native-scanout` is deliberately absent, for the reason the standalone
/// test above gives: it is gated on an environment variable, and setting one
/// from a test races every other test in the process. The cursor default is
/// parsed the same either way -- what native scanout decides is whether the
/// preference is ever consulted, since only a native session calls
/// `use_atomic_cursor_plane`.
fn cursor_flag_arguments() -> Vec<String> {
    vec![
        "--session-mode=normal".to_owned(),
        "--display=:77".to_owned(),
        "--no-config".to_owned(),
        "--session-app=probe=/usr/bin/true".to_owned(),
        "--session-start=probe".to_owned(),
        "--exit-when-startup-exits".to_owned(),
    ]
}

/// The cursor prefers an atomic plane without being asked.
///
/// A preference, not a guarantee: the startup probe decides per card, and
/// one that refuses keeps the legacy ioctl. What changed is what a session
/// asks for when nobody says otherwise.
#[test]
fn the_cursor_prefers_an_atomic_plane_by_default() {
    let config = PersistentXtermSessionConfig::from_args(&cursor_flag_arguments()).unwrap();
    assert!(config.atomic_cursor);
}

/// Asking for both paths is a contradiction, not a precedence puzzle.
#[test]
fn the_two_cursor_flags_are_mutually_exclusive() {
    let mut arguments = cursor_flag_arguments();
    arguments.extend(["--atomic-cursor".to_owned(), "--legacy-cursor".to_owned()]);
    let error = PersistentXtermSessionConfig::from_args(&arguments)
        .expect_err("a session cannot want both cursor paths");
    assert!(error.to_string().contains("mutually exclusive"), "{error}");
}

/// Either flag names a hardware path a session without native scanout does
/// not have. The default simply does not apply there; naming one explicitly
/// is what gets refused.
///
/// The remaining case -- that `--legacy-cursor` sets the preference false --
/// is not asserted anywhere in this process. It cannot be: observing the
/// field needs a config that parsed, which needs `--native-scanout`, which is
/// gated on an environment variable a test may not set without racing every
/// other test here. An earlier version of this comment claimed the release
/// binary checked it under `--validate-session-args`; that validates the
/// argument vector and reads no field, so the claim was false and the case
/// was uncovered.
///
/// It is covered by evidence instead. A session records
/// `sophia_live_cursor_path schema=1 status=selected path=...` at readiness,
/// and `tools/verify_hagia_native_session.sh` requires that line, so a run
/// that took the path it was not asked for is refused by the gate rather than
/// by a unit test.
#[test]
fn the_cursor_flags_need_native_scanout() {
    for flag in ["--atomic-cursor", "--legacy-cursor"] {
        let mut arguments = cursor_flag_arguments();
        arguments.push(flag.to_owned());
        let error = PersistentXtermSessionConfig::from_args(&arguments)
            .expect_err("a session without native scanout has no cursor plane to choose");
        assert!(
            error.to_string().contains("--native-scanout"),
            "{flag}: {error}"
        );
    }
}

#[test]
fn the_font_path_defaults_to_the_host_directories_and_can_be_emptied() {
    // A live session should find the host's core fonts without being told
    // where they are, which is what lets a terminal in UTF-8 mode render real
    // Unicode rather than the single built-in face. A proof that must not
    // depend on installed packages asks for none.
    let arguments = |extra: Option<&str>| {
        let mut args = vec![
            isolated_core_config_argument(),
            isolated_desktop_profile_argument(),
            "--session-mode=normal".to_owned(),
            "--session-app=mirror=/usr/bin/kitty".to_owned(),
            "--session-start=mirror".to_owned(),
            "--session-action-app=terminal=mirror".to_owned(),
            "--session-app=proof=/usr/bin/kitty".to_owned(),
            "--session-start=proof".to_owned(),
            "--session-action-app=browser=proof".to_owned(),
            "--wm-process=/usr/bin/true".to_owned(),
            "--wm-interface=sophia_wm_v1".to_owned(),
            "--max-runtime-ms=30000".to_owned(),
        ];
        if let Some(extra) = extra {
            args.push(extra.to_owned());
        }
        args
    };

    let config =
        PersistentXtermSessionConfig::from_args(&arguments(None)).expect("a default session");
    assert_eq!(
        config.font_path,
        sophia_x_authority::XFontCatalog::default_path(),
        "the default is the standard directories that exist on this host"
    );

    let configured =
        PersistentXtermSessionConfig::from_args(&arguments(Some("--font-path=/one/dir:/two/dir")))
            .expect("an explicit path");
    assert_eq!(
        configured.font_path,
        vec![
            std::path::PathBuf::from("/one/dir"),
            std::path::PathBuf::from("/two/dir"),
        ]
    );

    let empty = PersistentXtermSessionConfig::from_args(&arguments(Some("--font-path=")))
        .expect("an empty path");
    assert!(
        empty.font_path.is_empty(),
        "an empty path leaves only the built-in face, which is deterministic"
    );
}

#[test]
fn admit_xtest_is_a_dev_flag_that_never_stands_beside_a_proof() {
    let config = isolated_session_config(&["--admit-xtest".to_owned()]).unwrap();
    assert!(config.admit_xtest);
    assert!(
        !isolated_session_config(&[]).unwrap().admit_xtest,
        "off by default"
    );

    // A synthetic source could satisfy any of these. Rehearsal is not
    // acceptance, and the flag says so by refusing to stand beside them.
    for proof in [
        vec!["--expect-physical-pointer".to_owned()],
        vec![
            "--inject-text=sophia".to_owned(),
            "--max-ticks=10".to_owned(),
        ],
        vec![
            "--expect-physical-text=sophia".to_owned(),
            "--max-ticks=10".to_owned(),
        ],
    ] {
        let mut args = vec!["--admit-xtest".to_owned()];
        args.extend(proof.iter().cloned());
        let error = isolated_session_config(&args).unwrap_err().to_string();
        assert!(error.contains("--admit-xtest"), "{proof:?}: {error}");
    }
}
