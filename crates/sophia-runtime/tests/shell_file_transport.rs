//! `sophia_shell_fs_v1` over a real private socket, with the actual content
//! owners and supplied protection evidence. No protected child, compositor
//! or Session selection here; those are separate evidence.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

const MIB: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

const RLERROR: u8 = 7;
const EACCES: u32 = 13;
const EALREADY: u32 = 114;

fn transport() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-files-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
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
    (transport, directory)
}

fn limits(epoch: u64) -> ContentLimits {
    ContentLimits::prototype(ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    })
}

fn hello() -> ShellV1ClientHello {
    ShellV1ClientHello {
        minimum_revision: 5,
        maximum_revision: 6,
        required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
    }
}

fn candidate(kind: ShellFileKind, epoch: u64, id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id: id,
        sequence: 0,
    }
}

fn errno(reply: (u8, Vec<u8>)) -> u32 {
    assert_eq!(reply.0, RLERROR);
    u32::from_le_bytes(reply.1[..4].try_into().unwrap())
}

fn negotiate(
    transport: &mut ShellComponentTransport,
    registry: &mut ContentEpochRegistry,
    epoch: u64,
    policy: ShellContentAdmissionPolicy,
    peer_done: &std::thread::JoinHandle<()>,
) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
    transport.begin_file_negotiation(registry, epoch, Duration::from_secs(2), policy)?;
    let start = Instant::now();
    loop {
        if let Some(welcome) = transport.poll_negotiation(registry, 64 * 1024)? {
            return Ok(Some(welcome));
        }
        if peer_done.is_finished() {
            return Ok(None);
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
}

#[test]
fn file_negotiation_publishes_limits_and_correlates_a_rejected_allocation() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(1);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let limits_expected = expected.clone();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 1), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        let value = decode_shell_file_negotiated(&negotiated).unwrap();
        assert_eq!(value.welcome.connection_epoch, 1);
        assert_eq!(value.welcome.selected_revision, 6);
        assert!(value.limits_published);
        peer.ack(&negotiated);
        peer.open(6, b"limits", 0);
        let object = peer.read(6, 0);
        assert_eq!(decode_shell_file_limits(&object).unwrap(), limits_expected);

        let request = ContentAllocationRequest {
            grant: limits_expected.grant,
            output: ContentOutputId {
                id: 99,
                generation: 1,
            },
            allocation_request_id: 1,
            operation: 1,
            role: 1,
            edge: 1,
            prior: ContentAllocationId::default(),
            parent: ContentAllocationId::default(),
            parent_presentation_epoch: 0,
            anchor_parent_rect: ContentPixelRect::default(),
            desired_width: 64,
            desired_height: 32,
            margins: ContentMargins::default(),
        };
        let bytes = encode_shell_file_allocation_request(
            candidate(ShellFileKind::AllocationRequest, 1, 2),
            ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(40),
                record: ShellContentRecord::AllocationRequest(request),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&bytes, 2);
        let event = peer.next_event();
        let value = decode_shell_file_allocation_result(&event).unwrap();
        assert_eq!(value.transaction, TransactionId::from_raw(40));
        let ShellContentRecord::AllocationResult(result) = value.record else {
            panic!("allocation result");
        };
        assert_eq!(result.allocation_request_id, 1);
        assert_eq!(result.status, 2);
        peer.ack(&event);
    });
    let welcome = negotiate(
        &mut transport,
        &mut registry,
        1,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: false,
        },
        &peer,
    )
    .unwrap()
    .expect("negotiated");
    assert_eq!(welcome.connection_epoch, 1);
    assert_eq!(transport.content_limits(), Some(&expected));
    let start = Instant::now();
    while !peer.is_finished() {
        match transport.service_content_allocation_requests(
            &mut registry,
            &[],
            start.elapsed().as_millis() as u64,
        ) {
            Ok(_) => {}
            // The peer's own assertions explain an early end; join reports them.
            Err(ShellTransportError::NotConnected) => break,
            Err(error) => panic!("allocation service failed: {error}"),
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn content_refusal_is_journaled_before_revocation() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    transport.reserve_content(&mut registry, limits(1)).unwrap();
    let socket = transport.socket_path().to_owned();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 1), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let refused = peer.next_event();
        assert_eq!(decode_shell_file_refused(&refused).unwrap().reason, 1);
        peer.ack(&refused);
    });
    let result = negotiate(
        &mut transport,
        &mut registry,
        1,
        ShellContentAdmissionPolicy::Denied,
        &peer,
    );
    // The refusal reaches the peer as an event; revocation follows its ack.
    let error = match result {
        Err(error) => error,
        Ok(None) => loop {
            match transport.poll_negotiation(&mut registry, 64 * 1024) {
                Err(error) => break error,
                Ok(None) => std::thread::yield_now(),
                Ok(Some(_)) => panic!("refused negotiation completed"),
            }
        },
        Ok(Some(_)) => panic!("refused negotiation completed"),
    };
    assert!(matches!(
        error,
        ShellTransportError::ContentAdmissionRefused(ContentAdmissionRefused { reason: 1, .. })
    ));
    peer.join().unwrap();
    assert!(!transport.supports_content());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn content_before_negotiation_and_a_second_negotiate_are_refused_at_submit() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(1);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let grant = expected.grant;
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let request = encode_shell_file_allocation_request(
            candidate(ShellFileKind::AllocationRequest, 1, 1),
            ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(7),
                record: ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                    grant,
                    output: ContentOutputId {
                        id: 1,
                        generation: 1,
                    },
                    allocation_request_id: 1,
                    operation: 1,
                    role: 1,
                    edge: 1,
                    prior: ContentAllocationId::default(),
                    parent: ContentAllocationId::default(),
                    parent_presentation_epoch: 0,
                    anchor_parent_rect: ContentPixelRect::default(),
                    desired_width: 8,
                    desired_height: 8,
                    margins: ContentMargins::default(),
                }),
            },
        )
        .unwrap();
        // Content before negotiation: refused before custody, nothing journaled.
        assert_eq!(errno(peer.submit(&request)), EACCES);
        peer.clear();
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 2), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 2);
        let negotiated = peer.next_event();
        decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);
        // One selection per epoch.
        let again = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 3), hello())
            .unwrap();
        assert_eq!(errno(peer.submit(&again)), EALREADY);
        peer.clear();
    });
    negotiate(
        &mut transport,
        &mut registry,
        1,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: false,
        },
        &peer,
    )
    .unwrap();
    let start = Instant::now();
    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}
