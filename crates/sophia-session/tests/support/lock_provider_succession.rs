//! Protected provider replacement keeps the owner responsive and waits for
//! reap plus the service's retirement marker before a fresh grant/launch.
//! The application ignores TERM, but Bubblewrap may end its namespace without
//! delivering that signal; this is not a two-second grace-duration claim.
#![cfg(test)]
use std::time::{Duration, Instant};

use sophia_protocol::lock_files::{LockAllocation, LockFileLimits, LockObject, LockPhase};
use sophia_runtime::ProcessLaunchSpec;

use super::*;
use crate::session_lock_succession::LockProviderStart;

#[path = "lock_provider_authentication.rs"]
mod authentication;
use crate::live_session::tests::lock_provider_factotum as factotum;

fn policy() -> LaunchPolicy {
    LaunchPolicy {
        principal: 1,
        namespace: 2,
        audience: 1,
        role: ProtectionDomainRole::LockProvider,
    }
}

/// The supervisor's grace between TERM and KILL. A step that waited for the
/// process would take at least this long.
const GRACE: Duration = Duration::from_secs(2);

fn limits() -> LockFileLimits {
    LockFileLimits {
        max_outputs: 1,
        upload_slots: 1,
        max_chords: 1,
        max_width_px: 16,
        max_height_px: 16,
        max_resource_bytes: 1024,
        max_live_resources: 2,
        journal_records: 32,
        journal_bytes: 8192,
        assembly_timeout_ms: 2000,
        ack_progress_timeout_ms: 2000,
    }
}

fn locked() -> LockObject {
    LockObject {
        lock_epoch: 4,
        topology_generation: 1,
        phase: LockPhase::Locked,
        allocations: vec![LockAllocation {
            output_id: 1,
            output_generation: 1,
            allocation_id: 2,
            allocation_generation: 1,
            pixel_width: 4,
            pixel_height: 4,
            scale_numerator: 1,
            scale_denominator: 1,
        }],
    }
}

#[test]
fn a_protected_provider_replacement_does_not_hold_the_owner_step() {
    let directory =
        std::env::temp_dir().join(format!("lock-provider-succession-{}", std::process::id()));
    let transport = LockFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        1,
        limits(),
    )
    .unwrap();
    let reports = directory.with_extension("reports");
    std::fs::create_dir_all(&reports).unwrap();
    let ready = reports.join("ready");
    let _ = std::fs::remove_file(&ready);
    let base = ProcessLaunchSpec::new("/usr/bin/sh")
        .arg("-c")
        .arg("trap '' TERM; echo ready > \"$1\"; while :; do sleep 1; done")
        .arg("fixture")
        .arg(&ready)
        .process_group()
        .protection_domain(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider])
                .unwrap()
                .path(ProtectionPath::read_write(&reports))
                .unwrap(),
        );
    let mut auth = factotum::Fixture::new();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut provider = LockProvider::with_service(
        transport,
        base,
        ShellGpuMode::Denied,
        None,
        locked(),
        Vec::new(),
        wake.notifier(),
        policy(),
        auth.coordinator.endpoint(),
    )
    .unwrap();
    let deadline = Instant::now() + 2 * GRACE;
    while !ready.exists()
        || provider
            .supervisor
            .as_ref()
            .and_then(ProcessSupervisor::child_id)
            .is_none()
    {
        assert!(Instant::now() < deadline, "the process never trapped TERM");
        auth.tick();
        provider.poll(Instant::now(), &mut auth.coordinator);
    }
    let first = provider.supervisor.as_ref().unwrap().child_id().unwrap();
    assert_eq!(provider.grant_epoch, 1);

    let step = Instant::now();
    provider.replace_process(Instant::now(), &mut auth.coordinator);
    assert!(
        provider
            .poll(Instant::now(), &mut auth.coordinator)
            .is_empty()
    );
    assert!(
        step.elapsed() < GRACE / 2,
        "the step waited for the process"
    );
    // A fast protected exit still owes the service retirement barrier.
    assert_eq!(provider.succession.start(true), LockProviderStart::Wait);
    assert_eq!(provider.grant_epoch, 1, "no successor yet");
    assert!(provider.restart_at.is_some(), "the successor is still owed");

    let deadline = Instant::now() + 3 * GRACE;
    while provider
        .supervisor
        .as_ref()
        .and_then(ProcessSupervisor::child_id)
        .is_none_or(|id| id == first)
    {
        assert!(Instant::now() < deadline, "the provider was never replaced");
        let step = Instant::now();
        auth.tick();
        provider.poll(Instant::now(), &mut auth.coordinator);
        assert!(step.elapsed() < GRACE / 2, "a step waited for the process");
        std::thread::sleep(Duration::from_millis(10));
    }
    let second = provider
        .supervisor
        .as_ref()
        .unwrap()
        .child_id()
        .expect("the successor");
    assert_ne!(second, first);
    assert!(provider.succession.admits_provider_events());
    drop(provider);
    let _ = std::fs::remove_file(ready);
    let _ = std::fs::remove_dir_all(reports);
}

fn idle_provider(label: &str) -> LockProvider {
    let directory =
        std::env::temp_dir().join(format!("lock-provider-{label}-{}", std::process::id()));
    let transport = LockFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        1,
        limits(),
    )
    .unwrap();
    let wake = sophia_wake::Wake::new().unwrap();
    LockProvider::with_service(
        transport,
        ProcessLaunchSpec::new("/bin/true"),
        ShellGpuMode::Denied,
        None,
        locked(),
        Vec::new(),
        wake.notifier(),
        policy(),
        crate::launch_authentication::files::TransportBudget::new(factotum::namespaces())
            .endpoint(),
    )
    .unwrap()
}

/// A service that reports failure, first or last in a batch and with its
/// queue still open, ends the provider at once: nothing of the batch goes on,
/// and Session is told once to revoke its grants.
#[test]
fn a_reported_service_failure_is_terminal_and_discards_its_batch() {
    let mut launches = LaunchCoordinator::new(Default::default(), factotum::namespaces());
    let connected = || {
        Ok(LockFileServiceEvent::Connected {
            connection_epoch: 1,
            chords: Vec::new(),
        })
    };
    let failed = || {
        Ok(LockFileServiceEvent::Failed {
            message: "injected".into(),
        })
    };
    for at in [0, 7] {
        let mut provider = idle_provider(&format!("failed-{at}"));
        let mut batch = (0..7).map(|_| connected()).collect::<Vec<_>>();
        batch.insert(at, failed());
        assert!(
            provider.admit_events(batch, &mut launches).is_empty(),
            "failure at {at}"
        );
        assert!(provider.take_failure());
        assert!(!provider.take_failure(), "reported once");
        assert!(
            provider
                .admit_events(vec![connected()], &mut launches)
                .is_empty()
        );
        assert!(provider.poll(Instant::now(), &mut launches).is_empty());
        assert!(
            provider.supervisor.as_ref().unwrap().child_id().is_none(),
            "nothing launched"
        );
    }
    // A closed queue is the same.
    let mut provider = idle_provider("closed");
    assert!(
        provider
            .admit_events(vec![connected(), Err(())], &mut launches)
            .is_empty()
    );
    assert!(provider.take_failure());
}
