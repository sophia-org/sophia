//! Host custody is an explicit client-only handoff, never protection evidence.
#![cfg(test)]
use super::*;

#[test]
fn host_client_handoff_preserves_environment_but_excludes_other_descriptors_and_roles() {
    let probe = Probe::build();
    let sentinel = std::fs::File::open("/dev/null").unwrap();
    rustix::io::fcntl_setfd(&sentinel, rustix::io::FdFlags::empty()).unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::NamespaceClient,
        ProcessLaunchSpec::new(&probe.0)
            .arg("--host")
            .env("SOPHIA_HOST_PROBE", "preserved"),
    );
    let (mut owner, channel) = LaunchChannel::reserve(1).unwrap();
    let reservation = channel.reservation();
    owner
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    supervisor.start_host_namespace_client(channel).unwrap();
    let mut ready = [0; 6];
    owner.read_exact(&mut ready).unwrap();
    assert_eq!(
        &ready, b"READY\n",
        "host handoff must preserve environment and exclude unrelated descriptors"
    );
    assert!(
        supervisor.protection_evidence().is_none(),
        "host custody must not claim containment"
    );
    assert_eq!(supervisor.child_id(), supervisor.peer_id());
    let custody = supervisor
        .launch_custody(&reservation, ProtectionDomainRole::NamespaceClient)
        .unwrap()
        .unwrap();
    assert!(custody.is_live().unwrap());
    for role in [
        ProtectionDomainRole::SpatialPolicy,
        ProtectionDomainRole::MetadataShell,
        ProtectionDomainRole::LockProvider,
        ProtectionDomainRole::OutputAuthority,
    ] {
        assert!(
            !supervisor.has_live_launch(&reservation, role).unwrap(),
            "host client custody cannot authorize a desktop role"
        );
    }
    let (_, other) = LaunchChannel::reserve(1).unwrap();
    assert!(
        !supervisor
            .has_live_launch(&other.reservation(), ProtectionDomainRole::NamespaceClient)
            .unwrap(),
        "equal numbers cannot replace exact spawn custody"
    );
    owner.write_all(b"Q").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while custody.is_live().unwrap() {
        assert!(
            Instant::now() < deadline,
            "host exit withdraws custody without an owner poll"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    supervisor.terminate().unwrap();
    assert!(supervisor.take_exit_status().unwrap().success());
    assert!(
        !rustix::io::fcntl_getfd(&sentinel)
            .unwrap()
            .contains(rustix::io::FdFlags::CLOEXEC)
    );
}

#[test]
fn host_path_cannot_bypass_a_role_or_required_containment() {
    for process in [
        SupervisedProcessKind::WindowManager,
        SupervisedProcessKind::PortalBroker,
        SupervisedProcessKind::MetadataBroker,
        SupervisedProcessKind::Shell,
        SupervisedProcessKind::OutputAuthority,
        SupervisedProcessKind::SophiaXAuthority,
        SupervisedProcessKind::LockProvider,
    ] {
        let mut supervisor =
            ProcessSupervisor::new(process, ProcessLaunchSpec::new("/usr/bin/true"));
        let (owner, channel) = LaunchChannel::reserve(1).unwrap();
        assert!(matches!(
            supervisor.start_host_namespace_client(channel),
            Err(ProcessSupervisorError::WrongProcess { .. })
        ));
        assert!(supervisor.child_id().is_none());
        eof(owner);
    }
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::NamespaceClient,
        ProcessLaunchSpec::new("/usr/bin/true").protection_domain(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient]).unwrap(),
        ),
    );
    let (owner, channel) = LaunchChannel::reserve(1).unwrap();
    assert_eq!(
        supervisor.start_host_namespace_client(channel),
        Err(ProcessSupervisorError::HostLaunchDomainConflict)
    );
    eof(owner);
    let (owner, channel) = LaunchChannel::reserve(1).unwrap();
    assert_eq!(
        supervisor.start_with_launch_channel(channel),
        Err(ProcessSupervisorError::StaleLaunchGeneration)
    );
    eof(owner);
}
