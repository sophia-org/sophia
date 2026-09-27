use super::PersistentXtermSessionConfig;
use std::os::unix::fs::PermissionsExt;

#[test]
fn component_selection_validates_roles_without_legacy_fallback_or_execution() {
    let root = std::env::temp_dir().join(format!("shell-component-startup-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("desktop.kdl");
    let core = root.join("core.kdl");
    std::fs::write(&core, "schema 2\nsession { application-catalog \"installed\" launch-policy=\"trusted-host\" { source \"/absent/applications\"; }; }\n").unwrap();
    std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o600)).unwrap();
    let bar = r#"shell-component "panel" "bar" { executable "/absent/lom"; gpu "direct"; };"#;
    let launcher =
        r#"shell-component "menu" "application-launcher" { executable "/absent/bemenu-sophia"; };"#;
    let source = |roles: &str, panel: &str, input: &str, catalog: &str| {
        format!(
            "schema 1\nshell {{ enabled #true; content #true; {input} {panel} }}\nsession {{ {roles} {catalog} startup; }}\n"
        )
    };
    let catalog = r#"application-catalog "installed";"#;
    let input = "content-input #true;";
    let valid = source(&format!("{bar} {launcher}"), "panel 32;", input, catalog);
    let args = vec![
        format!("--config={}", core.display()),
        format!("--desktop-profile={}", path.display()),
        "--session-mode=normal".into(),
        "--wm-process=/absent/hagia".into(),
        "--wm-interface=sophia_wm_v1".into(),
        "--shell-process-default=/absent/narthex".into(),
    ];
    let parse = |text: &str, args: &[String]| {
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        PersistentXtermSessionConfig::from_args(args)
    };
    for text in [
        &valid,
        &source(bar, "panel 32;", "", ""),
        &source(launcher, "", input, catalog),
    ] {
        let config = parse(text, &args).unwrap();
        assert!(config.shell_process.is_none());
        assert!(config.shell_config.is_none());
        assert!(config.applications.startup.is_empty());
        assert!(!config.shell_dropped);
    }
    for (text, expected) in [
        (
            source(launcher, "", "", catalog),
            "catalog shell components require",
        ),
        (
            source(launcher, "", input, ""),
            "catalog shell components require",
        ),
        (source(bar, "", "", ""), "positive shell"),
        (
            valid.replace("content #true;", "content #false;"),
            "require shell content",
        ),
        (
            valid.replace("panel 32;", "panel 32; gpu \"direct\";"),
            "per component",
        ),
    ] {
        let error = parse(&text, &args).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
    let no_default: Vec<_> = args
        .iter()
        .filter(|a| !a.starts_with("--shell-process-default="))
        .cloned()
        .collect();
    let independent = parse(&valid, &no_default).unwrap();
    assert!(independent.shell_process.is_none());
    let entries = &independent
        .session_profile
        .candidate()
        .components
        .shell_components;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].gpu, sophia_config::ShellGpuMode::Direct);
    assert_eq!(entries[1].gpu, sophia_config::ShellGpuMode::Denied);
    let no_wm: Vec<_> = args
        .iter()
        .filter(|a| !a.starts_with("--wm-process=") && !a.starts_with("--wm-interface="))
        .cloned()
        .collect();
    assert!(parse(&valid, &no_wm).is_err());
    assert!(
        parse(&valid.replace("installed", "unknown"), &args)
            .unwrap_err()
            .to_string()
            .contains("unknown application catalog")
    );
    assert!(parse(&source(launcher, "panel 32;", input, catalog), &args).is_err());
    let mut conflicting = args.clone();
    conflicting.push("--shell-process=/absent/legacy".into());
    assert!(
        parse(&valid, &conflicting)
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );
    let non_normal: Vec<_> = args
        .iter()
        .filter(|a| *a != "--session-mode=normal")
        .cloned()
        .collect();
    assert!(
        parse(&valid, &non_normal)
            .unwrap_err()
            .to_string()
            .contains("normal-session")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn component_catalog_scan_is_outer_owned_and_shutdown_cannot_restart_it() {
    use super::super::{PreparedSessionProfile, component_catalog::ComponentCatalog};
    use crate::session_actions::SessionLaunchQueue;
    let profile = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/fixtures/mixed_output_probe.kdl");
    let mut config = PersistentXtermSessionConfig::from_args(&[format!(
        "--desktop-profile={}",
        profile.display()
    )])
    .unwrap();
    // Isolate worker lifetime from profile/catalog parsing covered above.
    let mut candidate = config.session_profile.candidate().clone();
    candidate
        .components
        .shell_components
        .push(sophia_config::ShellComponentConfig {
            id: "menu".into(),
            role: sophia_config::ShellComponentRole::ApplicationLauncher,
            executable: "/absent/bemenu-sophia".into(),
            config: None,
            reservation: None,
            gpu: sophia_config::ShellGpuMode::Denied,
            transport: Default::default(),
        });
    config.session_profile = PreparedSessionProfile::new(candidate).unwrap();
    config.application_catalog = Some(sophia_config::ApplicationCatalogConfig {
        name: "empty".into(),
        sources: vec![],
        applications: vec![],
        terminal: None,
        terminal_arguments: vec![],
    });
    let mut owner = ComponentCatalog::default();
    let mut queue = SessionLaunchQueue::default();
    let authority = std::path::Path::new("/unavailable-test-authority");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !owner.visit_scan(&config, &mut queue, authority).unwrap() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(owner.visit_scan(&config, &mut queue, authority).unwrap());
    assert!(queue.admission().is_none());
    let mut failures = vec![];
    owner.stop(&mut queue, &mut failures);
    assert!(failures.is_empty(), "{failures:?}");
    assert!(!owner.visit_scan(&config, &mut queue, authority).unwrap());
    owner.stop(&mut queue, &mut failures);
    assert!(failures.is_empty());
}

#[test]
#[ignore = "explicit device-hidden generated harness profile required"]
fn generated_component_probe_prepares_without_starting_processes() {
    generated_probe(false);
}

#[test]
#[ignore = "explicit device-hidden generated dock harness profile required"]
fn generated_dock_probe_prepares_without_starting_processes() {
    generated_probe(true);
}

fn generated_probe(dock: bool) {
    let profile = std::env::var("SOPHIA_TEST_COMPONENT_PROFILE").unwrap();
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/fixtures/native_launcher_core.kdl");
    let config = PersistentXtermSessionConfig::from_args(&[
        format!("--config={}", core.display()),
        format!("--desktop-profile={profile}"),
        "--session-mode=normal".into(),
        "--wm-process=/absent/hagia".into(),
        "--wm-interface=sophia_wm_v1".into(),
        "--shell-process-default=/absent/narthex".into(),
    ])
    .unwrap();
    assert!(config.shell_process.is_none());
    assert!(config.shell_config.is_none());
    assert!(config.applications.startup.is_empty());
    assert_eq!(config.shell_panel_thickness, Some(24));
    assert!(config.shell_content_enabled && config.shell_content_input_enabled);
    let components = &config
        .session_profile
        .candidate()
        .components
        .shell_components;
    assert_eq!(components.len(), if dock { 3 } else { 2 });
    assert_eq!(components[0].role, sophia_config::ShellComponentRole::Bar);
    assert_eq!(components[0].gpu, sophia_config::ShellGpuMode::Direct);
    assert_eq!(
        components[1].role,
        sophia_config::ShellComponentRole::ApplicationLauncher
    );
    assert_eq!(components[1].gpu, sophia_config::ShellGpuMode::Denied);
    if dock {
        assert_eq!(components[2].role, sophia_config::ShellComponentRole::Dock);
        assert_eq!(components[2].gpu, sophia_config::ShellGpuMode::Direct);
        assert_eq!(components[2].reservation.unwrap().max_thickness, 64);
        assert_ne!(
            components[0].reservation.unwrap().edge,
            components[2].reservation.unwrap().edge
        );
    }
    let catalog = config.application_catalog.as_ref().unwrap();
    assert_eq!(catalog.name, "native-launcher-gate");
    assert_eq!(catalog.applications, ["terminal"]);
}
