use super::*;

#[test]
fn retired_shell_wire_selection_is_refused() {
    assert!(
        isolated_session_config(&["--no-config".into(), "--shell-transport=9p2000.L".into(),])
            .unwrap_err()
            .to_string()
            .contains("--shell-transport is retired")
    );
}

#[test]
fn wm_transport_defaults_to_files_and_refuses_retired_ipc() {
    use crate::live_session::WmTransportSelection;
    let base = vec![
        "--wm-process=/usr/bin/true".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
    ];
    assert_eq!(
        isolated_session_config(&base).unwrap().wm_transport,
        WmTransportSelection::NineP2000L
    );
    let mut args = base.clone();
    args.push("--wm-transport=9p2000.L".into());
    assert_eq!(
        isolated_session_config(&args).unwrap().wm_transport,
        WmTransportSelection::NineP2000L
    );
    for flag in ["current-ipc", "auto", "9p", "sophia_wm_v1", ""] {
        let mut args = base.clone();
        args.push(format!("--wm-transport={flag}"));
        assert!(
            isolated_session_config(&args)
                .unwrap_err()
                .to_string()
                .contains("--wm-transport expects")
        );
    }
    assert!(
        isolated_session_config(&[
            "--no-config".to_owned(),
            "--wm-transport=9p2000.L".to_owned()
        ])
        .unwrap_err()
        .to_string()
        .contains("--wm-transport requires")
    );
}

#[test]
fn selected_wm_socket_does_not_change_profile_output_or_checkpoint_grants() {
    use crate::live_session::WmTransportSelection;
    let mut config = isolated_session_config(&["--wm-process=/usr/bin/true".to_owned()]).unwrap();
    let default = public_policy_launch_spec(
        &config,
        "/usr/bin/true",
        std::path::Path::new("/tmp/wm-endpoint/wm.sock"),
        std::path::Path::new("/tmp/wm-checkpoint/state"),
        std::path::Path::new("/tmp/wm-profile/candidate"),
        true,
        Some(std::path::Path::new("/tmp/output-endpoint/output.sock")),
    )
    .unwrap();
    config.wm_transport = WmTransportSelection::NineP2000L;
    let files = public_policy_launch_spec(
        &config,
        "/usr/bin/true",
        std::path::Path::new("/tmp/wm-endpoint/wm.sock"),
        std::path::Path::new("/tmp/wm-checkpoint/state"),
        std::path::Path::new("/tmp/wm-profile/candidate"),
        true,
        Some(std::path::Path::new("/tmp/output-endpoint/output.sock")),
    )
    .unwrap();
    assert_eq!(default.protection_domain, files.protection_domain);
    assert_eq!(default.args, files.args);
    assert_eq!(files.environment, default.environment);
    assert!(
        files
            .environment
            .iter()
            .any(|(key, _)| key == "SOPHIA_WM_9P_SOCKET")
    );
    assert!(
        !files
            .environment
            .iter()
            .any(|(key, _)| key == "SOPHIA_WM_SOCKET")
    );
    assert_eq!(config.wm_transport.wire_name(), "sophia_wm_fs_v1");
}
