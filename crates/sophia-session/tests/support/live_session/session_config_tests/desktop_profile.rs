#[test]
fn normal_hagia_session_resolves_one_separate_shell_executable() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{SystemTime, UNIX_EPOCH};

    let path = std::env::temp_dir().join(format!(
        "sophia-live-shell-profile-{}-{}.kdl",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &path,
        r#"schema 1
policy {}
shell { enabled #true; }
shortcut {
  profile "shell-test"
  bind "Super+p" "session:window-switcher"
}
session { terminal "terminal"; browser "browser"; }
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let base = [
        isolated_core_config_argument(),
        format!("--desktop-profile={}", path.display()),
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/true".to_owned(),
        "--session-start=terminal".to_owned(),
        "--session-app=browser=/usr/bin/true".to_owned(),
        "--wm-process=/opt/hagia".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
    ];
    let config = PersistentXtermSessionConfig::from_args(&base).unwrap();
    assert_eq!(config.shell_process.as_deref(), Some("/opt/narthex"));

    let mut explicit = base.to_vec();
    explicit.push("--shell-process=/srv/narthex".to_owned());
    explicit.push("--shell-proof-restart-after-visible=2".to_owned());
    let config = PersistentXtermSessionConfig::from_args(&explicit).unwrap();
    assert_eq!(config.shell_process.as_deref(), Some("/srv/narthex"));
    assert_eq!(config.shell_proof_restart_after_visible, Some(2));

    assert!(
        PersistentXtermSessionConfig::from_args(&[
            isolated_core_config_argument(),
            format!("--desktop-profile={}", path.display()),
            "--shell-process=/srv/narthex".to_owned(),
        ])
        .unwrap_err()
        .to_string()
        .contains("session-mode=normal")
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn production_content_requires_the_complete_explicit_shell_authority() {
    use std::os::unix::fs::PermissionsExt as _;

    let profile = std::env::temp_dir().join(format!(
        "sophia-content-profile-{}-{}.kdl",
        std::process::id(),
        line!()
    ));
    let arguments = |profile: &std::path::Path| {
        vec![
            format!("--desktop-profile={}", profile.display()),
            "--session-mode=normal".to_owned(),
            "--session-app=terminal=/usr/bin/true".to_owned(),
            "--session-start=terminal".to_owned(),
            "--wm-process=/opt/hagia".to_owned(),
            "--wm-interface=sophia_wm_v1".to_owned(),
            "--shell-process=/opt/lom".to_owned(),
        ]
    };
    let write = |source: &str| {
        std::fs::write(&profile, source).unwrap();
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    };

    write(
        "schema 1\nshell { enabled #true; content #true; content-input #true; panel 32; gpu \"direct\"; }\n",
    );
    let config = PersistentXtermSessionConfig::from_args(&arguments(&profile)).unwrap();
    assert!(config.shell_content_enabled);
    assert!(config.shell_content_input_enabled);
    assert_eq!(config.shell_gpu_mode, sophia_config::ShellGpuMode::Direct);

    write("schema 1\nshell { enabled #true; content #true; panel 32; }\n");
    let config = PersistentXtermSessionConfig::from_args(&arguments(&profile)).unwrap();
    assert!(config.shell_content_enabled);
    assert!(!config.shell_content_input_enabled);
    assert_eq!(config.shell_gpu_mode, sophia_config::ShellGpuMode::Denied);

    for source in [
        "schema 1\nshell { enabled #true; content #true; gpu \"direct\"; }\n",
        "schema 1\nshell { enabled #true; content #true; panel 32; gpu-memory-bytes 268435456; }\n",
        "schema 1\nshell { enabled #true; content-input #true; panel 32; }\n",
    ] {
        write(source);
        assert!(PersistentXtermSessionConfig::from_args(&arguments(&profile)).is_err());
    }
    std::fs::remove_file(profile).unwrap();
}

#[test]
fn desktop_profile_is_validated_and_partitioned_during_session_configuration() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{SystemTime, UNIX_EPOCH};

    let path = std::env::temp_dir().join(format!(
        "sophia-live-desktop-profile-{}-{}.kdl",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &path,
        "schema 1\npolicy { layout \"scroller\"; view-count 7; outer-gap 3; }\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let config =
        isolated_session_config(&[format!("--desktop-profile={}", path.display())]).unwrap();
    let policy = config
        .desktop_profile
        .candidates
        .get(&sophia_config::DesktopAuthority::Policy)
        .unwrap();
    assert_eq!(policy.values.len(), 3);

    std::fs::write(&path, "schema 1\npolicy { view-count 99; }\n").unwrap();
    // Session configuration admits the envelope; the WM owns view-count.
    isolated_session_config(&[format!("--desktop-profile={}", path.display())]).unwrap();
    std::fs::write(&path, "schema 1\npolicy { max-surfaces 99; }\n").unwrap();
    assert!(
        isolated_session_config(&[format!("--desktop-profile={}", path.display())])
            .unwrap_err()
            .to_string()
            .contains("reserved control")
    );
    assert!(
        isolated_session_config(&["--desktop-profile=relative.kdl".to_owned()])
            .unwrap_err()
            .to_string()
            .contains("absolute")
    );
    assert!(
        isolated_session_config(&[
            "--no-config".to_owned(),
            format!("--desktop-profile={}", path.display()),
        ])
        .unwrap_err()
        .to_string()
        .contains("mutually exclusive")
    );
    let compiled = isolated_session_config(&["--no-config".to_owned()]).unwrap();
    assert_eq!(
        compiled.desktop_profile.sources,
        vec![std::path::PathBuf::from("<compiled>")]
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn desktop_session_candidate_selects_only_registered_applications() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{SystemTime, UNIX_EPOCH};

    let path = std::env::temp_dir().join(format!(
        "sophia-live-session-candidate-{}-{}.kdl",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &path,
        r#"schema 1
policy {}
shortcut {
  profile "test"
  bind "Super+Return" "session:spawn-terminal"
  bind "Super+b" "session:spawn-browser"
}
session { terminal "kitty"; browser "helium"; startup "kitty"; }
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let base = [
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/kitty".to_owned(),
        "--session-app=browser=/opt/helium/helium".to_owned(),
        "--wm-process=/usr/bin/true".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
        format!("--desktop-profile={}", path.display()),
    ];
    let config = PersistentXtermSessionConfig::from_args(&base).unwrap();
    assert_eq!(config.applications.terminal.as_deref(), Some("terminal"));
    assert_eq!(config.applications.browser.as_deref(), Some("browser"));
    assert_eq!(config.applications.startup, ["terminal"]);

    let mut overridden = base.to_vec();
    overridden.extend([
        "--session-app=alternate=/usr/bin/xterm".to_owned(),
        "--session-action-app=terminal=alternate".to_owned(),
        "--session-start=alternate".to_owned(),
    ]);
    let config = PersistentXtermSessionConfig::from_args(&overridden).unwrap();
    assert_eq!(config.applications.terminal.as_deref(), Some("alternate"));
    assert_eq!(config.applications.startup, ["alternate"]);

    let unavailable = base
        .iter()
        .filter(|argument| !argument.starts_with("--session-app=browser="))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        PersistentXtermSessionConfig::from_args(&unavailable)
            .unwrap_err()
            .to_string()
            .contains("unavailable session capability")
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn desktop_input_candidate_overlays_keyboard_with_cli_precedence() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{SystemTime, UNIX_EPOCH};

    let path = std::env::temp_dir().join(format!(
        "sophia-live-input-candidate-{}-{}.kdl",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::write(
        &path,
        r#"schema 1
input {
  inherit-sophia #true
  keyboard {
    repeat-rate 40
    repeat-delay 300
    numlock #true
    capslock #false
    xkb { model "profile-model"; layout "de"; }
  }
  pointer {
    natural-scroll #true
    accel-profile "flat"
    accel-speed -0.25
    left-handed #true
    middle-emulation #true
    scroll-factor 1.5
  }
}
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let config = isolated_session_config(&[
        format!("--desktop-profile={}", path.display()),
        "--xkb-layout=us".to_owned(),
    ])
    .unwrap();
    assert_eq!(config.xkb_config.model, "profile-model");
    assert_eq!(config.xkb_config.layout, "us");
    assert_eq!(config.key_repeat_config.delay_msec, 300);
    assert_eq!(config.key_repeat_config.interval_msec, 25);
    assert_eq!(config.keyboard_mapper().modifier_mask(), 1 << 4);
    assert_eq!(
        config.native_pointer_policy(),
        sophia_backend_live::NativeLibinputPointerPolicy {
            natural_scroll: Some(true),
            accel_profile: Some(sophia_backend_live::NativeLibinputAccelProfile::Flat),
            accel_speed: Some(-0.25),
            left_handed: Some(true),
            middle_emulation: Some(true),
            scroll_factor: 1.5,
        }
    );

    std::fs::remove_file(path).unwrap();
}

#[test]
fn public_policy_launch_receives_only_the_staged_policy_candidate() {
    let config = isolated_session_config(&[]).unwrap();
    let spec = public_policy_launch_spec(
        &config,
        "/usr/bin/hagia",
        std::path::Path::new("/run/user/1000/sophia/policy/endpoint/wm.sock"),
        std::path::Path::new("/run/user/1000/sophia/policy/checkpoint/hagia-policy.checkpoint"),
        std::path::Path::new("/run/user/1000/sophia/policy/policy.profile.kdl"),
        false,
        None,
    )
    .unwrap();
    assert!(spec.environment.contains(&(
        "HAGIA_POLICY_CANDIDATE".into(),
        "/run/user/1000/sophia/policy/policy.profile.kdl".into()
    )));
    assert!(
        spec.environment
            .iter()
            .all(|(_, value)| !value.to_string_lossy().contains("session.profile.kdl"))
    );
    assert!(
        spec.environment
            .iter()
            .all(|(name, _)| name != "HAGIA_POLICY_PROFILE_ACTIVATION")
    );
    let domain = spec
        .protection_domain
        .as_ref()
        .expect("public policy always has a protection domain");
    assert_eq!(
        domain.roles(),
        &[sophia_runtime::ProtectionDomainRole::SpatialPolicy]
            .into_iter()
            .collect()
    );
    assert_eq!(
        domain.network(),
        sophia_runtime::ProtectionNetworkAccess::Denied
    );
    assert_eq!(domain.paths().len(), 3);
    assert_eq!(
        domain.paths()[0],
        sophia_runtime::ProtectionPath::read_only(
            "/run/user/1000/sophia/policy/policy.profile.kdl"
        )
    );
    assert_eq!(
        domain.paths()[1],
        sophia_runtime::ProtectionPath::read_only("/run/user/1000/sophia/policy/endpoint")
    );
    assert_eq!(
        domain.paths()[2],
        sophia_runtime::ProtectionPath::read_write("/run/user/1000/sophia/policy/checkpoint")
    );

    let activated = public_policy_launch_spec(
        &config,
        "/usr/bin/hagia",
        std::path::Path::new("/run/user/1000/sophia/policy/endpoint/wm.sock"),
        std::path::Path::new("/run/user/1000/sophia/policy/checkpoint/hagia-policy.checkpoint"),
        std::path::Path::new("/run/user/1000/sophia/policy/policy.profile.kdl"),
        true,
        Some(std::path::Path::new(
            "/run/user/1000/sophia/policy/output-endpoint/output.sock",
        )),
    )
    .unwrap();
    assert!(
        activated
            .environment
            .contains(&("HAGIA_POLICY_PROFILE_ACTIVATION".into(), "required".into()))
    );
    assert!(activated.environment.contains(&(
        sophia_runtime::SOPHIA_OUTPUT_SOCKET_ENV.into(),
        "/run/user/1000/sophia/policy/output-endpoint/output.sock".into(),
    )));
    let domain = activated.protection_domain.as_ref().unwrap();
    assert!(
        domain
            .roles()
            .contains(&sophia_runtime::ProtectionDomainRole::SpatialPolicy)
    );
    assert!(
        domain
            .roles()
            .contains(&sophia_runtime::ProtectionDomainRole::OutputAuthority)
    );
    assert_eq!(domain.paths().len(), 4);
    assert_eq!(
        domain.paths()[3],
        sophia_runtime::ProtectionPath::read_only("/run/user/1000/sophia/policy/output-endpoint")
    );
}
