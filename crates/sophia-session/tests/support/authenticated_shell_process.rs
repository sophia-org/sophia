//! Production component launch custody with a real Factotum owner. The child
//! intentionally sends no shell traffic: these are pre-negotiation lifetimes.
use super::*;
use crate::live_session::tests::lock_provider_factotum as factotum;

#[path = "authenticated_shell_sdk.rs"]
mod sdk;

#[path = "authenticated_component_external.rs"]
mod external;

fn owner(launches: &LaunchCoordinator) -> (ShellComponentProcesses, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-launch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut owner = ShellComponentProcesses::new().unwrap();
    let slot = owner
        .add(
            "bar",
            ShellComponentRole::Bar,
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
    owner
        .authenticate(
            slot,
            LaunchPolicy {
                principal: 4,
                namespace: 2,
                audience: 4,
                role: ProtectionDomainRole::MetadataShell,
            },
            "bar",
            launches.endpoint(),
        )
        .unwrap();
    (owner, directory)
}

fn start(owner: &mut ShellComponentProcesses) -> ComponentConnectionKey {
    owner
        .start(
            0,
            |_, _| {
                Ok(ProcessLaunchSpec::new("/usr/bin/sleep")
                    .arg("20")
                    .protection_domain(
                        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
                            .unwrap(),
                    ))
            },
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap()
}

#[test]
fn authenticated_shell_never_spawns_without_factotum_and_cancellation_reclaims_the_attempt() {
    let mut launches = LaunchCoordinator::new(Default::default(), factotum::namespaces());
    let (mut owner, _) = owner(&launches);
    let wake = sophia_wake::Wake::new().unwrap();
    let key = start(&mut owner);
    assert!(owner.process_retained(key));
    assert!(
        owner.slots[0].process.is_none(),
        "unverified shell must not spawn"
    );
    assert_eq!(
        owner.phase(key).unwrap(),
        ComponentConnectionPhase::Reserved
    );
    let visit = owner.visit_authenticated(65536, &mut launches, wake.notifier(), Instant::now());
    assert!(visit.processes.iter().all(Option::is_none));
    assert!(
        owner.slots[0].process.is_none(),
        "unverified shell must not spawn"
    );
    owner.request_stop(key).unwrap();
    assert!(
        owner.process_retained(key),
        "cancel retains pending launch until owner cleanup"
    );
    owner.visit_authenticated(65536, &mut launches, wake.notifier(), Instant::now());
    assert!(!owner.process_retained(key));
    assert_eq!(owner.phase(key).unwrap(), ComponentConnectionPhase::Revoked);
    let next = start(&mut owner);
    assert_ne!(
        next.grant, key.grant,
        "cancelled shell epoch cannot be reused"
    );
    owner.request_stop(next).unwrap();
    owner.visit_authenticated(65536, &mut launches, wake.notifier(), Instant::now());
    assert!(!owner.process_retained(next));
}

#[test]
fn authenticated_shell_revocation_reaps_the_exact_launch_before_replacement() {
    let mut auth = factotum::Fixture::new();
    let (mut owner, _) = owner(&auth.coordinator);
    let wake = sophia_wake::Wake::new().unwrap();
    let key = start(&mut owner);
    let deadline = Instant::now() + Duration::from_secs(10);
    while owner.slots[0].process.is_none() {
        auth.tick();
        let visit = owner.visit_authenticated(
            65536,
            &mut auth.coordinator,
            wake.notifier(),
            Instant::now(),
        );
        assert!(
            visit.processes.iter().all(Option::is_none),
            "protected shell launch failed: {:?}",
            visit.processes
        );
        assert!(Instant::now() < deadline, "protected shell launch deadline");
    }
    let registration = owner.slots[0]
        .authentication
        .as_ref()
        .unwrap()
        .registration
        .unwrap();
    assert_eq!(auth.coordinator.phase(registration), Some(Phase::Active));
    assert_eq!(
        owner.phase(key).unwrap(),
        ComponentConnectionPhase::Negotiating
    );
    assert!(
        owner.slots[0]
            .process
            .as_ref()
            .unwrap()
            .child_id()
            .is_some()
    );
    auth.coordinator
        .revoke(registration, Instant::now())
        .unwrap();
    let revoked = owner.visit_authenticated(
        65536,
        &mut auth.coordinator,
        wake.notifier(),
        Instant::now(),
    );
    assert!(revoked.processes.iter().flatten().any(|event| matches!(event,
        ComponentProcessEvent::Failed(k, _) | ComponentProcessEvent::ProcessRetired(k, _) if *k == key)),
        "launch revocation must notify presentation cleanup before reaping");
    assert_eq!(
        owner.phase(key).unwrap(),
        ComponentConnectionPhase::Revoked,
        "revoked launch must close shell negotiation immediately"
    );
    while owner.process_retained(key) {
        auth.tick();
        owner.visit_authenticated(
            65536,
            &mut auth.coordinator,
            wake.notifier(),
            Instant::now(),
        );
        assert!(Instant::now() < deadline, "revoked shell must be reaped");
    }
    assert_eq!(owner.phase(key).unwrap(), ComponentConnectionPhase::Revoked);
    assert!(
        owner.slots[0]
            .authentication
            .as_ref()
            .unwrap()
            .lease
            .is_none()
    );
    let next = start(&mut owner);
    assert_ne!(key.grant, next.grant);
    owner.request_stop(next).unwrap();
    owner.visit_authenticated(
        65536,
        &mut auth.coordinator,
        wake.notifier(),
        Instant::now(),
    );
    assert!(!owner.process_retained(next));
}
