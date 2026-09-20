#![cfg(test)]

//! Retry spacing across the failures a slot can have: refused starts, stops
//! after a service failure, and the healthy tenure that starts the count
//! afresh. The public scheduler contract is exercised in
//! `tests/shell_component_processes.rs`; these reach the count itself.

use super::*;
use std::time::{Duration, Instant};

fn session(root: &Path) -> ShellComponentSession {
    let selection = ShellComponentConfig {
        id: "bar".into(),
        role: ShellComponentRole::Bar,
        executable: "/nonexistent-sophia-component".into(),
        config: None,
        reservation: None,
        gpu: ShellGpuMode::Denied,
    };
    let mut owner = ShellComponentSession::prepare(
        &[selection],
        28,
        None,
        root,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
    )
    .unwrap();
    owner.set_presentation_available(true).unwrap();
    owner
}

fn retire(mut owner: ShellComponentSession, root: &Path) {
    let outputs = [sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(1),
        size: sophia_protocol::Size {
            width: 64,
            height: 64,
        },
        scale: 1,
    }];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    owner.request_shutdown().unwrap();
    owner.poll(1024).unwrap();
    owner.settle_revocations(Some(&mut runtime)).unwrap();
    assert!(owner.finish_after_backend_drop(()).unwrap().1.quiescent());
    drop(owner);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_service_failure_counts_like_a_refused_start() {
    let root =
        std::env::temp_dir().join(format!("component-service-failure-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut owner = session(&root);
    let now = Instant::now();

    // A refused start is the first failure, at the base interval.
    assert!(owner.start_next(now, |_| true).is_err());
    assert_eq!(owner.failures[0], 1);
    assert_eq!(owner.retry_at[0], Some(now + RETRY_BASE));
    assert_eq!(owner.entered_backoff, None);

    // The component then stopped after failing in service: the second
    // failure, and the one at which the spacing first widens.
    assert!(owner.record_service_failure(0, now).unwrap());
    assert_eq!(owner.failures[0], 2);
    assert_eq!(owner.retry_at[0], Some(now + 2 * RETRY_BASE));

    // A third is spaced further and is not the transition again.
    assert!(!owner.record_service_failure(0, now).unwrap());
    assert_eq!(owner.failures[0], 3);
    assert_eq!(owner.retry_at[0], Some(now + 4 * RETRY_BASE));

    assert!(owner.record_service_failure(1, now).is_err());
    retire(owner, &root);
}

#[test]
fn a_healthy_tenure_starts_the_count_afresh() {
    let root =
        std::env::temp_dir().join(format!("component-healthy-tenure-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut owner = session(&root);
    let now = Instant::now();

    // An old run of failures, then a start that succeeded at `now`.
    owner.failures[0] = 5;
    owner.started_at[0] = Some(now);
    // Failing within the tenure continues the run.
    let within = now + HEALTHY_TENURE - Duration::from_millis(1);
    assert!(!owner.record_service_failure(0, within).unwrap());
    assert_eq!(owner.failures[0], 6);
    assert_eq!(owner.started_at[0], None);

    // Failing after a healthy tenure is the first failure of a new run.
    owner.started_at[0] = Some(now);
    let later = now + HEALTHY_TENURE;
    assert!(!owner.record_service_failure(0, later).unwrap());
    assert_eq!(owner.failures[0], 1);
    assert_eq!(owner.retry_at[0], Some(later + RETRY_BASE));

    // A refused start after a healthy tenure counts the same way, and does
    // not report a backoff transition it did not make.
    let due = later + RETRY_BASE;
    owner.failures[0] = 5;
    owner.started_at[0] = Some(now);
    assert!(owner.start_next(due, |_| true).is_err());
    assert_eq!(owner.failures[0], 1);
    assert_eq!(owner.entered_backoff, None);
    assert_eq!(owner.retry_at[0], Some(due + RETRY_BASE));
    retire(owner, &root);
}
