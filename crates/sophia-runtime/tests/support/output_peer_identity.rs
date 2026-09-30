use super::*;

fn supervisor() -> ProcessSupervisor {
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::OutputAuthority,
        ProcessLaunchSpec::new("/bin/sleep").arg("30"),
    );
    supervisor.start_after(Duration::ZERO).unwrap();
    supervisor
}

#[test]
fn exited_peer_before_identity_capture_is_a_dead_assignment() {
    let mut supervisor = supervisor();
    assert!(supervisor.peer_pidfd().unwrap().is_some());
    let child = &mut supervisor.child.as_mut().unwrap().child;
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(supervisor.peer_pidfd().unwrap().is_none());
}

#[test]
fn protected_identity_must_still_belong_to_the_wrapper() {
    let mut supervisor = supervisor();
    let child = supervisor.child.as_mut().unwrap();
    // A readable pidfd alone is insufficient: this process is not a child
    // of the retained wrapper. Model discovery of a subsequently reused PID.
    child.peer_pid = std::process::id();
    child.protection = Some(ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: child.child.id(),
        peer_pid: child.peer_pid,
        roles: [ProtectionDomainRole::OutputAuthority]
            .into_iter()
            .collect(),
    });
    assert!(supervisor.peer_pidfd().unwrap().is_none());
}
