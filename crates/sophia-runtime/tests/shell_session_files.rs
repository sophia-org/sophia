//! The single-shell facade uses the production file export and epoch owner.
use sophia_protocol::{
    SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE, SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
};
use sophia_runtime::{
    ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole,
    ShellContentAdmissionPolicy, ShellSessionTransport,
};
use sophia_shell_client::{ShellClientOptions, ShellConnection};
use std::time::{Duration, Instant};

#[test]
fn single_shell_files_negotiate_and_reconnect_without_retained_content() {
    let directory =
        std::env::temp_dir().join(format!("shell-session-files-{}", std::process::id()));
    let mut host = ShellSessionTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    for epoch in 1..=2 {
        host.authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
        let socket = host.socket_path().to_owned();
        let peer = std::thread::spawn(move || {
            ShellConnection::connect_files(
                &socket,
                ShellClientOptions {
                    minimum_revision: 6,
                    maximum_revision: 6,
                    required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
                    handshake_timeout: Duration::from_secs(3),
                },
            )
        });
        let welcome = host
            .accept_files_with_content_policy(
                epoch,
                Duration::from_secs(3),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: false,
                },
            )
            .unwrap();
        // connect_files must fetch the Limits object after Negotiated. The
        // host must continue servicing the file lane during that fetch.
        let deadline = Instant::now() + Duration::from_secs(3);
        while !peer.is_finished() {
            assert!(
                Instant::now() < deadline,
                "client did not finish its Limits fetch"
            );
            host.poll_io().unwrap();
            std::thread::yield_now();
        }
        let client = peer.join().unwrap().unwrap();
        assert_eq!(welcome.connection_epoch, epoch);
        assert_eq!(client.connection_epoch(), epoch);
        assert_eq!(welcome.selected_revision, 6);
        assert!(host.content_reserved_bytes() > 0);
        host.disconnect().unwrap();
        drop(client);
        assert_eq!(host.content_reserved_bytes(), 0);
        assert_eq!(host.content_backing_reserved_bytes(), 0);
    }
    drop(host);
    assert!(
        !directory.exists(),
        "the endpoint owner removes its directory"
    );
}

#[test]
fn file_negotiation_deadline_releases_the_single_shell_owner() {
    let directory = std::env::temp_dir().join(format!(
        "shell-session-files-deadline-{}",
        std::process::id()
    ));
    let mut host = ShellSessionTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    let start = Instant::now();
    assert!(
        host.accept_files_with_content_policy(
            1,
            Duration::from_millis(20),
            ShellContentAdmissionPolicy::Unavailable,
        )
        .is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(host.content_reserved_bytes(), 0);
    drop(host);
    assert!(
        !directory.exists(),
        "the endpoint owner removes its directory"
    );
}
