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

#[test]
fn rejected_live_pid_stays_dead_through_output_worker_reassignment() {
    use crate::{
        OutputFileAssignee, OutputFileService, OutputFileServiceCommand, OutputFileServiceEvent,
        OutputFileTransport,
    };
    use sophia_protocol::{output_files::OutputFileLimits, *};
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    let mut supervisor = supervisor();
    let child = supervisor.child.as_mut().unwrap();
    child.peer_pid = std::process::id();
    child.protection = Some(ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: child.child.id(),
        peer_pid: child.peer_pid,
        roles: [ProtectionDomainRole::OutputAuthority]
            .into_iter()
            .collect(),
    });
    // The PID is live but fails the protected parent check. A worker that
    // reopens this number would incorrectly admit this test process.
    let assignee = OutputFileAssignee::from_supervisor(&supervisor).unwrap();
    let directory =
        std::env::temp_dir().join(format!("output-rejected-assignee-{}", std::process::id()));
    let transport = OutputFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        1,
        OutputFileLimits::default(),
    )
    .unwrap();
    let path = transport.socket_path().to_owned();
    let snapshot = OutputAuthoritySnapshot {
        topology_epoch: 1,
        primary_output: OutputId::from_raw(1),
        heads: vec![OutputHeadDescriptor {
            head: DisplayHeadId::from_raw(1),
            generation: 1,
            label: "panel".into(),
            connected: true,
            enabled: true,
            vrr_capable: false,
            transforms: OutputTransformSet::ALL,
            current_mode: Some(DisplayModeId::from_raw(1)),
            modes: vec![OutputModeDescriptor {
                mode: DisplayModeId::from_raw(1),
                pixel_size: Size {
                    width: 800,
                    height: 600,
                },
                refresh_millihz: 60_000,
                preferred: true,
            }],
        }],
        groups: vec![OutputLogicalGroupState {
            output: OutputId::from_raw(1),
            generation: 1,
            logical: Rect {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
            members: vec![OutputGroupMember {
                head: DisplayHeadId::from_raw(1),
                mapping: OutputHeadMapping::Exact,
            }],
        }],
    };
    let service = OutputFileService::spawn(transport, snapshot).unwrap();
    service
        .command(OutputFileServiceCommand::ReplaceSupervisedProcess(assignee))
        .unwrap();
    assert!(
        matches!(service.event_timeout(Duration::from_secs(2)).unwrap(),
        OutputFileServiceEvent::AssigneeReplaced {connection_epoch:1,abandoned} if abandoned.is_empty())
    );
    let mut peer = UnixStream::connect(path).unwrap();
    peer.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut version = 21u32.to_le_bytes().to_vec();
    version.push(100);
    version.extend(u16::MAX.to_le_bytes());
    version.extend(65536u32.to_le_bytes());
    version.extend(8u16.to_le_bytes());
    version.extend(b"9P2000.L");
    peer.write_all(&version).unwrap();
    let mut byte = [0];
    assert!(
        matches!(peer.read(&mut byte), Err(error) if matches!(error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
    );
    assert!(service.try_event().unwrap().is_none());
    drop(peer);
    drop(service);
    assert!(!directory.exists());
    supervisor.terminate().unwrap();
}
