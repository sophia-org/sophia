//! `sophia_shell_fs_v1` twins of `tests/shell_content_transport.rs`: the
//! single-shell `ShellSessionTransport` negotiated with
//! `accept_files_with_content_policy`, and the SDK's `connect_files` client.
//! Every assertion of the socket cases is kept. The socket file stays until
//! the socket wire is removed; these cases carry its coverage onto 9P.
//!
//! One wire difference is deliberate: 9P admits a candidate only as one
//! grouped transaction (`enqueue_content_group`), where the socket case sends
//! Begin, Chunk and End as transactions 20, 21 and 22. The SDK refuses a lone
//! part over files (see `shell_client_file_wire.rs`).
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{Custody, ShellClientOptions, ShellConnection};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sophia-content-session-files-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn evidence() -> ProtectionDomainEvidence {
    ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid: std::process::id(),
        roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
    }
}

fn session() -> ShellSessionTransport {
    let mut session = ShellSessionTransport::bind_for_supervised_uid(
        directory(),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    session.authorize_protected_peer(&evidence()).unwrap();
    session
}

fn connect(socket: std::path::PathBuf, capabilities: u64) -> ShellConnection {
    ShellConnection::connect_files(
        socket,
        ShellClientOptions {
            minimum_revision: 5,
            maximum_revision: 6,
            required_capabilities: capabilities,
            handshake_timeout: Duration::from_secs(2),
        },
    )
    .unwrap()
}

const CONTENT: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;

fn granted(discrete_input: bool) -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted { discrete_input }
}

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

fn resource_id(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

fn small_upload(grant: ContentGrant, resource: ContentResourceId) -> [ShellContentRecord; 3] {
    [
        ShellContentRecord::ResourceBegin(ContentResourceBegin {
            grant,
            resource,
            width_px: 2,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 8,
        }),
        ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant,
            resource,
            ordinal: 0,
            offset: 0,
            bytes: vec![0, 0, 255, 255, 0, 128, 0, 128],
        }),
        ShellContentRecord::ResourceEnd(ContentResourceEnd {
            grant,
            resource,
            total_bytes: 8,
            chunk_count: 1,
        }),
    ]
}

fn statuses(client: &mut ShellConnection) -> Vec<u16> {
    let mut statuses = Vec::new();
    while statuses.len() < 2 {
        if let ShellContentRecord::ResourceStatus(status) = next_content(client) {
            statuses.push(status.status);
        }
    }
    statuses
}

