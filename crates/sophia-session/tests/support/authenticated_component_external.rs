//! Caller-built component conformance without devices or client policy.
use super::*;

#[test]
#[ignore = "requires explicit prebuilt component, role, and exit/revoke mode"]
fn supplied_component_client_authenticates_across_launches() {
    let executable = std::env::var("SOPHIA_COMPONENT_AUTH_PEER").unwrap();
    assert!(Path::new(&executable).is_absolute() && Path::new(&executable).is_file());
    let name = std::env::var("SOPHIA_COMPONENT_AUTH_ROLE").unwrap();
    let (role, audience) = match name.as_str() {
        "bar" => (ShellComponentRole::Bar, 4),
        "dock" => (ShellComponentRole::Dock, 5),
        "launcher" => (ShellComponentRole::ApplicationLauncher, 6),
        _ => panic!("unsupported component role"),
    };
    let mode = std::env::var("SOPHIA_COMPONENT_AUTH_END").unwrap();
    assert!(matches!(mode.as_str(), "exit" | "revoke"));
    let config = std::env::var_os("SOPHIA_COMPONENT_AUTH_CONFIG").map(std::path::PathBuf::from);
    if let Some(path) = &config {
        assert!(path.is_absolute() && path.is_file());
    }
    let directory = std::env::temp_dir().join(format!("component-external-{}", std::process::id()));
    let mut auth = factotum::Fixture::new();
    let mut owner = ShellComponentProcesses::new().unwrap();
    let slot = owner
        .add(&name, role, &directory, rustix::process::geteuid().as_raw())
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
            &name,
            auth.coordinator.endpoint(),
        )
        .unwrap();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut previous = None;
    let mut previous_registration = None;
    for _ in 0..2 {
        let key = owner
            .start(
                slot,
                |_, socket| {
                    let mut domain =
                        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
                            .map_err(|error| error.to_string())?
                            .path(ProtectionPath::read_only(socket.parent().unwrap()))
                            .map_err(|error| error.to_string())?;
                    let mut spec = ProcessLaunchSpec::new(&executable)
                        .arg("--serve")
                        .env("SOPHIA_SHELL_BAR_THICKNESS", "48");
                    if let Some(path) = &config {
                        domain = domain
                            .path(ProtectionPath::read_only(path))
                            .map_err(|error| error.to_string())?;
                        spec = spec.env("SOPHIA_SHELL_CONFIG", path);
                    }
                    Ok(spec.protection_domain(domain))
                },
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: true,
                },
            )
            .unwrap();
        if let Some(old) = previous {
            assert_ne!(
                key.grant, old,
                "replacement must have a fresh component epoch"
            );
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut connected = false;
        let mut revoke_at = None;
        let mut revoked = false;
        let mut registration = None;
        loop {
            auth.tick();
            let visit = owner.visit_authenticated(
                65536,
                &mut auth.coordinator,
                wake.notifier(),
                Instant::now(),
            );
            if !connected {
                assert!(
                    visit
                        .negotiations
                        .iter()
                        .flatten()
                        .all(|(_, result)| result.is_ok()),
                    "supplied component negotiation refused: {:?}",
                    visit.negotiations
                );
            }
            if !connected && owner.phase(key).unwrap() == ComponentConnectionPhase::Connected {
                connected = true;
                let id = owner.slots[slot]
                    .authentication
                    .as_ref()
                    .unwrap()
                    .registration
                    .unwrap();
                assert_eq!(auth.coordinator.phase(id), Some(Phase::Active));
                if let Some(old) = previous_registration {
                    assert_ne!(id, old, "replacement must have fresh launch custody");
                }
                registration = Some(id);
                revoke_at = Some(Instant::now() + Duration::from_millis(250));
                println!(
                    "component_auth role={name} epoch={} phase=negotiated",
                    key.grant.connection_epoch
                );
            }
            if owner.phase(key).unwrap() == ComponentConnectionPhase::Connected {
                // Complete the file handshake after the owner's negotiation.
                let failures = owner.turn_connected();
                if mode == "revoke" && !revoked {
                    assert!(
                        failures.iter().all(Option::is_none),
                        "supplied component turn failed: {failures:?}"
                    );
                }
            }
            if mode == "revoke" && !revoked && revoke_at.is_some_and(|at| Instant::now() >= at) {
                assert_eq!(
                    owner.phase(key).unwrap(),
                    ComponentConnectionPhase::Connected,
                    "supplied component must survive until revocation"
                );
                auth.coordinator
                    .revoke(registration.unwrap(), Instant::now())
                    .unwrap();
                owner.visit_authenticated(
                    65536,
                    &mut auth.coordinator,
                    wake.notifier(),
                    Instant::now(),
                );
                assert_eq!(owner.phase(key).unwrap(), ComponentConnectionPhase::Revoked);
                revoked = true;
            }
            if !owner.process_retained(key) {
                assert!(
                    connected,
                    "supplied component exited before authenticated negotiation"
                );
                assert!(
                    mode == "exit" || revoked,
                    "component exited before requested revocation"
                );
                assert_ne!(
                    owner.phase(key).unwrap(),
                    ComponentConnectionPhase::Connected,
                    "reaped component must not retain connection authority"
                );
                assert_ne!(
                    auth.coordinator.phase(registration.unwrap()),
                    Some(Phase::Active),
                    "reaped launch must not retain active custody"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "supplied component authentication/retirement deadline"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        println!(
            "component_auth role={name} epoch={} phase=reaped end={mode}",
            key.grant.connection_epoch
        );
        previous = Some(key.grant);
        previous_registration = registration;
    }
    drop(owner);
    let _ = std::fs::remove_dir_all(directory);
}
