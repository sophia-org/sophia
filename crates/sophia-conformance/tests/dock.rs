use sophia_conformance::dock::{profile, verify};

fn positive() -> String {
    let mut log = String::from(
        "sophia_live_wm_configuration schema=2 status=committed\nsophia_shell_component_catalog schema=1 status=built generation=1 entries=1\n",
    );
    for (slot, role, revision, mode) in [
        (0, "bar", 6, "direct"),
        (1, "application_launcher", 7, "denied"),
        (2, "dock", 8, "direct"),
    ] {
        let g = slot + 1;
        let (gpu, major, minor) = if mode == "direct" {
            (g, 226, 128)
        } else {
            (0, 0, 0)
        };
        log += &format!(
            "sophia_shell_component schema=1 status=negotiated slot={slot} role={role} connection_epoch={g} content_grant_epoch={g} revision={revision} gpu_mode={mode} gpu_grant_epoch={gpu} device_major={major} device_minor={minor}\n"
        );
        log += &format!(
            "sophia_live_shell_content schema=1 status=outputs connection_epoch={g} content_grant_epoch={g} facts_generation=1 outputs=2\n"
        );
        for output in 1..=2 {
            for candidate in 1..=2 {
                log += &format!(
                    "sophia_live_shell_content schema=1 status=presented connection_epoch={g} content_grant_epoch={g} output={output} candidate_generation={candidate} presentation_epoch={candidate}\n"
                );
            }
            if slot != 0 {
                let cause = if slot == 1 { "transient" } else { "persistent" };
                log += &format!(
                    "sophia_catalog_launch schema=1 status=process_started transaction={} cause={cause} connection_epoch={g} content_grant_epoch={g} output={output} event_id={output}\n",
                    g * 10 + output
                );
            }
        }
    }
    log + "sophia_shell_components_shutdown schema=1 status=quiescent\n"
}

#[test]
fn three_component_smoke_requires_exact_independent_lifetimes_and_real_launch_records() {
    let good = positive();
    assert!(
        verify(&good)
            .unwrap()
            .contains("latency_acceptance=NOT_RUN")
    );
    let structured = good
        .lines()
        .map(|line| format!("1\t2\tinfo\t{line}\n"))
        .collect::<String>();
    assert!(verify(&structured).is_ok());
    for (from, to) in [
        ("role=dock", "role=bar"),
        ("revision=8", "revision=7"),
        ("gpu_mode=direct", "gpu_mode=denied"),
        ("gpu_grant_epoch=3", "gpu_grant_epoch=0"),
        ("connection_epoch=3", "connection_epoch=1"),
        ("device_major=226", "device_major=226 device_major=226"),
        ("device_minor=128", "device_minor=4294967296"),
        ("status=quiescent", "status=retained"),
        ("status=committed", "status=rejected"),
        ("cause=persistent", "cause=transient"),
        ("output=2", "output=0"),
        ("sophia_catalog_launch", "client_claimed_launch"),
    ] {
        assert!(good.contains(from));
        assert!(verify(&good.replace(from, to)).is_err(), "{from} -> {to}");
    }
    for prefix in [
        "sophia_shell_components_shutdown",
        "sophia_shell_component_catalog",
    ] {
        assert!(
            verify(
                &good
                    .lines()
                    .filter(|line| !line.starts_with(prefix))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
            .is_err()
        );
    }
    assert!(verify(&(good.clone() + "sophia_runtime_fatal schema=1\n")).is_err());
    assert!(verify(&(good + "sophia_live_shell_content schema=1 status=outputs\n")).is_err());
}

#[test]
fn actual_generated_profile_has_three_roles_separate_reservations_and_no_wm_overrides() {
    use sophia_config::*;
    use std::os::unix::fs::PermissionsExt;
    let paths = [
        "/opt/lom",
        "/opt/lom config.kdl",
        "/opt/bemenu",
        "/opt/provlita",
        "/opt/dock.kdl",
    ]
    .map(String::from);
    let text = profile(&paths).unwrap();
    let directory = std::env::temp_dir().join(format!("dock-profile-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.join("desktop.kdl");
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let prepared = load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
    let candidate = &prepared.candidates.session;
    assert_eq!(candidate.startup, Some(Vec::new()));
    let components = &candidate.components.shell_components;
    assert_eq!(components.len(), 3);
    assert_eq!(components[0].role, ShellComponentRole::Bar);
    assert_eq!(components[1].role, ShellComponentRole::ApplicationLauncher);
    assert_eq!(components[2].role, ShellComponentRole::Dock);
    assert_eq!(
        components[0].reservation.unwrap().edge,
        ShellComponentEdge::Top
    );
    assert_eq!(
        components[2].reservation.unwrap().edge,
        ShellComponentEdge::Bottom
    );
    assert_eq!(components[2].reservation.unwrap().max_thickness, 64);
    assert!(components[1].reservation.is_none());
    assert_eq!(components[1].gpu, ShellGpuMode::Denied);
    assert_eq!(components[2].gpu, ShellGpuMode::Direct);
    for authority in [DesktopAuthority::Policy, DesktopAuthority::Shortcut] {
        assert!(prepared.profile.candidates[&authority].values.is_empty());
    }
    assert!(profile(&paths[..4]).is_err());
    for bad in ["relative", "/tmp/a\ninput"] {
        let mut wrong = paths.clone();
        wrong[3] = bad.into();
        assert!(profile(&wrong).is_err());
    }
    std::fs::remove_dir_all(directory).unwrap();
}
