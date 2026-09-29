//! Nonblocking file negotiation with supplied protection evidence. Barriers
//! place partial requests and unread replies without sleeps or kernel pressure.
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;
const WAIT: Duration = Duration::from_secs(5);
struct Host {
    server: ShellComponentTransport,
    directory: std::path::PathBuf,
    epoch: u64,
    welcome: Option<ShellV1ServerWelcome>,
}
impl Host {
    fn new(
        registry: &mut ContentEpochRegistry,
        epoch: u64,
        policy: ShellContentAdmissionPolicy,
    ) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "shell-file-handshake-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut server = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        server
            .authorize_protected_peer(&ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            })
            .unwrap();
        let mut limits = ContentLimits::prototype(grant(epoch));
        limits.max_staging_bytes = 4 * 1024 * 1024;
        limits.max_resident_bytes = 12 * 1024 * 1024;
        limits.max_retiring_bytes = 8 * 1024 * 1024;
        server.reserve_content(registry, limits).unwrap();
        server
            .begin_file_negotiation(registry, epoch, WAIT, policy)
            .unwrap();
        Self {
            server,
            directory,
            epoch,
            welcome: None,
        }
    }
    fn turn(
        &mut self,
        registry: &mut ContentEpochRegistry,
        budget: usize,
    ) -> Result<(), ShellTransportError> {
        if self.welcome.is_some() {
            self.server.poll_io(registry)
        } else {
            self.welcome = self.server.poll_negotiation(registry, budget)?;
            Ok(())
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn grant(epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    }
}
fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    }
}
fn offer(epoch: u64) -> Vec<u8> {
    encode_shell_file_negotiate(
        header(epoch, 1, ShellFileKind::Negotiate),
        ShellV1ClientHello {
            minimum_revision: 5,
            maximum_revision: 6,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
        },
    )
    .unwrap()
}
fn header(epoch: u64, id: u64, kind: ShellFileKind) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id: id,
        sequence: 0,
    }
}
fn demand(epoch: u64) -> ContentFrameDemand {
    ContentFrameDemand {
        grant: grant(epoch),
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
        allocation: ContentAllocationId::default(),
        demand_id: 9,
        reason: 1,
    }
}
struct Staged {
    start: mpsc::Sender<()>,
    sent: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    worker: std::thread::JoinHandle<Peer>,
}
impl Staged {
    fn prepare(
        host: &mut Host,
        registry: &mut ContentEpochRegistry,
        prefix: Option<usize>,
        next: bool,
    ) -> Self {
        let socket = host.server.socket_path().to_owned();
        let epoch = host.epoch;
        let (ready_tx, ready) = mpsc::channel();
        let (start, start_rx) = mpsc::channel();
        let (sent_tx, sent) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut peer = Peer::connect(&socket);
            peer.setup();
            let bytes = offer(epoch);
            peer.open(5, b"transaction", 2);
            assert_eq!(peer.write(5, &bytes).0, 119);
            ready_tx.send(()).unwrap();
            start_rx.recv_timeout(WAIT).unwrap();
            let submit = encode_shell_file_submit(ShellFileSubmit {
                connection_epoch: epoch,
                submission_id: 1,
                candidate_bytes: bytes.len() as u32,
            })
            .unwrap();
            let body = [
                3u32.to_le_bytes().as_slice(),
                &0u64.to_le_bytes(),
                &(submit.len() as u32).to_le_bytes(),
                &submit,
            ]
            .concat();
            assert_eq!(
                peer.rpc_with_prefix(118, &body, prefix.unwrap_or(7 + body.len()), || {
                    sent_tx.send(()).unwrap();
                    release_rx.recv_timeout(WAIT).unwrap();
                })
                .unwrap()
                .0,
                119
            );
            let event = peer.next_event();
            assert_eq!(
                decode_shell_file_submitted(&event).unwrap().submission_id,
                1
            );
            peer.ack(&event);
            peer.clear();
            let event = peer.next_event();
            let negotiated = decode_shell_file_negotiated(&event).unwrap();
            assert_eq!(negotiated.welcome.connection_epoch, epoch);
            assert_eq!(negotiated.welcome.selected_revision, 6);
            assert!(negotiated.limits_published);
            peer.ack(&event);
            peer.open(6, b"limits", 0);
            assert_eq!(
                decode_shell_file_limits(&peer.read(6, 0)).unwrap().grant,
                grant(epoch)
            );
            if next {
                peer.submit_acknowledged(
                    &encode_shell_file_transaction(
                        header(epoch, 2, ShellFileKind::FrameDemand),
                        &ShellFileTransactionRecord {
                            transaction: TransactionId::from_raw(19),
                            record: ShellContentRecord::FrameDemand(demand(epoch)),
                        },
                    )
                    .unwrap(),
                    2,
                );
            }
            peer
        });
        let deadline = Instant::now() + WAIT;
        while ready.try_recv().is_err() {
            host.turn(registry, 65536).unwrap();
            assert!(Instant::now() < deadline, "staging did not finish");
            std::thread::yield_now();
        }
        assert!(host.welcome.is_none());
        Self {
            start,
            sent,
            release,
            worker,
        }
    }
    fn send(&self) {
        self.start.send(()).unwrap();
        self.sent.recv_timeout(WAIT).unwrap();
    }
    fn finish(self, host: &mut Host, registry: &mut ContentEpochRegistry) -> Peer {
        self.release.send(()).unwrap();
        let deadline = Instant::now() + WAIT;
        while !self.worker.is_finished() {
            host.turn(registry, 65536).unwrap();
            assert!(Instant::now() < deadline, "negotiation did not finish");
            std::thread::yield_now();
        }
        assert_eq!(host.welcome.unwrap().connection_epoch, host.epoch);
        self.worker.join().unwrap()
    }
}
#[test]
fn partial_submit_does_not_block_neighbour_or_lose_the_next_request() {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut a = Host::new(&mut registry, 1, granted());
    let mut b = Host::new(&mut registry, 2, granted());
    let pa = Staged::prepare(&mut a, &mut registry, Some(4), true);
    pa.send();
    a.turn(&mut registry, 65536).unwrap();
    assert!(a.welcome.is_none());
    let pb = Staged::prepare(&mut b, &mut registry, None, false);
    pb.send();
    let _pb = pb.finish(&mut b, &mut registry);
    a.turn(&mut registry, 65536).unwrap();
    assert!(a.welcome.is_none());
    assert!(registry.resources(grant(1)).is_some());
    assert!(registry.resources(grant(2)).is_some());
    let _pa = pa.finish(&mut a, &mut registry);
    assert_eq!(
        a.server
            .service_content_demands(&mut registry, &[demand(1).output], &[])
            .unwrap(),
        1
    );
    assert_eq!(
        a.server.next_content_demand(&registry),
        Some((TransactionId::from_raw(19), demand(1)))
    );
    assert_eq!(
        a.server
            .service_content_demands(&mut registry, &[demand(1).output], &[])
            .unwrap(),
        0
    );
    a.server.disconnect(&mut registry).unwrap();
    b.server.disconnect(&mut registry).unwrap();
    assert!(
        a.server
            .collect_content_accounting(&mut registry)
            .quiescent()
    );
}
#[test]
fn zero_budget_retains_handshake_then_journal_custody_completes_before_peer_read() {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut host = Host::new(&mut registry, 1, granted());
    let peer = Staged::prepare(&mut host, &mut registry, None, false);
    peer.send();
    let pending = host.server.content_accounting(&registry);
    assert!(pending.response_records >= 2 && pending.response_bytes >= 512);
    assert_eq!(host.server.poll_negotiation(&mut registry, 0), Ok(None));
    assert!(host.server.content_grant().is_none());
    assert_eq!(host.server.content_accounting(&registry), pending);
    assert_eq!(
        host.server
            .begin_file_negotiation(&registry, 1, WAIT, ShellContentAdmissionPolicy::Denied),
        Err(ShellTransportError::WrongContentGrant)
    );
    assert_eq!(host.server.content_accounting(&registry), pending);
    // For files, a positive visit budget runs bounded 9P turns. Completion
    // means the journal owns the whole reply, not its final byte or peer ACK.
    let deadline = Instant::now() + WAIT;
    while host.welcome.is_none() {
        host.turn(&mut registry, 1).unwrap();
        assert!(Instant::now() < deadline);
    }
    assert_eq!(host.server.content_grant(), Some(grant(1)));
    assert_eq!(
        host.server.content_accounting(&registry).response_records,
        0
    );
    assert_eq!(
        host.server.poll_negotiation(&mut registry, 1),
        Err(ShellTransportError::NotConnected)
    );
    let _peer = peer.finish(&mut host, &mut registry);
    host.server.disconnect(&mut registry).unwrap();
    assert!(
        host.server
            .collect_content_accounting(&mut registry)
            .quiescent()
    );
}
#[test]
fn malformed_and_eof_revoke_only_the_pending_reservation() {
    for eof in [false, true] {
        let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
        let mut a = Host::new(&mut registry, 1, granted());
        let mut b = Host::new(&mut registry, 2, granted());
        let mut peer = UnixStream::connect(a.server.socket_path()).unwrap();
        if eof {
            peer.write_all(&[19, 0, 0]).unwrap();
            peer.shutdown(std::net::Shutdown::Write).unwrap();
        } else {
            peer.write_all(&[255, 255, 255, 255, 100, 255, 255])
                .unwrap();
        }
        let deadline = Instant::now() + WAIT;
        loop {
            if a.server.poll_negotiation(&mut registry, 65536).is_err() {
                break;
            }
            assert!(Instant::now() < deadline);
        }
        assert!(registry.resources(grant(1)).is_none());
        assert!(registry.resources(grant(2)).is_some());
        assert_eq!(a.server.content_accounting(&registry).response_records, 0);
        assert!(!b.server.content_accounting(&registry).quiescent());
        b.server.disconnect(&mut registry).unwrap();
        assert!(
            b.server
                .collect_content_accounting(&mut registry)
                .quiescent()
        );
    }
}
#[test]
fn deadline_and_explicit_disconnect_release_pending_owner() {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut host = Host::new(&mut registry, 1, granted());
    host.server.disconnect(&mut registry).unwrap();
    assert!(
        host.server
            .collect_content_accounting(&mut registry)
            .quiescent()
    );
    host.server
        .begin_file_negotiation(
            &registry,
            2,
            Duration::ZERO,
            ShellContentAdmissionPolicy::Unavailable,
        )
        .unwrap();
    assert_eq!(
        host.server.poll_negotiation(&mut registry, 0),
        Err(ShellTransportError::Endpoint(
            PolicyRoleEndpointError::AcceptTimedOut
        ))
    );
    assert!(host.server.content_accounting(&registry).quiescent());
}
#[test]
fn content_refusal_waits_for_the_exact_peer_ack_before_terminal_error() {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut host = Host::new(&mut registry, 1, ShellContentAdmissionPolicy::Denied);
    let socket = host.server.socket_path().to_owned();
    let (read_tx, read) = mpsc::channel();
    let (ack, ack_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        peer.submit_acknowledged(&offer(1), 1);
        let event = peer.next_event();
        let refusal = decode_shell_file_refused(&event).unwrap();
        assert_eq!(
            refusal,
            ContentAdmissionRefused {
                reason: 1,
                denied_capabilities: SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            }
        );
        read_tx.send(()).unwrap();
        ack_rx.recv_timeout(WAIT).unwrap();
        peer.ack(&event);
        peer
    });
    let deadline = Instant::now() + WAIT;
    while read.try_recv().is_err() {
        assert_eq!(host.server.poll_negotiation(&mut registry, 65536), Ok(None));
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!host.server.content_accounting(&registry).quiescent());
    assert_eq!(host.server.poll_negotiation(&mut registry, 65536), Ok(None));
    assert!(registry.resources(grant(1)).is_some());
    ack.send(()).unwrap();
    let error = loop {
        match host.server.poll_negotiation(&mut registry, 65536) {
            Err(error) => break error,
            Ok(None) => {}
            other => panic!("unexpected {other:?}"),
        }
        assert!(Instant::now() < deadline);
    };
    assert_eq!(
        error,
        ShellTransportError::ContentAdmissionRefused(ContentAdmissionRefused {
            reason: 1,
            denied_capabilities: SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        })
    );
    let _peer = worker.join().unwrap();
    assert!(host.server.content_accounting(&registry).quiescent());
}
