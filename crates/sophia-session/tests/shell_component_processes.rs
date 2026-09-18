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
