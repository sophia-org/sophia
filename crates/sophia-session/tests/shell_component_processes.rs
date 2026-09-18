//! No live display/device. Preparation and failed-spawn process custody controls;
//! successful protected dual-peer negotiation is a separate integration gate.
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
            .add(
                name,
                role,
                &directory.join(name),
                rustix::process::geteuid().as_raw(),
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

#[test]
#[ignore = "requires nested user namespaces; run explicitly inside the device-hidden fixture"]
fn two_protected_processes_retain_independent_stop_and_replacement_custody() {
    use sophia_runtime::ProtectionPath;
    use std::time::{Duration, Instant};
    let directory = std::env::temp_dir().join(format!(
        "protected-component-processes-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let mut owner = ShellComponentProcesses::new().unwrap();
    for (name, role) in [
        ("bar", ShellComponentRole::Bar),
        ("menu", ShellComponentRole::ApplicationLauncher),
    ] {
        owner
            .add(
                name,
                role,
                &directory.join(name),
                rustix::process::geteuid().as_raw(),
            )
            .unwrap();
    }
    let prepare = |key: sophia_session::shell_component_connections::ComponentConnectionKey,
                   socket: &std::path::Path| {
        let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
            .map_err(|e| e.to_string())?
            .path(ProtectionPath::read_only(socket.parent().unwrap()))
            .map_err(|e| e.to_string())?;
        Ok(ProcessLaunchSpec::new(std::env::current_exe().unwrap())
            .arg("protected_component_peer")
            .arg("--exact")
            .arg("--ignored")
            .env("SOPHIA_FIXTURE_ROLE", key.slot.to_string())
            .protection_domain(domain))
    };
    let policy = ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    };
    let bar = owner.start(0, prepare, policy).unwrap();
    let menu = owner.start(1, prepare, policy).unwrap();
    assert_ne!(bar.grant, menu.grant);
    assert!(owner.process_retained(bar) && owner.process_retained(menu));
    let bar_pixels = peer::receive_resource(&mut owner, bar);
    let menu_pixels = peer::receive_resource(&mut owner, menu);
    assert_eq!(bar_pixels.bytes(), &[1, 2, 3, 255]);
    owner.request_stop(menu).unwrap();
    assert!(owner.start(1, prepare, policy).is_err());
    let deadline = Instant::now() + Duration::from_secs(3);
    while owner.process_retained(menu) {
        owner.visit(1024);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(owner.process_retained(bar));
    assert_eq!(
        owner.phase(bar).unwrap(),
        ComponentConnectionPhase::Connected
    );
    // Live bar + full launcher reservation fill the aggregate allowance.
    // Retained old pixels refuse a reconnect rather than being freed early.
    assert!(owner.start(1, prepare, policy).is_err());
    assert_eq!(menu_pixels.bytes(), &[1, 2, 3, 255]);
    assert!(owner.process_retained(bar));
    drop(menu_pixels);
    owner.collect();
    let replacement = owner.start(1, prepare, policy).unwrap();
    assert_ne!(replacement.grant, menu.grant);
    let replacement_pixels = peer::receive_resource(&mut owner, replacement);
    assert_eq!(replacement_pixels.bytes(), &[1, 2, 3, 255]);
    assert!(owner.request_stop(menu).is_err());
    for _ in 0..10 {
        owner.visit(1024);
        assert!(owner.process_retained(replacement));
        assert!(owner.process_retained(bar));
        std::thread::sleep(Duration::from_millis(1));
    }
    owner.request_stop(bar).unwrap();
    owner.request_stop(replacement).unwrap();
    while owner.process_retained(bar) || owner.process_retained(replacement) {
        owner.visit(1024);
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    drop((bar_pixels, replacement_pixels));
    assert!(owner.finish_after_backend_drop(()).unwrap().1.quiescent());
    drop(owner);
    std::fs::remove_dir_all(directory).unwrap();
}

#[path = "support/component_processes/peer.rs"]
mod peer;
#[test]
#[ignore = "child entry invoked only by protected parent fixture"]
fn protected_component_peer() {
    peer::run();
}

#[cfg(feature = "native-session")]
#[test]
fn selected_component_launches_bind_only_their_socket_and_config() {
    use sophia_session::shell_component_launch::ShellComponentLaunch;
    let root = std::env::temp_dir().join(format!("component-launch-plan-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut owner = ShellComponentProcesses::new().unwrap();
    for (name, role) in [
        ("bar", ShellComponentRole::Bar),
        ("menu", ShellComponentRole::ApplicationLauncher),
    ] {
        let config = root.join(format!("{name}.kdl"));
        std::fs::write(&config, "// fixture").unwrap();
        let directory = root.join(name);
        let slot = owner
            .add(name, role, &directory, rustix::process::geteuid().as_raw())
            .unwrap();
        let plan = ShellComponentLaunch::new(
            sophia_config::ShellComponentConfig {
                id: name.into(),
                role,
                executable: "/bin/true".into(),
                config: Some(config.clone()),
                gpu: sophia_config::ShellGpuMode::Denied,
            },
            Some(28),
            None,
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
                            .any(|(k, v)| k == "SOPHIA_SHELL_SOCKET" && v == socket.as_os_str())
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
                        role == ShellComponentRole::Bar
                    );
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
fn component_launch_refuses_unadmitted_gpu_and_unbounded_panel() {
    use sophia_session::shell_component_launch::ShellComponentLaunch;
    let selection = sophia_config::ShellComponentConfig {
        id: "bar".into(),
        role: ShellComponentRole::Bar,
        executable: "/bin/true".into(),
        config: None,
        gpu: sophia_config::ShellGpuMode::Denied,
    };
    for thickness in [None, Some(0)] {
        assert!(ShellComponentLaunch::new(selection.clone(), thickness, None).is_err());
    }
    let mut direct = selection.clone();
    direct.gpu = sophia_config::ShellGpuMode::Direct;
    assert!(ShellComponentLaunch::new(direct, Some(28), None).is_err());
    let plan = ShellComponentLaunch::new(selection, Some(28), None).unwrap();
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
        gpu: sophia_config::ShellGpuMode::Denied,
    };
    let mut owner = ShellComponentSession::prepare(
        &[selection],
        28,
        None,
        &root,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
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
