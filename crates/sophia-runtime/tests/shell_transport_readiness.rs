//! What a shell wire and its role endpoint offer an owner's wait: only
//! descriptors whose readiness the owner's next visit consumes. The test
//! thread is the owner and waits the way it does, with one poll over the
//! borrowed set.
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ShellClientOptions, ShellConnection};

const MIB: u64 = 1024 * 1024;
const EPOCH: u64 = 1;
/// Generous: separates "ready" from "never ready", not one delay from another.
const PROMPT: Duration = Duration::from_secs(5);
const BUDGET: Duration = Duration::from_millis(20);
static NEXT: AtomicU64 = AtomicU64::new(0);

fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "shell-transport-readiness-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

/// A shell transport whose authorized peer is this process.
fn transport() -> ShellComponentTransport {
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        directory(),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    transport
}

fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    }
}

fn limits() -> ContentLimits {
    ContentLimits::prototype(ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: EPOCH,
    })
}

/// Whether anything in `fds` becomes ready within `within`.
fn ready(mut fds: Vec<PollFd<'_>>, within: Duration) -> bool {
    let timeout = Timespec::try_from(within).unwrap();
    poll(&mut fds, Some(&timeout)).unwrap() != 0
}

/// Times an unready wait, so a set that is ready at once cannot pass.
fn waits_out(fds: Vec<PollFd<'_>>) -> bool {
    let started = Instant::now();
    !ready(fds, BUDGET) && started.elapsed() >= BUDGET
}

/// `Tversion` as 9P2000.L lays it out, written by hand so this file does not
/// share the server's codec.
fn tversion() -> Vec<u8> {
    let version = b"9P2000.L";
    let size = 4 + 1 + 2 + 4 + 2 + version.len();
    let mut frame = Vec::with_capacity(size);
    frame.extend((size as u32).to_le_bytes());
    frame.push(100);
    frame.extend(u16::MAX.to_le_bytes());
    frame.extend(8192u32.to_le_bytes());
    frame.extend((version.len() as u16).to_le_bytes());
    frame.extend(version);
    frame
}

/// The kind of the next reply frame.
fn reply_kind(stream: &mut UnixStream) -> u8 {
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let mut rest = vec![0; u32::from_le_bytes(size) as usize - 4];
    stream.read_exact(&mut rest).unwrap();
    rest[0]
}

/// The endpoint's listener, which it offers while a visit would accept.
fn offered(endpoint: &RoleEndpoint) -> Vec<PollFd<'_>> {
    let listener = endpoint
        .accept_readiness()
        .expect("an open role offers its listener");
    vec![PollFd::from_borrowed_fd(listener, PollFlags::IN)]
}

#[test]
fn a_held_role_does_not_offer_its_backlog() {
    let mut endpoint = RoleEndpoint::bind_role_for_supervised_uid(
        directory(),
        PolicyRole::Wm,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    assert!(
        endpoint.accept_readiness().is_none(),
        "no peer is authorized, so nothing would accept"
    );
    endpoint
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    assert!(
        waits_out(offered(&endpoint)),
        "an empty backlog woke the owner"
    );
    let _first = UnixStream::connect(endpoint.socket_path()).unwrap();
    assert!(ready(offered(&endpoint), PROMPT));
    let admitted = endpoint
        .poll_expected()
        .unwrap()
        .expect("the peer is admitted");
    // A second peer waits in the backlog while the first holds the role. Its
    // readiness persists, and no visit would accept it.
    let _second = UnixStream::connect(endpoint.socket_path()).unwrap();
    assert!(
        endpoint.accept_readiness().is_none(),
        "a held role offered its backlog"
    );
    let peer = endpoint.active_peer().unwrap();
    endpoint.release_peer(peer).unwrap();
    assert!(
        ready(offered(&endpoint), PROMPT),
        "a released role did not offer its queued peer"
    );
    drop(admitted);
}

#[test]
fn negotiation_offers_its_listener_then_only_its_accepted_wire() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut transport = transport();
    transport.reserve_content(&mut registry, limits()).unwrap();
    // Queued before any negotiation: no visit would accept it yet.
    let mut peer = UnixStream::connect(transport.socket_path()).unwrap();
    peer.set_read_timeout(Some(PROMPT)).unwrap();
    assert!(transport.poll_fds().is_empty());
    transport
        .begin_file_negotiation(&registry, EPOCH, PROMPT, granted())
        .unwrap();
    assert!(
        ready(transport.poll_fds(), PROMPT),
        "a waiting negotiation did not offer its queued peer"
    );
    // The visit accepts, adopts the stream as the pending wire, and turns it.
    assert!(matches!(
        transport.poll_negotiation(&mut registry, 64 * 1024),
        Ok(None)
    ));
    // Another peer queues behind the admitted one. Only the accepted wire is
    // offered, and it has nothing to read.
    let _queued = UnixStream::connect(transport.socket_path()).unwrap();
    assert!(
        waits_out(transport.poll_fds()),
        "an idle pending wire or a held backlog woke the owner"
    );
    peer.write_all(&tversion()).unwrap();
    assert!(
        ready(transport.poll_fds(), PROMPT),
        "a request on the pending wire did not wake the owner"
    );
    assert!(matches!(
        transport.poll_negotiation(&mut registry, 64 * 1024),
        Ok(None)
    ));
    assert!(
        !ready(transport.poll_fds(), Duration::ZERO),
        "the visit left its request ready"
    );
    assert_eq!(reply_kind(&mut peer), 101, "Rversion");
    transport.disconnect(&mut registry).unwrap();
}

#[test]
fn records_queued_after_a_turn_are_pending_until_the_next_one() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut transport = transport();
    transport.reserve_content(&mut registry, limits()).unwrap();
    let socket = transport.socket_path().to_owned();
    let (connected_tx, connected) = std::sync::mpsc::channel::<()>();
    let (finish, finished) = std::sync::mpsc::channel::<()>();
    let client = std::thread::spawn(move || {
        let options = ShellClientOptions {
            minimum_revision: 5,
            maximum_revision: 6,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            handshake_timeout: PROMPT,
        };
        let client = ShellConnection::connect_files(&socket, options).unwrap();
        connected_tx.send(()).unwrap();
        // Holds the connection open, reading nothing, until the owner is done.
        let _ = finished.recv_timeout(PROMPT);
        drop(client);
    });
    transport
        .begin_file_negotiation(&registry, EPOCH, PROMPT, granted())
        .unwrap();
    let started = Instant::now();
    while transport
        .poll_negotiation(&mut registry, 64 * 1024)
        .unwrap()
        .is_none()
    {
        assert!(started.elapsed() < PROMPT, "negotiation did not complete");
        std::thread::yield_now();
    }
    // The client finishes its handshake on the server's later turns.
    while connected.try_recv().is_err() {
        transport.poll_io(&mut registry).unwrap();
        assert!(started.elapsed() < PROMPT, "the client did not connect");
        std::thread::yield_now();
    }
    transport.poll_io(&mut registry).unwrap();
    assert!(!transport.output_pending());
    transport
        .publish_content_output_facts(
            &mut registry,
            TransactionId::from_raw(9),
            1,
            vec![ContentOutputFactsEntry {
                output: ContentOutputId {
                    id: 2,
                    generation: 1,
                },
                local_width: 64,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    // Nothing reaches a socket until a turn, so no descriptor reports it.
    assert!(transport.output_pending());
    transport.poll_io(&mut registry).unwrap();
    assert!(
        !transport.output_pending(),
        "a turn with journal room left the record pending"
    );
    finish.send(()).unwrap();
    client.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
}
