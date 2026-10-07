//! t294: an owner step with a provider that ignores TERM. Replacing it asks
//! it to exit and returns at once, keeping the process in custody and its
//! successor owed. The successor starts only after the process is killed at
//! the end of its grace, reaped, and the service's retirement marker seen,
//! under the next grant epoch.
#![cfg(test)]
use std::time::{Duration, Instant};

use sophia_protocol::lock_files::{LockAllocation, LockFileLimits, LockObject, LockPhase};
use sophia_runtime::ProcessLaunchSpec;

use super::*;
use crate::session_lock_succession::LockProviderStart;

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
fn a_provider_ignoring_term_does_not_hold_the_owner_step() {
    let directory =
        std::env::temp_dir().join(format!("lock-provider-succession-{}", std::process::id()));
    let ready = directory.with_extension("ready");
    let _ = std::fs::remove_file(&ready);
    let transport = LockFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        1,
        limits(),
    )
    .unwrap();
    let base = ProcessLaunchSpec::new("/bin/sh")
        .arg("-c")
        .arg("trap '' TERM; echo ready > \"$1\"; while :; do sleep 1; done")
        .arg("fixture")
        .arg(&ready)
        .process_group();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut provider = LockProvider::with_service(
        transport,
        base,
        ShellGpuMode::Denied,
        None,
        locked(),
        Vec::new(),
        wake.notifier(),
    )
    .unwrap();
    provider.poll(Instant::now());
    let first = provider.supervisor.child_id().expect("the first launch");
    assert_eq!(provider.grant_epoch, 1);
    let deadline = Instant::now() + 2 * GRACE;
    while !ready.exists() {
        assert!(Instant::now() < deadline, "the process never trapped TERM");
        std::thread::sleep(Duration::from_millis(1));
    }

    let step = Instant::now();
    provider.replace_process(Instant::now());
    assert!(provider.poll(Instant::now()).is_empty());
    assert!(
        step.elapsed() < GRACE / 2,
        "the step waited for the process"
    );
    assert_eq!(
        provider.supervisor.child_id(),
        Some(first),
        "still in custody"
    );
    // The marker may already have arrived; the reap cannot have.
    assert_eq!(provider.succession.start(true), LockProviderStart::Wait);
    assert_eq!(provider.grant_epoch, 1, "no successor yet");
    assert!(provider.restart_at.is_some(), "the successor is still owed");

    let deadline = Instant::now() + 3 * GRACE;
    while provider.grant_epoch < 2 {
        assert!(Instant::now() < deadline, "the provider was never replaced");
        let step = Instant::now();
        provider.poll(Instant::now());
        assert!(step.elapsed() < GRACE / 2, "a step waited for the process");
        std::thread::sleep(Duration::from_millis(10));
    }
    let second = provider.supervisor.child_id().expect("the successor");
    assert_ne!(second, first);
    assert!(provider.succession.admits_provider_events());
    drop(provider);
    let _ = std::fs::remove_file(ready);
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
    )
    .unwrap()
}

/// A service that reports failure, first or last in a batch and with its
/// queue still open, ends the provider at once: nothing of the batch goes on,
/// and Session is told once to revoke its grants.
#[test]
fn a_reported_service_failure_is_terminal_and_discards_its_batch() {
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
        assert!(provider.admit_events(batch).is_empty(), "failure at {at}");
        assert!(provider.take_failure());
        assert!(!provider.take_failure(), "reported once");
        assert!(provider.admit_events(vec![connected()]).is_empty());
        assert!(provider.poll(Instant::now()).is_empty());
        assert!(provider.supervisor.child_id().is_none(), "nothing launched");
    }
    // A closed queue is the same.
    let mut provider = idle_provider("closed");
    assert!(provider.admit_events(vec![connected(), Err(())]).is_empty());
    assert!(provider.take_failure());
}
