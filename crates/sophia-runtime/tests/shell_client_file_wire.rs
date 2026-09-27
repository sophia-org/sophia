//! `sophia_shell_fs_v1` through the production client: a real
//! `ShellComponentTransport` file export on one side,
//! `sophia_shell_client::ShellConnection::connect_files` on the other. Setup
//! mirrors `tests/shell_file_transport.rs`'s raw-peer scenarios (the exact
//! working protocol); this file swaps the raw `Peer` for the independently
//! seamed client crate, proving the two agree.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{Custody, ShellClientError, ShellClientOptions, ShellConnection};

const MIB: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);
const EPOCH: u64 = 1;

fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "shell-client-file-wire-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn transport() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = directory();
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

fn options(min: u16, max: u16, capabilities: u64) -> ShellClientOptions {
    ShellClientOptions {
        minimum_revision: min,
        maximum_revision: max,
        required_capabilities: capabilities,
        handshake_timeout: Duration::from_secs(2),
    }
}

fn base_capabilities() -> u64 {
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
}

fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    }
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

/// Drives the server side of negotiation to completion (or refusal, folded
/// into a `ShellTransportError` the way `poll_negotiation` reports it once
/// the peer thread has already ended).
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

/// Polls the client until its oldest content record arrives, bounded so a
/// protocol mismatch fails the test instead of hanging it.
fn next_content(client: &mut ShellConnection) -> ShellContentRecord {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some((_, record)) = client.poll_content().unwrap() {
            return record;
        }
        assert!(Instant::now() < deadline, "content response timed out");
        std::thread::yield_now();
    }
}

