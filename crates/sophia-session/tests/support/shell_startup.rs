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
        let peer = std::thread::spawn(move || {
            let client = sophia_runtime::ShellClientTransport::connect(socket).unwrap();
            assert_eq!(client.connection_epoch(), epoch);
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let welcome = shell
            .transport
            .accept_and_negotiate_with_content_policy(
                epoch,
                Duration::from_secs(5),
                shell.content.admission_policy(),
            )
            .unwrap();
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
