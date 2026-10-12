//! Real protected launch, Factotum and output service. Topology and Session's
//! private-channel class are supplied; this does not run the live startup owner.
use super::lock_authenticated_transport::{Directory, Peer};
use super::*;
use sophia_protocol::*;
use sophia_runtime::{
    OutputFileAssignee, OutputFileService, OutputFileServiceCommand, OutputFileServiceEvent,
    OutputFileTransport, ProtectionPath,
};
use sophia_session::launch_authentication::files::TransportBudget;
use std::os::unix::fs::DirBuilderExt;

#[path = "output_rust_peer.rs"]
mod rust_peer;

fn snapshot() -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: 4,
        primary_output: OutputId::from_raw(1),
        heads: vec![OutputHeadDescriptor {
            head: DisplayHeadId::from_raw(1),
            generation: 1,
            label: "fixture".into(),
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
    }
}

#[test]
fn protected_output_authenticates_reauthenticates_and_revokes_retained_files() {
    let peer =
        Peer::rust_child("output_authenticated_transport::rust_peer::protected_output_child");
    let root = Directory(std::env::temp_dir().join(format!("t317-output-{}", std::process::id())));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root.0)
        .unwrap();
    let reports = root.0.join("reports");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&reports)
        .unwrap();
    let report = reports.join("progress");
    let transport = OutputFileTransport::bind_for_supervised_uid(
        root.0.join("endpoint"),
        rustix::process::geteuid().as_raw(),
        7,
        Default::default(),
    )
    .unwrap();
    let socket = transport.socket_path().to_owned();
    let service = OutputFileService::spawn(transport, snapshot()).unwrap();
    let mut f = Fixture::new();
    let id = f
        .book
        .reserve(
            LaunchPolicy {
                principal: 8,
                namespace: 9,
                audience: 12,
                role: ProtectionDomainRole::OutputAuthority,
            },
            Instant::now(),
            deadline(),
        )
        .unwrap();
    let end = deadline();
    while f.book.phase(id) != Some(Phase::Ready) {
        f.tick(end);
    }
    let executable = std::env::current_exe().unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::OutputAuthority])
        .unwrap()
        .path(ProtectionPath::read_only(root.0.join("endpoint")))
        .unwrap()
        .path(ProtectionPath::read_write(&reports))
        .unwrap()
        .path(ProtectionPath::read_only(&executable))
        .unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::OutputAuthority,
        ProcessLaunchSpec::new(&peer.0)
            .arg(&executable)
            .arg(&socket)
            .arg(&report)
            .protection_domain(domain),
    );
    supervisor
        .start_with_launch_channel(f.book.take_channel(id, Instant::now()).unwrap())
        .unwrap();
    f.activate(id, &mut supervisor);
    let worker = f
        .book
        .worker_authority(id, &mut supervisor, Instant::now())
        .unwrap();
    let factory = TransportBudget::new(f.namespaces.clone())
        .endpoint()
        .bind(worker, b"output".to_vec(), b"output-authority".to_vec())
        .unwrap();
    service
        .command(OutputFileServiceCommand::ReplaceSupervisedProcess(
            OutputFileAssignee::from_supervisor(&supervisor)
                .unwrap()
                .with_authentication(Box::new(factory)),
        ))
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let mut connected = 0;
    let mut disconnected = 0;
    let mut revoked = false;
    let mut exited = false;
    loop {
        f.tick(end);
        while let Some(event) = service.try_event().unwrap() {
            match event {
                OutputFileServiceEvent::Connected {
                    connection_epoch: 7,
                } => connected += 1,
                OutputFileServiceEvent::Disconnected {
                    connection_epoch: 7,
                } => disconnected += 1,
                OutputFileServiceEvent::AssigneeReplaced { abandoned, .. } => {
                    assert!(abandoned.is_empty())
                }
                other => panic!("unexpected authenticated output event: {other:?}"),
            }
        }
        if !revoked
            && connected == 1
            && std::fs::read_to_string(&report).ok().as_deref() == Some("waiting\n")
        {
            assert_eq!(f.namespaces.lock().unwrap().admission_count(), 1);
            f.book.revoke(id, Instant::now()).unwrap();
            revoked = true;
        }
        exited |= supervisor.poll().unwrap().is_some();
        if exited && disconnected == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        supervisor.take_exit_status().unwrap().success(),
        "protected output client must finish its authenticated exchange"
    );
    assert!(revoked);
    assert_eq!(connected, 1);
    assert_eq!(std::fs::read_to_string(&report).unwrap(), "revoked\n");
    assert_eq!(f.namespaces.lock().unwrap().admission_count(), 0);
}
