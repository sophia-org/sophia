#![cfg(test)]

use super::*;
use std::sync::Arc;

fn profile(fixture: &ConfigFixture, source: &str) -> sophia_config::PreparedDesktopProfile {
    let path = fixture.directory.join("commands.kdl");
    std::fs::write(&path, source).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    sophia_config::load_prepared_desktop_profile(
        Some(&path),
        sophia_config::ConfigGeneration::from_raw(9),
    )
    .unwrap()
}

fn overrides(args: &[&str]) -> SessionApplicationOverrides {
    SessionApplicationOverrides::parse(
        &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
    )
    .unwrap()
}

#[test]
fn desktop_commands_replace_whole_specs_and_core_references_are_explicit() {
    let fixture = ConfigFixture::new(&[]);
    let prepared = profile(
        &fixture,
        r#"schema 1
session {
    application "brave-origin" { exec "brave-origin" "literal; no shell"; }
    application "advanced" { use-core "brave-origin"; }
}
shortcut {
    profile "desktop"
    bind "Super+b" { launch "brave-origin"; }
    bind "Super+a" { launch "advanced"; }
}
"#,
    );
    let mut core = PersistentXtermSessionConfig::applications_from_core(
        fixture.config.core_config_state.active(),
    )
    .unwrap();
    core.applications
        .get_mut("brave-origin")
        .unwrap()
        .placement_classification = Some(71);
    let resolved = overrides(&[])
        .prepare(core.clone(), &prepared.candidates.session)
        .unwrap();
    let direct = resolved.named_command("brave-origin").unwrap();
    assert_eq!(direct.executable, Path::new("brave-origin"));
    assert_eq!(direct.arguments, ["literal; no shell"]);
    assert_eq!(direct.placement_classification, None);
    let explicit = resolved.named_command("advanced").unwrap();
    assert_eq!(explicit.id, "advanced");
    assert_eq!(explicit.executable, Path::new("/usr/bin/sophia"));
    assert_eq!(
        explicit.arguments,
        ["client-launch", "--", "/usr/bin/brave-origin"]
    );
    assert_eq!(explicit.placement_classification, Some(71));
    assert!(resolved.applications.contains_key("panel"));
    assert!(resolved.named_command("panel").is_none());
    resolved
        .validate_shortcuts(&prepared.candidates.shortcut, false, false)
        .unwrap();
    let mut implicit = prepared.candidates.shortcut.clone();
    implicit.bindings[0].target =
        sophia_config::DesktopShortcutTarget::LaunchApplication("panel".to_owned());
    assert!(
        resolved
            .validate_shortcuts(&implicit, false, false)
            .is_err()
    );
    let mut missing = prepared.candidates.session.clone();
    missing.applications[1].command =
        sophia_config::DesktopApplicationCommand::UseCore("absent".to_owned());
    assert!(overrides(&[]).prepare(core, &missing).is_err());
}

#[test]
fn defaults_yield_to_complete_profile_and_cli_commands_without_argument_leakage() {
    let fixture = ConfigFixture::new(&[]);
    let prepared = profile(
        &fixture,
        r#"schema 1
session {
    application "terminal" { exec "/usr/bin/profile-terminal" "--profile"; }
    terminal "terminal"
    browser "brave-origin"
}
"#,
    );
    let core = PersistentXtermSessionConfig::applications_from_core(
        fixture.config.core_config_state.active(),
    )
    .unwrap();
    let defaults = [
        "--session-app-default=terminal=/usr/bin/default-terminal",
        "--session-app-default=brave-origin=/usr/bin/default-browser",
        "--session-app-default=extra=/usr/bin/default-extra",
        "--session-action-default=terminal=extra",
        "--session-action-default=browser=extra",
    ];
    let resolved = overrides(&defaults)
        .prepare(core.clone(), &prepared.candidates.session)
        .unwrap();
    assert_eq!(resolved.terminal.as_deref(), Some("terminal"));
    assert_eq!(resolved.browser.as_deref(), Some("brave-origin"));
    assert_eq!(
        resolved.named_command("terminal").unwrap().arguments,
        ["--profile"]
    );
    assert_eq!(
        resolved.named_command("terminal").unwrap().executable,
        Path::new("/usr/bin/profile-terminal")
    );
    assert_eq!(
        resolved.applications["brave-origin"].executable,
        Path::new("/usr/bin/sophia")
    );
    assert_eq!(
        resolved.applications["extra"].executable,
        Path::new("/usr/bin/default-extra")
    );
    let mut cli = defaults.to_vec();
    cli.extend([
        "--session-app=terminal=/usr/bin/cli-terminal",
        "--session-app-arg=terminal=--cli",
        "--session-app=brave-origin=/usr/bin/cli-browser",
    ]);
    let overridden = overrides(&cli)
        .prepare(core, &prepared.candidates.session)
        .unwrap();
    assert_eq!(
        overridden.named_command("terminal").unwrap().executable,
        Path::new("/usr/bin/cli-terminal")
    );
    assert_eq!(
        overridden.named_command("terminal").unwrap().arguments,
        ["--cli"]
    );
    assert!(overridden.applications["brave-origin"].arguments.is_empty());
    assert_eq!(
        overridden.applications["brave-origin"].placement_classification,
        None
    );
    let mut without_profile_roles = prepared.candidates.session.clone();
    without_profile_roles.terminal = None;
    without_profile_roles.browser = None;
    without_profile_roles.applications.clear();
    let fallback = overrides(&defaults)
        .prepare(SessionApplicationConfig::default(), &without_profile_roles)
        .unwrap();
    assert_eq!(fallback.terminal.as_deref(), Some("extra"));
    assert_eq!(fallback.browser.as_deref(), Some("extra"));
    assert!(fallback.applications["terminal"].arguments.is_empty());
}