#[test]
fn negotiation_accepted_delivers_limits_and_outputs() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(EPOCH);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let opts = options(5, 6, base_capabilities());
    let expected_in_client = expected.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let client = std::thread::spawn(move || {
        let mut client = ShellConnection::connect_files(&socket, opts).unwrap();
        assert_eq!(client.welcome().selected_revision, 6);
        assert_eq!(client.connection_epoch(), EPOCH);
        let ShellContentRecord::Limits(got) = next_content(&mut client) else {
            panic!("expected limits");
        };
        assert_eq!(got, expected_in_client);
        let ShellContentRecord::OutputFacts(facts) = next_content(&mut client) else {
            panic!("expected output facts");
        };
        assert_eq!(facts.outputs[0].local_width, 64);
        done_tx.send(()).unwrap();
    });
    let welcome = negotiate(&mut transport, &mut registry, EPOCH, granted(), &client)
        .unwrap()
        .expect("negotiated");
    assert_eq!(welcome.connection_epoch, EPOCH);
    assert_eq!(transport.content_limits(), Some(&expected));
    transport
        .publish_content_output_facts(&mut registry, TransactionId::from_raw(9), 1, facts(64))
        .unwrap();
    let start = Instant::now();
    while done_rx.try_recv().is_err() {
        match transport.poll_io(&mut registry) {
            Ok(()) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => panic!("poll: {error}"),
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    client.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn negotiation_refused_gives_the_exact_refusal() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    transport
        .reserve_content(&mut registry, limits(EPOCH))
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let opts = options(5, 6, base_capabilities());
    let client = std::thread::spawn(move || {
        let Err(error) = ShellConnection::connect_files(&socket, opts) else {
            panic!("connect_files unexpectedly succeeded");
        };
        assert!(matches!(
            error,
            ShellClientError::AdmissionRefused(ContentAdmissionRefused { reason: 1, .. })
        ));
    });
    let result = negotiate(
        &mut transport,
        &mut registry,
        EPOCH,
        ShellContentAdmissionPolicy::Denied,
        &client,
    );
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
    client.join().unwrap();
    assert!(!transport.supports_content());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn allocation_round_trips_through_the_client() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(EPOCH);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let opts = options(5, 6, base_capabilities());
    let grant = expected.grant;
    let client = std::thread::spawn(move || {
        let mut client = ShellConnection::connect_files(&socket, opts).unwrap();
        let ShellContentRecord::Limits(_) = next_content(&mut client) else {
            panic!("expected limits");
        };
        client
            .send_content(
                TransactionId::from_raw(40),
                &ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                    grant,
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
                }),
            )
            .unwrap();
        let ShellContentRecord::AllocationResult(result) = next_content(&mut client) else {
            panic!("expected allocation result");
        };
        assert_eq!(result.allocation_request_id, 1);
        // No allocation service is fed a real answer below: rejected (2).
        assert_eq!(result.status, 2);
    });
    negotiate(&mut transport, &mut registry, EPOCH, granted(), &client)
        .unwrap()
        .expect("negotiated");
    let start = Instant::now();
    while !client.is_finished() {
        match transport.service_content_allocation_requests(
            &mut registry,
            &[],
            start.elapsed().as_millis() as u64,
        ) {
            Ok(_) => {}
            Err(ShellTransportError::NotConnected) => break,
            Err(error) => panic!("allocation service failed: {error}"),
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    client.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}

/// 2048x8 BGRA: seven rows per canonical chunk, so canonical chunks of 57344
/// and 8192 bytes -- matching `tests/shell_file_transport.rs`'s
/// `upload_begin`. The client uploads it as two writes that deliberately do
/// not land on that canonical boundary (40000 then 25536 bytes), exercising
/// the file wire's own short-write/resume handling.
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

#[test]
fn a_split_upload_through_its_slot_reaches_accepted_status() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(EPOCH);
    transport
        .reserve_content(&mut registry, expected.clone())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let opts = options(5, 6, base_capabilities());
    let grant = expected.grant;
    let resource = ContentResourceId {
        id: 1,
        generation: 1,
    };
    let pixels: Vec<u8> = (0..65536u32)
        .map(|i| if i % 4 == 3 { 255 } else { (i / 4 % 200) as u8 })
        .collect();
    let sent = pixels.clone();
    let client = std::thread::spawn(move || {
        let mut client = ShellConnection::connect_files(&socket, opts).unwrap();
        let ShellContentRecord::Limits(_) = next_content(&mut client) else {
            panic!("expected limits");
        };
        let transaction = TransactionId::from_raw(70 + resource.id);
        client
            .send_content(
                transaction,
                &ShellContentRecord::ResourceBegin(upload_begin(grant, resource.id)),
            )
            .unwrap();
        client
            .send_content(
                transaction,
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant,
                    resource,
                    ordinal: 0,
                    offset: 0,
                    bytes: sent[..40000].to_vec(),
                }),
            )
            .unwrap();
        client
            .send_content(
                transaction,
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant,
                    resource,
                    ordinal: 1,
                    offset: 40000,
                    bytes: sent[40000..].to_vec(),
                }),
            )
            .unwrap();
        client
            .send_content(
                transaction,
                &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                    grant,
                    resource,
                    total_bytes: 65536,
                    chunk_count: 2,
                }),
            )
            .unwrap();
        let mut statuses = Vec::new();
        while statuses.len() < 2 {
            if let ShellContentRecord::ResourceStatus(status) = next_content(&mut client) {
                statuses.push(status.status);
            }
        }
        assert_eq!(statuses, [1, 2]);
    });
    negotiate(&mut transport, &mut registry, EPOCH, granted(), &client)
        .unwrap()
        .expect("negotiated");
    serve_resources(&mut transport, &mut registry, &client);
    client.join().unwrap();
    let lease = transport
        .lease_content_resource(&registry, grant, resource)
        .unwrap();
    assert_eq!(lease.bytes(), &pixels[..]);
    drop(lease);
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}

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

