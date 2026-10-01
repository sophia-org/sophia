use super::*;
use crate::OutputFileNode as Node;
use sophia_9p::{OpenFlags, export::Export};
use sophia_protocol::{output_files::*, *};
use std::os::unix::net::UnixStream;
use std::time::Instant;

fn snapshot() -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: 4,
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
    }
}

fn worker(label: &str) -> Worker {
    worker_with_limits(
        label,
        OutputFileLimits {
            ack_progress_timeout_millis: 1,
            ..OutputFileLimits::default()
        },
    )
}

fn worker_with_limits(label: &str, limits: OutputFileLimits) -> Worker {
    let directory =
        std::env::temp_dir().join(format!("output-worker-{}-{label}", std::process::id()));
    let mut transport = OutputFileTransport::bind_for_supervised_uid(
        directory,
        rustix::process::geteuid().as_raw(),
        7,
        limits,
    )
    .unwrap();
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    Worker {
        transport,
        snapshot: snapshot(),
        publish_pending: false,
        paused: false,
        pending: VecDeque::new(),
    }
}

#[path = "output_file_epoch_retirement.rs"]
mod epoch_retirement;

fn negotiate(worker: &mut Worker) -> UnixStream {
    let peer = UnixStream::connect(worker.transport.socket_path()).unwrap();
    assert!(worker.transport.poll_accept(&worker.snapshot).unwrap());
    let export = worker.transport.export_mut().unwrap();
    let candidate = encode_output_file_record(
        OutputFileHeader {
            kind: OutputFileKind::Negotiate,
            connection_epoch: 7,
            submission_id: 1,
            sequence: 0,
        },
        &encode_output_file_negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
        }),
    )
    .unwrap();
    let mut staging = export.open(&Node::Transaction, OpenFlags(2)).unwrap();
    export
        .write(&Node::Transaction, &mut staging, 0, &candidate)
        .unwrap();
    let mut submit = export.open(&Node::Submit, OpenFlags(1)).unwrap();
    export
        .write(
            &Node::Submit,
            &mut submit,
            0,
            &encode_output_file_submit(OutputFileSubmit {
                connection_epoch: 7,
                submission_id: 1,
                candidate_bytes: candidate.len() as u32,
            })
            .unwrap(),
        )
        .unwrap();
    peer
}

#[test]
fn expiry_inside_owner_operations_keeps_listener_available() {
    for publish in [false, true] {
        let mut worker = worker(if publish { "publish" } else { "settle" });
        let _peer = negotiate(&mut worker);
        // No transport turn runs between the deadline and the owner operation.
        std::thread::sleep(Duration::from_millis(3));
        assert!(!worker.transport.export().unwrap().is_revoked());
        if publish {
            worker.publish().unwrap();
        } else {
            worker
                .command(OutputFileServiceCommand::Settle {
                    transaction: TransactionId::from_raw(1),
                    outcome: OutputV1Outcome {
                        connection_epoch: 7,
                        topology_epoch: 4,
                        kind: OutputV1OutcomeKind::Validated,
                        reason: 0,
                    },
                })
                .unwrap();
        }
        assert!(worker.transport.export().is_none());
        assert!(matches!(
            worker.pending.pop_front(),
            Some(OutputFileServiceEvent::Disconnected {
                connection_epoch: 7
            })
        ));
        assert!(worker.pending.is_empty());
        let _next = UnixStream::connect(worker.transport.socket_path()).unwrap();
        assert!(worker.transport.poll_accept(&worker.snapshot).unwrap());
        assert_eq!(
            worker
                .transport
                .export()
                .unwrap()
                .admission()
                .connection()
                .connection_epoch(),
            8
        );
    }
}

#[test]
fn stale_owner_error_without_revocation_is_not_swallowed() {
    let mut worker = worker("owner-error");
    let _peer = UnixStream::connect(worker.transport.socket_path()).unwrap();
    assert!(worker.transport.poll_accept(&worker.snapshot).unwrap());
    assert!(matches!(
        worker.owner_error(Errno::ESTALE.into()),
        Err(OutputFileTransportError::File(Errno::ESTALE))
    ));
    assert!(worker.transport.export().is_some());
    assert!(worker.pending.is_empty());
}

#[test]
fn reaped_assignee_cannot_be_admitted_again() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut worker = worker("reaped");
    let mut child = Command::new("/bin/sh")
        .args(["-c", "read -r gate"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    worker
        .transport
        .authorize_supervised_pid(child.id())
        .unwrap();
    child.stdin.take().unwrap().write_all(b"exit\n").unwrap();
    assert!(child.wait().unwrap().success());
    // Model a recycled numeric credential matching this process. The original
    // pidfd must still prevent admission before the worker sees a pause.
    worker
        .transport
        .test_replace_expected_pid(std::process::id());
    let _peer = UnixStream::connect(worker.transport.socket_path()).unwrap();
    assert!(!worker.transport.poll_accept(&worker.snapshot).unwrap());
    assert!(worker.transport.export().is_none());
    assert_eq!(worker.transport.next_epoch(), 7);
}

#[test]
#[ignore = "socket identity fixture invoked by its parent"]
fn queued_connector_child() {
    let path = std::path::PathBuf::from(std::env::var_os("OUTPUT_IDENTITY_SOCKET").unwrap());
    let _peer = UnixStream::connect(&path).unwrap();
    std::fs::write(path.with_extension("ready"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(10));
}

#[test]
fn queued_socket_from_a_dead_connector_is_not_the_live_assignee() {
    use std::process::{Command, Stdio};
    let mut worker = worker("queued-connector");
    let path = worker.transport.socket_path().to_owned();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "output_file_service::tests::queued_connector_child",
            "--ignored",
        ])
        .env("OUTPUT_IDENTITY_SOCKET", &path)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !path.with_extension("ready").exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    std::fs::remove_file(path.with_extension("ready")).unwrap();
    // Model matching recycled numeric credentials while the real assignee
    // remains alive. The socket-pinned connector is the departed process.
    worker.transport.test_replace_expected_pid(pid);
    // A reaped connector can make SO_PEERPIDFD itself fail; both refusal
    // paths must release endpoint custody without spending an epoch.
    assert!(matches!(
        worker.transport.poll_accept(&worker.snapshot),
        Ok(false) | Err(OutputFileTransportError::Io(_))
    ));
    assert!(worker.transport.export().is_none());
    assert_eq!(worker.transport.next_epoch(), 7);
    worker
        .transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    let _next = UnixStream::connect(&path).unwrap();
    assert!(worker.transport.poll_accept(&worker.snapshot).unwrap());
    assert_eq!(worker.transport.next_epoch(), 8);
}

