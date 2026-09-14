use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ShellClientOptions, ShellConnection};

fn directory() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sophia-content-transport-{}-{}",
        std::process::id(),
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

fn next_content(client: &mut ShellConnection) -> ShellContentRecord {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some((_, record)) = client.poll_content().unwrap() {
            return record;
        }
        assert!(Instant::now() < deadline, "content response timed out");
        std::thread::yield_now();
    }
}

#[test]
fn admitted_resource_transfer_settles_and_releases_over_the_real_socket() {
    let mut session = ShellSessionTransport::bind_for_supervised_uid(
        directory(),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    session.authorize_protected_peer(&evidence()).unwrap();
    let socket = session.socket_path().to_path_buf();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let client = std::thread::spawn(move || {
        let capabilities =
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
        let mut client = ShellConnection::connect(
            socket,
            ShellClientOptions {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: capabilities,
                handshake_timeout: Duration::from_secs(2),
            },
        )
        .unwrap();
        let ShellContentRecord::Limits(limits) = next_content(&mut client) else {
            panic!("expected limits");
        };
        let transaction = TransactionId::from_raw(7);
        let resource = ContentResourceId {
            id: 1,
            generation: 1,
        };
        client
            .send_content(
                transaction,
                &ShellContentRecord::ResourceBegin(ContentResourceBegin {
                    grant: limits.grant,
                    resource,
                    width_px: 2,
                    height_px: 1,
                    rendered_scale_numerator: 1,
                    rendered_scale_denominator: 1,
                    pixel_format: 1,
                    chunk_count: 1,
                    total_bytes: 8,
                }),
            )
            .unwrap();
        client
            .send_content(
                transaction,
                &ShellContentRecord::ResourceChunk(ContentResourceChunk {
                    grant: limits.grant,
                    resource,
                    ordinal: 0,
                    offset: 0,
                    bytes: vec![0, 0, 255, 255, 0, 128, 0, 128],
                }),
            )
            .unwrap();
        client
            .send_content(
                transaction,
                &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                    grant: limits.grant,
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
        assert_eq!(released.resource, resource);
        assert_eq!(released.reason, ContentReason::None as u16);
        done_tx.send(()).unwrap();
    });

    session
        .accept_and_negotiate_with_content_policy(
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let start = Instant::now();
    while done_rx.try_recv().is_err() {
        session
            .service_content_resources(start.elapsed().as_millis() as u64)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    session.disconnect().unwrap();
    client.join().unwrap();
}

#[test]
fn admitted_candidate_crosses_the_real_socket_and_keeps_outcomes_ordered() {
    let mut session = ShellSessionTransport::bind_for_supervised_uid(
        directory(),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    session.authorize_protected_peer(&evidence()).unwrap();
    let socket = session.socket_path().to_path_buf();
    let (uploaded_tx, uploaded_rx) = std::sync::mpsc::channel();
    let client = std::thread::spawn(move || {
        let capabilities =
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
        let mut client = ShellConnection::connect(
            socket,
            ShellClientOptions {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: capabilities,
                handshake_timeout: Duration::from_secs(2),
            },
        )
        .unwrap();
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
                &ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                    grant: limits.grant,
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
        let resource = ContentResourceId {
            id: 1,
            generation: 1,
        };
        let upload = TransactionId::from_raw(7);
        for record in [
            ShellContentRecord::ResourceBegin(ContentResourceBegin {
                grant: limits.grant,
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
                grant: limits.grant,
                resource,
                ordinal: 0,
                offset: 0,
                bytes: vec![0, 0, 255, 255, 0, 128, 0, 128],
            }),
            ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant: limits.grant,
                resource,
                total_bytes: 8,
                chunk_count: 1,
            }),
        ] {
            client.send_content(upload, &record).unwrap();
        }
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
        let generation = 1;
        for (transaction, record) in [
            (
                20,
                ShellContentRecord::CandidateBegin(ContentCandidateBegin {
                    grant: limits.grant,
                    candidate_generation: generation,
                    output: permit.output,
                    facts_generation: facts.facts_generation,
                    pacing_permit: permit.permit_id,
                    interaction_generation: 4,
                    surface_count: 1,
                    placement_count: 1,
                    target_count: 1,
                }),
            ),
            (
                21,
                ShellContentRecord::CandidateChunk(ContentCandidateChunk {
                    grant: limits.grant,
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
            ),
            (
                22,
                ShellContentRecord::CandidateEnd(ContentCandidateEnd {
                    grant: limits.grant,
                    candidate_generation: generation,
                    surface_count: 1,
                    placement_count: 1,
                    target_count: 1,
                }),
            ),
        ] {
            client
                .send_content(TransactionId::from_raw(transaction), &record)
                .unwrap();
        }
        let mut outcomes = Vec::new();
        while outcomes.len() < 2 {
            if let ShellContentRecord::CandidateOutcome(outcome) = next_content(&mut client) {
                outcomes.push((outcome.kind, outcome.presentation_epoch));
            }
        }
        assert_eq!(outcomes, [(1, 0), (2, 9)]);
    });

    let welcome = session
        .accept_and_negotiate_with_content_policy(
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
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
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    let allocation_id = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    session
        .grant_content_allocation(
            1,
            ContentAllocationSnapshot {
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
        session
            .service_content_resources(start.elapsed().as_millis() as u64)
            .unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
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
        assert!(start.elapsed() < Duration::from_secs(2));
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
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    let render = session.begin_content_submission(output, 1, 20).unwrap();
    assert_eq!(render.resource(resource_id(1)).unwrap().bytes().len(), 8);
    session
        .content_prepared(grant, output, 1, 7, 8, 21)
        .unwrap();
    session
        .content_presented(grant, output, 1, 9, 7, 8)
        .unwrap();
    drop(render);
    while !client.is_finished() {
        session.poll_io().unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    session.disconnect().unwrap();
    client.join().unwrap();
    assert_eq!(welcome.selected_revision, 6);
}

#[test]
fn invalid_allocation_request_gets_a_correlated_result_without_closing_the_peer() {
    let mut session = ShellSessionTransport::bind_for_supervised_uid(
        directory(),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    session.authorize_protected_peer(&evidence()).unwrap();
    let socket = session.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let capabilities =
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
        let mut client = ShellConnection::connect(
            socket,
            ShellClientOptions {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: capabilities,
                handshake_timeout: Duration::from_secs(2),
            },
        )
        .unwrap();
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
                &ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                    grant: limits.grant,
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
            panic!("expected allocation rejection");
        };
        assert_eq!(result.allocation_request_id, 1);
        assert_eq!(result.status, 2);
        assert_eq!(result.reason, ContentReason::OutputLost as u16);
    });
    session
        .accept_and_negotiate_with_content_policy(
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
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
    while !client.is_finished() {
        match session.service_content_allocation_requests(&[], start.elapsed().as_millis() as u64) {
            Ok(_) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => panic!("allocation service failed: {error}"),
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    client.join().unwrap();
    session.disconnect().unwrap();
}

#[test]
fn a_discrete_action_and_its_exact_ack_cross_the_real_socket() {
    let mut session = ShellSessionTransport::bind_for_supervised_uid(
        directory(),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    session.authorize_protected_peer(&evidence()).unwrap();
    let socket = session.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let capabilities = SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        let mut client = ShellConnection::connect(
            socket,
            ShellClientOptions {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: capabilities,
                handshake_timeout: Duration::from_secs(2),
            },
        )
        .unwrap();
        let ShellContentRecord::Limits(_) = next_content(&mut client) else {
            panic!("expected limits");
        };
        let ShellContentRecord::Action(action) = next_content(&mut client) else {
            panic!("expected discrete action");
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
    });
    session
        .accept_and_negotiate_with_content_policy(
            1,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )
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
    let ack = loop {
        if let Some((transaction, ack)) = session.poll_content_action_ack().unwrap() {
            break (transaction, ack);
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    };
    assert_eq!(ack.0, TransactionId::from_raw(51));
    assert_eq!(ack.1.event_id, action.event_id);
    assert_eq!(ack.1.disposition, 1);
    client.join().unwrap();
    session.disconnect().unwrap();
}

fn resource_id(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}