#[test]
fn queued_registry_command_survives_reload_as_one_complete_spec() {
    let fixture = ConfigFixture::new(&[]);
    let prepared = profile(
        &fixture,
        r#"schema 1
session { application "launch" { exec "/usr/bin/original" "--old"; } }
"#,
    );
    let core = PersistentXtermSessionConfig::applications_from_core(
        fixture.config.core_config_state.active(),
    )
    .unwrap();
    let original = overrides(&[])
        .prepare(core.clone(), &prepared.candidates.session)
        .unwrap();
    let mut queue = crate::session_actions::SessionLaunchQueue::default();
    let transaction = sophia_protocol::TransactionId::from_raw(201);
    queue.enqueue_command(
        crate::session_actions::SessionLaunchIntent {
            transaction,
            application: sophia_protocol::SessionApplicationId::from_raw(20),
            placement_classification: None,
        },
        Arc::new(original.named_command("launch").unwrap().clone()),
        0,
    );
    let mut replacement = prepared.candidates.session.clone();
    replacement.applications[0].command = sophia_config::DesktopApplicationCommand::Exec {
        executable: "/usr/bin/replacement".into(),
        arguments: vec!["--new".to_owned()],
    };
    let replacement = overrides(&[]).prepare(core, &replacement).unwrap();
    assert_eq!(
        replacement.named_command("launch").unwrap().arguments,
        ["--new"]
    );
    drop(original);
    queue.begin_next(true).unwrap();
    let retained = queue.take_admitted_command(transaction).unwrap();
    assert_eq!(retained.executable, Path::new("/usr/bin/original"));
    assert_eq!(retained.arguments, ["--old"]);
    assert!(queue.complete_spawn(transaction).is_some());
}

#[test]
fn core_reload_cli_override_remains_complete_and_does_not_duplicate_arguments() {
    let mut fixture = ConfigFixture::new(&[]);
    let core = DIRECT_CORE.replace(
        "    startup 4",
        "    application \"terminal\" id=6 executable=\"/usr/bin/core-terminal\" { arg \"--core\"; }\n    startup 4",
    );
    fixture.config.reload_core_config(core.as_bytes()).unwrap();
    assert_launches(&fixture.config, "brave-origin");
    fixture
        .config
        .reload_core_config(format!("{core}diagnostics verbose=#true\n").as_bytes())
        .unwrap();
    assert_launches(&fixture.config, "brave-origin");
}

#[test]
fn live_command_resolution_does_not_replay_retired_startup_references() {
    let mut fixture = ConfigFixture::new(&[]);
    let original = profile(
        &fixture,
        "schema 1\nsession { application \"old\" { exec \"/usr/bin/true\"; }; startup \"old\"; }\n",
    );
    let core = PersistentXtermSessionConfig::applications_from_core(
        fixture.config.core_config_state.active(),
    )
    .unwrap();
    let original_apps = overrides(&[])
        .prepare(core.clone(), &original.candidates.session)
        .unwrap();
    assert_eq!(original_apps.startup, ["old"]);
    let mut changed = original.candidates.session.clone();
    changed.applications[0].name = "new".to_owned();
    // The already-executed startup selection is not part of a live launch update.
    assert!(overrides(&[]).prepare(core.clone(), &changed).is_err());
    let updated = overrides(&[])
        .prepare_live_reload(core.clone(), &changed, &original_apps.startup)
        .unwrap();
    assert_eq!(updated.startup, ["old"]);
    assert!(updated.named_command("old").is_none());
    assert!(updated.named_command("new").is_some());

    for options in [
        overrides(&["--session-start=old"]),
        overrides(&["--session-start-default=old"]),
    ] {
        let mut without_startup = changed.clone();
        without_startup.startup = None;
        let updated = options
            .prepare_live_reload(core.clone(), &without_startup, &original_apps.startup)
            .unwrap();
        assert_eq!(updated.startup, ["old"]);
        assert!(!updated.applications.contains_key("old"));
    }

    fixture.config.applications = updated;
    fixture.config.active_launch_profile = Some(changed.clone());
    fixture
        .config
        .reload_core_config(DIRECT_CORE.as_bytes())
        .unwrap();
    assert_eq!(fixture.config.applications.startup, ["old"]);
    assert!(fixture.config.applications.named_command("old").is_none());
    assert!(fixture.config.applications.named_command("new").is_some());

    changed.applications[0].command =
        sophia_config::DesktopApplicationCommand::UseCore("missing".to_owned());
    assert!(
        overrides(&[])
            .prepare_live_reload(core, &changed, &original_apps.startup)
            .is_err()
    );
}

