use super::*;

#[test]
fn hagia_pregraphics_profile_admission_rejects_invalid_policy_values() {
    let Some(hagia_bin) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    for source in [
        // Keyed to the literal the compiled profile actually contains. If that
        // spelling drifts, `replace` silently returns the profile unchanged and
        // this case starts asserting that Hagia accepts a valid profile.
        sophia_config::COMPILED_DESKTOP_PROFILE.replace("gaps 0", "gaps 513"),
        sophia_config::COMPILED_DESKTOP_PROFILE.to_owned()
            + "\npolicy { view-name 1 \"a\"; view-name 1 \"b\"; }\n",
        sophia_config::COMPILED_DESKTOP_PROFILE.to_owned() + "\npolicy { future-wm-setting 1; }\n",
    ] {
        let mut config = public_profile_test_config("sophia-hagia-invalid-policy");
        config.wm_process = Some(hagia_bin.to_string_lossy().into_owned());
        let path = config.wm_socket_path.with_extension("kdl");
        std::fs::write(&path, source).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        let socket_path = config.wm_socket_path.clone();
        config = PersistentXtermSessionConfig::from_args(&[
            "--session-mode=normal".to_owned(),
            format!("--wm-process={}", hagia_bin.to_string_lossy()),
            "--wm-interface=sophia_wm_v1".to_owned(),
            // This is an explicit profile: its launcher bindings must be
            // admitted before the test can reach Hagia's policy rejection.
            "--session-app=terminal=/usr/bin/true".to_owned(),
            "--session-app=browser=/usr/bin/true".to_owned(),
            format!("--desktop-profile={}", path.display()),
        ])
        .unwrap();
        config.wm_socket_path = socket_path;
        std::fs::remove_file(path).unwrap();
        let prepared = LiveWmSession::prepare_public_launch(&mut config).unwrap();
        assert!(LiveWmSession::activate_public_launch(&mut config, prepared).is_err());
        assert_eq!(config.desktop_profile_activation.active(), None);
        assert_eq!(
            config.desktop_profile_activation.phase(),
            sophia_config::DesktopProfileActivationPhase::Idle
        );
        for participant in [
            config.session_profile.slot().participant(),
            config.input_profile.slot().participant(),
            config.output_profile.slot().participant(),
        ] {
            assert_eq!(participant.active(), None);
            assert_eq!(participant.candidate(), None);
        }
        assert!(!config.wm_socket_path.with_extension("policy").exists());
        assert!(!config.wm_socket_path.exists());
    }
}
