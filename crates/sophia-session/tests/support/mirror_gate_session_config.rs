use super::session_config_tests::isolated_session_config;
use super::*;

#[test]
fn mirror_gate_declares_the_terminal_before_extending_its_arguments() {
    let profile = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/fixtures/mirror_group_probe.kdl");
    let config = isolated_session_config(&[
        format!("--desktop-profile={}", profile.display()),
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/xterm".to_owned(),
        "--session-start=terminal".to_owned(),
        "--session-app-arg=terminal=-cm".to_owned(),
        "--session-app-arg=terminal=-dc".to_owned(),
        "--session-app-arg=terminal=-fn".to_owned(),
        "--session-app-arg=terminal=6x13".to_owned(),
        "--max-runtime-ms=15000".to_owned(),
    ])
    .unwrap();

    assert!(config.normal_session);
    assert_eq!(config.applications.startup, ["terminal"]);
    let terminal = config.applications.applications.get("terminal").unwrap();
    assert_eq!(terminal.executable, std::path::Path::new("/usr/bin/xterm"));
    assert_eq!(terminal.arguments, ["-cm", "-dc", "-fn", "6x13"]);
}

#[test]
fn mirror_gate_profile_cannot_be_combined_with_no_config() {
    let error = PersistentXtermSessionConfig::from_args(&[
        "--desktop-profile=/tmp/mirror-group-probe.kdl".to_owned(),
        "--no-config".to_owned(),
        "--session-mode=normal".to_owned(),
        "--session-app=terminal=/usr/bin/xterm".to_owned(),
        "--session-start=terminal".to_owned(),
    ])
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("--no-config and --desktop-profile are mutually exclusive")
    );
}