#[test]
fn admitted_resource_transfer_settles_and_releases_over_the_file_wire() {
    let mut session = session();
    let socket = session.socket_path().to_path_buf();
    let (done_tx, done_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let client = std::thread::spawn(move || {
        let mut client = connect(socket, CONTENT);
        let ShellContentRecord::Limits(limits) = next_content(&mut client) else {
            panic!("expected limits");
        };
        let resource = resource_id(1);
        for record in small_upload(limits.grant, resource) {
            client
                .send_content(TransactionId::from_raw(7), &record)
                .unwrap();
        }
        assert_eq!(statuses(&mut client), [1, 2]);
        client
            .send_content(
                TransactionId::from_raw(8),
                &ShellContentRecord::ResourceRetire(ContentResourceRetire {
                    grant: limits.grant,
                    resource,
                }),
            )
            .unwrap();
        let ShellContentRecord::ResourceReleased(released) = next_content(&mut client) else {
            panic!("expected resource release");
        };
        assert_eq!(released.grant, limits.grant);
        assert_eq!(released.resource, resource);
        assert_eq!(released.reason, ContentReason::None as u16);
        done_tx.send(()).unwrap();
        // 9P reports the peer's close at once; keep the connection until the
        // owner disconnects, the socket case's order.
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });

    session
        .accept_files_with_content_policy(1, Duration::from_secs(2), granted(false))
        .unwrap();
    let start = Instant::now();
    while done_rx.try_recv().is_err() {
        session
            .service_content_resources(start.elapsed().as_millis() as u64)
            .unwrap();
        let accounting = session.content_accounting();
        let limits = ContentLimits::prototype(session.content_grant().unwrap());
        assert_eq!(accounting.epochs.active_epochs, 1);
        assert!(accounting.response_records <= limits.max_control_records as usize);
        assert!(accounting.response_bytes <= limits.max_output_queue_bytes as usize);
        assert!(!accounting.quiescent());
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    session.disconnect().unwrap();
    release_tx.send(()).unwrap();
    client.join().unwrap();
    assert!(session.collect_content_accounting().quiescent());
}

#[test]
fn admitted_candidate_crosses_the_file_wire_and_keeps_outcomes_ordered() {
    candidate_roundtrip(false);
}

#[test]
fn renderer_failed_candidate_over_files_collects_only_after_the_render_lease_ends() {
    candidate_roundtrip(true);
}

fn candidate(
    grant: ContentGrant,
    permit: &ContentFramePermit,
    facts_generation: u64,
    allocation: &ContentAllocationResult,
    resource: ContentResourceId,
) -> [ShellContentRecord; 3] {
    let generation = 1;
    [
        ShellContentRecord::CandidateBegin(ContentCandidateBegin {
            grant,
            candidate_generation: generation,
            output: permit.output,
            facts_generation,
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
    ]
}

fn allocation_request(grant: ContentGrant, output: ContentOutputId) -> ShellContentRecord {
    ShellContentRecord::AllocationRequest(ContentAllocationRequest {
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
    })
}

fn snapshot(output: ContentOutputId, allocation: ContentAllocationId) -> ContentAllocationSnapshot {
    ContentAllocationSnapshot {
        native_opening: None,
        output,
        allocation,
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
    }
}

fn candidate_roundtrip(renderer_failed: bool) {
    let mut session = session();
    let socket = session.socket_path().to_path_buf();
    let (uploaded_tx, uploaded_rx) = mpsc::channel();
    let client = std::thread::spawn(move || {
        let mut client = connect(socket, CONTENT);
        let ShellContentRecord::Limits(limits) = next_content(&mut client) else {
            panic!("expected limits");
        };
        let ShellContentRecord::OutputFacts(facts) = next_content(&mut client) else {
            panic!("expected output facts");
        };
        let output = facts.outputs[0].output;
        client
            .send_content(
                TransactionId::from_raw(6),
                &allocation_request(limits.grant, output),
            )
            .unwrap();
        let ShellContentRecord::AllocationResult(allocation) = next_content(&mut client) else {
            panic!("expected allocation result");
        };
        assert_eq!(allocation.status, 1);
        let resource = resource_id(1);
        for record in small_upload(limits.grant, resource) {
            client
                .send_content(TransactionId::from_raw(7), &record)
                .unwrap();
        }
        assert_eq!(statuses(&mut client), [1, 2]);
        client
            .send_content(
                TransactionId::from_raw(9),
                &ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant: limits.grant,
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
        let records = candidate(
            limits.grant,
            &permit,
            facts.facts_generation,
            &allocation,
            resource,
        );
        client
            .enqueue_content_group(TransactionId::from_raw(20), &records)
            .unwrap();
        let mut outcomes = Vec::new();
        while outcomes.len() < 2 {
            if let ShellContentRecord::CandidateOutcome(outcome) = next_content(&mut client) {
                if renderer_failed && outcome.kind == 3 {
                    assert_eq!(outcome.reason, ContentReason::RendererFailed as u16);
                }
                outcomes.push((outcome.kind, outcome.presentation_epoch));
            }
        }
        assert_eq!(
            outcomes,
            if renderer_failed {
                vec![(1, 0), (3, 0)]
            } else {
                vec![(1, 0), (2, 9)]
            }
        );
    });

    let welcome = session
        .accept_files_with_content_policy(1, Duration::from_secs(2), granted(false))
        .unwrap();
    let grant = session.content_grant().unwrap();
    let start = Instant::now();
    let output = ContentOutputId {
        id: 2,
        generation: 1,
    };
    session
        .publish_content_output_facts(
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
    while session.next_content_allocation_request().is_none() {
        session
            .service_content_allocation_requests(&[], start.elapsed().as_millis() as u64)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let allocation_id = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    session
        .grant_content_allocation(1, snapshot(output, allocation_id), &[])
        .unwrap();
    while uploaded_rx.try_recv().is_err() {
        session
            .service_content_resources(start.elapsed().as_millis() as u64)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let allocations = session.content_allocation_snapshots();
    let context = ContentCandidateContext {
        output,
        facts_generation: 3,
        interaction_generation: 4,
        allocations: &allocations,
    };
    while session.next_content_demand().is_none() {
        session
            .service_content_demands(&[output], &allocations)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    session
        .grant_content_demand(TransactionId::from_raw(19), output, 1, 10)
        .unwrap();
    let mut processed = 0;
    while processed < 3 {
        processed += session
            .service_content_candidates(&[context], 11 + u64::try_from(processed).unwrap())
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    let render = session.begin_content_submission(output, 1, 20).unwrap();
    assert_eq!(render.resource(resource_id(1)).unwrap().bytes().len(), 8);
    session
        .content_prepared(grant, output, 1, 7, 8, 21)
        .unwrap();
    if renderer_failed {
        session.content_renderer_failed(grant, output, 1).unwrap();
    } else {
        session
            .content_presented(grant, output, 1, 9, 7, 8)
            .unwrap();
    }
    while !client.is_finished() {
        session.poll_io().unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    session.disconnect().unwrap();
    client.join().unwrap();
    // The GPU preflight's disconnect / held renderer / release sequence.
    // Neither repeated disconnect nor collection can release a live consumer.
    assert!(session.content_reserved_bytes() > 0);
    assert!(session.content_backing_reserved_bytes() > 0);
    session.disconnect().unwrap();
    assert_eq!(render.resource(resource_id(1)).unwrap().bytes().len(), 8);
    assert!(session.content_reserved_bytes() > 0);
    assert!(!session.content_accounting().quiescent());
    drop(render);
    session.disconnect().unwrap();
    assert_eq!(session.content_reserved_bytes(), 0);
    assert_eq!(session.content_backing_reserved_bytes(), 0);
    assert!(session.content_accounting().quiescent());
    session.disconnect().unwrap();
    assert!(session.content_accounting().quiescent());
    assert_eq!(welcome.selected_revision, 6);
}

#[test]
fn invalid_allocation_request_over_files_gets_a_correlated_result_and_the_peer_stays() {
    let mut session = session();
    let socket = session.socket_path().to_path_buf();
    let (result_tx, result_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let client = std::thread::spawn(move || {
        let mut client = connect(socket, CONTENT);
        let ShellContentRecord::Limits(limits) = next_content(&mut client) else {
            panic!("expected limits");
        };
        assert!(matches!(
            next_content(&mut client),
            ShellContentRecord::OutputFacts(_)
        ));
        client
            .send_content(
                TransactionId::from_raw(40),
                &allocation_request(
                    limits.grant,
                    ContentOutputId {
                        id: 99,
                        generation: 1,
                    },
                ),
            )
            .unwrap();
        let ShellContentRecord::AllocationResult(result) = next_content(&mut client) else {
            panic!("expected allocation rejection");
        };
        assert_eq!(result.allocation_request_id, 1);
        assert_eq!(result.status, 2);
        assert_eq!(result.reason, ContentReason::OutputLost as u16);
        result_tx.send(()).unwrap();
        // Hold the connection open until the owner has checked it.
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    session
        .accept_files_with_content_policy(1, Duration::from_secs(2), granted(false))
        .unwrap();
    session
        .publish_content_output_facts(
            TransactionId::from_raw(39),
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
    let start = Instant::now();
    while result_rx.try_recv().is_err() {
        session
            .service_content_allocation_requests(&[], start.elapsed().as_millis() as u64)
            .unwrap();
        assert!(!client.is_finished(), "client ended before its result");
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    // The rejection closed nothing: the owner still serves the same peer.
    assert!(session.supports_content());
    session
        .service_content_allocation_requests(&[], start.elapsed().as_millis() as u64)
        .unwrap();
    session.poll_io().unwrap();
    release_tx.send(()).unwrap();
    client.join().unwrap();
    session.disconnect().unwrap();
}

#[test]
fn a_discrete_action_and_its_exact_ack_cross_the_file_wire_to_the_owner() {
    let mut session = session();
    let socket = session.socket_path().to_path_buf();
    let (seen_tx, seen_rx) = mpsc::channel();
    let (ack_tx, ack_rx) = mpsc::channel::<()>();
    let client = std::thread::spawn(move || {
        let mut client = connect(
            socket,
            CONTENT | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT,
        );
        let ShellContentRecord::Limits(_) = next_content(&mut client) else {
            panic!("expected limits");
        };
        let ShellContentRecord::Action(action) = next_content(&mut client) else {
            panic!("expected discrete action");
        };
        seen_tx.send(()).unwrap();
        // Acknowledge only after the owner has checked that nothing arrived.
        ack_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let admission = client
            .enqueue_content_tracked(
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
        // Keep the connection until the owner has taken custody of the ack.
        for ticket in admission.tickets() {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                client.poll_io().unwrap();
                match client.custody(ticket) {
                    Some(Custody::Queued | Custody::InFlight) => {}
                    Some(Custody::Submitted | Custody::Stored) => break,
                    other => panic!("ack custody {other:?}"),
                }
                assert!(Instant::now() < deadline, "ack custody timed out");
                std::thread::yield_now();
            }
        }
    });
    session
        .accept_files_with_content_policy(1, Duration::from_secs(2), granted(true))
        .unwrap();
    let grant = session.content_grant().unwrap();
    let action = ContentAction {
        grant,
        output: ContentOutputId {
            id: 2,
            generation: 3,
        },
        candidate_generation: 4,
        presentation_epoch: 5,
        interaction_generation: 1,
        allocation: ContentAllocationId {
            id: 6,
            generation: 7,
        },
        target_id: 8,
        target_generation: 9,
        action_id: 10,
        event_id: 11,
        kind: 1,
        reason: ContentReason::None as u16,
    };
    session
        .send_content_action(TransactionId::from_raw(50), &action)
        .unwrap();
    let start = Instant::now();
    while seen_rx.try_recv().is_err() {
        session.poll_io().unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    // Control: the owner reports no acknowledgement the client has not sent.
    for _ in 0..16 {
        assert_eq!(session.poll_content_action_ack().unwrap(), None);
    }
    ack_tx.send(()).unwrap();
    let ack = loop {
        if let Some(ack) = session.poll_content_action_ack().unwrap() {
            break ack;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    };
    assert_eq!(ack.0, TransactionId::from_raw(51));
    assert_eq!(ack.1.event_id, action.event_id);
    assert_eq!(ack.1.disposition, 1);
    assert_eq!(ack.1.grant, grant);
    assert_eq!(ack.1.action_id, action.action_id);
    client.join().unwrap();
    session.disconnect().unwrap();
}
