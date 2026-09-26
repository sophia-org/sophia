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

fn facts(width: u32) -> Vec<ContentOutputFactsEntry> {
    vec![ContentOutputFactsEntry {
        output: ContentOutputId {
            id: 2,
            generation: 1,
        },
        local_width: width,
        local_height: 64,
        scale_numerator: 1,
        scale_denominator: 1,
        scale_generation: 1,
    }]
}

#[test]
fn output_facts_are_pinned_objects_announced_by_publication() {
    const EBUSY: u32 = 16;
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    transport.reserve_content(&mut registry, limits(1)).unwrap();
    let socket = transport.socket_path().to_owned();
    let (published_tx, published) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 1), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        peer.ack(&negotiated);
        // First publication: announced, then read through a pin.
        let event = peer.next_event();
        let first = decode_shell_file_object_published(&event).unwrap();
        assert_eq!(
            (first.object, first.generation),
            (ShellFileKind::Outputs, 1)
        );
        peer.ack(&event);
        peer.open(6, b"outputs", 0);
        let pinned = peer.read(6, 0);
        let ShellContentRecord::OutputFacts(value) =
            decode_shell_file_outputs(&pinned).unwrap().record
        else {
            panic!("output facts");
        };
        assert_eq!(value.outputs[0].local_width, 64);
        // One pin per feed per attach.
        peer.walk(7, b"outputs");
        let second = peer
            .rpc(12, &[7u32.to_le_bytes(), 0u32.to_le_bytes()].concat())
            .unwrap();
        assert_eq!(errno(second), EBUSY);
        published_tx.send(()).unwrap();
        // A later publication has a fresh qid; the pin still reads its object.
        let event = peer.next_event();
        let next = decode_shell_file_object_published(&event).unwrap();
        assert_eq!(next.generation, 2);
        assert_ne!(next.qid, first.qid);
        peer.ack(&event);
        assert_eq!(peer.read(6, 0), pinned);
        // After the pin is clunked, a new open sees the current object.
        assert_eq!(peer.rpc(120, &6u32.to_le_bytes()).unwrap().0, 121);
        peer.open(8, b"outputs", 0);
        let ShellContentRecord::OutputFacts(value) =
            decode_shell_file_outputs(&peer.read(8, 0)).unwrap().record
        else {
            panic!("output facts");
        };
        assert_eq!(value.outputs[0].local_width, 128);
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
    .unwrap()
    .expect("negotiated");
    transport
        .publish_content_output_facts(&mut registry, TransactionId::from_raw(39), 1, facts(64))
        .unwrap();
    let start = Instant::now();
    let mut second = false;
    while !peer.is_finished() {
        if !second && published.try_recv().is_ok() {
            transport
                .publish_content_output_facts(
                    &mut registry,
                    TransactionId::from_raw(41),
                    2,
                    facts(128),
                )
                .unwrap();
            second = true;
        }
        match transport.poll_io(&mut registry) {
            Ok(()) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => panic!("poll: {error}"),
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

const ESTALE: u32 = 116;
const EINVAL: u32 = 22;

/// 2048x8 BGRA: seven rows per canonical chunk, so chunks of 57344 and 8192.
fn upload_begin(grant: ContentGrant, id: u64) -> ContentResourceBegin {
    ContentResourceBegin {
        grant,
        resource: ContentResourceId { id, generation: 1 },
        width_px: 2048,
        height_px: 8,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 2,
        total_bytes: 65536,
    }
}

fn begin_record(epoch: u64, id: u64, slot: u16, begin: ContentResourceBegin) -> Vec<u8> {
    encode_shell_file_resource_begin(
        candidate(ShellFileKind::ResourceBegin, epoch, id),
        &ShellFileResourceBegin {
            transaction: TransactionId::from_raw(70 + begin.resource.id),
            slot,
            record: ShellContentRecord::ResourceBegin(begin),
        },
    )
    .unwrap()
}

fn next_status(peer: &mut Peer) -> ContentResourceStatus {
    let event = peer.next_event();
    let ShellContentRecord::ResourceStatus(status) =
        decode_shell_file_resource_status(&event).unwrap().record
    else {
        panic!("resource status");
    };
    peer.ack(&event);
    status
}

fn serve_resources(
    transport: &mut ShellComponentTransport,
    registry: &mut ContentEpochRegistry,
    peer: &std::thread::JoinHandle<()>,
) {
    let start = Instant::now();
    while !peer.is_finished() {
        match transport.service_content_resources(registry, start.elapsed().as_millis() as u64) {
            Ok(_) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => panic!("resources: {error}"),
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
}

fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    }
}

#[test]
fn a_split_upload_through_its_slot_is_accepted_as_canonical_chunks() {
    const EBUSY: u32 = 16;
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(1);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let grant = expected.grant;
    let pixels: Vec<u8> = (0..65536u32)
        .map(|i| if i % 4 == 3 { 255 } else { (i / 4 % 200) as u8 })
        .collect();
    let sent = pixels.clone();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 1), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        peer.ack(&negotiated);
        // Nothing is bound yet.
        assert_eq!(errno(peer.open_path(9, &[b"upload", b"0"], 1)), 11);
        peer.submit_acknowledged(&begin_record(1, 2, 0, upload_begin(grant, 1)), 2);
        let admitted = next_status(&mut peer);
        assert_eq!((admitted.status, admitted.admitted_bytes), (1, 65536));
        assert_eq!(peer.open_path(10, &[b"upload", b"0"], 1).0, 13);
        // The first writer of a binding is its only writer; reads are refused.
        assert_eq!(errno(peer.open_path(11, &[b"upload", b"0"], 1)), EBUSY);
        assert_eq!(errno(peer.open_path(12, &[b"upload", b"0"], 2)), 13);
        // Split anywhere: a short write reports the prefix completing a chunk.
        let reply = peer.write_at(10, 0, &sent[..1000]);
        assert_eq!(reply.0, 119);
        assert_eq!(u32::from_le_bytes(reply.1[..4].try_into().unwrap()), 1000);
        assert_eq!(errno(peer.write_at(10, 999, &sent[999..1100])), EINVAL);
        assert_eq!(errno(peer.write_at(10, 2000, &sent[2000..2100])), EINVAL);
        let reply = peer.write_at(10, 1000, &sent[1000..61000]);
        assert_eq!(u32::from_le_bytes(reply.1[..4].try_into().unwrap()), 56344);
        let reply = peer.write_at(10, 57344, &sent[57344..]);
        assert_eq!(u32::from_le_bytes(reply.1[..4].try_into().unwrap()), 8192);
        assert_eq!(errno(peer.write_at(10, 65536, &[0; 4])), EINVAL);
        let end = encode_shell_file_resource_end(
            candidate(ShellFileKind::ResourceEnd, 1, 3),
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(71),
                record: ShellContentRecord::ResourceEnd(ContentResourceEnd {
                    grant,
                    resource: ContentResourceId {
                        id: 1,
                        generation: 1,
                    },
                    total_bytes: 65536,
                    chunk_count: 2,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&end, 3);
        assert_eq!(next_status(&mut peer).status, 2);
        // The binding ended: its fid is fenced, and its clunk cancels nothing.
        assert_eq!(errno(peer.write_at(10, 0, &[0; 4])), ESTALE);
        peer.clunk(10);
    });
    negotiate(&mut transport, &mut registry, 1, granted(), &peer)
        .unwrap()
        .expect("negotiated");
    serve_resources(&mut transport, &mut registry, &peer);
    peer.join().unwrap();
    let lease = transport
        .lease_content_resource(
            &registry,
            grant,
            ContentResourceId {
                id: 1,
                generation: 1,
            },
        )
        .unwrap();
    assert_eq!(lease.bytes(), &pixels[..]);
    drop(lease);
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn clunking_the_writer_before_end_cancels_and_frees_the_slot() {
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
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 1), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        peer.ack(&negotiated);
        peer.submit_acknowledged(&begin_record(1, 2, 0, upload_begin(grant, 1)), 2);
        assert_eq!(next_status(&mut peer).status, 1);
        // A bound slot refuses a second Begin.
        assert_eq!(
            errno(peer.submit(&begin_record(1, 3, 0, upload_begin(grant, 2)))),
            16
        );
        peer.clear();
        assert_eq!(peer.open_path(10, &[b"upload", b"0"], 1).0, 13);
        assert_eq!(peer.write_at(10, 0, &[64, 64, 64, 255].repeat(25)).0, 119);
        peer.clunk(10);
        assert_eq!(next_status(&mut peer).status, 4);
        // The slot is free again for a new binding.
        peer.submit_acknowledged(&begin_record(1, 4, 0, upload_begin(grant, 2)), 4);
        assert_eq!(next_status(&mut peer).status, 1);
    });
    negotiate(&mut transport, &mut registry, 1, granted(), &peer)
        .unwrap()
        .expect("negotiated");
    serve_resources(&mut transport, &mut registry, &peer);
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}