fn supervised(spec: crate::ProcessLaunchSpec) -> crate::ProcessSupervisor {
    let mut supervisor =
        crate::ProcessSupervisor::new(crate::SupervisedProcessKind::OutputAuthority, spec);
    supervisor
        .apply(crate::SupervisorCommand::StartProcess {
            process: crate::SupervisedProcessKind::OutputAuthority,
            delay: Duration::ZERO,
        })
        .unwrap();
    supervisor
}

#[test]
fn checked_reassignment_resumes_paused_worker_with_the_captured_peer() {
    let mut worker = worker("checked-reassignment");
    let old_peer = UnixStream::connect(worker.transport.socket_path()).unwrap();
    assert!(worker.transport.poll_accept(&worker.snapshot).unwrap());
    worker.paused = true;
    let path = worker.transport.socket_path().to_owned();
    // Queue the old assignee first so rejection cannot be hidden behind the
    // new child's accepted connection in the listen backlog.
    let _old_reconnect = UnixStream::connect(&path).unwrap();
    let mut supervisor = supervised(
        crate::ProcessLaunchSpec::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("output_file_service::tests::queued_connector_child")
            .arg("--ignored")
            .env("OUTPUT_IDENTITY_SOCKET", &path),
    );
    let assignee = OutputFileAssignee::from_supervisor(&supervisor).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.with_extension("ready").exists() {
        assert!(Instant::now() < deadline, "supervised peer did not connect");
        std::thread::sleep(Duration::from_millis(1));
    }
    std::fs::remove_file(path.with_extension("ready")).unwrap();
    worker
        .command(OutputFileServiceCommand::ReplaceSupervisedProcess(assignee))
        .unwrap();
    assert!(!worker.paused);
    assert!(worker.transport.export().is_none());
    assert!(
        matches!(worker.pending.pop_front(), Some(OutputFileServiceEvent::AssigneeReplaced {
        connection_epoch: 8, abandoned,
    }) if abandoned.is_empty())
    );
    assert!(worker.pending.is_empty());
    let mut byte = [0];
    old_peer
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    assert_eq!(std::io::Read::read(&mut &old_peer, &mut byte).unwrap(), 0);
    assert!(matches!(worker.transport.poll_accept(&worker.snapshot),
        Err(OutputFileTransportError::Endpoint(crate::PolicyRoleEndpointError::UnauthorizedPeer {
            expected, actual,
        })) if expected.pid == supervisor.peer_id().unwrap() && actual.pid == std::process::id()));
    assert_eq!(worker.transport.next_epoch(), 8);
    assert!(worker.transport.export().is_none());
    assert!(worker.transport.poll_accept(&worker.snapshot).unwrap());
    assert_eq!(
        worker
            .transport
            .export()
            .unwrap()
            .admission()
            .connection()
            .connection_epoch(),
        8
    );
    assert_eq!(worker.snapshot, snapshot());
    supervisor.terminate().unwrap();
}

#[test]
fn checked_assignment_that_dies_in_handoff_admits_nobody_and_spends_no_epoch() {
    let mut worker = worker("checked-dead-handoff");
    let mut supervisor = supervised(crate::ProcessLaunchSpec::new("/bin/sleep").arg("30"));
    let assignee = OutputFileAssignee::from_supervisor(&supervisor).unwrap();
    supervisor.terminate().unwrap();
    worker.paused = true;
    worker
        .command(OutputFileServiceCommand::ReplaceSupervisedProcess(assignee))
        .unwrap();
    assert!(!worker.paused);
    assert!(
        matches!(worker.pending.pop_front(), Some(OutputFileServiceEvent::AssigneeReplaced {
        connection_epoch: 7, abandoned,
    }) if abandoned.is_empty())
    );
    // Model a recycled numeric credential without changing the captured pidfd.
    worker
        .transport
        .test_replace_expected_pid(std::process::id());
    let _peer = UnixStream::connect(worker.transport.socket_path()).unwrap();
    assert!(!worker.transport.poll_accept(&worker.snapshot).unwrap());
    assert_eq!(worker.transport.next_epoch(), 7);
    assert!(worker.transport.export().is_none());
    assert!(worker.pending.is_empty());
}

#[test]
fn missing_supervised_process_cannot_construct_a_worker_assignment() {
    let supervisor = crate::ProcessSupervisor::new(
        crate::SupervisedProcessKind::OutputAuthority,
        crate::ProcessLaunchSpec::new("/bin/true"),
    );
    assert!(OutputFileAssignee::from_supervisor(&supervisor).is_err());
}
