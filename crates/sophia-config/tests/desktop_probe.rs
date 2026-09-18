#[path = "../examples/desktop_profile_probe.rs"]
mod probe;

use sophia_config::{ConfigGeneration, DesktopAuthority, load_prepared_desktop_profile};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn probe_preserves_real_wm_bindings_includes_and_commands_without_autostart() {
    let root = std::env::temp_dir().join(format!("sophia-desktop-probe-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let base = root.join("wm.kdl");
    let overrides = root.join("probe.kdl");
    let output = root.join("prepared.kdl");
    write(
        &root.join("keys.kdl"),
        r#"shortcut {
        profile "operator"
        bind "Super+4" "policy:focus-workspace 7"
        bind "Super+Return" { exec "terminal" "literal argument"; }
        bind "Super+b" { launch "browser"; }
    }
    "#,
    );
    write(
        &base,
        r#"schema 1
        include "keys.kdl"
        policy { layout "tile"; view-count 9; }
        shell { enabled #true; panel 30; }
        session { application "browser" { exec "browser"; }; startup "browser"; }
    "#,
    );
    write(
        &overrides,
        include_str!("../../../tools/fixtures/lom_panel_desktop.kdl"),
    );
    let original = load_prepared_desktop_profile(Some(&base), ConfigGeneration::INITIAL).unwrap();
    write(&output, &probe::compose(&base, &overrides).unwrap());
    let derived = load_prepared_desktop_profile(Some(&output), ConfigGeneration::INITIAL).unwrap();
    assert_eq!(
        original.candidates.shortcut.bindings,
        derived.candidates.shortcut.bindings
    );
    assert_eq!(
        original.candidates.shortcut.profile,
        derived.candidates.shortcut.profile
    );
    let commands = |p: &sophia_config::PreparedDesktopProfile| {
        p.candidates
            .session
            .applications
            .iter()
            .map(|a| (a.name.clone(), a.command.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(commands(&original), commands(&derived));
    assert!(
        derived
            .candidates
            .session
            .applications
            .iter()
            .all(|a| a.provenance.path == output)
    );
    assert_eq!(derived.candidates.session.startup, Some(Vec::new()));
    let policy = |p: &sophia_config::PreparedDesktopProfile| {
        p.profile.candidates[&DesktopAuthority::Policy]
            .values
            .iter()
            .map(|v| {
                let mut document = kdl::KdlDocument::parse_v2(&v.encoded).unwrap();
                document.autoformat();
                document.to_string()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(policy(&original), policy(&derived));
    assert!(sophia_config::desktop_profile_shell_content_enabled(
        &derived.profile
    ));
    for forbidden in [
        "shortcut { profile \"replacement\"; bind \"Super+1\" \"policy:focus-workspace 1\"; }",
        "policy { layout \"grid\"; }",
    ] {
        write(&overrides, &format!("schema 1\n{forbidden}\n"));
        assert!(
            probe::compose(&base, &overrides)
                .unwrap_err()
                .to_string()
                .contains("must not replace")
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_probe_requires_existing_wm_launcher_key_without_inventing_one() {
    let root = std::env::temp_dir().join(format!("sophia-launcher-binding-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("wm.kdl");
    write(&path, "schema 1\nshortcut { profile \"operator\"; }\n");
    assert!(probe::require_launcher_binding(&path).is_err());
    write(
        &path,
        "schema 1\nshortcut { profile \"operator\"; bind \"Super+Space\" \"session:application-launcher\"; }\n",
    );
    probe::require_launcher_binding(&path).unwrap();
    let before = fs::read(&path).unwrap();
    probe::require_launcher_binding(&path).unwrap();
    assert_eq!(before, fs::read(&path).unwrap());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_components_replace_only_the_inherited_provider_selection() {
    let root = std::env::temp_dir().join(format!("sophia-component-probe-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let base = root.join("wm.kdl");
    let overrides = root.join("probe.kdl");
    let output = root.join("prepared.kdl");
    write(
        &overrides,
        r#"schema 1
        shell { enabled #true; content #true; content-input #true; panel 24; gpu "denied"; }
        session {
            shell-component "panel" "bar" { executable "/new/lom"; config "/new/lom.kdl"; gpu "direct"; }
            shell-component "menu" "application-launcher" { executable "/new/bemenu"; }
            startup
        }
    "#,
    );
    for previous in [
        r#"shell-client "/old/narthex"; shell-config "/private/narthex.kdl";"#,
        r#"shell-component "old-bar" "bar" { executable "/old/bar"; }; shell-component "old-menu" "application-launcher" { executable "/old/menu"; };"#,
    ] {
        write(
            &base,
            &format!(
                r#"schema 1
            policy {{ layout "scroller"; view-count 6; }}
            shortcut {{ profile "operator"; bind "Super+Space" "session:application-launcher"; }}
            shell {{ enabled #true; panel 32; }}
            session {{ {previous} window-manager "/old/hagia"; application "browser" {{ exec "/usr/bin/browser"; }}; startup "browser"; }}
        "#
            ),
        );
        let before = fs::read(&base).unwrap();
        let original =
            load_prepared_desktop_profile(Some(&base), ConfigGeneration::INITIAL).unwrap();
        write(&output, &probe::compose(&base, &overrides).unwrap());
        let derived =
            load_prepared_desktop_profile(Some(&output), ConfigGeneration::INITIAL).unwrap();
        assert_eq!(before, fs::read(&base).unwrap());
        assert_eq!(
            original.candidates.shortcut.bindings,
            derived.candidates.shortcut.bindings
        );
        assert_eq!(
            original.candidates.session.components.window_manager,
            derived.candidates.session.components.window_manager
        );
        assert_eq!(
            original.candidates.session.applications[0].command,
            derived.candidates.session.applications[0].command
        );
        let components = derived.candidates.session.components;
        assert!(components.shell_client.is_none() && components.shell_config.is_none());
        assert_eq!(
            components
                .shell_components
                .iter()
                .map(|v| v.id.as_str())
                .collect::<Vec<_>>(),
            ["panel", "menu"]
        );
        assert_eq!(
            components.shell_components[0].config.as_deref(),
            Some(Path::new("/new/lom.kdl"))
        );
        assert_eq!(derived.candidates.session.startup, Some(Vec::new()));
        assert!(
            !fs::read_to_string(&output)
                .unwrap()
                .contains("/private/narthex")
        );
    }
    fs::remove_dir_all(root).unwrap();
}
