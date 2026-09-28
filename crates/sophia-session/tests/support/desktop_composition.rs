use super::*;

#[test]
fn desktop_components_override_launcher_defaults_and_cli_overrides_the_desktop() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("sophia-components-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let core = root.join("core.kdl");
    let desktop = root.join("desktop.kdl");
    std::fs::write(
        &core,
        "schema 2\nexternal-wm executable=\"/usr/bin/core-wm\" { arg \"--core\"; }\n",
    )
    .unwrap();
    std::fs::write(&desktop, "schema 1\nshell { enabled #true; }\nsession { window-manager \"/usr/bin/profile-wm\" \"--profile\"; shell-client \"/usr/bin/profile-shell\"; startup; }\n").unwrap();
    for path in [&core, &desktop] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut args = vec![
        format!("--config={}", core.display()),
        format!("--desktop-profile={}", desktop.display()),
        "--no-input".to_owned(),
        "--session-mode=normal".to_owned(),
        "--wm-process-default=/usr/bin/default-wm".to_owned(),
        "--session-app=terminal=/usr/bin/true".to_owned(),
        "--session-start-default=terminal".to_owned(),
        "--startup-ready-timeout-ms=8000".to_owned(),
    ];
    let config = PersistentXtermSessionConfig::from_args(&args).unwrap();
    assert_eq!(config.wm_process.as_deref(), Some("/usr/bin/profile-wm"));
    assert_eq!(config.wm_process_args, ["--profile"]);
    assert_eq!(
        config.shell_process.as_deref(),
        Some("/usr/bin/profile-shell")
    );
    assert!(config.applications.startup.is_empty());
    assert!(config.startup_ready_timeout.is_none());
    args.extend([
        "--wm-process=/usr/bin/explicit-wm".to_owned(),
        "--shell-process=/usr/bin/explicit-shell".to_owned(),
    ]);
    let config = PersistentXtermSessionConfig::from_args(&args).unwrap();
    assert_eq!(config.wm_process.as_deref(), Some("/usr/bin/explicit-wm"));
    assert!(config.wm_process_args.is_empty());
    assert_eq!(
        config.shell_process.as_deref(),
        Some("/usr/bin/explicit-shell")
    );
    args.truncate(args.len() - 2);
    std::fs::write(&desktop, "schema 1\nshell { enabled #true; }\n").unwrap();
    assert!(
        PersistentXtermSessionConfig::from_args(&args)
            .unwrap_err()
            .to_string()
            .contains("explicit shell executable")
    );
    args.push("--shell-process=/usr/bin/explicit-shell".to_owned());
    let config = PersistentXtermSessionConfig::from_args(&args).unwrap();
    assert_eq!(config.wm_process.as_deref(), Some("/usr/bin/core-wm"));
    assert_eq!(config.wm_process_args, ["--core"]);
    assert_eq!(
        config.shell_process.as_deref(),
        Some("/usr/bin/explicit-shell")
    );
    std::fs::write(&core, "schema 2\n").unwrap();
    let config = PersistentXtermSessionConfig::from_args(&args).unwrap();
    assert_eq!(config.wm_process.as_deref(), Some("/usr/bin/default-wm"));
    args.pop();
    std::fs::write(
        &desktop,
        "schema 1\nshell { enabled #false; }\nsession { startup; }\n",
    )
    .unwrap();
    assert!(
        PersistentXtermSessionConfig::from_args(&args)
            .unwrap()
            .shell_process
            .is_none()
    );
    std::fs::write(&desktop, "schema 1\nshell { enabled #true; }\nsession { shell-config \"/missing/private-shell.kdl\"; }\n").unwrap();
    args.push("--shell-process=/usr/bin/explicit-shell".to_owned());
    assert!(
        PersistentXtermSessionConfig::from_args(&args)
            .unwrap_err()
            .to_string()
            .contains("private shell config")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn retired_shell_default_is_refused_before_loading_configuration() {
    for retired in [
        "--shell-process-default",
        "--shell-process-default=",
        "--shell-process-default=/usr/bin/true",
        "--shell-process-default=relative",
    ] {
        for explicit in [None, Some("--shell-process=/usr/bin/true")] {
            let mut args = vec![
                "--desktop-profile=/does/not/exist.kdl".to_owned(),
                retired.to_owned(),
            ];
            args.extend(explicit.map(str::to_owned));
            assert_eq!(
                PersistentXtermSessionConfig::from_args(&args)
                    .unwrap_err()
                    .to_string(),
                "--shell-process-default is retired; select shell components in the desktop profile"
            );
        }
    }
}
