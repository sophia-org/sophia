//! Real private socket negotiation and shared Session success reporting. Peer
//! protection evidence is supplied by the fixture, not a supervisor/KMS proof.
use super::*;
std::thread_local! {
    static SUCCESS_RECORDS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
fn capture_success(line: &str) {
    if line.starts_with("sophia_live_metadata_shell schema=1 status=ready ")
        || line.starts_with("sophia_live_metadata_shell schema=1 status=reconnected ")
    {
        SUCCESS_RECORDS.with(|records| records.borrow_mut().push(line.to_owned()));
    }
}

#[test]
fn descriptor_launch_uses_only_files_without_changing_content_grants() {
    for content in [false, true] {
        let shell = LiveMetadataShell::prepare(
            "/bin/false",
            Some(24),
            content,
            content,
            sophia_config::ShellGpuMode::Denied,
            None,
            None,
        )
        .unwrap();
        let endpoints = shell
            .base_launch_spec
            .environment
            .iter()
            .filter(|(name, _)| name == "SOPHIA_SHELL_SOCKET" || name == "SOPHIA_SHELL_9P_SOCKET")
            .collect::<Vec<_>>();
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].0, "SOPHIA_SHELL_9P_SOCKET");
        assert_eq!(endpoints[0].1, shell.transport.socket_path());
        assert!(shell.presentation_paused);
        assert!(!shell.connected);
        assert_eq!(
            shell.content.admission_policy(),
            if content {
                sophia_runtime::ShellContentAdmissionPolicy::Granted {
                    discrete_input: true,
                }
            } else {
                sophia_runtime::ShellContentAdmissionPolicy::Denied
            }
        );
    }
}

