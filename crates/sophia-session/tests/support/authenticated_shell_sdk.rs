//! A protected generic C SDK peer through the production component owner.
use super::*;
use std::os::unix::fs::DirBuilderExt;

use crate::live_session::authenticated_shell_peer::Peer;

#[test]
fn protected_shell_sdk_authenticates_all_component_roles_and_replacements() {
    exercise(false);
}

#[test]
fn protected_rust_shell_sdk_authenticates_all_component_roles_and_replacements() {
    exercise(true);
}

fn exercise(rust: bool) {
    let peer = if rust {
        Peer::rust("components")
    } else {
        Peer::build("components")
    };
    for (name, role, audience) in [
        ("bar", ShellComponentRole::Bar, 4),
        ("launcher", ShellComponentRole::ApplicationLauncher, 6),
        ("dock", ShellComponentRole::Dock, 5),
    ] {
        let mut auth = factotum::Fixture::new();
        let directory =
            std::env::temp_dir().join(format!("shell-sdk-{rust}-{name}-{}", std::process::id()));
        let reports = directory.join("reports");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&reports)
            .unwrap();
        let mut owner = ShellComponentProcesses::new().unwrap();
        let slot = owner
            .add(
                name,
                role,
                &directory.join("endpoint"),
                rustix::process::geteuid().as_raw(),
            )
            .unwrap();
        owner
            .authenticate(
                slot,
                LaunchPolicy {
                    principal: audience,
                    namespace: 2,
                    audience,
                    role: ProtectionDomainRole::MetadataShell,
                },
                name,
                auth.coordinator.endpoint(),
            )
            .unwrap();
        let wake = sophia_wake::Wake::new().unwrap();
        let mut previous = None;
        for attempt in 0..2 {
            let report = reports.join(attempt.to_string());
            let key = owner
                .start(
                    slot,
                    |_, socket| {
                        let mut spec = ProcessLaunchSpec::new(&peer.0)
                            .arg(name)
                            .arg(&report)
                            .protection_domain(
                                ProtectionDomainSpec::bubblewrap([
                                    ProtectionDomainRole::MetadataShell,
                                ])
                                .unwrap()
                                .path(ProtectionPath::read_only(socket.parent().unwrap()))
                                .unwrap()
                                .path(ProtectionPath::read_write(&reports))
                                .unwrap(),
                            );
                        if rust {
                            let executable = std::env::current_exe().unwrap();
                            spec.args = vec![
                                executable.clone().into_os_string(),
                                socket.as_os_str().to_owned(),
                                report.clone().into_os_string(),
                            ];
                            spec = spec.env("SOPHIA_TEST_SHELL_ROLE", name);
                            spec.protection_domain = Some(
                                spec.protection_domain
                                    .take()
                                    .unwrap()
                                    .path(ProtectionPath::read_only(executable))
                                    .unwrap(),
                            );
                        }
                        Ok(spec)
                    },
                    ShellContentAdmissionPolicy::Granted {
                        discrete_input: true,
                    },
                )
                .unwrap();
            if let Some(old) = previous {
                assert_ne!(key.grant, old, "replacement needs a fresh shell epoch");
            }
            let deadline = Instant::now() + Duration::from_secs(15);
            while !report.exists() {
                auth.tick();
                let visit = owner.visit_authenticated(
                    65536,
                    &mut auth.coordinator,
                    wake.notifier(),
                    Instant::now(),
                );
                assert!(
                    visit
                        .negotiations
                        .iter()
                        .flatten()
                        .all(|(_, result)| result.is_ok()),
                    "shell negotiation failed: {:?}",
                    visit.negotiations
                );
                let failures = owner.turn_connected();
                assert!(
                    failures.iter().all(Option::is_none),
                    "shell turn failed: {failures:?}"
                );
                assert!(
                    visit.processes.iter().flatten().all(|event| !matches!(
                        event,
                        ComponentProcessEvent::Failed(..)
                            | ComponentProcessEvent::ProcessRetired(..)
                    )),
                    "authenticated shell bootstrap failed ({name}, attempt {attempt}): {:?}; {}",
                    visit.processes,
                    std::fs::read_to_string(report.with_extension("stderr")).unwrap_or_default()
                );
                assert!(
                    Instant::now() < deadline,
                    "authenticated shell readiness deadline"
                );
            }
            let observed = std::fs::read_to_string(&report).unwrap();
            assert_eq!(
                observed,
                format!("{name} ready {}\n", key.grant.connection_epoch),
                "SDK must finish negotiation for this exact shell epoch"
            );
            assert_eq!(
                owner.phase(key).unwrap(),
                ComponentConnectionPhase::Connected
            );
            let registration = owner.slots[slot]
                .authentication
                .as_ref()
                .unwrap()
                .registration
                .unwrap();
            assert_eq!(auth.coordinator.phase(registration), Some(Phase::Active));
            auth.coordinator
                .revoke(registration, Instant::now())
                .unwrap();
            owner.visit_authenticated(
                65536,
                &mut auth.coordinator,
                wake.notifier(),
                Instant::now(),
            );
            assert_eq!(
                owner.phase(key).unwrap(),
                ComponentConnectionPhase::Revoked,
                "authenticated shell revocation must close the admitted connection locally"
            );
            while owner.process_retained(key) {
                auth.tick();
                owner.visit_authenticated(
                    65536,
                    &mut auth.coordinator,
                    wake.notifier(),
                    Instant::now(),
                );
                assert!(
                    Instant::now() < deadline,
                    "authenticated shell reaping deadline"
                );
            }
            previous = Some(key.grant);
        }
        drop(owner);
        let _ = std::fs::remove_dir_all(directory);
    }
}
