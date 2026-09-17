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
