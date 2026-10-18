//! No live display/device. Preparation/failed-spawn controls plus explicitly
//! selected, device-hidden protected-process negotiation controls.
use sophia_config::ShellComponentRole;
use sophia_runtime::{
    ProcessLaunchSpec, ProtectionDomainRole, ProtectionDomainSpec, ShellContentAdmissionPolicy,
};
use sophia_session::shell_component_connections::ComponentConnectionPhase;
use sophia_session::shell_component_processes::*;

#[test]
fn failed_preparation_and_spawn_burn_exact_epochs_without_losing_neighbor() {
    let directory =
        std::env::temp_dir().join(format!("component-processes-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut owner = ShellComponentProcesses::new().unwrap();
    for (name, role) in [
        ("bar", ShellComponentRole::Bar),
        ("menu", ShellComponentRole::ApplicationLauncher),
    ] {
        owner
            .add_with_transport(
                name,
                role,
                &directory.join(name),
                rustix::process::geteuid().as_raw(),
                sophia_config::ShellTransportSelection::NineP2000L,
            )
            .unwrap();
    }
    let policy = ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    };
    assert!(
        owner
            .start(0, |_, _| Err("preparation refused".into()), policy)
            .is_err()
    );
    let first = owner.attempt(0).unwrap();
    assert_eq!(
        owner.phase(first).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    assert!(
        owner
            .start(1, |_, _| Ok(ProcessLaunchSpec::new("/bin/true")), policy)
            .is_err()
    );
    let neighbor = owner.attempt(1).unwrap();
    assert_ne!(first.grant, neighbor.grant);
    assert_eq!(
        owner.phase(first).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    assert!(!owner.process_retained(neighbor)); // unprotected spec never spawned
    assert!(
        owner
            .start(
                0,
                |key, _| {
                    assert_ne!(key, first);
                    Ok(
                        ProcessLaunchSpec::new("/nonexistent-sophia-component").protection_domain(
                            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
                                .unwrap(),
                        ),
                    )
                },
                policy
            )
            .is_err()
    );
    let second = owner.attempt(0).unwrap();
    assert!(owner.request_stop(first).is_err());
    assert!(owner.process_retained(second)); // retained supervisor after spawn failure
    assert!(owner.finish_after_backend_drop(()).is_err());
    let visit = owner.visit(1024);
    assert!(visit.processes.iter().flatten().any(
        |event| matches!(event, ComponentProcessEvent::ProcessRetired(key, Ok(())) if *key == second)
    ));
    assert!(!owner.process_retained(second));
    assert_eq!(
        owner.phase(neighbor).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    assert!(owner.finish_after_backend_drop(()).unwrap().1.quiescent());
    drop(owner);
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(feature = "native-session")]
#[test]
fn component_scheduler_spaces_a_service_failure_like_a_refused_start() {
    use sophia_session::shell_component_session::ShellComponentSession;
    use std::time::{Duration, Instant};
    let root =
        std::env::temp_dir().join(format!("component-service-spacing-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let selection = sophia_config::ShellComponentConfig {
        id: "bar".into(),
        role: ShellComponentRole::Bar,
        executable: "/nonexistent-sophia-component".into(),
        config: None,
        reservation: None,
        gpu: sophia_config::ShellGpuMode::Denied,
        transport: sophia_config::ShellTransportSelection::NineP2000L,
    };
    let mut owner = ShellComponentSession::prepare(
        &[selection],
        28,
        None,
        &root,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
        std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
    )
    .unwrap();
    owner.set_presentation_available(true).unwrap();
    let outputs = [sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(1),
        size: sophia_protocol::Size {
            width: 64,
            height: 64,
        },
        scale: 1,
    }];
    let mut runtime =
        sophia_backend_live::LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let now = Instant::now();
    // One refused start keeps the base interval.
    assert!(owner.start_next(now, |_| true).is_err());
    let first = owner.attempt(0).unwrap();
    owner.poll(1024).unwrap();
    owner.settle_revocations(Some(&mut runtime)).unwrap();
    // A stop after a service failure is the second consecutive failure, so
    // the spacing widens here rather than on the next refused start.
    assert!(owner.record_service_failure(0, now).unwrap());
    assert_eq!(
        owner
            .start_next(now + Duration::from_secs(1), |_| true)
            .unwrap(),
        None,
        "the slot is spaced beyond the base interval"
    );
    assert!(
        owner
            .start_next(now + Duration::from_secs(2), |_| true)
            .is_err()
    );
    let second = owner.attempt(0).unwrap();
    assert_ne!(second, first);
    owner.poll(1024).unwrap();
    owner.settle_revocations(Some(&mut runtime)).unwrap();
    // Three failures in a row: four seconds from the last attempt.
    assert_eq!(
        owner
            .start_next(now + Duration::from_secs(5), |_| true)
            .unwrap(),
        None
    );
    assert!(
        owner
            .start_next(now + Duration::from_secs(6), |_| true)
            .is_err()
    );
    assert_ne!(owner.attempt(0), Some(second));
    owner.request_shutdown().unwrap();
    owner.poll(1024).unwrap();
    owner.settle_revocations(Some(&mut runtime)).unwrap();
    assert!(owner.finish_after_backend_drop(()).unwrap().1.quiescent());
    drop(owner);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "native-session")]
#[test]
fn selected_component_launches_bind_only_their_socket_and_config() {
    use sophia_session::shell_component_launch::ShellComponentLaunch;
    let root = std::env::temp_dir().join(format!("component-launch-plan-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut owner = ShellComponentProcesses::new().unwrap();
    let selections = [
        ("bar", ShellComponentRole::Bar),
        ("menu", ShellComponentRole::ApplicationLauncher),
        ("dock", ShellComponentRole::Dock),
    ];
    for (name, role) in selections {
        owner
            .add_with_transport(
                name,
                role,
                &root.join(name),
                rustix::process::geteuid().as_raw(),
                sophia_config::ShellTransportSelection::NineP2000L,
            )
            .unwrap();
    }
    for (slot, (name, role)) in selections.into_iter().enumerate() {
        let config = root.join(format!("{name}.kdl"));
        std::fs::write(&config, "// fixture").unwrap();
        let directory = root.join(name);

        let plan = ShellComponentLaunch::new(
            sophia_config::ShellComponentConfig {
                id: name.into(),
                role,
                executable: "/bin/true".into(),
                config: Some(config.clone()),
                reservation: (role == ShellComponentRole::Dock).then_some(
                    sophia_config::ShellComponentReservation {
                        edge: sophia_config::ShellComponentEdge::Bottom,
                        max_thickness: 64,
                    },
                ),
                gpu: sophia_config::ShellGpuMode::Denied,
                transport: sophia_config::ShellTransportSelection::NineP2000L,
            },
            Some(28),
            None,
            std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
        )
        .unwrap();
        // Reserve through the real process owner; inspect its actual closure
        // inputs and production plan, then intentionally refuse before spawn.
        let result =
            owner.start(
                slot,
                |key, socket| {
                    let (spec, gpu) = plan.prepare(key, socket).unwrap();
                    assert!(gpu.is_none());
                    assert_eq!(spec.args, [std::ffi::OsString::from("--serve")]);
                    assert!(spec.process_group);
                    assert!(
                        spec.environment
                            .iter()
                            .any(|(k, v)| k == "SOPHIA_SHELL_9P_SOCKET" && v == socket.as_os_str())
                    );
                    assert!(
                        spec.environment
                            .iter()
                            .any(|(k, v)| k == "SOPHIA_SHELL_CONFIG" && v == config.as_os_str())
                    );
                    assert_eq!(
                        spec.environment
                            .iter()
                            .any(|(k, _)| k == "SOPHIA_SHELL_BAR_THICKNESS"),
                        role != ShellComponentRole::ApplicationLauncher
                    );
                    if role == ShellComponentRole::Dock {
                        assert!(
                            spec.environment
                                .iter()
                                .any(|(k, v)| k == "SOPHIA_SHELL_BAR_THICKNESS" && v == "64")
                        );
                    }
                    assert!(!spec.environment.iter().any(|(k, _)| k == "DISPLAY"
                        || k == "XAUTHORITY"
                        || k == "WAYLAND_DISPLAY"));
                    let domain = spec.protection_domain.unwrap();
                    assert!(domain.devices().is_empty());
                    assert_eq!(domain.roles().len(), 1);
                    assert!(
                        domain
                            .roles()
                            .contains(&ProtectionDomainRole::MetadataShell)
                    );
                    assert_eq!(domain.paths().len(), 2);
                    assert!(
                        domain
                            .paths()
                            .iter()
                            .all(|p| p.access == sophia_runtime::ProtectionPathAccess::ReadOnly)
                    );
                    assert!(domain.paths().iter().any(|p| p.source == directory));
                    assert!(domain.paths().iter().any(|p| p.source == config));
                    Err("inspected; deliberately no spawn".into())
                },
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: true,
                },
            );
        assert!(result.is_err());
        let key = owner.attempt(slot).unwrap();
        assert!(!owner.process_retained(key));
        assert_eq!(owner.phase(key).unwrap(), ComponentConnectionPhase::Revoked);
    }
    assert!(owner.collect().quiescent());
    drop(owner);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "native-session")]
#[test]
fn a_file_selected_component_is_told_only_its_9p_endpoint() {
    use sophia_session::shell_component_launch::ShellComponentLaunch;
    let root = std::env::temp_dir().join(format!("component-9p-launch-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut owner = ShellComponentProcesses::new().unwrap();
    let slot = owner
        .add_with_transport(
            "bar",
            ShellComponentRole::Bar,
            &root.join("bar"),
            rustix::process::geteuid().as_raw(),
            sophia_config::ShellTransportSelection::NineP2000L,
        )
        .unwrap();
    let plan = ShellComponentLaunch::new(
        sophia_config::ShellComponentConfig {
            id: "bar".into(),
            role: ShellComponentRole::Bar,
            executable: "/bin/true".into(),
            config: None,
            reservation: None,
            gpu: sophia_config::ShellGpuMode::Denied,
            transport: sophia_config::ShellTransportSelection::NineP2000L,
        },
        Some(28),
        None,
        std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
    )
    .unwrap();
    let result = owner.start(
        slot,
        |key, socket| {
            let (spec, _) = plan.prepare(key, socket).unwrap();
            assert!(
                spec.environment
                    .iter()
                    .any(|(k, v)| k == "SOPHIA_SHELL_9P_SOCKET" && v == socket.as_os_str())
            );
            assert!(
                !spec
                    .environment
                    .iter()
                    .any(|(k, _)| k == "SOPHIA_SHELL_SOCKET")
            );
            Err("inspected; deliberately no spawn".into())
        },
        ShellContentAdmissionPolicy::Granted {
            discrete_input: false,
        },
    );
    assert!(result.is_err());
    assert!(owner.collect().quiescent());
    drop(owner);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "native-session")]
#[test]
fn component_launch_refuses_unadmitted_gpu_and_unbounded_panel() {
    use sophia_session::shell_component_launch::ShellComponentLaunch;
    let selection = sophia_config::ShellComponentConfig {
        id: "bar".into(),
        role: ShellComponentRole::Bar,
        executable: "/bin/true".into(),
        config: None,
        reservation: None,
        gpu: sophia_config::ShellGpuMode::Denied,
        transport: sophia_config::ShellTransportSelection::NineP2000L,
    };
    for thickness in [None, Some(0)] {
        assert!(
            ShellComponentLaunch::new(
                selection.clone(),
                thickness,
                None,
                std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH)
            )
            .is_err()
        );
    }
    let mut direct = selection.clone();
    direct.gpu = sophia_config::ShellGpuMode::Direct;
    assert!(
        ShellComponentLaunch::new(
            direct,
            Some(28),
            None,
            std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH)
        )
        .is_err()
    );
    let plan = ShellComponentLaunch::new(
        selection,
        Some(28),
        None,
        std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
    )
    .unwrap();
    assert!(
        plan.prepare(
            sophia_session::shell_component_connections::ComponentConnectionKey {
                slot: 0,
                grant: sophia_protocol::ContentGrant::default(),
            },
            std::path::Path::new("/tmp/not-a-live-socket/shell.sock")
        )
        .is_err()
    );
}

#[cfg(feature = "native-session")]
#[test]
fn joined_session_retains_failed_attempt_until_reap_and_exact_cleanup() {
    use sophia_backend_live::LiveProductionVisualRuntime;
    use sophia_engine::HeadlessOutput;
    use sophia_protocol::{OutputId, Size};
    use sophia_session::shell_component_session::ShellComponentSession;
    use std::sync::Arc;
    let root = std::env::temp_dir().join(format!("joined-components-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let selection = sophia_config::ShellComponentConfig {
        id: "menu".into(),
        role: ShellComponentRole::ApplicationLauncher,
        executable: "/nonexistent-sophia-native-launcher".into(),
        config: None,
        reservation: None,
        gpu: sophia_config::ShellGpuMode::Denied,
        transport: sophia_config::ShellTransportSelection::NineP2000L,
    };
    let mut owner = ShellComponentSession::prepare(
        &[selection],
        28,
        None,
        &root,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
        std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
    )
    .unwrap();
    assert!(owner.start(0).is_err());
    assert_eq!(
        owner.attempt(0),
        None,
        "paused refusal must not reserve or spawn"
    );
    owner.set_presentation_available(true).unwrap();
    assert!(owner.start(0).is_err());
    let first = owner.attempt(0).unwrap();
    assert!(owner.process_retained(first));
    assert_eq!(
        owner.phase(first).unwrap(),
        ComponentConnectionPhase::Revoked
    );
    assert_eq!(owner.pending_revocations(), 1);
    assert!(owner.start(0).is_err());
    assert_eq!(owner.attempt(0), Some(first));
    assert!(
        owner
            .with_service(first, |_, _| panic!("unnegotiated service escaped"))
            .is_err()
    );
    let payload = Arc::new(vec![1u8]);
    let held = owner
        .finish_after_backend_drop(payload.clone())
        .unwrap_err();
    assert_eq!(Arc::strong_count(&payload), 2);
    let visit = owner.poll(1024).unwrap();
    assert!(visit.processes.iter().flatten().any(|event| matches!(event,
        ComponentProcessEvent::ProcessRetired(key, _) if *key == first)));
    assert!(!owner.process_retained(first));
    assert_eq!(owner.settle_revocations(None).unwrap(), 0);
    assert_eq!(owner.pending_revocations(), 1);
    assert!(
        owner.start(0).is_err(),
        "process exit does not settle runtime claims"
    );
    assert_eq!(owner.attempt(0), Some(first));
    let outputs = [HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 64,
            height: 64,
        },
        scale: 1,
    }];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    assert_eq!(owner.settle_revocations(Some(&mut runtime)).unwrap(), 1);
    assert_eq!(owner.settle_revocations(Some(&mut runtime)).unwrap(), 0);
    assert!(owner.start(0).is_err()); // new attempt, still deliberately missing binary
    let second = owner.attempt(0).unwrap();
    assert_ne!(first.grant, second.grant);
    assert!(owner.stop(first).is_err());
    owner.request_shutdown().unwrap();
    owner.set_presentation_available(true).unwrap();
    assert!(owner.start(0).is_err(), "shutdown cannot be reopened");
    owner.poll(1024).unwrap();
    assert_eq!(owner.settle_revocations(Some(&mut runtime)).unwrap(), 1);
    let (_, accounting) = owner.finish_after_backend_drop(held).unwrap();
    assert!(accounting.quiescent());
    assert_eq!(Arc::strong_count(&payload), 1);
    drop(owner);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "native-session")]
