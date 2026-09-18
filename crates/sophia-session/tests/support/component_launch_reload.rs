use super::*;

fn profile(provider: &str, terminal: &str, shell_bindings: bool) -> String {
    let bindings = if shell_bindings {
        "bind Super+Space session:application-launcher; bind Super+p session:window-switcher;"
    } else {
        ""
    };
    format!(
        r#"schema 1
shell {{ enabled #true; panel 24; content #true; content-input #true; }}
session {{
    {provider}
    application "terminal" {{ exec "{terminal}"; }}
    terminal "terminal"
    application-catalog "installed"
    startup
}}
shortcut {{ profile "reload"; bind Super+Return session:spawn-terminal; {bindings} }}
"#
    )
}

const COMPONENTS: &str = r#"
shell-component "panel" "bar" { executable "/absent/lom"; reservation "top" 24; }
shell-component "menu" "application-launcher" { executable "/absent/bemenu"; }
"#;

fn fixture(provider: &str) -> ReloadFixture {
    let mut source = ConfigFixture::new(&[]);
    let core = source.directory.join("core.kdl");
    let desktop = source.directory.join("desktop.kdl");
    std::fs::write(&core, "schema 2\nsession { application-catalog \"installed\" launch-policy=\"trusted-host\" {} }\n").unwrap();
    std::fs::write(&desktop, profile(provider, "/usr/bin/xterm", true)).unwrap();
    source.config = PersistentXtermSessionConfig::from_args(&[
        format!("--config={}", core.display()),
        format!("--desktop-profile={}", desktop.display()),
        "--session-mode=normal".into(),
        "--no-input".into(),
        "--wm-process=/absent/hagia".into(),
    ])
    .unwrap();
    ReloadFixture::from_config_with_actions(
        source,
        vec![sophia_protocol::PolicyActionRegistration {
            action: WmActionId::from_raw(7),
            name: "application-launcher".into(),
            session_operation_slot: Some(7),
        }],
    )
}

#[test]
fn component_terminal_reload_updates_real_shortcut_without_restarting_providers() {
    check_terminal_reload(COMPONENTS);
}

#[test]
fn narthex_terminal_reload_uses_the_same_capability() {
    check_terminal_reload("shell-client \"/absent/narthex\"");
}

fn check_terminal_reload(provider: &str) {
    let mut fixture = fixture(provider);
    let config = &fixture.source.config;
    assert_eq!(config.shell_process.is_none(), provider == COMPONENTS);
    let original_slot = config.session_profile.slot().clone();
    let original_spec = fixture.wm.supervisor.launch_spec().clone();
    let old_action = fixture.wm.command_registry.roles[&TERMINAL_APPLICATION_ID];
    let old_command = fixture.wm.command_registry.command(old_action).unwrap();
    assert_eq!(old_command.executable, Path::new("/usr/bin/xterm"));
    std::fs::write(
        config.desktop_profile_source.as_ref().unwrap(),
        profile(provider, "/usr/bin/kitty", true),
    )
    .unwrap();
    assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Applied);
    let router = fixture.wm.shortcuts.as_mut().unwrap();
    let seat = SeatId::from_raw(1);
    assert!(router.route_key(seat, 125, true).action.is_none());
    let action = router.route_key(seat, 28, true).action.unwrap();
    let command = fixture.wm.command_registry.command(action).unwrap();
    assert_eq!(command.executable, Path::new("/usr/bin/kitty"));
    assert_ne!(action, old_action);
    assert!(fixture.wm.command_registry.command(old_action).is_none());
    assert_eq!(old_command.executable, Path::new("/usr/bin/xterm"));
    assert_eq!(fixture.source.config.session_profile.slot(), &original_slot);
    assert_eq!(fixture.wm.supervisor.launch_spec(), &original_spec);
    assert!(!fixture.wm.desktop_reload_pending());
    assert!(fixture.source.config.applications.startup.is_empty());
}

#[test]
fn requested_components_do_not_authorize_shortcuts_without_active_provider() {
    let mut fixture = ReloadFixture::new();
    let old_applications = fixture.source.config.applications.clone();
    let old_generation = fixture.wm.command_registry.generation;
    std::fs::write(
        fixture
            .source
            .config
            .desktop_profile_source
            .as_ref()
            .unwrap(),
        // The switcher needs a shell, but no application catalog: otherwise
        // absence of the startup catalog could mask an incorrectly allowed provider.
        profile(COMPONENTS, "/usr/bin/kitty", true)
            .replace("bind Super+Space session:application-launcher;", ""),
    )
    .unwrap();
    let prepared = sophia_config::load_prepared_desktop_profile(
        fixture.source.config.desktop_profile_source.as_deref(),
        sophia_config::ConfigGeneration::from_raw(2),
    )
    .unwrap();
    let refusal = PreparedDesktopLaunch::prepare(&fixture.source.config, prepared, 2)
        .err()
        .expect("an unstarted provider cannot authorize shell shortcuts");
    assert_eq!(
        refusal.to_string(),
        "desktop shortcut references an unavailable session capability"
    );
    assert_eq!(fixture.reload(), DesktopProfileReloadOutcome::Declined);
    assert_eq!(fixture.source.config.applications, old_applications);
    assert_eq!(fixture.wm.command_registry.generation, old_generation);
}
