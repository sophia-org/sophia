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

// Candidates, pacing and discrete actions over the file wire: the 262/263/
//264/265 client transactions and the 35/36/37 journal events. The socket
// wire's equivalents are `candidate_roundtrip` and
// `a_discrete_action_and_its_exact_ack_cross_the_real_socket` in
// shell_content_transport.rs; the store behaviour under refusal is
// exercised directly in shell_content_candidates.rs.

/// A small two-pixel resource, matching the one the socket wire's
/// `candidate_roundtrip` uploads before pacing a candidate.
fn small_upload(grant: ContentGrant, id: u64) -> ContentResourceBegin {
    ContentResourceBegin {
        grant,
        resource: ContentResourceId { id, generation: 1 },
        width_px: 2,
        height_px: 1,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 8,
    }
}

/// Negotiates, allocates, uploads one small resource, paces a FrameDemand
/// through its FramePermit, submits one Candidate record end to end and
/// drives it to Presented -- the file-wire equivalent of the socket wire's
/// `candidate_roundtrip` (shell_content_transport.rs, ~170-505). When
/// `with_action` is set, the presented candidate's target then carries one
/// discrete Action to its exact ActionAck, mirroring
/// `a_discrete_action_and_its_exact_ack_cross_the_real_socket`.
fn paced_candidate(with_action: bool) {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(1);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let grant = expected.grant;
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let sent: [u8; 8] = [0, 0, 255, 255, 0, 128, 0, 128];
    let (uploaded_tx, uploaded_rx) = std::sync::mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let capabilities = SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        let offer = encode_shell_file_negotiate(
            candidate(ShellFileKind::Negotiate, 1, 1),
            ShellV1ClientHello {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: capabilities,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);

        let published = peer.next_event();
        let first = decode_shell_file_object_published(&published).unwrap();
        assert_eq!(
            (first.object, first.generation),
            (ShellFileKind::Outputs, 3)
        );
        peer.ack(&published);
        peer.open(6, b"outputs", 0);
        let object = peer.read(6, 0);
        let ShellContentRecord::OutputFacts(facts) =
            decode_shell_file_outputs(&object).unwrap().record
        else {
            panic!("output facts");
        };
        let output = facts.outputs[0].output;

        let request = ContentAllocationRequest {
            grant,
            output,
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
                transaction: TransactionId::from_raw(6),
                record: ShellContentRecord::AllocationRequest(request),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&bytes, 2);
        let event = peer.next_event();
        let value = decode_shell_file_allocation_result(&event).unwrap();
        let ShellContentRecord::AllocationResult(allocation) = value.record else {
            panic!("allocation result");
        };
        assert_eq!(allocation.status, 1);
        peer.ack(&event);

        peer.submit_acknowledged(&begin_record(1, 3, 0, small_upload(grant, 1)), 3);
        let admitted = next_status(&mut peer);
        assert_eq!((admitted.status, admitted.admitted_bytes), (1, 8));
        assert_eq!(peer.open_path(10, &[b"upload", b"0"], 1).0, 13);
        let reply = peer.write_at(10, 0, &sent);
        assert_eq!(reply.0, 119);
        assert_eq!(u32::from_le_bytes(reply.1[..4].try_into().unwrap()), 8);
        let end = encode_shell_file_resource_end(
            candidate(ShellFileKind::ResourceEnd, 1, 4),
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(71),
                record: ShellContentRecord::ResourceEnd(ContentResourceEnd {
                    grant,
                    resource,
                    total_bytes: 8,
                    chunk_count: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&end, 4);
        assert_eq!(next_status(&mut peer).status, 2);

        let demand = encode_shell_file_transaction(
            candidate(ShellFileKind::FrameDemand, 1, 5),
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(9),
                record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant,
                    output,
                    allocation: ContentAllocationId::default(),
                    demand_id: 1,
                    reason: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&demand, 5);
        uploaded_tx.send(()).unwrap();
        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
        let ShellContentRecord::FramePermit(permit) = value.record else {
            panic!("frame permit");
        };
        assert_eq!(permit.state, 1);
        peer.ack(&event);

        let generation = 1;
        let candidate_bytes = encode_shell_file_candidate(
            candidate(ShellFileKind::Candidate, 1, 6),
            &ShellFileCandidate {
                transaction: TransactionId::from_raw(20),
                begin: ContentCandidateBegin {
                    grant,
                    candidate_generation: generation,
                    output: permit.output,
                    facts_generation: facts.facts_generation,
                    pacing_permit: permit.permit_id,
                    interaction_generation: 4,
                    surface_count: 1,
                    placement_count: 1,
                    target_count: 1,
                },
                chunk: ContentCandidateChunk {
                    grant,
                    candidate_generation: generation,
                    chunk_ordinal: 0,
                    surfaces: vec![ContentSurface {
                        allocation: allocation.allocation,
                        scale_generation: allocation.scale_generation,
                        role: 1,
                        edge: 1,
                        margins: ContentMargins::default(),
                        reservation_extent: 24,
                        parent_surface_index: u16::MAX,
                        anchor_parent_rect: ContentPixelRect::default(),
                    }],
                    placements: vec![ContentPlacement {
                        resource,
                        surface_index: 0,
                        destination_x_px: 3,
                        destination_y_px: 4,
                    }],
                    targets: vec![ContentTarget {
                        surface_index: 0,
                        action_kind: 1,
                        target_id: 1,
                        target_generation: 1,
                        action_id: 1,
                        bounds_px: ContentPixelRect {
                            x: 3,
                            y: 4,
                            width: 2,
                            height: 1,
                        },
                    }],
                },
                end: ContentCandidateEnd {
                    grant,
                    candidate_generation: generation,
                    surface_count: 1,
                    placement_count: 1,
                    target_count: 1,
                },
            },
        )
        .unwrap();
        peer.submit_acknowledged(&candidate_bytes, 6);

        let mut outcomes = Vec::new();
        while outcomes.len() < 2 {
            let event = peer.next_event();
            let value =
                decode_shell_file_transaction(&event, ShellFileKind::CandidateOutcome).unwrap();
            let ShellContentRecord::CandidateOutcome(outcome) = value.record else {
                panic!("candidate outcome");
            };
            outcomes.push((outcome.kind, outcome.presentation_epoch));
            peer.ack(&event);
        }
        assert_eq!(outcomes, vec![(1, 0), (2, 9)]);

        if with_action {
            let event = peer.next_event();
            let value = decode_shell_file_transaction(&event, ShellFileKind::Action).unwrap();
            let ShellContentRecord::Action(action) = value.record else {
                panic!("action");
            };
            peer.ack(&event);
            let ack = encode_shell_file_transaction(
                candidate(ShellFileKind::ActionAck, 1, 7),
                &ShellFileTransactionRecord {
                    transaction: TransactionId::from_raw(51),
                    record: ShellContentRecord::ActionAck(ContentActionAck {
                        grant: action.grant,
                        output: action.output,
                        candidate_generation: action.candidate_generation,
                        presentation_epoch: action.presentation_epoch,
                        interaction_generation: action.interaction_generation,
                        allocation: action.allocation,
                        target_id: action.target_id,
                        target_generation: action.target_generation,
                        action_id: action.action_id,
                        event_id: action.event_id,
                        disposition: 1,
                    }),
                },
            )
            .unwrap();
            peer.submit_acknowledged(&ack, 7);
        }
    });

    negotiate(
        &mut transport,
        &mut registry,
        1,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
        &peer,
    )
    .unwrap()
    .expect("negotiated");
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };
    let start = Instant::now();
    transport
        .publish_content_output_facts(
            &mut registry,
            TransactionId::from_raw(5),
            3,
            vec![ContentOutputFactsEntry {
                output,
                local_width: 64,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 5,
            }],
        )
        .unwrap();
    while transport
        .next_content_allocation_request(&registry)
        .is_none()
    {
        transport
            .service_content_allocation_requests(
                &mut registry,
                &[],
                start.elapsed().as_millis() as u64,
            )
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    let allocation_id = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    transport
        .grant_content_allocation(
            &mut registry,
            1,
            ContentAllocationSnapshot {
                native_opening: None,
                output,
                allocation: allocation_id,
                scale_generation: 5,
                scale_numerator: 1,
                scale_denominator: 1,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                logical: ContentLogicalRect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 32,
                },
                pixel: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 32,
                },
                parent: ContentAllocationId::default(),
                anchor_parent_rect: ContentPixelRect::default(),
                allowed_reservation_extent: 32,
            },
            &[],
        )
        .unwrap();
    while uploaded_rx.try_recv().is_err() {
        transport
            .service_content_resources(&mut registry, start.elapsed().as_millis() as u64)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    let allocations = transport.content_allocation_snapshots(&registry);
    let context = ContentCandidateContext {
        output,
        facts_generation: 3,
        interaction_generation: 4,
        allocations: &allocations,
    };
    while transport.next_content_demand(&registry).is_none() {
        transport
            .service_content_demands(&mut registry, &[output], &allocations)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    transport
        .grant_content_demand(&mut registry, TransactionId::from_raw(19), output, 1, 10)
        .unwrap();
    let mut processed = 0;
    while processed < 3 {
        processed += transport
            .service_content_candidates(&mut registry, &[context], 11 + processed as u64)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    let render = transport
        .begin_content_submission(&mut registry, output, 1, 20)
        .unwrap();
    assert_eq!(render.resource(resource).unwrap().bytes().len(), 8);
    transport
        .content_prepared(&mut registry, grant, output, 1, 7, 8, 21)
        .unwrap();
    transport
        .content_presented(&mut registry, grant, output, 1, 9, 7, 8)
        .unwrap();
    if with_action {
        let action = ContentAction {
            grant,
            output,
            candidate_generation: 1,
            presentation_epoch: 9,
            interaction_generation: 4,
            allocation: allocation_id,
            target_id: 1,
            target_generation: 1,
            action_id: 1,
            event_id: 11,
            kind: 1,
            reason: ContentReason::None as u16,
        };
        transport
            .send_content_action(&mut registry, TransactionId::from_raw(50), &action)
            .unwrap();
    }
    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    drop(render);
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_paced_candidate_crosses_the_file_wire_with_ordered_outcomes() {
    paced_candidate(false);
}

#[test]
fn an_action_and_its_exact_ack_cross_the_file_wire() {
    paced_candidate(true);
}

#[test]
fn a_candidate_without_its_permit_is_rejected_through_its_outcome() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(1);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let grant = expected.grant;
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 1), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);

        // A real permit is granted for this output, then deliberately not
        // the one the Candidate names.
        let demand = encode_shell_file_transaction(
            candidate(ShellFileKind::FrameDemand, 1, 2),
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(9),
                record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant,
                    output,
                    allocation: ContentAllocationId::default(),
                    demand_id: 1,
                    reason: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&demand, 2);
        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
        let ShellContentRecord::FramePermit(permit) = value.record else {
            panic!("frame permit");
        };
        assert_eq!(permit.state, 1);
        peer.ack(&event);

        let wrong_permit = permit.permit_id + 1;
        let bytes = encode_shell_file_candidate(
            candidate(ShellFileKind::Candidate, 1, 3),
            &ShellFileCandidate {
                transaction: TransactionId::from_raw(20),
                begin: ContentCandidateBegin {
                    grant,
                    candidate_generation: 1,
                    output,
                    facts_generation: 1,
                    pacing_permit: wrong_permit,
                    interaction_generation: 1,
                    surface_count: 0,
                    placement_count: 0,
                    target_count: 0,
                },
                chunk: ContentCandidateChunk {
                    grant,
                    candidate_generation: 1,
                    chunk_ordinal: 0,
                    surfaces: vec![],
                    placements: vec![],
                    targets: vec![],
                },
                end: ContentCandidateEnd {
                    grant,
                    candidate_generation: 1,
                    surface_count: 0,
                    placement_count: 0,
                    target_count: 0,
                },
            },
        )
        .unwrap();
        // The custody transfer itself succeeds: a wrong permit is a store
        // refusal, not a malformed record.
        peer.submit_acknowledged(&bytes, 3);
    });
    negotiate(&mut transport, &mut registry, 1, granted(), &peer)
        .unwrap()
        .expect("negotiated");
    let start = Instant::now();
    while transport.next_content_demand(&registry).is_none() {
        transport
            .service_content_demands(&mut registry, &[output], &[])
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    transport
        .grant_content_demand(&mut registry, TransactionId::from_raw(15), output, 7, 10)
        .unwrap();
    let mut now = 10;
    let error = loop {
        now += 1;
        match transport.service_content_candidates(&mut registry, &[], now) {
            Ok(_) => {}
            Err(error) => break error,
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    };
    // The store's own `begin_inner` returns this refusal before it ever
    // reaches the outcome-emitting branch (crates/sophia-runtime/src/
    // shell_content/candidates.rs): a Candidate naming an unknown or wrong
    // pacing permit gets no CandidateOutcome at all, only this propagated
    // error from the service call. See the final report for whether that
    // silent-refusal asymmetry (other invalid Begins do emit an outcome) is
    // intended.
    assert!(matches!(
        error,
        ShellTransportError::ContentCandidate(ContentCandidateError::Stale)
    ));
    // The Candidate's own custody transfer (Submitted) already crossed the
    // wire before the store's refusal; drain it so the peer's blocking read
    // completes instead of timing out.
    while !peer.is_finished() {
        transport.poll_io(&mut registry).unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn demand_cancel_crosses_the_file_wire() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(1);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let grant = expected.grant;
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 1), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);

        let demand = encode_shell_file_transaction(
            candidate(ShellFileKind::FrameDemand, 1, 2),
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(1),
                record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant,
                    output,
                    allocation: ContentAllocationId::default(),
                    demand_id: 1,
                    reason: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&demand, 2);
        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
        let ShellContentRecord::FramePermit(permit) = value.record else {
            panic!("frame permit");
        };
        assert_eq!(permit.state, 1);
        peer.ack(&event);

        let cancel = encode_shell_file_transaction(
            candidate(ShellFileKind::FrameDemandCancel, 1, 3),
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(4),
                record: ShellContentRecord::FrameDemandCancel(ContentFrameDemandCancel {
                    grant,
                    output,
                    demand_id: 1,
                    permit_id: permit.permit_id,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&cancel, 3);

        let event = peer.next_event();
        let value = decode_shell_file_transaction(&event, ShellFileKind::FramePermit).unwrap();
        let ShellContentRecord::FramePermit(cancelled) = value.record else {
            panic!("frame permit");
        };
        assert_eq!(cancelled.permit_id, permit.permit_id);
        assert_eq!(cancelled.state, 3);
        assert_eq!(cancelled.reason, ContentReason::Cancelled as u16);
        peer.ack(&event);
    });
    negotiate(&mut transport, &mut registry, 1, granted(), &peer)
        .unwrap()
        .expect("negotiated");
    let start = Instant::now();
    while transport.next_content_demand(&registry).is_none() {
        transport
            .service_content_demands(&mut registry, &[output], &[])
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    transport
        .grant_content_demand(&mut registry, TransactionId::from_raw(2), output, 4, 10)
        .unwrap();
    while !peer.is_finished() {
        match transport.service_content_demands(&mut registry, &[output], &[]) {
            Ok(_) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => panic!("demands: {error}"),
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn candidate_records_are_refused_at_submit_before_negotiation() {
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
        let demand = encode_shell_file_transaction(
            candidate(ShellFileKind::FrameDemand, 1, 1),
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(9),
                record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant,
                    output: ContentOutputId {
                        id: 2,
                        generation: 1,
                    },
                    allocation: ContentAllocationId::default(),
                    demand_id: 1,
                    reason: 1,
                }),
            },
        )
        .unwrap();
        assert_eq!(errno(peer.submit(&demand)), EACCES);
        peer.clear();

        let empty_candidate = encode_shell_file_candidate(
            candidate(ShellFileKind::Candidate, 1, 2),
            &ShellFileCandidate {
                transaction: TransactionId::from_raw(20),
                begin: ContentCandidateBegin {
                    grant,
                    candidate_generation: 1,
                    output: ContentOutputId {
                        id: 2,
                        generation: 1,
                    },
                    facts_generation: 1,
                    pacing_permit: 1,
                    interaction_generation: 1,
                    surface_count: 0,
                    placement_count: 0,
                    target_count: 0,
                },
                chunk: ContentCandidateChunk {
                    grant,
                    candidate_generation: 1,
                    chunk_ordinal: 0,
                    surfaces: vec![],
                    placements: vec![],
                    targets: vec![],
                },
                end: ContentCandidateEnd {
                    grant,
                    candidate_generation: 1,
                    surface_count: 0,
                    placement_count: 0,
                    target_count: 0,
                },
            },
        )
        .unwrap();
        assert_eq!(errno(peer.submit(&empty_candidate)), EACCES);
        peer.clear();

        // The connection is otherwise unharmed: negotiation still succeeds.
        let offer = encode_shell_file_negotiate(candidate(ShellFileKind::Negotiate, 1, 3), hello())
            .unwrap();
        peer.submit_acknowledged(&offer, 3);
        let negotiated = peer.next_event();
        decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);
    });
    negotiate(&mut transport, &mut registry, 1, granted(), &peer).unwrap();
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

