//! Real stdio custody, including Bubblewrap; no display or GPU.
use sophia_runtime::{
    LaunchChannel, ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole,
    ProtectionDomainSpec, SupervisedProcessKind, SupervisorCommand,
};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

#[test]
fn private_output_endpoints_are_consumed_by_the_exact_spawn_and_close_before_reap() {
    for protected in [false, true] {
        let (mut output, writer) = UnixStream::pair().unwrap();
        let (mut errors, error_writer) = UnixStream::pair().unwrap();
        output
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        errors
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut spec = ProcessLaunchSpec::new("/usr/bin/sh").arg("-c").arg(
            "printf private; printf error >&2; read line <&\"$SOPHIA_LAUNCH_AUTH_FD\"; exit 17",
        );
        if protected {
            spec = spec.protection_domain(
                ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient]).unwrap(),
            );
        }
        let mut supervisor = ProcessSupervisor::new_with_stderr(
            SupervisedProcessKind::NamespaceClient,
            spec,
            error_writer.into(),
        )
        .with_stdout(writer.into());
        let (mut gate, channel) = LaunchChannel::reserve(1).unwrap();
        if protected {
            supervisor.start_with_launch_channel(channel).unwrap();
        } else {
            supervisor.start_host_namespace_client(channel).unwrap();
        }
        let mut bytes = [0; 7];
        output.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"private");
        let mut bytes = [0; 5];
        errors.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"error");
        gate.write_all(b"done\n").unwrap();
        assert_eq!(
            output.read(&mut [0; 1]).unwrap(),
            0,
            "parent retained stdout writer"
        );
        assert_eq!(
            errors.read(&mut [0; 1]).unwrap(),
            0,
            "parent retained stderr writer"
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while supervisor.exit_status().is_none() {
            supervisor.poll().unwrap();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(supervisor.exit_status().unwrap().code(), Some(17));
    }
}

#[test]
fn stdout_refuses_domain_policy_mismatch_and_closes_on_failed_spawn() {
    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::NamespaceClient])
        .unwrap()
        .inherited_fds([0, 2])
        .unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::NamespaceClient,
        ProcessLaunchSpec::new("/usr/bin/true").protection_domain(domain),
    )
    .with_stdout(writer.into());
    let (_peer, channel) = LaunchChannel::reserve(1).unwrap();
    assert!(
        supervisor
            .start_with_launch_channel(channel)
            .unwrap_err()
            .to_string()
            .contains("PrivateOutputNotInherited")
    );
    assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::NamespaceClient,
        ProcessLaunchSpec::new("/nonexistent-capture-executable"),
    )
    .with_stdout(writer.into());
    assert!(
        supervisor
            .apply(SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::NamespaceClient,
                delay: Duration::ZERO
            })
            .is_err()
    );
    assert_eq!(reader.read(&mut [0; 1]).unwrap(), 0);
}
