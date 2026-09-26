use super::*;

#[test]
fn host_application_endpoints_replace_inherited_values_independently() {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::Command;

    for (control, inspection) in [
        (None, None),
        (Some(Path::new("/session/control.sock")), None),
        (None, Some(Path::new("/session/inspection.sock"))),
        (
            Some(Path::new("/session/control.sock")),
            Some(Path::new("/session/inspection.sock")),
        ),
    ] {
        let mut command = Command::new("/bin/true");
        command
            .env(
                sophia_runtime::SOPHIA_CONTROL_SOCKET_ENV,
                "/inherited/control.sock",
            )
            .env(
                sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV,
                "/inherited/inspection.sock",
            );
        crate::application_catalog::configure_host_application_environment(
            &mut command,
            control,
            inspection,
        );
        for (name, expected) in [
            (sophia_runtime::SOPHIA_CONTROL_SOCKET_ENV, control),
            (sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV, inspection),
        ] {
            // None records an explicit removal, including inherited OS values.
            assert_eq!(
                command.get_envs().find(|(key, _)| *key == OsStr::new(name)),
                Some((OsStr::new(name), expected.map(Path::as_os_str))),
            );
        }
    }
}

#[test]
fn inspection_socket_is_absent_until_the_session_starts_its_service() {
    use std::os::unix::fs::PermissionsExt;
    let profile = std::env::temp_dir().join(format!(
        "sophia-inspection-startup-{}.kdl",
        std::process::id(),
    ));
    for (settings, control, inspection) in [
        (
            "",
            sophia_config::DesktopControlAccess::Disabled,
            sophia_config::DesktopInspectionAccess::Disabled,
        ),
        (
            "control \"host-admin\";",
            sophia_config::DesktopControlAccess::HostAdmin,
            sophia_config::DesktopInspectionAccess::Disabled,
        ),
        (
            "inspection \"host-admin\";",
            sophia_config::DesktopControlAccess::Disabled,
            sophia_config::DesktopInspectionAccess::HostAdmin,
        ),
    ] {
        std::fs::write(&profile, format!("schema 1\nsession {{ {settings} }}\n")).unwrap();
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
        let config =
            isolated_session_config(&[format!("--desktop-profile={}", profile.display())]).unwrap();
        assert_eq!(config.control_access, control);
        assert_eq!(config.inspection_access, inspection);
        assert_eq!(config.control_socket, None);
        assert_eq!(config.inspection_socket, None);
    }
    std::fs::remove_file(profile).unwrap();
}

#[test]
fn protected_wm_environment_and_grants_do_not_receive_host_inspection() {
    let mut config = isolated_session_config(&["--wm-process=/usr/bin/true".to_owned()]).unwrap();
    let spec = |config: &PersistentXtermSessionConfig| {
        public_policy_launch_spec(
            config,
            "/usr/bin/true",
            std::path::Path::new("/tmp/wm-endpoint/wm.sock"),
            std::path::Path::new("/tmp/wm-checkpoint/state"),
            std::path::Path::new("/tmp/wm-profile/candidate"),
            true,
            Some(std::path::Path::new("/tmp/output-endpoint/output.sock")),
        )
        .unwrap()
    };
    let disabled = spec(&config);
    config.inspection_access = sophia_config::DesktopInspectionAccess::HostAdmin;
    config.inspection_socket = Some("/private/inspection.sock".into());
    let enabled = spec(&config);
    assert_eq!(disabled, enabled);
    assert!(enabled.protection_domain.is_some());
    assert!(
        !enabled
            .environment
            .iter()
            .any(|(name, _)| { name == sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV })
    );
}