#[test]
fn deferred_first_negotiation_is_ready_and_only_later_connection_is_reconnected() {
    crate::install_session_output(crate::SessionOutput::new(capture_success, |_| {})).unwrap();
    let mut shell = LiveMetadataShell::prepare(
        "/bin/false",
        None,
        false,
        false,
        sophia_config::ShellGpuMode::Denied,
        None,
        None,
    )
    .unwrap();
    assert!(matches!(
        shell.poll().unwrap(),
        LiveMetadataShellPoll::Unavailable
    ));
    assert_eq!(shell.next_connection_epoch, 1);
    assert!(!shell.connected);
    for epoch in 1..=2 {
        shell
            .transport
            .authorize_protected_peer(&sophia_runtime::ProtectionDomainEvidence {
                backend: sophia_runtime::ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [sophia_runtime::ProtectionDomainRole::MetadataShell]
                    .into_iter()
                    .collect(),
            })
            .unwrap();
        let socket = shell.transport.socket_path().to_path_buf();
        let (release, wait) = std::sync::mpsc::channel();
        let (ready, observed) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let client = sophia_shell_client::ShellConnection::connect_files(
                socket,
                sophia_shell_client::ShellClientOptions {
                    minimum_revision: 8,
                    maximum_revision: 8,
                    required_capabilities: 1,
                    handshake_timeout: Duration::from_secs(5),
                },
            )
            .unwrap();
            assert_eq!(client.welcome().connection_epoch, epoch);
            ready.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let welcome = shell
            .transport
            .accept_descriptor_files_with_content_policy(
                epoch,
                Duration::from_secs(5),
                shell.content.admission_policy(),
            )
            .unwrap();
        // Server negotiation completes before the file client has consumed
        // its bootstrap custody and acknowledgement replies.
        let deadline = Instant::now() + Duration::from_secs(5);
        while observed.try_recv().is_err() {
            assert!(
                Instant::now() < deadline,
                "descriptor client readiness timed out"
            );
            shell.transport.poll_io().unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        let result = shell
            .finish_negotiation(
                std::process::id(),
                welcome.selected_revision,
                epoch,
                "fixture_retry",
            )
            .unwrap();
        assert_eq!(
            result,
            if epoch == 1 {
                LiveMetadataShellPoll::Connected {
                    connection_epoch: 1,
                }
            } else {
                LiveMetadataShellPoll::Reconnected {
                    connection_epoch: 2,
                }
            }
        );
        assert!(shell.connected);
        assert_eq!(shell.next_connection_epoch, epoch + 1);
        assert!(
            shell
                .finish_negotiation(
                    std::process::id(),
                    welcome.selected_revision,
                    epoch,
                    "duplicate"
                )
                .is_err()
        );
        assert_eq!(shell.next_connection_epoch, epoch + 1);
        release.send(()).unwrap();
        peer.join().unwrap();
        shell.retire_connection_state("fixture_disconnect").unwrap();
    }
    SUCCESS_RECORDS.with(|records| {
        let records = records.borrow();
        assert_eq!(records.len(), 2);
        assert!(
            records[0]
                .starts_with("sophia_live_metadata_shell schema=1 status=ready protected=true ")
        );
        assert!(records[0].ends_with("connection_epoch=1"));
        assert!(
            records[1].starts_with(
                "sophia_live_metadata_shell schema=1 status=reconnected protected=true "
            )
        );
        assert!(records[1].ends_with("connection_epoch=2 reason=fixture_retry"));
    });
}

const DESCRIPTOR_PEER: &str =
    "live_session::metadata_shell::launch::tests::protected_descriptor_peer";

fn protected_descriptor_shell() -> LiveMetadataShell {
    let executable = std::env::current_exe().unwrap();
    let mut shell = LiveMetadataShell::prepare(
        executable.to_str().unwrap(),
        None,
        false,
        false,
        sophia_config::ShellGpuMode::Denied,
        None,
        None,
    )
    .unwrap();
    // The real launch plan and supervisor are retained; only the fixture's
    // test-harness arguments replace the product-independent --serve entry.
    shell.base_launch_spec.args = ["--exact", DESCRIPTOR_PEER, "--ignored", "--nocapture"]
        .into_iter()
        .map(Into::into)
        .collect();
    shell
        .base_launch_spec
        .environment
        .push(("SOPHIA_DESCRIPTOR_FIXTURE".into(), "1".into()));
    shell.presentation_paused = false;
    shell
}

#[test]
fn protected_descriptor_files_select_the_role_and_reconnect_with_a_new_epoch() {
    use sophia_protocol::*;
    let mut shell = protected_descriptor_shell();
    for epoch in 1..=2 {
        let (pid, revision, selected_epoch) = shell.launch_and_negotiate().unwrap();
        assert_ne!(pid, std::process::id());
        assert_eq!(revision, 8);
        assert_eq!(selected_epoch, epoch);
        assert!(shell.transport.content_limits().is_none());
        assert!(shell.transport.supports_tabs());
        assert!(shell.transport.supports_reference());
        assert!(shell.transport.supports_launcher());
        assert_eq!(
            shell
                .finish_negotiation(pid, revision, epoch, "fixture")
                .unwrap(),
            if epoch == 1 {
                LiveMetadataShellPoll::Connected {
                    connection_epoch: epoch,
                }
            } else {
                LiveMetadataShellPoll::Reconnected {
                    connection_epoch: epoch,
                }
            }
        );
        let snapshot = ShellV1DescriptorSnapshot {
            connection_epoch: epoch,
            snapshot_generation: epoch,
            output: OutputId::from_raw(1),
            output_generation: 1,
            broker_epoch: 1,
            broker_revocation_epoch: 1,
            descriptors: vec![],
        };
        shell
            .transport
            .begin_candidate_request(TransactionId::from_raw(epoch), &snapshot)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let candidate = loop {
            if let Some(candidate) = shell.transport.poll_candidate().unwrap() {
                break candidate;
            }
            assert!(
                Instant::now() < deadline,
                "protected descriptor candidate timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(candidate.connection_epoch, epoch);
        assert_eq!(candidate.snapshot_generation, epoch);
        assert!(!candidate.visible);
        // This is admission/transport evidence, not an Engine commit. Ending
        // the epoch must release an accepted but not yet presented candidate.
        shell.transport.disconnect().unwrap();
        shell.supervisor.terminate().unwrap();
        shell.connected = false;
    }
}

#[test]
fn the_content_profile_cannot_admit_a_descriptor_child() {
    let mut shell = protected_descriptor_shell();
    // This negative intentionally binds the content-only export instead of
    // the descriptor owner's fixed negotiation path. The same protected peer
    // must be refused before it gains descriptor authority.
    shell
        .supervisor
        .apply(sophia_runtime::SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::Shell,
            delay: Duration::ZERO,
        })
        .unwrap();
    shell
        .transport
        .authorize_protected_peer(shell.supervisor.protection_evidence().unwrap())
        .unwrap();
    assert!(
        shell
            .transport
            .accept_files_with_content_policy(
                1,
                Duration::from_secs(5),
                shell.content.admission_policy(),
            )
            .is_err()
    );
    shell.supervisor.terminate().unwrap();
    assert!(!shell.connected);
    assert_eq!(shell.next_connection_epoch, 1);
}

#[test]
#[ignore = "protected child entry, invoked only by the parent test"]
fn protected_descriptor_peer() {
    use sophia_protocol::shell_files::*;
    use sophia_protocol::*;
    use sophia_shell_client::{
        DescriptorObservation, ShellClientError, ShellClientOptions, ShellConnection,
    };
    assert_eq!(std::env::var("SOPHIA_DESCRIPTOR_FIXTURE").unwrap(), "1");
    assert!(std::env::var_os("SOPHIA_SHELL_SOCKET").is_none());
    assert!(std::env::var_os("DISPLAY").is_none());
    assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
    assert!(!std::path::Path::new("/dev/dri").exists());
    let socket = std::env::var_os("SOPHIA_SHELL_9P_SOCKET").unwrap();
    let Ok(mut client) = ShellConnection::connect_files(
        socket,
        ShellClientOptions {
            minimum_revision: 8,
            maximum_revision: 8,
            required_capabilities: 1 | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6),
            handshake_timeout: Duration::from_secs(5),
        },
    ) else {
        return;
    }; // The content-only parent is the explicit refusal control.
    assert_eq!(client.welcome().selected_revision, 8);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match client.poll_io() {
            Ok(()) => {}
            Err(ShellClientError::PeerClosed) => return,
            Err(error) => panic!("descriptor fixture I/O: {error:?}"),
        }
        loop {
            let observation = match client.take_descriptor_observation() {
                Ok(Some(value)) => value,
                Ok(None) => break,
                Err(ShellClientError::PeerClosed) => return,
                Err(error) => panic!("descriptor fixture: {error:?}"),
            };
            let DescriptorObservation::Record(ShellFileDescriptorRecord {
                transaction,
                record: ShellDescriptorRecord::Descriptors(snapshot),
            }) = observation
            else {
                panic!("unexpected descriptor fixture observation")
            };
            let candidate = ShellV1Candidate {
                connection_epoch: snapshot.connection_epoch,
                snapshot_generation: snapshot.snapshot_generation,
                candidate_generation: snapshot.snapshot_generation,
                output: snapshot.output,
                visible: false,
                selected_slot: None,
                reservation: None,
                entries: vec![],
            };
            client
                .enqueue_descriptor_tracked(&ShellFileDescriptorRecord {
                    transaction,
                    record: ShellDescriptorRecord::DescriptorCandidate(candidate),
                })
                .unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "protected fixture exceeded deadline"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