#[test]
fn retained_command_spawns_literal_arguments_and_authorized_environment_without_a_window() {
    use std::os::unix::ffi::OsStrExt;
    use std::time::{Duration, Instant};

    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let fixture = ConfigFixture::new(&[]);
    let script = fixture.directory.join("capture.sh");
    std::fs::write(
        &script,
        r#"output=$1
shift
{
    printf '%s\000' "$@"
    printf '%s\000' "$DISPLAY" "$XAUTHORITY" "${SOPHIA_CONTROL_SOCKET-unset}" "${SOPHIA_WM_INSPECT_SOCKET-unset}" "${ENV+set}" "${BASH_ENV+set}"
    printf '%s\000' "$(pwd -P)"
} > "$output"
"#,
    )
    .unwrap();
    let prepared = profile(
        &fixture,
        "schema 1\nsession { application \"capture\" { exec \"/bin/sh\"; } }\n",
    );
    let core = PersistentXtermSessionConfig::applications_from_core(
        fixture.config.core_config_state.active(),
    )
    .unwrap();
    let original = overrides(&[])
        .prepare(core.clone(), &prepared.candidates.session)
        .unwrap();
    let xauthority = fixture.directory.join("authority with spaces");
    let control = fixture.directory.join("control.sock");
    let inspection = fixture.directory.join("inspection.sock");
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    let literals = [
        "$HOME",
        "'single' and \"double\"",
        "two words",
        "",
        "$(exit 9); *",
    ];

    for (index, (socket, inspection_socket)) in [
        (Some(control.as_path()), None),
        (None, Some(inspection.as_path())),
        (Some(control.as_path()), Some(inspection.as_path())),
        (None, None),
    ]
    .into_iter()
    .enumerate()
    {
        let output = fixture.directory.join(format!("capture-{index}"));
        let mut command = original.named_command("capture").unwrap().clone();
        command.arguments = vec![
            script.to_str().unwrap().to_owned(),
            output.to_str().unwrap().to_owned(),
        ];
        command.arguments.extend(literals.map(str::to_owned));
        let transaction = sophia_protocol::TransactionId::from_raw(300 + index as u64);
        let mut queue = crate::session_actions::SessionLaunchQueue::default();
        assert!(matches!(
            queue.enqueue_command(
                crate::session_actions::SessionLaunchIntent {
                    transaction,
                    application: sophia_protocol::SessionApplicationId::from_raw(20),
                    placement_classification: None,
                },
                Arc::new(command),
                0,
            ),
            crate::session_actions::SessionLaunchQueueOutcome::Queued { depth: 1 }
        ));
        let mut replacement = prepared.candidates.session.clone();
        replacement.applications[0].command = sophia_config::DesktopApplicationCommand::Exec {
            executable: fixture.directory.join("not-an-executable"),
            arguments: vec!["replacement".to_owned()],
        };
        let replacement = overrides(&[]).prepare(core.clone(), &replacement).unwrap();
        queue.begin_next(true).unwrap();
        let retained = queue.take_admitted_command(transaction).unwrap();
        let mut child = ChildGuard(
            PersistentXtermSessionConfig::spawn_session_application(
                &retained,
                ":private-no-connection",
                &xauthority,
                socket,
                inspection_socket,
                crate::diagnostics::application::LaunchContext {
                    source: crate::diagnostics::application::LaunchSource::Shortcut,
                    transaction: Some(transaction.raw()),
                },
            )
            .unwrap(),
        );
        assert!(queue.complete_spawn(transaction).is_some());
        assert!(queue.admission().is_none());
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "capture child did not exit");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(status.success());
        let mut expected = Vec::new();
        for field in literals.iter().map(|value| value.as_bytes()).chain([
            b":private-no-connection".as_slice(),
            xauthority.as_os_str().as_bytes(),
            socket.map_or(b"unset".as_slice(), |path| path.as_os_str().as_bytes()),
            inspection_socket.map_or(b"unset".as_slice(), |path| path.as_os_str().as_bytes()),
            b"".as_slice(),
            b"".as_slice(),
            cwd.as_os_str().as_bytes(),
        ]) {
            expected.extend_from_slice(field);
            expected.push(0);
        }
        assert_eq!(std::fs::read(output).unwrap(), expected);
        assert!(
            PersistentXtermSessionConfig::spawn_session_application(
                replacement.named_command("capture").unwrap(),
                ":private-no-connection",
                &xauthority,
                socket,
                inspection_socket,
                crate::diagnostics::application::LaunchContext {
                    source: crate::diagnostics::application::LaunchSource::Shortcut,
                    transaction: Some(transaction.raw()),
                },
            )
            .is_err()
        );
        assert!(queue.admission().is_none());
    }
}