/// Negotiates, allocates, uploads one small resource, paces a `FrameDemand`
/// through its `FramePermit`, submits one whole candidate end to end and
/// drives it to Presented through the production client -- the client-level
/// counterpart of `tests/shell_file_transport.rs`'s `paced_candidate`, which
/// proves the same sequence directly against the raw wire. When
/// `with_action` is set, the presented candidate's target then carries one
/// discrete Action to its exact ActionAck.
fn paced_candidate_via_client(with_action: bool) {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    let expected = limits(EPOCH);
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
    let capabilities = base_capabilities() | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
    let opts = options(5, 6, capabilities);
    let client = std::thread::spawn(move || {
        let mut client = ShellConnection::connect_files(&socket, opts).unwrap();
        let ShellContentRecord::Limits(_) = next_content(&mut client) else {
            panic!("expected limits");
        };
        let ShellContentRecord::OutputFacts(facts) = next_content(&mut client) else {
            panic!("expected output facts");
        };
        let output = facts.outputs[0].output;

        client
            .send_content(
                TransactionId::from_raw(6),
                &ShellContentRecord::AllocationRequest(ContentAllocationRequest {
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
                }),
            )
            .unwrap();
        let ShellContentRecord::AllocationResult(allocation) = next_content(&mut client) else {
            panic!("expected allocation result");
        };
        assert_eq!(allocation.status, 1);

        let upload = TransactionId::from_raw(7);
        client
            .send_content(
                upload,
                &ShellContentRecord::ResourceBegin(small_upload(grant, resource.id)),
            )
            .unwrap();
        client
            .send_content(
                upload,
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant,
                    resource,
                    ordinal: 0,
                    offset: 0,
                    bytes: sent.to_vec(),
                }),
            )
            .unwrap();
        client
            .send_content(
                upload,
                &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                    grant,
                    resource,
                    total_bytes: 8,
                    chunk_count: 1,
                }),
            )
            .unwrap();
        let mut statuses = Vec::new();
        while statuses.len() < 2 {
            if let ShellContentRecord::ResourceStatus(status) = next_content(&mut client) {
                statuses.push(status.status);
            }
        }
        assert_eq!(statuses, [1, 2]);

        client
            .send_content(
                TransactionId::from_raw(9),
                &ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant,
                    output,
                    allocation: ContentAllocationId::default(),
                    demand_id: 1,
                    reason: 1,
                }),
            )
            .unwrap();
        uploaded_tx.send(()).unwrap();
        let ShellContentRecord::FramePermit(permit) = next_content(&mut client) else {
            panic!("expected frame permit");
        };
        assert_eq!(permit.state, 1);

        let generation = 1;
        let candidate_transaction = TransactionId::from_raw(20);
        let records = [
            ShellContentRecord::CandidateBegin(ContentCandidateBegin {
                grant,
                candidate_generation: generation,
                output: permit.output,
                facts_generation: 3,
                pacing_permit: permit.permit_id,
                interaction_generation: 4,
                surface_count: 1,
                placement_count: 1,
                target_count: 1,
            }),
            ShellContentRecord::CandidateChunk(ContentCandidateChunk {
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
            }),
            ShellContentRecord::CandidateEnd(ContentCandidateEnd {
                grant,
                candidate_generation: generation,
                surface_count: 1,
                placement_count: 1,
                target_count: 1,
            }),
        ];
        client
            .enqueue_content_group(candidate_transaction, &records)
            .unwrap();

        let mut outcomes = Vec::new();
        while outcomes.len() < 2 {
            if let ShellContentRecord::CandidateOutcome(outcome) = next_content(&mut client) {
                outcomes.push((outcome.kind, outcome.presentation_epoch));
            }
        }
        assert_eq!(outcomes, vec![(1, 0), (2, 9)]);

        if with_action {
            let ShellContentRecord::Action(action) = next_content(&mut client) else {
                panic!("expected action");
            };
            client
                .send_content(
                    TransactionId::from_raw(51),
                    &ShellContentRecord::ActionAck(ContentActionAck {
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
                )
                .unwrap();
        }
    });

    negotiate(
        &mut transport,
        &mut registry,
        EPOCH,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
        &client,
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
        assert!(start.elapsed() < Duration::from_secs(5));
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
        assert!(start.elapsed() < Duration::from_secs(5));
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
        assert!(start.elapsed() < Duration::from_secs(5));
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
        assert!(start.elapsed() < Duration::from_secs(5));
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
    while !client.is_finished() {
        match transport.poll_io(&mut registry) {
            Ok(()) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => panic!("poll: {error}"),
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    client.join().unwrap();
    drop(render);
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_paced_candidate_crosses_the_file_wire_via_the_client_with_ordered_outcomes() {
    paced_candidate_via_client(false);
}

#[test]
fn an_action_and_its_exact_ack_cross_the_file_wire_via_the_client() {
    paced_candidate_via_client(true);
}

/// Polls until `ticket` settles past `InFlight`, bounded.
fn settled(client: &mut ShellConnection, ticket: sophia_shell_client::Ticket) -> Custody {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        client.poll_io().unwrap();
        match client.custody(ticket) {
            Some(Custody::Queued | Custody::InFlight) => {}
            Some(custody) => return custody,
            None => panic!("ticket evicted"),
        }
        assert!(Instant::now() < deadline, "custody timed out");
        std::thread::yield_now();
    }
}

#[test]
fn an_action_response_pair_is_two_submissions_and_a_lone_candidate_part_is_refused() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let (mut transport, directory) = transport();
    transport
        .reserve_content(&mut registry, limits(EPOCH))
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let opts = options(5, 6, base_capabilities());
    let client = std::thread::spawn(move || {
        let mut client = ShellConnection::connect_files(&socket, opts).unwrap();
        let ShellContentRecord::Limits(limits) = next_content(&mut client) else {
            panic!("expected limits");
        };
        // An action response rides the file wire as its own record, with its
        // own ticket; the session takes custody even of an ACK naming no live
        // event (it has no effect there).
        let ack = ContentActionAck {
            grant: limits.grant,
            output: ContentOutputId {
                id: 2,
                generation: 1,
            },
            candidate_generation: 1,
            presentation_epoch: 1,
            interaction_generation: 1,
            allocation: ContentAllocationId {
                id: 1,
                generation: 1,
            },
            target_id: 0,
            target_generation: 0,
            action_id: 0,
            event_id: 1,
            disposition: 1,
        };
        let admission = client
            .enqueue_indicator_action_response_tracked(TransactionId::from_raw(1), &ack, None)
            .unwrap();
        assert_eq!(admission.count, 1);
        assert_eq!(settled(&mut client, admission.first), Custody::Submitted);

        // A lone CandidateBegin outside a ContentGroup has no single-record
        // file-wire shape either.
        let error = client
            .enqueue_content(
                TransactionId::from_raw(2),
                &ShellContentRecord::CandidateBegin(ContentCandidateBegin {
                    grant: limits.grant,
                    candidate_generation: 1,
                    output: ContentOutputId {
                        id: 1,
                        generation: 1,
                    },
                    facts_generation: 1,
                    pacing_permit: 1,
                    interaction_generation: 1,
                    surface_count: 0,
                    placement_count: 0,
                    target_count: 0,
                }),
            )
            .unwrap_err();
        assert!(matches!(error, ShellClientError::UnsupportedOnWire));
    });
    negotiate(&mut transport, &mut registry, EPOCH, granted(), &client)
        .unwrap()
        .expect("negotiated");
    let start = Instant::now();
    while !client.is_finished() {
        match transport.poll_io(&mut registry) {
            Ok(()) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => panic!("poll: {error}"),
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    client.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

/// `connect_from_env` selects exactly one wire from the environment: neither
/// nor both of `SOPHIA_SHELL_SOCKET`/`SOPHIA_SHELL_9P_SOCKET` set is refused
/// outright, with no fallback and no sniffing.
///
/// Exercising the "exactly one set" branches end to end would need
/// `std::env::set_var`/`remove_var`, which are `unsafe` since edition 2024
/// and this workspace forbids unsafe code outright (`unsafe_code = "forbid"`
/// -- confirmed by `cargo test` itself refusing to build a version of this
/// file that used them). `sophia_shell_client`'s own `select_env_wire` tests
/// (`crates/sophia-shell-client/tests/connection.rs`) cover those rules
/// directly as pure logic, with no process-global mutation. This
/// test covers the one branch observable without mutating anything: the
/// ambient default of this test binary, where neither variable is set.
#[test]
fn connect_from_env_requires_a_wire_selection_by_default() {
    let opts = options(5, 6, base_capabilities());
    let Err(error) = ShellConnection::connect_from_env(opts) else {
        panic!("connect_from_env unexpectedly succeeded with no wire selected");
    };
    assert!(matches!(error, ShellClientError::Environment(_)));
}

// A malformed `api` line is refused. The real export (`shell_transport/files/
// export.rs`) always writes a well-formed line, so this cannot be produced
// through it without editing that file; this is a minimal standalone 9P
// export -- built from `sophia_9p`'s own public `Export` trait and
// `unix::Server` harness seam, no production code touched -- that serves
// nothing but a deliberately malformed `api`.

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
enum MalformedNode {
    Root,
    Api,
}

struct MalformedApiExport {
    line: Vec<u8>,
}

impl sophia_9p::Export for MalformedApiExport {
    type Node = MalformedNode;
    type Handle = ();

    fn attach(
        &mut self,
        _context: &sophia_9p::AttachContext<'_>,
    ) -> Result<sophia_9p::Attachment<Self::Node>, sophia_9p::Errno> {
        Ok(sophia_9p::Attachment {
            root: MalformedNode::Root,
            epoch: sophia_9p::Epoch(1),
        })
    }

    fn check(
        &mut self,
        _access: &sophia_9p::Access<'_, Self::Node>,
    ) -> Result<(), sophia_9p::Errno> {
        Ok(())
    }

    fn lookup(
        &mut self,
        directory: &Self::Node,
        name: sophia_9p::WalkName<'_>,
    ) -> Result<Self::Node, sophia_9p::Errno> {
        if *directory != MalformedNode::Root {
            return Err(sophia_9p::Errno::ENOTDIR);
        }
        match name {
            sophia_9p::WalkName::Parent => Ok(MalformedNode::Root),
            sophia_9p::WalkName::Child(b"api") => Ok(MalformedNode::Api),
            sophia_9p::WalkName::Child(_) => Err(sophia_9p::Errno::ENOENT),
        }
    }

    fn describe(&self, node: &Self::Node, _handle: Option<&Self::Handle>) -> sophia_9p::Entry {
        sophia_9p::Entry {
            kind: if *node == MalformedNode::Root {
                sophia_9p::NodeKind::Directory
            } else {
                sophia_9p::NodeKind::File
            },
            qid_path: match node {
                MalformedNode::Root => 0,
                MalformedNode::Api => 1,
            },
            qid_version: 0,
            permissions: if *node == MalformedNode::Root {
                0o500
            } else {
                0o400
            },
            size: if *node == MalformedNode::Api {
                self.line.len() as u64
            } else {
                0
            },
        }
    }

    fn open(
        &mut self,
        node: &Self::Node,
        flags: sophia_9p::OpenFlags,
    ) -> Result<Self::Handle, sophia_9p::Errno> {
        if *node != MalformedNode::Api || flags.access() != Some(sophia_9p::OpenAccess::Read) {
            return Err(sophia_9p::Errno::EACCES);
        }
        Ok(())
    }

    fn read(
        &mut self,
        node: &Self::Node,
        _handle: &mut Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<sophia_9p::ReadOutcome, sophia_9p::Errno> {
        if *node != MalformedNode::Api {
            return Err(sophia_9p::Errno::EACCES);
        }
        let start = (offset as usize).min(self.line.len());
        let end = start.saturating_add(count as usize).min(self.line.len());
        Ok(sophia_9p::ReadOutcome::Ready(
            self.line[start..end].to_vec(),
        ))
    }

    fn write(
        &mut self,
        _node: &Self::Node,
        _handle: &mut Self::Handle,
        _offset: u64,
        _data: &[u8],
    ) -> Result<u32, sophia_9p::Errno> {
        Err(sophia_9p::Errno::EOPNOTSUPP)
    }

    fn release(&mut self, _node: Self::Node, _handle: Option<Self::Handle>) {}
}

#[test]
fn a_malformed_api_line_is_refused_at_connect() {
    let socket_dir = directory();
    std::fs::create_dir_all(&socket_dir).unwrap();
    let socket = socket_dir.join("socket");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let limits = sophia_9p::Limits::new(65536, 512, 16, 32, 131072, 1).unwrap();
    let export = MalformedApiExport {
        // An otherwise well-formed line with one extra trailing field.
        line: b"sophia-shell-files version=1 role=bar epoch=7 fd_transfer=none extra=1\n".to_vec(),
    };
    let mut server = sophia_9p::unix::Server::new(export, limits).unwrap();
    server.listen(listener).unwrap();
    let wake = server.wake();
    let server_thread = std::thread::spawn(move || server.run());

    let opts = options(5, 6, base_capabilities());
    let Err(error) = ShellConnection::connect_files(&socket, opts) else {
        panic!("connect_files unexpectedly succeeded against a malformed api line");
    };
    assert!(matches!(error, ShellClientError::Protocol(_)));

    wake.stop();
    server_thread.join().unwrap().unwrap();
    std::fs::remove_dir_all(socket_dir).unwrap();
}
