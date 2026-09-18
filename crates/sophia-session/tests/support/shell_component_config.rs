use super::PersistentXtermSessionConfig;
use std::os::unix::fs::PermissionsExt;

#[test]
fn component_selection_refuses_before_legacy_fallback_or_executable_inspection() {
    let root = std::env::temp_dir().join(format!("shell-component-startup-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("desktop.kdl");
    std::fs::write(
        &path,
        r#"schema 1
shell { enabled #true; }
session {
  shell-component "panel" "bar" { executable "/absent/lom"; gpu "direct"; }
  shell-component "menu" "application-launcher" { executable "/absent/bemenu-sophia"; }
  startup
}
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    for fallback in [
        None,
        Some("--shell-process=/absent/legacy"),
        Some("--shell-process-default=/absent/default"),
    ] {
        let mut args = vec![
            format!("--desktop-profile={}", path.display()),
            "--session-mode=normal".to_owned(),
        ];
        if let Some(fallback) = fallback {
            args.push(fallback.to_owned());
        }
        let error = PersistentXtermSessionConfig::from_args(&args)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "independent shell components require revision-7 Session admission, which is not implemented"
        );
    }
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
    // Prepare the internal selected state directly: the public configuration
    // guard is intentionally still closed until all live input is integrated.
    let mut candidate = config.session_profile.candidate().clone();
    candidate
        .components
        .shell_components
        .push(sophia_config::ShellComponentConfig {
            id: "menu".into(),
            role: sophia_config::ShellComponentRole::ApplicationLauncher,
            executable: "/absent/bemenu-sophia".into(),
            config: None,
            gpu: sophia_config::ShellGpuMode::Denied,
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