#[test]
fn a_malformed_candidate_record_is_refused_at_submit() {
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
        decode_shell_file_negotiated(&negotiated).unwrap();
        peer.ack(&negotiated);

        let mut bytes = encode_shell_file_candidate(
            candidate(ShellFileKind::Candidate, 1, 2),
            &ShellFileCandidate {
                transaction: TransactionId::from_raw(20),
                begin: ContentCandidateBegin {
                    grant,
                    candidate_generation: 1,
                    output: ContentOutputId {
                        id: 2,
                        generation: 1,
                    },
                    facts_generation: 1,
                    pacing_permit: 1,
                    interaction_generation: 1,
                    surface_count: 0,
                    placement_count: 0,
                    target_count: 0,
                },
                chunk: ContentCandidateChunk {
                    grant,
                    candidate_generation: 1,
                    chunk_ordinal: 0,
                    surfaces: vec![],
                    placements: vec![],
                    targets: vec![],
                },
                end: ContentCandidateEnd {
                    grant,
                    candidate_generation: 1,
                    surface_count: 0,
                    placement_count: 0,
                    target_count: 0,
                },
            },
        )
        .unwrap();
        // Patch the encoded Chunk's ordinal from 0 to 1 in place. The body
        // (after the header) is tx:8 + begin_len:4 + chunk_len:4 + Begin +
        // Chunk + End, and the Chunk payload opens with grant:16 +
        // candidate_generation:8 before chunk_ordinal:4
        // (crates/sophia-protocol/src/ipc/shell_content/codec.rs).
        let begin_len = u32::from_le_bytes(
            bytes[SHELL_FILE_HEADER_BYTES + 8..SHELL_FILE_HEADER_BYTES + 12]
                .try_into()
                .unwrap(),
        ) as usize;
        let ordinal_at = SHELL_FILE_HEADER_BYTES + 16 + begin_len + 16 + 8;
        assert_eq!(
            u32::from_le_bytes(bytes[ordinal_at..ordinal_at + 4].try_into().unwrap()),
            0
        );
        bytes[ordinal_at..ordinal_at + 4].copy_from_slice(&1u32.to_le_bytes());

        assert_eq!(errno(peer.submit(&bytes)), EINVAL);
        peer.clear();

        // Nothing was journaled for the malformed submission: the same
        // submission id is still free, and a valid record reusing it is
        // accepted normally rather than refused as a stale retry.
        let demand = encode_shell_file_transaction(
            candidate(ShellFileKind::FrameDemand, 1, 2),
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(9),
                record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant,
                    output: ContentOutputId {
                        id: 2,
                        generation: 1,
                    },
                    allocation: ContentAllocationId::default(),
                    demand_id: 1,
                    reason: 1,
                }),
            },
        )
        .unwrap();
        peer.submit_acknowledged(&demand, 2);
    });
    negotiate(&mut transport, &mut registry, 1, granted(), &peer)
        .unwrap()
        .expect("negotiated");
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
