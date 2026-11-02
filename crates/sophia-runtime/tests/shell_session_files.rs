//! The single-shell facade uses the production file export and epoch owner.
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::{
    ProtectionBackendKind, ProtectionDomainEvidence, ProtectionDomainRole,
    ShellContentAdmissionPolicy, ShellSessionTransport,
};
use sophia_shell_client::{ShellClientOptions, ShellConnection};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

const WAIT: Duration = Duration::from_secs(5);

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

fn polled(fds: &mut [rustix::event::PollFd<'_>]) -> usize {
    rustix::event::poll(fds, Some(&rustix::event::Timespec::default())).unwrap()
}

/// The owner's wait hands the single-shell wire its readiness. An observed
/// request is served by the next turn; a request arriving after an idle
/// observation stays level-ready for the next wait instead of being probed;
/// a stale, discarded or missing observation falls back to the probe.
#[test]
fn the_owner_wait_hands_readiness_to_the_single_shell_turn() {
    let directory =
        std::env::temp_dir().join(format!("shell-session-readiness-{}", std::process::id()));
    let mut host = ShellSessionTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    host.authorize_protected_peer(&ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid: std::process::id(),
        roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
    })
    .unwrap();
    let socket = host.socket_path().to_owned();
    let (go, proceed) = mpsc::channel();
    let (signal, arrived) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: 1,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 6,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let event = peer.next_event();
        peer.ack(&event);
        signal.send(()).unwrap();
        // One getattr per owner-side case; each is sent only when told to.
        for _ in 0..5 {
            proceed.recv_timeout(WAIT).unwrap();
            let body = [1u32.to_le_bytes().as_slice(), &0x7ffu64.to_le_bytes()].concat();
            let (kind, _) = peer
                .rpc_with_prefix(24, &body, 7 + body.len(), || signal.send(()).unwrap())
                .unwrap();
            assert_eq!(kind, 25);
            signal.send(()).unwrap();
        }
    });
    host.accept_files_with_content_policy(
        1,
        WAIT,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: false,
        },
    )
    .unwrap();
    let started = Instant::now();
    while arrived.try_recv().is_err() {
        host.poll_io().unwrap();
        assert!(started.elapsed() < WAIT);
        std::thread::yield_now();
    }

    // Valid handoff: the wait sees the request and the turn answers it.
    go.send(()).unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    let mut fds = Vec::new();
    let wait = host.subscribe_wait(fds.len(), |fd| fds.push(fd)).unwrap();
    assert!(polled(&mut fds) > 0);
    wait.observe(&fds);
    drop(fds);
    host.service_owner_turn().unwrap();
    arrived.recv_timeout(WAIT).unwrap();

    // Late readiness: a request after an idle observation is not probed by
    // the turn that consumes it; it stays level-ready for the next wait.
    let mut fds = Vec::new();
    let wait = host.subscribe_wait(fds.len(), |fd| fds.push(fd)).unwrap();
    polled(&mut fds);
    assert!(
        fds[1].revents().is_empty(),
        "the socket was ready before the request"
    );
    wait.observe(&fds);
    drop(fds);
    go.send(()).unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    host.service_owner_turn().unwrap();
    let mut fds = Vec::new();
    let wait = host.subscribe_wait(fds.len(), |fd| fds.push(fd)).unwrap();
    polled(&mut fds);
    assert!(
        !fds[1].revents().is_empty(),
        "an idle observation's turn probed the socket"
    );
    wait.observe(&fds);
    drop(fds);
    host.service_owner_turn().unwrap();
    arrived.recv_timeout(WAIT).unwrap();

    // Stale: a publication's flush between the wait and the turn invalidates
    // the idle observation, so the turn probes and finds the request.
    let mut fds = Vec::new();
    let wait = host.subscribe_wait(fds.len(), |fd| fds.push(fd)).unwrap();
    polled(&mut fds);
    wait.observe(&fds);
    drop(fds);
    host.publish_indicators(
        TransactionId::from_raw(1),
        &ShellIndicatorSnapshot {
            connection_epoch: 1,
            generation: 1,
            active_output: Some(OutputId::from_raw(3)),
            statuses: vec![ShellOutputStatus {
                output: OutputId::from_raw(3),
                focus_bits: 1,
                layout: "single".to_owned(),
            }],
            indicators: vec![ShellIndicator {
                output: OutputId::from_raw(3),
                indicator: 1,
                action: 2,
                slot: 0,
                state_bits: 1,
                label: "clock".to_owned(),
            }],
        },
    )
    .unwrap();
    host.poll_io().unwrap();
    go.send(()).unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    host.service_owner_turn().unwrap();
    arrived.recv_timeout(WAIT).unwrap();

    // Discarded: a pass that does not turn the wire drops the observation.
    let mut fds = Vec::new();
    let wait = host.subscribe_wait(fds.len(), |fd| fds.push(fd)).unwrap();
    polled(&mut fds);
    wait.observe(&fds);
    drop(fds);
    host.discard_readiness();
    go.send(()).unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    host.service_owner_turn().unwrap();
    arrived.recv_timeout(WAIT).unwrap();

    // Missing: no wait at all, the turn probes as before.
    go.send(()).unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    host.service_owner_turn().unwrap();
    arrived.recv_timeout(WAIT).unwrap();

    peer.join().unwrap();
    host.disconnect().unwrap();
    drop(host);
    assert!(!directory.exists());
}