#[test]
fn component_scheduler_skips_unready_role_and_bounds_retries() {
    use sophia_session::shell_component_session::ShellComponentSession;
    use std::time::{Duration, Instant};
    let root = std::env::temp_dir().join(format!("component-schedule-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let selections = [
        ("menu", ShellComponentRole::ApplicationLauncher),
        ("bar", ShellComponentRole::Bar),
    ]
    .map(|(id, role)| sophia_config::ShellComponentConfig {
        id: id.into(),
        role,
        executable: "/nonexistent-sophia-component".into(),
        config: None,
        reservation: None,
        gpu: sophia_config::ShellGpuMode::Denied,
        transport: sophia_config::ShellTransportSelection::NineP2000L,
    });
    let mut owner = ShellComponentSession::prepare(
        &selections,
        28,
        None,
        &root,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
        std::path::Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
    )
    .unwrap();
    let now = Instant::now();
    assert_eq!(owner.start_next(now, |_| true).unwrap(), None);
    assert_eq!(owner.connected_roles(), [None, None, None]);
    owner.set_presentation_available(true).unwrap();
    assert!(
        owner
            .start_next(now, |role| role == ShellComponentRole::Bar)
            .is_err()
    );
    assert_eq!(owner.attempt(0), None);
    let bar = owner.attempt(1).unwrap();
    assert_eq!(owner.start_next(now, |_| true).unwrap(), None);
    owner.poll(1024).unwrap();
    let outputs = [sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(1),
        size: sophia_protocol::Size {
            width: 64,
            height: 64,
        },
        scale: 1,
    }];
    let mut runtime =
        sophia_backend_live::LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    owner.settle_revocations(Some(&mut runtime)).unwrap();
    assert!(owner.start_next(now, |_| true).is_err());
    let menu = owner.attempt(0).unwrap();
    assert_eq!(owner.attempt(1), Some(bar));
    owner.poll(1024).unwrap();
    owner.settle_revocations(Some(&mut runtime)).unwrap();
    assert_eq!(owner.start_next(now, |_| true).unwrap(), None);
    assert_eq!(owner.attempt(0), Some(menu));
    assert!(
        owner
            .start_next(now - Duration::from_millis(1), |_| true)
            .is_err()
    );
    assert_eq!(owner.attempt(1), Some(bar));
    assert!(
        owner
            .start_next(now + Duration::from_secs(2), |_| true)
            .is_err()
    );
    assert_ne!(owner.attempt(1), Some(bar));
    assert_eq!(owner.attempt(0), Some(menu));
    owner.request_shutdown().unwrap();
    owner.poll(1024).unwrap();
    owner.settle_revocations(Some(&mut runtime)).unwrap();
    assert_eq!(
        owner
            .start_next(now + Duration::from_secs(4), |_| true)
            .unwrap(),
        None
    );
    assert!(owner.finish_after_backend_drop(()).unwrap().1.quiescent());
    drop(owner);
    std::fs::remove_dir_all(root).unwrap();
}
